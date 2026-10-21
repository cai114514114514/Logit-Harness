use super::{Effect, Tool, ToolContext, ToolEvent, ToolResult, ToolSpec};
use crate::domain::EvidenceKind;
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

struct Search {
    listing: bool,
}
pub fn tools() -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(Search { listing: false }),
        Arc::new(Search { listing: true }),
    ]
}

#[async_trait::async_trait]
impl Tool for Search {
    fn spec(&self) -> ToolSpec {
        let parameters = if self.listing {
            json!({"type":"object","properties":{"pattern":{"type":"string"},"limit":{"type":"integer"}},"additionalProperties":false})
        } else {
            json!({"type":"object","properties":{"pattern":{"type":"string"},"limit":{"type":"integer"}},"required":["pattern"],"additionalProperties":false})
        };
        ToolSpec { name:if self.listing {"list_files"} else {"search_text"}.into(),
            description:if self.listing {"List workspace source paths, optionally filtered by regex."} else {"Search source with a regex. Returns paths, 1-based line numbers and file hashes. Truncated large files are reported."}.into(),parameters,effect:Effect::Read }
    }
    async fn call(
        &self,
        arguments: Value,
        context: ToolContext,
        cancel: CancellationToken,
        _events: mpsc::UnboundedSender<ToolEvent>,
    ) -> anyhow::Result<ToolResult> {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Args {
            pattern: Option<String>,
            limit: Option<usize>,
        }
        let args: Args = serde_json::from_value(arguments)?;
        anyhow::ensure!(
            self.listing || args.pattern.is_some(),
            "search needs a pattern"
        );
        let regex = regex::RegexBuilder::new(args.pattern.as_deref().unwrap_or(".*"))
            .size_limit(2 * 1024 * 1024)
            .build()?;
        let limit = args.limit.unwrap_or(100).clamp(1, 500);
        let paths = context.workspace.fingerprints()?;
        let mut matches = vec![];
        // A no-match result is also evidence, and expires when the searched tree changes.
        let mut files = BTreeMap::from([("$workspace".into(), paths["$workspace"].clone())]);
        let mut truncated = vec![];
        for (path, hash) in paths.iter().filter(|(p, _)| p.as_str() != "$workspace") {
            anyhow::ensure!(!cancel.is_cancelled(), "search cancelled");
            if self.listing {
                if regex.is_match(path) {
                    matches.push(json!({"path":path,"hash":hash}));
                }
            } else {
                let Ok(read) = context.workspace.read(path, 256 * 1024) else {
                    continue;
                };
                if read.truncated {
                    truncated.push(path);
                }
                for (line, text) in read.content.lines().enumerate() {
                    if regex.is_match(text) {
                        matches.push(json!({"path":path,"line":line+1,"text":text.chars().take(1000).collect::<String>()}));
                        files.insert(path.clone(), read.hash.clone());
                        if matches.len() == limit {
                            break;
                        }
                    }
                }
            }
            if matches.len() == limit {
                break;
            }
        }
        let limited = matches.len() == limit;
        Ok(ToolResult {
            data: json!({"matches":matches,"limit_reached":limited,"truncated_files":truncated}),
            success: true,
            files,
            kind: EvidenceKind::Read,
        })
    }
}
