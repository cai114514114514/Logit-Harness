use super::Recent;
use crate::{
    config::{Config, Paths},
    store::Store,
};
use std::path::Path;

pub enum Launch {
    Exit,
}
pub async fn welcome(paths: &Paths, _config: &Config, cwd: &Path) -> anyhow::Result<Launch> {
    super::client::run(paths, cwd, None, false).await?;
    Ok(Launch::Exit)
}
pub async fn configure(paths: &Paths, _config: &Config, cwd: &Path) -> anyhow::Result<()> {
    super::client::run(paths, cwd, None, true).await
}

pub(super) fn recent(paths: &Paths, cwd: &Path) -> anyhow::Result<Vec<Recent>> {
    if !paths.database().exists() {
        return Ok(vec![]);
    }
    Store::open(&paths.database())?
        .snapshots()?
        .into_iter()
        .filter(|(_, state)| state["workspace"].as_str() == cwd.to_str())
        .map(|(id, state)| {
            Ok(Recent {
                id,
                objective: state["title"]
                    .as_str()
                    .filter(|title| !title.is_empty())
                    .or_else(|| state["goal"]["original"].as_str())
                    .unwrap_or("新对话")
                    .to_owned(),
                status: serde_json::from_value(state["status"].clone())?,
            })
        })
        .collect()
}
