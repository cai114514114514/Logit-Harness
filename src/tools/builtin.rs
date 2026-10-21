use super::files::FileEdit;
use super::{Effect, Tool, ToolContext, ToolEvent, ToolResult, ToolSpec};
use crate::domain::EvidenceKind;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy)]
enum Builtin {
    Read,
    Patch,
    Shell,
}

pub fn tools() -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(Builtin::Read),
        Arc::new(Builtin::Patch),
        Arc::new(Builtin::Shell),
    ]
}

#[async_trait::async_trait]
impl Tool for Builtin {
    fn spec(&self) -> ToolSpec {
        let (name, description, parameters, effect) = match self {
            Self::Read => (
                "read_file",
                "Read a workspace file and obtain its hash for guarded editing. Large content is truncated explicitly.",
                json!({"type":"object","properties":{"path":{"type":"string"},"limit":{"type":"integer"}},"required":["path"],"additionalProperties":false}),
                Effect::Read,
            ),
            Self::Patch => (
                "apply_patch",
                "Apply hash-guarded file edits. Use operation=create with expected=missing for new files, replace with exact unique old/new text for edits, or delete. Never guess a hash: read the file first.",
                json!({"type":"object","properties":{"edits":{"type":"array","items":{"type":"object","properties":{
                    "path":{"type":"string"},"expected":{"type":"string"},"operation":{"type":"string","enum":["create","replace","delete"]},
                    "content":{"type":"string"},"old":{"type":"string"},"new":{"type":"string"}},"required":["path","expected","operation"]}}},"required":["edits"],"additionalProperties":false}),
                Effect::Write,
            ),
            Self::Shell => (
                "shell",
                "Run a local command in a real terminal and wait for its actual exit. The source mutation lock is held throughout. Set timeout_seconds for long commands; cancellation terminates the process group.",
                json!({"type":"object","properties":{"command":{"type":"string"},"timeout_seconds":{"type":"integer"}},"required":["command"],"additionalProperties":false}),
                Effect::Write,
            ),
        };
        ToolSpec {
            name: name.into(),
            description: description.into(),
            parameters,
            effect,
        }
    }
    fn policy(&self, arguments: &Value) -> super::policy::Decision {
        if matches!(self, Self::Shell) {
            super::policy::shell(arguments["command"].as_str().unwrap_or(""))
        } else {
            super::policy::Decision::Allow
        }
    }
    async fn call(
        &self,
        arguments: Value,
        context: ToolContext,
        cancel: CancellationToken,
        events: mpsc::UnboundedSender<ToolEvent>,
    ) -> anyhow::Result<ToolResult> {
        match self {
            Self::Read => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Args {
                    path: String,
                    limit: Option<usize>,
                }
                let args: Args = serde_json::from_value(arguments)?;
                let read = context
                    .workspace
                    .read(&args.path, args.limit.unwrap_or(32768))?;
                Ok(ToolResult {
                    files: BTreeMap::from([(args.path, read.hash.clone())]),
                    data: serde_json::to_value(read)?,
                    success: true,
                    kind: EvidenceKind::Read,
                })
            }
            Self::Patch => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Args {
                    edits: Vec<FileEdit>,
                }
                let args: Args = serde_json::from_value(arguments)?;
                let changes = context.workspace.apply(&args.edits)?;
                let files = changes
                    .iter()
                    .map(|change| (change.path.clone(), change.after.clone()))
                    .collect();
                Ok(ToolResult {
                    data: json!({"changes":changes}),
                    files,
                    success: true,
                    kind: EvidenceKind::Read,
                })
            }
            Self::Shell => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Args {
                    command: String,
                    timeout_seconds: Option<u64>,
                }
                let args: Args = serde_json::from_value(arguments)?;
                let timeout = args.timeout_seconds.unwrap_or(120).clamp(1, 21600);
                let (output, mut receiver) = mpsc::unbounded_channel();
                let relay = tokio::spawn(async move {
                    while let Some(event) = receiver.recv().await {
                        let event = match event {
                            super::process::ProcessEvent::Started { pid } => {
                                ToolEvent::Process { pid }
                            }
                            super::process::ProcessEvent::Output(text) => ToolEvent::Output(text),
                        };
                        let _ = events.send(event);
                    }
                });
                let log = context.logs.join(format!("{}.log", safe_id(&context.id)));
                let result = super::process::run(
                    super::process::ProcessSpec {
                        command: args.command,
                        cwd: context.workspace.root.clone(),
                        log: log.clone(),
                        timeout: Duration::from_secs(timeout),
                        profile: None,
                    },
                    cancel,
                    output,
                )
                .await;
                relay.await?;
                let result = result?;
                let success = result.code == Some(0) && !result.cancelled && !result.timed_out;
                let snapshot = context.workspace.fingerprints()?;
                let files = BTreeMap::from([("$workspace".into(), snapshot["$workspace"].clone())]);
                Ok(ToolResult {
                    data: json!({"process":result,"log":log}),
                    files,
                    success,
                    kind: EvidenceKind::Command,
                })
            }
        }
    }
}

fn safe_id(id: &str) -> String {
    // Provider call identifiers are untrusted; never use them directly as a filesystem path.
    blake3::hash(id.as_bytes()).to_hex()[..24].to_string()
}
