use super::{McpClient, Server, Transport, journal};
use crate::tools::Effect;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

struct Connection {
    config: Server,
    // A single server is serialized: cancelling a local call terminates its process group.
    client: Mutex<Option<Arc<McpClient>>>,
    directory: PathBuf,
    enabled: AtomicBool,
}

pub struct Registry {
    workspace: PathBuf,
    servers: BTreeMap<String, Connection>,
}

impl Registry {
    pub fn new(config: BTreeMap<String, Server>, workspace: PathBuf, logs: PathBuf) -> Self {
        Self {
            workspace,
            servers: config
                .into_iter()
                .map(|(name, config)| {
                    let directory = logs.join(blake3::hash(name.as_bytes()).to_hex().as_str());
                    (
                        name,
                        Connection {
                            config,
                            client: Mutex::new(None),
                            directory,
                            enabled: AtomicBool::new(true),
                        },
                    )
                })
                .collect(),
        }
    }

    pub fn names(&self) -> Vec<&str> {
        self.servers.keys().map(String::as_str).collect()
    }
    fn server(&self, name: &str) -> anyhow::Result<&Connection> {
        self.servers
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("unknown MCP server: {name}"))
    }
    pub fn effect(&self, server: &str, tool: &str) -> Effect {
        self.servers
            .get(server)
            .map(|entry| entry.config.effect(tool))
            .unwrap_or(Effect::Unknown)
    }

    async fn connect(
        &self,
        server: &Connection,
        client: &mut Option<Arc<McpClient>>,
    ) -> anyhow::Result<Arc<McpClient>> {
        if client.as_ref().is_none_or(|client| client.is_closed()) {
            *client = Some(Arc::new(
                McpClient::connect(&server.config, &self.workspace, &server.directory).await?,
            ));
        }
        Ok(client.as_ref().unwrap().clone())
    }

    pub async fn catalog(&self, name: &str, cancel: &CancellationToken) -> anyhow::Result<Value> {
        let server = self.server(name)?;
        let mut slot = tokio::select! { biased; _ = cancel.cancelled() => anyhow::bail!("MCP discovery cancelled"), slot = server.client.lock() => slot };
        anyhow::ensure!(
            server.enabled.load(Ordering::SeqCst),
            "MCP server is disabled: {name}"
        );
        let client = self.connect(server, &mut slot).await?;
        anyhow::ensure!(!cancel.is_cancelled(), "MCP discovery cancelled");
        let mut catalog = serde_json::to_value(client.catalog())?;
        if let Some(tools) = catalog.as_array_mut() {
            tools.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
        }
        Ok(catalog)
    }

    pub async fn call(
        &self,
        name: &str,
        tool: &str,
        arguments: Value,
        cancel: CancellationToken,
    ) -> anyhow::Result<super::client::CallResult> {
        let server = self.server(name)?;
        let mut slot = tokio::select! { biased; _ = cancel.cancelled() => anyhow::bail!("MCP call cancelled before ownership"), slot = server.client.lock() => slot };
        anyhow::ensure!(
            server.enabled.load(Ordering::SeqCst),
            "MCP server is disabled: {name}"
        );
        let pending = server.directory.join("pending.json");
        let effect = server.config.effect(tool);
        let mutation = !effect.readonly();
        if mutation && pending.exists() {
            anyhow::bail!(
                "MCP mutation has an uncertain outcome. Inspect remote state with read tools, then use mcp_resolve with explicit user authorization. Pending record: {}",
                pending.display()
            );
        }
        let client = self.connect(server, &mut slot).await?;
        anyhow::ensure!(
            !cancel.is_cancelled(),
            "MCP call cancelled before submission"
        );
        if mutation {
            journal::save(
                &pending,
                &json!({"server":name,"tool":tool,"arguments":arguments,"effect":effect}),
            )?;
        }
        let result = client.call(tool, arguments, cancel).await;
        let complete = result.as_ref().is_ok_and(|result| result.complete);
        if !complete && matches!(server.config.transport, Transport::Stdio { .. }) {
            // No replacement writer may run while a cancelled/failed server can still write.
            client.close().await?;
            if mutation && !matches!(effect, Effect::External | Effect::Unknown) {
                journal::remove(&pending)?;
            }
        } else if mutation && complete {
            journal::remove(&pending)?;
        }
        result
    }

    pub async fn pending(&self, name: &str) -> anyhow::Result<Value> {
        let server = self.server(name)?;
        let _owner = server.client.lock().await;
        let path = server.directory.join("pending.json");
        if !path.exists() {
            return Ok(Value::Null);
        }
        let bytes = std::fs::read(path)?;
        Ok(
            json!({"record":serde_json::from_slice::<Value>(&bytes)?,"hash":blake3::hash(&bytes).to_hex().to_string()}),
        )
    }

    /// The per-server mutex is the settlement boundary shared with call/catalog.
    /// A reconnect or disable never deletes uncertain mutation journals.
    pub async fn control(&self, name: &str, operation: super::Operation) -> anyhow::Result<Value> {
        let server = self.server(name)?;
        let mut slot = server.client.lock().await;
        if let Some(client) = slot.take() {
            client.close().await?;
        }
        match operation {
            super::Operation::Disable => server.enabled.store(false, Ordering::SeqCst),
            super::Operation::Enable => server.enabled.store(true, Ordering::SeqCst),
            super::Operation::Reconnect => {
                anyhow::ensure!(
                    server.enabled.load(Ordering::SeqCst),
                    "MCP server is disabled; enable it first"
                );
                self.connect(server, &mut slot).await?;
            }
        }
        Ok(
            json!({"server":name,"operation":operation,"enabled":server.enabled.load(Ordering::SeqCst),"uncertain_mutation":server.directory.join("pending.json").exists()}),
        )
    }

    /// Only registered as an external-effect tool: the user must approve this exact reconciliation.
    pub async fn resolve(&self, name: &str, hash: &str, evidence: &str) -> anyhow::Result<()> {
        anyhow::ensure!(
            !evidence.trim().is_empty(),
            "reconciliation requires actual evidence"
        );
        let server = self.server(name)?;
        let _owner = server.client.lock().await;
        let path = server.directory.join("pending.json");
        let bytes = std::fs::read(&path)?;
        anyhow::ensure!(
            blake3::hash(&bytes).to_hex().as_str() == hash,
            "pending operation changed"
        );
        journal::save(
            &server
                .directory
                .join(format!("{}.resolved.json", uuid::Uuid::new_v4())),
            &json!({"pending":serde_json::from_slice::<Value>(&bytes)?,"evidence":evidence}),
        )?;
        journal::remove(&path)
    }

    pub async fn recover(&self) -> anyhow::Result<()> {
        for server in self.servers.values() {
            McpClient::recover(&server.directory).await?;
            if matches!(server.config.transport, Transport::Stdio { .. }) {
                let path = server.directory.join("pending.json");
                if path.exists() {
                    let record: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
                    let effect: Effect =
                        serde_json::from_value(record["effect"].clone()).unwrap_or(Effect::Unknown);
                    // Transport locality proves nothing about remote side effects.
                    // Missing provenance is conservatively unknown, even if config changed.
                    if !matches!(effect, Effect::External | Effect::Unknown) {
                        journal::remove(&path)?;
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn close(&self) -> anyhow::Result<()> {
        for server in self.servers.values() {
            if let Some(client) = server.client.lock().await.as_ref() {
                client.close().await?;
            }
        }
        Ok(())
    }
}
