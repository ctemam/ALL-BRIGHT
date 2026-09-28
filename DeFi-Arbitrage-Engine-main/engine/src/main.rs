//! Binary entry point for the DeFi arbitrage engine.
//!
//! Why this file exists: the deployment artifacts (`Docker/Dockerfile`) build
//! `--bin arbitrage-engine` and run `./arbitrage-engine run`, but the package previously
//! shipped only `lib.rs`, so no binary target existed and the image could never be built.
//!
//! What it actually does today: loads configuration from the environment, brings up the
//! engine state machine and serves `/health` and `/status`. The trading pipeline (market
//! data, strategy evaluation, transaction execution) is still unimplemented — see
//! `docs/ARBITRAGE-COMPARISON.md`, items A-P0-1 to A-P0-3.

use arbitrage_engine::config::Config;
use arbitrage_engine::engine::ArbitrageEngine;
use arbitrage_engine::error::Result;
use arbitrage_engine::server::Server;
use clap::Parser;
use tracing::{info, warn};

/// Command line interface.
#[derive(Debug, Parser)]
#[command(name = "arbitrage-engine", version = arbitrage_engine::VERSION)]
struct Cli {
    /// Command to execute.
    #[command(subcommand)]
    command: Option<Command>,
}

/// Supported commands.
#[derive(Debug, clap::Subcommand)]
enum Command {
    /// Start the engine and serve the HTTP API.
    Run,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    init_tracing();

    match cli.command.unwrap_or(Command::Run) {
        Command::Run => run().await,
    }
}

/// Load configuration, start the engine and serve the HTTP API.
async fn run() -> Result<()> {
    let config = Config::from_env()?;
    info!(
        "Starting arbitrage engine {} on {}:{}",
        arbitrage_engine::VERSION,
        config.server.host,
        config.server.port
    );

    let engine = ArbitrageEngine::new(config.clone()).await?;
    engine.start().await?;

    warn!(
        "Trading is NOT active: no market-data feed, strategy evaluation or transaction \
         execution is implemented yet. This process only serves /health and /status."
    );

    let server = Server::new(&config.server.host, config.server.port)?;
    server.start().await?;

    engine.stop().await?;
    Ok(())
}

/// Initialise tracing/logging from `RUST_LOG` (defaults to `info`).
fn init_tracing() {
    use tracing_subscriber::EnvFilter;

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
}
