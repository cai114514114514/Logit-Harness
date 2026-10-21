//! Hash-guarded edits are committed by the single workspace mutation owner.
use super::files::{EditOperation, FileDelta, FileEdit, MISSING, Workspace, fingerprint};
use std::{
    collections::BTreeSet,
    io::Write,
    path::{Path, PathBuf},
};

/// One validated edit: the resolved path plus content before and after.
struct Prepared<'a> {
    edit: &'a FileEdit,
    path: PathBuf,
    before: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
}

impl Workspace {
    pub fn apply(&self, edits: &[FileEdit]) -> anyhow::Result<Vec<FileDelta>> {
        anyhow::ensure!(!edits.is_empty(), "empty patch");
        let prepared = self.prepare(edits)?;
        commit(&prepared)?;
        prepared
            .iter()
            .map(|p| {
                Ok(FileDelta {
                    path: p.edit.path.clone(),
                    before: p.edit.expected.clone(),
                    after: fingerprint(&p.path)?,
                })
            })
            .collect()
    }

    /// Validate every edit against the current tree before any write happens.
    fn prepare<'a>(&self, edits: &'a [FileEdit]) -> anyhow::Result<Vec<Prepared<'a>>> {
        let mut prepared = vec![];
        let mut names = BTreeSet::new();
        for edit in edits {
            let path = self.resolve(&edit.path)?;
            // Normalize components ("./a" and "a" resolve to the same entry) so
            // spelling variants cannot slip past the duplicate detection below.
            let path: PathBuf = path.components().collect();
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
                    // old == new is a legitimate no-op replacement: the file is
                    // still rewritten unchanged, refreshing its mtime like any write.
                    Some(original.replacen(old, new, 1).into_bytes())
                }
                EditOperation::Delete => {
                    anyhow::ensure!(before.is_some(), "delete needs existing file");
                    None
                }
            };
            prepared.push(Prepared {
                edit,
                path,
                before,
                after,
            });
        }
        Ok(prepared)
    }
}

/// Write each edit in order; on the first failure, roll back earlier writes.
fn commit(prepared: &[Prepared]) -> anyhow::Result<()> {
    let mut completed: Vec<usize> = vec![];
    for (index, p) in prepared.iter().enumerate() {
        let write_one = (|| -> anyhow::Result<()> {
            // Recheck immediately before writing, including files validated earlier in this patch.
            anyhow::ensure!(
                fingerprint(&p.path)? == p.edit.expected,
                "file changed during patch: {}",
                p.edit.path
            );
            write(&p.path, p.after.as_deref())
        })();
        if let Err(error) = write_one {
            return Err(match rollback(prepared, &completed) {
                // Rollback failures must not vanish: the workspace is left in a
                // partial state and the caller needs to know.
                Some(failures) => error.context(failures),
                None => error,
            });
        }
        completed.push(index);
    }
    Ok(())
}

/// Best-effort restore of already-written files. Returns a description of any
/// files whose rollback itself failed, so the caller can attach it to the error.
fn rollback(prepared: &[Prepared], completed: &[usize]) -> Option<String> {
    let mut failures = vec![];
    for previous in completed.iter().rev() {
        let p = &prepared[*previous];
        let own_hash = p
            .after
            .as_deref()
            .map(|b| blake3::hash(b).to_hex().to_string())
            .unwrap_or_else(|| MISSING.into());
        if fingerprint(&p.path).is_ok_and(|h| h == own_hash)
            && let Err(error) = write(&p.path, p.before.as_deref())
        {
            failures.push(format!("{}: {error}", p.edit.path));
        }
    }
    (!failures.is_empty()).then(|| format!("rollback also failed: {}", failures.join("; ")))
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
