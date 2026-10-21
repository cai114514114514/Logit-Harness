use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Context {
    pub items: Vec<Value>,
}

impl Context {
    pub fn user(&mut self, text: &str) {
        self.items.push(json!({"role":"user", "content":text}));
    }
    pub fn append(&mut self, output: &[Value]) {
        self.items.extend_from_slice(output);
    }
    /// The event history preserves when a correction arrived. The Responses
    /// wire must keep every assistant tool batch adjacent to all its outputs:
    /// inserting a user message between them starts a new message and loses the
    /// association in DeepSeek's adapter. Delay only that message's wire position.
    pub fn request_items(&self) -> anyhow::Result<Vec<Value>> {
        self.validate_tool_pairing()?;
        let mut pending = BTreeSet::new();
        let mut delayed = vec![];
        let mut items = vec![];
        for item in &self.items {
            match item["type"].as_str() {
                Some("function_call" | "custom_tool_call") => {
                    pending.insert(item["call_id"].as_str().unwrap());
                }
                Some("function_call_output" | "custom_tool_call_output") => {
                    anyhow::ensure!(
                        pending.remove(item["call_id"].as_str().unwrap()),
                        "orphan or duplicate tool output"
                    );
                }
                _ => (),
            }
            if matches!(item["role"].as_str(), Some("user" | "developer")) && !pending.is_empty() {
                delayed.push(item.clone());
            } else {
                items.push(item.clone());
                if pending.is_empty() {
                    items.append(&mut delayed);
                }
            }
        }
        anyhow::ensure!(
            pending.is_empty(),
            "cannot dispatch across outstanding tool calls"
        );
        Ok(items)
    }
    pub fn tool_result(&mut self, id: &str, result: &Value) {
        // `Value::to_string` re-serializes: an already-string result gains a second layer of
        // quotes/escaping. Tool outputs are JSON-shaped for the model, so the uniform encoding
        // is intended, but callers must not pass pre-serialized JSON text.
        self.items.push(
            json!({"type":"function_call_output", "call_id":id, "output":result.to_string()}),
        );
    }
    pub fn compact(&mut self, summary: &str) -> anyhow::Result<()> {
        self.validate_tool_pairing()?;
        anyhow::ensure!(
            self.unanswered().is_empty(),
            "cannot compact across outstanding tool calls"
        );
        anyhow::ensure!(!summary.trim().is_empty(), "empty context summary");
        self.items = vec![json!({"role":"user", "content":summary})];
        Ok(())
    }
    pub fn tool_images(&mut self, id: &str, result: &Value, images: &[crate::tools::ToolImage]) {
        if images.is_empty() {
            self.tool_result(id, result);
            return;
        }
        let mut output = vec![json!({"type":"input_text","text":result.to_string()})];
        output.extend(
            images.iter().map(
                |image| json!({"type":"input_image","image_url":image.data_url,"detail":"high"}),
            ),
        );
        self.items
            .push(json!({"type":"function_call_output","call_id":id,"output":output}));
    }
    /// Ids of tool calls that have no matching result yet.
    ///
    /// Corrupt items (missing or empty call_id) are skipped rather than reported, because
    /// recovery consumes this infallibly; `compact` rejects such contexts via
    /// `validate_tool_pairing`, so corruption cannot be laundered through compaction.
    pub fn unanswered(&self) -> BTreeSet<String> {
        let mut pending = BTreeSet::new();
        for item in &self.items {
            let Some(id) = item["call_id"].as_str().filter(|id| !id.is_empty()) else {
                continue;
            };
            match item["type"].as_str() {
                Some("function_call" | "custom_tool_call") => {
                    pending.insert(id.to_owned());
                }
                // An output whose call was never seen is silently ignored: it answers nothing,
                // and recovery may only synthesize results for calls it can still find.
                Some("function_call_output" | "custom_tool_call_output") => {
                    pending.remove(id);
                }
                _ => (),
            }
        }
        pending
    }
    /// Hard pairing invariants enforced before compaction: a missing or empty call_id used to
    /// key both sides as "", letting an unanswered call and an orphan output cancel each other
    /// out; two calls sharing one id would collapse in the BTreeSet and look answerable by a
    /// single result. Both are data corruption, not answerable state.
    fn validate_tool_pairing(&self) -> anyhow::Result<()> {
        let mut calls = BTreeSet::new();
        for item in &self.items {
            match item["type"].as_str() {
                Some("function_call" | "custom_tool_call") => {
                    let id = item["call_id"]
                        .as_str()
                        .filter(|id| !id.is_empty())
                        .ok_or_else(|| anyhow::anyhow!("tool call is missing call_id"))?;
                    anyhow::ensure!(calls.insert(id), "duplicate tool call id in context");
                }
                Some("function_call_output" | "custom_tool_call_output") => {
                    anyhow::ensure!(
                        item["call_id"].as_str().is_some_and(|id| !id.is_empty()),
                        "tool result is missing call_id"
                    );
                }
                _ => (),
            }
        }
        Ok(())
    }
}
