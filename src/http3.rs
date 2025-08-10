use anyhow::{anyhow, Result};
use bytes::Bytes;
use h3::{quic, server::RequestStream};
use h3_quinn::quinn;
use quinn::{Endpoint, ServerConfig, TransportConfig};
use rustls::{Certificate, PrivateKey};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use crate::config::Config;

pub struct Http3Server {
    config: Config,
    endpoint: Option<Endpoint>,
    connections: Arc<RwLock<Vec<Http3Connection>>>,
}

struct Http3Connection {
    id: String,
    remote_addr: SocketAddr,
    streams: u32,
    bytes_sent: u64,
    bytes_received: u64,
}

impl Http3Server {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            endpoint: None,
            connections: Arc::new(RwLock::new(Vec::new())),
        }
    }
    
    pub async fn start(&mut self, addr: SocketAddr, cert: Vec<Certificate>, key: PrivateKey) -> Result<()> {
        info!("Starting HTTP/3 server on {}", addr);
        
        // Configure QUIC
        let mut transport_config = TransportConfig::default();
        transport_config
            .max_concurrent_bidi_streams(100u32.into())
            .max_concurrent_uni_streams(100u32.into())
            .max_idle_timeout(Some(Duration::from_secs(30).try_into()?))
            .keep_alive_interval(Some(Duration::from_secs(10)));
        
        // Create rustls server config
        let mut tls_config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(cert, key)?;
        
        // Enable HTTP/3 ALPN
        tls_config.alpn_protocols = vec![b"h3".to_vec()];
        
        // Create Quinn server config
        let mut server_config = ServerConfig::with_crypto(Arc::new(
            quinn::crypto::rustls::QuicServerConfig::try_from(tls_config)?
        ));
        server_config.transport_config(Arc::new(transport_config));
        
        // Create endpoint
        let endpoint = Endpoint::server(server_config, addr)?;
        self.endpoint = Some(endpoint.clone());
        
        // Accept connections
        let connections = self.connections.clone();
        tokio::spawn(async move {
            while let Some(incoming) = endpoint.accept().await {
                let connections = connections.clone();
                tokio::spawn(async move {
                    if let Err(e) = Self::handle_connection(incoming, connections).await {
                        error!("HTTP/3 connection error: {}", e);
                    }
                });
            }
        });
        
        info!("HTTP/3 server started successfully");
        Ok(())
    }
    
    async fn handle_connection(
        incoming: quinn::Incoming,
        connections: Arc<RwLock<Vec<Http3Connection>>>,
    ) -> Result<()> {
        let remote_addr = incoming.remote_address();
        let conn = incoming.await?;
        
        info!("HTTP/3 connection from {}", remote_addr);
        
        // Create connection tracking
        let conn_info = Http3Connection {
            id: uuid::Uuid::new_v4().to_string(),
            remote_addr,
            streams: 0,
            bytes_sent: 0,
            bytes_received: 0,
        };
        
        connections.write().await.push(conn_info);
        
        // Create H3 connection
        let mut h3_conn = h3::server::Connection::new(h3_quinn::Connection::new(conn)).await?;
        
        // Handle requests
        while let Some(result) = h3_conn.accept().await {
            match result {
                Ok((req, stream)) => {
                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_request(req, stream).await {
                            error!("Request handling error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    warn!("Error accepting stream: {}", e);
                    match e.try_get_code() {
                        Some(h3::error::Code::H3_NO_ERROR) => {
                            debug!("Connection closed normally");
                            break;
                        }
                        _ => {
                            error!("HTTP/3 error: {}", e);
                            break;
                        }
                    }
                }
            }
        }
        
        Ok(())
    }
    
    async fn handle_request(
        req: http::Request<()>,
        mut stream: RequestStream<h3_quinn::BidiStream<Bytes>, Bytes>,
    ) -> Result<()> {
        let (method, uri, headers) = (req.method(), req.uri(), req.headers());
        
        info!("HTTP/3 {} {}", method, uri);
        debug!("Headers: {:?}", headers);
        
        // Read request body if present
        let mut body = Vec::new();
        while let Some(data) = stream.recv_data().await? {
            body.extend_from_slice(&data);
        }
        
        // Create response
        let response = Self::create_response(method, uri, &body).await?;
        
        // Send response
        stream.send_response(response).await?;
        stream.send_data(Bytes::from("Hello from HTTP/3!")).await?;
        stream.finish().await?;
        
        Ok(())
    }
    
    async fn create_response(
        method: &http::Method,
        uri: &http::Uri,
        body: &[u8],
    ) -> Result<http::Response<()>> {
        let mut response = http::Response::builder()
            .status(200)
            .header("content-type", "text/plain")
            .header("alt-svc", r#"h3=":443"; ma=86400"#);
        
        // Add custom headers
        response = response
            .header("server", "miwidothttp/0.1.0")
            .header("x-http-version", "3");
        
        Ok(response.body(())?)
    }
    
    pub async fn shutdown(&mut self) -> Result<()> {
        if let Some(endpoint) = self.endpoint.take() {
            endpoint.close(0u32.into(), b"server shutdown");
            endpoint.wait_idle().await;
            info!("HTTP/3 server shut down");
        }
        Ok(())
    }
    
    pub async fn get_stats(&self) -> Http3Stats {
        let connections = self.connections.read().await;
        
        Http3Stats {
            total_connections: connections.len(),
            total_streams: connections.iter().map(|c| c.streams).sum(),
            bytes_sent: connections.iter().map(|c| c.bytes_sent).sum(),
            bytes_received: connections.iter().map(|c| c.bytes_received).sum(),
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub struct Http3Stats {
    pub total_connections: usize,
    pub total_streams: u32,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

// HTTP/3 client for testing
pub struct Http3Client {
    endpoint: Endpoint,
}

impl Http3Client {
    pub async fn new() -> Result<Self> {
        let mut endpoint = Endpoint::client("0.0.0.0:0".parse()?)?;
        
        // Configure TLS
        let crypto = rustls::ClientConfig::builder()
            .with_root_certificates(rustls::RootCertStore::empty())
            .with_no_client_auth();
        
        let client_config = quinn::ClientConfig::new(Arc::new(
            quinn::crypto::rustls::QuicClientConfig::try_from(crypto)?
        ));
        
        endpoint.set_default_client_config(client_config);
        
        Ok(Self { endpoint })
    }
    
    pub async fn get(&self, url: &str) -> Result<String> {
        let uri: http::Uri = url.parse()?;
        let host = uri.host().ok_or_else(|| anyhow!("No host in URL"))?;
        let port = uri.port_u16().unwrap_or(443);
        
        // Connect
        let addr = format!("{}:{}", host, port).parse()?;
        let conn = self.endpoint.connect(addr, host)?.await?;
        
        // Create H3 connection
        let (mut conn, mut send_req) = h3::client::new(h3_quinn::Connection::new(conn)).await?;
        
        // Send request
        let req = http::Request::get(uri.path())
            .header("host", host)
            .body(())?;
        
        let mut stream = send_req.send_request(req).await?;
        stream.finish().await?;
        
        // Receive response
        let resp = stream.recv_response().await?;
        let status = resp.status();
        
        let mut body = Vec::new();
        while let Some(data) = stream.recv_data().await? {
            body.extend_from_slice(&data);
        }
        
        if status.is_success() {
            Ok(String::from_utf8(body)?)
        } else {
            Err(anyhow!("HTTP/3 request failed: {}", status))
        }
    }
}

// Integration with main server
pub async fn start_http3_server(config: &Config) -> Result<Http3Server> {
    let mut server = Http3Server::new(config.clone());
    
    // Load certificates
    let cert_path = config.ssl.cert_path.as_ref()
        .ok_or_else(|| anyhow!("No certificate path for HTTP/3"))?;
    let key_path = config.ssl.key_path.as_ref()
        .ok_or_else(|| anyhow!("No key path for HTTP/3"))?;
    
    let cert_chain = load_certs(cert_path)?;
    let key = load_key(key_path)?;
    
    let addr = format!("0.0.0.0:{}", config.server.https_port).parse()?;
    server.start(addr, cert_chain, key).await?;
    
    Ok(server)
}

fn load_certs(path: &str) -> Result<Vec<Certificate>> {
    let cert_file = std::fs::read(path)?;
    let mut reader = std::io::BufReader::new(&cert_file[..]);
    
    let certs = rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(Certificate)
        .collect();
    
    Ok(certs)
}

fn load_key(path: &str) -> Result<PrivateKey> {
    let key_file = std::fs::read(path)?;
    let mut reader = std::io::BufReader::new(&key_file[..]);
    
    let keys = rustls_pemfile::pkcs8_private_keys(&mut reader)
        .collect::<Result<Vec<_>, _>>()?;
    
    if keys.is_empty() {
        return Err(anyhow!("No private key found"));
    }
    
    Ok(PrivateKey(keys[0].secret_pkcs8_der().to_vec()))
}