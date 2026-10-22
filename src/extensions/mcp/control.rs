use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Enable,
    Disable,
    Reconnect,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Change {
    pub server: String,
    pub operation: Operation,
}
