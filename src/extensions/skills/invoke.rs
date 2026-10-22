use super::{Entry, Skills};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub id: String,
    pub hash: String,
    pub arguments: String,
}

/// A loaded workflow is data, separate from the user's literal invocation.
/// Persisting provenance lets later goal/worker requests retain the same body.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Invocation {
    pub id: String,
    pub name: String,
    pub source: String,
    pub path: PathBuf,
    pub hash: String,
    pub content: String,
    pub arguments: String,
}
impl Invocation {
    pub fn control(&self) -> crate::runtime::state::Control {
        crate::runtime::state::Control::Skill {
            id: self.id.clone(),
            hash: self.hash.clone(),
            arguments: self.arguments.clone(),
        }
    }
    pub fn request(&self) -> String {
        format!(
            "/skills run {}{}{}",
            self.id,
            if self.arguments.is_empty() { "" } else { " " },
            self.arguments
        )
    }
}

impl Skills {
    pub fn candidates(&self, selector: &str) -> Vec<&Entry> {
        self.entries
            .iter()
            .filter(|entry| entry.id == selector || entry.name == selector)
            .collect()
    }
    pub fn invoke(&self, selector: &str, arguments: &str) -> anyhow::Result<Invocation> {
        let candidates = self.candidates(selector);
        anyhow::ensure!(
            !candidates.is_empty(),
            "未知命令或技能；输入 /help 或 /skills 查看"
        );
        anyhow::ensure!(
            candidates.len() == 1,
            "同名技能有多个来源；用 /skills run <ID> 选择：{}",
            candidates
                .iter()
                .map(|e| format!("{} [{}] {}", e.id, e.source, e.path.display()))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let entry = candidates[0];
        let loaded = self.load(&entry.id, None)?;
        Ok(Invocation {
            id: entry.id.clone(),
            name: entry.name.clone(),
            source: entry.source.clone(),
            path: entry.path.clone(),
            hash: loaded.hash,
            content: loaded.content,
            arguments: arguments.into(),
        })
    }
}
