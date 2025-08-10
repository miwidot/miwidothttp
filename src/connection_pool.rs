use anyhow::Result;
use deadpool::managed::{Manager, Pool, PoolConfig, RecycleResult};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::RwLock;

pub struct ConnectionPool {
    pools: Arc<RwLock<HashMap<String, Pool<TcpConnectionManager>>>>,
    max_size: usize,
    idle_timeout: Duration,
}

impl ConnectionPool {
    pub async fn new(max_size: usize, idle_timeout: Duration) -> Result<Self> {
        Ok(Self {
            pools: Arc::new(RwLock::new(HashMap::new())),
            max_size,
            idle_timeout,
        })
    }
    
    pub async fn get_connection(&self, host: &str, port: u16) -> Result<deadpool::managed::Object<TcpConnectionManager>> {
        let key = format!("{}:{}", host, port);
        
        let pools = self.pools.read().await;
        
        if let Some(pool) = pools.get(&key) {
            return Ok(pool.get().await?);
        }
        
        drop(pools);
        
        // Create new pool for this host
        let mut pools = self.pools.write().await;
        
        if !pools.contains_key(&key) {
            let manager = TcpConnectionManager {
                host: host.to_string(),
                port,
            };
            
            let config = PoolConfig {
                max_size: self.max_size,
                timeouts: deadpool::managed::Timeouts {
                    wait: Some(Duration::from_secs(30)),
                    create: Some(Duration::from_secs(30)),
                    recycle: Some(Duration::from_secs(30)),
                },
            };
            
            let pool = Pool::builder(manager)
                .config(config)
                .build()?;
            
            pools.insert(key.clone(), pool);
        }
        
        Ok(pools.get(&key).unwrap().get().await?)
    }
    
    pub async fn stats(&self) -> ConnectionPoolStats {
        let pools = self.pools.read().await;
        
        let mut total_size = 0;
        let mut total_available = 0;
        let mut total_waiting = 0;
        
        for pool in pools.values() {
            let status = pool.status();
            total_size += status.size;
            total_available += status.available;
            total_waiting += status.waiting;
        }
        
        ConnectionPoolStats {
            total_pools: pools.len(),
            total_size,
            total_available,
            total_waiting,
        }
    }
}

pub struct TcpConnectionManager {
    host: String,
    port: u16,
}

#[async_trait::async_trait]
impl Manager for TcpConnectionManager {
    type Type = TcpStream;
    type Error = anyhow::Error;
    
    async fn create(&self) -> Result<TcpStream, Self::Error> {
        let addr = format!("{}:{}", self.host, self.port);
        Ok(TcpStream::connect(addr).await?)
    }
    
    async fn recycle(&self, conn: &mut TcpStream, _: &deadpool::managed::Metrics) -> RecycleResult<Self::Error> {
        // Check if connection is still alive
        match conn.peer_addr() {
            Ok(_) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub struct ConnectionPoolStats {
    pub total_pools: usize,
    pub total_size: usize,
    pub total_available: usize,
    pub total_waiting: usize,
}