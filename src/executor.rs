use crate::config::{Command, Network, Upload};
use anyhow::{Context, Result};
use colored::*;
use regex::Regex;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Command as ProcessCommand, Stdio};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};
use shell_quote;

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

#[derive(Debug, Clone)]
pub struct Executor {
    network: Network,
    env: std::collections::HashMap<String, String>,
    only: Option<Regex>,
    except: Option<Regex>,
    disable_prefix: bool,
}

impl Executor {
    pub fn new(
        network: Network,
        env: std::collections::HashMap<String, String>,
        only: Option<String>,
        except: Option<String>,
        disable_prefix: bool,
    ) -> Result<Self> {
        let only = only.map(|r| Regex::new(&r)).transpose()?;
        let except = except.map(|r| Regex::new(&r)).transpose()?;
        
        Ok(Self {
            network,
            env,
            only,
            except,
            disable_prefix,
        })
    }

    fn filter_hosts(&self, hosts: &[String]) -> Vec<String> {
        hosts.iter()
            .filter(|host| {
                // Apply --only filter
                if let Some(only) = &self.only {
                    if !only.is_match(host) {
                        return false;
                    }
                }
                
                // Apply --except filter
                if let Some(except) = &self.except {
                    if except.is_match(host) {
                        return false;
                    }
                }
                
                true
            })
            .cloned()
            .collect()
    }

    async fn resolve_hosts(&self) -> Result<Vec<String>> {
        let mut hosts = Vec::new();

        // Add static hosts
        hosts.extend(self.network.hosts.clone());

        // Run inventory command if present
        if let Some(inventory) = &self.network.inventory {
            debug!("Running inventory command: {}", inventory);
            let output = ProcessCommand::new("sh")
                .arg("-c")
                .arg(inventory)
                .env_clear()
                .envs(&self.env)
                .output()?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("Inventory command failed: {}", stderr);
            }

            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if !line.trim().is_empty() {
                    hosts.push(line.trim().to_string());
                }
            }
        }

        // Apply host filters
        Ok(self.filter_hosts(&hosts))
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

    pub async fn execute_script(&self, script: &str) -> Result<()> {
        let script_path = Path::new(script);
        if !script_path.exists() {
            anyhow::bail!("Script file does not exist: {}", script);
        }

        println!("{} {}", "SCRIPT".green(), script);
        
        let status = ProcessCommand::new("sh")
            .arg(script)
            .env_clear()
            .envs(&self.env)
            .status()?;

        if !status.success() {
            anyhow::bail!("Script failed with status: {}", status);
        }
        Ok(())
    }

    pub async fn execute_ssh(&self, cmd: &str, interactive: bool, serial: Option<usize>, once: bool) -> Result<()> {
        let hosts = self.resolve_hosts().await?;
        
        if hosts.is_empty() {
            warn!("No hosts matched the filters");
            return Ok(());
        }

        if interactive {
            // For interactive mode, we only support one host at a time
            if hosts.len() > 1 {
                anyhow::bail!("Interactive mode only supports one host at a time");
            }
            let host = SshHost::parse(&hosts[0])?;
            self.handle_interactive_session(&host, cmd).await
        } else if once {
            // For once mode, only run on the first host
            if let Some(host) = hosts.first() {
                let host = SshHost::parse(host)?;
                self.handle_ssh_session(&host, cmd, None).await?;
            }
            Ok(())
        } else if let Some(batch_size) = serial {
            // For serial mode, run on hosts in batches
            for chunk in hosts.chunks(batch_size) {
                let mut handles = Vec::new();
                for host in chunk {
                    let host = SshHost::parse(host)?;
                    let cmd = cmd.to_string();
                    let (tx, mut rx) = mpsc::channel(32);
                    let executor = self.clone();
                    
                    let handle = tokio::spawn(async move {
                        if let Err(e) = executor.handle_ssh_session(&host, &cmd, Some(tx)).await {
                            eprintln!("Error on host {}: {}", host.to_string(), e);
                        }
                    });
                    handles.push((handle, rx));
                }

                // Process output from all hosts in this batch
                for (handle, mut rx) in handles {
                    while let Some((host, output)) = rx.recv().await {
                        if self.disable_prefix {
                            print!("{}", output);
                        } else {
                            println!("{} {}", host.blue(), output);
                        }
                    }
                    handle.await?;
                }
            }
            Ok(())
        } else {
            // For parallel mode, run on all hosts at once
            self.handle_parallel_sessions(cmd).await
        }
    }

    pub async fn execute_upload(&self, uploads: &[Upload]) -> Result<()> {
        debug!("Starting upload process for {} files", uploads.len());
        let hosts = self.resolve_hosts().await?;
        
        for host_str in hosts {
            let host = SshHost::parse(&host_str)?;
            for upload in uploads {
                self.handle_upload(&host, upload).await?;
            }
        }
        Ok(())
    }

    async fn ensure_remote_dir(&self, host: &SshHost, dir: &str) -> Result<()> {
        debug!("Ensuring remote directory exists: {}", dir);
        let mut ssh_cmd = ProcessCommand::new("ssh");
        ssh_cmd
            .arg(&host.to_string())
            .arg(format!("mkdir -p '{}'", dir))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let output = ssh_cmd.output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to create remote directory: {}", stderr);
        }
        Ok(())
    }

    async fn handle_upload(&self, host: &SshHost, upload: &Upload) -> Result<()> {
        let src_path = Path::new(&upload.src);
        if !src_path.exists() {
            anyhow::bail!("Source path does not exist: {}", upload.src);
        }

        info!("Uploading {} to {}:{}", upload.src, host.to_string(), upload.dst);

        // Ensure remote directory exists
        self.ensure_remote_dir(host, &upload.dst).await?;

        // Get source file/directory info
        let src_metadata = src_path.metadata()?;
        debug!("Source metadata: {:?}", src_metadata);

        // Create tar process to read from source
        let mut tar_cmd = ProcessCommand::new("tar");
        tar_cmd
            .arg("-czf")
            .arg("-")
            .arg("-C")
            .arg(src_path.parent().unwrap_or_else(|| Path::new(".")))
            .arg(src_path.file_name().unwrap())
            .stdout(Stdio::piped());

        debug!("Running tar command: {:?}", tar_cmd);
        let mut tar_process = tar_cmd.spawn()?;
        let tar_output = tar_process.stdout.take()
            .context("Failed to get tar stdout")?;

        // Create SSH process to write to destination
        let mut ssh_cmd = ProcessCommand::new("ssh");
        ssh_cmd
            .arg(&host.to_string())
            .arg(format!("cd '{}' && tar xzf -", upload.dst))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        debug!("Running SSH command: {:#?}", ssh_cmd);
        let mut ssh_process = ssh_cmd.spawn()?;
        let mut ssh_input = ssh_process.stdin.take()
            .context("Failed to get SSH stdin")?;

        // Copy tar output to SSH input
        debug!("Starting file transfer");
        let bytes_copied = std::io::copy(&mut BufReader::new(tar_output), &mut ssh_input)?;
        debug!("Transferred {} bytes", bytes_copied);
        drop(ssh_input); // Close stdin to signal EOF

        // Wait for both processes and capture output
        let tar_status = tar_process.wait()?;
        if !tar_status.success() {
            anyhow::bail!("Tar command failed with status: {}", tar_status);
        }

        let ssh_output = ssh_process.wait_with_output()?;
        if !ssh_output.status.success() {
            let stderr = String::from_utf8_lossy(&ssh_output.stderr);
            anyhow::bail!("SSH command failed: {}", stderr);
        }

        info!("Successfully uploaded {} to {}:{}", upload.src, host.to_string(), upload.dst);
        Ok(())
    }

    async fn handle_parallel_sessions(&self, cmd: &str) -> Result<()> {
        let hosts = self.resolve_hosts().await?;
        let (tx, mut rx) = mpsc::channel(32);
        let mut handles = Vec::new();
        
        for host_str in hosts {
            let tx = tx.clone();
            let host = match SshHost::parse(&host_str) {
                Ok(h) => h,
                Err(e) => {
                    eprintln!("Error parsing host {}: {}", host_str, e);
                    continue;
                }
            };
            info!("Connecting to {}", host.to_string());
            let cmd = cmd.to_string();
            let host_str = host_str.to_string();
            let executor = self.clone();
            
            let handle = tokio::spawn(async move {
                if let Err(e) = executor.handle_ssh_session(&host, &cmd, Some(tx)).await {
                    eprintln!("Error on host {}: {}", host_str, e);
                }
            });
            handles.push(handle);
        }

        // Drop the original sender so the channel can close when all tasks complete
        drop(tx);
        
        // Process output from all hosts
        while let Some((host, output)) = rx.recv().await {
            if self.disable_prefix {
                print!("{}", output);
            } else {
                println!("{} {}", host.blue(), output);
            }
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

        debug!("Running command: {:#?}", ssh_cmd);
        let status = ssh_cmd.status()?;

        if !status.success() {
            anyhow::bail!("SSH command failed with status: {}", status);
        }

        Ok(())
    }

    fn prepare_remote_command(&self, cmd: &str) -> String {
        // If command starts with sudo, ensure we preserve environment and handle quoting
        if cmd.trim().starts_with("sudo") {
            // Preserve environment variables with -E flag
            // Use bash -c to properly handle complex commands
            let quoted = shell_quote::bash::quote(cmd.trim_start_matches("sudo").trim());
            format!("sudo -E bash -c '{}'", quoted.to_string_lossy())
        } else {
            cmd.to_string()
        }
    }

    async fn handle_ssh_session(
        &self,
        host: &SshHost,
        cmd: &str,
        tx: Option<mpsc::Sender<(String, String)>>,
    ) -> Result<()> {
        debug!("Starting SSH session to {}", host.to_string());

        let mut ssh_cmd = ProcessCommand::new("ssh");
        ssh_cmd.arg(&host.to_string());

        // Prepare the command with proper sudo handling
        let prepared_cmd = self.prepare_remote_command(cmd);

        // For non-interactive mode, use sh -c to properly handle command with arguments
        ssh_cmd
            .arg("sh")
            .arg("-c")
            .arg(&prepared_cmd);

        ssh_cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        debug!("Running command: {:#?}", ssh_cmd);
        let mut child = ssh_cmd.spawn()?;
        
        let stdout = child.stdout.take()
            .context("Failed to capture stdout")?;
        let stderr = child.stderr.take()
            .context("Failed to capture stderr")?;

        // Read output line by line
        let stdout_reader = BufReader::new(stdout);
        let stderr_reader = BufReader::new(stderr);

        if let Some(tx) = tx {
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
        } else {
            // Direct output mode
            for line in stdout_reader.lines() {
                if let Ok(line) = line {
                    println!("{}", line);
                }
            }

            for line in stderr_reader.lines() {
                if let Ok(line) = line {
                    eprintln!("stderr: {}", line);
                }
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

        if let Some(script) = &command.script {
            self.execute_script(script).await?;
        }

        if let Some(remote_cmd) = &command.run {
            self.execute_ssh(
                remote_cmd,
                command.stdin,
                command.serial,
                command.once
            ).await?;
        }

        if let Some(uploads) = &command.upload {
            self.execute_upload(uploads).await?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn create_test_executor() -> Executor {
        let network = Network {
            hosts: vec!["test@localhost".to_string()],
            inventory: None,
            env: None,
        };
        let env = HashMap::new();
        Executor::new(network, env, None, None, false).unwrap()
    }

    #[test]
    fn test_prepare_remote_command() {
        let executor = create_test_executor();

        // Test regular command
        let cmd = "echo 'hello world'";
        assert_eq!(executor.prepare_remote_command(cmd), cmd.to_string());

        // Test sudo command
        let sudo_cmd = "sudo apt-get install -y package";
        let prepared = executor.prepare_remote_command(sudo_cmd);
        assert!(prepared.starts_with("sudo -E bash -c "));
        assert!(prepared.contains("apt-get"));
        assert!(prepared.contains("install"));
        assert!(prepared.contains("package"));

        // Test sudo command with complex arguments
        let complex_sudo = r#"sudo sh -c 'echo "complex argument" > /etc/file'"#;
        let prepared = executor.prepare_remote_command(complex_sudo);
        assert!(prepared.starts_with("sudo -E bash -c "));
        assert!(prepared.contains("complex argument"));
        assert!(prepared.contains("/etc/file"));

        // Test sudo command with environment variables
        let env_sudo = r#"sudo DEBIAN_FRONTEND=noninteractive apt-get install -y package"#;
        let prepared = executor.prepare_remote_command(env_sudo);
        assert!(prepared.starts_with("sudo -E bash -c "));
        assert!(prepared.contains("DEBIAN_FRONTEND=noninteractive"));
        assert!(prepared.contains("apt-get"));
    }

    #[test]
    fn test_sudo_command_whitespace() {
        let executor = create_test_executor();

        // Test sudo command with leading whitespace
        let cmd_with_space = "   sudo apt-get install -y package";
        let prepared = executor.prepare_remote_command(cmd_with_space);
        assert!(prepared.starts_with("sudo -E bash -c "));
        assert!(prepared.contains("apt-get"));

        // Test sudo command with multiple spaces
        let cmd_multi_space = "sudo   apt-get    install   -y    package";
        let prepared = executor.prepare_remote_command(cmd_multi_space);
        assert!(prepared.starts_with("sudo -E bash -c "));
        assert!(prepared.contains("apt-get"));
        assert!(prepared.contains("install"));
        assert!(prepared.contains("package"));
    }
}