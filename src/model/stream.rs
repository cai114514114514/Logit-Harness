use super::{ModelEvent, Response};
use serde_json::Value;

/// One SSE frame may never exceed this size; the buffer holds at most one partial frame,
/// so this also bounds the total memory a runaway provider can make us hold.
const MAX_BUFFER_BYTES: usize = 16 * 1024 * 1024;

#[derive(Default)]
pub struct StreamDecoder {
    buffer: Vec<u8>,
    terminal: Option<Response>,
}
impl StreamDecoder {
    /// Keep bytes until a complete SSE frame exists: UTF-8 characters can span network chunks.
    ///
    /// Error atomicity: each frame is fully validated before it mutates `terminal`, so a
    /// failed frame leaves earlier events and terminal state committed. This is safe because
    /// every caller abandons the stream on the first error; reusing the decoder after an
    /// error would observe that partial state.
    pub fn push(&mut self, bytes: &[u8]) -> anyhow::Result<Vec<ModelEvent>> {
        self.buffer.extend_from_slice(bytes);
        // The buffer is deliberately not cleared on overflow: this decoder is poisoned and
        // must not be reused, since it is unknowable which bytes belong to the oversized frame.
        anyhow::ensure!(
            self.buffer.len() <= MAX_BUFFER_BYTES,
            "provider frame exceeds 16 MiB"
        );
        let mut events = vec![];
        while let Some((end, width)) = find_frame_end(&self.buffer) {
            // `drain(..).collect()` copies the frame out instead of parsing in place; frames
            // are tiny next to the cap, and the copy buys a simple immutable parse.
            // Strict UTF-8: one invalid byte fails the whole stream rather than lossy-decoding,
            // because a shifted frame boundary could smuggle malformed JSON past validation.
            let frame = String::from_utf8(self.buffer.drain(..end + width).collect())?;
            let Some(value) = parse_frame(&frame)? else {
                continue;
            };
            match value["type"].as_str().unwrap_or("") {
                // A missing "delta" is pushed as an empty event rather than skipped: display
                // consumers treat deltas as pure appends, so an empty chunk is a harmless
                // no-op, while skipping would hide provider shape drift.
                "response.output_text.delta" => events.push(ModelEvent::Text(
                    value["delta"].as_str().unwrap_or("").into(),
                )),
                "response.reasoning_text.delta" => events.push(ModelEvent::Reasoning(
                    value["delta"].as_str().unwrap_or("").into(),
                )),
                "response.completed" => {
                    let terminal = build_terminal(&value["response"])
                        .map_err(|error| super::failure::reported(&value["response"], error))?;
                    anyhow::ensure!(self.terminal.is_none(), "duplicate terminal response");
                    self.terminal = Some(terminal);
                }
                "response.failed" | "response.incomplete" | "error" => {
                    return Err(super::failure::reported(
                        &value["response"],
                        anyhow::anyhow!("provider response did not complete: {}", value["type"]),
                    ));
                }
                _ => (), // Tool deltas are display-only until the terminal response validates them.
            }
        }
        Ok(events)
    }
    pub fn finish(self) -> anyhow::Result<Response> {
        // A provider that drops the connection right after the last `data:` line leaves a
        // partial frame with no trailing separator; error instead of silently discarding it,
        // since that frame could have been the terminal response.
        anyhow::ensure!(
            self.buffer.is_empty(),
            "stream ended with an unterminated trailing frame"
        );
        self.terminal
            .ok_or_else(|| anyhow::anyhow!("response ended without a complete terminal event"))
    }
}

/// Find the first SSE frame separator, accepting both `\n\n` and `\r\n\r\n` line styles.
/// Returns the separator's offset and width; the earliest separator wins when both occur.
/// The two patterns can never overlap (`\r\n\r\n` contains no `\n\n` run), so comparing
/// start offsets alone is unambiguous.
fn find_frame_end(buffer: &[u8]) -> Option<(usize, usize)> {
    buffer
        .windows(2)
        .position(|w| w == b"\n\n")
        .map(|p| (p, 2))
        .into_iter()
        .chain(
            buffer
                .windows(4)
                .position(|w| w == b"\r\n\r\n")
                .map(|p| (p, 4)),
        )
        .min_by_key(|(p, _)| *p)
}

/// Extract the payload of one SSE frame: its `data:` lines joined with `\n`, per the SSE
/// spec. Returns `None` for comment/heartbeat frames that carry no data.
fn parse_frame(frame: &str) -> anyhow::Result<Option<Value>> {
    let data = frame
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(str::trim_start)
        .collect::<Vec<_>>()
        .join("\n");
    if data.is_empty() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_str(&data)?))
}

/// Validate a `response.completed` payload into an executable answer. Every field that
/// downstream accounting depends on (status, output, usage, model) is mandatory; the
/// response is not accepted until its tool calls also pass `Response::calls` validation.
fn build_terminal(response: &Value) -> anyhow::Result<Response> {
    anyhow::ensure!(
        response["status"] == "completed",
        "inconsistent terminal status"
    );
    let output = response["output"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("missing response output"))?
        .clone();
    let result = Response {
        output,
        model: response["model"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing response model"))?
            .into(),
        usage: super::failure::usage(response)?,
    };
    result.calls()?;
    Ok(result)
}
