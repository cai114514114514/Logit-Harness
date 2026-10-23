use crate::config::Paths;
pub async fn attach(paths: &Paths, id: &str, _after: u64) -> anyhow::Result<()> {
    let session: crate::runtime::state::Session =
        crate::store::Store::open(&paths.database())?.load(id)?;
    super::client::run(paths, &session.workspace, Some(id.into()), false).await
}
