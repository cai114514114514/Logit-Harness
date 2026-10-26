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

/// Only the transport can establish whether inference was submitted or rejected.
/// An arbitrary HTTP failure is not evidence of zero usage, especially via a proxy.
#[derive(Debug, thiserror::Error)]
pub enum RequestFailure {
    #[error("model request was not submitted: {0}")]
    NotSent(String),
    #[error("provider HTTP {status}: {detail}")]
    Rejected { status: u16, detail: String },
    #[error("provider HTTP {status}: {detail}")]
    Http { status: u16, detail: String },
}

impl RequestFailure {
    pub fn retryable(&self) -> bool {
        match self {
            Self::NotSent(_) => true,
            // Documented rate limits already exhausted the transport's backoff budget.
            Self::Rejected { .. } => false,
            Self::Http { status, .. } => matches!(status, 408 | 429 | 500..=599),
        }
    }

    /// The request exceeded the model's context window. Unlike other 4xx
    /// rejections the harness can repair this itself by compacting and retrying.
    pub fn context_overflow(&self) -> bool {
        let (Self::Rejected { status, detail } | Self::Http { status, detail }) = self else {
            return false;
        };
        let detail = detail.to_lowercase();
        matches!(status, 400 | 413)
            && (detail.contains("maximum context length")
                || detail.contains("context_length_exceeded")
                || detail.contains("context length"))
    }

    pub fn advice(&self) -> &'static str {
        let status = match self {
            Self::NotSent(_) => return "请检查 API 地址与网络连接；可用 /setup 修改地址。",
            Self::Rejected { status, .. } | Self::Http { status, .. } => *status,
        };
        match status {
            400 | 422 => "请根据错误详情修正请求格式或参数，再用 /resume 继续。",
            401 => {
                "请用 /setup 检查 API key。同地址密钥更新后先 /stop，待已停止再 /resume 重新加载；环境变量密钥需重新启动 CLI。"
            }
            402 => "请检查供应商账户余额，处理后用 /resume 继续。",
            403 => "请检查当前 API 地址的访问权限，处理后用 /resume 继续。",
            404 => {
                "请用 /setup 检查 API 地址与 /responses 接口。更换地址后用 /new；当前会话保留原地址，服务修复后可 /resume。"
            }
            429 => "请求已达到速率或并发上限，请稍后用 /resume 继续。",
            500..=599 => "供应商服务暂时失败，请稍后重试。",
            _ => "请根据错误详情检查 API 配置，处理后用 /resume 继续。",
        }
    }
}

/// Money and session-token accounting must consume the same provider evidence.
/// None means the request may have been billable, never that it was free.
pub fn failure_usage(error: &anyhow::Error) -> Option<Usage> {
    if let Some(failure) = error.downcast_ref::<ReportedFailure>() {
        return Some(failure.usage.clone());
    }
    match error.downcast_ref::<RequestFailure>()? {
        RequestFailure::NotSent(_) | RequestFailure::Rejected { .. } => Some(Usage::default()),
        RequestFailure::Http { .. } => None,
    }
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
