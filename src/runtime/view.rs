//! A terminal refresh needs execution state, not every actor's model and image history.
use super::{
    graph::Graph,
    state::{Question, Session},
};
use crate::domain::{Budget, Goal, Status};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionView {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub mode: super::conversation::Mode,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub queue: Vec<super::conversation::Task>,
    #[serde(default)]
    pub activities: Vec<Activity>,
    #[serde(default)]
    pub approval_actions: Vec<String>,
    #[serde(default)]
    pub verifying: bool,
    pub id: String,
    pub workspace: PathBuf,
    pub goal: Goal,
    pub status: Status,
    pub graph: Graph,
    pub questions: BTreeMap<String, Question>,
    pub budget: Budget,
    pub seconds: u64,
    pub elapsed: u64,
}
impl From<Session> for SessionView {
    fn from(session: Session) -> Self {
        let mut activities = session
            .actions
            .values()
            .filter(|a| a.running)
            .map(|a| Activity {
                id: a.id.clone(),
                actor: a.actor.clone(),
                title: format!(
                    "{} {}",
                    a.pending.call.name,
                    a.pending.call.arguments["command"]
                        .as_str()
                        .or_else(|| a.pending.call.arguments["path"].as_str())
                        .unwrap_or("")
                ),
            })
            .collect::<Vec<_>>();
        for actor in session.actors.values().filter(|a| !a.retired && a.busy > 0) {
            if !activities.iter().any(|a| a.actor == actor.id) {
                activities.push(Activity {
                    id: actor.id.clone(),
                    actor: actor.id.clone(),
                    title: format!("模型响应 · {}", actor.task.lines().next().unwrap_or("")),
                });
            }
        }
        let approval_actions = session
            .actions
            .values()
            .filter_map(|a| {
                a.approval
                    .as_ref()
                    .map(|id| format!("{} · /approve {id}", a.pending.call.name))
            })
            .collect();
        Self {
            activities,
            approval_actions,
            verifying: session.verification_pending,
            title: if session.title.is_empty() {
                if session.goal.original.is_empty() {
                    "新对话".into()
                } else {
                    session.goal.original.clone()
                }
            } else {
                session.title
            },
            mode: session.mode,
            owner: session.owner,
            queue: session.queue,
            id: session.id,
            workspace: session.workspace,
            goal: session.goal,
            status: session.status,
            graph: session.graph,
            questions: session.questions,
            budget: session.budget,
            seconds: session.seconds,
            elapsed: session.elapsed,
        }
    }
}

/// Lightweight execution facts; never expose model contexts or private reasoning.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Activity {
    pub id: String,
    pub actor: String,
    pub title: String,
}
