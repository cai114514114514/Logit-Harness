//! Hash-guarded edits are committed by the single workspace mutation owner.
use super::files::{EditOperation, FileDelta, FileEdit, MISSING, Workspace, fingerprint};
use std::{collections::BTreeSet, io::Write, path::Path};

impl Workspace {
    pub fn apply(&self, edits: &[FileEdit]) -> anyhow::Result<Vec<FileDelta>> {
        anyhow::ensure!(!edits.is_empty(), "empty patch");
        let mut prepared = vec![];
        let mut names = BTreeSet::new();
        for edit in edits {
            let path = self.resolve(&edit.path)?;
            anyhow::ensure!(names.insert(path.clone()), "duplicate file in patch");
            let mut ancestor = path.as_path();
            while ancestor != self.root {
                anyhow::ensure!(
                    !std::fs::symlink_metadata(ancestor).is_ok_and(|m| m.file_type().is_symlink()),
                    "edit a symlink's resolved path explicitly"
                );
                ancestor = ancestor
                    .parent()
                    .ok_or_else(|| anyhow::anyhow!("invalid edit path"))?;
            }
            anyhow::ensure!(
                fingerprint(&path)? == edit.expected,
                "stale file: {}",
                edit.path
            );
            let before = if path.exists() {
                Some(std::fs::read(&path)?)
            } else {
                None
            };
            let after = match &edit.operation {
                EditOperation::Create { content } => {
                    anyhow::ensure!(
                        before.is_none() && edit.expected == MISSING,
                        "create cannot overwrite existing file"
                    );
                    Some(content.as_bytes().to_vec())
                }
                EditOperation::Replace { old, new } => {
                    let original = std::str::from_utf8(
                        before
                            .as_deref()
                            .ok_or_else(|| anyhow::anyhow!("replace needs existing file"))?,
                    )?;
                    anyhow::ensure!(
                        !old.is_empty() && original.matches(old).count() == 1,
                        "replacement must match exactly once: {}",
                        edit.path
                    );
                    Some(original.replacen(old, new, 1).into_bytes())
                }
                EditOperation::Delete => {
                    anyhow::ensure!(before.is_some(), "delete needs existing file");
                    None
                }
            };
            prepared.push((edit, path, before, after));
        }
        let mut completed: Vec<usize> = vec![];
        for (index, (edit, path, _, after)) in prepared.iter().enumerate() {
            let commit = (|| -> anyhow::Result<()> {
                // Recheck immediately before writing, including files validated earlier in this patch.
                anyhow::ensure!(
                    fingerprint(path)? == edit.expected,
                    "file changed during patch: {}",
                    edit.path
                );
                write(path, after.as_deref())
            })();
            if let Err(error) = commit {
                for previous in completed.into_iter().rev() {
                    let (_, path, before, after) = &prepared[previous];
                    let own_hash = after
                        .as_deref()
                        .map(|b| blake3::hash(b).to_hex().to_string())
                        .unwrap_or_else(|| MISSING.into());
                    if fingerprint(path).is_ok_and(|h| h == own_hash) {
                        let _ = write(path, before.as_deref());
                    }
                }
                return Err(error);
            }
            completed.push(index);
        }
        prepared
            .into_iter()
            .map(|(edit, path, _, _)| {
                Ok(FileDelta {
                    path: edit.path.clone(),
                    before: edit.expected.clone(),
                    after: fingerprint(&path)?,
                })
            })
            .collect()
    }
}

fn write(path: &Path, content: Option<&[u8]>) -> anyhow::Result<()> {
    if let Some(content) = content {
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("missing parent"))?;
        std::fs::create_dir_all(parent)?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        if let Ok(metadata) = std::fs::metadata(path) {
            temporary
                .as_file()
                .set_permissions(metadata.permissions())?;
        }
        temporary.write_all(content)?;
        temporary.as_file().sync_all()?;
        temporary.persist(path)?;
    } else {
        std::fs::remove_file(path)?;
    }
    Ok(())
}
