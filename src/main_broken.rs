use anyhow::Result;
use axum::{
    body::Body,
    extract::{Host, Request, State},
    http::{StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use std::{net::SocketAddr, sync::Arc};
use tower::ServiceBuilder;
use tower_http::{
    compression::CompressionLayer,
    cors::CorsLayer,
    trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer},
};
use tracing::{info, Level};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod config;
mod error;
mod logging;
mod middleware;
mod process;
mod proxy;
mod rewrite;
mod session;
mod ssl;
mod vhost;

use config::Config;
use process::ProcessManager;
use proxy::ProxyManager;
use ssl::SslManager;

#[derive(Clone)]
struct AppState {
    config: Arc<Config>,
    process_manager: Arc<ProcessManager>,
    proxy_manager: Arc<ProxyManager>,
    ssl_manager: Arc<SslManager>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "miwidothttp=debug,tower_http=debug,axum=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("Starting miwidothttp server...");

    let config = Config::load("config.toml")?;
    let process_manager = Arc::new(ProcessManager::new());
    let proxy_manager = Arc::new(ProxyManager::new());
    let ssl_manager = Arc::new(SslManager::new(config.clone()));

    // Start backend processes
    for (name, backend) in &config.backends {
        if let Err(e) = process_manager.start_backend(name.clone(), backend).await {
            tracing::warn!("Failed to start backend {}: {}", name, e);
        }
    }

    let state = AppState {
        config: Arc::new(config.clone()),
        process_manager: process_manager.clone(),
        proxy_manager,
        ssl_manager,
    };

    let app = create_router(state.clone());

    let addr = SocketAddr::from(([0, 0, 0, 0], config.server.http_port));
    info!("HTTP server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    
    if config.server.enable_https {
        let https_addr = SocketAddr::from(([0, 0, 0, 0], config.server.https_port));
        info!("HTTPS server listening on {}", https_addr);
        
        tokio::spawn(async move {
            if let Err(e) = start_https_server(state.clone(), https_addr).await {
                tracing::error!("HTTPS server error: {}", e);
            }
        });
    }

    axum::serve(listener, app).await?;

    Ok(())
}

fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(root_handler))
        .route("/health", get(health_handler))
        .route("/metrics", get(metrics_handler))
        .fallback(proxy_handler)
        .layer(
            ServiceBuilder::new()
                .layer(
                    TraceLayer::new_for_http()
                        .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                        .on_response(DefaultOnResponse::new().level(Level::INFO)),
                )
                .layer(CompressionLayer::new())
                .layer(CorsLayer::permissive()),
        )
        .with_state(state)
}

async fn start_https_server(state: AppState, addr: SocketAddr) -> Result<()> {
    let app = create_router(state.clone());
    
    let tls_config = state.ssl_manager.get_tls_config().await?;
    
    axum_server::bind_rustls(addr, tls_config)
        .serve(app.into_make_service())
        .await?;
    
    Ok(())
}

async fn root_handler() -> impl IntoResponse {
    (
        StatusCode::OK,
        "miwidothttp - High-performance HTTP server with Cloudflare SSL",
    )
}

async fn health_handler() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}

async fn metrics_handler() -> impl IntoResponse {
    (StatusCode::OK, "Metrics endpoint - TODO")
}

async fn proxy_handler(
    State(state): State<AppState>,
    Host(host): Host,
    uri: Uri,
    req: Request<Body>,
) -> Result<Response, StatusCode> {
    info!("Proxy request: {} {}", host, uri);
    
    if let Some(backend) = state.config.get_backend(&host) {
        match state.proxy_manager.proxy_request(backend, req).await {
            Ok(response) => Ok(response),
            Err(e) => {
                tracing::error!("Proxy error: {}", e);
                Err(StatusCode::BAD_GATEWAY)
            }
        }
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}
