use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, Mutex, broadcast};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use super::{ClusterConfig, ClusterEvent, NodeRole};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaftState {
    pub current_term: u64,
    pub voted_for: Option<String>,
    pub log: Vec<LogEntry>,
    pub commit_index: u64,
    pub last_applied: u64,
    pub next_index: HashMap<String, u64>,
    pub match_index: HashMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub term: u64,
    pub index: u64,
    pub command: Command,
    pub timestamp: Instant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    NoOp,
    ConfigChange(ConfigChange),
    StateUpdate(StateUpdate),
    Custom(Vec<u8>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigChange {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateUpdate {
    pub node_id: String,
    pub state_data: Vec<u8>,
}

pub struct ConsensusManager {
    config: ClusterConfig,
    node_id: String,
    role: Arc<RwLock<NodeRole>>,
    state: Arc<RwLock<RaftState>>,
    leader_id: Arc<RwLock<Option<String>>>,
    election_timeout: Arc<Mutex<Option<tokio::time::Instant>>>,
    heartbeat_interval: Duration,
    event_tx: broadcast::Sender<ClusterEvent>,
    shutdown: Arc<Mutex<bool>>,
}

impl ConsensusManager {
    pub async fn new(config: &ClusterConfig, event_tx: broadcast::Sender<ClusterEvent>) -> Result<Self> {
        let state = RaftState {
            current_term: 0,
            voted_for: None,
            log: vec![LogEntry {
                term: 0,
                index: 0,
                command: Command::NoOp,
                timestamp: Instant::now(),
            }],
            commit_index: 0,
            last_applied: 0,
            next_index: HashMap::new(),
            match_index: HashMap::new(),
        };

        Ok(ConsensusManager {
            config: config.clone(),
            node_id: config.node_id.clone(),
            role: Arc::new(RwLock::new(NodeRole::Follower)),
            state: Arc::new(RwLock::new(state)),
            leader_id: Arc::new(RwLock::new(None)),
            election_timeout: Arc::new(Mutex::new(None)),
            heartbeat_interval: config.heartbeat_interval,
            event_tx,
            shutdown: Arc::new(Mutex::new(false)),
        })
    }

    pub async fn start(&self) -> Result<()> {
        info!("Starting consensus manager for node: {}", self.node_id);

        // Start election timer
        self.reset_election_timer().await;

        // Start main consensus loop
        let manager = self.clone();
        tokio::spawn(async move {
            manager.consensus_loop().await;
        });

        Ok(())
    }

    async fn consensus_loop(&self) {
        let mut ticker = tokio::time::interval(Duration::from_millis(100));
        
        loop {
            ticker.tick().await;

            if *self.shutdown.lock().await {
                break;
            }

            let role = self.role.read().await.clone();

            match role {
                NodeRole::Leader => {
                    self.leader_duties().await;
                }
                NodeRole::Candidate => {
                    self.candidate_duties().await;
                }
                NodeRole::Follower => {
                    self.follower_duties().await;
                }
                NodeRole::Observer => {
                    // Observers don't participate in consensus
                }
            }
        }
    }

    async fn leader_duties(&self) {
        // Send heartbeats to all followers
        self.send_heartbeats().await;

        // Process client requests
        self.process_client_requests().await;

        // Replicate log entries
        self.replicate_log_entries().await;
    }

    async fn candidate_duties(&self) {
        // Check election timeout
        if self.is_election_timeout().await {
            self.start_election().await;
        }
    }

    async fn follower_duties(&self) {
        // Check for election timeout
        if self.is_election_timeout().await {
            info!("Election timeout reached, becoming candidate");
            self.become_candidate().await;
        }
    }

    async fn start_election(&self) {
        info!("Starting leader election");

        // Increment current term
        let mut state = self.state.write().await;
        state.current_term += 1;
        let current_term = state.current_term;
        state.voted_for = Some(self.node_id.clone());
        drop(state);

        // Reset election timer
        self.reset_election_timer().await;

        // Request votes from other nodes
        let votes_needed = (self.config.quorum_size + 1) / 2;
        let mut votes_received = 1; // Vote for self

        // Simulate vote collection (would use gRPC in real implementation)
        let vote_responses = self.request_votes(current_term).await;

        for response in vote_responses {
            if response.vote_granted {
                votes_received += 1;
                if votes_received >= votes_needed {
                    info!("Won election with {} votes", votes_received);
                    self.become_leader().await;
                    return;
                }
            } else if response.term > current_term {
                // Found a node with higher term, become follower
                self.become_follower(response.term).await;
                return;
            }
        }

        // Not enough votes, remain candidate or become follower
        info!("Election failed, received {} votes, needed {}", votes_received, votes_needed);
        self.become_follower(current_term).await;
    }

    async fn become_leader(&self) {
        info!("Becoming leader for term {}", self.state.read().await.current_term);
        
        *self.role.write().await = NodeRole::Leader;
        *self.leader_id.write().await = Some(self.node_id.clone());
        
        // Initialize next_index and match_index for all nodes
        let mut state = self.state.write().await;
        let last_log_index = state.log.last().map(|e| e.index).unwrap_or(0);
        
        // In real implementation, would get list of all nodes
        let nodes = vec![]; // Placeholder
        for node_id in nodes {
            state.next_index.insert(node_id.clone(), last_log_index + 1);
            state.match_index.insert(node_id, 0);
        }
        
        drop(state);
        
        // Send initial heartbeat immediately
        self.send_heartbeats().await;
        
        // Notify cluster of new leader
        let _ = self.event_tx.send(ClusterEvent::LeaderElected(self.node_id.clone()));
    }

    async fn become_candidate(&self) {
        info!("Becoming candidate");
        
        *self.role.write().await = NodeRole::Candidate;
        *self.leader_id.write().await = None;
        
        // Start election
        self.start_election().await;
    }

    async fn become_follower(&self, term: u64) {
        info!("Becoming follower for term {}", term);
        
        *self.role.write().await = NodeRole::Follower;
        
        let mut state = self.state.write().await;
        state.current_term = term;
        state.voted_for = None;
        
        self.reset_election_timer().await;
    }

    async fn send_heartbeats(&self) {
        debug!("Sending heartbeats");
        
        // In real implementation, send AppendEntries RPC to all nodes
        // For now, we'll simulate this
        let state = self.state.read().await;
        let heartbeat = AppendEntriesRequest {
            term: state.current_term,
            leader_id: self.node_id.clone(),
            prev_log_index: state.log.last().map(|e| e.index).unwrap_or(0),
            prev_log_term: state.log.last().map(|e| e.term).unwrap_or(0),
            entries: vec![],
            leader_commit: state.commit_index,
        };
        
        // Send to all followers (would use gRPC)
        self.broadcast_append_entries(heartbeat).await;
    }

    async fn process_client_requests(&self) {
        // Process any pending client requests
        // In real implementation, would dequeue from request queue
    }

    async fn replicate_log_entries(&self) {
        // Replicate any uncommitted log entries to followers
        let state = self.state.read().await;
        
        for (node_id, next_idx) in &state.next_index {
            if *next_idx <= state.log.len() as u64 {
                // Send log entries from next_idx onwards
                let entries: Vec<LogEntry> = state.log
                    .iter()
                    .skip(*next_idx as usize - 1)
                    .cloned()
                    .collect();
                
                if !entries.is_empty() {
                    debug!("Replicating {} entries to {}", entries.len(), node_id);
                    // Send AppendEntries RPC with entries
                }
            }
        }
    }

    async fn request_votes(&self, term: u64) -> Vec<VoteResponse> {
        // In real implementation, send RequestVote RPC to all nodes
        // For simulation, return empty vec
        vec![]
    }

    async fn broadcast_append_entries(&self, request: AppendEntriesRequest) {
        // In real implementation, send to all nodes via gRPC
        debug!("Broadcasting append entries for term {}", request.term);
    }

    async fn reset_election_timer(&self) {
        let timeout_duration = Duration::from_millis(
            rand::random::<u64>() % 150 + 150 // Random between 150-300ms
        );
        
        let timeout_instant = tokio::time::Instant::now() + timeout_duration;
        *self.election_timeout.lock().await = Some(timeout_instant);
        
        debug!("Reset election timer to {:?}", timeout_duration);
    }

    async fn is_election_timeout(&self) -> bool {
        if let Some(timeout) = *self.election_timeout.lock().await {
            tokio::time::Instant::now() >= timeout
        } else {
            false
        }
    }

    pub async fn propose_command(&self, command: Command) -> Result<u64> {
        // Only leader can propose commands
        if *self.role.read().await != NodeRole::Leader {
            return Err(anyhow::anyhow!("Not the leader"));
        }

        let mut state = self.state.write().await;
        let index = state.log.len() as u64;
        
        let entry = LogEntry {
            term: state.current_term,
            index,
            command,
            timestamp: Instant::now(),
        };
        
        state.log.push(entry);
        
        Ok(index)
    }

    pub async fn get_leader(&self) -> Option<String> {
        self.leader_id.read().await.clone()
    }

    pub async fn is_leader(&self) -> bool {
        *self.role.read().await == NodeRole::Leader
    }

    pub async fn get_state(&self) -> RaftState {
        self.state.read().await.clone()
    }

    pub async fn handle_append_entries(&self, request: AppendEntriesRequest) -> AppendEntriesResponse {
        let mut state = self.state.write().await;
        
        // Reply false if term < currentTerm
        if request.term < state.current_term {
            return AppendEntriesResponse {
                term: state.current_term,
                success: false,
            };
        }

        // If RPC request or response contains term T > currentTerm:
        // set currentTerm = T, convert to follower
        if request.term > state.current_term {
            state.current_term = request.term;
            state.voted_for = None;
            drop(state);
            self.become_follower(request.term).await;
            state = self.state.write().await;
        }

        // Reset election timer
        self.reset_election_timer().await;

        // Update leader
        *self.leader_id.write().await = Some(request.leader_id.clone());

        // Reply false if log doesn't contain an entry at prevLogIndex
        // whose term matches prevLogTerm
        if request.prev_log_index > 0 {
            if state.log.len() < request.prev_log_index as usize {
                return AppendEntriesResponse {
                    term: state.current_term,
                    success: false,
                };
            }
            
            let prev_entry = &state.log[request.prev_log_index as usize - 1];
            if prev_entry.term != request.prev_log_term {
                return AppendEntriesResponse {
                    term: state.current_term,
                    success: false,
                };
            }
        }

        // Append new entries
        for entry in request.entries {
            state.log.push(entry);
        }

        // Update commit index
        if request.leader_commit > state.commit_index {
            state.commit_index = request.leader_commit.min(state.log.len() as u64 - 1);
        }

        AppendEntriesResponse {
            term: state.current_term,
            success: true,
        }
    }

    pub async fn handle_request_vote(&self, request: VoteRequest) -> VoteResponse {
        let mut state = self.state.write().await;
        
        // Reply false if term < currentTerm
        if request.term < state.current_term {
            return VoteResponse {
                term: state.current_term,
                vote_granted: false,
            };
        }

        // If RPC request contains term T > currentTerm:
        // set currentTerm = T, convert to follower
        if request.term > state.current_term {
            state.current_term = request.term;
            state.voted_for = None;
        }

        // Check if we can vote for this candidate
        let can_vote = state.voted_for.is_none() 
            || state.voted_for.as_ref() == Some(&request.candidate_id);

        // Check if candidate's log is at least as up-to-date as ours
        let last_log_index = state.log.last().map(|e| e.index).unwrap_or(0);
        let last_log_term = state.log.last().map(|e| e.term).unwrap_or(0);
        
        let log_ok = request.last_log_term > last_log_term
            || (request.last_log_term == last_log_term && request.last_log_index >= last_log_index);

        let vote_granted = can_vote && log_ok;

        if vote_granted {
            state.voted_for = Some(request.candidate_id);
            self.reset_election_timer().await;
        }

        VoteResponse {
            term: state.current_term,
            vote_granted,
        }
    }

    pub async fn shutdown(&self) -> Result<()> {
        info!("Shutting down consensus manager");
        *self.shutdown.lock().await = true;
        Ok(())
    }
}

impl Clone for ConsensusManager {
    fn clone(&self) -> Self {
        ConsensusManager {
            config: self.config.clone(),
            node_id: self.node_id.clone(),
            role: self.role.clone(),
            state: self.state.clone(),
            leader_id: self.leader_id.clone(),
            election_timeout: self.election_timeout.clone(),
            heartbeat_interval: self.heartbeat_interval,
            event_tx: self.event_tx.clone(),
            shutdown: self.shutdown.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppendEntriesRequest {
    term: u64,
    leader_id: String,
    prev_log_index: u64,
    prev_log_term: u64,
    entries: Vec<LogEntry>,
    leader_commit: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppendEntriesResponse {
    term: u64,
    success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VoteRequest {
    term: u64,
    candidate_id: String,
    last_log_index: u64,
    last_log_term: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VoteResponse {
    term: u64,
    vote_granted: bool,
}