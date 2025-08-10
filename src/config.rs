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
    pub cluster: Option<ClusterConfig>,
    pub logging: Option<LogConfig>,
    pub backends: HashMap<String, BackendConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClusterConfig {
    pub enabled: bool,
    pub node_id: String,
    pub bind_addr: String,
    pub advertise_addr: String,
    pub join_nodes: Vec<String>,
    pub raft: RaftConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RaftConfig {
    pub enabled: bool,
    pub bind_addr: String,
    pub data_dir: String,
    pub election_timeout: u64,
    pub heartbeat_interval: u64,
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
            cluster: Some(ClusterConfig {
                enabled: false,
                node_id: "node-1".to_string(),
                bind_addr: "0.0.0.0:7946".to_string(),
                advertise_addr: "127.0.0.1:7946".to_string(),
                join_nodes: vec![],
                raft: RaftConfig {
                    enabled: false,
                    bind_addr: "0.0.0.0:8090".to_string(),
                    data_dir: "/var/lib/miwidothttp/raft".to_string(),
                    election_timeout: 150,
                    heartbeat_interval: 50,
                },
            }),
            logging: Some(LogConfig::default()),
            backends: HashMap::new(),
        }
    }
}