use super::{ModelEvent, Response};
use crate::domain::Usage;
use serde_json::Value;

#[derive(Default)]
pub struct StreamDecoder {
    buffer: Vec<u8>,
    terminal: Option<Response>,
}
impl StreamDecoder {
    /// Keep bytes until a complete SSE frame exists: UTF-8 characters can span network chunks.
    pub fn push(&mut self, bytes: &[u8]) -> anyhow::Result<Vec<ModelEvent>> {
        self.buffer.extend_from_slice(bytes);
        anyhow::ensure!(
            self.buffer.len() <= 16 * 1024 * 1024,
            "provider frame exceeds 16 MiB"
        );
        let mut events = vec![];
        loop {
            let separator = self
                .buffer
                .windows(2)
                .position(|w| w == b"\n\n")
                .map(|p| (p, 2))
                .into_iter()
                .chain(
                    self.buffer
                        .windows(4)
                        .position(|w| w == b"\r\n\r\n")
                        .map(|p| (p, 4)),
                )
                .min_by_key(|(p, _)| *p);
            let Some((end, width)) = separator else { break };
            let frame = String::from_utf8(self.buffer.drain(..end + width).collect())?;
            let data = frame
                .lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .map(str::trim_start)
                .collect::<Vec<_>>()
                .join("\n");
            if data.is_empty() {
                continue;
            }
            let value: Value = serde_json::from_str(&data)?;
            match value["type"].as_str().unwrap_or("") {
                "response.output_text.delta" => events.push(ModelEvent::Text(
                    value["delta"].as_str().unwrap_or("").into(),
                )),
                "response.reasoning_text.delta" => events.push(ModelEvent::Reasoning(
                    value["delta"].as_str().unwrap_or("").into(),
                )),
                "response.completed" => {
                    let response = &value["response"];
                    anyhow::ensure!(
                        response["status"] == "completed",
                        "inconsistent terminal status"
                    );
                    let output = response["output"]
                        .as_array()
                        .ok_or_else(|| anyhow::anyhow!("missing response output"))?
                        .clone();
                    let input = response["usage"]["input_tokens"]
                        .as_u64()
                        .ok_or_else(|| anyhow::anyhow!("missing input usage"))?;
                    let output_tokens = response["usage"]["output_tokens"]
                        .as_u64()
                        .ok_or_else(|| anyhow::anyhow!("missing output usage"))?;
                    let result = Response {
                        output,
                        model: response["model"].as_str().unwrap_or("").into(),
                        usage: Usage {
                            input,
                            output: output_tokens,
                            cached: response["usage"]["input_tokens_details"]["cached_tokens"]
                                .as_u64()
                                .unwrap_or(0),
                        },
                    };
                    result.calls()?;
                    anyhow::ensure!(self.terminal.is_none(), "duplicate terminal response");
                    self.terminal = Some(result);
                }
                "response.failed" | "response.incomplete" | "error" => {
                    anyhow::bail!("provider response did not complete: {}", value["type"]);
                }
                _ => (), // Tool deltas are display-only until the terminal response validates them.
            }
        }
        Ok(events)
    }
    pub fn finish(self) -> anyhow::Result<Response> {
        self.terminal
            .ok_or_else(|| anyhow::anyhow!("response ended without a complete terminal event"))
    }
}
