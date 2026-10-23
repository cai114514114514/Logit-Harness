use super::{actor::Phase, runner::Runner};
use crate::domain::{EventData, Evidence, EvidenceKind};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

impl Runner {
    pub(super) fn verify_report(
        &mut self,
        actor_id: &str,
        arguments: &Value,
    ) -> anyhow::Result<()> {
        #[derive(Deserialize)]
        struct Check {
            requirement: String,
            passed: bool,
            evidence: Vec<String>,
            reason: String,
        }
        #[derive(Deserialize)]
        struct Report {
            summary: String,
            checks: Vec<Check>,
        }
        let report: Report = serde_json::from_value(arguments.clone())?;
        anyhow::ensure!(
            self.session.actions.is_empty()
                && self.session.team.engineering_settled()
                && self.blocked_requirements().is_empty(),
            "active actions or missing user input must settle before completion"
        );
        let snapshot = self.tools.workspace.snapshot_for(
            self.session
                .evidence
                .iter()
                .flat_map(|evidence| evidence.files.keys()),
        )?;
        let versions = self.session.goal.active_versions();
        // A malformed verdict is a verifier protocol error, not a reason to rewrite
        // a correct deliverable. Validate the entire report before adding evidence.
        let mut reported = BTreeSet::new();
        for check in &report.checks {
            anyhow::ensure!(
                versions.contains_key(&check.requirement),
                "requirement must be an active ID, not its text; valid IDs: {:?}",
                versions.keys()
            );
            anyhow::ensure!(
                reported.insert(check.requirement.clone()),
                "duplicate requirement verdict"
            );
            for id in &check.evidence {
                let evidence = self
                    .session
                    .evidence
                    .iter()
                    .find(|evidence| evidence.id == *id)
                    .ok_or_else(|| anyhow::anyhow!("unknown evidence ID: {id}"))?;
                // A passing verdict that cites a failed/obsolete observation is
                // internally inconsistent. Let the verifier refresh or correct
                // its verdict before making the implementer change working code.
                if check.passed {
                    anyhow::ensure!(
                        evidence.current_at(
                            &self.session.goal,
                            &snapshot,
                            &self.session.graph.versions(),
                        ),
                        "passing verdict for {} cites failed or obsolete evidence {id}; refresh the evidence and correct the verdict. If the requirement actually fails, report passed=false",
                        check.requirement
                    );
                }
            }
        }
        anyhow::ensure!(
            reported == versions.keys().cloned().collect(),
            "report must cover every active requirement ID"
        );
        anyhow::ensure!(
            self.session.graph.covers_requirements(&self.session.goal),
            "plan omits current requirements; reconcile task coverage before acceptance"
        );
        let mut covered = BTreeSet::new();
        let mut failures = vec![];
        let actor_scope = self.session.actors[actor_id].scope.clone();
        for check in report.checks {
            let support: Vec<_> = check
                .evidence
                .iter()
                .filter_map(|id| self.session.evidence.iter().find(|e| e.id == *id))
                .collect();
            let supported = !support.is_empty()
                && support.len() == check.evidence.len()
                && support.iter().all(|e| {
                    e.current_at(
                        &self.session.goal,
                        &snapshot,
                        &self.session.graph.versions(),
                    )
                })
                && support
                    .iter()
                    .any(|e| e.requirements.contains_key(&check.requirement));
            if !check.passed || !supported || !versions.contains_key(&check.requirement) {
                failures.push(format!(
                    "{}: {} (valid evidence: {supported})",
                    check.requirement, check.reason
                ));
                continue;
            }
            let files = support.iter().flat_map(|e| e.files.clone()).collect();
            covered.insert(check.requirement.clone());
            self.session.evidence.push(Evidence {
                id: format!("review-{}", uuid::Uuid::new_v4()),
                kind: EvidenceKind::Review,
                epoch: self.session.goal.epoch,
                requirements: BTreeMap::from([(
                    check.requirement.clone(),
                    versions[&check.requirement],
                )]),
                files,
                steps: support.iter().flat_map(|e| e.steps.clone()).collect(),
                passed: true,
                description: check.reason,
                artifact: None,
            });
        }
        for id in versions.keys() {
            if !covered.contains(id) {
                failures.push(format!("no independent supported verdict for {id}"));
            }
        }
        self.session.actors.get_mut(actor_id).unwrap().phase = Phase::Done;
        self.session.verification_pending = false;
        if failures.is_empty()
            && actor_scope.requirements == versions
            && !self.frozen()
            && self.session.team.engineering_settled()
        {
            self.session.final_report = Some(report.summary);
            self.completion_candidate(
                "every active requirement passed independent evidence-backed verification",
            )?;
        } else {
            self.session.final_report = None;
            self.main_feedback(&format!("Independent verification did not pass:\n{}\nContinue fixing the actual deliverable.",failures.join("\n")));
            self.emit(EventData::Diagnostic {
                message: "completion withheld; verification is incomplete".into(),
            })?;
            self.record_progress("main", &serde_json::json!({"verification_failures":failures,"requirements":versions,"files":snapshot}))?;
        }
        Ok(())
    }
}
