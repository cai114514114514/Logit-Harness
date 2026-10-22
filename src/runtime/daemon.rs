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
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt, unix::process::CommandExt},
    path::Path,
    process::Stdio,
    sync::Arc,
};

pub async fn serve(paths: Paths, id: &str) -> anyhow::Result<()> {
    paths.prepare()?;
    let mut config = Config::load(&paths.home)?;
    let store = Arc::new(Store::open(&paths.database())?);
    let mut session: Session = store.load(id)?;
    if let Some(defaults) = &session.defaults {
        defaults.apply(&mut config);
    } else {
        session.defaults = Some(crate::config::defaults::Snapshot::capture(&config));
    }
    if let Some(spec) = &session.evaluation {
        config.model = spec.protocol.model.clone();
        config.workers = spec.protocol.workers;
        config.max_output = spec.protocol.max_output;
        config.context_bytes = spec.protocol.context_bytes;
        config.user_skills = spec.protocol.user_skills;
        config.search_limit = spec.protocol.search_limit;
        config.mcp.clear();
    }
    // One daemon per session; different chats in the same workspace coexist.
    // Source ownership is acquired by Runner only when engineering starts.
    let _daemon = session_lock(&paths, id)?;
    let key = crate::config::api::key(&paths, &config.base_url)?
        .ok_or_else(|| anyhow::anyhow!("API Key 未配置，请运行 loh setup"))?;
    let mut provider: Arc<dyn crate::model::Provider> = Arc::new(DeepSeek::new(
        config.base_url.clone(),
        config.model.clone(),
        key.clone(),
    )?);
    let money = if let Some(campaign) = &session.campaign {
        let money = Arc::new(
            super::money::Money::open(
                &paths.home.join("money.sqlite"),
                &campaign.name,
                campaign.limit,
                &campaign.prices,
            )?
            .for_session(id),
        );
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
    let mcp = Arc::new(crate::extensions::mcp::Registry::new(
        config.mcp.clone(),
        session.workspace.clone(),
        paths.logs(id)?.join("mcp"),
    ));
    mcp.recover().await?;
    for name in &session.mcp_disabled {
        if mcp.names().contains(&name.as_str()) {
            mcp.control(name, crate::extensions::mcp::Operation::Disable)
                .await?;
        }
    }
    for tool in crate::extensions::mcp::tools(mcp.clone()) {
        tools.register(tool)?;
    }
    tools.mcp = Some(mcp.clone());
    let mut skill_roots = vec![];
    if config.user_skills {
        skill_roots.push(paths.home.join("skills"));
        if let Some(user) = directories::BaseDirs::new() {
            skill_roots.push(user.home_dir().join(".agents/skills"));
        }
    }
    let knowledge = Arc::new(crate::extensions::knowledge::Knowledge::new(
        session.workspace.clone(),
        skill_roots,
    )?);
    for tool in crate::extensions::skills::tools(knowledge.clone()) {
        tools.register(tool)?;
    }
    tools.knowledge = Some(knowledge);
    let mut web = WebClient::new(
        config.base_url.clone(),
        key,
        config.search_limit,
        WebUsage::default(),
    )?
    .with_usage_file(paths.logs(id)?.join("web-usage.json"))?;
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
    let runner = runner.recover_on_start();
    let state: Session = store.load(id)?;
    let runner = if let Some(spec) = state.evaluation {
        let observer = Arc::new(crate::eval::correction::Correction::new(
            spec,
            id.into(),
            store.clone(),
        ));
        observer.recover()?;
        runner.observe(observer)
    } else {
        runner
    };
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
    mcp.close().await?;
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
    launch_with(paths, id, &std::env::current_exe()?).await
}

pub async fn resume(paths: &Paths, id: &str) -> anyhow::Result<()> {
    let store = Store::open(&paths.database())?;
    // Resume is idempotent. Save it before startup: a Stopped runner otherwise
    // exits before a client can reach the socket and deliver the instruction.
    let sequence = store.enqueue(id, &serde_json::to_value(Control::Resume)?)?;
    for _ in 0..3 {
        launch(paths, id).await?;
        for _ in 0..200 {
            if store.load::<Session>(id)?.cursor >= sequence {
                return Ok(());
            }
            if tokio::net::UnixStream::connect(paths.socket(id)?)
                .await
                .is_err()
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        // An old server can exit just after launch observed its socket. Retry
        // startup of the same session, without enqueueing a second instruction.
    }
    anyhow::bail!("resume intent saved but not acknowledged; inspect the session before retrying")
}

pub async fn launch_with(paths: &Paths, id: &str, executable: &Path) -> anyhow::Result<()> {
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
    let mut command = std::process::Command::new(executable);
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

fn session_lock(paths: &Paths, id: &str) -> anyhow::Result<std::fs::File> {
    let logs = paths.logs(id)?;
    std::fs::create_dir_all(&logs)?;
    let handle = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(logs.join("daemon.lock"))?;
    let result = unsafe { libc::flock(handle.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    anyhow::ensure!(result == 0, "this session already has a live daemon");
    Ok(handle)
}
