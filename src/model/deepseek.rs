use super::StreamDecoder;
use super::{ModelEvent, Provider, Request, Response};
use futures::StreamExt;
use serde_json::json;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub struct DeepSeek {
    pub base_url: String,
    pub model: String,
    key: String,
    client: reqwest::Client,
}
impl DeepSeek {
    pub fn new(base_url: String, model: String, key: String) -> anyhow::Result<Self> {
        let url = reqwest::Url::parse(&base_url)?;
        anyhow::ensure!(
            url.scheme() == "https"
                || matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]")),
            "API credentials require HTTPS outside localhost"
        );
        anyhow::ensure!(!key.trim().is_empty(), "DEEPSEEK_API_KEY is missing");
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(600))
            .build()?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').into(),
            model,
            key,
            client,
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
        let body = json!({"model": self.model, "instructions":request.instructions,
            "input":request.input, "tools":request.tools, "stream":true,
            "reasoning":{"effort":"high"}, "max_output_tokens":request.max_output});
        for attempt in 0..3_u32 {
            let send = self
                .client
                .post(format!("{}/responses", self.base_url))
                .bearer_auth(&self.key)
                .json(&body)
                .send();
            let response = tokio::select! {
                biased;
                _ = cancel.cancelled() => anyhow::bail!("model request cancelled"),
                response = send => response?,
            };
            if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS && attempt < 2 {
                let delay = response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(1 << attempt)
                    .min(30);
                let _ = events.send(ModelEvent::Retry(attempt + 1));
                tokio::select! {
                    _ = cancel.cancelled() => anyhow::bail!("model request cancelled"),
                    _ = tokio::time::sleep(Duration::from_secs(delay)) => (),
                }
                continue;
            }
            // A 5xx may follow billable inference. Let the metered runner retry as a NEW
            // reservation instead of hiding multiple uncertain calls behind one allowance.
            anyhow::ensure!(
                response.status().is_success(),
                "provider HTTP {}",
                response.status()
            );
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
        anyhow::bail!("provider rate limit retry budget exhausted")
    }
}
