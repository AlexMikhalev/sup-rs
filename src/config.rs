use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Supfile {
    pub version: String,
    #[serde(default)]
    pub env: Option<HashMap<String, String>>,
    pub networks: HashMap<String, Network>,
    pub commands: HashMap<String, Command>,
    #[serde(default)]
    pub targets: HashMap<String, Vec<String>>,
}

impl Supfile {
    pub fn from_file(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path)
            .context("Failed to read Supfile")?;
        serde_yaml::from_str(&contents)
            .context("Failed to parse Supfile")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Network {
    #[serde(default)]
    pub hosts: Vec<String>,
    #[serde(default)]
    pub inventory: Option<String>,
    #[serde(default)]
    pub env: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    #[serde(default)]
    pub desc: Option<String>,
    #[serde(default)]
    pub local: Option<String>,
    #[serde(default)]
    pub run: Option<String>,
    #[serde(default)]
    pub script: Option<String>,
    #[serde(default)]
    pub upload: Option<Vec<Upload>>,
    #[serde(default)]
    pub stdin: bool,
    #[serde(default)]
    pub once: bool,
    #[serde(default)]
    pub serial: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Upload {
    pub src: String,
    pub dst: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use anyhow::Result;

    fn create_test_file(content: &str, filename: &str) -> Result<PathBuf> {
        let path = PathBuf::from(filename);
        fs::write(&path, content)?;
        Ok(path)
    }

    fn cleanup_test_file(path: PathBuf) {
        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_parse_simple_config() -> Result<()> {
        let simple_yaml = include_str!("../example_simple.yml");
        let path = create_test_file(simple_yaml, "test_simple.yml")?;
        
        let config = Supfile::from_file(&path)?;
        
        // Test version
        assert_eq!(config.version, "0.4");
        
        // Test networks
        assert!(config.networks.contains_key("dev"));
        assert!(config.networks.contains_key("staging"));
        assert!(config.networks.contains_key("prod"));
        
        // Test dev network hosts
        let dev_network = config.networks.get("dev").unwrap();
        assert_eq!(dev_network.hosts.len(), 2);
        assert!(dev_network.hosts.contains(&"alex@bigbox".to_string()));
        assert!(dev_network.hosts.contains(&"alex@100.106.66.7".to_string()));
        
        // Test commands
        assert!(config.commands.contains_key("bash"));
        assert!(config.commands.contains_key("ping"));
        assert!(config.commands.contains_key("upload"));
        assert!(config.commands.contains_key("build"));
        assert!(config.commands.contains_key("test"));
        
        // Test specific command properties
        let bash_cmd = config.commands.get("bash").unwrap();
        assert_eq!(bash_cmd.desc.as_deref(), Some("Interactive Bash on all hosts"));
        assert!(bash_cmd.stdin);
        assert_eq!(bash_cmd.run.as_deref(), Some("bash"));
        
        let upload_cmd = config.commands.get("upload").unwrap();
        let uploads = upload_cmd.upload.as_ref().unwrap();
        assert_eq!(uploads.len(), 1);
        assert_eq!(uploads[0].src, "./dist");
        assert_eq!(uploads[0].dst, "/tmp/");
        
        cleanup_test_file(path);
        Ok(())
    }

    #[test]
    fn test_parse_full_config() -> Result<()> {
        let full_yaml = include_str!("../example_full.yml");
        let path = create_test_file(full_yaml, "test_full.yml")?;
        
        let config = Supfile::from_file(&path)?;
        
        // Test version
        assert_eq!(config.version, "0.4");
        
        // Test environment variables
        let env = config.env.as_ref().unwrap();
        assert_eq!(env.get("NAME").unwrap(), "example-app");
        assert_eq!(env.get("IMAGE").unwrap(), "example/api:latest");
        assert_eq!(env.get("HOST_PORT").unwrap(), "8000");
        
        // Test networks
        assert!(config.networks.contains_key("local"));
        assert!(config.networks.contains_key("dev"));
        assert!(config.networks.contains_key("staging"));
        assert!(config.networks.contains_key("prod-us"));
        assert!(config.networks.contains_key("prod-eu"));
        
        // Test staging inventory
        let staging = config.networks.get("staging").unwrap();
        assert!(staging.inventory.is_some());
        
        // Test prod-us network
        let prod_us = config.networks.get("prod-us").unwrap();
        assert_eq!(prod_us.hosts.len(), 3);
        let prod_us_env = prod_us.env.as_ref().unwrap();
        assert_eq!(prod_us_env.get("ENV").unwrap(), "production");
        assert_eq!(prod_us_env.get("REGION").unwrap(), "us-east-1");
        
        // Test commands
        let rolling_update = config.commands.get("rolling-update").unwrap();
        assert!(rolling_update.run.is_some());
        assert_eq!(rolling_update.serial, Some(2));
        
        // Test targets
        let targets = &config.targets;
        let deploy_steps = targets.get("deploy").unwrap();
        assert_eq!(deploy_steps.len(), 6);
        assert!(deploy_steps.contains(&"build".to_string()));
        assert!(deploy_steps.contains(&"test".to_string()));
        assert!(deploy_steps.contains(&"push".to_string()));
        assert!(deploy_steps.contains(&"upload-config".to_string()));
        assert!(deploy_steps.contains(&"rolling-update".to_string()));
        assert!(deploy_steps.contains(&"status".to_string()));
        
        cleanup_test_file(path);
        Ok(())
    }

    #[test]
    fn test_invalid_yaml() {
        let invalid_yaml = "version: 0.4\nnetworks: not_a_map";
        let path = create_test_file(invalid_yaml, "test_invalid.yml").unwrap();
        
        let result = Supfile::from_file(&path);
        assert!(result.is_err());
        
        cleanup_test_file(path);
    }

    #[test]
    fn test_missing_required_fields() {
        let incomplete_yaml = r#"
version: "0.4"
networks: {}
commands:
  dummy:
    run: "echo test"
"#;
        let path = create_test_file(incomplete_yaml, "test_incomplete.yml").unwrap();
        
        let result = Supfile::from_file(&path);
        assert!(result.is_ok());
        let config = result.unwrap();
        assert_eq!(config.networks.len(), 0);
        assert_eq!(config.commands.len(), 1);
        assert!(config.targets.is_empty());
        
        cleanup_test_file(path);
    }

    #[test]
    fn test_network_env_inheritance() -> Result<()> {
        let yaml = r#"
version: "0.4"
env:
  GLOBAL: value
networks:
  dev:
    hosts: ["host1"]
    env:
      LOCAL: dev_value
commands: {}
"#;
        let path = create_test_file(yaml, "test_env.yml")?;
        
        let config = Supfile::from_file(&path)?;
        let global_env = config.env.unwrap();
        let dev_env = config.networks.get("dev").unwrap().env.as_ref().unwrap();
        
        assert_eq!(global_env.get("GLOBAL").unwrap(), "value");
        assert_eq!(dev_env.get("LOCAL").unwrap(), "dev_value");
        
        cleanup_test_file(path);
        Ok(())
    }

    #[test]
    fn test_command_properties() -> Result<()> {
        let yaml = r#"
version: "0.4"
networks: {}
commands:
  test_cmd:
    desc: "Test command"
    local: "local_command"
    run: "remote_command"
    stdin: true
    once: true
    serial: 5
"#;
        let path = create_test_file(yaml, "test_cmd.yml")?;
        
        let config = Supfile::from_file(&path)?;
        let cmd = config.commands.get("test_cmd").unwrap();
        
        assert_eq!(cmd.desc.as_deref(), Some("Test command"));
        assert_eq!(cmd.local.as_deref(), Some("local_command"));
        assert_eq!(cmd.run.as_deref(), Some("remote_command"));
        assert!(cmd.stdin);
        assert!(cmd.once);
        assert_eq!(cmd.serial, Some(5));
        
        cleanup_test_file(path);
        Ok(())
    }
} 