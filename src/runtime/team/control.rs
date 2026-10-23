use super::{MemberControl, Receipt, ReceiptStatus, Request, Target};
use crate::runtime::{
    actor::{Actor, Role},
    runner::Runner,
    state::Control,
};
use crate::{domain::EventData, tools::Scope};

impl Runner {
    pub(crate) async fn directed_control(&mut self, request: Request) -> anyhow::Result<()> {
        let key = format!("{}:{}", request.client_id, request.id);
        if let Some(receipt) = self.session.team.receipts.get(&key) {
            if receipt.request != request {
                self.emit(EventData::Diagnostic {
                    message: "directed request ID reused with different content; rejected".into(),
                })?;
            }
            return Ok(());
        }
        self.session.team.receipts.insert(
            key.clone(),
            Receipt {
                request: request.clone(),
                status: ReceiptStatus::Pending,
                error: None,
            },
        );
        let result = self.apply_member_control(&request).await;
        let receipt = self.session.team.receipts.get_mut(&key).unwrap();
        receipt.status = if result.is_ok() {
            ReceiptStatus::Applied
        } else {
            ReceiptStatus::Failed
        };
        receipt.error = result.err().map(|e| e.to_string());
        let message = format!(
            "directed request {key}: {:?}{}",
            receipt.status,
            receipt
                .error
                .as_ref()
                .map(|e| format!(" ({e})"))
                .unwrap_or_default()
        );
        self.emit(EventData::Diagnostic { message })
    }

    async fn apply_member_control(&mut self, request: &Request) -> anyhow::Result<()> {
        self.session.team.validate(&request.target)?;
        if request.target == Target::Main {
            let control = match &request.action {
                MemberControl::Pause => Control::Pause,
                MemberControl::Resume => Control::Resume,
                MemberControl::Stop => Control::Stop,
                MemberControl::Compact => Control::Compact,
                MemberControl::Message { text } => Control::Message { text: text.clone() },
                MemberControl::Side { text } => Control::Side { text: text.clone() },
                MemberControl::Limit { limit } => {
                    self.session.team.limit = *limit;
                    return Ok(());
                }
            };
            return Box::pin(self.handle_control(control)).await;
        }
        let id = request.target.member_id();
        let member = self.session.team.members.get_mut(id).unwrap();
        match &request.action {
            MemberControl::Limit { .. } => anyhow::bail!("team limit requires main target"),
            MemberControl::Pause | MemberControl::Stop => {
                member.paused = true;
                member.stopping |= request.action == MemberControl::Stop;
                for job in self
                    .jobs
                    .values()
                    .filter(|j| member.attempts.contains(&j.actor))
                {
                    job.cancel.cancel();
                }
            }
            MemberControl::Resume => {
                anyhow::ensure!(!member.stopping, "member stop is still settling");
                member.paused = false;
                // Global status remains authoritative; an idle member is never revived.
            }
            MemberControl::Compact => member.compact_pending = true,
            MemberControl::Side { text } => self.member_question(id, text, &request.id)?,
            MemberControl::Message { text } => {
                if member.assignment.is_none() {
                    self.member_question(id, text, &request.id)?;
                } else {
                    // Preserve unknown user intent for Task 2's scoped correction classifier.
                    // It must never enter the worker as lower-trust peer mail.
                    self.session.team.send(
                        &format!("user:{}:{}", request.client_id, request.id),
                        "user",
                        request.target.clone(),
                        text,
                        true,
                    )?;
                    self.session.team.members.get_mut(id).unwrap().paused = true;
                    for job in self
                        .jobs
                        .values()
                        .filter(|j| self.session.team.members[id].attempts.contains(&j.actor))
                    {
                        job.cancel.cancel();
                    }
                }
            }
        }
        Ok(())
    }

    fn member_question(
        &mut self,
        member_id: &str,
        text: &str,
        request: &str,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(!text.trim().is_empty(), "question cannot be empty");
        let member = &self.session.team.members[member_id];
        let id = format!("member-question-{}", uuid::Uuid::new_v4());
        let mut actor = Actor::new(
            id.clone(),
            Role::Question,
            text.into(),
            Scope {
                epoch: self.session.goal.epoch,
                requirements: self.session.goal.active_versions(),
                readonly: true,
                ..Scope::default()
            },
        );
        actor.parent = Some(member_id.into());
        actor.target = Some(self.session.member_target(member_id)?);
        actor.turn = request.into();
        if let Some(previous) = member
            .actor
            .as_ref()
            .and_then(|id| self.session.actors.get(id))
        {
            if previous.context.unanswered().is_empty() {
                actor.context = previous.context.clone();
            }
            actor.context.user(text);
        }
        self.session.actors.insert(id.clone(), actor);
        let member = self.session.team.members.get_mut(member_id).unwrap();
        member.attempts.push(id.clone());
        if member.assignment.is_none() {
            member.actor = Some(id);
        }
        Ok(())
    }
}
