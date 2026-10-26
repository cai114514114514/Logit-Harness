use super::Goal;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub type Fingerprints = BTreeMap<String, String>;

/// Beyond this many files a fingerprint map tells the model nothing it can act
/// on, yet a workspace-wide snapshot of thousands of hashes costs more than a
/// million tokens. The harness keeps every map in full for evidence checks.
pub const VISIBLE_FINGERPRINTS: usize = 128;

/// What the model is shown of a fingerprint map.
pub fn visible_fingerprints(files: &Fingerprints) -> serde_json::Value {
    if files.len() <= VISIBLE_FINGERPRINTS {
        serde_json::json!(files)
    } else {
        serde_json::json!({
            "count": files.len(),
            "omitted": "fingerprints are kept by the harness for evidence checks; read a file to see its current hash",
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    Read,
    Command,
    Browser,
    Review,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Evidence {
    pub id: String,
    pub kind: EvidenceKind,
    pub requirements: BTreeMap<String, u64>,
    #[serde(default)]
    pub epoch: u64,
    #[serde(default)]
    pub steps: BTreeMap<String, u64>,
    pub files: Fingerprints,
    pub passed: bool,
    pub description: String,
    pub artifact: Option<String>,
}

impl Evidence {
    /// Bound to the current clause versions, epoch and step versions, whether or
    /// not the observation passed. File fingerprints are deliberately ignored:
    /// the terminal refreshes every two seconds and cannot hash the workspace each
    /// time, so file drift is left to the verifier, which checks it at acceptance.
    pub fn matches_versions(&self, goal: &Goal, steps: &BTreeMap<String, u64>) -> bool {
        !self.requirements.is_empty()
            && self.epoch == goal.epoch
            && self.requirements.iter().all(|(id, revision)| {
                goal.requirements
                    .get(id)
                    .is_some_and(|r| r.active && r.revision == *revision)
            })
            && self
                .steps
                .iter()
                .all(|(id, revision)| steps.get(id) == Some(revision))
    }
    /// `current_at` without the file comparison; used only for display.
    pub fn current_version(&self, goal: &Goal, steps: &BTreeMap<String, u64>) -> bool {
        self.passed && self.matches_versions(goal, steps)
    }
    pub fn current_at(
        &self,
        goal: &Goal,
        files: &Fingerprints,
        steps: &BTreeMap<String, u64>,
    ) -> bool {
        self.current(goal, files)
            && self
                .steps
                .iter()
                .all(|(id, revision)| steps.get(id) == Some(revision))
    }
    pub fn current(&self, goal: &Goal, files: &Fingerprints) -> bool {
        // 只有实际通过的检查才能作为验收依据。
        let passed = self.passed;
        // 不指向任何需求的证据不构成验收覆盖。
        let covers_any_requirement = !self.requirements.is_empty();
        // 证据绑定的是需求当时的版本；需求一旦修订，旧证据随之过期。
        let versions_still_match = self.requirements.iter().all(|(id, revision)| {
            goal.requirements
                .get(id)
                .is_some_and(|r| r.active && r.revision == *revision)
        });
        // 证据记录的文件指纹必须与当前一致才仍有效。空 files 表示该证据不依赖
        // 任何文件内容（例如纯需求评审），因此永不因文件变化而过期——保持该语义。
        let files_still_match = self.files.iter().all(|(path, hash)| match files.get(path) {
            Some(current) => current == hash,
            None => hash == "missing",
        });
        passed
            && covers_any_requirement
            && versions_still_match
            && files_still_match
            && self.epoch == goal.epoch
    }
}

impl Goal {
    pub fn missing_evidence(&self, evidence: &[Evidence], files: &Fingerprints) -> Vec<String> {
        self.requirements
            .values()
            .filter(|r| r.active)
            .filter(|requirement| {
                !evidence.iter().any(|e| {
                    // Read 只是观察，不构成"需求已被满足"的验证——否则 agent
                    // 读完代码就算完成验收，所以阅读类证据不计入覆盖。
                    e.kind != EvidenceKind::Read
                        && e.requirements.contains_key(&requirement.id)
                        && e.current(self, files)
                })
            })
            .map(|r| r.id.clone())
            .collect()
    }
}
