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
    #[serde(default)]
    generation: u64,
}

impl Graph {
    /// Every validation runs before `self.steps` is touched: a rejected plan
    /// must leave the previous plan intact (tests/runtime.rs relies on this
    /// atomicity), so no early-return may follow the assignment at the end.
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
            step.revision = self.generation.saturating_add(1);
            if let Some(old) = self.steps.get(&step.id) {
                let unchanged = old.description == step.description
                    && old.requirements == step.requirements
                    && old.dependencies == step.dependencies
                    && old.files == step.files;
                // A re-proposed step that changed in no way keeps its revision
                // and status, so existing scopes and evidence stay current.
                if unchanged {
                    step.revision = old.revision;
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
        // Topological flood fill: repeatedly visit every step whose
        // dependencies are all visited. If and only if the graph is acyclic,
        // every step becomes reachable this way; a cycle's members all keep a
        // dependency outside `visited`, so a leftover count means a cycle.
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
        self.generation = self.generation.saturating_add(1);
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
        self.generation += 1;
        for id in &affected {
            let step = self.steps.get_mut(id).expect("step from this graph");
            step.status = StepStatus::Pending;
            step.revision = self.generation;
        }
        affected
    }
    pub fn versions(&self) -> BTreeMap<String, u64> {
        self.steps
            .values()
            .map(|s| (s.id.clone(), s.revision))
            .collect()
    }
    pub fn scope(&self, id: &str, goal: &Goal) -> anyhow::Result<crate::tools::Scope> {
        let mut scope = crate::tools::Scope {
            readonly: true,
            epoch: goal.epoch,
            ..Default::default()
        };
        let mut pending = vec![id.to_owned()];
        let versions = goal.active_versions();
        while let Some(id) = pending.pop() {
            if scope.steps.contains_key(&id) {
                continue;
            }
            let step = self
                .steps
                .get(&id)
                .ok_or_else(|| anyhow::anyhow!("unknown step: {id}"))?;
            scope.steps.insert(id, step.revision);
            for requirement in &step.requirements {
                let version = versions
                    .get(requirement)
                    .ok_or_else(|| anyhow::anyhow!("inactive step requirement"))?;
                scope.requirements.insert(requirement.clone(), *version);
            }
            pending.extend(step.dependencies.iter().cloned());
        }
        Ok(scope)
    }
}
