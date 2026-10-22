use super::files::{WORKSPACE_FINGERPRINT_KEY, Workspace};
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
    /// Source-readonly services have separate lifecycle ownership.
    Service,
    Control,
    External,
    /// Reserved: for tools whose effects cannot be classified; always escalates
    /// to an approval rather than guessing.
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
    #[serde(default)]
    pub epoch: u64,
    #[serde(default)]
    pub steps: BTreeMap<String, u64>,
    pub observed: Fingerprints,
    pub readonly: bool,
    pub approvals: BTreeSet<String>,
}

#[derive(Clone)]
pub struct ToolContext {
    pub workspace: Workspace,
    pub logs: PathBuf,
    pub id: String,
    pub scope: Scope,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<ToolImage>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolImage {
    pub data_url: String,
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
    /// Dispatchers such as MCP classify the concrete target before authorization.
    fn effect(&self, _arguments: &Value) -> Effect {
        self.spec().effect
    }
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
    pub steps: Arc<RwLock<BTreeMap<String, u64>>>,
    pub blocked: Arc<RwLock<BTreeSet<String>>>,
    pub services: Option<Arc<super::services::Services>>,
    pub knowledge: Option<Arc<crate::extensions::knowledge::Knowledge>>,
    pub mcp: Option<Arc<crate::extensions::mcp::Registry>>,
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
            steps: Arc::new(RwLock::new(BTreeMap::new())),
            blocked: Arc::new(RwLock::new(BTreeSet::new())),
            services: None,
            knowledge: None,
            mcp: None,
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
    pub fn call_effect(&self, call: &ToolCall) -> Option<Effect> {
        self.registry
            .get(&call.name)
            .map(|tool| tool.effect(&call.arguments))
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
        let effect = tool.effect(&call.arguments);
        self.authorize(tool.as_ref(), effect, &call, &scope)?;
        // Lock order is a load-bearing invariant: the mutation mutex is always
        // acquired before goal.read()/steps.read() in check_stale, never the other
        // way around, so no lock cycle can form. Check also holds the mutex for
        // its whole run — its verdict is only meaningful if no writer interleaves.
        // Services are allowed to overlap: their OS profile forbids source writes
        // and only permits a separate, owned build-output directory.
        let _owner = if matches!(effect, Effect::Write | Effect::Check | Effect::Unknown) {
            Some(tokio::select! {
                biased;
                _ = cancel.cancelled() => anyhow::bail!("tool cancelled before mutation ownership"),
                owner = self.mutation.lock() => owner,
            })
        } else {
            None
        };
        if (!effect.readonly() || effect == Effect::Check) && effect != Effect::Control {
            self.check_stale(&scope).await?;
        }
        if call.name == "apply_patch"
            && let Some(knowledge) = &self.knowledge
        {
            let paths = call.arguments["edits"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|edit| edit["path"].as_str().map(str::to_owned))
                .collect::<Vec<_>>();
            for entry in knowledge.instructions.for_paths(&paths)? {
                anyhow::ensure!(
                    scope.observed.get(&entry.path) == Some(&entry.hash),
                    "read project_instructions before editing this directory: {}",
                    entry.path
                );
            }
        }
        anyhow::ensure!(!cancel.is_cancelled(), "tool cancelled before dispatch");
        let mut result = self.dispatch(tool, call, scope, cancel, events).await?;
        if matches!(effect, Effect::Write | Effect::Unknown) {
            let snapshot = self.workspace.fingerprints()?;
            let aggregate = snapshot
                .get(WORKSPACE_FINGERPRINT_KEY)
                .ok_or_else(|| anyhow::anyhow!("snapshot lacks an aggregate fingerprint"))?;
            result
                .files
                .insert(WORKSPACE_FINGERPRINT_KEY.into(), aggregate.clone());
        }
        Ok(result)
    }

    /// Readonly scope, policy decision and approval gate, before any lock is taken.
    fn authorize(
        &self,
        tool: &dyn Tool,
        effect: Effect,
        call: &ToolCall,
        scope: &Scope,
    ) -> anyhow::Result<()> {
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
        Ok(())
    }

    /// Reject work scoped against a goal, plan or tree that has since moved on.
    /// Runs with the mutation lock held, so the state checked here cannot change
    /// before dispatch.
    async fn check_stale(&self, scope: &Scope) -> anyhow::Result<()> {
        let blocked = self.blocked.read().await;
        anyhow::ensure!(
            !scope.requirements.keys().any(|id| blocked.contains(id)),
            "action depends on unanswered user input; focus an independent step"
        );
        drop(blocked);
        let goal = self.goal.read().await;
        if scope.epoch != goal.epoch
            || scope.requirements.is_empty()
            || scope.requirements.iter().any(|(id, version)| {
                !goal
                    .requirements
                    .get(id)
                    .is_some_and(|r| r.active && r.revision == *version)
            })
        {
            return Err(ToolError::Stale("requirements changed".into()).into());
        }
        let steps = self.steps.read().await;
        if scope
            .steps
            .iter()
            .any(|(id, revision)| steps.get(id) != Some(revision))
        {
            return Err(ToolError::Stale("plan dependency changed".into()).into());
        }
        for (path, hash) in &scope.observed {
            let current = if path.as_str() == WORKSPACE_FINGERPRINT_KEY {
                self.workspace
                    .fingerprints()?
                    .get(WORKSPACE_FINGERPRINT_KEY)
                    .ok_or_else(|| anyhow::anyhow!("snapshot lacks an aggregate fingerprint"))?
                    .clone()
            } else {
                super::files::fingerprint(&self.workspace.resolve(path)?)?
            };
            if current != *hash {
                return Err(ToolError::Stale(format!("observed file changed: {path}")).into());
            }
        }
        Ok(())
    }

    async fn dispatch(
        &self,
        tool: &Arc<dyn Tool>,
        call: ToolCall,
        scope: Scope,
        cancel: CancellationToken,
        events: mpsc::UnboundedSender<ToolEvent>,
    ) -> anyhow::Result<ToolResult> {
        tool.call(
            call.arguments,
            ToolContext {
                workspace: self.workspace.clone(),
                logs: self.logs.clone(),
                id: call.id,
                scope,
            },
            cancel,
            events,
        )
        .await
    }
}

/// Stable identity of one call for approval matching. Relies on serde_json's
/// default BTreeMap-backed object representation: keys serialize in sorted order,
/// so equal payloads hash identically regardless of insertion order.
pub fn approval_id(name: &str, arguments: &Value) -> String {
    blake3::hash(format!("{name}:{}", arguments).as_bytes())
        .to_hex()
        .to_string()
}
