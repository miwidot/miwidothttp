use async_graphql::{Context, EmptyMutation, EmptySubscription, Object, Schema, SimpleObject};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, SimpleObject, Serialize, Deserialize)]
pub struct ServerStatus {
    pub version: String,
    pub uptime: u64,
    pub requests_total: u64,
    pub active_connections: u32,
    pub cpu_usage: f32,
    pub memory_usage: f32,
}

#[derive(Debug, Clone, SimpleObject, Serialize, Deserialize)]
pub struct BackendInfo {
    pub name: String,
    pub url: String,
    pub health: String,
    pub requests: u64,
    pub errors: u64,
}

pub struct QueryRoot;

#[Object]
impl QueryRoot {
    async fn server_status(&self) -> ServerStatus {
        ServerStatus {
            version: "0.1.0".to_string(),
            uptime: 3600,
            requests_total: 10000,
            active_connections: 50,
            cpu_usage: 25.5,
            memory_usage: 35.2,
        }
    }
    
    async fn backends(&self) -> Vec<BackendInfo> {
        vec![
            BackendInfo {
                name: "api".to_string(),
                url: "http://localhost:3000".to_string(),
                health: "healthy".to_string(),
                requests: 5000,
                errors: 2,
            },
            BackendInfo {
                name: "static".to_string(),
                url: "/".to_string(),
                health: "healthy".to_string(),
                requests: 5000,
                errors: 0,
            },
        ]
    }
    
    async fn health(&self) -> bool {
        true
    }
}

pub struct MutationRoot;

#[Object]
impl MutationRoot {
    async fn reload_config(&self) -> bool {
        // Reload configuration
        true
    }
    
    async fn clear_cache(&self) -> bool {
        // Clear cache
        true
    }
}

pub struct SubscriptionRoot;

#[async_graphql::Subscription]
impl SubscriptionRoot {
    async fn metrics(&self) -> impl futures::Stream<Item = ServerStatus> {
        use futures::stream;
        use tokio::time::{interval, Duration};
        
        let interval = interval(Duration::from_secs(1));
        
        stream::unfold((0u64, interval), move |(counter, mut interval)| async move {
            interval.tick().await;
            Some((
                ServerStatus {
                    version: "0.1.0".to_string(),
                    uptime: counter,
                    requests_total: counter * 100,
                    active_connections: (counter % 100) as u32,
                    cpu_usage: (counter % 50) as f32,
                    memory_usage: (counter % 40) as f32,
                },
                (counter + 1, interval),
            ))
        })
    }
}

pub async fn create_schema() -> Result<Schema<QueryRoot, MutationRoot, SubscriptionRoot>, anyhow::Error> {
    Ok(Schema::new(QueryRoot, MutationRoot, SubscriptionRoot))
}