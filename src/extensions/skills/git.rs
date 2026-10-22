//! A disposable Git directory receives data, never executable project config.
use std::{io::Read, path::Path};

pub(super) struct Inspection {
    pub directory: tempfile::TempDir,
}
impl Inspection {
    pub fn new(root: &Path) -> anyhow::Result<Self> {
        let dot = root.join(".git");
        let gitdir = if dot.is_dir() {
            dot.canonicalize()?
        } else {
            let text = read(&dot, 4096)?;
            let target = text
                .trim()
                .strip_prefix("gitdir: ")
                .ok_or_else(|| anyhow::anyhow!("unsupported .git file; expected gitdir pointer"))?;
            root.join(target).canonicalize()?
        };
        let common = if gitdir.join("commondir").exists() {
            gitdir
                .join(read(&gitdir.join("commondir"), 4096)?.trim())
                .canonicalize()?
        } else {
            gitdir.clone()
        };
        let config = read(&common.join("config"), 1024 * 1024)?;
        validate_format(&config)?;
        let directory = tempfile::tempdir()?;
        let destination = directory.path();
        std::fs::create_dir(destination.join("refs"))?;
        std::fs::write(
            destination.join("config"),
            "[core]\nrepositoryformatversion = 0\nbare = false\nfsmonitor = false\n[submodule]\nrecurse = false\n",
        )?;
        copy(&gitdir.join("HEAD"), &destination.join("HEAD"), 4096)?;
        for name in ["index", "packed-refs", "shallow"] {
            let source = if name == "index" {
                gitdir.join(name)
            } else {
                common.join(name)
            };
            if source.exists() {
                copy(&source, &destination.join(name), 32 * 1024 * 1024)?;
            }
        }
        for (count, entry) in walkdir::WalkDir::new(common.join("refs"))
            .follow_links(false)
            .into_iter()
            .enumerate()
        {
            anyhow::ensure!(count < 4096, "project_diff reference limit exceeded");
            let entry = entry?;
            anyhow::ensure!(
                !entry.file_type().is_symlink(),
                "project_diff does not follow symlinked refs"
            );
            let target = destination.join(entry.path().strip_prefix(&common)?);
            if entry.file_type().is_dir() {
                std::fs::create_dir_all(target)?;
            } else {
                copy(entry.path(), &target, 16 * 1024)?;
            }
        }
        // Copy split-index companions; never share a writable original index.
        let mut shared = 0;
        for entry in std::fs::read_dir(&gitdir)? {
            let path = entry?.path();
            if path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("sharedindex."))
            {
                shared += 1;
                anyhow::ensure!(shared <= 16, "project_diff shared-index limit exceeded");
                copy(
                    &path,
                    &destination.join(path.file_name().unwrap()),
                    32 * 1024 * 1024,
                )?;
            }
        }
        std::os::unix::fs::symlink(
            common.join("objects").canonicalize()?,
            destination.join("objects"),
        )?;
        Ok(Self { directory })
    }
}
fn validate_format(config: &str) -> anyhow::Result<()> {
    let mut extensions = false;
    for line in config.lines() {
        let line = line.trim();
        if let Some(section) = line
            .strip_prefix('[')
            .and_then(|s| s.split_once(']').map(|(s, _)| s))
        {
            extensions = section.trim().eq_ignore_ascii_case("extensions");
        } else if extensions && !line.is_empty() && !line.starts_with(['#', ';']) {
            let (name, value) = line.split_once('=').unwrap_or((line, "true"));
            let name = name.trim().to_ascii_lowercase();
            let value = value
                .split(['#', ';'])
                .next()
                .unwrap_or("")
                .trim()
                .trim_matches('"');
            // Worktree-specific configuration is deliberately not loaded, but
            // its presence changes neither SHA-1 object data nor file refs.
            anyhow::ensure!(
                name == "worktreeconfig"
                    || (name == "objectformat" && value == "sha1")
                    || (name == "refstorage" && value == "files"),
                "project_diff does not support Git extension formats (such as SHA-256 or reftable)"
            );
        }
    }
    Ok(())
}
fn read(path: &Path, limit: u64) -> anyhow::Result<String> {
    Ok(String::from_utf8(bytes(path, limit)?)?)
}
fn copy(source: &Path, destination: &Path, limit: u64) -> anyhow::Result<()> {
    std::fs::write(destination, bytes(source, limit)?)?;
    Ok(())
}
fn bytes(path: &Path, limit: u64) -> anyhow::Result<Vec<u8>> {
    let metadata = std::fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "unsupported Git metadata path: {}",
        path.display()
    );
    anyhow::ensure!(
        metadata.len() <= limit,
        "Git metadata exceeds inspection limit: {}",
        path.display()
    );
    let mut data = vec![];
    std::fs::File::open(path)?
        .take(limit + 1)
        .read_to_end(&mut data)?;
    anyhow::ensure!(
        data.len() as u64 <= limit,
        "Git metadata grew beyond inspection limit"
    );
    Ok(data)
}
