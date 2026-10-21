use crate::domain::Usage;
use serde_json::Value;

/// A terminal response can be billable without being safe to execute. Keep those two facts
/// separate so truncation or malformed tool arguments do not erase authoritative usage.
#[derive(Debug, thiserror::Error)]
#[error("{reason}")]
pub struct ReportedFailure {
    pub reason: String,
    pub usage: Usage,
    pub response: Value,
}

pub(super) fn usage(response: &Value) -> anyhow::Result<Usage> {
    let input = response["usage"]["input_tokens"]
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("missing input usage"))?;
    let output = response["usage"]["output_tokens"]
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("missing output usage"))?;
    let cached = response["usage"]["input_tokens_details"]["cached_tokens"]
        .as_u64()
        .unwrap_or(0);
    anyhow::ensure!(cached <= input, "cached usage exceeds total input usage");
    Ok(Usage {
        input,
        output,
        cached,
    })
}

pub(super) fn reported(response: &Value, error: anyhow::Error) -> anyhow::Error {
    match usage(response) {
        Ok(usage) => ReportedFailure {
            reason: error.to_string(),
            usage,
            response: response.clone(),
        }
        .into(),
        Err(_) => error,
    }
}
