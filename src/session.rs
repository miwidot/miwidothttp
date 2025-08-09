use anyhow::{anyhow, Result};
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use uuid::Uuid;

// Redis support
use redis::{AsyncCommands, Client as RedisClient};

// Cookie handling
use axum::http::header::{COOKIE, SET_COOKIE};
use axum::http::{HeaderMap, HeaderValue};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub data: HashMap<String, serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub last_accessed: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub user_id: Option<String>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub csrf_token: Option<String>,
}

impl Session {
    pub fn new(ttl: Duration) -> Self {
        let now = Utc::now();
        let session_id = Self::generate_session_id();
        let csrf_token = Self::generate_csrf_token();
        
        Session {
            id: session_id,
            data: HashMap::new(),
            created_at: now,
            last_accessed: now,
            expires_at: now + ttl,
            user_id: None,
            ip_address: None,
            user_agent: None,
            csrf_token: Some(csrf_token),
        }
    }

    fn generate_session_id() -> String {
        // Generate cryptographically secure session ID
        let uuid = Uuid::new_v4();
        let random_bytes: [u8; 16] = rand::thread_rng().gen();
        
        let mut hasher = Sha256::new();
        hasher.update(uuid.as_bytes());
        hasher.update(&random_bytes);
        hasher.update(Utc::now().timestamp_nanos_opt().unwrap_or(0).to_le_bytes());
        
        format!("{:x}", hasher.finalize())
    }

    fn generate_csrf_token() -> String {
        // Generate CSRF token
        let random_bytes: [u8; 32] = rand::thread_rng().gen();
        base64::encode(random_bytes)
    }

    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }

    pub fn refresh(&mut self, ttl: Duration) {
        self.last_accessed = Utc::now();
        self.expires_at = self.last_accessed + ttl;
    }

    pub fn get<T: for<'de> Deserialize<'de>>(&self, key: &str) -> Option<T> {
        self.data.get(key)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    pub fn set<T: Serialize>(&mut self, key: String, value: T) -> Result<()> {
        let json_value = serde_json::to_value(value)?;
        self.data.insert(key, json_value);
        Ok(())
    }

    pub fn remove(&mut self, key: &str) -> Option<serde_json::Value> {
        self.data.remove(key)
    }

    pub fn clear(&mut self) {
        self.data.clear();
    }

    pub fn regenerate_id(&mut self) {
        self.id = Self::generate_session_id();
        self.csrf_token = Some(Self::generate_csrf_token());
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionConfig {
    pub cookie_name: String,
    pub cookie_domain: Option<String>,
    pub cookie_path: String,
    pub cookie_secure: bool,
    pub cookie_http_only: bool,
    pub cookie_same_site: SameSite,
    pub ttl_seconds: u64,
    pub cleanup_interval_seconds: u64,
    pub max_sessions_per_user: Option<usize>,
    pub regenerate_id_on_login: bool,
    pub check_ip: bool,
    pub check_user_agent: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SameSite {
    Strict,
    Lax,
    None,
}

impl Default for SessionConfig {
    fn default() -> Self {
        SessionConfig {
            cookie_name: "session_id".to_string(),
            cookie_domain: None,
            cookie_path: "/".to_string(),
            cookie_secure: true,
            cookie_http_only: true,
            cookie_same_site: SameSite::Lax,
            ttl_seconds: 3600, // 1 hour
            cleanup_interval_seconds: 300, // 5 minutes
            max_sessions_per_user: Some(5),
            regenerate_id_on_login: true,
            check_ip: false,
            check_user_agent: true,
        }
    }
}

#[async_trait]
pub trait SessionStore: Send + Sync {
    async fn load(&self, session_id: &str) -> Result<Option<Session>>;
    async fn save(&self, session: &Session) -> Result<()>;
    async fn delete(&self, session_id: &str) -> Result<()>;
    async fn cleanup(&self) -> Result<usize>;
    async fn exists(&self, session_id: &str) -> Result<bool>;
    async fn user_sessions(&self, user_id: &str) -> Result<Vec<Session>>;
    async fn delete_user_sessions(&self, user_id: &str) -> Result<()>;
}

// In-memory session store
pub struct MemoryStore {
    sessions: Arc<RwLock<HashMap<String, Session>>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        MemoryStore {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl SessionStore for MemoryStore {
    async fn load(&self, session_id: &str) -> Result<Option<Session>> {
        let sessions = self.sessions.read().await;
        Ok(sessions.get(session_id).cloned())
    }

    async fn save(&self, session: &Session) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        sessions.insert(session.id.clone(), session.clone());
        Ok(())
    }

    async fn delete(&self, session_id: &str) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        sessions.remove(session_id);
        Ok(())
    }

    async fn cleanup(&self) -> Result<usize> {
        let mut sessions = self.sessions.write().await;
        let now = Utc::now();
        let before_count = sessions.len();
        
        sessions.retain(|_, session| session.expires_at > now);
        
        let removed = before_count - sessions.len();
        if removed > 0 {
            debug!("Cleaned up {} expired sessions", removed);
        }
        Ok(removed)
    }

    async fn exists(&self, session_id: &str) -> Result<bool> {
        let sessions = self.sessions.read().await;
        Ok(sessions.contains_key(session_id))
    }

    async fn user_sessions(&self, user_id: &str) -> Result<Vec<Session>> {
        let sessions = self.sessions.read().await;
        Ok(sessions.values()
            .filter(|s| s.user_id.as_deref() == Some(user_id))
            .cloned()
            .collect())
    }

    async fn delete_user_sessions(&self, user_id: &str) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        sessions.retain(|_, session| session.user_id.as_deref() != Some(user_id));
        Ok(())
    }
}

// Redis session store
pub struct RedisStore {
    client: RedisClient,
    prefix: String,
}

impl RedisStore {
    pub async fn new(redis_url: &str, prefix: &str) -> Result<Self> {
        let client = RedisClient::open(redis_url)?;
        Ok(RedisStore {
            client,
            prefix: prefix.to_string(),
        })
    }

    fn key(&self, session_id: &str) -> String {
        format!("{}:{}", self.prefix, session_id)
    }

    fn user_key(&self, user_id: &str) -> String {
        format!("{}:user:{}", self.prefix, user_id)
    }
}

#[async_trait]
impl SessionStore for RedisStore {
    async fn load(&self, session_id: &str) -> Result<Option<Session>> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let key = self.key(session_id);
        
        let data: Option<String> = conn.get(&key).await?;
        match data {
            Some(json) => {
                let session: Session = serde_json::from_str(&json)?;
                Ok(Some(session))
            }
            None => Ok(None),
        }
    }

    async fn save(&self, session: &Session) -> Result<()> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let key = self.key(&session.id);
        let json = serde_json::to_string(session)?;
        
        let ttl = (session.expires_at - Utc::now()).num_seconds().max(0) as u64;
        conn.set_ex(&key, json, ttl).await?;
        
        // Track user sessions
        if let Some(user_id) = &session.user_id {
            let user_key = self.user_key(user_id);
            conn.sadd(&user_key, &session.id).await?;
            conn.expire(&user_key, ttl as i64).await?;
        }
        
        Ok(())
    }

    async fn delete(&self, session_id: &str) -> Result<()> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let key = self.key(session_id);
        
        // Get session to find user_id
        if let Some(session) = self.load(session_id).await? {
            if let Some(user_id) = session.user_id {
                let user_key = self.user_key(&user_id);
                conn.srem(&user_key, session_id).await?;
            }
        }
        
        conn.del(&key).await?;
        Ok(())
    }

    async fn cleanup(&self) -> Result<usize> {
        // Redis handles expiration automatically
        Ok(0)
    }

    async fn exists(&self, session_id: &str) -> Result<bool> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let key = self.key(session_id);
        Ok(conn.exists(&key).await?)
    }

    async fn user_sessions(&self, user_id: &str) -> Result<Vec<Session>> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let user_key = self.user_key(user_id);
        
        let session_ids: Vec<String> = conn.smembers(&user_key).await?;
        let mut sessions = Vec::new();
        
        for session_id in session_ids {
            if let Some(session) = self.load(&session_id).await? {
                sessions.push(session);
            }
        }
        
        Ok(sessions)
    }

    async fn delete_user_sessions(&self, user_id: &str) -> Result<()> {
        let sessions = self.user_sessions(user_id).await?;
        for session in sessions {
            self.delete(&session.id).await?;
        }
        Ok(())
    }
}

// File-based session store
pub struct FileStore {
    base_path: std::path::PathBuf,
}

impl FileStore {
    pub fn new(base_path: impl Into<std::path::PathBuf>) -> Result<Self> {
        let base_path = base_path.into();
        std::fs::create_dir_all(&base_path)?;
        Ok(FileStore { base_path })
    }

    fn session_path(&self, session_id: &str) -> std::path::PathBuf {
        // Use first 2 chars as subdirectory for better file system performance
        let subdir = if session_id.len() >= 2 {
            &session_id[..2]
        } else {
            "00"
        };
        
        self.base_path.join(subdir).join(format!("{}.json", session_id))
    }
}

#[async_trait]
impl SessionStore for FileStore {
    async fn load(&self, session_id: &str) -> Result<Option<Session>> {
        let path = self.session_path(session_id);
        
        if !path.exists() {
            return Ok(None);
        }
        
        let data = tokio::fs::read_to_string(&path).await?;
        let session: Session = serde_json::from_str(&data)?;
        
        if session.is_expired() {
            tokio::fs::remove_file(&path).await?;
            return Ok(None);
        }
        
        Ok(Some(session))
    }

    async fn save(&self, session: &Session) -> Result<()> {
        let path = self.session_path(&session.id);
        
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        
        let json = serde_json::to_string_pretty(session)?;
        tokio::fs::write(&path, json).await?;
        
        Ok(())
    }

    async fn delete(&self, session_id: &str) -> Result<()> {
        let path = self.session_path(session_id);
        if path.exists() {
            tokio::fs::remove_file(&path).await?;
        }
        Ok(())
    }

    async fn cleanup(&self) -> Result<usize> {
        let mut removed = 0;
        let now = Utc::now();
        
        for entry in std::fs::read_dir(&self.base_path)? {
            let entry = entry?;
            if entry.path().is_dir() {
                for session_file in std::fs::read_dir(entry.path())? {
                    let session_file = session_file?;
                    let path = session_file.path();
                    
                    if path.extension().and_then(|s| s.to_str()) == Some("json") {
                        if let Ok(data) = tokio::fs::read_to_string(&path).await {
                            if let Ok(session) = serde_json::from_str::<Session>(&data) {
                                if session.expires_at < now {
                                    tokio::fs::remove_file(&path).await?;
                                    removed += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
        
        if removed > 0 {
            debug!("Cleaned up {} expired session files", removed);
        }
        Ok(removed)
    }

    async fn exists(&self, session_id: &str) -> Result<bool> {
        Ok(self.session_path(session_id).exists())
    }

    async fn user_sessions(&self, user_id: &str) -> Result<Vec<Session>> {
        let mut sessions = Vec::new();
        
        for entry in std::fs::read_dir(&self.base_path)? {
            let entry = entry?;
            if entry.path().is_dir() {
                for session_file in std::fs::read_dir(entry.path())? {
                    let session_file = session_file?;
                    let path = session_file.path();
                    
                    if path.extension().and_then(|s| s.to_str()) == Some("json") {
                        if let Ok(data) = tokio::fs::read_to_string(&path).await {
                            if let Ok(session) = serde_json::from_str::<Session>(&data) {
                                if session.user_id.as_deref() == Some(user_id) {
                                    sessions.push(session);
                                }
                            }
                        }
                    }
                }
            }
        }
        
        Ok(sessions)
    }

    async fn delete_user_sessions(&self, user_id: &str) -> Result<()> {
        let sessions = self.user_sessions(user_id).await?;
        for session in sessions {
            self.delete(&session.id).await?;
        }
        Ok(())
    }
}

pub struct SessionManager {
    store: Arc<dyn SessionStore>,
    config: SessionConfig,
}

impl SessionManager {
    pub fn new(store: Arc<dyn SessionStore>, config: SessionConfig) -> Self {
        let manager = SessionManager { store, config };
        
        // Start cleanup task
        manager.start_cleanup_task();
        
        manager
    }

    fn start_cleanup_task(&self) {
        let store = self.store.clone();
        let interval = self.config.cleanup_interval_seconds;
        
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(
                tokio::time::Duration::from_secs(interval)
            );
            
            loop {
                interval.tick().await;
                if let Err(e) = store.cleanup().await {
                    warn!("Session cleanup error: {}", e);
                }
            }
        });
    }

    pub async fn create_session(&self, headers: &HeaderMap) -> Result<Session> {
        let ttl = Duration::seconds(self.config.ttl_seconds as i64);
        let mut session = Session::new(ttl);
        
        // Extract client info
        if let Some(ip) = headers.get("x-real-ip")
            .or_else(|| headers.get("x-forwarded-for")) {
            session.ip_address = ip.to_str().ok().map(|s| s.to_string());
        }
        
        if let Some(ua) = headers.get("user-agent") {
            session.user_agent = ua.to_str().ok().map(|s| s.to_string());
        }
        
        self.store.save(&session).await?;
        Ok(session)
    }

    pub async fn load_session(&self, session_id: &str, headers: &HeaderMap) -> Result<Option<Session>> {
        let mut session = match self.store.load(session_id).await? {
            Some(s) => s,
            None => return Ok(None),
        };
        
        // Validate session
        if session.is_expired() {
            self.store.delete(session_id).await?;
            return Ok(None);
        }
        
        // Check IP if configured
        if self.config.check_ip {
            if let Some(current_ip) = headers.get("x-real-ip")
                .or_else(|| headers.get("x-forwarded-for")) {
                let current_ip = current_ip.to_str().ok().map(|s| s.to_string());
                if session.ip_address != current_ip {
                    warn!("Session IP mismatch for {}", session_id);
                    return Ok(None);
                }
            }
        }
        
        // Check User-Agent if configured
        if self.config.check_user_agent {
            if let Some(current_ua) = headers.get("user-agent") {
                let current_ua = current_ua.to_str().ok().map(|s| s.to_string());
                if session.user_agent != current_ua {
                    warn!("Session User-Agent mismatch for {}", session_id);
                    return Ok(None);
                }
            }
        }
        
        // Refresh session
        let ttl = Duration::seconds(self.config.ttl_seconds as i64);
        session.refresh(ttl);
        self.store.save(&session).await?;
        
        Ok(Some(session))
    }

    pub async fn destroy_session(&self, session_id: &str) -> Result<()> {
        self.store.delete(session_id).await
    }

    pub async fn login(&self, session: &mut Session, user_id: String) -> Result<()> {
        // Check max sessions per user
        if let Some(max) = self.config.max_sessions_per_user {
            let user_sessions = self.store.user_sessions(&user_id).await?;
            if user_sessions.len() >= max {
                // Remove oldest session
                if let Some(oldest) = user_sessions.iter()
                    .min_by_key(|s| s.created_at) {
                    self.store.delete(&oldest.id).await?;
                }
            }
        }
        
        // Regenerate session ID if configured
        if self.config.regenerate_id_on_login {
            let old_id = session.id.clone();
            session.regenerate_id();
            self.store.delete(&old_id).await?;
        }
        
        session.user_id = Some(user_id);
        self.store.save(session).await?;
        
        Ok(())
    }

    pub async fn logout(&self, session: &mut Session) -> Result<()> {
        session.user_id = None;
        session.clear();
        self.store.save(session).await?;
        Ok(())
    }

    pub fn create_cookie(&self, session_id: &str) -> String {
        let same_site = match self.config.cookie_same_site {
            SameSite::Strict => "Strict",
            SameSite::Lax => "Lax",
            SameSite::None => "None",
        };
        
        let mut cookie = format!(
            "{}={}; Path={}; SameSite={}",
            self.config.cookie_name,
            session_id,
            self.config.cookie_path,
            same_site
        );
        
        if let Some(domain) = &self.config.cookie_domain {
            cookie.push_str(&format!("; Domain={}", domain));
        }
        
        if self.config.cookie_secure {
            cookie.push_str("; Secure");
        }
        
        if self.config.cookie_http_only {
            cookie.push_str("; HttpOnly");
        }
        
        cookie.push_str(&format!("; Max-Age={}", self.config.ttl_seconds));
        
        cookie
    }

    pub fn extract_session_id(&self, headers: &HeaderMap) -> Option<String> {
        headers.get(COOKIE)
            .and_then(|v| v.to_str().ok())
            .and_then(|cookies| {
                for cookie in cookies.split(';') {
                    let parts: Vec<&str> = cookie.trim().splitn(2, '=').collect();
                    if parts.len() == 2 && parts[0] == self.config.cookie_name {
                        return Some(parts[1].to_string());
                    }
                }
                None
            })
    }
}

// Helper function for CSRF validation
pub fn validate_csrf_token(session: &Session, provided_token: &str) -> bool {
    session.csrf_token.as_deref() == Some(provided_token)
}

// Helper function to extract CSRF token from headers
pub fn extract_csrf_token(headers: &HeaderMap) -> Option<String> {
    headers.get("x-csrf-token")
        .or_else(|| headers.get("x-xsrf-token"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

use base64;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_memory_store() {
        let store = Arc::new(MemoryStore::new());
        let ttl = Duration::seconds(3600);
        
        let mut session = Session::new(ttl);
        session.set("user".to_string(), "john").unwrap();
        
        store.save(&session).await.unwrap();
        
        let loaded = store.load(&session.id).await.unwrap().unwrap();
        assert_eq!(loaded.id, session.id);
        
        let user: String = loaded.get("user").unwrap();
        assert_eq!(user, "john");
        
        store.delete(&session.id).await.unwrap();
        assert!(store.load(&session.id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_session_expiration() {
        let ttl = Duration::seconds(-1); // Already expired
        let session = Session::new(ttl);
        assert!(session.is_expired());
    }

    #[tokio::test]
    async fn test_csrf_token_generation() {
        let ttl = Duration::seconds(3600);
        let session = Session::new(ttl);
        assert!(session.csrf_token.is_some());
        assert!(!session.csrf_token.unwrap().is_empty());
    }
}