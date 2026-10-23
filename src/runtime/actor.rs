use crate::{
    model::{Context, ToolCall},
    tools::Scope,
};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Conversation,
    Main,
    Worker,
    Verifier,
    Steering,
    Question,
    Diagnosis,
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
    /// Immutable submission target: a late side reply belongs to this assignment,
    /// even when its stable member has already moved on to another task.
    #[serde(default)]
    pub target: Option<super::team::Target>,
    #[serde(default = "identity")]
    pub turn: String,
    #[serde(default)]
    pub response: Option<String>,
    #[serde(default)]
    pub compact: bool,
    /// Promotion in plan mode is offered only for a fresh, actual user turn.
    /// Reading tool/skill content consumes this opportunity.
    #[serde(default)]
    pub allow_goal: bool,
    pub id: String,
    pub role: Role,
    pub task: String,
    pub context: Context,
    pub scope: Scope,
    /// Scheduling state. Ready means the actor may receive a model turn;
    /// Busy/Awaiting mirror in-flight work (`busy`/`pending`); Blocked waits
    /// on a user decision; Done never runs again.
    pub phase: Phase,
    /// Queued provider tool calls not yet dispatched. Non-empty implies
    /// phase Awaiting.
    pub pending: VecDeque<Pending>,
    /// Number of in-flight jobs (model or tool) owned by this actor. Kept as
    /// a counter next to the runner's job map: it persists across restarts
    /// where the job map cannot, and recovery resets it to 0.
    pub busy: u32,
    /// Consecutive model-level failures since the last success; reaching the
    /// runner's failure cap halts the session as Failed. Reset by any
    /// successful model response.
    pub failures: u32,
    /// Lifetime count of model turns dispatched for this actor; diagnostic
    /// only, never reset.
    pub turns: u32,
    /// The plan step a Worker owns; None for every other role.
    pub step: Option<String>,
    /// A retired actor's work was invalidated by a goal/plan revision; it
    /// receives no new work and its finished results are ignored.
    #[serde(default)]
    pub retired: bool,
    #[serde(default)]
    pub progress: super::progress::Progress,
    pub parent: Option<String>,
}
impl Actor {
    pub fn new(id: String, role: Role, task: String, scope: Scope) -> Self {
        let mut context = Context::default();
        context.user(&task);
        Self {
            target: None,
            turn: identity(),
            response: None,
            compact: false,
            allow_goal: false,
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
            step: None,
            retired: false,
            progress: Default::default(),
            parent: None,
        }
    }
}

fn identity() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Action {
    pub id: String,
    pub actor: String,
    pub pending: Pending,
    pub running: bool,
    pub pid: Option<u32>,
    #[serde(default)]
    pub approval: Option<String>,
}
