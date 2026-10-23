use super::args::{Args, Command};
use crate::{
    config::{Config, Paths},
    domain::EventData,
    runtime::{
        daemon,
        state::{Control, Session},
    },
    store::Store,
};
use serde_json::json;
use std::{
    io::{IsTerminal, Write},
    path::PathBuf,
};

pub async fn run(args: Args) -> anyhow::Result<()> {
    let home = args
        .home
        .or_else(|| std::env::var_os("LOH_HOME").map(PathBuf::from))
        .unwrap_or_else(|| {
            directories::BaseDirs::new()
                .expect("home directory")
                .home_dir()
                .join(".loh")
        });
    let home = if home.is_absolute() {
        home
    } else {
        std::env::current_dir()?.join(home)
    };
    let paths = Paths::new(home);
    let config = Config::load(&paths.home)?;
    let cwd = if let Some(Command::Attach { id, .. }) = args.command.as_ref() {
        // History remains attachable even when the original workspace no longer
        // exists. The caller's shell directory never defines session commands.
        let id = resolve(&paths, id)?;
        Store::open(&paths.database())?
            .load::<Session>(&id)?
            .workspace
    } else {
        args.cwd
            .unwrap_or(std::env::current_dir()?)
            .canonicalize()?
    };
    let command = if let Some(command) = args.command {
        command
    } else if args.prompt.is_none()
        && std::io::stdin().is_terminal()
        && std::io::stdout().is_terminal()
    {
        match super::terminal::welcome(&paths, &config, &cwd).await? {
            super::terminal::Launch::Exit => return Ok(()),
        }
    } else {
        let prompt = if let Some(prompt) = args.prompt {
            prompt
        } else {
            print!("任务 › ");
            std::io::stdout().flush()?;
            let mut prompt = String::new();
            std::io::stdin().read_line(&mut prompt)?;
            prompt.trim().into()
        };
        if prompt.is_empty() {
            return Ok(());
        }
        Command::Run {
            prompt,
            json: false,
            detach: false,
        }
    };
    // /setup may have changed the endpoint while the welcome screen was open.
    let config = Config::load(&paths.home)?;
    match command {
        Command::Setup => {
            anyhow::ensure!(
                std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
                "loh setup 需要交互终端；也可使用 DEEPSEEK_API_KEY 环境变量"
            );
            super::terminal::configure(&paths, &config, &cwd).await?;
        }
        Command::Doctor { json } => {
            let result = crate::commands::doctor(&paths, &config)?;
            if json {
                println!("{result}");
            } else {
                println!(
                    "Logit- Harness · {}\nAPI key: {}\nChrome: {}\nState: {}",
                    config.model,
                    if result["api_key_present"] == true {
                        "configured"
                    } else {
                        "run loh setup"
                    },
                    if config.chrome.is_file() {
                        "available"
                    } else {
                        "not found"
                    },
                    paths.home.display()
                );
            }
        }
        Command::Sessions { json } => {
            let rows = if paths.database().exists() {
                Store::open(&paths.database())?.snapshots()?
            } else {
                vec![]
            };
            let summaries: Vec<_> = rows.into_iter().map(|(id, state)| {
                let title = state["title"].as_str().filter(|title| !title.is_empty())
                    .or_else(|| state["goal"]["original"].as_str().filter(|title| !title.is_empty())).unwrap_or("新对话");
                json!({"id":id,"status":state["status"],"objective":title,"workspace":state["workspace"]})
            }).collect();
            if json {
                println!("{}", serde_json::to_string(&summaries)?);
            } else {
                for row in summaries {
                    println!(
                        "{}  {}  {}",
                        row["id"].as_str().unwrap_or(""),
                        row["status"].as_str().unwrap_or(""),
                        row["objective"].as_str().unwrap_or("")
                    );
                }
            }
        }
        Command::Run {
            prompt,
            json,
            detach,
        } => {
            anyhow::ensure!(
                crate::config::api::configured(&paths, &config.base_url)?,
                "API Key 未配置，请运行 loh setup"
            );
            paths.prepare()?;
            let store = Store::open(&paths.database())?;
            let mut session = Session::conversation(cwd, &prompt, config.tokens, config.seconds)
                .with_defaults(&config);
            session.campaign = config.campaign.clone();
            store.commit(
                &session.id,
                &session,
                EventData::Created {
                    objective: prompt,
                    workspace: session.workspace.display().to_string(),
                },
            )?;
            daemon::launch(&paths, &session.id).await?;
            if detach {
                println!(
                    "{}",
                    json!({"id":session.id,"socket":paths.socket(&session.id)?,"detached":true})
                );
            } else {
                super::stream::attach(&paths, &session.id, 0, json).await?;
            }
        }
        Command::Serve { id } => daemon::serve(paths, &id).await?,
        Command::Attach {
            id,
            agent,
            after,
            json,
        } => {
            let id = resolve(&paths, &id)?;
            let mut state: Session = Store::open(&paths.database())?.load(&id)?;
            state.normalize_team();
            let target = state.resolve_view(&agent)?;
            super::stream::attach_view(&paths, &id, after, json, target).await?;
        }
        Command::Pause { id } => {
            let id = resolve(&paths, &id)?;
            daemon::command(&paths, &id, Control::Pause).await?;
            println!("pause requested: {id}");
        }
        Command::Stop { id } => {
            let id = resolve(&paths, &id)?;
            daemon::command(&paths, &id, Control::Stop).await?;
            println!("stop requested: {id}");
        }
        Command::Resume { id } => {
            let id = resolve(&paths, &id)?;
            daemon::resume(&paths, &id).await?;
            println!("resume requested: {id}");
        }
        Command::Eval { command } => crate::eval::cli::run(command, &paths, &config).await?,
    }
    Ok(())
}

fn resolve(paths: &Paths, id: &str) -> anyhow::Result<String> {
    let store = Store::open(&paths.database())?;
    let matches: Vec<_> = store
        .snapshots()?
        .into_iter()
        .map(|(key, _)| key)
        .filter(|key| key.starts_with(id))
        .collect();
    anyhow::ensure!(
        matches.len() == 1,
        "session prefix must identify exactly one saved session"
    );
    Ok(matches[0].clone())
}
