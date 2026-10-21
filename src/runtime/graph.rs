use crate::domain::Goal;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    Running,
    Blocked,
    Verified,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Step {
    pub id: String,
    pub description: String,
    pub requirements: BTreeSet<String>,
    #[serde(default)]
    pub dependencies: BTreeSet<String>,
    #[serde(default)]
    pub files: BTreeSet<String>,
    pub revision: u64,
    pub status: StepStatus,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Graph {
    pub steps: BTreeMap<String, Step>,
}

impl Graph {
    pub fn install(&mut self, steps: Vec<Step>, goal: &Goal) -> anyhow::Result<()> {
        anyhow::ensure!(!steps.is_empty(), "plan cannot be empty");
        let mut proposed = BTreeMap::new();
        let mut covered = BTreeSet::new();
        for mut step in steps {
            anyhow::ensure!(
                !step.id.trim().is_empty() && !step.description.trim().is_empty(),
                "step identity and description are required"
            );
            anyhow::ensure!(
                !step.requirements.is_empty(),
                "each step needs requirement provenance"
            );
            for id in &step.requirements {
                anyhow::ensure!(
                    goal.requirements.get(id).is_some_and(|r| r.active),
                    "step references inactive requirement: {id}"
                );
                covered.insert(id.clone());
            }
            step.status = StepStatus::Pending;
            step.revision = 1;
            if let Some(old) = self.steps.get(&step.id) {
                let unchanged = old.description == step.description
                    && old.requirements == step.requirements
                    && old.dependencies == step.dependencies
                    && old.files == step.files;
                step.revision = old.revision + u64::from(!unchanged);
                if unchanged {
                    step.status = old.status.clone();
                }
            }
            anyhow::ensure!(
                proposed.insert(step.id.clone(), step).is_none(),
                "duplicate step ID"
            );
        }
        anyhow::ensure!(
            goal.active_versions().keys().all(|id| covered.contains(id)),
            "plan omits an active requirement"
        );
        for step in proposed.values() {
            anyhow::ensure!(
                step.dependencies.iter().all(|id| proposed.contains_key(id)),
                "step references missing dependency"
            );
        }
        let mut visited = BTreeSet::new();
        loop {
            let ready: Vec<_> = proposed
                .values()
                .filter(|s| !visited.contains(&s.id) && s.dependencies.is_subset(&visited))
                .map(|s| s.id.clone())
                .collect();
            if ready.is_empty() {
                break;
            }
            visited.extend(ready);
        }
        anyhow::ensure!(visited.len() == proposed.len(), "cyclic plan");
        self.steps = proposed;
        Ok(())
    }
    pub fn affected(&self, requirements: &BTreeSet<String>) -> BTreeSet<String> {
        let mut affected: BTreeSet<_> = self
            .steps
            .values()
            .filter(|s| !s.requirements.is_disjoint(requirements))
            .map(|s| s.id.clone())
            .collect();
        loop {
            let prior = affected.len();
            for step in self.steps.values() {
                if !step.dependencies.is_disjoint(&affected) {
                    affected.insert(step.id.clone());
                }
            }
            if affected.len() == prior {
                return affected;
            }
        }
    }
    pub fn invalidate(&mut self, requirements: &BTreeSet<String>) -> BTreeSet<String> {
        let affected = self.affected(requirements);
        for id in &affected {
            let step = self.steps.get_mut(id).expect("step from this graph");
            step.status = StepStatus::Pending;
            step.revision += 1;
        }
        affected
    }
}
