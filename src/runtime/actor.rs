use crate::{
    model::{Context, ToolCall},
    tools::Scope,
};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Main,
    Worker,
    Verifier,
    Steering,
    Question,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Ready,
    Busy,
    Awaiting,
    Done,
    Blocked,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pending {
    pub call: ToolCall,
    pub scope: Scope,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Actor {
    pub id: String,
    pub role: Role,
    pub task: String,
    pub context: Context,
    pub scope: Scope,
    pub phase: Phase,
    pub pending: VecDeque<Pending>,
    pub busy: u32,
    pub failures: u32,
    pub turns: u32,
}
impl Actor {
    pub fn new(id: String, role: Role, task: String, scope: Scope) -> Self {
        let mut context = Context::default();
        context.user(&task);
        Self {
            id,
            role,
            task,
            context,
            scope,
            phase: Phase::Ready,
            pending: VecDeque::new(),
            busy: 0,
            failures: 0,
            turns: 0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Action {
    pub id: String,
    pub actor: String,
    pub pending: Pending,
    pub running: bool,
    pub pid: Option<u32>,
}
