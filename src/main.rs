use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use tracing::{debug, info};
use chrono::Local;
use whoami;

mod config;
mod executor;

use config::Supfile;
use executor::Executor;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to Supfile
    #[arg(short, long, default_value = "Supfile.yml")]
    file: PathBuf,

    /// Network to use
    #[arg(default_value = "dev")]
    network: String,

    /// Command to execute
    #[arg(default_value = "bash")]
    command: String,

    /// Enable debug output
    #[arg(short = 'D', long)]
    debug: bool,

    /// Set environment variables
    #[arg(short, long = "env", value_delimiter = ',')]
    env_vars: Vec<String>,

    /// Filter hosts matching regexp
    #[arg(long)]
    only: Option<String>,

    /// Filter out hosts matching regexp
    #[arg(long)]
    except: Option<String>,

    /// Disable hostname prefix in output
    #[arg(long = "disable-prefix")]
    disable_prefix: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(if args.debug { tracing::Level::DEBUG } else { tracing::Level::INFO })
        .with_target(false)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .init();

    debug!("Loading Supfile from {}", args.file.display());
    let supfile = Supfile::from_file(&args.file)?;

    let network = supfile.networks.get(&args.network)
        .ok_or_else(|| anyhow::anyhow!("Network {} not found", args.network))?;

    // Check if this is a target or a command
    let commands = if let Some(target) = supfile.targets.get(&args.command) {
        // For targets, we need to run multiple commands in sequence
        target.iter()
            .map(|cmd| supfile.commands.get(cmd)
                .ok_or_else(|| anyhow::anyhow!("Command {} not found in target {}", cmd, args.command)))
            .collect::<Result<Vec<_>>>()?
    } else {
        // For single commands, just get that command
        vec![supfile.commands.get(&args.command)
            .ok_or_else(|| anyhow::anyhow!("Command {} not found", args.command))?]
    };

    // Setup environment variables
    let mut env = std::env::vars().collect::<std::collections::HashMap<_, _>>();
    
    // Add Sup-specific environment variables
    env.insert("SUP_TIME".to_string(), Local::now().to_rfc3339());
    env.insert("SUP_USER".to_string(), whoami::username());
    env.insert("SUP_NETWORK".to_string(), args.network.clone());
    
    // Add global environment variables from Supfile
    if let Some(vars) = &supfile.env {
        env.extend(vars.clone());
    }
    
    // Add network-specific environment variables
    if let Some(net_env) = &network.env {
        env.extend(net_env.clone());
    }
    
    // Add command-line environment variables
    for var in &args.env_vars {
        if let Some((key, value)) = var.split_once('=') {
            env.insert(key.to_string(), value.to_string());
        }
    }

    let executor = Executor::new(
        network.clone(),
        env,
        args.only,
        args.except,
        args.disable_prefix,
    )?;

    // Execute all commands in sequence
    for command in commands {
        executor.execute_command(command).await?;
    }

    Ok(())
}
