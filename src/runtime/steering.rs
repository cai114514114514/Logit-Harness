use super::{
    actor::{Actor, Phase, Role},
    runner::Runner,
    state::Question,
};
use crate::domain::{Change, EventData, Status};
use crate::tools::Scope;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

impl Runner {
    pub(super) async fn apply_steering(
        &mut self,
        actor_id: &str,
        arguments: &Value,
    ) -> anyhow::Result<()> {
        #[derive(Deserialize)]
        struct Input {
            intent: String,
            updates: BTreeMap<String, String>,
            additions: Vec<String>,
            answer: String,
            reason: String,
            #[serde(default)]
            needs_tools: bool,
            question_id: Option<String>,
            #[serde(default)]
            objective_quotes: Vec<String>,
        }
        let input: Input = serde_json::from_value(arguments.clone())?;
        let message_id = actor_id
            .strip_prefix("steer-")
            .ok_or_else(|| anyhow::anyhow!("steering identity missing"))?;
        let message = self
            .session
            .inbox
            .iter()
            .find(|m| m.id == message_id)
            .ok_or_else(|| anyhow::anyhow!("message missing"))?
            .text
            .clone();
        let reconciliation = self
            .session
            .implementation
            .as_ref()
            .is_some_and(|task| task.id == message_id);
        let new_plan = reconciliation && self.session.goal.id.is_empty();
        anyhow::ensure!(
            !reconciliation
                || if new_plan {
                    matches!(input.intent.as_str(), "start" | "clarify")
                } else {
                    matches!(input.intent.as_str(), "refine" | "nudge")
                },
            "planning reconciliation must start a real quoted objective, clarify missing requirements, or selectively refine/nudge an existing goal; greetings and authorization are not requirements"
        );
        match input.intent.as_str() {
            "start" => {
                anyhow::ensure!(
                    new_plan,
                    "start is only available for planning without an existing goal"
                );
                anyhow::ensure!(
                    !input.objective_quotes.is_empty()
                        && input
                            .objective_quotes
                            .iter()
                            .all(|quote| !quote.trim().is_empty()
                                && self
                                    .session
                                    .plan_turns
                                    .iter()
                                    .any(|turn| turn.contains(quote))),
                    "initial objective requires nonempty exact quotes from actual planning user requests"
                );
                let mut goal = crate::domain::Goal::new(&input.objective_quotes.join("\n"));
                goal.epoch = self.session.goal.epoch.saturating_add(1);
                self.session.goal = goal;
                *self.tools.goal.write().await = self.session.goal.clone();
            }
            "clarify" if new_plan => {
                self.session.implementation = None;
                self.emit(EventData::Message {
                    actor: "question".into(),
                    text: input.answer,
                })?;
            }
            "queue" => self.queue_goal(message.clone(), message_id.into())?,
            "replace" if self.session.interactive => {
                self.request_boundary(super::conversation::Boundary::Goal {
                    text: message.clone(),
                    turn: message_id.into(),
                })?;
            }
            "answer" => {
                let id = input
                    .question_id
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("answer needs a question_id"))?;
                self.answer_question(id, &message).await?;
            }
            "refine" | "replace" => {
                let impact = self.session.goal.revise(Change {
                    source: message.clone(),
                    updates: input.updates,
                    additions: input.additions,
                    replacement: input.intent == "replace",
                })?;
                if self.session.condition == super::condition::Condition::Replan {
                    self.session.goal.epoch += 1;
                    self.session.graph = Default::default();
                    self.session.replan_pending = true;
                } else {
                    self.session.graph.invalidate(&impact.changed);
                }
                self.refresh_plan().await?;
                *self.tools.goal.write().await = self.session.goal.clone();
                self.refresh_questions().await;
                for job in self.jobs.values() {
                    if self.session.condition.cancels_active()
                        && job
                            .scope
                            .requirements
                            .keys()
                            .any(|id| impact.changed.contains(id))
                    {
                        job.cancel.cancel();
                    }
                }
                // Queued calls keep their original leases; ToolHost rejects them if the correction made them stale.
                self.main_feedback(&format!("User correction: {message}\nThe active requirements changed. Inspect affected work and continue. {}",input.reason));
                self.session.final_report = None;
                self.emit(EventData::Revised {
                    revision: self.session.goal.revision,
                    changed: impact.changed.into_iter().collect(),
                })?;
            }
            "question" => {
                if input.needs_tools {
                    let id = format!("question-{message_id}");
                    let mut actor = Actor::new(
                        id.clone(),
                        Role::Question,
                        message.clone(),
                        Scope {
                            requirements: self.session.goal.active_versions(),
                            readonly: true,
                            ..Scope::default()
                        },
                    );
                    actor.turn = message_id.into();
                    self.session.actors.insert(id, actor);
                } else {
                    self.emit(EventData::Message {
                        actor: "answer".into(),
                        text: input.answer,
                    })?;
                    let prior = self
                        .session
                        .inbox
                        .iter()
                        .find(|m| m.id == message_id)
                        .unwrap()
                        .prior
                        .clone();
                    let same_goal = self
                        .session
                        .inbox
                        .iter()
                        .find(|m| m.id == message_id)
                        .unwrap()
                        .revision
                        == self.session.goal.revision;
                    if same_goal
                        && matches!(prior, Status::Completed | Status::Waiting | Status::Paused)
                    {
                        self.status(prior, "side question answered; task state preserved")?;
                    }
                }
            }
            "nudge" => {
                self.main_feedback("Continue the existing goal. The user has not replaced it.")
            }
            "clarify" => {
                self.session.questions.insert(
                    message_id.into(),
                    Question {
                        text: input.answer.clone(),
                        requirements: self.session.goal.active_versions(),
                        answer: None,
                    },
                );
                self.refresh_questions().await;
                self.session.actors.get_mut("main").unwrap().phase = Phase::Blocked;
                self.emit(EventData::Message {
                    actor: "question".into(),
                    text: input.answer,
                })?;
            }
            _ => anyhow::bail!("unknown input classification"),
        }
        self.session
            .inbox
            .iter_mut()
            .find(|m| m.id == message_id)
            .unwrap()
            .applied = true;
        self.session.actors.get_mut(actor_id).unwrap().phase = Phase::Done;
        self.emit(EventData::Diagnostic {
            message: "user input classified and applied".into(),
        })?;
        if reconciliation && self.session.implementation.is_some() {
            let task = self.session.implementation.take().unwrap();
            self.request_boundary(super::conversation::Boundary::ResumeGoal {
                text: task.text,
                turn: task.turn,
            })?;
        }
        Ok(())
    }

    pub(super) fn finish_question(&mut self, actor_id: &str) -> anyhow::Result<()> {
        if self.session.actors[actor_id]
            .parent
            .as_ref()
            .is_some_and(|id| self.session.team.members.contains_key(id))
        {
            return Ok(());
        }

        let id = actor_id
            .strip_prefix("question-")
            .ok_or_else(|| anyhow::anyhow!("question identity missing"))?;
        let message = self
            .session
            .inbox
            .iter()
            .find(|m| m.id == id)
            .ok_or_else(|| anyhow::anyhow!("question source missing"))?;
        let prior = message.prior.clone();
        if matches!(prior, Status::Completed | Status::Waiting | Status::Paused)
            && self.session.status == Status::Running
            && message.revision == self.session.goal.revision
        {
            self.status(
                prior,
                "side question answered from actual evidence; task state preserved",
            )?;
        }
        Ok(())
    }
}
