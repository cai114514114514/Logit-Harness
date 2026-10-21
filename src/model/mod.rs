mod context;
mod deepseek;
mod stream;

pub use context::Context;
pub use deepseek::DeepSeek;
pub use stream::StreamDecoder;

use crate::domain::Usage;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Clone, Debug)]
pub struct Request {
    pub instructions: String,
    pub input: Vec<Value>,
    pub tools: Vec<Value>,
    pub max_output: u32,
}

impl Request {
    pub fn reserve_tokens(&self) -> u64 {
        self.instructions.len() as u64
            + serde_json::to_vec(&self.input).map_or(0, |v| v.len()) as u64
            + serde_json::to_vec(&self.tools).map_or(0, |v| v.len()) as u64
            + u64::from(self.max_output)
            + 4096
    }
}

#[derive(Clone, Debug)]
pub enum ModelEvent {
    Text(String),
    Reasoning(String),
    Retry(u32),
}

#[derive(Clone, Debug)]
pub struct Response {
    pub output: Vec<Value>,
    pub usage: Usage,
    pub model: String,
}

impl Response {
    pub fn text(&self) -> String {
        self.output
            .iter()
            .filter(|v| v["type"] == "message")
            .filter_map(|v| v["content"].as_array())
            .flatten()
            .filter_map(|v| v["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
    pub fn calls(&self) -> anyhow::Result<Vec<ToolCall>> {
        let mut ids = std::collections::BTreeSet::new();
        self.output
            .iter()
            .filter(|v| v["type"] == "function_call")
            .map(|v| {
                let call = ToolCall {
                    id: v["call_id"]
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("missing call_id"))?
                        .into(),
                    name: v["name"]
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("missing tool name"))?
                        .into(),
                    arguments: serde_json::from_str(
                        v["arguments"]
                            .as_str()
                            .ok_or_else(|| anyhow::anyhow!("missing arguments"))?,
                    )?,
                };
                anyhow::ensure!(
                    !call.id.is_empty() && !call.name.is_empty(),
                    "empty tool identity"
                );
                anyhow::ensure!(ids.insert(call.id.clone()), "duplicate tool call id");
                anyhow::ensure!(
                    call.arguments.is_object(),
                    "tool arguments must be an object"
                );
                Ok(call)
            })
            .collect()
    }
}

#[async_trait::async_trait]
pub trait Provider: Send + Sync {
    async fn respond(
        &self,
        request: Request,
        cancel: CancellationToken,
        events: mpsc::UnboundedSender<ModelEvent>,
    ) -> anyhow::Result<Response>;
}
