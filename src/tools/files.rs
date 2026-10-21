use crate::domain::Fingerprints;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};

pub const MISSING: &str = "missing";

#[derive(Clone, Debug)]
pub struct Workspace {
    pub root: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileEdit {
    pub path: String,
    pub expected: String,
    #[serde(flatten)]
    pub operation: EditOperation,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum EditOperation {
    Create { content: String },
    Replace { old: String, new: String },
    Delete,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileRead {
    pub path: String,
    pub hash: String,
    pub content: String,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileDelta {
    pub path: String,
    pub before: String,
    pub after: String,
}

impl Workspace {
    pub fn open(root: &Path) -> anyhow::Result<Self> {
        Ok(Self {
            root: root.canonicalize()?,
        })
    }
    pub fn resolve(&self, path: &str) -> anyhow::Result<PathBuf> {
        use std::path::Component;
        let relative = Path::new(path);
        anyhow::ensure!(!relative.is_absolute(), "use a workspace-relative path");
        for part in relative.components() {
            anyhow::ensure!(
                !matches!(
                    part,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                ),
                "path escapes workspace"
            );
            anyhow::ensure!(
                part.as_os_str() != ".git" && part.as_os_str() != ".weft",
                "internal state is not editable through file tools"
            );
        }
        let destination = self.root.join(relative);
        let mut existing = destination.as_path();
        while !existing.exists() {
            // Broken symlinks also need rejecting, rather than treating their targets as new files.
            anyhow::ensure!(
                std::fs::symlink_metadata(existing).is_err(),
                "broken symlink in path"
            );
            existing = existing
                .parent()
                .ok_or_else(|| anyhow::anyhow!("invalid path"))?;
        }
        anyhow::ensure!(
            existing.canonicalize()?.starts_with(&self.root),
            "symlink escapes workspace"
        );
        Ok(destination)
    }
    pub fn read(&self, path: &str, limit: usize) -> anyhow::Result<FileRead> {
        let resolved = self.resolve(path)?;
        let hash = fingerprint(&resolved)?;
        let cap = limit.clamp(1, 256 * 1024);
        let mut bytes = vec![];
        std::fs::File::open(&resolved)?
            .take((cap + 4) as u64)
            .read_to_end(&mut bytes)?;
        let truncated = std::fs::metadata(&resolved)?.len() > cap as u64;
        let content = String::from_utf8_lossy(&bytes);
        let mut end = content.len().min(cap);
        while !content.is_char_boundary(end) {
            end -= 1;
        }
        Ok(FileRead {
            path: path.into(),
            hash,
            content: content[..end].into(),
            truncated,
        })
    }
    pub fn fingerprints(&self) -> anyhow::Result<Fingerprints> {
        let mut result = Fingerprints::new();
        let entries = walkdir::WalkDir::new(&self.root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                e.depth() == 0
                    || !matches!(
                        e.file_name().to_str(),
                        Some(
                            ".git"
                                | ".weft"
                                | "target"
                                | "node_modules"
                                | ".venv"
                                | "__pycache__"
                                | ".DS_Store"
                        )
                    )
            });
        for entry in entries {
            let entry = entry?;
            if entry.file_type().is_file() {
                let name = entry
                    .path()
                    .strip_prefix(&self.root)?
                    .to_string_lossy()
                    .into_owned();
                result.insert(name, fingerprint(entry.path())?);
            } else if entry.file_type().is_symlink() {
                let name = entry
                    .path()
                    .strip_prefix(&self.root)?
                    .to_string_lossy()
                    .into_owned();
                result.insert(
                    name,
                    format!("symlink:{}", std::fs::read_link(entry.path())?.display()),
                );
            }
        }
        let mut hash = blake3::Hasher::new();
        for (path, value) in &result {
            hash.update(path.as_bytes());
            hash.update(&[0]);
            hash.update(value.as_bytes());
        }
        result.insert("$workspace".into(), hash.finalize().to_hex().to_string());
        Ok(result)
    }
}

pub fn fingerprint(path: &Path) -> anyhow::Result<String> {
    match std::fs::File::open(path) {
        Ok(mut file) => {
            let mut hash = blake3::Hasher::new();
            let mut buffer = [0; 65536];
            loop {
                let count = file.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                hash.update(&buffer[..count]);
            }
            Ok(hash.finalize().to_hex().to_string())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(MISSING.into()),
        Err(error) => Err(error.into()),
    }
}
