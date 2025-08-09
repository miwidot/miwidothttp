use anyhow::{anyhow, Result};
use chitchat::{ChitchatConfig, ChitchatHandle, FailureDetectorConfig};
use hashring::HashRing;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::{RwLock, Mutex, broadcast};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

pub mod gossip;
pub mod consensus;
pub mod distribution;
pub mod replication;
pub mod health;

use crate::config::Config;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterConfig {
    pub enabled: bool,
    pub node_id: String,
    pub cluster_name: String,
    pub bind_addr: SocketAddr,
    pub advertise_addr: Option<SocketAddr>,
    pub seed_nodes: Vec<String>,
    pub gossip_interval: Duration,
    pub gossip_port: u16,
    pub grpc_port: u16,
    pub heartbeat_interval: Duration,
    pub election_timeout: Duration,
    pub replication_factor: usize,
    pub quorum_size: usize,
    pub enable_auto_join: bool,
    pub enable_auto_failover: bool,
    pub data_sync_interval: Duration,
    pub etcd_endpoints: Vec<String>,
}

impl Default for ClusterConfig {
    fn default() -> Self {
        ClusterConfig {
            enabled: false,
            node_id: Uuid::new_v4().to_string(),
            cluster_name: "miwidothttp-cluster".to_string(),
            bind_addr: "0.0.0.0:7946".parse().unwrap(),
            advertise_addr: None,
            seed_nodes: vec![],
            gossip_interval: Duration::from_secs(1),
            gossip_port: 7946,
            grpc_port: 7947,
            heartbeat_interval: Duration::from_secs(5),
            election_timeout: Duration::from_secs(30),
            replication_factor: 3,
            quorum_size: 2,
            enable_auto_join: true,
            enable_auto_failover: true,
            data_sync_interval: Duration::from_secs(10),
            etcd_endpoints: vec!["http://localhost:2379".to_string()],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    pub id: String,
    pub name: String,
    pub addr: SocketAddr,
    pub grpc_addr: SocketAddr,
    pub state: NodeState,
    pub role: NodeRole,
    pub capacity: NodeCapacity,
    pub load: NodeLoad,
    pub version: String,
    pub started_at: SystemTime,
    pub last_seen: SystemTime,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NodeState {
    Joining,
    Active,
    Leaving,
    Failed,
    Suspended,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NodeRole {
    Leader,
    Follower,
    Candidate,
    Observer,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeCapacity {
    pub cpu_cores: u32,
    pub memory_mb: u64,
    pub disk_gb: u64,
    pub network_mbps: u32,
    pub max_connections: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeLoad {
    pub cpu_percent: f32,
    pub memory_percent: f32,
    pub disk_percent: f32,
    pub active_connections: u32,
    pub requests_per_second: f64,
    pub response_time_ms: f64,
}

pub struct ClusterManager {
    config: ClusterConfig,
    node_info: Arc<RwLock<NodeInfo>>,
    nodes: Arc<RwLock<HashMap<String, NodeInfo>>>,
    hash_ring: Arc<RwLock<HashRing<String>>>,
    gossip_handle: Option<ChitchatHandle>,
    consensus_manager: Arc<consensus::ConsensusManager>,
    health_monitor: Arc<health::HealthMonitor>,
    distribution_manager: Arc<distribution::DistributionManager>,
    replication_manager: Arc<replication::ReplicationManager>,
    event_tx: broadcast::Sender<ClusterEvent>,
    shutdown: Arc<Mutex<bool>>,
}

#[derive(Debug, Clone)]
pub enum ClusterEvent {
    NodeJoined(String),
    NodeLeft(String),
    NodeFailed(String),
    LeaderElected(String),
    ConfigChanged(String),
    RebalanceStarted,
    RebalanceCompleted,
    FailoverTriggered(String),
}

impl ClusterManager {
    pub async fn new(config: ClusterConfig) -> Result<Self> {
        let node_info = NodeInfo {
            id: config.node_id.clone(),
            name: hostname::get()?.to_string_lossy().to_string(),
            addr: config.bind_addr,
            grpc_addr: SocketAddr::new(config.bind_addr.ip(), config.grpc_port),
            state: NodeState::Joining,
            role: NodeRole::Follower,
            capacity: Self::detect_capacity(),
            load: NodeLoad {
                cpu_percent: 0.0,
                memory_percent: 0.0,
                disk_percent: 0.0,
                active_connections: 0,
                requests_per_second: 0.0,
                response_time_ms: 0.0,
            },
            version: env!("CARGO_PKG_VERSION").to_string(),
            started_at: SystemTime::now(),
            last_seen: SystemTime::now(),
            metadata: HashMap::new(),
        };

        let (event_tx, _) = broadcast::channel(1000);

        let consensus_manager = Arc::new(
            consensus::ConsensusManager::new(&config, event_tx.clone()).await?
        );

        let health_monitor = Arc::new(
            health::HealthMonitor::new(&config).await?
        );

        let distribution_manager = Arc::new(
            distribution::DistributionManager::new(&config).await?
        );

        let replication_manager = Arc::new(
            replication::ReplicationManager::new(&config).await?
        );

        Ok(ClusterManager {
            config: config.clone(),
            node_info: Arc::new(RwLock::new(node_info)),
            nodes: Arc::new(RwLock::new(HashMap::new())),
            hash_ring: Arc::new(RwLock::new(HashRing::new())),
            gossip_handle: None,
            consensus_manager,
            health_monitor,
            distribution_manager,
            replication_manager,
            event_tx,
            shutdown: Arc::new(Mutex::new(false)),
        })
    }

    pub async fn start(&mut self) -> Result<()> {
        info!("Starting cluster manager for node: {}", self.config.node_id);

        // Start gossip protocol
        self.start_gossip().await?;

        // Start consensus manager
        self.consensus_manager.start().await?;

        // Start health monitoring
        self.health_monitor.start(self.nodes.clone()).await?;

        // Join cluster
        if self.config.enable_auto_join {
            self.auto_join_cluster().await?;
        }

        // Start background tasks
        self.start_background_tasks().await?;

        // Update node state
        {
            let mut node = self.node_info.write().await;
            node.state = NodeState::Active;
        }

        info!("Cluster manager started successfully");
        Ok(())
    }

    async fn start_gossip(&mut self) -> Result<()> {
        let chitchat_config = ChitchatConfig {
            node_id: self.config.node_id.clone().into(),
            cluster_id: self.config.cluster_name.clone(),
            gossip_addr: self.config.bind_addr,
            gossip_interval: self.config.gossip_interval,
            listen_addr: self.config.advertise_addr.unwrap_or(self.config.bind_addr),
            seed_nodes: self.config.seed_nodes
                .iter()
                .map(|s| s.parse().unwrap())
                .collect(),
            failure_detector_config: FailureDetectorConfig {
                phi_threshold: 8.0,
                sampling_window: Duration::from_secs(60),
                min_std_deviation: Duration::from_millis(100),
                ..Default::default()
            },
            ..Default::default()
        };

        let (gossip_handle, gossip_stream) = chitchat::spawn_chitchat(
            chitchat_config,
            vec![],
            &tokio::runtime::Handle::current(),
        ).await?;

        self.gossip_handle = Some(gossip_handle);

        // Process gossip events
        let nodes = self.nodes.clone();
        let event_tx = self.event_tx.clone();
        tokio::spawn(async move {
            Self::process_gossip_events(gossip_stream, nodes, event_tx).await;
        });

        Ok(())
    }

    async fn process_gossip_events(
        mut stream: chitchat::ChitchatStream,
        nodes: Arc<RwLock<HashMap<String, NodeInfo>>>,
        event_tx: broadcast::Sender<ClusterEvent>,
    ) {
        while let Some(event) = stream.recv().await {
            match event {
                chitchat::ChitchatEvent::NodeJoined(node_id) => {
                    info!("Node joined cluster: {}", node_id);
                    let _ = event_tx.send(ClusterEvent::NodeJoined(node_id.to_string()));
                }
                chitchat::ChitchatEvent::NodeLeft(node_id) => {
                    info!("Node left cluster: {}", node_id);
                    nodes.write().await.remove(&node_id.to_string());
                    let _ = event_tx.send(ClusterEvent::NodeLeft(node_id.to_string()));
                }
                chitchat::ChitchatEvent::NodeFailed(node_id) => {
                    warn!("Node failed: {}", node_id);
                    if let Some(mut node) = nodes.write().await.get_mut(&node_id.to_string()) {
                        node.state = NodeState::Failed;
                    }
                    let _ = event_tx.send(ClusterEvent::NodeFailed(node_id.to_string()));
                }
                _ => {}
            }
        }
    }

    async fn auto_join_cluster(&self) -> Result<()> {
        info!("Auto-joining cluster: {}", self.config.cluster_name);

        // Try to connect to seed nodes
        for seed in &self.config.seed_nodes {
            match self.connect_to_node(seed).await {
                Ok(_) => {
                    info!("Connected to seed node: {}", seed);
                    break;
                }
                Err(e) => {
                    warn!("Failed to connect to seed node {}: {}", seed, e);
                }
            }
        }

        // Register with etcd if configured
        if !self.config.etcd_endpoints.is_empty() {
            self.register_with_etcd().await?;
        }

        Ok(())
    }

    async fn connect_to_node(&self, addr: &str) -> Result<()> {
        // Implement gRPC connection to node
        debug!("Connecting to node: {}", addr);
        // TODO: Implement actual gRPC connection
        Ok(())
    }

    async fn register_with_etcd(&self) -> Result<()> {
        let mut client = etcd_rs::Client::connect(
            etcd_rs::ClientConfig {
                endpoints: self.config.etcd_endpoints.clone(),
                auth: None,
                tls: None,
            },
            None,
        ).await?;

        let node_info = self.node_info.read().await;
        let key = format!("/clusters/{}/nodes/{}", self.config.cluster_name, node_info.id);
        let value = serde_json::to_string(&*node_info)?;

        client.put(
            etcd_rs::PutRequest::new(key.as_bytes(), value.as_bytes())
                .with_lease(60) // 60 second TTL
        ).await?;

        info!("Registered with etcd");
        Ok(())
    }

    async fn start_background_tasks(&self) -> Result<()> {
        // Heartbeat task
        let node_info = self.node_info.clone();
        let interval = self.config.heartbeat_interval;
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                ticker.tick().await;
                let mut node = node_info.write().await;
                node.last_seen = SystemTime::now();
                node.load = Self::measure_load().await;
            }
        });

        // Hash ring update task
        let nodes = self.nodes.clone();
        let hash_ring = self.hash_ring.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(5));
            loop {
                ticker.tick().await;
                Self::update_hash_ring(nodes.clone(), hash_ring.clone()).await;
            }
        });

        // Data sync task
        let replication_mgr = self.replication_manager.clone();
        let sync_interval = self.config.data_sync_interval;
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(sync_interval);
            loop {
                ticker.tick().await;
                if let Err(e) = replication_mgr.sync_data().await {
                    error!("Data sync failed: {}", e);
                }
            }
        });

        Ok(())
    }

    pub async fn get_node_for_key(&self, key: &str) -> Option<String> {
        let ring = self.hash_ring.read().await;
        ring.get(&key.to_string()).cloned()
    }

    pub async fn get_replicas_for_key(&self, key: &str) -> Vec<String> {
        let ring = self.hash_ring.read().await;
        let nodes = self.nodes.read().await;
        
        let mut replicas = Vec::new();
        if let Some(primary) = ring.get(&key.to_string()) {
            replicas.push(primary.clone());
            
            // Get additional replicas based on replication factor
            let active_nodes: Vec<_> = nodes.values()
                .filter(|n| n.state == NodeState::Active && n.id != *primary)
                .map(|n| n.id.clone())
                .collect();
            
            for i in 0..self.config.replication_factor.saturating_sub(1) {
                if i < active_nodes.len() {
                    replicas.push(active_nodes[i].clone());
                }
            }
        }
        
        replicas
    }

    pub async fn is_leader(&self) -> bool {
        let node = self.node_info.read().await;
        node.role == NodeRole::Leader
    }

    pub async fn get_leader(&self) -> Option<NodeInfo> {
        let nodes = self.nodes.read().await;
        nodes.values()
            .find(|n| n.role == NodeRole::Leader)
            .cloned()
    }

    pub async fn trigger_failover(&self, failed_node_id: &str) -> Result<()> {
        if !self.config.enable_auto_failover {
            return Ok(());
        }

        info!("Triggering failover for node: {}", failed_node_id);
        let _ = self.event_tx.send(ClusterEvent::FailoverTriggered(failed_node_id.to_string()));

        // Redistribute load from failed node
        self.distribution_manager.redistribute_load(failed_node_id).await?;

        // Update hash ring
        let mut ring = self.hash_ring.write().await;
        ring.remove(&failed_node_id.to_string());

        Ok(())
    }

    pub async fn rebalance_cluster(&self) -> Result<()> {
        info!("Starting cluster rebalance");
        let _ = self.event_tx.send(ClusterEvent::RebalanceStarted);

        // Calculate optimal distribution
        let nodes = self.nodes.read().await;
        let active_nodes: Vec<_> = nodes.values()
            .filter(|n| n.state == NodeState::Active)
            .collect();

        if active_nodes.is_empty() {
            return Err(anyhow!("No active nodes for rebalancing"));
        }

        // Rebalance based on node capacity and current load
        self.distribution_manager.rebalance(&active_nodes).await?;

        let _ = self.event_tx.send(ClusterEvent::RebalanceCompleted);
        info!("Cluster rebalance completed");
        
        Ok(())
    }

    pub async fn get_cluster_stats(&self) -> ClusterStats {
        let nodes = self.nodes.read().await;
        
        let total_nodes = nodes.len();
        let active_nodes = nodes.values().filter(|n| n.state == NodeState::Active).count();
        let failed_nodes = nodes.values().filter(|n| n.state == NodeState::Failed).count();
        
        let total_capacity = nodes.values().fold(0, |acc, n| acc + n.capacity.max_connections);
        let total_load = nodes.values().fold(0, |acc, n| acc + n.load.active_connections);
        
        let avg_cpu = nodes.values().map(|n| n.load.cpu_percent).sum::<f32>() / nodes.len() as f32;
        let avg_memory = nodes.values().map(|n| n.load.memory_percent).sum::<f32>() / nodes.len() as f32;
        
        ClusterStats {
            total_nodes,
            active_nodes,
            failed_nodes,
            total_capacity,
            total_load,
            avg_cpu_percent: avg_cpu,
            avg_memory_percent: avg_memory,
            replication_factor: self.config.replication_factor,
            quorum_size: self.config.quorum_size,
        }
    }

    pub async fn shutdown(&self) -> Result<()> {
        info!("Shutting down cluster manager");
        
        // Set shutdown flag
        *self.shutdown.lock().await = true;
        
        // Leave cluster gracefully
        {
            let mut node = self.node_info.write().await;
            node.state = NodeState::Leaving;
        }
        
        // Notify other nodes
        if let Some(handle) = &self.gossip_handle {
            handle.shutdown().await?;
        }
        
        // Stop background tasks
        self.consensus_manager.shutdown().await?;
        self.health_monitor.shutdown().await?;
        
        info!("Cluster manager shutdown complete");
        Ok(())
    }

    fn detect_capacity() -> NodeCapacity {
        NodeCapacity {
            cpu_cores: num_cpus::get() as u32,
            memory_mb: sys_info::mem_info()
                .map(|m| m.total / 1024)
                .unwrap_or(8192),
            disk_gb: sys_info::disk_info()
                .map(|d| d.total / 1024 / 1024)
                .unwrap_or(100),
            network_mbps: 1000, // Default assumption
            max_connections: 10000, // Default limit
        }
    }

    async fn measure_load() -> NodeLoad {
        NodeLoad {
            cpu_percent: sys_info::loadavg()
                .map(|l| (l.one * 100.0) as f32)
                .unwrap_or(0.0),
            memory_percent: sys_info::mem_info()
                .map(|m| ((m.total - m.avail) as f32 / m.total as f32) * 100.0)
                .unwrap_or(0.0),
            disk_percent: sys_info::disk_info()
                .map(|d| ((d.total - d.free) as f32 / d.total as f32) * 100.0)
                .unwrap_or(0.0),
            active_connections: 0, // Would be updated by connection tracking
            requests_per_second: 0.0, // Would be calculated from metrics
            response_time_ms: 0.0, // Would be calculated from metrics
        }
    }

    async fn update_hash_ring(
        nodes: Arc<RwLock<HashMap<String, NodeInfo>>>,
        hash_ring: Arc<RwLock<HashRing<String>>>,
    ) {
        let nodes = nodes.read().await;
        let mut ring = hash_ring.write().await;
        
        // Clear and rebuild ring with active nodes
        *ring = HashRing::new();
        for (id, node) in nodes.iter() {
            if node.state == NodeState::Active {
                // Add node multiple times for better distribution
                for i in 0..150 {
                    ring.add(format!("{}:{}", id, i));
                }
            }
        }
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<ClusterEvent> {
        self.event_tx.subscribe()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterStats {
    pub total_nodes: usize,
    pub active_nodes: usize,
    pub failed_nodes: usize,
    pub total_capacity: u32,
    pub total_load: u32,
    pub avg_cpu_percent: f32,
    pub avg_memory_percent: f32,
    pub replication_factor: usize,
    pub quorum_size: usize,
}

// External dependencies for capacity detection
use hostname;
use num_cpus;
use sys_info;