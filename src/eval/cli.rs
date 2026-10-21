use super::manifest::{Frozen, Manifest};
use clap::Subcommand;
use std::path::PathBuf;

#[derive(Debug, Subcommand)]
pub enum Command {
    Run {
        #[command(flatten)]
        options: super::run::Options,
    },
    Report {
        #[arg(long, default_value = "artifacts/evaluation/development")]
        output: PathBuf,
        #[arg(long, default_value = "eval/corpus")]
        corpus: PathBuf,
        #[arg(long, default_value = "eval/registered.json")]
        frozen: PathBuf,
    },
    Freeze {
        #[arg(long, default_value = "eval/corpus")]
        corpus: PathBuf,
        #[arg(long, default_value = "eval/registered.json")]
        output: PathBuf,
    },
    Status {
        #[arg(long, default_value = "eval/corpus")]
        corpus: PathBuf,
        #[arg(long, default_value = "eval/registered.json")]
        frozen: PathBuf,
    },
}

pub async fn run(
    command: Option<Command>,
    paths: &crate::config::Paths,
    config: &crate::config::Config,
) -> anyhow::Result<()> {
    match command.unwrap_or(Command::Status {
        corpus: "eval/corpus".into(),
        frozen: "eval/registered.json".into(),
    }) {
        Command::Run { options } => super::run::run(options, paths.clone(), config.clone()).await?,
        Command::Report {
            output,
            frozen,
            corpus,
        } => {
            let frozen = Frozen::load(&frozen)?;
            frozen.verify(&corpus)?;
            super::report::write(&output, &frozen, paths)?;
            println!("{}", output.join("report.md").display());
        }
        Command::Freeze { corpus, output } => {
            let frozen = Manifest::load(&corpus)?.freeze(&output)?;
            println!(
                "{}",
                serde_json::json!({"frozen":output,"digest":frozen.digest,"tasks":frozen.tasks.len(),"created":frozen.created})
            );
        }
        Command::Status { corpus, frozen } => {
            if !frozen.exists() {
                println!(
                    "{}",
                    serde_json::json!({"frozen":false,"paid_trials_started":false})
                );
                return Ok(());
            }
            let manifest = Frozen::load(&frozen)?;
            manifest.verify(&corpus)?;
            println!(
                "{}",
                serde_json::json!({"frozen":true,"digest":manifest.digest,"tasks":manifest.tasks.len(),"schedule_trials":manifest.tasks.len()*((manifest.protocol.conditions.len()+manifest.protocol.ablations.len())*manifest.protocol.repetitions as usize)})
            );
        }
    }
    Ok(())
}
