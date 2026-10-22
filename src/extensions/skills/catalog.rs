use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read},
    os::unix::fs::MetadataExt,
    path::{Component, Path, PathBuf},
};

const FILE_LIMIT: u64 = 256 * 1024;

#[derive(Clone, Debug, Serialize)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub source: String,
    pub path: PathBuf,
    pub hash: String,
    /// Discovery fingerprints only metadata and file identity, not the body.
    #[serde(skip)]
    pub(super) stamp: String,
}

#[derive(Debug, Serialize)]
pub struct Loaded {
    pub path: PathBuf,
    pub hash: String,
    pub content: String,
}

#[derive(Default)]
pub struct Skills {
    pub entries: Vec<Entry>,
    pub warnings: Vec<String>,
    roots: BTreeMap<String, PathBuf>,
}

#[derive(Deserialize)]
struct Metadata {
    name: String,
    description: String,
}

impl Skills {
    pub fn discover(workspace: &Path, user_roots: &[PathBuf]) -> anyhow::Result<Self> {
        let mut catalog = Self {
            entries: vec![],
            warnings: vec![],
            roots: BTreeMap::new(),
        };
        // An explicitly configured user root keeps its provenance even when a
        // test/custom state home happens to be nested inside the workspace.
        let roots = user_roots
            .iter()
            .map(|root| (root.as_path(), "user"))
            .chain(std::iter::once((workspace, "project")));
        let mut seen = std::collections::BTreeSet::new();
        for (root, source) in roots {
            if !root.is_dir() {
                continue;
            }
            let root = root.canonicalize()?;
            let mut count = 0;
            for entry in walkdir::WalkDir::new(&root)
                .follow_links(false)
                .into_iter()
                .filter_entry(|entry| visible(entry.file_name().to_str().unwrap_or("")))
            {
                count += 1;
                if count > 30_000 {
                    catalog
                        .warnings
                        .push(format!("Discovery entry limit reached: {}", root.display()));
                    break;
                }
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) => {
                        catalog.warnings.push(error.to_string());
                        continue;
                    }
                };
                if !entry.file_type().is_file() || entry.file_name() != "SKILL.md" {
                    continue;
                }
                let path = entry.path().canonicalize()?;
                if !seen.insert(path.clone()) {
                    continue;
                }
                match parse(&path, source) {
                    Ok(item) => {
                        catalog
                            .roots
                            .insert(item.id.clone(), path.parent().unwrap().to_owned());
                        catalog.entries.push(item);
                    }
                    Err(error) => catalog
                        .warnings
                        .push(format!("{}: {error}", path.display())),
                }
            }
        }
        catalog
            .entries
            .sort_by(|a, b| (&a.source, &a.name, &a.path).cmp(&(&b.source, &b.name, &b.path)));
        Ok(catalog)
    }

    pub fn load(&self, id: &str, resource: Option<&str>) -> anyhow::Result<Loaded> {
        if let Some(loaded) = super::bundled::load(id, resource)? {
            return Ok(loaded);
        }
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.id == id)
            .ok_or_else(|| anyhow::anyhow!("unknown skill ID; use skills_list"))?;
        let root = &self.roots[id];
        anyhow::ensure!(
            stamp(&entry.path)? == entry.stamp,
            "skill changed since discovery; refresh skills_list before loading"
        );
        let relative = Path::new(resource.unwrap_or("SKILL.md"));
        anyhow::ensure!(
            !relative.as_os_str().is_empty()
                && relative
                    .components()
                    .all(|part| matches!(part, Component::Normal(_) | Component::CurDir)),
            "skill resource must stay inside its directory"
        );
        let path = root.join(relative);
        let mut prefix = root.clone();
        for part in relative.components() {
            prefix.push(part);
            anyhow::ensure!(
                !std::fs::symlink_metadata(&prefix)?.file_type().is_symlink(),
                "symlinked skill resources are not loaded"
            );
        }
        let path = path.canonicalize()?;
        anyhow::ensure!(
            path.starts_with(root),
            "skill resource escaped its directory"
        );
        let content = read(&path)?;
        anyhow::ensure!(
            stamp(&entry.path)? == entry.stamp,
            "skill changed during load; refresh skills_list"
        );
        Ok(Loaded {
            hash: blake3::hash(content.as_bytes()).to_hex().to_string(),
            path,
            content,
        })
    }
}

fn parse(path: &Path, source: &str) -> anyhow::Result<Entry> {
    let before = stamp(path)?;
    let mut reader = BufReader::with_capacity(512, std::fs::File::open(path)?.take(16 * 1024 + 1));
    let mut line = String::new();
    reader.read_line(&mut line)?;
    anyhow::ensure!(
        line.trim_end() == "---",
        "SKILL.md requires YAML frontmatter"
    );
    let mut header = String::new();
    loop {
        line.clear();
        anyhow::ensure!(
            reader.read_line(&mut line)? > 0,
            "unterminated or oversized skill metadata"
        );
        if line.trim_end() == "---" {
            break;
        }
        header.push_str(&line);
        anyhow::ensure!(header.len() <= 16 * 1024, "skill metadata too large");
    }
    let metadata: Metadata = serde_saphyr::from_str(&header)?;
    let name = &metadata.name;
    anyhow::ensure!(
        !name.is_empty()
            && name.len() <= 64
            && name
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
            && !name.starts_with('-')
            && !name.ends_with('-')
            && !name.contains("--"),
        "invalid skill name"
    );
    anyhow::ensure!(
        path.parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            == Some(name),
        "skill name must match its directory"
    );
    anyhow::ensure!(
        !metadata.description.trim().is_empty() && metadata.description.chars().count() <= 1024,
        "skill description must contain 1..1024 characters"
    );
    Ok(Entry {
        id: blake3::hash(path.as_os_str().as_encoded_bytes()).to_hex()[..24].to_owned(),
        name: metadata.name,
        description: metadata.description,
        source: source.into(),
        path: path.to_owned(),
        hash: blake3::hash(header.as_bytes()).to_hex().to_string(),
        stamp: before,
    })
}

fn stamp(path: &Path) -> anyhow::Result<String> {
    let metadata = std::fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "symlinked skill resources are not loaded"
    );
    Ok(format!(
        "{}:{}:{}:{}:{}:{}:{}",
        metadata.dev(),
        metadata.ino(),
        metadata.len(),
        metadata.mtime(),
        metadata.mtime_nsec(),
        metadata.ctime(),
        metadata.ctime_nsec()
    ))
}

pub(crate) fn read(path: &Path) -> anyhow::Result<String> {
    let mut content = String::new();
    std::fs::File::open(path)?
        .take(FILE_LIMIT + 1)
        .read_to_string(&mut content)?;
    anyhow::ensure!(
        content.len() as u64 <= FILE_LIMIT,
        "instruction file exceeds 256 KiB"
    );
    Ok(content)
}

fn visible(name: &str) -> bool {
    !matches!(
        name,
        ".git" | ".loh" | ".loh-build" | "node_modules" | "target" | ".venv" | "__pycache__"
    )
}
