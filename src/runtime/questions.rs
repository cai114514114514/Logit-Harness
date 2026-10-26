//! Missing input blocks its dependencies, while independent steps can keep moving.
use super::{
    actor::{Pending, Phase, Role},
    runner::Runner,
    state::Question,
};
use crate::{
    domain::{Change, EventData, Status},
    tools::Scope,
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

impl Runner {
    pub(super) fn blocked_requirements(&self) -> BTreeSet<String> {
        self.session
            .questions
            .values()
            .filter(|q| q.answer.is_none())
            .flat_map(|q| {
                q.requirements
                    .keys()
                    .filter(|id| {
                        self.session
                            .goal
                            .requirements
                            .get(*id)
                            .is_some_and(|r| r.active)
                    })
                    .cloned()
            })
            .collect()
    }
    pub(super) fn scope_blocked(&self, scope: &Scope) -> bool {
        let blocked = self.blocked_requirements();
        scope.requirements.keys().any(|id| blocked.contains(id))
    }
    pub(super) async fn refresh_questions(&mut self) {
        let blocked = self.blocked_requirements();
        *self.tools.blocked.write().await = blocked.clone();
        for job in self.jobs.values() {
            let role = &self.session.actors[&job.actor].role;
            if !matches!(role, Role::Steering | Role::Question)
                && job.scope.requirements.keys().any(|id| blocked.contains(id))
            {
                job.cancel.cancel();
            }
        }
    }
    pub(super) async fn ask(&mut self, actor_id: &str, pending: &Pending) -> anyhow::Result<()> {
        let text = pending.call.arguments["question"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("missing question"))?
            .to_owned();
        let ids: BTreeSet<String> =
            serde_json::from_value(pending.call.arguments["requirements"].clone())?;
        let versions = self.session.goal.active_versions();
        if self.session.actors[actor_id].role == Role::Worker {
            anyhow::ensure!(
                ids.iter()
                    .all(|id| pending.scope.requirements.contains_key(id)),
                "member question exceeds assignment requirements"
            );
        }
        anyhow::ensure!(
            !ids.is_empty() && ids.iter().all(|id| versions.contains_key(id)),
            "question needs active requirement IDs"
        );
        self.session.questions.insert(
            pending.call.id.clone(),
            Question {
                target: self.session.actors[actor_id].target.clone(),
                text: text.clone(),
                requirements: ids
                    .into_iter()
                    .map(|id| {
                        let rev = versions[&id];
                        (id, rev)
                    })
                    .collect(),
                answer: None,
            },
        );
        self.refresh_questions().await;
        let all_blocked = versions
            .keys()
            .all(|id| self.blocked_requirements().contains(id));
        let actor = self.session.actors.get_mut(actor_id).unwrap();
        actor.context.tool_result(&pending.call.id, &json!({"question":pending.call.id,"waiting_requirements":self.session.questions[&pending.call.id].requirements,"instruction":"Continue independent steps; use wait_input when no useful work remains."}));
        actor.phase = if all_blocked {
            Phase::Blocked
        } else {
            Phase::Ready
        };
        self.session.final_report = None;
        self.emit(EventData::Message {
            actor: if actor_id == "main" {
                "question".into()
            } else {
                actor_id.into()
            },
            text,
        })
    }
    pub(super) async fn answer_question(&mut self, id: &str, text: &str) -> anyhow::Result<()> {
        let question = self
            .session
            .questions
            .get(id)
            .filter(|q| q.answer.is_none())
            .ok_or_else(|| anyhow::anyhow!("unknown unanswered question"))?
            .clone();
        anyhow::ensure!(!text.trim().is_empty(), "answer cannot be empty");
        let mut updates = BTreeMap::new();
        for requirement in question.requirements.keys() {
            if let Some(current) = self
                .session
                .goal
                .requirements
                .get(requirement)
                .filter(|r| r.active)
            {
                updates.insert(
                    requirement.clone(),
                    format!("{}\nUser clarification: {text}", current.text),
                );
            }
        }
        anyhow::ensure!(
            !updates.is_empty(),
            "question dependencies are no longer active"
        );
        let before = self.contract_before();
        let impact = self.session.goal.revise(Change {
            source: text.into(),
            updates,
            additions: vec![],
            replacement: false,
        })?;
        self.session.questions.get_mut(id).unwrap().answer = Some(text.into());
        self.session.graph.invalidate(&impact.changed);
        *self.tools.goal.write().await = self.session.goal.clone();
        self.refresh_plan().await?;
        self.refresh_questions().await;
        self.main_feedback(&format!(
            "User answered question {id}: {text}. Reconcile dependent steps and continue."
        ));
        self.session.final_report = None;
        self.emit(EventData::Revised {
            revision: self.session.goal.revision,
            changed: impact.changed.into_iter().collect(),
        })?;
        self.emit_contract("answer", text, Some(before))?;
        if self.session.status == Status::Waiting {
            self.status(Status::Running, "required input received")?;
        }
        Ok(())
    }
    pub(super) fn focus_step(&mut self, actor_id: &str, pending: &Pending) -> anyhow::Result<()> {
        let step = pending.call.arguments["step"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing step ID"))?;
        let mut scope = self.session.graph.scope(step, &self.session.goal)?;
        anyhow::ensure!(
            !self.scope_blocked(&scope),
            "step is waiting for user input"
        );
        scope.readonly = false;
        let actor = self.session.actors.get_mut(actor_id).unwrap();
        scope.observed = actor.scope.observed.clone();
        actor.scope = scope;
        actor.step = Some(step.into());
        actor
            .context
            .tool_result(&pending.call.id, &json!({"step":step,"scope":actor.scope}));
        actor.phase = Phase::Ready;
        Ok(())
    }
}
