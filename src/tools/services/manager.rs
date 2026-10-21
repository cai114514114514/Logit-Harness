use crate::{
    domain::Goal,
    tools::{
        Scope,
        files::Workspace,
        identity::Identity,
        process::{self, ProcessEvent, ProcessOutput, ProcessRecord, ProcessSpec},
    },
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf, sync::Mutex, time::Duration};
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServiceInfo {
    pub id: String,
    pub pid: u32,
    pub command: String,
    pub url: String,
    pub output: PathBuf,
    pub log: PathBuf,
}

#[derive(Clone)]
struct Managed {
    info: ServiceInfo,
    scope: Scope,
    cancel: CancellationToken,
    done: watch::Receiver<Option<Result<ProcessOutput, String>>>,
}
#[derive(Serialize, Deserialize)]
struct Record {
    owner: Identity,
    scope: Scope,
    info: ServiceInfo,
}

pub struct Services {
    workspace: Workspace,
    logs: PathBuf,
    entries: Mutex<BTreeMap<String, Managed>>,
}
impl Services {
    pub fn new(workspace: Workspace, logs: PathBuf) -> Self {
        Self {
            workspace,
            logs,
            entries: Mutex::new(BTreeMap::new()),
        }
    }

    pub async fn start(
        &self,
        command: &str,
        ready: &str,
        scope: Scope,
        cancel: CancellationToken,
    ) -> anyhow::Result<ServiceInfo> {
        let url = reqwest::Url::parse(ready)?;
        anyhow::ensure!(
            url.scheme() == "http"
                && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")),
            "service readiness must use loopback HTTP"
        );
        let address = (
            url.host_str().unwrap().trim_matches(['[', ']']),
            url.port_or_known_default().unwrap(),
        );
        anyhow::ensure!(
            tokio::net::TcpStream::connect(address).await.is_err(),
            "readiness port is already occupied; choose a free port"
        );
        anyhow::ensure!(!cancel.is_cancelled(), "service launch cancelled");
        std::fs::create_dir_all(&self.logs)?;
        let id = uuid::Uuid::new_v4().simple().to_string();
        let output = self.workspace.resolve(&format!(".weft-build/{id}"))?;
        std::fs::create_dir_all(&output)?;
        let log = self.logs.join(format!("{id}.log"));
        let token = CancellationToken::new();
        let launch_guard = token.clone().drop_guard();
        let spec = ProcessSpec {
            command: format!(
                "export WEFT_OUTPUT_DIR={}; {command}",
                quote(&output.to_string_lossy())
            ),
            cwd: self.workspace.root.clone(),
            log: log.clone(),
            timeout: Duration::from_secs(21600),
            profile: Some(super::super::service::profile(
                &self.workspace.root,
                std::slice::from_ref(&output),
            )?),
        };
        let (events, mut receiver) = mpsc::unbounded_channel();
        let (done_send, mut done) = watch::channel(None);
        let process_cancel = token.clone();
        tokio::spawn(async move {
            let result = process::run(spec, process_cancel, events)
                .await
                .map_err(|e| e.to_string());
            let _ = done_send.send(Some(result));
        });
        let pid = tokio::select! {
            _ = cancel.cancelled() => { token.cancel(); wait_exit(&mut done).await?; anyhow::bail!("service launch cancelled") },
            started = receiver.recv() => match started {
                Some(ProcessEvent::Started { pid }) => pid,
                _ => { token.cancel(); wait_exit(&mut done).await?; anyhow::bail!("service exited before starting") }
            }
        };
        // Process output has its own durable log. The foreground tool's event
        // channel must not stay open for the server's entire lifetime.
        tokio::spawn(async move { while receiver.recv().await.is_some() {} });
        let info = ServiceInfo {
            id: id.clone(),
            pid,
            command: command.into(),
            url: ready.into(),
            output,
            log,
        };
        self.entries
            .lock()
            .map_err(|_| anyhow::anyhow!("service registry poisoned"))?
            .insert(
                id.clone(),
                Managed {
                    info: info.clone(),
                    scope: scope.clone(),
                    cancel: token,
                    done: done.clone(),
                },
            );
        let record = Record {
            owner: Identity::capture(std::process::id())?
                .ok_or_else(|| anyhow::anyhow!("runner identity unavailable"))?,
            scope,
            info: info.clone(),
        };
        if let Err(error) = write_record(&self.logs.join(format!("{id}.service.json")), &record) {
            self.stop(&id).await?;
            return Err(error);
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(500))
            .build()?;
        let readiness = async {
            loop {
                anyhow::ensure!(
                    done.borrow().is_none(),
                    "service exited before readiness; inspect {}",
                    info.log.display()
                );
                if client
                    .get(ready)
                    .send()
                    .await
                    .is_ok_and(|r| r.status().is_success())
                {
                    return Ok::<(), anyhow::Error>(());
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        };
        let result = tokio::select! {
            _ = cancel.cancelled() => Err(anyhow::anyhow!("service readiness cancelled")),
            result = tokio::time::timeout(Duration::from_secs(30), readiness) => match result { Ok(result)=>result, Err(_)=>Err(anyhow::anyhow!("service readiness timed out")) },
        };
        if let Err(error) = result {
            self.stop(&id).await?;
            return Err(error);
        }
        launch_guard.disarm();
        Ok(info)
    }

    pub fn list(&self) -> anyhow::Result<serde_json::Value> {
        let entries = self
            .entries
            .lock()
            .map_err(|_| anyhow::anyhow!("service registry poisoned"))?;
        Ok(serde_json::Value::Array(entries.values().map(|entry| serde_json::json!({"service":entry.info,"running":entry.done.borrow().is_none(),"result":entry.done.borrow().clone()})).collect()))
    }
    pub async fn stop(&self, id: &str) -> anyhow::Result<()> {
        let entry = self
            .entries
            .lock()
            .map_err(|_| anyhow::anyhow!("service registry poisoned"))?
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("unknown owned service"))?;
        entry.cancel.cancel();
        wait_exit(&mut entry.done.clone()).await
    }
    pub async fn shutdown(&self) -> anyhow::Result<()> {
        let ids: Vec<_> = self
            .entries
            .lock()
            .map_err(|_| anyhow::anyhow!("service registry poisoned"))?
            .keys()
            .cloned()
            .collect();
        for id in ids {
            self.stop(&id).await?;
        }
        Ok(())
    }
    pub async fn invalidate(
        &self,
        goal: &Goal,
        steps: &BTreeMap<String, u64>,
    ) -> anyhow::Result<()> {
        let ids: Vec<_> = self
            .entries
            .lock()
            .map_err(|_| anyhow::anyhow!("service registry poisoned"))?
            .iter()
            .filter(|(_, entry)| {
                entry.scope.epoch != goal.epoch
                    || entry.scope.requirements.iter().any(|(id, rev)| {
                        !goal
                            .requirements
                            .get(id)
                            .is_some_and(|r| r.active && r.revision == *rev)
                    })
                    || entry
                        .scope
                        .steps
                        .iter()
                        .any(|(id, rev)| steps.get(id) != Some(rev))
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            self.stop(&id).await?;
        }
        Ok(())
    }
    /// Called only after acquiring the daemon's exclusive workspace-owner lock.
    pub async fn recover(&self) -> anyhow::Result<()> {
        if !self.logs.is_dir() {
            return Ok(());
        }
        for entry in std::fs::read_dir(&self.logs)? {
            let path = entry?.path();
            if !path.to_string_lossy().ends_with(".process.json") {
                continue;
            }
            let record: ProcessRecord = serde_json::from_slice(&std::fs::read(&path)?)?;
            if record.output.is_none() {
                record.identity.stop_group().await?;
            }
        }
        Ok(())
    }
}
impl Drop for Services {
    fn drop(&mut self) {
        if let Ok(entries) = self.entries.lock() {
            for entry in entries.values() {
                entry.cancel.cancel();
            }
        }
    }
}
async fn wait_exit(
    done: &mut watch::Receiver<Option<Result<ProcessOutput, String>>>,
) -> anyhow::Result<()> {
    loop {
        if let Some(result) = done.borrow().clone() {
            result.map_err(anyhow::Error::msg)?;
            return Ok(());
        }
        done.changed()
            .await
            .map_err(|_| anyhow::anyhow!("service exit could not be confirmed"))?;
    }
}
fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
fn write_record(path: &std::path::Path, record: &Record) -> anyhow::Result<()> {
    let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    serde_json::to_writer(&mut file, record)?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    std::fs::File::open(path.parent().unwrap())?.sync_all()?;
    Ok(())
}
