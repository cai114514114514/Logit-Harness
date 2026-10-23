use super::BrowserClient;
use crate::{
    domain::EvidenceKind,
    tools::{Effect, Tool, ToolContext, ToolEvent, ToolImage, ToolResult, ToolSpec},
};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

struct BrowserTool {
    client: Arc<BrowserClient>,
    name: &'static str,
}
pub fn tools(client: Arc<BrowserClient>) -> Vec<Arc<dyn Tool>> {
    [
        "browser_open",
        "browser_inspect",
        "browser_click",
        "browser_input",
    ]
    .into_iter()
    .map(|name| {
        Arc::new(BrowserTool {
            client: client.clone(),
            name,
        }) as Arc<dyn Tool>
    })
    .collect()
}
#[async_trait::async_trait]
impl Tool for BrowserTool {
    fn spec(&self) -> ToolSpec {
        let (description, properties, required) = match self.name {
            "browser_open" => (
                "Open an HTTP(S) URL in an owned Chrome profile. Loopback development is allowed; known LAN/link-local addresses are refused, including intercepted redirects and page subresources. Returns DOM/AX/screenshot evidence; this is not a network sandbox.",
                json!({"url":{"type":"string"}}),
                json!(["url"]),
            ),
            "browser_click" => (
                "Click an actual DOM element in an owned page. Remote-site interactions require scoped authorization.",
                json!({"page":{"type":"string"},"selector":{"type":"string"}}),
                json!(["page", "selector"]),
            ),
            "browser_input" => (
                "Focus an input, select its text and insert Unicode through the browser input protocol.",
                json!({"page":{"type":"string"},"selector":{"type":"string"},"text":{"type":"string"}}),
                json!(["page", "selector", "text"]),
            ),
            _ => (
                "Inspect actual DOM, accessibility tree, screenshot and console/errors for an owned page.",
                json!({"page":{"type":"string"}}),
                json!(["page"]),
            ),
        };
        ToolSpec {
            name: self.name.into(),
            description: description.into(),
            parameters: json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}),
            effect: if matches!(self.name, "browser_click" | "browser_input") {
                Effect::Check
            } else {
                Effect::Read
            },
        }
    }
    async fn call(
        &self,
        args: Value,
        context: ToolContext,
        cancel: CancellationToken,
        _events: mpsc::UnboundedSender<ToolEvent>,
    ) -> anyhow::Result<ToolResult> {
        let before = context.workspace.fingerprints()?;
        let permission = super::Permission {
            name: self.name,
            arguments: &args,
            scope: &context.scope,
        };
        let get = |key: &str| {
            args[key]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("missing {key}"))
        };
        let page = if self.name == "browser_open" {
            self.client.open(get("url")?, cancel.clone()).await?
        } else {
            get("page")?.into()
        };
        match self.name {
            "browser_click" => {
                self.client
                    .click(&page, get("selector")?, &permission, cancel.clone())
                    .await?
            }
            "browser_input" => {
                self.client
                    .input(
                        &page,
                        get("selector")?,
                        get("text")?,
                        &permission,
                        cancel.clone(),
                    )
                    .await?
            }
            _ => (),
        }
        let snapshot = self.client.inspect(&page, cancel).await?;
        let image = ToolImage {
            data_url: snapshot.image.clone(),
        };
        let mut data = serde_json::to_value(snapshot)?;
        if let Some(dom) = data["dom"].as_str()
            && dom.len() > 60000
        {
            data["dom"] = json!(dom.chars().take(60000).collect::<String>());
            data["truncated"] = json!(true);
        }
        let files = context.workspace.fingerprints()?;
        let stable = before["$workspace"] == files["$workspace"];
        data["source_stable"] = json!(stable);
        Ok(ToolResult {
            data,
            images: vec![image],
            success: stable,
            files: std::collections::BTreeMap::from([(
                "$workspace".into(),
                files["$workspace"].clone(),
            )]),
            kind: EvidenceKind::Browser,
        })
    }
}
