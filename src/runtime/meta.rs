use super::{
    actor::{Pending, Phase, Role},
    graph::{Step, StepStatus},
    runner::Runner,
};
use crate::domain::{Change, EventData, Evidence, EvidenceKind, Status};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

impl Runner {
    pub(super) async fn special_call(
        &mut self,
        actor_id: &str,
        pending: &Pending,
    ) -> anyhow::Result<bool> {
        let role = self.session.actors[actor_id].role.clone();
        match (role, pending.call.name.as_str()) {
            (Role::Steering, "classify_input") => {
                self.apply_steering(actor_id, &pending.call.arguments)
                    .await?;
            }
            (Role::Verifier, "report_verification") => {
                self.verify_report(actor_id, &pending.call.arguments)?;
            }
            (Role::Main, "ask_user") => {
                let question = pending.call.arguments["question"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing question"))?
                    .to_owned();
                self.session
                    .questions
                    .insert(pending.call.id.clone(), question.clone());
                let actor = self.session.actors.get_mut(actor_id).unwrap();
                actor
                    .context
                    .tool_result(&pending.call.id, &json!({"waiting":true}));
                actor.phase = Phase::Blocked;
                self.session.final_report = None;
                self.emit(EventData::Message {
                    actor: actor_id.into(),
                    text: question,
                })?;
                self.status(Status::Waiting, "a user decision is needed")?;
            }
            (Role::Main, "set_plan") => {
                #[derive(Deserialize)]
                struct Input {
                    id: String,
                    description: String,
                    requirements: BTreeSet<String>,
                    #[serde(default)]
                    dependencies: BTreeSet<String>,
                    #[serde(default)]
                    files: BTreeSet<String>,
                }
                let inputs: Vec<Input> =
                    serde_json::from_value(pending.call.arguments["steps"].clone())?;
                let steps = inputs
                    .into_iter()
                    .map(|s| Step {
                        id: s.id,
                        description: s.description,
                        requirements: s.requirements,
                        dependencies: s.dependencies,
                        files: s.files,
                        revision: 1,
                        status: StepStatus::Pending,
                    })
                    .collect();
                let result = self.session.graph.install(steps, &self.session.goal);
                self.session
                    .actors
                    .get_mut(actor_id)
                    .unwrap()
                    .context
                    .tool_result(
                        &pending.call.id,
                        &match result {
                            Ok(()) => json!({"accepted":true}),
                            Err(error) => json!({"error":error.to_string()}),
                        },
                    );
                self.session.actors.get_mut(actor_id).unwrap().phase = Phase::Ready;
                self.emit(EventData::Plan {
                    steps: serde_json::to_value(&self.session.graph)?,
                })?;
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
    async fn apply_steering(&mut self, actor_id: &str, arguments: &Value) -> anyhow::Result<()> {
        #[derive(Deserialize)]
        struct Input {
            intent: String,
            updates: BTreeMap<String, String>,
            additions: Vec<String>,
            answer: String,
            reason: String,
        }
        let input: Input = serde_json::from_value(arguments.clone())?;
        let message_id = actor_id
            .strip_prefix("steer-")
            .ok_or_else(|| anyhow::anyhow!("steering identity missing"))?;
        let message = self
            .session
            .inbox
            .iter()
            .find(|m| m.id == message_id)
            .ok_or_else(|| anyhow::anyhow!("message missing"))?
            .text
            .clone();
        match input.intent.as_str() {
            "refine" | "replace" => {
                let impact = self.session.goal.revise(Change {
                    source: message.clone(),
                    updates: input.updates,
                    additions: input.additions,
                    replacement: input.intent == "replace",
                })?;
                self.session.graph.invalidate(&impact.changed);
                *self.tools.goal.write().await = self.session.goal.clone();
                for job in self.jobs.values() {
                    if job
                        .scope
                        .requirements
                        .keys()
                        .any(|id| impact.changed.contains(id))
                    {
                        job.cancel.cancel();
                    }
                }
                // Queued calls keep their original leases; ToolHost rejects them if the correction made them stale.
                self.main_feedback(&format!("User correction: {message}\nThe active requirements changed. Inspect affected work and continue. {}",input.reason));
                self.session.final_report = None;
                self.emit(EventData::Revised {
                    revision: self.session.goal.revision,
                    changed: impact.changed.into_iter().collect(),
                })?;
            }
            "question" => {
                self.emit(EventData::Message {
                    actor: "answer".into(),
                    text: input.answer,
                })?;
                let prior = self
                    .session
                    .inbox
                    .iter()
                    .find(|m| m.id == message_id)
                    .unwrap()
                    .prior
                    .clone();
                if matches!(prior, Status::Completed | Status::Waiting | Status::Paused) {
                    self.status(prior, "side question answered; task state preserved")?;
                }
            }
            "nudge" => {
                self.main_feedback("Continue the existing goal. The user has not replaced it.")
            }
            "clarify" => {
                self.emit(EventData::Message {
                    actor: "question".into(),
                    text: input.answer,
                })?;
                self.status(Status::Waiting, "clarification required")?;
            }
            _ => anyhow::bail!("unknown input classification"),
        }
        self.session
            .inbox
            .iter_mut()
            .find(|m| m.id == message_id)
            .unwrap()
            .applied = true;
        self.session.actors.get_mut(actor_id).unwrap().phase = Phase::Done;
        self.emit(EventData::Diagnostic {
            message: "user input classified and applied".into(),
        })?;
        Ok(())
    }
    fn verify_report(&mut self, actor_id: &str, arguments: &Value) -> anyhow::Result<()> {
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
        let snapshot = self.tools.workspace.fingerprints()?;
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
                anyhow::ensure!(
                    self.session.evidence.iter().any(|e| e.id == *id),
                    "unknown evidence ID: {id}"
                );
            }
        }
        anyhow::ensure!(
            reported == versions.keys().cloned().collect(),
            "report must cover every active requirement ID"
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
                && support
                    .iter()
                    .all(|e| e.current(&self.session.goal, &snapshot));
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
                requirements: BTreeMap::from([(
                    check.requirement.clone(),
                    versions[&check.requirement],
                )]),
                files,
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
        if failures.is_empty()
            && actor_scope.requirements == versions
            && !self.frozen()
            && !self.has_workers()
        {
            self.session.final_report = Some(report.summary);
            self.status(
                Status::Completed,
                "every active requirement passed independent evidence-backed verification",
            )?;
        } else {
            self.session.final_report = None;
            self.main_feedback(&format!("Independent verification did not pass:\n{}\nContinue fixing the actual deliverable.",failures.join("\n")));
            self.emit(EventData::Diagnostic {
                message: "completion withheld; verification is incomplete".into(),
            })?;
        }
        Ok(())
    }
}
