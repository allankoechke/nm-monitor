use nm_core::config::{default_agent_name_from_hostname, load_config, save_config};
use std::io::{self, Write};
use std::path::Path;

pub async fn run_setup(
    template_path: &Path,
    output_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut config = if template_path.exists() {
        println!("Using setup template: {}", template_path.display());
        load_config(template_path)?
    } else {
        println!(
            "Template not found at {} — using built-in defaults",
            template_path.display()
        );
        nm_core::AppConfig::default()
    };

    if output_path.exists() {
        println!(
            "Warning: {} already exists and will be overwritten",
            output_path.display()
        );
    }

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

    save_config(output_path, &config)?;
    println!("Saved runtime config to {}", output_path.display());
    println!("Agent name: {}", config.agent.name.as_ref().unwrap());
    println!("Run the agent with: nm-agent run");
    Ok(())
}
