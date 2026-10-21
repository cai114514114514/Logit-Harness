use super::{Server, Transport, journal};
use crate::tools::identity::Identity;
use rmcp::{
    Peer, RoleClient,
    model::{CallToolRequestParams, CallToolResponse, ProtocolVersion, Tool},
    service::{ClientLifecycleMode, RunningService, serve_client_with_lifecycle},
    transport::{
        StreamableHttpClientTransport, TokioChildProcess,
        streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

pub struct McpClient {
    peer: Peer<RoleClient>,
    service: Mutex<RunningService<RoleClient, ()>>,
    catalog: Vec<Tool>,
    identity: Option<Identity>,
    journal: PathBuf,
    closed: AtomicBool,
}

#[derive(serde::Serialize)]
pub struct CallResult {
    pub data: Value,
    pub complete: bool,
    pub success: bool,
}

impl CallResult {
    fn from_response(response: CallToolResponse) -> anyhow::Result<Self> {
        Ok(match response {
            CallToolResponse::Complete(result) => Self {
                success: !result.is_error.unwrap_or(false),
                data: serde_json::to_value(result)?,
                complete: true,
            },
            CallToolResponse::InputRequired(result) => Self {
                data: serde_json::to_value(result)?,
                complete: false,
                success: false,
            },
            CallToolResponse::Task(result) => Self {
                data: serde_json::to_value(result)?,
                complete: false,
                success: false,
            },
            _ => anyhow::bail!(
                "unsupported MCP continuation; inspect server state before proceeding"
            ),
        })
    }
}

impl McpClient {
    pub async fn connect(server: &Server, workspace: &Path, logs: &Path) -> anyhow::Result<Self> {
        std::fs::create_dir_all(logs)?;
        let journal = logs.join("process.json");
        // The caller holds the workspace owner lock before recovering extension processes.
        Self::recover(logs).await?;
        let mut identity = None;
        let service: RunningService<RoleClient, ()> = match &server.transport {
            Transport::Stdio {
                command,
                args,
                environment,
            } => {
                let mut child = tokio::process::Command::new(command);
                child
                    .args(args)
                    .current_dir(workspace)
                    .process_group(0)
                    .env_clear();
                // Pass ordinary process setup plus an explicit allowlist of secrets.
                for name in ["PATH", "HOME", "TMPDIR", "LANG", "LC_ALL"]
                    .into_iter()
                    .chain(environment.iter().map(String::as_str))
                {
                    if let Some(value) = std::env::var_os(name) {
                        child.env(name, value);
                    }
                }
                let stderr = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(logs.join("stderr.log"))?;
                let (transport, _) = TokioChildProcess::builder(child).stderr(stderr).spawn()?;
                identity = transport.id().map(Identity::capture).transpose()?.flatten();
                let owned = identity
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("MCP server exited during startup"))?;
                journal::save(&journal, owned)?;
                match tokio::time::timeout(
                    Duration::from_secs(30),
                    serve_client_with_lifecycle((), transport, lifecycle()),
                )
                .await
                {
                    Ok(Ok(service)) => service,
                    result => {
                        owned.stop_group().await?;
                        anyhow::bail!("MCP stdio discovery failed: {result:?}");
                    }
                }
            }
            Transport::Http { url, headers } => {
                let parsed = reqwest::Url::parse(url)?;
                anyhow::ensure!(
                    parsed.scheme() == "https"
                        || (parsed.scheme() == "http"
                            && matches!(
                                parsed.host_str(),
                                Some("127.0.0.1" | "localhost" | "[::1]")
                            )),
                    "MCP HTTP requires HTTPS except on loopback"
                );
                anyhow::ensure!(
                    parsed.username().is_empty() && parsed.password().is_none(),
                    "MCP URL cannot contain credentials"
                );
                let mut settings = StreamableHttpClientTransportConfig::with_uri(url.clone());
                // Never automatically retry an expired-session mutation.
                settings.reinit_on_expired_session = false;
                for (name, variable) in headers {
                    let secret = std::env::var(variable).map_err(|_| {
                        anyhow::anyhow!("missing MCP header environment variable: {variable}")
                    })?;
                    let mut value = reqwest::header::HeaderValue::from_str(&secret)?;
                    value.set_sensitive(true);
                    settings.custom_headers.insert(name.parse()?, value);
                }
                let http = reqwest::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .connect_timeout(Duration::from_secs(15))
                    .build()?;
                let transport = StreamableHttpClientTransport::with_client(http, settings);
                tokio::time::timeout(
                    Duration::from_secs(30),
                    serve_client_with_lifecycle((), transport, lifecycle()),
                )
                .await??
            }
        };
        let catalog =
            match tokio::time::timeout(Duration::from_secs(30), service.list_all_tools()).await {
                Ok(Ok(catalog)) => catalog,
                result => {
                    if let Some(owned) = &identity {
                        owned.stop_group().await?;
                    }
                    anyhow::bail!("MCP tool discovery failed: {result:?}");
                }
            };
        Ok(Self {
            peer: service.peer().clone(),
            service: Mutex::new(service),
            catalog,
            identity,
            journal,
            closed: AtomicBool::new(false),
        })
    }

    pub fn catalog(&self) -> &[Tool] {
        &self.catalog
    }
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    pub async fn call(
        &self,
        name: &str,
        arguments: Value,
        cancel: CancellationToken,
    ) -> anyhow::Result<CallResult> {
        anyhow::ensure!(!self.is_closed(), "MCP connection closed");
        anyhow::ensure!(
            self.catalog.iter().any(|tool| tool.name == name),
            "unknown MCP tool: {name}"
        );
        anyhow::ensure!(
            !cancel.is_cancelled(),
            "MCP call cancelled before submission"
        );
        let params = CallToolRequestParams::new(name.to_owned()).with_arguments(
            arguments
                .as_object()
                .ok_or_else(|| anyhow::anyhow!("MCP arguments must be an object"))?
                .clone(),
        );
        tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                // The SDK sends a cancellation notification when the request future is dropped.
                // For local servers we additionally wait for every owned process to exit.
                self.close().await?;
                anyhow::bail!("MCP call cancelled; inspect current state before a replacement");
            }
            response = self.peer.call_tool_once(params) => CallResult::from_response(response?),
        }
    }

    pub async fn close(&self) -> anyhow::Result<()> {
        self.closed.store(true, Ordering::SeqCst);
        if let Some(identity) = &self.identity {
            identity.stop_group().await?;
        }
        self.service.lock().await.close().await?;
        journal::remove(&self.journal)?;
        Ok(())
    }

    pub async fn recover(logs: &Path) -> anyhow::Result<()> {
        let path = logs.join("process.json");
        if path.exists() {
            let identity: Identity = serde_json::from_slice(&std::fs::read(&path)?)?;
            identity.stop_group().await?;
            journal::remove(&path)?;
        }
        Ok(())
    }
}

fn lifecycle() -> ClientLifecycleMode {
    ClientLifecycleMode::Auto {
        preferred_versions: vec![ProtocolVersion::LATEST],
        legacy_version: Some(ProtocolVersion::V_2025_11_25),
    }
}
