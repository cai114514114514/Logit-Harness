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
    /// Configure the API interactively; secrets are never command arguments.
    Setup,
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
        #[arg(long, default_value = "main")]
        agent: String,
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
        #[command(subcommand)]
        command: Option<crate::eval::cli::Command>,
    },
    #[command(name = "_serve", hide = true)]
    Serve {
        id: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn attach_captures_member_without_reinterpreting_other_workspace() {
        let args = Args::try_parse_from([
            "loh",
            "--home",
            "/state home",
            "--cwd",
            "/other workspace",
            "attach",
            "abc123",
            "--agent",
            "member-123",
            "--after",
            "42",
            "--json",
        ])
        .unwrap();
        assert!(
            matches!(args.command, Some(Command::Attach { id, agent, after:42, json:true }) if id=="abc123" && agent=="member-123")
        );
        assert_eq!(args.cwd, Some(PathBuf::from("/other workspace")));
    }
}
