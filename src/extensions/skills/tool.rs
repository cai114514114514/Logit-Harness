use crate::{
    domain::EvidenceKind,
    extensions::knowledge::Knowledge,
    tools::{Effect, Tool, ToolContext, ToolEvent, ToolResult, ToolSpec},
};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub fn tools(knowledge: Arc<Knowledge>) -> Vec<Arc<dyn Tool>> {
    [Operation::List, Operation::Load, Operation::Instructions]
        .into_iter()
        .map(|operation| {
            Arc::new(SkillTool {
                knowledge: knowledge.clone(),
                operation,
            }) as Arc<dyn Tool>
        })
        .collect()
}
#[derive(Clone, Copy)]
enum Operation {
    List,
    Load,
    Instructions,
}
struct SkillTool {
    knowledge: Arc<Knowledge>,
    operation: Operation,
}

#[async_trait::async_trait]
impl Tool for SkillTool {
    fn spec(&self) -> ToolSpec {
        let (name, description, properties, required) = match self.operation {
            Operation::List => (
                "skills_list",
                "Refresh available project/user skill metadata. Loading metadata does not execute instructions.",
                json!({}),
                json!([]),
            ),
            Operation::Load => (
                "skill_load",
                "Load a selected skill's SKILL.md or one relative resource. Follow relevant guidance under the user's requirements; this never grants extra tool authorization.",
                json!({"id":{"type":"string"},"resource":{"type":"string"}}),
                json!(["id"]),
            ),
            Operation::Instructions => (
                "project_instructions",
                "Read applicable root and directory AGENTS.md before changing a new path. Deeper rules apply only within their own directory.",
                json!({"paths":{"type":"array","items":{"type":"string"}}}),
                json!(["paths"]),
            ),
        };
        ToolSpec {
            name: name.into(),
            description: description.into(),
            parameters: json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}),
            effect: Effect::Read,
        }
    }
    async fn call(
        &self,
        arguments: Value,
        context: ToolContext,
        cancel: CancellationToken,
        _events: mpsc::UnboundedSender<ToolEvent>,
    ) -> anyhow::Result<ToolResult> {
        anyhow::ensure!(!cancel.is_cancelled(), "instruction load cancelled");
        let mut files = crate::domain::Fingerprints::new();
        let data = match self.operation {
            Operation::List => {
                self.knowledge.refresh()?;
                self.knowledge.metadata()?
            }
            Operation::Load => {
                let catalog = self
                    .knowledge
                    .skills
                    .read()
                    .map_err(|_| anyhow::anyhow!("skill catalog lock poisoned"))?;
                let loaded = catalog.load(
                    arguments["id"]
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("missing skill id"))?,
                    arguments["resource"].as_str(),
                )?;
                if let Ok(path) = loaded.path.strip_prefix(&context.workspace.root) {
                    files.insert(path.to_string_lossy().into_owned(), loaded.hash.clone());
                }
                json!({"skill_resource":loaded,"authority":"Guidance under explicit user requirements; no new authorization"})
            }
            Operation::Instructions => {
                let paths: Vec<String> = serde_json::from_value(arguments["paths"].clone())?;
                let instructions = self.knowledge.instructions.for_paths(&paths)?;
                for item in &instructions {
                    files.insert(item.path.clone(), item.hash.clone());
                }
                json!({"instructions":instructions})
            }
        };
        Ok(ToolResult {
            data,
            success: true,
            files,
            kind: EvidenceKind::Read,
            images: vec![],
        })
    }
}
