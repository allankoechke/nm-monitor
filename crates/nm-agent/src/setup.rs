use nm_core::config::{default_agent_name_from_hostname, load_config, save_config};
use std::io::{self, Write};
use std::path::Path;

pub async fn run_setup(config_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut config = if config_path.exists() {
        load_config(config_path)?
    } else {
        nm_core::AppConfig::default()
    };

    let default_name = config
        .agent
        .name
        .clone()
        .unwrap_or_else(default_agent_name_from_hostname);

    println!("Network Monitor Agent setup");
    println!("Default agent name: {default_name}");
    print!("Agent name [{default_name}]: ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let name = input.trim();
    config.agent.name = Some(if name.is_empty() {
        default_name
    } else {
        name.to_string()
    });

    save_config(config_path, &config)?;
    println!("Saved config to {}", config_path.display());
    println!("Agent name: {}", config.agent.name.as_ref().unwrap());
    Ok(())
}
