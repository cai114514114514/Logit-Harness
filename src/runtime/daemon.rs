//! The runner's lifetime is independent from terminal clients.
use super::{
    ipc::{self, ClientMessage, ServerMessage},
    runner::{Runner, Settings},
    state::{Control, Session},
};
use crate::{
    config::{Config, Paths},
    domain::{EventData, Status},
    model::DeepSeek,
    store::Store,
    tools::{
        ToolHost,
        files::Workspace,
        web::{WebClient, WebUsage},
    },
};
use std::{
    io::Write,
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt, unix::process::CommandExt},
    path::Path,
    process::Stdio,
    sync::Arc,
};

pub async fn serve(paths: Paths, id: &str) -> anyhow::Result<()> {
    paths.prepare()?;
    let config = Config::load(&paths.home)?;
    let store = Arc::new(Store::open(&paths.database())?);
    let mut session: Session = store.load(id)?;
    let _owner = workspace_lock(&session.workspace, id)?;
    super::recovery::reconcile(&mut session, &store, &paths.logs(id)?).await?;
    let key = std::env::var("DEEPSEEK_API_KEY")
        .map_err(|_| anyhow::anyhow!("DEEPSEEK_API_KEY is missing"))?;
    let mut provider: Arc<dyn crate::model::Provider> = Arc::new(DeepSeek::new(
        config.base_url.clone(),
        config.model.clone(),
        key.clone(),
    )?);
    let money = if let Some(campaign) = &session.campaign {
        let money = Arc::new(super::money::Money::open(
            &paths.home.join("money.sqlite"),
            &campaign.name,
            campaign.limit,
            &campaign.prices,
        )?);
        provider = Arc::new(super::meter::Metered::new(
            provider,
            money.clone(),
            campaign.prices.clone(),
        ));
        Some(money)
    } else {
        None
    };
    let mut tools = ToolHost::new(
        Workspace::open(&session.workspace)?,
        Arc::new(tokio::sync::RwLock::new(session.goal.clone())),
        paths.logs(id)?,
    );
    let services = Arc::new(crate::tools::services::Services::new(
        tools.workspace.clone(),
        paths.logs(id)?.join("services"),
    ));
    services.recover().await?;
    for tool in crate::tools::services::tools(services.clone()) {
        tools.register(tool)?;
    }
    tools.services = Some(services.clone());
    let browser = Arc::new(crate::tools::browser::BrowserClient::new(
        config.chrome.clone(),
        paths.logs(id)?.join("browser"),
    ));
    browser.recover().await?;
    for tool in crate::tools::browser::tools(browser.clone()) {
        tools.register(tool)?;
    }
    let mut web = WebClient::new(
        config.base_url.clone(),
        key,
        config.search_limit,
        WebUsage::default(),
    )?;
    if let (Some(money), Some(campaign)) = (money, &session.campaign) {
        web = web.with_money(money, campaign.prices.clone());
    }
    let web = Arc::new(web);
    for tool in crate::tools::web::tools(web) {
        tools.register(tool)?;
    }
    let settings = Settings {
        workers: config.workers,
        max_output: config.max_output,
        context_limit: config.context_bytes,
        linger: true,
    };
    let (runner, control, _events) =
        Runner::new(session, store.clone(), provider, Arc::new(tools), settings);
    let server = ipc::Server::bind(
        paths.socket(id)?,
        id.into(),
        store.clone(),
        control.clone(),
        runner.events.clone(),
    )
    .await?;
    let cancel = tokio_util::sync::CancellationToken::new();
    let mut server_task = tokio::spawn(server.run(cancel.clone()));
    let mut run_task = tokio::spawn(runner.run());
    let mut termination =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let result = tokio::select! {
        result=&mut run_task=>result?,
        result=&mut server_task=>{
            control.send(Control::Stop).await?;let _=run_task.await;
            result??;anyhow::bail!("socket server ended unexpectedly")
        }
        _=termination.recv()=>{control.send(Control::Stop).await?;run_task.await?}
        _=tokio::signal::ctrl_c()=>{control.send(Control::Stop).await?;run_task.await?}
    };
    cancel.cancel();
    services.shutdown().await?;
    browser.close().await?;
    let _ = server_task.await;
    if let Err(error) = result {
        let mut state: Session = store.load(id)?;
        state.status = Status::Failed;
        store.commit(
            id,
            &state,
            EventData::Diagnostic {
                message: format!("runner failed: {error}"),
            },
        )?;
        return Err(error);
    }
    Ok(())
}

pub async fn launch(paths: &Paths, id: &str) -> anyhow::Result<()> {
    let socket = paths.socket(id)?;
    if tokio::net::UnixStream::connect(&socket).await.is_ok() {
        return Ok(());
    }
    let logs = paths.logs(id)?;
    std::fs::create_dir_all(&logs)?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(logs.join("runner.log"))?;
    let mut command = std::process::Command::new(std::env::current_exe()?);
    command
        .args([
            "--home",
            paths
                .home
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("state path must be UTF-8"))?,
            "_serve",
            id,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log));
    // setsid is async-signal-safe; no Rust allocation happens inside the post-fork hook.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    let mut child = command.spawn()?;
    for _ in 0..200 {
        if tokio::net::UnixStream::connect(&socket).await.is_ok() {
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            let text = std::fs::read_to_string(logs.join("runner.log")).unwrap_or_default();
            anyhow::bail!(
                "runner exited ({status}): {}",
                text.chars()
                    .rev()
                    .take(2000)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect::<String>()
            );
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    anyhow::bail!(
        "runner is still starting; inspect {} and attach to {id}",
        logs.join("runner.log").display()
    )
}

pub async fn command(paths: &Paths, id: &str, command: Control) -> anyhow::Result<()> {
    match ipc::request(&paths.socket(id)?, ClientMessage::Command { command }).await? {
        ServerMessage::Ack => Ok(()),
        ServerMessage::Error { message } => anyhow::bail!(message),
        _ => anyhow::bail!("unexpected control response"),
    }
}

fn workspace_lock(workspace: &Path, id: &str) -> anyhow::Result<std::fs::File> {
    let path = workspace.canonicalize()?;
    let hash = blake3::hash(path.as_os_str().as_encoded_bytes());
    let file = std::path::PathBuf::from(format!(
        "/tmp/weft-owner-{}-{}.lock",
        unsafe { libc::geteuid() },
        &hash.to_hex()[..24]
    ));
    let mut handle = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(file)?;
    let result = unsafe { libc::flock(handle.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    anyhow::ensure!(
        result == 0,
        "another runner owns this workspace; attach to that session first"
    );
    handle.set_len(0)?;
    handle.write_all(id.as_bytes())?;
    handle.sync_all()?;
    Ok(handle)
}
