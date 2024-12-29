use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize)]
pub struct Supfile {
    #[serde(default)]
    pub env: HashMap<String, String>,
    pub networks: HashMap<String, Network>,
    pub commands: HashMap<String, Command>,
    #[serde(default)]
    pub targets: HashMap<String, Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Network {
    pub hosts: Vec<String>,
    #[serde(default)]
    pub inventory: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Command {
    pub desc: Option<String>,
    #[serde(default)]
    pub local: Option<String>,
    #[serde(default)]
    pub run: Option<String>,
    #[serde(default)]
    pub stdin: bool,
    #[serde(default)]
    pub serial: Option<usize>,
    #[serde(default)]
    pub once: bool,
    #[serde(default)]
    pub upload: Option<Vec<Upload>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Upload {
    pub src: String,
    pub dst: String,
}

impl Supfile {
    pub fn from_file(path: &str) -> anyhow::Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        let config: Supfile = serde_yaml::from_str(&contents)?;
        Ok(config)
    }
} 