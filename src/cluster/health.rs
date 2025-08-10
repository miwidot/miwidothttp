use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;
use tokio::time;
use tracing::{info, warn, error, debug};

use super::{ClusterConfig, NodeInfo, NodeState};

pub struct HealthMonitor {
    config: ClusterConfig,
    health_checks: Arc<RwLock<HashMap<String, HealthCheck>>>,
}

#[derive(Debug, Clone)]
pub struct HealthCheck {
    pub node_id: String,
    pub last_check: SystemTime,
    pub last_success: SystemTime,
    pub consecutive_failures: u32,
    pub is_healthy: bool,
    pub response_time_ms: u64,
}

impl HealthMonitor {
    pub async fn new(config: &ClusterConfig) -> Result<Self> {
        Ok(Self {
            config: config.clone(),
            health_checks: Arc::new(RwLock::new(HashMap::new())),
        })
    }
    
    pub async fn start(&self, nodes: Arc<RwLock<HashMap<String, NodeInfo>>>) -> Result<()> {
        info!("Starting health monitor");
        
        let health_checks = self.health_checks.clone();
        let interval = self.config.heartbeat_interval;
        
        tokio::spawn(async move {
            let mut ticker = time::interval(interval);
            
            loop {
                ticker.tick().await;
                Self::check_all_nodes(nodes.clone(), health_checks.clone()).await;
            }
        });
        
        Ok(())
    }
    
    async fn check_all_nodes(
        nodes: Arc<RwLock<HashMap<String, NodeInfo>>>,
        health_checks: Arc<RwLock<HashMap<String, HealthCheck>>>,
    ) {
        let nodes = nodes.read().await;
        
        for (node_id, node_info) in nodes.iter() {
            let start = std::time::Instant::now();
            
            // Perform health check (ping, HTTP check, etc.)
            let is_healthy = Self::check_node_health(node_info).await;
            let response_time = start.elapsed().as_millis() as u64;
            
            let mut checks = health_checks.write().await;
            let check = checks.entry(node_id.clone()).or_insert(HealthCheck {
                node_id: node_id.clone(),
                last_check: SystemTime::now(),
                last_success: SystemTime::now(),
                consecutive_failures: 0,
                is_healthy: true,
                response_time_ms: 0,
            });
            
            check.last_check = SystemTime::now();
            check.response_time_ms = response_time;
            
            if is_healthy {
                check.last_success = SystemTime::now();
                check.consecutive_failures = 0;
                check.is_healthy = true;
                
                if node_info.state == NodeState::Failed {
                    info!("Node {} is back online", node_id);
                }
            } else {
                check.consecutive_failures += 1;
                
                if check.consecutive_failures >= 3 {
                    check.is_healthy = false;
                    warn!("Node {} marked as unhealthy after {} failures", 
                          node_id, check.consecutive_failures);
                }
            }
        }
    }
    
    async fn check_node_health(node: &NodeInfo) -> bool {
        // Try to connect to the node's gRPC endpoint
        match Self::ping_node(&node.grpc_addr).await {
            Ok(_) => {
                debug!("Health check passed for node {}", node.id);
                true
            }
            Err(e) => {
                warn!("Health check failed for node {}: {}", node.id, e);
                false
            }
        }
    }
    
    async fn ping_node(addr: &std::net::SocketAddr) -> Result<()> {
        // Simple TCP connection test
        match tokio::time::timeout(
            Duration::from_secs(5),
            tokio::net::TcpStream::connect(addr)
        ).await {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => Err(e.into()),
            Err(_) => Err(anyhow::anyhow!("Connection timeout")),
        }
    }
    
    pub async fn get_health_status(&self) -> HashMap<String, HealthCheck> {
        self.health_checks.read().await.clone()
    }
    
    pub async fn is_node_healthy(&self, node_id: &str) -> bool {
        let checks = self.health_checks.read().await;
        checks.get(node_id)
            .map(|c| c.is_healthy)
            .unwrap_or(false)
    }
    
    pub async fn shutdown(&self) -> Result<()> {
        info!("Shutting down health monitor");
        Ok(())
    }
}