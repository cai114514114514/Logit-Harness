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
        match input.intent.as_str() {
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
                self.session.graph.invalidate(&impact.changed);
                self.refresh_plan().await?;
                *self.tools.goal.write().await = self.session.goal.clone();
                self.refresh_questions().await;
                for job in self.jobs.values() {
                    if job
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
                    let actor = Actor::new(
                        id.clone(),
                        Role::Question,
                        message.clone(),
                        Scope {
                            requirements: self.session.goal.active_versions(),
                            readonly: true,
                            ..Scope::default()
                        },
                    );
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
        Ok(())
    }

    pub(super) fn finish_question(&mut self, actor_id: &str) -> anyhow::Result<()> {
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
