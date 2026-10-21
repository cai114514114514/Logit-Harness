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
    pub fn tool_result(&mut self, id: &str, result: &Value) {
        self.items.push(
            json!({"type":"function_call_output", "call_id":id, "output":result.to_string()}),
        );
    }
    pub fn compact(&mut self, summary: &str) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.unanswered().is_empty(),
            "cannot compact across outstanding tool calls"
        );
        anyhow::ensure!(!summary.trim().is_empty(), "empty context summary");
        self.items = vec![json!({"role":"user", "content":summary})];
        Ok(())
    }
    pub fn unanswered(&self) -> BTreeSet<String> {
        let mut pending = BTreeSet::new();
        for item in &self.items {
            match item["type"].as_str() {
                Some("function_call" | "custom_tool_call") => {
                    pending.insert(item["call_id"].as_str().unwrap_or("").to_owned());
                }
                Some("function_call_output" | "custom_tool_call_output") => {
                    pending.remove(item["call_id"].as_str().unwrap_or(""));
                }
                _ => (),
            }
        }
        pending
    }
}
