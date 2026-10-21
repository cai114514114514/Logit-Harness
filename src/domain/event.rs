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
    Pausing,
    Stopping,
}

// 持久化契约：serde 形状（tag="type"、snake_case 变体名、字段名）与 SQLite 中
// 已写入的事件一一对应，任何字段名或标签改动都会破坏历史数据读取，只可加变体。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventData {
    ModelRequested {
        actor: String,
        artifact: String,
    },
    ModelFinished {
        actor: String,
        artifact: String,
        model: String,
    },
    ProcessStarted {
        id: String,
        pid: u32,
    },
    Plan {
        steps: serde_json::Value,
    },
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
        // 历史原因：worker 状态来自外部 worker 协议，取值不收敛到本体的 Status
        // 枚举；且持久化契约要求字段保持字符串，故不改类型。
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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SessionEvent {
    pub sequence: u64,
    pub session: String,
    // 宽进设计：RFC3339 字符串原样存储，不强制解析成时间类型——任何来源的
    // 时间格式都能写入，展示层原样回放；flatten 属持久化契约，不可改。
    pub timestamp: String,
    #[serde(flatten)]
    pub data: EventData,
}
