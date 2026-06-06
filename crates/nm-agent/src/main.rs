mod daemon;
mod setup;

use clap::{Parser, Subcommand};
use daemon::run_daemon;
use nm_core::{default_config_path, default_setup_template_path};
use setup::{ensure_configured, run_setup};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "nm-agent", about = "Fing-like network monitor agent")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Runtime config file (default: ~/.config/network-monitor/config.toml)
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    /// Setup template to copy from (default: config/example.toml in project tree)
    #[arg(long, global = true, default_value = "config/example.toml")]
    template: PathBuf,

    #[arg(long)]
    scan_once: bool,

    #[arg(long)]
    daemon: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Copy template config to home dir and set agent name
    Setup,
    /// Run as background daemon
    Run,
}

fn resolve_runtime_config(cli: &Cli) -> PathBuf {
    cli.config
        .clone()
        .unwrap_or_else(default_config_path)
}

fn resolve_setup_template(cli: &Cli) -> PathBuf {
    if cli.template.exists() {
        cli.template.clone()
    } else {
        default_setup_template_path()
    }
}

async fn run_agent(
    template: &PathBuf,
    config: &PathBuf,
    scan_once: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    ensure_configured(template, config).await?;
    run_daemon(config, scan_once).await
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("nm_agent=info".parse()?))
        .init();

    let cli = Cli::parse();
    let runtime_config = resolve_runtime_config(&cli);
    let setup_template = resolve_setup_template(&cli);

    match cli.command {
        Some(Commands::Setup) => run_setup(&setup_template, &runtime_config, false).await?,
        Some(Commands::Run) => run_agent(&setup_template, &runtime_config, cli.scan_once).await?,
        None if cli.daemon => run_agent(&setup_template, &runtime_config, cli.scan_once).await?,
        None if cli.scan_once => run_agent(&setup_template, &runtime_config, true).await?,
        None => run_agent(&setup_template, &runtime_config, false).await?,
    }
    Ok(())
}
