//! Model interpretations are versioned annotations, never replacements for user requirements.
use super::Goal;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NoteKind {
    Assumption,
    Acceptance,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Note {
    pub id: String,
    pub kind: NoteKind,
    pub text: String,
    pub author: String,
    pub requirements: BTreeMap<String, u64>,
    pub epoch: u64,
    pub quotes: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteInput {
    pub kind: NoteKind,
    pub text: String,
    pub requirements: BTreeSet<String>,
    pub quotes: Vec<String>,
}

impl Note {
    pub fn current(&self, goal: &Goal) -> bool {
        self.epoch == goal.epoch
            && self.requirements.iter().all(|(id, revision)| {
                goal.requirements
                    .get(id)
                    .is_some_and(|r| r.active && r.revision == *revision)
            })
    }
}

impl Goal {
    pub fn annotate(&mut self, input: NoteInput, author: &str) -> anyhow::Result<String> {
        anyhow::ensure!(
            !input.text.trim().is_empty() && !input.requirements.is_empty(),
            "a note needs text and requirement provenance"
        );
        let mut requirements = BTreeMap::new();
        for id in &input.requirements {
            let requirement = self
                .requirements
                .get(id)
                .filter(|r| r.active)
                .ok_or_else(|| anyhow::anyhow!("unknown active requirement: {id}"))?;
            requirements.insert(id.clone(), requirement.revision);
        }
        anyhow::ensure!(
            input.kind != NoteKind::Acceptance || !input.quotes.is_empty(),
            "acceptance criteria need exact quotes from the user's requirement sources"
        );
        for quote in &input.quotes {
            anyhow::ensure!(
                !quote.trim().is_empty()
                    && input
                        .requirements
                        .iter()
                        .any(|id| self.requirements[id].source.contains(quote)),
                "note provenance must quote an associated user source exactly"
            );
        }
        let id = format!("note-{}", uuid::Uuid::new_v4());
        self.notes.push(Note {
            id: id.clone(),
            kind: input.kind,
            text: input.text,
            author: author.into(),
            requirements,
            epoch: self.epoch,
            quotes: input.quotes,
        });
        Ok(id)
    }
}
