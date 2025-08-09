use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};
use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc, Duration};
use uuid::Uuid;
use redis::AsyncCommands;
use std::path::PathBuf;
use tokio::fs;
use tracing::{info, warn, error};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SessionConfig {
    pub backend: SessionBackend,
    pub ttl_seconds: i64,
    pub cookie_name: String,
    pub cookie_secure: bool,
    pub cookie_http_only: bool,
    pub cookie_same_site: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionBackend {
    Memory,
    Redis { url: String },
    File { path: String },
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            backend: SessionBackend::Memory,
            ttl_seconds: 3600, // 1 hour
            cookie_name: "session_id".to_string(),
            cookie_secure: false,
            cookie_http_only: true,
            cookie_same_site: "lax".to_string(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub data: HashMap<String, serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

impl Session {
    pub fn new(ttl_seconds: i64) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            data: HashMap::new(),
            created_at: now,
            updated_at: now,
            expires_at: now + Duration::seconds(ttl_seconds),
        }
    }

    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }

    pub fn set(&mut self, key: String, value: serde_json::Value) {
        self.data.insert(key, value);
        self.updated_at = Utc::now();
    }

    pub fn get(&self, key: &str) -> Option<&serde_json::Value> {
        self.data.get(key)
    }

    pub fn remove(&mut self, key: &str) -> Option<serde_json::Value> {
        self.updated_at = Utc::now();
        self.data.remove(key)
    }
}

#[async_trait::async_trait]
pub trait SessionStore: Send + Sync {
    async fn get(&self, session_id: &str) -> Result<Option<Session>>;
    async fn set(&self, session: &Session) -> Result<()>;
    async fn delete(&self, session_id: &str) -> Result<()>;
    async fn cleanup_expired(&self) -> Result<usize>;
}

// Memory-based session store
pub struct MemorySessionStore {
    sessions: Arc<RwLock<HashMap<String, Session>>>,
}

impl MemorySessionStore {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait::async_trait]
impl SessionStore for MemorySessionStore {
    async fn get(&self, session_id: &str) -> Result<Option<Session>> {
        let sessions = self.sessions.read().await;
        if let Some(session) = sessions.get(session_id) {
            if !session.is_expired() {
                return Ok(Some(session.clone()));
            }
        }
        Ok(None)
    }

    async fn set(&self, session: &Session) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        sessions.insert(session.id.clone(), session.clone());
        Ok(())
    }

    async fn delete(&self, session_id: &str) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        sessions.remove(session_id);
        Ok(())
    }

    async fn cleanup_expired(&self) -> Result<usize> {
        let mut sessions = self.sessions.write().await;
        let before = sessions.len();
        sessions.retain(|_, session| !session.is_expired());
        let removed = before - sessions.len();
        if removed > 0 {
            info!("Cleaned up {} expired sessions", removed);
        }
        Ok(removed)
    }
}

// Redis-based session store
pub struct RedisSessionStore {
    client: redis::Client,
    ttl_seconds: i64,
}

impl RedisSessionStore {
    pub fn new(url: &str, ttl_seconds: i64) -> Result<Self> {
        let client = redis::Client::open(url)
            .map_err(|e| anyhow!("Failed to connect to Redis: {}", e))?;
        Ok(Self { client, ttl_seconds })
    }
}

#[async_trait::async_trait]
impl SessionStore for RedisSessionStore {
    async fn get(&self, session_id: &str) -> Result<Option<Session>> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let key = format!("session:{}", session_id);
        
        let data: Option<String> = conn.get(&key).await?;
        if let Some(json) = data {
            let session: Session = serde_json::from_str(&json)?;
            if !session.is_expired() {
                return Ok(Some(session));
            } else {
                // Clean up expired session
                let _: () = conn.del(&key).await?;
            }
        }
        Ok(None)
    }

    async fn set(&self, session: &Session) -> Result<()> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let key = format!("session:{}", session.id);
        let json = serde_json::to_string(session)?;
        
        let _: () = conn.set_ex(&key, json, self.ttl_seconds as u64).await?;
        Ok(())
    }

    async fn delete(&self, session_id: &str) -> Result<()> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let key = format!("session:{}", session_id);
        let _: () = conn.del(&key).await?;
        Ok(())
    }

    async fn cleanup_expired(&self) -> Result<usize> {
        // Redis handles expiration automatically with TTL
        Ok(0)
    }
}

// File-based session store
pub struct FileSessionStore {
    path: PathBuf,
}

impl FileSessionStore {
    pub fn new(path: &str) -> Result<Self> {
        let path = PathBuf::from(path);
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }
}

#[async_trait::async_trait]
impl SessionStore for FileSessionStore {
    async fn get(&self, session_id: &str) -> Result<Option<Session>> {
        let file_path = self.path.join(format!("{}.json", session_id));
        
        if file_path.exists() {
            let data = fs::read_to_string(&file_path).await?;
            let session: Session = serde_json::from_str(&data)?;
            
            if !session.is_expired() {
                return Ok(Some(session));
            } else {
                // Clean up expired session
                fs::remove_file(&file_path).await.ok();
            }
        }
        Ok(None)
    }

    async fn set(&self, session: &Session) -> Result<()> {
        let file_path = self.path.join(format!("{}.json", session.id));
        let json = serde_json::to_string_pretty(session)?;
        fs::write(&file_path, json).await?;
        Ok(())
    }

    async fn delete(&self, session_id: &str) -> Result<()> {
        let file_path = self.path.join(format!("{}.json", session_id));
        if file_path.exists() {
            fs::remove_file(&file_path).await?;
        }
        Ok(())
    }

    async fn cleanup_expired(&self) -> Result<usize> {
        let mut removed = 0;
        let mut entries = fs::read_dir(&self.path).await?;
        
        while let Some(entry) = entries.next_entry().await? {
            if let Some(ext) = entry.path().extension() {
                if ext == "json" {
                    if let Ok(data) = fs::read_to_string(entry.path()).await {
                        if let Ok(session) = serde_json::from_str::<Session>(&data) {
                            if session.is_expired() {
                                fs::remove_file(entry.path()).await.ok();
                                removed += 1;
                            }
                        }
                    }
                }
            }
        }
        
        if removed > 0 {
            info!("Cleaned up {} expired session files", removed);
        }
        Ok(removed)
    }
}

pub struct SessionManager {
    store: Arc<Box<dyn SessionStore>>,
    config: SessionConfig,
}

impl SessionManager {
    pub fn new(config: SessionConfig) -> Result<Self> {
        let store: Box<dyn SessionStore> = match &config.backend {
            SessionBackend::Memory => {
                Box::new(MemorySessionStore::new())
            }
            SessionBackend::Redis { url } => {
                Box::new(RedisSessionStore::new(url, config.ttl_seconds)?)
            }
            SessionBackend::File { path } => {
                Box::new(FileSessionStore::new(path)?)
            }
        };

        Ok(Self {
            store: Arc::new(store),
            config,
        })
    }

    pub async fn create_session(&self) -> Result<Session> {
        let session = Session::new(self.config.ttl_seconds);
        self.store.set(&session).await?;
        Ok(session)
    }

    pub async fn get_session(&self, session_id: &str) -> Result<Option<Session>> {
        self.store.get(session_id).await
    }

    pub async fn update_session(&self, session: &Session) -> Result<()> {
        self.store.set(session).await
    }

    pub async fn delete_session(&self, session_id: &str) -> Result<()> {
        self.store.delete(session_id).await
    }

    pub async fn cleanup_expired(&self) -> Result<usize> {
        self.store.cleanup_expired().await
    }

    pub fn generate_cookie_header(&self, session_id: &str) -> String {
        let mut cookie = format!("{}={}", self.config.cookie_name, session_id);
        
        if self.config.cookie_http_only {
            cookie.push_str("; HttpOnly");
        }
        
        if self.config.cookie_secure {
            cookie.push_str("; Secure");
        }
        
        cookie.push_str(&format!("; SameSite={}", self.config.cookie_same_site));
        cookie.push_str(&format!("; Max-Age={}", self.config.ttl_seconds));
        cookie.push_str("; Path=/");
        
        cookie
    }

    pub async fn start_cleanup_task(&self) {
        let store = self.store.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(300)).await; // Every 5 minutes
                if let Err(e) = store.cleanup_expired().await {
                    error!("Failed to cleanup expired sessions: {}", e);
                }
            }
        });
    }
}

impl Clone for SessionManager {
    fn clone(&self) -> Self {
        Self {
            store: self.store.clone(),
            config: self.config.clone(),
        }
    }
}