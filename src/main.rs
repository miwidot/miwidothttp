use axum::{
    body::Body,
    extract::{Host, Request, State},
    http::{StatusCode, Uri, HeaderValue, Method},
    response::{Html, IntoResponse, Response},
    routing::{get, post, any},
    Router,
};
use axum_server::tls_rustls::RustlsConfig;
use std::{net::SocketAddr, sync::Arc, path::PathBuf, time::Duration};
use tower::ServiceBuilder;
use tower_http::{
    compression::CompressionLayer,
    cors::CorsLayer,
    services::ServeDir,
    trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer},
};
use tracing::{info, warn, error, Level};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use serde::{Deserialize, Serialize};
use tokio::fs;
use std::collections::HashMap;

mod process_manager;
mod security;
mod session_manager;
mod rewrite_engine;
mod metrics;

use process_manager::{ProcessManager, ProcessConfig, AppType};
use security::{SecurityConfig, RateLimiter, security_headers_middleware};
use session_manager::{SessionManager, SessionConfig};
use rewrite_engine::{RewriteEngine, RewriteConfig, RewriteResult};
use metrics::{MetricsCollector, RequestMetrics};

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Config {
    #[serde(default)]
    server: ServerConfig,
    #[serde(default)]
    ssl: SslConfig,
    #[serde(skip)]
    security: SecurityConfig,
    #[serde(default)]
    backends: HashMap<String, BackendConfig>,
    #[serde(skip)]
    processes: HashMap<String, ProcessConfig>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ServerConfig {
    #[serde(default = "default_http_port")]
    http_port: u16,
    #[serde(default = "default_https_port")]
    https_port: u16,
    #[serde(default = "default_bind_address")]
    bind_address: String,
    #[serde(default = "default_static_dir")]
    static_dir: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SslConfig {
    #[serde(default)]
    enabled: bool,
    cert_path: Option<String>,
    key_path: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct BackendConfig {
    #[serde(flatten)]
    process: Option<ProcessConfig>,
    target: Option<String>,
    #[serde(default)]
    health_check: Option<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            http_port: default_http_port(),
            https_port: default_https_port(),
            bind_address: default_bind_address(),
            static_dir: default_static_dir(),
        }
    }
}

impl Default for SslConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            cert_path: None,
            key_path: None,
        }
    }
}

fn default_http_port() -> u16 { 8080 }
fn default_https_port() -> u16 { 8443 }
fn default_bind_address() -> String { "0.0.0.0".to_string() }
fn default_static_dir() -> String { "./static".to_string() }

#[derive(Clone)]
struct AppState {
    config: Arc<Config>,
    static_dir: PathBuf,
    http_client: reqwest::Client,
    process_manager: Arc<ProcessManager>,
    rate_limiter: Arc<RateLimiter>,
    session_manager: Option<Arc<SessionManager>>,
    metrics: Arc<MetricsCollector>,
}

#[tokio::main]
async fn main() {
    // Install default crypto provider for rustls
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");
    
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "miwidothttp=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Load configuration
    let config = load_config().await;
    
    // Create static directory
    let static_dir = PathBuf::from(&config.server.static_dir);
    std::fs::create_dir_all(&static_dir).unwrap();
    
    // Create a test file if it doesn't exist
    let index_path = static_dir.join("index.html");
    if !index_path.exists() {
        std::fs::write(&index_path, 
            r#"<!DOCTYPE html>
<html>
<head>
    <title>miwidothttp</title>
    <style>
        body { font-family: system-ui; max-width: 800px; margin: 50px auto; padding: 20px; }
        h1 { color: #333; }
        .status { background: #f0f0f0; padding: 15px; border-radius: 5px; }
        .feature { margin: 10px 0; }
        .enabled { color: green; }
    </style>
</head>
<body>
    <h1>🚀 miwidothttp Server</h1>
    <div class="status">
        <h2>Server Status</h2>
        <div class="feature enabled">✅ HTTP/1.1 Support</div>
        <div class="feature enabled">✅ HTTP/2 Support</div>
        <div class="feature enabled">✅ Static File Serving</div>
        <div class="feature enabled">✅ Compression (gzip/brotli)</div>
        <div class="feature enabled">✅ CORS Support</div>
        <div class="feature enabled">✅ Request Logging</div>
        <div class="feature enabled">✅ Proxy Support</div>
        <div class="feature enabled">✅ Configuration File Support</div>
    </div>
    <p>Configuration: <code>/etc/miwidothttp/config.toml</code> or <code>./config.toml</code></p>
    <p>API Status: <a href="/api/status">/api/status</a></p>
    <p>Metrics: <a href="/metrics">/metrics</a></p>
    <p>Health: <a href="/health">/health</a></p>
</body>
</html>"#).unwrap();
    }

    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("Failed to create HTTP client");

    // Initialize process manager
    let process_manager = Arc::new(ProcessManager::new());
    
    // Start configured processes
    for (name, proc_config) in &config.processes {
        info!("Starting process: {}", name);
        if let Err(e) = process_manager.start_process(name.clone(), proc_config.clone()).await {
            error!("Failed to start process {}: {}", name, e);
        }
    }
    
    // Start process monitoring
    process_manager.monitor_processes().await;
    
    // Initialize rate limiter
    let rate_limiter = Arc::new(RateLimiter::new(config.security.clone()));
    
    // Initialize session manager (optional)
    let session_manager = None; // TODO: Add session config to Config struct
    
    // Initialize metrics collector
    let metrics = Arc::new(MetricsCollector::new());
    
    let app_state = Arc::new(AppState {
        config: Arc::new(config.clone()),
        static_dir: static_dir.clone(),
        http_client,
        process_manager,
        rate_limiter,
        session_manager,
        metrics,
    });

    // Build our application with routes
    let app = create_app(app_state.clone());

    // Start HTTP server
    let http_addr = SocketAddr::new(
        config.server.bind_address.parse().unwrap(),
        config.server.http_port
    );
    
    info!("🚀 miwidothttp server starting");
    info!("📁 Serving static files from {}", config.server.static_dir);
    info!("🌐 HTTP server on http://{}", http_addr);
    
    let http_server = tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind(http_addr)
            .await
            .expect("Failed to bind HTTP address");
        
        axum::serve(listener, app)
            .await
            .expect("HTTP server failed");
    });

    // Start HTTPS server if SSL is enabled
    if config.ssl.enabled {
        if let (Some(cert_path), Some(key_path)) = (&config.ssl.cert_path, &config.ssl.key_path) {
            let https_addr = SocketAddr::new(
                config.server.bind_address.parse().unwrap(),
                config.server.https_port
            );
            
            // Check if certificate files exist, if not create self-signed
            if !PathBuf::from(cert_path).exists() || !PathBuf::from(key_path).exists() {
                info!("Certificate files not found, generating self-signed certificate...");
                generate_self_signed_cert(cert_path, key_path).await;
            }
            
            match RustlsConfig::from_pem_file(cert_path, key_path).await {
                Ok(tls_config) => {
                    info!("🔒 HTTPS server on https://{}", https_addr);
                    
                    let app = create_app(app_state);
                    let https_server = tokio::spawn(async move {
                        axum_server::bind_rustls(https_addr, tls_config)
                            .serve(app.into_make_service())
                            .await
                            .expect("HTTPS server failed");
                    });
                    
                    // Wait for both servers
                    tokio::select! {
                        _ = http_server => {},
                        _ = https_server => {},
                    }
                }
                Err(e) => {
                    error!("Failed to load TLS configuration: {}", e);
                    warn!("HTTPS server disabled, running HTTP only");
                    http_server.await.unwrap();
                }
            }
        } else {
            warn!("SSL enabled but cert_path or key_path not configured");
            http_server.await.unwrap();
        }
    } else {
        http_server.await.unwrap();
    }
}

fn create_app(state: Arc<AppState>) -> Router {
    Router::new()
        // Health check endpoint
        .route("/health", get(|| async { "OK" }))
        // API endpoints
        .route("/api/status", get(api_status))
        .route("/api/backends", get(list_backends))
        .route("/api/processes", get(list_processes))
        .route("/api/processes/:name/restart", post(restart_process))
        // Metrics endpoint
        .route("/metrics", get(metrics))
        // Static files
        .nest_service("/static", ServeDir::new(&state.static_dir))
        .fallback_service(ServeDir::new(&state.static_dir))
        // Proxy handler for configured backends
        .route("/*path", any(proxy_handler))
        .layer(
            ServiceBuilder::new()
                // Standard middlewares
                .layer(TraceLayer::new_for_http()
                    .make_span_with(DefaultMakeSpan::new()
                        .level(Level::INFO))
                    .on_response(DefaultOnResponse::new()
                        .level(Level::INFO)))
                .layer(CompressionLayer::new())
                .layer(CorsLayer::permissive())
        )
        // Security middlewares (added separately for now)
        .layer(axum::middleware::from_fn(security_headers_middleware))
        .with_state(state)
}

async fn load_config() -> Config {
    // Try to load from various locations
    let paths = vec![
        "/etc/miwidothttp/config.toml",
        "./config.toml",
        "config.toml",
    ];
    
    for path in paths {
        if PathBuf::from(path).exists() {
            match fs::read_to_string(path).await {
                Ok(content) => {
                    match toml::from_str(&content) {
                        Ok(config) => {
                            info!("Loaded configuration from {}", path);
                            return config;
                        }
                        Err(e) => {
                            error!("Failed to parse config file {}: {}", path, e);
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to read config file {}: {}", path, e);
                }
            }
        }
    }
    
    info!("Using default configuration");
    Config {
        server: ServerConfig::default(),
        ssl: SslConfig::default(),
        security: SecurityConfig::default(),
        backends: HashMap::new(),
        processes: HashMap::new(),
    }
}

async fn generate_self_signed_cert(cert_path: &str, key_path: &str) {
    use std::process::Command;
    
    // Create directories if they don't exist
    if let Some(parent) = PathBuf::from(cert_path).parent() {
        std::fs::create_dir_all(parent).ok();
    }
    if let Some(parent) = PathBuf::from(key_path).parent() {
        std::fs::create_dir_all(parent).ok();
    }
    
    // Generate self-signed certificate using openssl
    let output = Command::new("openssl")
        .args(&[
            "req", "-x509", "-newkey", "rsa:4096",
            "-keyout", key_path,
            "-out", cert_path,
            "-days", "365",
            "-nodes",
            "-subj", "/C=US/ST=State/L=City/O=Organization/CN=localhost"
        ])
        .output();
    
    match output {
        Ok(output) => {
            if output.status.success() {
                info!("Generated self-signed certificate successfully");
            } else {
                error!("Failed to generate certificate: {}", String::from_utf8_lossy(&output.stderr));
            }
        }
        Err(e) => {
            error!("Failed to run openssl: {}", e);
        }
    }
}

async fn api_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    axum::Json(serde_json::json!({
        "status": "running",
        "version": "1.0.0",
        "server": "miwidothttp",
        "config": {
            "http_port": state.config.server.http_port,
            "https_port": state.config.server.https_port,
            "ssl_enabled": state.config.ssl.enabled,
            "backends_configured": state.config.backends.len(),
        }
    }))
}

async fn list_backends(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let backends: Vec<_> = state.config.backends.iter()
        .map(|(name, config)| {
            serde_json::json!({
                "name": name,
                "target": config.target,
                "health_check": config.health_check,
            })
        })
        .collect();
    
    axum::Json(serde_json::json!({
        "backends": backends
    }))
}

async fn metrics(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let metrics = state.metrics.get_prometheus_metrics().await;
    (StatusCode::OK, metrics)
}

async fn list_processes(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let statuses = state.process_manager.get_status().await;
    axum::Json(serde_json::json!({
        "processes": statuses
    }))
}

async fn restart_process(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> impl IntoResponse {
    match state.process_manager.restart_process(&name).await {
        Ok(_) => (StatusCode::OK, axum::Json(serde_json::json!({
            "status": "success",
            "message": format!("Process {} restarted", name)
        }))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, axum::Json(serde_json::json!({
            "status": "error",
            "message": format!("Failed to restart process: {}", e)
        })))
    }
}

async fn proxy_handler(
    Host(host): Host,
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
) -> impl IntoResponse {
    // Check if this host has a configured backend
    let backend = state.config.backends.get(&host);
    
    if let Some(backend_config) = backend {
        // Get the target URL - either from direct target or from process config
        let target = if let Some(ref target_str) = backend_config.target {
            target_str.clone()
        } else if let Some(ref process_config) = backend_config.process {
            format!("http://localhost:{}", process_config.port)
        } else {
            return Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(Body::from("No backend target configured"))
                .unwrap();
        };
        
        // Proxy the request to the backend
        let target_url = format!("{}{}", target, req.uri().path());
        
        info!("Proxying request from {} to {}", host, target_url);
        
        // Create proxy request
        let method = req.method().clone();
        let headers = req.headers().clone();
        let body_bytes = match axum::body::to_bytes(req.into_body(), usize::MAX).await {
            Ok(bytes) => bytes,
            Err(e) => {
                error!("Failed to read request body: {}", e);
                return Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .body(Body::from("Failed to read request body"))
                    .unwrap();
            }
        };
        
        // Build the proxy request
        let mut proxy_req = state.http_client
            .request(method, &target_url)
            .body(body_bytes.to_vec());
        
        // Copy headers (except Host)
        for (name, value) in headers.iter() {
            if name != "host" {
                proxy_req = proxy_req.header(name, value);
            }
        }
        
        // Send the request
        match proxy_req.send().await {
            Ok(resp) => {
                let status = StatusCode::from_u16(resp.status().as_u16()).unwrap();
                let headers = resp.headers().clone();
                let body = match resp.bytes().await {
                    Ok(bytes) => Body::from(bytes),
                    Err(e) => {
                        error!("Failed to read response body: {}", e);
                        Body::from("Failed to read response from backend")
                    }
                };
                
                let mut response = Response::builder().status(status);
                
                // Copy response headers
                for (name, value) in headers.iter() {
                    response = response.header(name, value);
                }
                
                response.body(body).unwrap()
            }
            Err(e) => {
                error!("Failed to proxy request: {}", e);
                Response::builder()
                    .status(StatusCode::BAD_GATEWAY)
                    .body(Body::from(format!("Backend unavailable: {}", e)))
                    .unwrap()
            }
        }
    } else {
        // No backend configured for this host, serve from static
        let path = req.uri().path();
        let file_path = state.static_dir.join(path.trim_start_matches('/'));
        
        if file_path.exists() && file_path.is_file() {
            // Serve the file
            match fs::read(&file_path).await {
                Ok(contents) => {
                    let mime = mime_guess::from_path(&file_path)
                        .first_or_octet_stream();
                    
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("Content-Type", mime.as_ref())
                        .body(Body::from(contents))
                        .unwrap()
                }
                Err(e) => {
                    error!("Failed to read file {:?}: {}", file_path, e);
                    Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(Body::from("Internal server error"))
                        .unwrap()
                }
            }
        } else {
            // Try index.html for directories
            let index_path = if file_path.is_dir() {
                file_path.join("index.html")
            } else {
                state.static_dir.join("index.html")
            };
            
            if index_path.exists() {
                match fs::read(&index_path).await {
                    Ok(contents) => {
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("Content-Type", "text/html")
                            .body(Body::from(contents))
                            .unwrap()
                    }
                    Err(_) => {
                        Response::builder()
                            .status(StatusCode::NOT_FOUND)
                            .body(Body::from("404 Not Found"))
                            .unwrap()
                    }
                }
            } else {
                Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Body::from("404 Not Found"))
                    .unwrap()
            }
        }
    }
}