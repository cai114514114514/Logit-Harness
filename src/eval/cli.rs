use super::manifest::{Frozen, Manifest};
use clap::Subcommand;
use std::path::PathBuf;

#[derive(Debug, Subcommand)]
pub enum Command {
    Freeze {
        #[arg(long, default_value = "eval/corpus")]
        corpus: PathBuf,
        #[arg(long, default_value = "eval/frozen.json")]
        output: PathBuf,
    },
    Status {
        #[arg(long, default_value = "eval/corpus")]
        corpus: PathBuf,
        #[arg(long, default_value = "eval/frozen.json")]
        frozen: PathBuf,
    },
}

pub fn run(command: Option<Command>) -> anyhow::Result<()> {
    match command.unwrap_or(Command::Status {
        corpus: "eval/corpus".into(),
        frozen: "eval/frozen.json".into(),
    }) {
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
