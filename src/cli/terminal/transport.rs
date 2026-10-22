use crate::config::Paths;
pub async fn attach(paths: &Paths, id: &str, _after: u64) -> anyhow::Result<()> {
    super::client::run(paths, &std::env::current_dir()?, Some(id.into()), false).await
}
