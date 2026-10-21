use super::{
    actor::{Pending, Phase, Role},
    graph::{Step, StepStatus},
    runner::Runner,
};
use crate::domain::EventData;
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeSet;

impl Runner {
    pub(super) async fn special_call(
        &mut self,
        actor_id: &str,
        pending: &Pending,
    ) -> anyhow::Result<bool> {
        let role = self.session.actors[actor_id].role.clone();
        match (role, pending.call.name.as_str()) {
            (Role::Main, "record_note") => {
                anyhow::ensure!(
                    self.scope_current(&pending.scope),
                    "note uses obsolete requirements"
                );
                let input = serde_json::from_value(pending.call.arguments.clone())?;
                let author = format!("{actor_id}:turn-{}", self.session.actors[actor_id].turns);
                let id = self.session.goal.annotate(input, &author)?;
                *self.tools.goal.write().await = self.session.goal.clone();
                let actor = self.session.actors.get_mut(actor_id).unwrap();
                actor
                    .context
                    .tool_result(&pending.call.id, &json!({"note":id}));
                actor.phase = Phase::Ready;
                self.emit(EventData::Diagnostic {
                    message: "model annotation saved separately from user requirements".into(),
                })?;
            }
            (Role::Diagnosis, "report_diagnosis") => {
                self.finish_diagnosis(actor_id, &pending.call.arguments)?
            }
            (Role::Main, "extract_requirements") => {
                let quotes: Vec<String> =
                    serde_json::from_value(pending.call.arguments["quotes"].clone())?;
                let ids = self.session.goal.extract(&quotes)?;
                *self.tools.goal.write().await = self.session.goal.clone();
                let actor = self.session.actors.get_mut(actor_id).unwrap();
                actor
                    .context
                    .tool_result(&pending.call.id, &json!({"requirements":ids}));
                actor.phase = Phase::Ready;
                self.emit(EventData::Diagnostic {
                    message: "initial user requirements decomposed with exact provenance".into(),
                })?;
            }
            (Role::Main, "delegate") => self.delegate(actor_id, pending)?,
            (Role::Main, "finish_step") => {
                let result = self.finish_step(pending)?;
                let actor = self.session.actors.get_mut(actor_id).unwrap();
                actor.context.tool_result(&pending.call.id, &result);
                actor.phase = Phase::Ready;
                self.emit(EventData::Plan {
                    steps: serde_json::to_value(&self.session.graph)?,
                })?;
            }
            (Role::Main, "wait_workers") => {
                let waiting = self.has_workers();
                let actor = self.session.actors.get_mut(actor_id).unwrap();
                actor
                    .context
                    .tool_result(&pending.call.id, &json!({"waiting":waiting}));
                actor.phase = if waiting {
                    Phase::Blocked
                } else {
                    Phase::Ready
                };
            }
            (Role::Worker, "propose_patch") => self.propose(actor_id, pending).await?,
            (Role::Steering, "classify_input") => {
                self.apply_steering(actor_id, &pending.call.arguments)
                    .await?;
            }
            (Role::Verifier, "report_verification") => {
                self.verify_report(actor_id, &pending.call.arguments)?;
            }
            (Role::Main, "ask_user") => self.ask(actor_id, pending).await?,
            (Role::Main, "focus_step") => self.focus_step(actor_id, pending)?,
            (Role::Main, "wait_input") => {
                anyhow::ensure!(
                    !self.blocked_requirements().is_empty(),
                    "no unanswered input to wait for"
                );
                let actor = self.session.actors.get_mut(actor_id).unwrap();
                actor
                    .context
                    .tool_result(&pending.call.id, &json!({"waiting":true}));
                actor.phase = Phase::Blocked;
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
                if result.is_ok() {
                    self.refresh_plan().await?;
                }
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
}
