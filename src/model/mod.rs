mod context;
mod deepseek;
mod failure;
mod stream;

pub use context::Context;
pub use deepseek::DeepSeek;
pub use failure::{ReportedFailure, RequestFailure, failure_usage};
pub use stream::StreamDecoder;

use crate::domain::Usage;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Headroom reserved on top of the measured request bytes, covering the envelope the
/// provider adds on its side (role wrappers, tool schema framing) that we cannot measure.
const RESERVE_HEADROOM: u64 = 4096;

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
    /// Byte lengths stand in for token counts: a conservative upper bound, since one token
    /// never spans more text than its UTF-8 bytes.
    pub fn reserve_tokens(&self) -> u64 {
        // Serialization failure is counted as 0: input/tools are already `Value`s, so failure
        // is practically impossible, and under-reserving beats rejecting the request over an
        // unreachable error.
        self.instructions.len() as u64
            + serde_json::to_vec(&self.input).map_or(0, |v| v.len()) as u64
            + serde_json::to_vec(&self.tools).map_or(0, |v| v.len()) as u64
            + u64::from(self.max_output)
            + RESERVE_HEADROOM
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
    /// Indexing a missing field yields `Value::Null` (serde_json semantics), so items without
    /// content or text simply fail the filters below instead of panicking.
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
        let mut calls = vec![];
        for (index, item) in self.output.iter().enumerate() {
            if item["type"] != "function_call" {
                continue;
            }
            let call =
                ToolCall {
                    id: item["call_id"]
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("missing call_id at output item {index}"))?
                        .into(),
                    name: item["name"]
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("missing tool name at output item {index}"))?
                        .into(),
                    arguments: serde_json::from_str(item["arguments"].as_str().ok_or_else(
                        || anyhow::anyhow!("missing arguments at output item {index}"),
                    )?)?,
                };
            // A whitespace-only name still passes this check; providers do not emit one in
            // practice, and the tool registry lookup downstream rejects unknown names anyway.
            anyhow::ensure!(
                !call.id.is_empty() && !call.name.is_empty(),
                "empty tool identity at output item {index}"
            );
            anyhow::ensure!(
                ids.insert(call.id.clone()),
                "duplicate tool call id at output item {index}"
            );
            anyhow::ensure!(
                call.arguments.is_object(),
                "tool arguments must be an object at output item {index}"
            );
            calls.push(call);
        }
        Ok(calls)
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
