use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tonic::{transport::Server, Request, Response, Status};
use tracing::{debug, error, info};

use super::NodeInfo;

// Proto definitions
pub mod cluster_rpc {
    tonic::include_proto!("cluster");
}

use cluster_rpc::{
    cluster_rpc_server::{ClusterRpc, ClusterRpcServer},
    Empty, NodeStatus, NodeList, HeartbeatRequest, HeartbeatResponse,
    SyncRequest, SyncResponse, ElectionRequest, ElectionResponse,
    DataRequest, DataResponse, ReplicationRequest, ReplicationResponse,
};

pub struct ClusterService {
    nodes: Arc<RwLock<HashMap<String, NodeInfo>>>,
}

impl ClusterService {
    pub fn new(nodes: Arc<RwLock<HashMap<String, NodeInfo>>>) -> Self {
        Self { nodes }
    }
}

#[tonic::async_trait]
impl ClusterRpc for ClusterService {
    async fn heartbeat(
        &self,
        request: Request<HeartbeatRequest>,
    ) -> Result<Response<HeartbeatResponse>, Status> {
        let req = request.into_inner();
        debug!("Received heartbeat from node: {}", req.node_id);
        
        // Update node last seen time
        let mut nodes = self.nodes.write().await;
        if let Some(node) = nodes.get_mut(&req.node_id) {
            node.last_seen = std::time::SystemTime::now();
            node.load.cpu_percent = req.cpu_load;
            node.load.memory_percent = req.memory_load;
            node.load.active_connections = req.connections;
        }
        
        Ok(Response::new(HeartbeatResponse {
            success: true,
            leader_id: String::new(), // TODO: Get from Raft
            cluster_time: chrono::Utc::now().timestamp(),
        }))
    }
    
    async fn get_nodes(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<NodeList>, Status> {
        let nodes = self.nodes.read().await;
        
        let node_statuses: Vec<NodeStatus> = nodes.values().map(|n| NodeStatus {
            node_id: n.id.clone(),
            address: n.addr.to_string(),
            state: format!("{:?}", n.state),
            role: format!("{:?}", n.role),
            cpu_load: n.load.cpu_percent,
            memory_load: n.load.memory_percent,
            connections: n.load.active_connections,
            last_seen: n.last_seen.duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default().as_secs(),
        }).collect();
        
        Ok(Response::new(NodeList {
            nodes: node_statuses,
        }))
    }
    
    async fn sync_data(
        &self,
        request: Request<SyncRequest>,
    ) -> Result<Response<SyncResponse>, Status> {
        let req = request.into_inner();
        info!("Syncing data with node: {}", req.node_id);
        
        // TODO: Implement actual data synchronization
        Ok(Response::new(SyncResponse {
            success: true,
            data_version: 1,
            items_synced: 0,
        }))
    }
    
    async fn request_vote(
        &self,
        request: Request<ElectionRequest>,
    ) -> Result<Response<ElectionResponse>, Status> {
        let req = request.into_inner();
        info!("Vote requested by {} for term {}", req.candidate_id, req.term);
        
        // TODO: Implement Raft voting logic
        Ok(Response::new(ElectionResponse {
            term: req.term,
            vote_granted: true,
            voter_id: "node-1".to_string(),
        }))
    }
    
    async fn replicate_data(
        &self,
        request: Request<ReplicationRequest>,
    ) -> Result<Response<ReplicationResponse>, Status> {
        let req = request.into_inner();
        debug!("Replicating {} bytes to {} replicas", 
               req.data.len(), req.replica_count);
        
        // TODO: Implement data replication
        Ok(Response::new(ReplicationResponse {
            success: true,
            replicas_confirmed: req.replica_count,
            replication_time_ms: 10,
        }))
    }
    
    async fn get_data(
        &self,
        request: Request<DataRequest>,
    ) -> Result<Response<DataResponse>, Status> {
        let req = request.into_inner();
        debug!("Data requested for key: {}", req.key);
        
        // TODO: Implement distributed data retrieval
        Ok(Response::new(DataResponse {
            found: false,
            value: vec![],
            version: 0,
            owner_node: String::new(),
        }))
    }
}

// gRPC client for cluster communication
pub struct ClusterClient {
    clients: HashMap<String, cluster_rpc::cluster_rpc_client::ClusterRpcClient<tonic::transport::Channel>>,
}

impl ClusterClient {
    pub fn new() -> Self {
        Self {
            clients: HashMap::new(),
        }
    }
    
    pub async fn connect(&mut self, node_id: &str, addr: &str) -> Result<()> {
        let client = cluster_rpc::cluster_rpc_client::ClusterRpcClient::connect(
            format!("http://{}", addr)
        ).await?;
        
        self.clients.insert(node_id.to_string(), client);
        info!("Connected to cluster node: {} at {}", node_id, addr);
        Ok(())
    }
    
    pub async fn send_heartbeat(&mut self, target: &str, node_id: &str) -> Result<()> {
        if let Some(client) = self.clients.get_mut(target) {
            let request = HeartbeatRequest {
                node_id: node_id.to_string(),
                cpu_load: 0.0, // TODO: Get actual metrics
                memory_load: 0.0,
                connections: 0,
                timestamp: chrono::Utc::now().timestamp(),
            };
            
            client.heartbeat(request).await?;
        }
        Ok(())
    }
    
    pub async fn get_cluster_nodes(&mut self, target: &str) -> Result<Vec<NodeStatus>> {
        if let Some(client) = self.clients.get_mut(target) {
            let response = client.get_nodes(Empty {}).await?;
            return Ok(response.into_inner().nodes);
        }
        Ok(vec![])
    }
}

// Proto file content (save as proto/cluster.proto)
pub const CLUSTER_PROTO: &str = r#"
syntax = "proto3";

package cluster;

service ClusterRpc {
    rpc Heartbeat(HeartbeatRequest) returns (HeartbeatResponse);
    rpc GetNodes(Empty) returns (NodeList);
    rpc SyncData(SyncRequest) returns (SyncResponse);
    rpc RequestVote(ElectionRequest) returns (ElectionResponse);
    rpc ReplicateData(ReplicationRequest) returns (ReplicationResponse);
    rpc GetData(DataRequest) returns (DataResponse);
}

message Empty {}

message HeartbeatRequest {
    string node_id = 1;
    float cpu_load = 2;
    float memory_load = 3;
    uint32 connections = 4;
    int64 timestamp = 5;
}

message HeartbeatResponse {
    bool success = 1;
    string leader_id = 2;
    int64 cluster_time = 3;
}

message NodeStatus {
    string node_id = 1;
    string address = 2;
    string state = 3;
    string role = 4;
    float cpu_load = 5;
    float memory_load = 6;
    uint32 connections = 7;
    uint64 last_seen = 8;
}

message NodeList {
    repeated NodeStatus nodes = 1;
}

message SyncRequest {
    string node_id = 1;
    uint64 last_sync_version = 2;
    repeated string keys = 3;
}

message SyncResponse {
    bool success = 1;
    uint64 data_version = 2;
    uint32 items_synced = 3;
}

message ElectionRequest {
    uint64 term = 1;
    string candidate_id = 2;
    uint64 last_log_index = 3;
    uint64 last_log_term = 4;
}

message ElectionResponse {
    uint64 term = 1;
    bool vote_granted = 2;
    string voter_id = 3;
}

message ReplicationRequest {
    string key = 1;
    bytes data = 2;
    uint32 replica_count = 3;
    uint64 version = 4;
}

message ReplicationResponse {
    bool success = 1;
    uint32 replicas_confirmed = 2;
    uint64 replication_time_ms = 3;
}

message DataRequest {
    string key = 1;
    bool include_metadata = 2;
}

message DataResponse {
    bool found = 1;
    bytes value = 2;
    uint64 version = 3;
    string owner_node = 4;
}
"#;