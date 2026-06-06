mod daemon;
mod setup;

use clap::{Parser, Subcommand};
use daemon::run_daemon;
use setup::run_setup;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "nm-agent", about = "Fing-like network monitor agent")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    #[arg(long, default_value = "config/example.toml")]
    config: PathBuf,

    #[arg(long)]
    scan_once: bool,

    #[arg(long)]
    daemon: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Interactive first-run setup (agent name, etc.)
    Setup,
    /// Run as background daemon
    Run,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("nm_agent=info".parse()?))
        .init();

    let cli = Cli::parse();
    match cli.command {
        Some(Commands::Setup) => run_setup(&cli.config).await?,
        Some(Commands::Run) | None if cli.daemon => run_daemon(&cli.config, cli.scan_once).await?,
        None if cli.scan_once => run_daemon(&cli.config, true).await?,
        None => run_daemon(&cli.config, false).await?,
    }
    Ok(())
}
