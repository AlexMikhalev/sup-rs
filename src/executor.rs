use crate::config::{Command, Network};
use anyhow::{Context, Result};
use colored::*;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command as ProcessCommand, Stdio};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, info};

#[derive(Debug, Clone)]
struct SshHost {
    username: String,
    hostname: String,
}

impl SshHost {
    fn parse(host_str: &str) -> Result<Self> {
        // Parse user@host
        let (username, hostname) = host_str.split_once('@')
            .context("Host must be in format user@host")?;

        Ok(Self {
            username: username.to_string(),
            hostname: hostname.to_string(),
        })
    }

    fn to_string(&self) -> String {
        format!("{}@{}", self.username, self.hostname)
    }
}

pub struct Executor {
    network: Network,
    env: std::collections::HashMap<String, String>,
}

impl Executor {
    pub fn new(network: Network, env: std::collections::HashMap<String, String>) -> Self {
        Self { network, env }
    }

    pub async fn execute_local(&self, cmd: &str) -> Result<()> {
        println!("{} {}", "LOCAL".green(), cmd);
        
        let status = ProcessCommand::new("sh")
            .arg("-c")
            .arg(cmd)
            .env_clear()
            .envs(&self.env)
            .status()?;

        if !status.success() {
            anyhow::bail!("Local command failed with status: {}", status);
        }
        Ok(())
    }

    pub async fn execute_ssh(&self, cmd: &str, interactive: bool) -> Result<()> {
        if interactive {
            // For interactive mode, we only support one host at a time
            if self.network.hosts.len() > 1 {
                anyhow::bail!("Interactive mode only supports one host at a time");
            }
            let host = SshHost::parse(&self.network.hosts[0])?;
            self.handle_interactive_session(&host, cmd).await
        } else {
            self.handle_parallel_sessions(cmd).await
        }
    }

    async fn handle_parallel_sessions(&self, cmd: &str) -> Result<()> {
        let (tx, mut rx) = mpsc::channel(32);
        let mut handles = Vec::new();
        
        for host_str in &self.network.hosts {
            let tx = tx.clone();
            let host = match SshHost::parse(host_str) {
                Ok(h) => h,
                Err(e) => {
                    eprintln!("Error parsing host {}: {}", host_str, e);
                    continue;
                }
            };
            info!("Connecting to {}", host.to_string());
            let cmd = cmd.to_string();
            let host_str = host_str.to_string();
            
            let handle = tokio::spawn(async move {
                if let Err(e) = Self::handle_ssh_session(&host, &cmd, tx).await {
                    eprintln!("Error on host {}: {}", host_str, e);
                }
            });
            handles.push(handle);
        }

        // Drop the original sender so the channel can close when all tasks complete
        drop(tx);
        
        // Process output from all hosts
        while let Some((host, output)) = rx.recv().await {
            println!("{} {}", host.blue(), output);
        }

        // Wait for all tasks to complete
        for handle in handles {
            handle.await?;
        }
        
        Ok(())
    }

    async fn handle_interactive_session(&self, host: &SshHost, cmd: &str) -> Result<()> {
        debug!("Starting interactive SSH session to {}", host.to_string());

        let mut ssh_cmd = ProcessCommand::new("ssh");
        ssh_cmd
            .arg("-tt") // Force TTY allocation
            .arg(&host.to_string())
            .arg(cmd)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        debug!("Running command: {:?}", ssh_cmd);
        let status = ssh_cmd.status()?;

        if !status.success() {
            anyhow::bail!("SSH command failed with status: {}", status);
        }

        Ok(())
    }

    async fn handle_ssh_session(
        host: &SshHost,
        cmd: &str,
        tx: mpsc::Sender<(String, String)>,
    ) -> Result<()> {
        debug!("Starting SSH session to {}", host.to_string());

        let mut ssh_cmd = ProcessCommand::new("ssh");
        ssh_cmd.arg(&host.to_string());

        // For non-interactive mode, use sh -c to properly handle command with arguments
        ssh_cmd
            .arg("sh")
            .arg("-c")
            .arg(cmd);

        ssh_cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        debug!("Running command: {:?}", ssh_cmd);
        let mut child = ssh_cmd.spawn()?;
        
        let stdout = child.stdout.take()
            .context("Failed to capture stdout")?;
        let stderr = child.stderr.take()
            .context("Failed to capture stderr")?;

        // Read output line by line
        let stdout_reader = BufReader::new(stdout);
        let stderr_reader = BufReader::new(stderr);

        // Process stdout
        for line in stdout_reader.lines() {
            if let Ok(line) = line {
                tx.send((host.to_string(), format!("{}\n", line))).await?;
            }
        }

        // Process stderr
        for line in stderr_reader.lines() {
            if let Ok(line) = line {
                tx.send((host.to_string(), format!("stderr: {}\n", line))).await?;
            }
        }

        let status = child.wait()?;
        if !status.success() {
            anyhow::bail!("SSH command failed with status: {}", status);
        }

        Ok(())
    }

    pub async fn execute_command(&self, command: &Command) -> Result<()> {
        if let Some(local_cmd) = &command.local {
            self.execute_local(local_cmd).await?;
        }

        if let Some(remote_cmd) = &command.run {
            self.execute_ssh(remote_cmd, command.stdin).await?;
        }

        Ok(())
    }
} 