use super::{
    actor::{Actor, Phase, Role},
    runner::Runner,
    state::{Control, UserMessage},
};
use crate::{
    domain::{EventData, Status},
    tools::Scope,
};

impl Runner {
    pub(super) async fn handle_control(&mut self, control: Control) -> anyhow::Result<()> {
        if self.session.interactive
            && self.session.status == Status::Completed
            && self.jobs.is_empty()
        {
            self.settle_boundary().await?;
        }
        match control {
            Control::Plan { text } => {
                let turn = text.as_ref().map(|text| self.record_turn(text));
                self.request_boundary(super::conversation::Boundary::Plan { text, turn })?;
            }
            Control::Goal { text } => {
                if text.trim().is_empty() {
                    self.emit(EventData::Diagnostic {
                        message: "goal cannot be empty".into(),
                    })?;
                } else if self.session.active_goal().is_some() {
                    self.submit_work_message(text)?;
                } else {
                    let turn = self.record_turn(&text);
                    self.request_boundary(
                        if self.session.mode == super::conversation::Mode::Plan {
                            super::conversation::Boundary::Implement { text, turn }
                        } else {
                            super::conversation::Boundary::Goal { text, turn }
                        },
                    )?;
                }
            }
            Control::Side { text } => self.side_question(text)?,
            Control::Compact => {
                self.session.compact_pending = true;
                self.emit(EventData::Diagnostic {
                    message: "compaction requested; waiting for in-flight tools to settle".into(),
                })?;
            }
            Control::Answer { id, text } => {
                self.emit(EventData::User {
                    message: text.clone(),
                })?;
                if let Err(error) = self.answer_question(&id, &text).await {
                    self.emit(EventData::Diagnostic {
                        message: error.to_string(),
                    })?;
                }
            }
            Control::Wake => (), // The runner drains the durable inbox before dispatching work.
            Control::Pause => self.halt(
                Status::Paused,
                "pause requested; cancelling active operations",
            )?,
            Control::Stop => {
                self.session.boundary = None;
                self.session.compact_pending = false;
                self.halt(
                    Status::Stopped,
                    "stop requested; cancelling active operations",
                )?;
            }
            Control::Resume => {
                if self.session.mode != super::conversation::Mode::Engineering {
                    self.emit(EventData::Diagnostic { message: "conversation has no paused engineering execution; submit an explicit goal to implement".into() })?;
                    return Ok(());
                }
                // A resume received during cancellation changes its destination;
                // it must not launch replacements before the old work has exited.
                if self.session.transition.is_some() {
                    self.session.transition = Some(Status::Running);
                    self.emit(EventData::Diagnostic {
                        message: "resume queued until active cancellations settle".into(),
                    })?;
                } else if matches!(
                    self.session.status,
                    Status::Running | Status::Completed | Status::Waiting
                ) {
                    self.emit(EventData::Diagnostic {
                        message: "session has no paused execution to resume".into(),
                    })?;
                } else {
                    for actor in self.session.actors.values_mut() {
                        if actor.phase == Phase::Blocked && self.session.questions.is_empty() {
                            actor.phase = Phase::Ready;
                        }
                    }
                    // Stopping a completed session keeps its artifacts, but its old
                    // verdict may no longer describe the tree. Resume a fresh review.
                    if self.session.actors["main"].phase == Phase::Done
                        && self.session.final_report.is_some()
                    {
                        self.session.verification_pending = true;
                    }
                    self.status(Status::Running, "resuming saved work")?;
                }
            }
            Control::Budget { tokens, seconds } => {
                if let Some(tokens) = tokens {
                    self.session.budget.limit = tokens;
                }
                if let Some(seconds) = seconds {
                    self.session.seconds = seconds;
                }
                self.emit(EventData::Diagnostic {
                    message: "budget updated; use resume to continue a paused run".into(),
                })?;
            }
            Control::Approve { id } => {
                self.session.approvals.insert(id.clone());
                let pending: Vec<_> = self
                    .session
                    .actions
                    .values()
                    .filter(|a| !a.running && a.approval.as_deref() == Some(id.as_str()))
                    .cloned()
                    .collect();
                for action in pending {
                    self.session.actions.remove(&action.id);
                    let actor = self
                        .session
                        .actors
                        .get_mut(&action.actor)
                        .ok_or_else(|| anyhow::anyhow!("approval actor missing"))?;
                    actor.pending.push_front(action.pending);
                    actor.phase = Phase::Awaiting;
                }
                self.status(Status::Running, "authorized action queued for revalidation")?;
            }
            Control::Message { text } => {
                let trimmed = text.trim().trim_end_matches(['。', '!', '！', '.']);
                if matches!(trimmed, "stop" | "停止" | "停下" | "停") {
                    return self.halt(Status::Stopped, "user requested stop");
                }
                if matches!(trimmed, "pause" | "暂停" | "等一下" | "等一下，停着") {
                    return self.halt(Status::Paused, "user requested pause");
                }
                if self.session.mode != super::conversation::Mode::Engineering {
                    self.emit(EventData::User {
                        message: text.clone(),
                    })?;
                    return self.conversation_message(text);
                }
                self.submit_work_message(text)?;
            }
        }
        Ok(())
    }
    fn submit_work_message(&mut self, text: String) -> anyhow::Result<()> {
        let id = uuid::Uuid::new_v4().to_string();
        self.session.turns.push(super::conversation::Turn {
            id: id.clone(),
            text: text.clone(),
        });
        let prompt_only = self.session.condition == super::condition::Condition::Prompt;
        self.session.inbox.push(UserMessage {
            id,
            text: text.clone(),
            applied: prompt_only,
            prior: self.session.status.clone(),
            revision: self.session.goal.revision,
        });
        if prompt_only {
            self.append_prompt_correction(&text);
        }
        self.emit(EventData::User { message: text })?;
        if matches!(self.session.status, Status::Completed | Status::Waiting) {
            self.status(Status::Running, "received new user input")?;
        }
        Ok(())
    }
    pub(super) fn start_steering(&mut self) {
        if self
            .session
            .actors
            .values()
            .any(|a| a.role == Role::Steering && a.phase != Phase::Done)
        {
            return;
        }
        if let Some(message) = self.session.inbox.iter().find(|m| !m.applied) {
            let mut actor = Actor::new(
                format!("steer-{}", message.id),
                Role::Steering,
                message.text.clone(),
                Scope {
                    requirements: self.session.goal.active_versions(),
                    readonly: true,
                    ..Scope::default()
                },
            );
            actor.turn = message.id.clone();
            if let Some(task) = self
                .session
                .implementation
                .as_ref()
                .filter(|task| task.id == message.id)
            {
                actor.turn = task.turn.clone();
            }
            self.session.actors.insert(actor.id.clone(), actor);
        }
    }
    pub(super) async fn drain_inbox(&mut self) -> anyhow::Result<()> {
        for (id, value) in self.store.queued(&self.session.id, self.session.cursor)? {
            self.session.cursor = id;
            match serde_json::from_value(value) {
                Ok(control) => self.handle_control(control).await?,
                Err(error) => self.emit(EventData::Diagnostic {
                    message: format!("invalid persisted control: {error}"),
                })?,
            }
        }
        Ok(())
    }
    fn record_turn(&mut self, text: &str) -> String {
        if self.session.title.trim().is_empty() && !text.trim().is_empty() {
            self.session.title = text.into();
        }
        let id = uuid::Uuid::new_v4().to_string();
        self.session.turns.push(super::conversation::Turn {
            id: id.clone(),
            text: text.into(),
        });
        id
    }
}
