mod args;
mod commands;
mod stream;
use clap::Parser;
pub async fn run() -> anyhow::Result<()> {
    commands::run(args::Args::parse()).await
}
