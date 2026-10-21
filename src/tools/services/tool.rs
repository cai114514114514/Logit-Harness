use super::Services;
use crate::{
    domain::EvidenceKind,
    tools::{Effect, Tool, ToolContext, ToolEvent, ToolResult, ToolSpec, policy},
};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

enum Operation {
    Start,
    List,
    Stop,
}
struct ServiceTool {
    services: Arc<Services>,
    operation: Operation,
}
pub fn tools(services: Arc<Services>) -> Vec<Arc<dyn Tool>> {
    [Operation::Start, Operation::List, Operation::Stop]
        .into_iter()
        .map(|operation| {
            Arc::new(ServiceTool {
                services: services.clone(),
                operation,
            }) as Arc<dyn Tool>
        })
        .collect()
}
#[async_trait::async_trait]
impl Tool for ServiceTool {
    fn spec(&self) -> ToolSpec {
        let (name, description, properties, required, effect) = match self.operation {
            Operation::Start => (
                "service_start",
                "Start a development server and wait for loopback HTTP readiness. Source writes are denied; write build output only to $LOH_OUTPUT_DIR. It stays alive independently of foreground edits.",
                json!({"command":{"type":"string"},"ready_url":{"type":"string"}}),
                json!(["command", "ready_url"]),
                Effect::Service,
            ),
            Operation::List => (
                "service_list",
                "Inspect owned service state and log locations.",
                json!({}),
                json!([]),
                Effect::Read,
            ),
            Operation::Stop => (
                "service_stop",
                "Stop an owned service and confirm its process exited.",
                json!({"id":{"type":"string"}}),
                json!(["id"]),
                Effect::Control,
            ),
        };
        ToolSpec {
            name: name.into(),
            description: description.into(),
            parameters: json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}),
            effect,
        }
    }
    fn policy(&self, arguments: &Value) -> policy::Decision {
        if matches!(self.operation, Operation::Start) {
            policy::shell(arguments["command"].as_str().unwrap_or(""))
        } else {
            policy::Decision::Allow
        }
    }
    async fn call(
        &self,
        arguments: Value,
        context: ToolContext,
        cancel: CancellationToken,
        _events: mpsc::UnboundedSender<ToolEvent>,
    ) -> anyhow::Result<ToolResult> {
        let data = match self.operation {
            Operation::Start => serde_json::to_value(
                self.services
                    .start(
                        arguments["command"]
                            .as_str()
                            .ok_or_else(|| anyhow::anyhow!("missing command"))?,
                        arguments["ready_url"]
                            .as_str()
                            .ok_or_else(|| anyhow::anyhow!("missing readiness URL"))?,
                        context.scope,
                        cancel,
                    )
                    .await?,
            )?,
            Operation::List => self.services.list()?,
            Operation::Stop => {
                self.services
                    .stop(
                        arguments["id"]
                            .as_str()
                            .ok_or_else(|| anyhow::anyhow!("missing service ID"))?,
                    )
                    .await?;
                json!({"stopped":true})
            }
        };
        Ok(ToolResult {
            images: vec![],
            data,
            success: true,
            files: Default::default(),
            kind: EvidenceKind::Read,
        })
    }
}
