mod args;
mod commands;
mod stream;
pub mod terminal;
use clap::Parser;
pub async fn run() -> anyhow::Result<()> {
    commands::run(args::Args::parse()).await
}
