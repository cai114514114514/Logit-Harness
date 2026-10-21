use super::Goal;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub type Fingerprints = BTreeMap<String, String>;

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
    pub files: Fingerprints,
    pub passed: bool,
    pub description: String,
    pub artifact: Option<String>,
}

impl Evidence {
    pub fn current(&self, goal: &Goal, files: &Fingerprints) -> bool {
        self.passed
            && !self.requirements.is_empty()
            && self.requirements.iter().all(|(id, revision)| {
                goal.requirements
                    .get(id)
                    .is_some_and(|r| r.active && r.revision == *revision)
            })
            && self
                .files
                .iter()
                .all(|(path, hash)| files.get(path) == Some(hash))
    }
}

impl Goal {
    pub fn missing_evidence(&self, evidence: &[Evidence], files: &Fingerprints) -> Vec<String> {
        self.requirements
            .values()
            .filter(|r| r.active)
            .filter(|requirement| {
                !evidence.iter().any(|e| {
                    e.kind != EvidenceKind::Read
                        && e.requirements.contains_key(&requirement.id)
                        && e.current(self, files)
                })
            })
            .map(|r| r.id.clone())
            .collect()
    }
}
