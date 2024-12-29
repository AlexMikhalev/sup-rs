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