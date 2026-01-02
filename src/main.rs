mod cli;
mod config;
mod error;
mod llm;
mod summarizer;
mod transcript;
mod web;
mod youtube;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Commands};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "yousummary=info".into()),
        )
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Fetch(args) => {
            cli::handle_fetch(args).await?;
        }
        Commands::Summarize(args) => {
            cli::handle_summarize(args).await?;
        }
        Commands::Serve(args) => {
            cli::handle_serve(args).await?;
        }
    }

    Ok(())
}
