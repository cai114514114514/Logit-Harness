use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

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

impl Goal {
    pub fn new(text: &str) -> Self {
        let requirement = Requirement {
            id: "r1".into(),
            text: text.into(),
            source: text.into(),
            revision: 1,
            active: true,
        };
        Self {
            original: text.into(),
            revision: 1,
            requirements: BTreeMap::from([("r1".into(), requirement)]),
        }
    }

    /// Validate the complete delta before changing anything. A rejected correction
    /// must not leave half of a new specification installed.
    pub fn revise(&mut self, change: Change) -> anyhow::Result<Impact> {
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
        let mut changed = BTreeSet::new();
        if change.updates.is_empty() && change.additions.is_empty() && !change.replacement {
            return Ok(Impact { changed });
        }
        self.revision += 1;
        if change.replacement {
            for requirement in self.requirements.values_mut().filter(|r| r.active) {
                requirement.active = false;
                changed.insert(requirement.id.clone());
            }
        }
        for (id, text) in change.updates {
            let requirement = self.requirements.get_mut(&id).expect("validated above");
            requirement.text = text;
            requirement.source = change.source.clone();
            requirement.revision = self.revision;
            changed.insert(id);
        }
        for text in change.additions {
            let id = format!("r{}", self.requirements.len() + 1);
            self.requirements.insert(
                id.clone(),
                Requirement {
                    id: id.clone(),
                    text,
                    source: change.source.clone(),
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
}
