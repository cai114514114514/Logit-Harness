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
use std::{io::Write, path::PathBuf};

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
    let cwd = args
        .cwd
        .unwrap_or(std::env::current_dir()?)
        .canonicalize()?;
    let command = if let Some(command) = args.command {
        command
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
    match command {
        Command::Doctor { json } => {
            let result = json!({"model":config.model,"base_url":config.base_url,"api_key_present":std::env::var("DEEPSEEK_API_KEY").is_ok_and(|v|!v.trim().is_empty()),
                "chrome_present":config.chrome.is_file(),"service_sandbox_present":std::path::Path::new("/usr/bin/sandbox-exec").is_file(),
                "paid_probe_performed":false,"state_directory":paths.home,"platform":std::env::consts::OS});
            if json {
                println!("{result}");
            } else {
                println!(
                    "Logit- Harness · {}\nAPI key: {}\nChrome: {}\nState: {}",
                    config.model,
                    if result["api_key_present"] == true {
                        "configured"
                    } else {
                        "set DEEPSEEK_API_KEY"
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
            let summaries:Vec<_>=rows.into_iter().map(|(id,state)|json!({"id":id,"status":state["status"],"objective":state["goal"]["original"],"workspace":state["workspace"]})).collect();
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
            paths.prepare()?;
            let store = Store::open(&paths.database())?;
            let mut session = Session::new(cwd, &prompt, config.tokens, config.seconds);
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
        Command::Attach { id, after, json } => {
            let id = resolve(&paths, &id)?;
            super::stream::attach(&paths, &id, after, json).await?;
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
            daemon::launch(&paths, &id).await?;
            daemon::command(&paths, &id, Control::Resume).await?;
            println!("resume requested: {id}");
        }
        Command::Eval { command } => crate::eval::cli::run(command)?,
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
