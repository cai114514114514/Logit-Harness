use super::BrowserClient;
use crate::{
    domain::EvidenceKind,
    tools::{
        Effect, Tool, ToolContext, ToolEvent, ToolImage, ToolResult, ToolSpec, policy::Decision,
    },
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
                "Open an HTTP(S) URL in an owned, isolated Chrome profile. Returns its page ID and actual DOM/AX/screenshot evidence.",
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
            effect: Effect::Read,
        }
    }
    fn policy(&self, args: &Value) -> Decision {
        if matches!(self.name, "browser_click" | "browser_input")
            && !self.client.local(args["page"].as_str().unwrap_or(""))
        {
            Decision::Ask("interaction with a remote website may modify external state".into())
        } else {
            Decision::Allow
        }
    }
    async fn call(
        &self,
        args: Value,
        context: ToolContext,
        cancel: CancellationToken,
        _events: mpsc::UnboundedSender<ToolEvent>,
    ) -> anyhow::Result<ToolResult> {
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
                    .click(&page, get("selector")?, cancel.clone())
                    .await?
            }
            "browser_input" => {
                self.client
                    .input(&page, get("selector")?, get("text")?, cancel.clone())
                    .await?
            }
            _ => (),
        }
        let snapshot = self.client.inspect(&page, cancel).await?;
        let image = ToolImage {
            data_url: snapshot.image.clone(),
        };
        let mut data = serde_json::to_value(snapshot)?;
        if let Some(dom) = data["dom"].as_str() {
            if dom.len() > 60000 {
                data["dom"] = json!(dom.chars().take(60000).collect::<String>());
                data["truncated"] = json!(true);
            }
        }
        let files = context.workspace.fingerprints()?;
        Ok(ToolResult {
            data,
            images: vec![image],
            success: true,
            files: std::collections::BTreeMap::from([(
                "$workspace".into(),
                files["$workspace"].clone(),
            )]),
            kind: EvidenceKind::Browser,
        })
    }
}
