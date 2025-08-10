// Advanced Features Module - Ties together all advanced capabilities

pub mod websocket;
pub mod http3;
pub mod graphql;
pub mod wasm_plugins;
pub mod circuit_breaker;
pub mod connection_pool;
pub mod cache;

#[cfg(target_os = "linux")]
pub mod io_uring;

use anyhow::Result;
use std::sync::Arc;
use tracing::info;

use crate::config::Config;

/// Initialize all advanced features based on configuration
pub async fn init_advanced_features(config: &Config) -> Result<AdvancedFeatures> {
    info!("Initializing advanced features...");
    
    let mut features = AdvancedFeatures::default();
    
    // WebSocket support
    #[cfg(feature = "websocket")]
    {
        features.websocket_manager = Some(Arc::new(websocket::WebSocketManager::new()));
        info!("WebSocket support enabled");
    }
    
    // HTTP/3 & QUIC
    #[cfg(feature = "http3")]
    {
        if config.server.enable_https {
            match http3::start_http3_server(config).await {
                Ok(server) => {
                    features.http3_server = Some(server);
                    info!("HTTP/3 server started");
                }
                Err(e) => {
                    tracing::warn!("Failed to start HTTP/3: {}", e);
                }
            }
        }
    }
    
    // GraphQL
    #[cfg(feature = "graphql")]
    {
        features.graphql_schema = Some(graphql::create_schema().await?);
        info!("GraphQL schema initialized");
    }
    
    // WebAssembly plugins
    #[cfg(feature = "wasm-plugins")]
    {
        features.wasm_runtime = Some(wasm_plugins::WasmRuntime::new()?);
        info!("WebAssembly plugin runtime initialized");
    }
    
    // Circuit breaker for proxy
    features.circuit_breaker = Some(circuit_breaker::CircuitBreaker::new(
        circuit_breaker::Config {
            failure_threshold: 5,
            success_threshold: 2,
            timeout: std::time::Duration::from_secs(30),
            half_open_max_calls: 3,
        }
    ));
    
    // Connection pooling
    features.connection_pool = Some(connection_pool::ConnectionPool::new(
        100, // max connections
        std::time::Duration::from_secs(60), // idle timeout
    ).await?);
    
    // Multi-tier cache
    features.cache_manager = Some(cache::CacheManager::new(
        cache::CacheConfig {
            memory_capacity: 1000,
            redis_url: config.backends.get("redis")
                .map(|b| b.url.clone()),
            disk_path: Some("/var/cache/miwidothttp".to_string()),
            ttl_seconds: 3600,
        }
    ).await?);
    
    // io_uring for Linux
    #[cfg(all(target_os = "linux", feature = "io-uring"))]
    {
        features.io_uring_enabled = io_uring::is_available();
        if features.io_uring_enabled {
            info!("io_uring support enabled");
        }
    }
    
    info!("Advanced features initialized successfully");
    Ok(features)
}

#[derive(Default)]
pub struct AdvancedFeatures {
    #[cfg(feature = "websocket")]
    pub websocket_manager: Option<Arc<websocket::WebSocketManager>>,
    
    #[cfg(feature = "http3")]
    pub http3_server: Option<http3::Http3Server>,
    
    #[cfg(feature = "graphql")]
    pub graphql_schema: Option<async_graphql::Schema<
        graphql::QueryRoot,
        graphql::MutationRoot,
        graphql::SubscriptionRoot,
    >>,
    
    #[cfg(feature = "wasm-plugins")]
    pub wasm_runtime: Option<wasm_plugins::WasmRuntime>,
    
    pub circuit_breaker: Option<circuit_breaker::CircuitBreaker>,
    pub connection_pool: Option<connection_pool::ConnectionPool>,
    pub cache_manager: Option<cache::CacheManager>,
    
    #[cfg(all(target_os = "linux", feature = "io-uring"))]
    pub io_uring_enabled: bool,
}

// Re-export commonly used types
pub use websocket::WebSocketManager;
pub use http3::Http3Server;
pub use circuit_breaker::CircuitBreaker;
pub use connection_pool::ConnectionPool;
pub use cache::CacheManager;