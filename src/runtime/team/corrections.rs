//! Addressed user intent never enters the global inbox or lower-trust peer mail.
use super::{AssignmentStatus, Correction, MemberControl, ReceiptStatus, Target};
use crate::{
    domain::{Change, EventData},
    runtime::{
        actor::{Actor, Phase, Role},
        runner::Runner,
        state::Session,
    },
    tools::Scope,
};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

impl Session {
    pub fn invalidate_step(
        &mut self,
        step: &str,
        source: &str,
    ) -> anyhow::Result<BTreeSet<String>> {
        anyhow::ensure!(
            self.graph.steps.contains_key(step),
            "unknown corrected step"
        );
        anyhow::ensure!(
            !source.trim().is_empty(),
            "correction needs exact user provenance"
        );
        let target = self
            .team
            .assignments
            .values()
            .find(|a| a.step == step && a.status == AssignmentStatus::Active)
            .map(|a| self.member_target(&a.member))
            .transpose()?;
        let affected = self.graph.invalidate_steps(&BTreeSet::from([step.into()]));
        for assignment in self
            .team
            .assignments
            .values_mut()
            .filter(|a| a.status == AssignmentStatus::Active && affected.contains(&a.step))
        {
            assignment.version = assignment.version.saturating_add(1);
        }
        self.team.corrections.push(Correction {
            target,
            source: source.into(),
            steps: affected.clone(),
        });
        self.final_report = None;
        self.verification_pending = false;
        Ok(affected)
    }
}

#[derive(Deserialize)]
struct Classification {
    intent: String,
    #[serde(default)]
    updates: BTreeMap<String, String>,
    #[serde(default)]
    additions: Vec<String>,
    #[serde(default)]
    answer: String,
    #[serde(default)]
    needs_tools: bool,
    question_id: Option<String>,
}

impl Runner {
    pub(crate) async fn start_member_steering(&mut self) -> anyhow::Result<()> {
        if self
            .session
            .actors
            .values()
            .any(|a| a.role == Role::Steering && a.phase != Phase::Done)
        {
            return Ok(());
        }
        let Some(mail) = self
            .session
            .team
            .mail
            .iter()
            .find(|m| m.user && !m.delivered && m.error.is_none())
        else {
            return Ok(());
        };
        if let Err(error) = self.session.team.validate(&mail.target) {
            let id = mail.id.clone();
            return self
                .finish_member_input("", &id, Some(error.to_string()))
                .await;
        }
        let mut actor = Actor::new(
            format!("member-steer-{}", mail.id),
            Role::Steering,
            mail.text.clone(),
            Scope {
                epoch: self.session.goal.epoch,
                requirements: self.session.goal.active_versions(),
                readonly: true,
                ..Scope::default()
            },
        );
        actor.target = Some(mail.target.clone());
        actor.parent = Some(mail.target.member_id().into());
        actor.turn = mail.id.clone();
        self.session
            .team
            .members
            .get_mut(mail.target.member_id())
            .unwrap()
            .attempts
            .push(actor.id.clone());
        self.session.actors.insert(actor.id.clone(), actor);
        Ok(())
    }

    pub(crate) async fn apply_member_steering(
        &mut self,
        actor_id: &str,
        arguments: &serde_json::Value,
    ) -> anyhow::Result<()> {
        let input: Classification = serde_json::from_value(arguments.clone())?;
        let mail_id = self.session.actors[actor_id].turn.clone();
        let mail = self
            .session
            .team
            .mail
            .iter()
            .find(|m| m.id == mail_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("directed user input missing"))?;
        if let Err(error) = self.session.team.validate(&mail.target) {
            return self
                .finish_member_input(actor_id, &mail_id, Some(error.to_string()))
                .await;
        }
        let member_id = mail.target.member_id();
        match input.intent.as_str() {
            "question" => {
                self.member_question(member_id, &mail.text, &mail.id)?;
                if !input.needs_tools {
                    let id = self.session.team.members[member_id]
                        .attempts
                        .last()
                        .unwrap()
                        .clone();
                    self.session.actors.get_mut(&id).unwrap().phase = Phase::Done;
                    self.emit(EventData::Message {
                        actor: id,
                        text: input.answer,
                    })?;
                }
            }
            "answer" => {
                let question = input
                    .question_id
                    .ok_or_else(|| anyhow::anyhow!("answer needs question_id"))?;
                anyhow::ensure!(
                    self.session
                        .questions
                        .get(&question)
                        .is_some_and(|q| q.target.as_ref() == Some(&mail.target)),
                    "question belongs to another assignment"
                );
                self.answer_question(&question, &mail.text).await?;
            }
            "local" | "nudge" => {
                anyhow::ensure!(
                    input.updates.is_empty() && input.additions.is_empty(),
                    "delivery changes must use refine"
                );
                let Target::Member {
                    assignment: Some(reference),
                    ..
                } = &mail.target
                else {
                    anyhow::bail!("local correction needs an assignment");
                };
                let step = self.session.team.assignments[&reference.id].step.clone();
                let affected = self.session.invalidate_step(&step, &mail.text)?;
                for job in self
                    .jobs
                    .values()
                    .filter(|job| job.scope.steps.keys().any(|step| affected.contains(step)))
                {
                    job.cancel.cancel();
                }
                self.refresh_plan().await?;
                self.main_feedback(&format!("User execution correction for step {step}: {}\nAffected attempts were invalidated. Inspect settled work and reassign corrected steps.", mail.text));
            }
            "refine" | "clarify" => {
                // Unclear impact retains exact user words in acceptance rather
                // than silently downgrading the request to context.
                let mut additions = input.additions;
                if input.updates.is_empty() && additions.is_empty() {
                    additions.push(mail.text.clone());
                }
                let impact = self.session.goal.revise(Change {
                    source: mail.text.clone(),
                    updates: input.updates,
                    additions,
                    replacement: false,
                })?;
                self.session.graph.invalidate(&impact.changed);
                *self.tools.goal.write().await = self.session.goal.clone();
                self.refresh_plan().await?;
                self.refresh_questions().await;
                self.session.final_report = None;
                self.main_feedback(&format!(
                    "Member-directed user correction changed canonical requirements: {}",
                    mail.text
                ));
                self.emit(EventData::Revised {
                    revision: self.session.goal.revision,
                    changed: impact.changed.into_iter().collect(),
                })?;
            }
            _ => anyhow::bail!(
                "member input permits question, answer, local, refine or clarify; whole-goal replacement requires main"
            ),
        }
        self.finish_member_input(actor_id, &mail_id, None).await
    }

    async fn finish_member_input(
        &mut self,
        actor: &str,
        mail_id: &str,
        error: Option<String>,
    ) -> anyhow::Result<()> {
        let mail = self
            .session
            .team
            .mail
            .iter_mut()
            .find(|m| m.id == mail_id)
            .unwrap();
        mail.delivered = error.is_none();
        mail.error = error.clone();
        let member = mail.target.member_id().to_owned();
        for receipt in self.session.team.receipts.values_mut().filter(|r| {
            r.request.mail_id() == mail_id
                && matches!(r.request.action, MemberControl::Message { .. })
        }) {
            receipt.status = if error.is_none() {
                ReceiptStatus::Applied
            } else {
                ReceiptStatus::Failed
            };
            receipt.error = error.clone();
        }
        let pending =
            self.session.team.mail.iter().any(|m| {
                m.user && m.target.member_id() == member && !m.delivered && m.error.is_none()
            });
        self.session
            .team
            .members
            .get_mut(&member)
            .unwrap()
            .interrupted = pending;
        if let Some(actor) = self.session.actors.get_mut(actor) {
            actor.phase = Phase::Done;
        }
        self.sync_assignments().await;
        self.emit(EventData::Diagnostic {
            message: format!(
                "directed input {mail_id}: {}",
                error.as_deref().unwrap_or("applied")
            ),
        })
    }
}
