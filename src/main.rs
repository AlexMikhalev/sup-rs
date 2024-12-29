mod config;
mod executor;

use anyhow::Result;
use clap::Parser;
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::Level;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to Supfile
    #[arg(short, long, default_value = "Supfile.yml")]
    file: PathBuf,

    /// Network to use from Supfile
    network: String,

    /// Command to execute
    command: String,

    /// Environment variables in KEY=VALUE format
    #[arg(short, long)]
    env: Vec<String>,

    /// Enable debug logging
    #[arg(short, long)]
    debug: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let args = Args::parse();

    // Initialize logging
    if args.debug {
        tracing_subscriber::fmt()
            .with_max_level(Level::DEBUG)
            .with_target(false)
            .with_thread_ids(true)
            .with_file(true)
            .with_line_number(true)
            .with_thread_names(true)
            .with_level(true)
            .with_ansi(true)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_max_level(Level::INFO)
            .with_target(false)
            .init();
    }

    // Parse environment variables
    let mut env = HashMap::new();
    for env_var in args.env {
        if let Some((key, value)) = env_var.split_once('=') {
            env.insert(key.to_string(), value.to_string());
        }
    }

    // Add default environment variables
    env.insert("SUP_USER".to_string(), whoami::username());
    env.insert("SUP_TIME".to_string(), chrono::Local::now().to_rfc3339());
    env.insert("SUP_NETWORK".to_string(), args.network.clone());

    // Load and parse Supfile
    let supfile = config::Supfile::from_file(args.file.to_str().unwrap())?;

    // Get the network configuration
    let network = supfile.networks.get(&args.network).ok_or_else(|| {
        anyhow::anyhow!("Network '{}' not found in Supfile", args.network)
    })?.clone();

    // Get the command configuration
    let command = supfile.commands.get(&args.command).ok_or_else(|| {
        anyhow::anyhow!("Command '{}' not found in Supfile", args.command)
    })?;

    // Create executor and run the command
    let executor = executor::Executor::new(network, env);
    executor.execute_command(command).await?;

    Ok(())
}
