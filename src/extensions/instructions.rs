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
            let scope = self.resolve(&root, Path::new(path))?;
            let mut parent = if scope.is_dir() {
                scope
            } else {
                scope.parent().unwrap_or(&root).to_owned()
            };
            while parent.starts_with(&root) {
                candidates.insert(parent.join("AGENTS.md"));
                if parent == root || !parent.pop() {
                    break;
                }
            }
        }
        let mut instructions = vec![];
        for path in candidates {
            let metadata = match std::fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            anyhow::ensure!(
                !metadata.file_type().is_symlink() && path.canonicalize()?.starts_with(&root),
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

    fn resolve(&self, root: &Path, path: &Path) -> anyhow::Result<PathBuf> {
        // Accept the caller's workspace spelling as well as its canonical path
        // (for example /var vs /private/var on macOS). Every existing component
        // is then resolved and checked before any instruction file is read.
        let workspace = std::path::absolute(&self.root)?;
        let relative = if path.is_absolute() {
            path.strip_prefix(root)
                .or_else(|_| path.strip_prefix(&workspace))?
        } else {
            path
        };
        anyhow::ensure!(
            relative
                .components()
                .all(|part| matches!(part, Component::Normal(_) | Component::CurDir)),
            "instruction scope escaped workspace"
        );
        let mut resolved = root.to_owned();
        for part in relative.components() {
            resolved.push(part);
            match std::fs::symlink_metadata(&resolved) {
                Ok(_) => {
                    resolved = resolved.canonicalize()?;
                    anyhow::ensure!(
                        resolved.starts_with(root),
                        "instruction scope escaped workspace"
                    );
                }
                // Future edit targets still inherit their existing parents'
                // guidance; a missing suffix cannot hide a symlink ancestor.
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                Err(error) => return Err(error.into()),
            }
        }
        Ok(resolved)
    }
}
