use super::WebUsage;
use std::{io::Write, path::PathBuf, sync::Mutex};

/// The daemon owns one quota per session; workers share it. Persist before dispatch so a crash
/// can consume an uncertain attempt, but can never silently restore network allowance.
pub(super) struct Quota {
    usage: Mutex<WebUsage>,
    limit: u32,
    path: Option<PathBuf>,
}
impl Quota {
    pub fn new(limit: u32, usage: WebUsage) -> Self {
        Self {
            usage: Mutex::new(usage),
            limit,
            path: None,
        }
    }
    pub fn attach(&mut self, path: PathBuf) -> anyhow::Result<()> {
        if path.exists() {
            let saved: WebUsage = serde_json::from_slice(&std::fs::read(&path)?)?;
            let usage = self
                .usage
                .get_mut()
                .map_err(|_| anyhow::anyhow!("quota lock poisoned"))?;
            usage.searches = usage.searches.max(saved.searches);
            usage.fetches = usage.fetches.max(saved.fetches);
        }
        self.path = Some(path);
        Ok(())
    }
    pub fn usage(&self) -> WebUsage {
        self.usage
            .lock()
            .expect("recorded usage cannot be lost to a poisoned lock")
            .clone()
    }
    pub fn consume(&self, search: bool) -> anyhow::Result<()> {
        let mut current = self
            .usage
            .lock()
            .map_err(|_| anyhow::anyhow!("quota lock poisoned"))?;
        let mut updated = current.clone();
        if search {
            anyhow::ensure!(updated.searches < self.limit, "search limit exhausted");
            updated.searches += 1;
        } else {
            anyhow::ensure!(
                updated.fetches < self.limit.saturating_mul(2),
                "fetch limit exhausted"
            );
            updated.fetches += 1;
        }
        if let Some(path) = &self.path {
            let parent = path
                .parent()
                .ok_or_else(|| anyhow::anyhow!("quota path needs a parent"))?;
            std::fs::create_dir_all(parent)?;
            let mut file = tempfile::NamedTempFile::new_in(parent)?;
            file.write_all(&serde_json::to_vec(&updated)?)?;
            file.as_file().sync_all()?;
            file.persist(path)?;
            std::fs::File::open(parent)?.sync_all()?;
        }
        *current = updated;
        Ok(())
    }
}
