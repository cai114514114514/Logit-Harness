use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ready,
    Running,
    Waiting,
    Paused,
    Exhausted,
    Completed,
    Stopped,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventData {
    Created {
        objective: String,
        workspace: String,
    },
    Status {
        status: Status,
        reason: String,
    },
    User {
        message: String,
    },
    Message {
        actor: String,
        text: String,
    },
    Delta {
        actor: String,
        text: String,
    },
    Revised {
        revision: u64,
        changed: Vec<String>,
    },
    ActionStarted {
        id: String,
        tool: String,
        arguments: Value,
    },
    ActionFinished {
        id: String,
        result: Value,
        success: bool,
    },
    Usage {
        actor: String,
        input: u64,
        output: u64,
        cached: u64,
    },
    Evidence {
        id: String,
        description: String,
    },
    Worker {
        id: String,
        status: String,
        task: String,
    },
    Approval {
        id: String,
        tool: String,
        reason: String,
    },
    Diagnostic {
        message: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionEvent {
    pub sequence: u64,
    pub session: String,
    pub timestamp: String,
    #[serde(flatten)]
    pub data: EventData,
}
