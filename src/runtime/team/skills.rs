//! Deferred member workflows keep the original assignment and tool pairs intact.
use super::{MemberControl, ReceiptStatus, Request, Target};
use crate::{
    extensions::skills::Invocation,
    runtime::{
        actor::{Actor, Phase, Role},
        runner::Runner,
    },
    tools::Scope,
};

impl Runner {
    fn selected_skill(&self, id: &str, hash: &str, arguments: &str) -> anyhow::Result<Invocation> {
        let knowledge = self
            .tools
            .knowledge
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("skills unavailable in this runner"))?;
        knowledge.refresh()?;
        let invocation = knowledge
            .skills
            .read()
            .map_err(|_| anyhow::anyhow!("skill catalog lock poisoned"))?
            .invoke(id, arguments)?;
        anyhow::ensure!(
            invocation.hash == hash,
            "skill changed since selection; invoke it again after reviewing /skills"
        );
        Ok(invocation)
    }
    pub(super) fn validate_member_skill(
        &self,
        id: &str,
        hash: &str,
        arguments: &str,
    ) -> anyhow::Result<()> {
        self.selected_skill(id, hash, arguments).map(|_| ())
    }
    pub(crate) fn settle_member_skills(&mut self) -> anyhow::Result<()> {
        let pending = self
            .session
            .team
            .receipts
            .values()
            .filter(|r| {
                r.status == ReceiptStatus::Pending
                    && matches!(r.request.action, MemberControl::Skill { .. })
                    && r.request.target != Target::Main
            })
            .map(|r| r.request.clone())
            .collect::<Vec<_>>();
        for request in pending {
            let result = self.apply_member_skill(&request);
            let status = match &result {
                Ok(false) => continue,
                Ok(true) => ReceiptStatus::Applied,
                Err(_) => ReceiptStatus::Failed,
            };
            let receipt = self.session.team.receipts.get_mut(&request.key()).unwrap();
            receipt.status = status;
            receipt.error = result.err().map(|e| e.to_string());
            self.publish_metadata()?;
        }
        Ok(())
    }
    fn apply_member_skill(&mut self, request: &Request) -> anyhow::Result<bool> {
        let MemberControl::Skill {
            id,
            hash,
            arguments,
        } = &request.action
        else {
            unreachable!()
        };
        let archived = !self
            .session
            .team
            .members
            .contains_key(request.target.member_id());
        let invocation = self.selected_skill(id, hash, arguments)?;
        if archived {
            anyhow::ensure!(
                self.session.archived_target(&request.target).is_some(),
                "stale archived member assignment"
            );
            self.readonly_member_skill(request, invocation)?;
            return Ok(true);
        }
        self.session.team.validate(&request.target)?;
        let member = &self.session.team.members[request.target.member_id()];
        if member.paused || member.stopping || member.interrupted {
            return Ok(false);
        }
        if member
            .attempts
            .iter()
            .any(|actor| self.jobs.values().any(|j| &j.actor == actor))
        {
            return Ok(false);
        }
        let readonly = member.assignment.is_none()
            || invocation.name == "review"
            || self.session.mode != crate::runtime::conversation::Mode::Engineering;
        if readonly {
            self.readonly_member_skill(request, invocation)?;
            return Ok(true);
        }
        let actor_id = member
            .actor
            .clone()
            .ok_or_else(|| anyhow::anyhow!("member assignment has no actor"))?;
        let actor = &self.session.actors[&actor_id];
        anyhow::ensure!(
            !actor.retired,
            "member assignment retired before skill application"
        );
        if !actor.pending.is_empty()
            || !actor.context.unanswered().is_empty()
            || self.session.actions.values().any(|a| a.actor == actor_id)
        {
            return Ok(false);
        }
        let text = invocation.request();
        let turn = format!("member-skill-{}", uuid::Uuid::new_v4());
        self.session.workflows.insert(turn.clone(), invocation);
        let actor = self.session.actors.get_mut(&actor_id).unwrap();
        actor.turn = turn;
        actor.response = None;
        actor.context.user(&text);
        actor.phase = Phase::Ready;
        // scope, lease, task, explicit pause and global status are untouched.
        Ok(true)
    }
    fn readonly_member_skill(
        &mut self,
        request: &Request,
        invocation: Invocation,
    ) -> anyhow::Result<()> {
        let text = invocation.request();
        let id = format!("member-skill-{}", uuid::Uuid::new_v4());
        let mut actor = Actor::new(
            id.clone(),
            Role::Question,
            text,
            Scope {
                epoch: self.session.goal.epoch,
                readonly: true,
                ..Scope::default()
            },
        );
        let source = self
            .session
            .team
            .members
            .get(request.target.member_id())
            .and_then(|m| m.actor.as_ref())
            .and_then(|id| self.session.actors.get(id))
            .or_else(|| {
                self.session.archives.iter().rev().find_map(|archive| {
                    archive
                        .team
                        .members
                        .get(request.target.member_id())
                        .and_then(|m| m.actor.as_ref())
                        .and_then(|id| archive.actors.get(id))
                })
            });
        if let Some(source) = source
            && source.context.unanswered().is_empty()
        {
            actor.context = source.context.clone();
            actor.context.user(&invocation.request());
        }
        actor.target = Some(request.target.clone());
        actor.parent = Some(request.target.member_id().into());
        actor.turn = id.clone();
        self.session.workflows.insert(id.clone(), invocation);
        self.session.actors.insert(id.clone(), actor);
        if let Some(member) = self
            .session
            .team
            .members
            .get_mut(request.target.member_id())
        {
            member.attempts.push(id.clone());
            if member.assignment.is_none() {
                member.actor = Some(id);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
