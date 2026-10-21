use super::{
    actor::{Actor, Pending, Phase, Role},
    runner::{JobKind, ResultData, Runner, Wire},
};
use crate::{
    domain::{EventData, Evidence, Status},
    model::ModelEvent,
    tools::{Scope, ToolError, ToolEvent},
};
use serde_json::json;

impl Runner {
    pub(super) fn handle_wire(&mut self, wire: Wire) -> anyhow::Result<()> {
        match wire {
            Wire::Model {
                actor,
                event: ModelEvent::Text(text),
            } => self.emit(EventData::Delta { actor, text })?,
            Wire::Model {
                actor: _,
                event: ModelEvent::Reasoning(_),
            } => (),
            Wire::Model {
                actor,
                event: ModelEvent::Retry(attempt),
            } => self.emit(EventData::Diagnostic {
                message: format!("{actor}: rate limit retry {attempt}"),
            })?,
            Wire::Tool {
                id,
                event: ToolEvent::Output(text),
            } => self.emit(EventData::Delta { actor: id, text })?,
            Wire::Tool {
                id,
                event: ToolEvent::Process { pid },
            } => {
                if let Some(action) = self.session.actions.get_mut(&id) {
                    action.pid = Some(pid);
                }
                self.emit(EventData::ProcessStarted { id, pid })?;
            }
        }
        Ok(())
    }
    pub(super) async fn handle_result(
        &mut self,
        id: String,
        result: ResultData,
    ) -> anyhow::Result<()> {
        let job = self
            .jobs
            .remove(&id)
            .ok_or_else(|| anyhow::anyhow!("missing job"))?;
        if let Some(actor) = self.session.actors.get_mut(&job.actor) {
            actor.busy = actor.busy.saturating_sub(1);
        }
        match (job.kind, result) {
            (
                JobKind::Model {
                    reservation,
                    compact,
                },
                ResultData::Model(result),
            ) => {
                let refused = result
                    .as_ref()
                    .err()
                    .is_some_and(|error| error.is::<super::meter::BudgetReached>());
                let zero = crate::domain::Usage::default();
                let not_submitted = result
                    .as_ref()
                    .err()
                    .is_some_and(|error| error.is::<super::charge::NotSubmitted>());
                self.session.budget.settle(
                    &reservation,
                    if refused || not_submitted {
                        Some(&zero)
                    } else {
                        result.as_ref().ok().map(|r| &r.usage)
                    },
                )?;
                if refused {
                    self.session.actors.get_mut(&job.actor).unwrap().phase = Phase::Ready;
                    self.halt(
                        Status::Exhausted,
                        "campaign money limit reached before submission",
                    )?;
                    return Ok(());
                }
                match result {
                    Ok(response) => {
                        let path = self.tools.logs.join(format!("{id}.response.json"));
                        std::fs::write(
                            &path,
                            serde_json::to_vec(
                                &json!({"output":response.output,"usage":response.usage,"model":response.model}),
                            )?,
                        )?;
                        self.emit(EventData::ModelFinished {
                            actor: job.actor.clone(),
                            artifact: path.to_string_lossy().into_owned(),
                            model: response.model.clone(),
                        })?;
                        self.emit(EventData::Usage {
                            actor: job.actor.clone(),
                            input: response.usage.input,
                            output: response.usage.output,
                            cached: response.usage.cached,
                        })?;
                        let stale = job.cancel.is_cancelled()
                            || job.scope.requirements.iter().any(|(id, rev)| {
                                !self
                                    .session
                                    .goal
                                    .requirements
                                    .get(id)
                                    .is_some_and(|r| r.active && r.revision == *rev)
                            });
                        let actor = self.session.actors.get_mut(&job.actor).unwrap();
                        if stale {
                            actor.phase = Phase::Ready;
                            return Ok(());
                        }
                        actor.failures = 0;
                        if compact {
                            actor.context.compact(&response.text())?;
                            actor.phase = Phase::Ready;
                            return Ok(());
                        }
                        let calls = response.calls()?;
                        let text = response.text();
                        actor.context.append(&response.output);
                        if !text.is_empty() {
                            self.emit(EventData::Message {
                                actor: job.actor.clone(),
                                text: text.clone(),
                            })?;
                        }
                        let actor = self.session.actors.get_mut(&job.actor).unwrap();
                        if calls.is_empty() {
                            actor.phase = Phase::Done;
                            match actor.role {
                                Role::Main => {
                                    self.session.final_report = Some(text);
                                    self.start_verifier();
                                }
                                Role::Verifier | Role::Steering => {
                                    actor.failures += 1;
                                    actor.context.user("Use the required structured reporting tool; prose cannot change state.");
                                    actor.phase = Phase::Ready;
                                }
                                _ => (),
                            }
                        } else {
                            actor.pending.extend(calls.into_iter().map(|call| Pending {
                                call,
                                scope: job.scope.clone(),
                            }));
                            actor.phase = Phase::Awaiting;
                        }
                    }
                    Err(error) => {
                        let actor = self.session.actors.get_mut(&job.actor).unwrap();
                        actor.phase = Phase::Ready;
                        if !job.cancel.is_cancelled() {
                            actor.failures += 1;
                            let failures = actor.failures;
                            self.emit(EventData::Diagnostic {
                                message: format!("{}: {error}", job.actor),
                            })?;
                            if failures >= 3 {
                                self.halt(
                                    Status::Failed,
                                    "provider failed repeatedly; progress is saved",
                                )?;
                            }
                        }
                    }
                }
            }
            (JobKind::Tool { action }, ResultData::Tool(result)) => {
                let mut record = self
                    .session
                    .actions
                    .remove(&action)
                    .ok_or_else(|| anyhow::anyhow!("missing action record"))?;
                if let Err(error) = &result {
                    if let Some(ToolError::Approval { id, reason }) =
                        error.downcast_ref::<ToolError>()
                    {
                        record.running = false;
                        self.session.actions.insert(action, record);
                        self.session.actors.get_mut(&job.actor).unwrap().phase = Phase::Blocked;
                        self.emit(EventData::Approval {
                            id: id.clone(),
                            tool: "external action".into(),
                            reason: reason.clone(),
                        })?;
                        self.status(Status::Waiting, "waiting for scoped authorization")?;
                        return Ok(());
                    }
                }
                let data = match result {
                    Ok(result) => {
                        let evidence_id = format!("e-{action}");
                        self.session.evidence.push(Evidence {
                            id: evidence_id.clone(),
                            kind: result.kind.clone(),
                            requirements: job.scope.requirements.clone(),
                            files: result.files.clone(),
                            passed: result.success,
                            description: record.pending.call.name.clone(),
                            artifact: Some(action.clone()),
                        });
                        self.session
                            .actors
                            .get_mut(&job.actor)
                            .unwrap()
                            .scope
                            .observed
                            .extend(result.files.clone());
                        json!({"result":result,"evidence":evidence_id})
                    }
                    Err(error) => json!({"error":error.to_string()}),
                };
                let success = data["result"]["success"].as_bool().unwrap_or(false);
                let actor = self.session.actors.get_mut(&job.actor).unwrap();
                actor.context.tool_result(&record.pending.call.id, &data);
                if actor.pending.is_empty() {
                    actor.phase = Phase::Ready;
                }
                self.emit(EventData::ActionFinished {
                    id: record.id,
                    result: data,
                    success,
                })?;
            }
            _ => anyhow::bail!("job result kind mismatch"),
        }
        Ok(())
    }
    pub(super) fn start_verifier(&mut self) {
        if self.has_workers() {
            return;
        }
        let id = format!("verify-{}", uuid::Uuid::new_v4());
        let actor = Actor::new(
            id.clone(),
            Role::Verifier,
            "Inspect the actual deliverable and report evidence for every current requirement."
                .into(),
            Scope {
                requirements: self.session.goal.active_versions(),
                readonly: true,
                ..Scope::default()
            },
        );
        self.session.actors.insert(id, actor);
    }
}
