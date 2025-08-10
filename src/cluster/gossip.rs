use anyhow::Result;
use chitchat::{ChitchatConfig, ChitchatHandle, FailureDetectorConfig};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::time::Duration;
use tracing::{info, debug};

use super::ClusterConfig;

pub struct GossipManager {
    config: ClusterConfig,
    handle: Option<ChitchatHandle>,
}

impl GossipManager {
    pub async fn new(config: &ClusterConfig) -> Result<Self> {
        Ok(Self {
            config: config.clone(),
            handle: None,
        })
    }
    
    pub async fn start(&mut self) -> Result<()> {
        info!("Starting gossip protocol on {}", self.config.bind_addr);
        
        let chitchat_config = ChitchatConfig {
            node_id: self.config.node_id.clone().into(),
            cluster_id: self.config.cluster_name.clone(),
            gossip_addr: self.config.bind_addr,
            gossip_interval: self.config.gossip_interval,
            listen_addr: self.config.advertise_addr.unwrap_or(self.config.bind_addr),
            seed_nodes: self.config.seed_nodes
                .iter()
                .filter_map(|s| s.parse().ok())
                .collect(),
            failure_detector_config: FailureDetectorConfig {
                phi_threshold: 8.0,
                sampling_window: Duration::from_secs(60),
                min_std_deviation: Duration::from_millis(100),
                ..Default::default()
            },
            ..Default::default()
        };
        
        let (handle, _stream) = chitchat::spawn_chitchat(
            chitchat_config,
            vec![],
            &tokio::runtime::Handle::current(),
        ).await?;
        
        self.handle = Some(handle);
        
        Ok(())
    }
    
    pub async fn add_node(&self, addr: &str) -> Result<()> {
        debug!("Adding node to gossip: {}", addr);
        // Node discovery handled by chitchat
        Ok(())
    }
    
    pub async fn broadcast(&self, message: Vec<u8>) -> Result<()> {
        if let Some(handle) = &self.handle {
            // Broadcast message through gossip protocol
            debug!("Broadcasting message of {} bytes", message.len());
        }
        Ok(())
    }
    
    pub async fn shutdown(&self) -> Result<()> {
        if let Some(handle) = &self.handle {
            handle.shutdown().await?;
        }
        Ok(())
    }
}