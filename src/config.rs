use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;

use crate::logging::LogConfig;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub server: ServerConfig,
    pub ssl: SslConfig,
    pub cloudflare: CloudflareConfig,
    pub logging: Option<LogConfig>,
    pub backends: HashMap<String, BackendConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    pub http_port: u16,
    pub https_port: u16,
    pub enable_https: bool,
    pub workers: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SslConfig {
    pub cert_path: Option<String>,
    pub key_path: Option<String>,
    pub auto_cert: bool,
    pub domains: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CloudflareConfig {
    pub api_token: Option<String>,
    pub zone_id: Option<String>,
    pub email: Option<String>,
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BackendConfig {
    pub url: String,
    pub app_type: AppType,
    pub health_check: Option<String>,
    pub process: Option<ProcessConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AppType {
    Static,
    NodeJS,
    Python,
    Proxy,
    Tomcat,
    PhpFpm,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProcessConfig {
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub working_dir: Option<String>,
    pub auto_restart: bool,
}

impl Config {
    pub fn load(path: &str) -> Result<Self> {
        let contents = fs::read_to_string(path)?;
        let config: Config = toml::from_str(&contents)?;
        Ok(config)
    }

    pub fn get_backend(&self, host: &str) -> Option<&BackendConfig> {
        self.backends.get(host)
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            server: ServerConfig {
                http_port: 8080,
                https_port: 8443,
                enable_https: false,
                workers: None,
            },
            ssl: SslConfig {
                cert_path: None,
                key_path: None,
                auto_cert: true,
                domains: vec![],
            },
            cloudflare: CloudflareConfig {
                api_token: None,
                zone_id: None,
                email: None,
                api_key: None,
            },
            logging: Some(LogConfig::default()),
            backends: HashMap::new(),
        }
    }
}