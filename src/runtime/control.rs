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
        match control {
            Control::Pause => self.halt(
                Status::Paused,
                "pause requested; cancelling active operations",
            )?,
            Control::Stop => self.halt(
                Status::Stopped,
                "stop requested; cancelling active operations",
            )?,
            Control::Resume => {
                if self.target.is_none() {
                    for actor in self.session.actors.values_mut() {
                        if actor.phase == Phase::Blocked {
                            actor.phase = Phase::Ready;
                        }
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
                    .filter(|a| {
                        !a.running
                            && crate::tools::approval_id(
                                &a.pending.call.name,
                                &a.pending.call.arguments,
                            ) == id
                    })
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
                let id = uuid::Uuid::new_v4().to_string();
                self.session.inbox.push(UserMessage {
                    id,
                    text: text.clone(),
                    applied: false,
                    prior: self.session.status.clone(),
                });
                self.emit(EventData::User { message: text })?;
                if matches!(self.session.status, Status::Completed | Status::Waiting) {
                    self.status(Status::Running, "received new user input")?;
                }
            }
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
            let actor = Actor::new(
                format!("steer-{}", message.id),
                Role::Steering,
                message.text.clone(),
                Scope {
                    requirements: self.session.goal.active_versions(),
                    readonly: true,
                    ..Scope::default()
                },
            );
            self.session.actors.insert(actor.id.clone(), actor);
        }
    }
}
