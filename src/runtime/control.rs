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
            Control::Directed { request } => self.directed_control(request).await?,
            Control::Rename { title } => {
                if title.trim().is_empty() || title.len() > 4096 {
                    self.emit(EventData::Diagnostic {
                        message: "title must contain 1..4096 bytes".into(),
                    })?;
                } else {
                    self.session.title = title;
                    self.emit(EventData::Diagnostic {
                        message: "session renamed".into(),
                    })?;
                }
            }
            Control::Mcp { server, operation } => {
                if self
                    .tools
                    .mcp
                    .as_ref()
                    .is_some_and(|registry| registry.names().contains(&server.as_str()))
                {
                    self.session
                        .mcp_pending
                        .push(crate::extensions::mcp::Change { server, operation });
                    self.emit(EventData::Diagnostic {
                        message: "MCP change queued until active calls settle".into(),
                    })?;
                } else {
                    self.emit(EventData::Diagnostic {
                        message: format!("unknown configured MCP server: {server}"),
                    })?;
                }
            }
            Control::Skill {
                id,
                hash,
                arguments,
            } => {
                if self.session.boundary.is_some() {
                    self.session
                        .skill_pending
                        .push(crate::extensions::skills::Request {
                            id,
                            hash,
                            arguments,
                        });
                    self.emit(EventData::Diagnostic {
                        message: "skill queued until mode transition settles".into(),
                    })?;
                } else if let Err(error) = self.invoke_skill(&id, &hash, &arguments) {
                    self.emit(EventData::Diagnostic {
                        message: format!("skill invocation failed: {error}"),
                    })?;
                }
            }
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
                    turn: None,
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
                self.session.skill_pending.clear();
                self.halt(
                    Status::Stopped,
                    "stop requested; cancelling active operations",
                )?;
            }
            Control::Resume => {
                for actor in self.session.actors.values_mut() {
                    actor.global_pause = false;
                }
                if self.session.mode != super::conversation::Mode::Engineering {
                    return self.resume_readonly();
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
            Control::Message { text } => self.submit_message(text, None)?,
        }
        Ok(())
    }
    /// Directed inputs provide a stable turn so recovery can quarantine exactly
    /// this input, without guessing by text or replaying an uncertain effect.
    pub(crate) fn submit_message(
        &mut self,
        text: String,
        turn: Option<String>,
    ) -> anyhow::Result<()> {
        let trimmed = text.trim().trim_end_matches(['。', '!', '！', '.']);
        if matches!(trimmed, "stop" | "停止" | "停下" | "停") {
            return self.halt(Status::Stopped, "user requested stop");
        }
        if self.session.mode == super::conversation::Mode::Conversation
            && self.session.followup_goal().is_some()
        {
            let turn = turn.unwrap_or_else(|| self.record_turn(&text));
            return self.work_turn(text, turn);
        }
        if matches!(trimmed, "pause" | "暂停" | "等一下" | "等一下，停着") {
            return self.halt(Status::Paused, "user requested pause");
        }
        if self.session.mode != super::conversation::Mode::Engineering {
            let turn = turn.unwrap_or_else(|| self.record_turn(&text));
            self.emit(EventData::User {
                message: text.clone(),
                turn: Some(turn.clone()),
            })?;
            return self.conversation_turn(text, turn);
        }
        let turn = turn.unwrap_or_else(|| self.record_turn(&text));
        self.work_turn(text, turn)?;
        Ok(())
    }
    fn resume_readonly(&mut self) -> anyhow::Result<()> {
        let eligible = self.session.actors.values().any(|actor| {
            !actor.retired
                && matches!(actor.role, Role::Conversation | Role::Question)
                && matches!(actor.phase, Phase::Ready | Phase::Busy | Phase::Awaiting)
        });
        if !eligible
            || matches!(
                self.session.status,
                Status::Running | Status::Waiting | Status::Completed
            )
        {
            self.emit(EventData::Diagnostic {
                message: "session has no paused execution to resume".into(),
            })?;
            return Ok(());
        }
        // Resume restores execution, not fresh user authorization. Retain the
        // original turn/context and read-only scopes, including across recovery.
        for actor in self.session.actors.values_mut() {
            if matches!(actor.role, Role::Conversation | Role::Question) {
                actor.allow_goal = false;
            }
        }
        if self.session.transition.is_some() {
            self.session.transition = Some(Status::Running);
            self.emit(EventData::Diagnostic {
                message: "resume queued until active cancellations settle".into(),
            })?;
        } else {
            self.status(Status::Running, "resuming saved read-only work")?;
        }
        Ok(())
    }

    pub(super) fn submit_work_message(&mut self, text: String) -> anyhow::Result<()> {
        let id = self.record_turn(&text);
        self.work_turn(text, id)
    }

    pub(super) fn work_turn(&mut self, text: String, id: String) -> anyhow::Result<()> {
        let prompt_only = self.session.condition == super::condition::Condition::Prompt;
        self.session.inbox.push(UserMessage {
            id: id.clone(),
            text: text.clone(),
            applied: prompt_only,
            prior: self.session.status.clone(),
            revision: self.session.goal.revision,
        });
        if prompt_only {
            self.append_prompt_correction(&text);
        }
        self.emit(EventData::User {
            message: text,
            turn: Some(id),
        })?;
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
    pub(super) fn record_turn(&mut self, text: &str) -> String {
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
