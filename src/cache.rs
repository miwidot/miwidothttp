use anyhow::Result;
use moka::future::Cache as MokaCache;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct CacheConfig {
    pub memory_capacity: u64,
    pub redis_url: Option<String>,
    pub disk_path: Option<String>,
    pub ttl_seconds: u64,
}

pub struct CacheManager {
    memory_cache: MokaCache<String, Vec<u8>>,
    redis_conn: Option<ConnectionManager>,
    disk_cache: Option<cacache::AsyncCache>,
    config: CacheConfig,
}

impl CacheManager {
    pub async fn new(config: CacheConfig) -> Result<Self> {
        // Create memory cache
        let memory_cache = MokaCache::builder()
            .max_capacity(config.memory_capacity)
            .time_to_live(Duration::from_secs(config.ttl_seconds))
            .build();
        
        // Create Redis connection if configured
        let redis_conn = if let Some(url) = &config.redis_url {
            let client = redis::Client::open(url.as_str())?;
            Some(ConnectionManager::new(client).await?)
        } else {
            None
        };
        
        // Create disk cache if configured
        let disk_cache = if let Some(path) = &config.disk_path {
            std::fs::create_dir_all(path)?;
            Some(cacache::AsyncCache::new(path))
        } else {
            None
        };
        
        Ok(Self {
            memory_cache,
            redis_conn,
            disk_cache,
            config,
        })
    }
    
    pub async fn get(&self, key: &str) -> Option<Vec<u8>> {
        // L1: Check memory cache
        if let Some(value) = self.memory_cache.get(key).await {
            return Some(value);
        }
        
        // L2: Check Redis cache
        if let Some(conn) = &self.redis_conn {
            if let Ok(value) = conn.clone().get::<_, Vec<u8>>(key).await {
                // Populate memory cache
                self.memory_cache.insert(key.to_string(), value.clone()).await;
                return Some(value);
            }
        }
        
        // L3: Check disk cache
        if let Some(cache) = &self.disk_cache {
            if let Ok(data) = cache.read(key).await {
                let value = data.bytes;
                // Populate upper caches
                self.memory_cache.insert(key.to_string(), value.clone()).await;
                if let Some(mut conn) = self.redis_conn.clone() {
                    let _ = conn.set_ex::<_, _, ()>(
                        key,
                        value.as_slice(),
                        self.config.ttl_seconds,
                    ).await;
                }
                return Some(value);
            }
        }
        
        None
    }
    
    pub async fn set(&self, key: String, value: Vec<u8>) -> Result<()> {
        // Write to all cache layers
        
        // L1: Memory cache
        self.memory_cache.insert(key.clone(), value.clone()).await;
        
        // L2: Redis cache
        if let Some(mut conn) = self.redis_conn.clone() {
            conn.set_ex::<_, _, ()>(
                &key,
                value.as_slice(),
                self.config.ttl_seconds,
            ).await?;
        }
        
        // L3: Disk cache
        if let Some(cache) = &self.disk_cache {
            cache.write(&key, value).await?;
        }
        
        Ok(())
    }
    
    pub async fn delete(&self, key: &str) -> Result<()> {
        // Delete from all cache layers
        
        // L1: Memory cache
        self.memory_cache.remove(key).await;
        
        // L2: Redis cache
        if let Some(mut conn) = self.redis_conn.clone() {
            conn.del::<_, ()>(key).await?;
        }
        
        // L3: Disk cache
        if let Some(cache) = &self.disk_cache {
            cache.remove(key).await?;
        }
        
        Ok(())
    }
    
    pub async fn clear(&self) -> Result<()> {
        // Clear all caches
        
        // L1: Memory cache
        self.memory_cache.invalidate_all();
        
        // L2: Redis cache
        if let Some(mut conn) = self.redis_conn.clone() {
            redis::cmd("FLUSHDB").query_async::<_, ()>(&mut conn).await?;
        }
        
        // L3: Disk cache
        if let Some(cache) = &self.disk_cache {
            cache.clear().await?;
        }
        
        Ok(())
    }
    
    pub async fn stats(&self) -> CacheStats {
        let memory_stats = self.memory_cache.entry_count();
        
        let redis_size = if let Some(mut conn) = self.redis_conn.clone() {
            redis::cmd("DBSIZE")
                .query_async::<_, i64>(&mut conn)
                .await
                .unwrap_or(0) as u64
        } else {
            0
        };
        
        let disk_size = if let Some(cache) = &self.disk_cache {
            cache.list().await.map(|entries| entries.len() as u64).unwrap_or(0)
        } else {
            0
        };
        
        CacheStats {
            memory_entries: memory_stats,
            redis_entries: redis_size,
            disk_entries: disk_size,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct CacheStats {
    pub memory_entries: u64,
    pub redis_entries: u64,
    pub disk_entries: u64,
}