//! Durable conversation state. Engineering goals remain compatible with old sessions,
//! but a chat has no requirement or goal identity until the user asks for work.
use super::{
    actor::{Actor, Phase, Role},
    graph::Graph,
    runner::Runner,
    state::{Session, UserMessage},
};
use crate::{
    domain::{EventData, Evidence, Goal, Status},
    tools::Scope,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Conversation,
    Plan,
    #[default]
    Engineering,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub text: String,
    pub turn: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Turn {
    pub id: String,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Archive {
    pub goal: Goal,
    pub status: Status,
    pub graph: Graph,
    pub actors: BTreeMap<String, Actor>,
    pub evidence: Vec<Evidence>,
    pub actions: BTreeMap<String, super::actor::Action>,
    pub report: Option<String>,
}

/// Applied only after cancelled operations have returned and their usage settled.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Boundary {
    Plan {
        text: Option<String>,
        turn: Option<String>,
    },
    Goal {
        text: String,
        turn: String,
    },
    Implement {
        text: String,
        turn: String,
    },
    ResumeGoal {
        text: String,
        turn: String,
    },
    Conversation {
        text: String,
        turn: String,
    },
}

impl Session {
    pub fn conversation(workspace: PathBuf, input: &str, tokens: u64, seconds: u64) -> Self {
        let mut session = Self::new(workspace, input, tokens, seconds);
        session.interactive = true;
        session.mode = Mode::Conversation;
        session.goal = empty_goal(0);
        session
            .actors
            .insert("main".into(), conversation_actor(input, 0, true));
        session.turns[0].id = session.actors["main"].turn.clone();
        if input.trim().is_empty() {
            session.status = Status::Waiting;
            session.actors.get_mut("main").unwrap().phase = Phase::Done;
        }
        session
    }

    pub fn active_goal(&self) -> Option<&Goal> {
        (self.mode == Mode::Engineering
            && !self.goal.id.is_empty()
            && self.status != Status::Completed)
            .then_some(&self.goal)
    }
}

pub(super) fn empty_goal(epoch: u64) -> Goal {
    let mut goal = Goal::new("");
    goal.id.clear();
    goal.requirements.clear();
    goal.revision = 0;
    goal.epoch = epoch;
    goal
}

pub(super) fn conversation_actor(text: &str, epoch: u64, allow_goal: bool) -> Actor {
    let mut actor = Actor::new(
        "main".into(),
        Role::Conversation,
        text.into(),
        Scope {
            epoch,
            readonly: true,
            ..Scope::default()
        },
    );
    actor.allow_goal = allow_goal;
    actor
}

impl Runner {
    pub(super) async fn conversation_call(
        &mut self,
        id: &str,
        pending: &super::actor::Pending,
    ) -> anyhow::Result<bool> {
        let actor = &self.session.actors[id];
        if !matches!(actor.role, Role::Conversation | Role::Question) {
            return Ok(false);
        }
        if pending.call.name == "begin_goal" {
            anyhow::ensure!(
                actor.role == Role::Conversation && actor.allow_goal,
                "only a fresh explicit user request can leave read-only planning"
            );
            let text = actor.task.clone();
            let turn = actor.turn.clone();
            anyhow::ensure!(!text.trim().is_empty(), "engineering requires user input");
            self.session
                .actors
                .get_mut(id)
                .unwrap()
                .context
                .tool_result(&pending.call.id, &serde_json::json!({"accepted":true}));
            self.request_boundary(if self.session.mode == Mode::Plan {
                Boundary::Implement { text, turn }
            } else {
                Boundary::Goal { text, turn }
            })?;
            return Ok(true);
        }
        // Reading a skill or other tool result cannot create plan authorization.
        if self.session.mode == Mode::Plan {
            self.session.actors.get_mut(id).unwrap().allow_goal = false;
        }
        anyhow::ensure!(
            self.tools.call_effect(&pending.call) == Some(crate::tools::Effect::Read),
            "conversation and planning permit only read-only tools"
        );
        Ok(false)
    }

    pub(super) fn conversation_message(&mut self, text: String) -> anyhow::Result<()> {
        if self.session.actors["main"].busy != 0 || !self.jobs.is_empty() {
            let turn = uuid::Uuid::new_v4().to_string();
            self.session.turns.push(Turn {
                id: turn.clone(),
                text: text.clone(),
            });
            self.request_boundary(Boundary::Conversation { text, turn })?;
        } else {
            self.add_turn(&text, true, None);
            self.status(Status::Running, "answering user input")?;
        }
        Ok(())
    }

    pub(super) fn add_turn(&mut self, text: &str, allow_goal: bool, turn: Option<String>) {
        if self.session.mode == Mode::Plan {
            self.session.plan_turns.push(text.into());
        }
        if self.session.title.trim().is_empty() {
            self.session.title = text.into();
        }
        let actor = self.session.actors.get_mut("main").unwrap();
        actor.role = Role::Conversation;
        actor.task = text.into();
        actor.turn = turn.unwrap_or_else(|| {
            let id = uuid::Uuid::new_v4().to_string();
            self.session.turns.push(Turn {
                id: id.clone(),
                text: text.into(),
            });
            id
        });
        actor.response = None;
        actor.context.user(text);
        actor.phase = Phase::Ready;
        actor.retired = false;
        actor.allow_goal = allow_goal;
        actor.scope.readonly = true;
        actor.scope.requirements.clear();
        actor.scope.steps.clear();
    }

    pub(super) fn side_question(&mut self, text: String) -> anyhow::Result<()> {
        let id = uuid::Uuid::new_v4().to_string();
        self.session.inbox.push(UserMessage {
            id: id.clone(),
            text: text.clone(),
            applied: true,
            prior: self.session.status.clone(),
            revision: self.session.goal.revision,
        });
        let mut actor = Actor::new(
            format!("question-{id}"),
            Role::Question,
            text.clone(),
            Scope {
                epoch: self.session.goal.epoch,
                requirements: self.session.goal.active_versions(),
                readonly: true,
                ..Scope::default()
            },
        );
        actor.turn = id.clone();
        self.session.turns.push(Turn {
            id,
            text: text.clone(),
        });
        self.session.actors.insert(actor.id.clone(), actor);
        self.emit(EventData::User { message: text })
    }

    pub(super) fn queue_goal(&mut self, text: String, turn: String) -> anyhow::Result<()> {
        anyhow::ensure!(!text.trim().is_empty(), "goal cannot be empty");
        self.session.queue.push(Task {
            id: uuid::Uuid::new_v4().to_string(),
            text,
            turn,
        });
        self.emit(EventData::Diagnostic {
            message: "independent task saved in the pending queue".into(),
        })
    }
}
