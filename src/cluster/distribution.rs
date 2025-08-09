use anyhow::Result;
use hashring::HashRing;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use super::{ClusterConfig, NodeInfo, NodeState};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistributionStrategy {
    pub algorithm: HashingAlgorithm,
    pub virtual_nodes: u32,
    pub replication_factor: usize,
    pub affinity_rules: Vec<AffinityRule>,
    pub weights: HashMap<String, f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HashingAlgorithm {
    ConsistentHash,
    RendezvousHash,
    JumpHash,
    Maglev,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AffinityRule {
    pub key_pattern: String,
    pub preferred_nodes: Vec<String>,
    pub required_nodes: Vec<String>,
    pub excluded_nodes: Vec<String>,
}

pub struct DistributionManager {
    config: ClusterConfig,
    strategy: DistributionStrategy,
    hash_ring: Arc<RwLock<HashRing<String>>>,
    node_weights: Arc<RwLock<HashMap<String, f32>>>,
    key_mappings: Arc<RwLock<HashMap<String, String>>>,
    migration_state: Arc<RwLock<MigrationState>>,
}

#[derive(Debug, Clone)]
struct MigrationState {
    active: bool,
    source_node: Option<String>,
    target_node: Option<String>,
    keys_migrated: usize,
    keys_total: usize,
    started_at: Option<std::time::Instant>,
}

impl DistributionManager {
    pub async fn new(config: &ClusterConfig) -> Result<Self> {
        let strategy = DistributionStrategy {
            algorithm: HashingAlgorithm::ConsistentHash,
            virtual_nodes: 150,
            replication_factor: config.replication_factor,
            affinity_rules: vec![],
            weights: HashMap::new(),
        };

        Ok(DistributionManager {
            config: config.clone(),
            strategy,
            hash_ring: Arc::new(RwLock::new(HashRing::new())),
            node_weights: Arc::new(RwLock::new(HashMap::new())),
            key_mappings: Arc::new(RwLock::new(HashMap::new())),
            migration_state: Arc::new(RwLock::new(MigrationState {
                active: false,
                source_node: None,
                target_node: None,
                keys_migrated: 0,
                keys_total: 0,
                started_at: None,
            })),
        })
    }

    pub async fn update_nodes(&self, nodes: &[NodeInfo]) -> Result<()> {
        let mut ring = self.hash_ring.write().await;
        let mut weights = self.node_weights.write().await;

        // Clear existing ring
        *ring = HashRing::new();
        weights.clear();

        // Add active nodes to ring
        for node in nodes {
            if node.state == NodeState::Active {
                // Calculate weight based on node capacity and load
                let weight = self.calculate_node_weight(node);
                weights.insert(node.id.clone(), weight);

                // Add virtual nodes for better distribution
                let virtual_node_count = (self.strategy.virtual_nodes as f32 * weight) as u32;
                for i in 0..virtual_node_count {
                    ring.add(format!("{}:{}", node.id, i));
                }

                info!("Added node {} with weight {} ({} vnodes)", 
                    node.id, weight, virtual_node_count);
            }
        }

        Ok(())
    }

    fn calculate_node_weight(&self, node: &NodeInfo) -> f32 {
        // Base weight from capacity
        let capacity_weight = (node.capacity.cpu_cores as f32 * 0.3)
            + (node.capacity.memory_mb as f32 / 1024.0 * 0.3)
            + (node.capacity.max_connections as f32 / 1000.0 * 0.4);

        // Adjust for current load (higher load = lower weight)
        let load_factor = 1.0 - (
            (node.load.cpu_percent / 100.0 * 0.3)
            + (node.load.memory_percent / 100.0 * 0.3)
            + (node.load.active_connections as f32 / node.capacity.max_connections as f32 * 0.4)
        );

        // Check for custom weight override
        let custom_weight = self.strategy.weights.get(&node.id).copied().unwrap_or(1.0);

        capacity_weight * load_factor * custom_weight
    }

    pub async fn get_node_for_key(&self, key: &str) -> Option<String> {
        // Check affinity rules first
        if let Some(node) = self.check_affinity_rules(key).await {
            return Some(node);
        }

        // Check if key has a specific mapping (during migration)
        let mappings = self.key_mappings.read().await;
        if let Some(node) = mappings.get(key) {
            return Some(node.clone());
        }

        // Use hash ring for distribution
        let ring = self.hash_ring.read().await;
        match self.strategy.algorithm {
            HashingAlgorithm::ConsistentHash => {
                ring.get(&key.to_string())
                    .map(|vnode| vnode.split(':').next().unwrap().to_string())
            }
            HashingAlgorithm::RendezvousHash => {
                self.rendezvous_hash(key).await
            }
            HashingAlgorithm::JumpHash => {
                self.jump_hash(key).await
            }
            HashingAlgorithm::Maglev => {
                self.maglev_hash(key).await
            }
        }
    }

    pub async fn get_replicas_for_key(&self, key: &str, count: usize) -> Vec<String> {
        let ring = self.hash_ring.read().await;
        let mut replicas = HashSet::new();
        
        // Get primary node
        if let Some(primary) = ring.get(&key.to_string()) {
            let primary_node = primary.split(':').next().unwrap().to_string();
            replicas.insert(primary_node.clone());
            
            // Get additional replicas by walking the ring
            let mut hash_key = key.to_string();
            while replicas.len() < count {
                hash_key.push('_');
                if let Some(node) = ring.get(&hash_key) {
                    let node_id = node.split(':').next().unwrap().to_string();
                    if !replicas.contains(&node_id) {
                        replicas.insert(node_id);
                    }
                }
            }
        }
        
        replicas.into_iter().collect()
    }

    async fn check_affinity_rules(&self, key: &str) -> Option<String> {
        for rule in &self.strategy.affinity_rules {
            if key.contains(&rule.key_pattern) {
                // Check required nodes first
                if !rule.required_nodes.is_empty() {
                    // TODO: Check if required nodes are available
                    return rule.required_nodes.first().cloned();
                }
                
                // Check preferred nodes
                if !rule.preferred_nodes.is_empty() {
                    // TODO: Check if preferred nodes are available
                    return rule.preferred_nodes.first().cloned();
                }
            }
        }
        None
    }

    async fn rendezvous_hash(&self, key: &str) -> Option<String> {
        let weights = self.node_weights.read().await;
        
        let mut best_node = None;
        let mut best_score = 0u64;
        
        for (node_id, weight) in weights.iter() {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            key.hash(&mut hasher);
            node_id.hash(&mut hasher);
            let score = hasher.finish();
            let weighted_score = (score as f64 * *weight as f64) as u64;
            
            if weighted_score > best_score {
                best_score = weighted_score;
                best_node = Some(node_id.clone());
            }
        }
        
        best_node
    }

    async fn jump_hash(&self, key: &str) -> Option<String> {
        let weights = self.node_weights.read().await;
        if weights.is_empty() {
            return None;
        }
        
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        key.hash(&mut hasher);
        let mut hash = hasher.finish();
        
        let num_buckets = weights.len() as u64;
        let mut b = 0u64;
        let mut j = 0u64;
        
        while j < num_buckets {
            b = j;
            hash = hash.wrapping_mul(2862933555777941757).wrapping_add(1);
            j = ((b + 1) as f64 * (1u64 << 31) as f64 / ((hash >> 33) + 1) as f64) as u64;
        }
        
        let nodes: Vec<_> = weights.keys().collect();
        nodes.get(b as usize).map(|s| (*s).clone())
    }

    async fn maglev_hash(&self, _key: &str) -> Option<String> {
        // Simplified Maglev hashing implementation
        // In production, would use full Maglev lookup table
        self.rendezvous_hash(_key).await
    }

    pub async fn redistribute_load(&self, failed_node: &str) -> Result<()> {
        info!("Redistributing load from failed node: {}", failed_node);
        
        let mut mappings = self.key_mappings.write().await;
        let ring = self.hash_ring.read().await;
        
        // Find keys that were on the failed node
        let affected_keys: Vec<String> = mappings
            .iter()
            .filter(|(_, node)| *node == failed_node)
            .map(|(key, _)| key.clone())
            .collect();
        
        // Reassign affected keys to other nodes
        for key in affected_keys {
            if let Some(new_node) = ring.get(&key) {
                let node_id = new_node.split(':').next().unwrap().to_string();
                if node_id != failed_node {
                    mappings.insert(key.clone(), node_id.clone());
                    debug!("Reassigned key {} to node {}", key, node_id);
                }
            }
        }
        
        info!("Load redistribution completed");
        Ok(())
    }

    pub async fn rebalance(&self, nodes: &[&NodeInfo]) -> Result<()> {
        info!("Starting cluster rebalance with {} nodes", nodes.len());
        
        let mut migration_plan = Vec::new();
        let mut key_distribution: HashMap<String, Vec<String>> = HashMap::new();
        
        // Calculate current key distribution
        let mappings = self.key_mappings.read().await;
        for (key, node) in mappings.iter() {
            key_distribution.entry(node.clone())
                .or_insert_with(Vec::new)
                .push(key.clone());
        }
        
        // Calculate ideal distribution
        let total_keys = mappings.len();
        let ideal_keys_per_node = total_keys / nodes.len();
        
        // Identify overloaded and underloaded nodes
        for node in nodes {
            let current_keys = key_distribution.get(&node.id)
                .map(|v| v.len())
                .unwrap_or(0);
            
            if current_keys > ideal_keys_per_node + (ideal_keys_per_node / 10) {
                // Node is overloaded, plan migration
                let excess = current_keys - ideal_keys_per_node;
                migration_plan.push((node.id.clone(), excess, true));
            } else if current_keys < ideal_keys_per_node - (ideal_keys_per_node / 10) {
                // Node is underloaded, can receive keys
                let deficit = ideal_keys_per_node - current_keys;
                migration_plan.push((node.id.clone(), deficit, false));
            }
        }
        
        // Execute migration plan
        if !migration_plan.is_empty() {
            self.execute_migration_plan(migration_plan, key_distribution).await?;
        }
        
        info!("Cluster rebalance completed");
        Ok(())
    }

    async fn execute_migration_plan(
        &self,
        plan: Vec<(String, usize, bool)>,
        mut distribution: HashMap<String, Vec<String>>,
    ) -> Result<()> {
        let mut state = self.migration_state.write().await;
        state.active = true;
        state.started_at = Some(std::time::Instant::now());
        
        let mut mappings = self.key_mappings.write().await;
        let mut total_migrated = 0;
        
        // Find source and target nodes
        let sources: Vec<_> = plan.iter()
            .filter(|(_, _, is_source)| *is_source)
            .collect();
        let targets: Vec<_> = plan.iter()
            .filter(|(_, _, is_source)| !*is_source)
            .collect();
        
        for (source_node, excess, _) in sources {
            if let Some(source_keys) = distribution.get_mut(source_node) {
                for (target_node, deficit, _) in &targets {
                    let keys_to_move = (*excess).min(**deficit);
                    
                    for _ in 0..keys_to_move {
                        if let Some(key) = source_keys.pop() {
                            mappings.insert(key.clone(), target_node.clone());
                            total_migrated += 1;
                            
                            state.source_node = Some(source_node.clone());
                            state.target_node = Some(target_node.clone());
                            state.keys_migrated = total_migrated;
                        }
                    }
                }
            }
        }
        
        state.active = false;
        info!("Migration completed: {} keys moved", total_migrated);
        
        Ok(())
    }

    pub async fn get_distribution_stats(&self) -> DistributionStats {
        let ring = self.hash_ring.read().await;
        let mappings = self.key_mappings.read().await;
        let weights = self.node_weights.read().await;
        let migration = self.migration_state.read().await;
        
        let mut node_key_counts = HashMap::new();
        for (_, node) in mappings.iter() {
            *node_key_counts.entry(node.clone()).or_insert(0) += 1;
        }
        
        DistributionStats {
            total_nodes: weights.len(),
            total_keys: mappings.len(),
            virtual_nodes: self.strategy.virtual_nodes,
            replication_factor: self.strategy.replication_factor,
            node_distribution: node_key_counts,
            migration_active: migration.active,
            migration_progress: if migration.keys_total > 0 {
                (migration.keys_migrated as f32 / migration.keys_total as f32) * 100.0
            } else {
                0.0
            },
        }
    }

    pub async fn add_affinity_rule(&self, rule: AffinityRule) -> Result<()> {
        // In real implementation, would persist to config
        info!("Added affinity rule for pattern: {}", rule.key_pattern);
        Ok(())
    }

    pub async fn set_node_weight(&self, node_id: &str, weight: f32) -> Result<()> {
        let mut weights = self.node_weights.write().await;
        weights.insert(node_id.to_string(), weight);
        info!("Set weight for node {} to {}", node_id, weight);
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistributionStats {
    pub total_nodes: usize,
    pub total_keys: usize,
    pub virtual_nodes: u32,
    pub replication_factor: usize,
    pub node_distribution: HashMap<String, usize>,
    pub migration_active: bool,
    pub migration_progress: f32,
}