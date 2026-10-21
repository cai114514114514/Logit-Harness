use serde::Serialize;
use std::{
    collections::BTreeSet,
    path::{Component, Path, PathBuf},
};

#[derive(Debug, Serialize)]
pub struct Instruction {
    pub path: String,
    pub hash: String,
    pub content: String,
}

pub struct Instructions {
    root: PathBuf,
}

impl Instructions {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn for_paths(&self, paths: &[String]) -> anyhow::Result<Vec<Instruction>> {
        let root = self.root.canonicalize()?;
        let mut candidates = BTreeSet::from([root.join("AGENTS.md")]);
        for path in paths.iter().filter(|path| path.as_str() != "$workspace") {
            let relative = Path::new(path);
            anyhow::ensure!(
                relative
                    .components()
                    .all(|part| matches!(part, Component::Normal(_) | Component::CurDir)),
                "instruction scope escaped workspace"
            );
            let mut parent = root.join(relative).parent().unwrap_or(&root).to_owned();
            while parent.starts_with(&root) {
                candidates.insert(parent.join("AGENTS.md"));
                if parent == root || !parent.pop() {
                    break;
                }
            }
        }
        let mut instructions = vec![];
        for path in candidates {
            if !path.exists() {
                continue;
            }
            anyhow::ensure!(
                !std::fs::symlink_metadata(&path)?.file_type().is_symlink()
                    && path.canonicalize()?.starts_with(&root),
                "instruction path escaped workspace"
            );
            let content = super::skills::read(&path)?;
            instructions.push(Instruction {
                path: path.strip_prefix(&root)?.to_string_lossy().into_owned(),
                hash: blake3::hash(content.as_bytes()).to_hex().to_string(),
                content,
            });
        }
        instructions.sort_by_key(|item| (item.path.matches('/').count(), item.path.clone()));
        Ok(instructions)
    }
}
