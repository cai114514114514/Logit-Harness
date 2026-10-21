use super::files::Workspace;
use crate::domain::{EvidenceKind, Fingerprints, Goal};
use crate::model::ToolCall;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::Arc,
};
use tokio::sync::{Mutex, RwLock, mpsc};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    Read,
    Check,
    Write,
    Service,
    External,
    Unknown,
}

impl Effect {
    pub fn readonly(self) -> bool {
        matches!(self, Self::Read | Self::Check)
    }
}

#[derive(Clone, Debug)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    pub effect: Effect,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Scope {
    pub requirements: BTreeMap<String, u64>,
    pub observed: Fingerprints,
    pub readonly: bool,
    pub approvals: BTreeSet<String>,
}

#[derive(Clone)]
pub struct ToolContext {
    pub workspace: Workspace,
    pub logs: PathBuf,
    pub id: String,
}

#[derive(Clone, Debug)]
pub enum ToolEvent {
    Output(String),
    Process { pid: u32 },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolResult {
    pub data: Value,
    pub success: bool,
    pub files: Fingerprints,
    pub kind: EvidenceKind,
}

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("authorization required: {reason}")]
    Approval { id: String, reason: String },
    #[error("stale action: {0}")]
    Stale(String),
}

#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    fn spec(&self) -> ToolSpec;
    fn policy(&self, _arguments: &Value) -> super::policy::Decision {
        super::policy::Decision::Allow
    }
    async fn call(
        &self,
        arguments: Value,
        context: ToolContext,
        cancel: CancellationToken,
        events: mpsc::UnboundedSender<ToolEvent>,
    ) -> anyhow::Result<ToolResult>;
}

pub struct ToolHost {
    pub workspace: Workspace,
    pub goal: Arc<RwLock<Goal>>,
    pub logs: PathBuf,
    pub mutation: Arc<Mutex<()>>,
    registry: BTreeMap<String, Arc<dyn Tool>>,
}

impl ToolHost {
    pub fn new(workspace: Workspace, goal: Arc<RwLock<Goal>>, logs: PathBuf) -> Self {
        let registry = super::builtin::tools()
            .into_iter()
            .chain(super::search::tools())
            .map(|tool| (tool.spec().name, tool))
            .collect();
        Self {
            workspace,
            goal,
            logs,
            mutation: Arc::new(Mutex::new(())),
            registry,
        }
    }
    pub fn register(&mut self, tool: Arc<dyn Tool>) -> anyhow::Result<()> {
        let name = tool.spec().name;
        anyhow::ensure!(
            !self.registry.contains_key(&name),
            "tool name already registered: {name}"
        );
        self.registry.insert(name, tool);
        Ok(())
    }
    pub fn definitions(&self) -> Vec<Value> {
        self.registry.values().map(|tool| {
            let spec = tool.spec();
            serde_json::json!({"type":"function","name":spec.name,"description":spec.description,"parameters":spec.parameters})
        }).collect()
    }
    pub fn effect(&self, name: &str) -> Option<Effect> {
        self.registry.get(name).map(|tool| tool.spec().effect)
    }
    pub async fn execute(
        &self,
        call: ToolCall,
        scope: Scope,
        cancel: CancellationToken,
        events: mpsc::UnboundedSender<ToolEvent>,
    ) -> anyhow::Result<ToolResult> {
        let tool = self
            .registry
            .get(&call.name)
            .ok_or_else(|| anyhow::anyhow!("unknown tool: {}", call.name))?;
        let effect = tool.spec().effect;
        anyhow::ensure!(
            !scope.readonly || effect.readonly(),
            "worker cannot directly mutate the workspace or external state"
        );
        let decision = match effect {
            Effect::External | Effect::Unknown => {
                super::policy::Decision::Ask("tool has external or unknown effects".into())
            }
            _ => tool.policy(&call.arguments),
        };
        match decision {
            super::policy::Decision::Deny(reason) => anyhow::bail!(reason),
            super::policy::Decision::Ask(reason) => {
                let id = approval_id(&call.name, &call.arguments);
                if !scope.approvals.contains(&id) {
                    return Err(ToolError::Approval { id, reason }.into());
                }
            }
            super::policy::Decision::Allow => (),
        }
        let _owner = if matches!(effect, Effect::Write | Effect::Check) {
            Some(tokio::select! {
                biased;
                _ = cancel.cancelled() => anyhow::bail!("tool cancelled before mutation ownership"),
                owner = self.mutation.lock() => owner,
            })
        } else {
            None
        };
        if !effect.readonly() {
            let goal = self.goal.read().await;
            if scope.requirements.is_empty()
                || scope.requirements.iter().any(|(id, version)| {
                    !goal
                        .requirements
                        .get(id)
                        .is_some_and(|r| r.active && r.revision == *version)
                })
            {
                return Err(ToolError::Stale("requirements changed".into()).into());
            }
            for (path, hash) in &scope.observed {
                let current = if path == "$workspace" {
                    self.workspace.fingerprints()?[path].clone()
                } else {
                    super::files::fingerprint(&self.workspace.resolve(path)?)?
                };
                if current != *hash {
                    return Err(ToolError::Stale(format!("observed file changed: {path}")).into());
                }
            }
        }
        anyhow::ensure!(!cancel.is_cancelled(), "tool cancelled before dispatch");
        let mut result = tool
            .call(
                call.arguments,
                ToolContext {
                    workspace: self.workspace.clone(),
                    logs: self.logs.clone(),
                    id: call.id,
                },
                cancel,
                events,
            )
            .await?;
        if effect == Effect::Write {
            let snapshot = self.workspace.fingerprints()?;
            result
                .files
                .insert("$workspace".into(), snapshot["$workspace"].clone());
        }
        Ok(result)
    }
}

pub fn approval_id(name: &str, arguments: &Value) -> String {
    blake3::hash(format!("{name}:{}", arguments).as_bytes())
        .to_hex()
        .to_string()
}
