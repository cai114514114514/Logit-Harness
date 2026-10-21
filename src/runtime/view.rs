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
