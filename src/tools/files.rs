use crate::domain::Fingerprints;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};

pub const MISSING: &str = "missing";

/// Snapshot key under which `Workspace::fingerprints` stores the aggregate hash
/// of the whole tree. Referenced from host, builtin and search code; the string
/// itself is part of the result contract, only its spelling is centralized here.
pub const WORKSPACE_FINGERPRINT_KEY: &str = "$workspace";

/// Hard ceiling for a single file read, regardless of the requested limit.
pub const MAX_READ_BYTES: usize = 256 * 1024;

/// Bytes read past the limit so truncation can back off to a UTF-8 char boundary
/// (a scalar value is at most 4 bytes) without splitting a multi-byte character.
const UTF8_BOUNDARY_SLACK: usize = 4;

/// Hashing and copying chunk size for file contents.
const HASH_CHUNK: usize = 65536;

/// Components that must never be reached through file tools: edit admission
/// control. Deliberately narrower than SNAPSHOT_NOISE — inspecting build output
/// is legitimate, mutating internal state is not.
const EDIT_DENIED: [&str; 2] = [".git", ".loh"];

/// Entries excluded from workspace fingerprints: snapshot noise. Build caches and
/// OS droppings change constantly and would mark every observation stale without
/// carrying signal. Unlike EDIT_DENIED these stay reachable through file tools.
const SNAPSHOT_NOISE: [&str; 8] = [
    ".loh-build",
    ".git",
    ".loh",
    "target",
    "node_modules",
    ".venv",
    "__pycache__",
    ".DS_Store",
];

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
    /// Exact line counts for display; absent in older results and when the
    /// difference was too large to measure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removed: Option<u64>,
}

impl Workspace {
    pub fn open(root: &Path) -> anyhow::Result<Self> {
        Ok(Self {
            root: root.canonicalize()?,
        })
    }
    pub fn resolve(&self, path: &str) -> anyhow::Result<PathBuf> {
        use std::path::Component;
        anyhow::ensure!(
            !path.is_empty() && path != ".",
            "path must name an entry inside the workspace"
        );
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
                !EDIT_DENIED.iter().any(|denied| part.as_os_str() == *denied),
                "internal state is not editable through file tools"
            );
        }
        let destination = self.root.join(relative);
        // The destination may not exist yet (a create). Walk up to the nearest
        // existing ancestor and canonicalize that: any symlink along the way then
        // resolves to its real location, which must stay inside the workspace.
        let mut existing = destination.as_path();
        while !existing.exists() {
            // A dangling symlink must not be treated as a new file. Only NotFound
            // means "keep walking"; permission and other I/O errors propagate
            // instead of being silently read as "missing".
            match std::fs::symlink_metadata(existing) {
                Ok(_) => anyhow::bail!("broken symlink in path"),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                Err(error) => return Err(error.into()),
            }
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
        let cap = limit.clamp(1, MAX_READ_BYTES);
        // Hash, content and truncation flag must describe one version of the file:
        // derive all three from a single open descriptor instead of independent
        // accesses that could observe different contents under a concurrent writer.
        let mut file = std::fs::File::open(&resolved)?;
        let truncated = file.metadata()?.len() > cap as u64;
        let mut hash = blake3::Hasher::new();
        let mut bytes = vec![];
        let mut chunk = [0; HASH_CHUNK];
        loop {
            let count = file.read(&mut chunk)?;
            if count == 0 {
                break;
            }
            hash.update(&chunk[..count]);
            let slack = cap + UTF8_BOUNDARY_SLACK;
            if bytes.len() < slack {
                bytes.extend_from_slice(&chunk[..count.min(slack - bytes.len())]);
            }
        }
        let content = String::from_utf8_lossy(&bytes);
        let mut end = content.len().min(cap);
        while !content.is_char_boundary(end) {
            end -= 1;
        }
        Ok(FileRead {
            path: path.into(),
            hash: hash.finalize().to_hex().to_string(),
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
                    || !e
                        .file_name()
                        .to_str()
                        .is_some_and(|name| SNAPSHOT_NOISE.contains(&name))
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
                // The link target is recorded as the fingerprint so retargeting is
                // observable, but a symlink alone never raises an alarm here.
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
        result.insert(
            WORKSPACE_FINGERPRINT_KEY.into(),
            hash.finalize().to_hex().to_string(),
        );
        Ok(result)
    }

    /// Explicitly observed files remain dependencies even inside excluded build
    /// directories. Keep them separate from the aggregate source-tree digest.
    pub fn snapshot_for<'a>(
        &self,
        paths: impl IntoIterator<Item = &'a String>,
    ) -> anyhow::Result<Fingerprints> {
        let mut snapshot = self.fingerprints()?;
        for path in paths {
            if path != WORKSPACE_FINGERPRINT_KEY {
                snapshot.insert(path.clone(), fingerprint(&self.resolve(path)?)?);
            }
        }
        Ok(snapshot)
    }
}

pub fn fingerprint(path: &Path) -> anyhow::Result<String> {
    match std::fs::File::open(path) {
        Ok(mut file) => {
            let mut hash = blake3::Hasher::new();
            let mut buffer = [0; HASH_CHUNK];
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
