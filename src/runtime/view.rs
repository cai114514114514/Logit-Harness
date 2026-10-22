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
        Self {
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
