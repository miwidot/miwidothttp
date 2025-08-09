use anyhow::{anyhow, Result};
use axum::{
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, Version},
    response::{IntoResponse, Response},
};
use hyper::client::conn::http1::Builder;
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, AsyncReadExt};
use tokio::net::{TcpStream, TcpListener};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

mod forward;
mod reverse;
mod socks;
mod websocket;

pub use forward::ForwardProxy;
pub use reverse::ReverseProxy;
pub use socks::{SocksProxy, SocksVersion};
pub use websocket::WebSocketProxy;

use crate::config::BackendConfig;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProxyConfig {
    pub mode: ProxyMode,
    pub bind_addr: Option<SocketAddr>,
    pub upstream_proxy: Option<UpstreamProxy>,
    pub authentication: Option<ProxyAuth>,
    pub connection_pool: ConnectionPoolConfig,
    pub headers: HeaderConfig,
    pub timeout: TimeoutConfig,
    pub limits: ProxyLimits,
    pub logging: ProxyLogging,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ProxyMode {
    Reverse,    // Reverse proxy (default)
    Forward,    // Forward proxy with CONNECT
    Transparent,// Transparent proxy
    Socks4,     // SOCKS4 proxy
    Socks5,     // SOCKS5 proxy
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UpstreamProxy {
    pub url: String,
    pub auth: Option<ProxyAuth>,
    pub use_for_https: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProxyAuth {
    pub auth_type: AuthType,
    pub username: String,
    pub password: String,
    pub realm: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AuthType {
    Basic,
    Digest,
    Ntlm,
    Bearer,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConnectionPoolConfig {
    pub max_idle_per_host: usize,
    pub idle_timeout_seconds: u64,
    pub max_lifetime_seconds: u64,
    pub http2: bool,
    pub keep_alive: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HeaderConfig {
    pub preserve_host: bool,
    pub add_forwarded_headers: bool,
    pub add_real_ip: bool,
    pub add_proxy_headers: bool,
    pub remove_headers: Vec<String>,
    pub add_headers: HashMap<String, String>,
    pub via_header: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TimeoutConfig {
    pub connect_timeout_seconds: u64,
    pub read_timeout_seconds: u64,
    pub write_timeout_seconds: u64,
    pub idle_timeout_seconds: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProxyLimits {
    pub max_request_size: u64,
    pub max_response_size: u64,
    pub max_concurrent_connections: usize,
    pub rate_limit_per_ip: Option<u32>,
    pub bandwidth_limit_kbps: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProxyLogging {
    pub log_requests: bool,
    pub log_responses: bool,
    pub log_headers: bool,
    pub log_body: bool,
    pub max_body_size: usize,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        ProxyConfig {
            mode: ProxyMode::Reverse,
            bind_addr: None,
            upstream_proxy: None,
            authentication: None,
            connection_pool: ConnectionPoolConfig {
                max_idle_per_host: 32,
                idle_timeout_seconds: 90,
                max_lifetime_seconds: 3600,
                http2: true,
                keep_alive: true,
            },
            headers: HeaderConfig {
                preserve_host: false,
                add_forwarded_headers: true,
                add_real_ip: true,
                add_proxy_headers: true,
                remove_headers: vec!["Connection".to_string(), "Upgrade".to_string()],
                add_headers: HashMap::new(),
                via_header: Some("miwidothttp/1.0".to_string()),
            },
            timeout: TimeoutConfig {
                connect_timeout_seconds: 10,
                read_timeout_seconds: 30,
                write_timeout_seconds: 30,
                idle_timeout_seconds: 90,
            },
            limits: ProxyLimits {
                max_request_size: 100 * 1024 * 1024, // 100MB
                max_response_size: 100 * 1024 * 1024, // 100MB
                max_concurrent_connections: 10000,
                rate_limit_per_ip: Some(1000),
                bandwidth_limit_kbps: None,
            },
            logging: ProxyLogging {
                log_requests: true,
                log_responses: false,
                log_headers: false,
                log_body: false,
                max_body_size: 4096,
            },
        }
    }
}

pub struct ProxyManager {
    config: ProxyConfig,
    client: Client<hyper_util::client::legacy::connect::HttpConnector, Body>,
    forward_proxy: Option<Arc<ForwardProxy>>,
    reverse_proxy: Arc<ReverseProxy>,
    socks_proxy: Option<Arc<SocksProxy>>,
    websocket_proxy: Arc<WebSocketProxy>,
    connection_count: Arc<RwLock<HashMap<IpAddr, u32>>>,
}

impl ProxyManager {
    pub fn new(config: ProxyConfig) -> Result<Self> {
        let connector = hyper_util::client::legacy::connect::HttpConnector::new();
        let client = Client::builder(TokioExecutor::new()).build(connector);

        let forward_proxy = if config.mode == ProxyMode::Forward {
            Some(Arc::new(ForwardProxy::new(config.clone())?))
        } else {
            None
        };

        let reverse_proxy = Arc::new(ReverseProxy::new(config.clone())?);

        let socks_proxy = if matches!(config.mode, ProxyMode::Socks4 | ProxyMode::Socks5) {
            let version = if config.mode == ProxyMode::Socks4 {
                SocksVersion::V4
            } else {
                SocksVersion::V5
            };
            Some(Arc::new(SocksProxy::new(version, config.clone())?))
        } else {
            None
        };

        let websocket_proxy = Arc::new(WebSocketProxy::new(config.clone())?);

        Ok(ProxyManager {
            config,
            client,
            forward_proxy,
            reverse_proxy,
            socks_proxy,
            websocket_proxy,
            connection_count: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    pub async fn handle_request(&self, req: Request<Body>) -> Result<Response, StatusCode> {
        let method = req.method();
        let uri = req.uri();
        let headers = req.headers();

        // Check for WebSocket upgrade
        if self.is_websocket_request(headers) {
            return self.websocket_proxy.handle_upgrade(req).await
                .map_err(|_| StatusCode::BAD_GATEWAY);
        }

        // Handle different proxy modes
        match self.config.mode {
            ProxyMode::Forward => {
                if method == Method::CONNECT {
                    self.handle_connect_method(req).await
                } else {
                    self.handle_forward_proxy(req).await
                }
            }
            ProxyMode::Reverse => {
                self.handle_reverse_proxy(req).await
            }
            ProxyMode::Transparent => {
                self.handle_transparent_proxy(req).await
            }
            ProxyMode::Socks4 | ProxyMode::Socks5 => {
                Err(StatusCode::METHOD_NOT_ALLOWED) // SOCKS handled separately
            }
        }
    }

    async fn handle_connect_method(&self, req: Request<Body>) -> Result<Response, StatusCode> {
        if let Some(forward_proxy) = &self.forward_proxy {
            forward_proxy.handle_connect(req).await
                .map_err(|_| StatusCode::BAD_GATEWAY)
        } else {
            Err(StatusCode::METHOD_NOT_ALLOWED)
        }
    }

    async fn handle_forward_proxy(&self, req: Request<Body>) -> Result<Response, StatusCode> {
        if let Some(forward_proxy) = &self.forward_proxy {
            forward_proxy.handle_request(req).await
                .map_err(|_| StatusCode::BAD_GATEWAY)
        } else {
            Err(StatusCode::METHOD_NOT_ALLOWED)
        }
    }

    async fn handle_reverse_proxy(&self, req: Request<Body>) -> Result<Response, StatusCode> {
        self.reverse_proxy.handle_request(req).await
            .map_err(|_| StatusCode::BAD_GATEWAY)
    }

    async fn handle_transparent_proxy(&self, req: Request<Body>) -> Result<Response, StatusCode> {
        // Transparent proxy intercepts traffic at network level
        // Implementation would depend on iptables/netfilter integration
        warn!("Transparent proxy not yet implemented");
        Err(StatusCode::NOT_IMPLEMENTED)
    }

    pub async fn proxy_request(&self, backend: &BackendConfig, req: Request<Body>) -> Result<Response> {
        self.reverse_proxy.proxy_to_backend(backend, req).await
    }

    pub async fn start_socks_server(&self) -> Result<()> {
        if let Some(socks_proxy) = &self.socks_proxy {
            if let Some(bind_addr) = self.config.bind_addr {
                socks_proxy.start_server(bind_addr).await?;
            } else {
                return Err(anyhow!("SOCKS proxy requires bind_addr"));
            }
        }
        Ok(())
    }

    fn is_websocket_request(&self, headers: &HeaderMap) -> bool {
        headers.get("upgrade")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.to_lowercase() == "websocket")
            .unwrap_or(false)
    }

    pub async fn check_rate_limit(&self, client_ip: IpAddr) -> bool {
        if let Some(limit) = self.config.limits.rate_limit_per_ip {
            let mut counts = self.connection_count.write().await;
            let count = counts.entry(client_ip).or_insert(0);
            
            if *count >= limit {
                return false;
            }
            
            *count += 1;
        }
        true
    }

    pub async fn health_check(&self, backend: &BackendConfig) -> bool {
        self.reverse_proxy.health_check(backend).await
    }

    pub fn add_proxy_headers(&self, headers: &mut HeaderMap, client_ip: &str, original_host: &str) {
        if self.config.headers.add_forwarded_headers {
            // RFC 7239 Forwarded header
            let forwarded = format!("for={};host={};proto=http", client_ip, original_host);
            headers.insert("Forwarded", HeaderValue::from_str(&forwarded).unwrap());
        }

        if self.config.headers.add_real_ip {
            headers.insert("X-Real-IP", HeaderValue::from_str(client_ip).unwrap());
            headers.insert("X-Forwarded-For", HeaderValue::from_str(client_ip).unwrap());
            headers.insert("X-Forwarded-Proto", HeaderValue::from_str("http").unwrap());
        }

        if self.config.headers.add_proxy_headers {
            headers.insert("X-Proxied-By", HeaderValue::from_str("miwidothttp").unwrap());
        }

        if let Some(via) = &self.config.headers.via_header {
            headers.insert("Via", HeaderValue::from_str(via).unwrap());
        }

        // Add custom headers
        for (key, value) in &self.config.headers.add_headers {
            if let (Ok(name), Ok(val)) = (
                axum::http::HeaderName::from_bytes(key.as_bytes()),
                HeaderValue::from_str(value)
            ) {
                headers.insert(name, val);
            }
        }

        // Remove unwanted headers
        for header in &self.config.headers.remove_headers {
            if let Ok(name) = axum::http::HeaderName::from_bytes(header.as_bytes()) {
                headers.remove(&name);
            }
        }
    }
}

// Proxy protocol support (HAProxy protocol)
pub struct ProxyProtocol {
    pub version: u8,
    pub command: ProxyCommand,
    pub family: ProxyFamily,
    pub protocol: ProxyTransport,
    pub src_addr: SocketAddr,
    pub dest_addr: SocketAddr,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProxyCommand {
    Local,
    Proxy,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProxyFamily {
    Inet,
    Inet6,
    Unix,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProxyTransport {
    Stream,
    Dgram,
}

impl ProxyProtocol {
    pub async fn parse<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Option<Self>> {
        let mut buf = [0u8; 108]; // Max proxy protocol v2 header size
        
        // Read first 16 bytes to determine version
        reader.read_exact(&mut buf[..16]).await?;
        
        // Check for proxy protocol v2 signature
        if &buf[..12] == b"\r\n\r\n\0\r\nQUIT\n" {
            return Self::parse_v2(&buf).await;
        }
        
        // Check for proxy protocol v1
        let header = String::from_utf8_lossy(&buf[..16]);
        if header.starts_with("PROXY ") {
            return Self::parse_v1(&header).await;
        }
        
        Ok(None)
    }

    async fn parse_v1(header: &str) -> Result<Option<Self>> {
        // PROXY TCP4 192.168.1.1 192.168.1.2 12345 80\r\n
        let parts: Vec<&str> = header.trim().split_whitespace().collect();
        
        if parts.len() >= 6 && parts[0] == "PROXY" {
            let family = match parts[1] {
                "TCP4" => ProxyFamily::Inet,
                "TCP6" => ProxyFamily::Inet6,
                _ => return Ok(None),
            };
            
            let src_ip: IpAddr = parts[2].parse()?;
            let dest_ip: IpAddr = parts[3].parse()?;
            let src_port: u16 = parts[4].parse()?;
            let dest_port: u16 = parts[5].parse()?;
            
            Ok(Some(ProxyProtocol {
                version: 1,
                command: ProxyCommand::Proxy,
                family,
                protocol: ProxyTransport::Stream,
                src_addr: SocketAddr::new(src_ip, src_port),
                dest_addr: SocketAddr::new(dest_ip, dest_port),
            }))
        } else {
            Ok(None)
        }
    }

    async fn parse_v2(buf: &[u8]) -> Result<Option<Self>> {
        // Proxy protocol v2 binary format
        if buf.len() < 16 {
            return Ok(None);
        }
        
        let version = (buf[12] & 0xF0) >> 4;
        let command = buf[12] & 0x0F;
        let family = (buf[13] & 0xF0) >> 4;
        let protocol = buf[13] & 0x0F;
        let length = u16::from_be_bytes([buf[14], buf[15]]) as usize;
        
        if version != 2 || buf.len() < 16 + length {
            return Ok(None);
        }
        
        // Parse addresses based on family
        let (src_addr, dest_addr) = match family {
            1 => { // IPv4
                if length < 12 { return Ok(None); }
                let src_ip = IpAddr::V4(std::net::Ipv4Addr::new(
                    buf[16], buf[17], buf[18], buf[19]
                ));
                let dest_ip = IpAddr::V4(std::net::Ipv4Addr::new(
                    buf[20], buf[21], buf[22], buf[23]
                ));
                let src_port = u16::from_be_bytes([buf[24], buf[25]]);
                let dest_port = u16::from_be_bytes([buf[26], buf[27]]);
                (SocketAddr::new(src_ip, src_port), SocketAddr::new(dest_ip, dest_port))
            }
            2 => { // IPv6
                if length < 36 { return Ok(None); }
                // IPv6 parsing implementation
                return Ok(None); // Simplified for now
            }
            _ => return Ok(None),
        };
        
        Ok(Some(ProxyProtocol {
            version: 2,
            command: if command == 1 { ProxyCommand::Proxy } else { ProxyCommand::Local },
            family: ProxyFamily::Inet,
            protocol: ProxyTransport::Stream,
            src_addr,
            dest_addr,
        }))
    }
}

// Connection statistics
#[derive(Debug, Clone)]
pub struct ProxyStats {
    pub total_connections: u64,
    pub active_connections: u64,
    pub bytes_transferred: u64,
    pub requests_per_second: f64,
    pub average_response_time: Duration,
    pub error_rate: f64,
}

impl ProxyManager {
    pub async fn get_stats(&self) -> ProxyStats {
        let connection_count = self.connection_count.read().await;
        
        ProxyStats {
            total_connections: connection_count.values().sum::<u32>() as u64,
            active_connections: connection_count.len() as u64,
            bytes_transferred: 0, // Would track in real implementation
            requests_per_second: 0.0, // Would calculate from metrics
            average_response_time: Duration::from_millis(0),
            error_rate: 0.0,
        }
    }
}