use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "loh",
    version,
    about = "Logit- Harness — a persistent coding agent with dynamic steering",
    subcommand_precedence_over_arg = true
)]
pub struct Args {
    #[arg(long, global = true)]
    pub home: Option<PathBuf>,
    #[arg(long, global = true)]
    pub cwd: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Option<Command>,
    pub prompt: Option<String>,
}
#[derive(Debug, Subcommand)]
pub enum Command {
    Run {
        prompt: String,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        detach: bool,
    },
    Sessions {
        #[arg(long)]
        json: bool,
    },
    Attach {
        id: String,
        #[arg(long, default_value_t = 0)]
        after: u64,
        #[arg(long)]
        json: bool,
    },
    Pause {
        id: String,
    },
    Resume {
        id: String,
    },
    Stop {
        id: String,
    },
    Doctor {
        #[arg(long)]
        json: bool,
    },
    Eval {
        #[arg(default_value = "status")]
        action: String,
    },
    #[command(name = "_serve", hide = true)]
    Serve {
        id: String,
    },
}
