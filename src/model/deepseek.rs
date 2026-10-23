use super::StreamDecoder;
use super::{ModelEvent, Provider, Request, RequestFailure, Response};
use futures::StreamExt;
use serde_json::json;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[cfg(test)]
mod tests;

/// Total attempts per request: the initial send plus retries on HTTP 429.
const RETRY_BUDGET: u32 = 3;
/// Fallback backoff when the provider sends no usable Retry-After: 2^attempt seconds.
const FALLBACK_RETRY_BASE: Duration = Duration::from_secs(1);
/// Longest wait between retries, even if Retry-After asks for more.
const MAX_RETRY_DELAY: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Covers the entire response including the streaming phase, not just the handshake: a slow
/// but progressing stream is still bounded, so a stuck provider cannot hold a token
/// reservation forever.
const TOTAL_TIMEOUT: Duration = Duration::from_secs(600);
/// Provider error bodies are truncated to this many characters in error messages.
const ERROR_BODY_CHARS: usize = 512;

pub struct DeepSeek {
    base_url: String,
    pub model: String,
    key: String,
    client: reqwest::Client,
    official: bool,
}
impl DeepSeek {
    pub fn new(base_url: String, model: String, key: String) -> anyhow::Result<Self> {
        let url = reqwest::Url::parse(&base_url)?;
        anyhow::ensure!(
            url.scheme() == "https"
                || matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]")),
            "API credentials require HTTPS outside localhost"
        );
        // A query or fragment would be silently dropped when "{base}/responses" is formatted,
        // which usually means a misconfigured proxy URL; reject it instead of silently
        // calling an endpoint the operator did not write down.
        anyhow::ensure!(
            url.query().is_none() && url.fragment().is_none(),
            "base URL must not contain a query string or fragment"
        );
        anyhow::ensure!(!key.trim().is_empty(), "DEEPSEEK_API_KEY is missing");
        let client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(TOTAL_TIMEOUT)
            // Following a POST redirect could submit before a later connection fails.
            // Keep one transport attempt per reservation except proven free rejections.
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .build()?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').into(),
            model,
            key,
            client,
            official: url.scheme() == "https"
                && url.host_str() == Some("api.deepseek.com")
                && url.port_or_known_default() == Some(443)
                && url.username().is_empty()
                && url.password().is_none()
                && matches!(url.path(), "" | "/" | "/v1" | "/v1/"),
        })
    }
}
#[async_trait::async_trait]
impl Provider for DeepSeek {
    async fn respond(
        &self,
        request: Request,
        cancel: CancellationToken,
        events: mpsc::UnboundedSender<ModelEvent>,
    ) -> anyhow::Result<Response> {
        let body = json!({
            "model": self.model,
            "instructions": request.instructions,
            "input": request.input,
            "tools": request.tools,
            "stream": true,
            // The approved Flash configuration uses high reasoning effort. Its quality
            // impact must be measured by the frozen evaluation, not assumed here.
            "reasoning": {"effort": "high"},
            "max_output_tokens": request.max_output,
        });
        let mut attempt = 0;
        loop {
            if cancel.is_cancelled() {
                return Err(RequestFailure::NotSent("cancelled before sending".into()).into());
            }
            let send = self
                .client
                .post(format!("{}/responses", self.base_url))
                .bearer_auth(&self.key)
                .json(&body)
                .send();
            let response = tokio::select! {
                biased;
                _ = cancel.cancelled() => anyhow::bail!("model request cancelled"),
                response = send => match response {
                    Ok(response) => response,
                    Err(error) if error.is_connect() || error.is_builder() => {
                        return Err(RequestFailure::NotSent(error.to_string()).into());
                    }
                    Err(error) => return Err(error.into()),
                },
            };
            // A 5xx or an unclassified proxy error may follow billable inference.
            // Return those to the runner so any retry needs a NEW reservation.
            let status = response.status();
            if !status.is_success() {
                // No usable Retry-After: fall back to exponential backoff (1s, 2s, 4s, ...).
                // A malformed header is treated the same as an absent one — it is a hint,
                // not a contract.
                let delay = response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .map(Duration::from_secs)
                    .unwrap_or(FALLBACK_RETRY_BASE * 2u32.pow(attempt))
                    .min(MAX_RETRY_DELAY);
                let body = tokio::select! {
                    _ = cancel.cancelled() => anyhow::bail!("model request cancelled"),
                    body = response.bytes() => body?,
                };
                let rejection = self.official && documented_rejection(status.as_u16(), &body);
                // Providers may echo request headers. Redact before truncation because
                // this error can travel to durable logs and the terminal.
                let detail: String = String::from_utf8_lossy(&body)
                    .replace(&self.key, "[REDACTED]")
                    .chars()
                    .take(ERROR_BODY_CHARS)
                    .collect();
                let failure = if rejection {
                    RequestFailure::Rejected {
                        status: status.as_u16(),
                        detail,
                    }
                } else {
                    RequestFailure::Http {
                        status: status.as_u16(),
                        detail,
                    }
                };
                if !rejection
                    || status != reqwest::StatusCode::TOO_MANY_REQUESTS
                    || attempt + 1 >= RETRY_BUDGET
                {
                    return Err(failure.into());
                }
                // The receiver may already be gone (caller dropped the channel); retry
                // notifications are informational, so the send result is discarded on purpose.
                let _ = events.send(ModelEvent::Retry(attempt + 1));
                tokio::select! {
                    _ = cancel.cancelled() => return Err(RequestFailure::NotSent(
                        "cancelled before retry; earlier attempts were rejected".into()
                    ).into()),
                    _ = tokio::time::sleep(delay) => (),
                }
                attempt += 1;
                continue;
            }
            let mut stream = response.bytes_stream();
            let mut decoder = StreamDecoder::default();
            loop {
                let chunk = tokio::select! {
                    biased;
                    _ = cancel.cancelled() => anyhow::bail!("model request cancelled"),
                    chunk = stream.next() => chunk,
                };
                let Some(chunk) = chunk else { break };
                for event in decoder.push(&chunk?)? {
                    let _ = events.send(event);
                }
            }
            return decoder.finish();
        }
    }
}

fn documented_rejection(status: u16, body: &[u8]) -> bool {
    // DeepSeek documents these statuses as request validation/authentication,
    // balance or admission failures. It bills model input/output tokens. This
    // deliberately does not claim that every 4xx (or every proxy error) is free.
    // https://api-docs.deepseek.com/quick_start/error_codes/
    // https://api-docs.deepseek.com/quick_start/pricing/
    if !matches!(status, 400 | 401 | 402 | 422 | 429) {
        return false;
    }
    let Ok(body) = serde_json::from_slice::<serde_json::Value>(body) else {
        return false;
    };
    body["error"]["message"]
        .as_str()
        .is_some_and(|message| !message.is_empty())
        && body["usage"].is_null()
        && body["response"].is_null()
        && body["output"].is_null()
}
