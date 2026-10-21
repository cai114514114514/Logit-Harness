use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// 初始需求固定使用这个 ID；持久化数据与测试都依赖它是稳定标识。
const INITIAL_REQUIREMENT_ID: &str = "r1";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Requirement {
    pub id: String,
    pub text: String,
    pub source: String,
    pub revision: u64,
    pub active: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Goal {
    pub original: String,
    pub revision: u64,
    /// Unscoped additions/replacements invalidate every old dependency assumption.
    #[serde(default)]
    pub epoch: u64,
    pub requirements: BTreeMap<String, Requirement>,
    #[serde(default)]
    pub history: Vec<Revision>,
    #[serde(default)]
    pub notes: Vec<super::notes::Note>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Revision {
    pub revision: u64,
    #[serde(default)]
    pub epoch: u64,
    pub requirements: BTreeMap<String, Requirement>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Change {
    pub source: String,
    pub updates: BTreeMap<String, String>,
    pub additions: Vec<String>,
    pub replacement: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Impact {
    pub changed: BTreeSet<String>,
}

/// revise 校验阶段的产出：只包含已核对过存在性、活跃状态与文本的条目，
/// 应用阶段据此直接写入，不再重复查表。
struct ValidatedChange {
    updates: BTreeMap<String, String>,
    additions: Vec<String>,
    replacement: bool,
}

impl Goal {
    /// 注意：这里不校验空 text，与 revise 拒绝空需求的规则并不一致——
    /// 创建时允许先放一个占位目标，是否接受空目标由上层交互决定。
    /// 保持现状以免改变创建语义。
    pub fn new(text: &str) -> Self {
        let requirement = Requirement {
            id: INITIAL_REQUIREMENT_ID.into(),
            text: text.into(),
            source: text.into(),
            revision: 1,
            active: true,
        };
        Self {
            original: text.into(),
            revision: 1,
            epoch: 0,
            requirements: BTreeMap::from([(INITIAL_REQUIREMENT_ID.into(), requirement)]),
            history: vec![],
            notes: vec![],
        }
    }

    pub fn revise(&mut self, change: Change) -> anyhow::Result<Impact> {
        let validated = self.validate(&change)?;
        if validated.updates.is_empty() && validated.additions.is_empty() && !validated.replacement
        {
            return Ok(Impact {
                changed: BTreeSet::new(),
            });
        }
        self.remember();
        if !validated.additions.is_empty() || validated.replacement {
            self.epoch = self.epoch.saturating_add(1);
        }
        self.revision = self.revision.saturating_add(1);
        self.apply(validated, &change.source)
    }

    /// Validate the complete delta before changing anything. A rejected correction
    /// must not leave half of a new specification installed.
    fn validate(&self, change: &Change) -> anyhow::Result<ValidatedChange> {
        anyhow::ensure!(
            !change.source.trim().is_empty(),
            "correction needs user provenance"
        );
        for (id, text) in &change.updates {
            anyhow::ensure!(
                self.requirements.get(id).is_some_and(|r| r.active),
                "unknown active requirement: {id}"
            );
            anyhow::ensure!(
                !text.trim().is_empty(),
                "cannot silently delete a requirement"
            );
        }
        anyhow::ensure!(
            change.additions.iter().all(|s| !s.trim().is_empty()),
            "empty requirement"
        );
        anyhow::ensure!(
            !change.replacement || !change.additions.is_empty(),
            "replacement needs a new objective"
        );
        anyhow::ensure!(
            !change.replacement || change.updates.is_empty(),
            "replacement cannot also update old requirements: use refine for a scoped correction, or supply the complete new objective in additions without updates"
        );
        let mut additions = change.additions.clone();
        if change.replacement && !additions.iter().any(|text| text == &change.source) {
            // A replacement's model-derived subrequirements cannot substitute
            // for the user's complete request at the acceptance gate.
            additions.insert(0, change.source.clone());
        }
        Ok(ValidatedChange {
            updates: change.updates.clone(),
            additions,
            replacement: change.replacement,
        })
    }

    /// 应用已校验的变更。调用前必须已完成 remember() 与 revision 递增，
    /// 使写入的 requirement.revision 指向本次修订。
    fn apply(&mut self, validated: ValidatedChange, source: &str) -> anyhow::Result<Impact> {
        let mut changed = BTreeSet::new();
        if validated.replacement {
            for requirement in self.requirements.values_mut().filter(|r| r.active) {
                requirement.active = false;
                changed.insert(requirement.id.clone());
            }
        }
        // Validation makes replacement and updates mutually exclusive, so an
        // updated requirement always stays in the active acceptance set.
        for requirement in self.requirements.values_mut() {
            if let Some(text) = validated.updates.get(&requirement.id) {
                requirement.text = text.clone();
                requirement.source = source.into();
                requirement.revision = self.revision;
            }
        }
        changed.extend(validated.updates.into_keys());
        for text in validated.additions {
            let id = self.next_requirement_id()?;
            self.requirements.insert(
                id.clone(),
                Requirement {
                    id: id.clone(),
                    text,
                    source: source.into(),
                    revision: self.revision,
                    active: true,
                },
            );
            changed.insert(id);
        }
        Ok(Impact { changed })
    }

    pub fn active_versions(&self) -> BTreeMap<String, u64> {
        self.requirements
            .values()
            .filter(|r| r.active)
            .map(|r| (r.id.clone(), r.revision))
            .collect()
    }

    /// Initial decomposition can quote the user, but cannot invent requirements or
    /// remove the full original objective from the independent acceptance gate.
    pub fn extract(&mut self, quotes: &[String]) -> anyhow::Result<Vec<String>> {
        anyhow::ensure!(
            self.revision == 1,
            "extract initial requirements before revising the goal"
        );
        anyhow::ensure!(
            !quotes.is_empty()
                && quotes
                    .iter()
                    .all(|q| !q.trim().is_empty() && self.original.contains(q)),
            "each extracted requirement must be an exact nonempty quote from the original user request"
        );
        self.remember();
        let mut ids = vec![];
        for quote in quotes {
            if let Some(existing) = self.requirements.values().find(|r| r.text == *quote) {
                ids.push(existing.id.clone());
                continue;
            }
            let id = self.next_requirement_id()?;
            self.requirements.insert(
                id.clone(),
                Requirement {
                    id: id.clone(),
                    text: quote.clone(),
                    source: quote.clone(),
                    revision: 1,
                    active: true,
                },
            );
            ids.push(id);
        }
        Ok(ids)
    }

    /// 反序列化恢复的 Goal 可能带有任意形态的 ID，用现有 ID 的最大数字后缀 +1
    /// （而非 len()+1）生成新 ID，并确认不存在后才允许插入，避免静默覆盖既有需求。
    fn next_requirement_id(&self) -> anyhow::Result<String> {
        let max = self
            .requirements
            .keys()
            .filter_map(|id| id.strip_prefix('r')?.parse::<u64>().ok())
            .max()
            .unwrap_or(0);
        let id = format!("r{}", max.saturating_add(1));
        anyhow::ensure!(
            !self.requirements.contains_key(&id),
            "requirement id collision: {id}"
        );
        Ok(id)
    }

    fn remember(&mut self) {
        self.history.push(Revision {
            revision: self.revision,
            epoch: self.epoch,
            requirements: self.requirements.clone(),
        });
    }
}
