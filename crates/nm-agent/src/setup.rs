use nm_core::config::{default_agent_name_from_hostname, load_config, save_config};
use std::io::{self, IsTerminal, Write};
use std::path::Path;

pub fn is_configured(output_path: &Path) -> bool {
    output_path.is_file()
}

pub async fn run_setup(
    template_path: &Path,
    output_path: &Path,
    auto_continue: bool,
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

    let agent_name = if io::stdin().is_terminal() {
        println!("Network Monitor Agent setup");
        println!("Default agent name: {default_name}");
        print!("Agent name [{default_name}]: ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let name = input.trim();
        if name.is_empty() {
            default_name
        } else {
            name.to_string()
        }
    } else {
        println!(
            "Non-interactive setup — using agent name: {default_name}"
        );
        default_name
    };

    config.agent.name = Some(agent_name);

    save_config(output_path, &config)?;
    println!("Saved runtime config to {}", output_path.display());
    println!("Agent name: {}", config.agent.name.as_ref().unwrap());
    if !auto_continue {
        println!("Run the agent with: nm-agent run");
    }
    Ok(())
}

pub async fn ensure_configured(
    template_path: &Path,
    output_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if is_configured(output_path) {
        return Ok(());
    }
    println!(
        "No config at {} — running first-time setup",
        output_path.display()
    );
    run_setup(template_path, output_path, true).await
}
