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
        if let Wire::Tool { id, .. } = &wire
            && !self
                .session
                .actions
                .get(id)
                .is_some_and(|action| action.running)
        {
            return Ok(());
        }
        if let Wire::Tool { id, index, .. } = &wire {
            let seen = self.tool_seen.entry(id.clone()).or_default();
            if *index < *seen {
                return Ok(());
            }
            *seen = index + 1;
        }
        if let Wire::Model { id, .. } = &wire {
            let Some(job) = self.jobs.get(id) else {
                return Ok(());
            };
            if job.cancel.is_cancelled()
                || job.goal != self.session.goal.id
                || self
                    .session
                    .actors
                    .get(&job.actor)
                    .is_none_or(|a| a.retired || a.turn != job.turn)
            {
                return Ok(());
            }
        }
        let acknowledgement = if let Wire::Tool { id, index, .. } = &wire {
            Some((id.clone(), *index))
        } else {
            None
        };
        match wire {
            Wire::Model {
                id: _,
                actor,
                event: ModelEvent::Text(text),
            } => self.emit(EventData::Delta { actor, text })?,
            Wire::Model {
                id: _,
                actor: _,
                event: ModelEvent::Reasoning(_),
            } => (),
            Wire::Model {
                id: _,
                actor,
                event: ModelEvent::Retry(attempt),
            } => self.emit(EventData::Diagnostic {
                message: format!("{actor}: rate limit retry {attempt}"),
            })?,
            Wire::Tool {
                id,
                index: _,
                event: ToolEvent::Output(text),
            } => self.emit(EventData::Delta { actor: id, text })?,
            Wire::Tool {
                id,
                index: _,
                event: ToolEvent::Process { pid },
            } => {
                if let Some(action) = self.session.actions.get_mut(&id) {
                    action.pid = Some(pid);
                }
                self.emit(EventData::ProcessStarted { id, pid })?;
            }
        }
        if let Some((id, index)) = acknowledgement
            && let Some(log) = self.tool_logs.get(&id)
        {
            log.lock()
                .map_err(|_| anyhow::anyhow!("tool relay log poisoned"))?
                .acknowledge(index);
        }
        Ok(())
    }
    pub(super) async fn handle_result(
        &mut self,
        id: String,
        result: ResultData,
    ) -> anyhow::Result<()> {
        if matches!(&result, ResultData::Tool(_)) {
            // Drain only this finished tool's unseen tail. Live acknowledgements
            // have already released its consumed prefix from memory.
            let tail = self
                .tool_logs
                .get(&id)
                .map(|log| {
                    log.lock()
                        .map(|mut log| log.drain())
                        .map_err(|_| anyhow::anyhow!("tool relay log poisoned"))
                })
                .transpose()?
                .unwrap_or_default();
            for (offset, (index, event)) in tail.into_iter().enumerate() {
                if offset > 0 && offset % 64 == 0 {
                    self.check_time()?;
                    self.drain_inbox().await?;
                    while let Ok(control) = self.control.try_recv() {
                        self.handle_control(control).await?;
                    }
                    tokio::task::yield_now().await;
                }
                self.handle_wire(Wire::Tool {
                    id: id.clone(),
                    index,
                    event,
                })?;
            }
        }
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
                    prior,
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
                let failure = result
                    .as_ref()
                    .err()
                    .and_then(|error| error.downcast_ref::<crate::model::ReportedFailure>());
                let usage = result
                    .as_ref()
                    .ok()
                    .map(|response| response.usage.clone())
                    .or_else(|| crate::model::failure_usage(result.as_ref().err()?));
                self.session.budget.settle(
                    &reservation,
                    if refused || not_submitted {
                        Some(&zero)
                    } else {
                        usage.as_ref()
                    },
                )?;
                if let Some(failure) = failure {
                    let artifact = self.tools.logs.join(format!("{id}.failed-response.json"));
                    std::fs::create_dir_all(&self.tools.logs)?;
                    std::fs::write(&artifact, serde_json::to_vec(&failure.response)?)?;
                    self.emit(EventData::Usage {
                        actor: job.actor.clone(),
                        input: failure.usage.input,
                        output: failure.usage.output,
                        cached: failure.usage.cached,
                    })?;
                    self.emit(EventData::Diagnostic {
                        message: format!(
                            "non-executable model response retained for accounting: {}",
                            artifact.display()
                        ),
                    })?;
                }
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
                            || job.goal != self.session.goal.id
                            || self.session.actors[&job.actor].turn != job.turn
                            // Summarizing an obsolete attempt is read-only history
                            // maintenance; its summary cannot revive write authority.
                            || (!compact && (self.session.actors[&job.actor].retired
                                || !self.scope_current(&job.scope)));
                        let actor = self.session.actors.get_mut(&job.actor).unwrap();
                        if stale {
                            actor.phase = if actor.retired {
                                Phase::Done
                            } else {
                                Phase::Ready
                            };
                            return Ok(());
                        }
                        actor.failures = 0;
                        if compact {
                            actor.context.compact(&response.text())?;
                            actor.compact = false;
                            actor.phase = prior;
                            self.emit(EventData::Diagnostic { message: format!("{} context compacted; current requirements and pending work retained", job.actor) })?;
                            return Ok(());
                        }
                        let calls = response.calls()?;
                        actor.workflow_pending = false;
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
                                Role::Conversation => {
                                    self.status(
                                        Status::Waiting,
                                        "response complete; waiting for user input",
                                    )?;
                                }
                                Role::Main => {
                                    if self.session.prompt_pending {
                                        actor.phase = Phase::Ready;
                                    } else {
                                        self.session.final_report = Some(text);
                                        self.session.verification_pending = true;
                                        self.start_verifier()?;
                                    }
                                }
                                Role::Verifier | Role::Steering | Role::Diagnosis => {
                                    actor.failures += 1;
                                    actor.context.user("Use the required structured reporting tool; prose cannot change state.");
                                    actor.phase = Phase::Ready;
                                }
                                Role::Worker => self.worker_finished(&job.actor, &text)?,
                                Role::Question => self.finish_question(&job.actor)?,
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
                        actor.phase = if actor.retired {
                            Phase::Done
                        } else {
                            Phase::Ready
                        };
                        if !job.cancel.is_cancelled() {
                            actor.failures += 1;
                            let failures = actor.failures;
                            let request_failure =
                                error.downcast_ref::<crate::model::RequestFailure>();
                            // Invalid credentials and request formats cannot be repaired by
                            // asking the model for a smaller completion with the same request.
                            if request_failure.is_none() {
                                actor.context.user(&format!("The previous model response was not executable: {error}. Retry with a smaller, complete response; split large edits into separate tool calls. No partial tool call was executed."));
                            }
                            let advice = request_failure.map_or("", |failure| failure.advice());
                            self.emit(EventData::Diagnostic {
                                message: format!("{}: {error} {advice}", job.actor),
                            })?;
                            if request_failure.is_some_and(|failure| !failure.retryable()) {
                                self.halt(
                                    Status::Failed,
                                    &format!(
                                        "provider request stopped; progress is saved. {advice}"
                                    ),
                                )?;
                            } else if failures >= 3 {
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
                self.tool_logs.remove(&action);
                self.tool_seen.remove(&action);
                let mut record = self
                    .session
                    .actions
                    .remove(&action)
                    .ok_or_else(|| anyhow::anyhow!("missing action record"))?;
                if let Err(error) = &result
                    && !job.cancel.is_cancelled()
                    && let Some(ToolError::Approval { id, reason }) =
                        error.downcast_ref::<ToolError>()
                {
                    record.running = false;
                    record.approval = Some(id.clone());
                    let tool = record.pending.call.name.clone();
                    let preview_reason = crate::tools::approval::redact(&json!(reason))
                        .as_str()
                        .unwrap_or("authorization required")
                        .to_owned();
                    let (arguments, details) = crate::tools::approval::save(
                        &self.tools.logs,
                        id,
                        &tool,
                        &record.pending.call.arguments,
                        &preview_reason,
                    )?;
                    self.session.actions.insert(action, record);
                    self.session.actors.get_mut(&job.actor).unwrap().phase = Phase::Blocked;
                    self.emit(EventData::Approval {
                        id: id.clone(),
                        tool,
                        reason: preview_reason,
                        arguments,
                        details: details.display().to_string(),
                    })?;
                    self.status(Status::Waiting, "waiting for scoped authorization")?;
                    return Ok(());
                }
                let mut images = vec![];
                let data = match result {
                    Ok(mut result) => {
                        images = std::mem::take(&mut result.images);
                        let evidence_id = format!("e-{action}");
                        self.session.evidence.push(Evidence {
                            id: evidence_id.clone(),
                            kind: result.kind.clone(),
                            requirements: job.scope.requirements.clone(),
                            epoch: job.scope.epoch,
                            steps: job.scope.steps.clone(),
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
                record.running = false;
                self.session
                    .settled_actions
                    .insert(record.id.clone(), record.clone());
                let observation = json!({"tool":record.pending.call.name,"outcome":data,"requirements":job.scope.requirements,"steps":job.scope.steps});
                let actor = self.session.actors.get_mut(&job.actor).unwrap();
                actor
                    .context
                    .tool_images(&record.pending.call.id, &data, &images);
                if actor.retired {
                    actor.phase = Phase::Done;
                } else if actor.pending.is_empty() {
                    actor.phase = Phase::Ready;
                }
                self.emit(EventData::ActionFinished {
                    id: record.id,
                    result: data,
                    success,
                })?;
                self.record_progress(&job.actor, &observation)?;
            }
            _ => anyhow::bail!("job result kind mismatch"),
        }
        Ok(())
    }
    pub(super) fn start_verifier(&mut self) -> anyhow::Result<()> {
        if !self.session.verification_pending
            || !self.session.team.engineering_settled()
            || !self.session.actions.is_empty()
            || !self.blocked_requirements().is_empty()
            || self
                .session
                .actors
                .values()
                .any(|a| a.role == Role::Verifier && !a.retired && a.phase != Phase::Done)
        {
            return Ok(());
        }
        if !self.session.condition.verifies() {
            self.session.verification_pending = false;
            self.completion_candidate(
                "prompt baseline ended its attempt; external acceptance is separate",
            )?;
            return Ok(());
        }
        if !self.session.graph.covers_requirements(&self.session.goal) {
            self.main_feedback("The current plan omits active requirements. Reconcile task coverage before requesting independent acceptance.");
            self.emit(EventData::Diagnostic {
                message: "acceptance deferred to main for current task coverage".into(),
            })?;
            return Ok(());
        }
        let id = format!("verify-{}", uuid::Uuid::new_v4());
        let mut actor = Actor::new(
            id.clone(),
            Role::Verifier,
            "Inspect the actual deliverable and report evidence for every current requirement."
                .into(),
            Scope {
                requirements: self.session.goal.active_versions(),
                steps: self.session.graph.versions(),
                readonly: true,
                ..Scope::default()
            },
        );
        actor.turn = self.session.actors["main"].turn.clone();
        self.session.actors.insert(id, actor);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
