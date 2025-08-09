use anyhow::Result;
use axum::{
    body::Body,
    extract::Request,
    http::{HeaderMap, Method, StatusCode, Uri},
    response::Response,
};
use hyper_util::client::legacy::Client;
use std::convert::Infallible;
use std::net::SocketAddr;
use tokio::io::{AsyncRead, AsyncWrite, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::{debug, error, info, warn};

use super::{ProxyAuth, ProxyConfig, UpstreamProxy};

pub struct ForwardProxy {
    config: ProxyConfig,
    client: Client<hyper_util::client::legacy::connect::HttpConnector, Body>,
}

impl ForwardProxy {
    pub fn new(config: ProxyConfig) -> Result<Self> {
        let connector = hyper_util::client::legacy::connect::HttpConnector::new();
        let client = Client::builder(hyper_util::rt::TokioExecutor::new()).build(connector);

        Ok(ForwardProxy { config, client })
    }

    // Handle HTTP CONNECT method for HTTPS tunneling
    pub async fn handle_connect(&self, req: Request<Body>) -> Result<Response> {
        let uri = req.uri();
        let authority = uri.authority().ok_or_else(|| {
            anyhow::anyhow!("Missing authority in CONNECT request")
        })?;

        info!("CONNECT request to: {}", authority);

        // Parse target address
        let target = format!("{}:{}", 
            authority.host(), 
            authority.port_u16().unwrap_or(443)
        );

        // Authenticate if required
        if self.config.authentication.is_some() {
            if !self.authenticate_request(req.headers()).await? {
                return Ok(Response::builder()
                    .status(StatusCode::PROXY_AUTHENTICATION_REQUIRED)
                    .header("Proxy-Authenticate", "Basic realm=\"Proxy\"")
                    .body(Body::empty())?);
            }
        }

        // Check if we should use upstream proxy
        if let Some(upstream) = &self.config.upstream_proxy {
            if upstream.use_for_https {
                return self.connect_through_upstream(&target, upstream).await;
            }
        }

        // Direct connection to target
        match TcpStream::connect(&target).await {
            Ok(mut target_stream) => {
                info!("Connected to target: {}", target);
                
                // Send 200 Connection Established
                let response = Response::builder()
                    .status(StatusCode::OK)
                    .body(Body::empty())?;

                // Note: In a real implementation, we would need to:
                // 1. Upgrade the connection to raw TCP
                // 2. Tunnel data bidirectionally
                // This requires more complex integration with Axum/Hyper

                Ok(response)
            }
            Err(e) => {
                error!("Failed to connect to {}: {}", target, e);
                Ok(Response::builder()
                    .status(StatusCode::BAD_GATEWAY)
                    .body(Body::from(format!("Failed to connect to {}", target)))?)
            }
        }
    }

    // Handle regular HTTP requests through forward proxy
    pub async fn handle_request(&self, mut req: Request<Body>) -> Result<Response> {
        let uri = req.uri().clone();
        
        // Ensure absolute URI for forward proxy
        if !uri.scheme_str().is_some() {
            return Err(anyhow::anyhow!("Forward proxy requires absolute URI"));
        }

        info!("Forward proxy request to: {}", uri);

        // Authenticate if required
        if self.config.authentication.is_some() {
            if !self.authenticate_request(req.headers()).await? {
                return Ok(Response::builder()
                    .status(StatusCode::PROXY_AUTHENTICATION_REQUIRED)
                    .header("Proxy-Authenticate", "Basic realm=\"Proxy\"")
                    .body(Body::empty())?);
            }
        }

        // Remove proxy-specific headers
        let headers = req.headers_mut();
        headers.remove("proxy-authorization");
        headers.remove("proxy-connection");

        // Check if we should use upstream proxy
        if let Some(upstream) = &self.config.upstream_proxy {
            return self.request_through_upstream(req, upstream).await;
        }

        // Direct request to target
        match self.client.request(req).await {
            Ok(response) => {
                debug!("Forward proxy response: {}", response.status());
                Ok(response)
            }
            Err(e) => {
                error!("Forward proxy request failed: {}", e);
                Ok(Response::builder()
                    .status(StatusCode::BAD_GATEWAY)
                    .body(Body::from("Proxy request failed"))?)
            }
        }
    }

    async fn authenticate_request(&self, headers: &HeaderMap) -> Result<bool> {
        let auth = match &self.config.authentication {
            Some(auth) => auth,
            None => return Ok(true),
        };

        let proxy_auth = headers.get("proxy-authorization")
            .and_then(|v| v.to_str().ok());

        match proxy_auth {
            Some(auth_header) => {
                self.validate_proxy_auth(auth_header, auth).await
            }
            None => Ok(false),
        }
    }

    async fn validate_proxy_auth(&self, auth_header: &str, config: &ProxyAuth) -> Result<bool> {
        match config.auth_type {
            super::AuthType::Basic => {
                if let Some(encoded) = auth_header.strip_prefix("Basic ") {
                    match base64::decode(encoded) {
                        Ok(decoded) => {
                            let credentials = String::from_utf8_lossy(&decoded);
                            let expected = format!("{}:{}", config.username, config.password);
                            Ok(credentials == expected)
                        }
                        Err(_) => Ok(false),
                    }
                } else {
                    Ok(false)
                }
            }
            _ => {
                warn!("Unsupported proxy authentication type: {:?}", config.auth_type);
                Ok(false)
            }
        }
    }

    async fn connect_through_upstream(&self, target: &str, upstream: &UpstreamProxy) -> Result<Response> {
        // Parse upstream proxy URL
        let upstream_uri: Uri = upstream.url.parse()?;
        let upstream_host = upstream_uri.host().unwrap_or("localhost");
        let upstream_port = upstream_uri.port_u16().unwrap_or(8080);
        let upstream_addr = format!("{}:{}", upstream_host, upstream_port);

        // Connect to upstream proxy
        match TcpStream::connect(&upstream_addr).await {
            Ok(mut upstream_stream) => {
                // Send CONNECT request to upstream
                let connect_req = format!(
                    "CONNECT {} HTTP/1.1\r\nHost: {}\r\n",
                    target, target
                );

                // Add upstream authentication if configured
                let connect_req = if let Some(auth) = &upstream.auth {
                    let encoded = base64::encode(format!("{}:{}", auth.username, auth.password));
                    format!("{}Proxy-Authorization: Basic {}\r\n", connect_req, encoded)
                } else {
                    connect_req
                };

                let connect_req = format!("{}\r\n", connect_req);

                upstream_stream.write_all(connect_req.as_bytes()).await?;

                // Read response from upstream
                let mut response_buf = Vec::new();
                let mut temp_buf = [0u8; 1024];
                
                loop {
                    let n = upstream_stream.read(&mut temp_buf).await?;
                    if n == 0 { break; }
                    response_buf.extend_from_slice(&temp_buf[..n]);
                    
                    // Check if we have complete HTTP response headers
                    if let Ok(response_str) = String::from_utf8_lossy(&response_buf).find("\r\n\r\n") {
                        break;
                    }
                }

                let response_str = String::from_utf8_lossy(&response_buf);
                if response_str.contains("200") {
                    info!("Successfully connected through upstream proxy to: {}", target);
                    
                    Ok(Response::builder()
                        .status(StatusCode::OK)
                        .body(Body::empty())?)
                } else {
                    error!("Upstream proxy connection failed: {}", response_str);
                    Ok(Response::builder()
                        .status(StatusCode::BAD_GATEWAY)
                        .body(Body::from("Upstream proxy connection failed"))?)
                }
            }
            Err(e) => {
                error!("Failed to connect to upstream proxy {}: {}", upstream_addr, e);
                Ok(Response::builder()
                    .status(StatusCode::BAD_GATEWAY)
                    .body(Body::from("Failed to connect to upstream proxy"))?)
            }
        }
    }

    async fn request_through_upstream(&self, mut req: Request<Body>, upstream: &UpstreamProxy) -> Result<Response> {
        // Add upstream proxy authentication
        if let Some(auth) = &upstream.auth {
            let encoded = base64::encode(format!("{}:{}", auth.username, auth.password));
            req.headers_mut().insert(
                "proxy-authorization",
                format!("Basic {}", encoded).parse()?
            );
        }

        // Forward request through upstream proxy
        // Note: This would require configuring the HTTP client to use the upstream proxy
        // For now, we'll use direct connection
        match self.client.request(req).await {
            Ok(response) => Ok(response),
            Err(e) => {
                error!("Upstream proxy request failed: {}", e);
                Ok(Response::builder()
                    .status(StatusCode::BAD_GATEWAY)
                    .body(Body::from("Upstream proxy request failed"))?)
            }
        }
    }

    // Start forward proxy server
    pub async fn start_server(&self, bind_addr: SocketAddr) -> Result<()> {
        let listener = TcpListener::bind(bind_addr).await?;
        info!("Forward proxy server listening on {}", bind_addr);

        loop {
            match listener.accept().await {
                Ok((stream, peer_addr)) => {
                    info!("New forward proxy connection from: {}", peer_addr);
                    
                    let proxy = self.clone();
                    tokio::spawn(async move {
                        if let Err(e) = proxy.handle_connection(stream, peer_addr).await {
                            error!("Forward proxy connection error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    error!("Failed to accept forward proxy connection: {}", e);
                }
            }
        }
    }

    async fn handle_connection(&self, mut stream: TcpStream, peer_addr: SocketAddr) -> Result<()> {
        let mut buffer = vec![0u8; 4096];
        
        loop {
            let n = stream.read(&mut buffer).await?;
            if n == 0 {
                break; // Connection closed
            }

            // Parse HTTP request
            let request_data = &buffer[..n];
            let request_str = String::from_utf8_lossy(request_data);
            
            // Extract method and URI
            let lines: Vec<&str> = request_str.lines().collect();
            if let Some(request_line) = lines.first() {
                let parts: Vec<&str> = request_line.split_whitespace().collect();
                if parts.len() >= 2 {
                    let method = parts[0];
                    let uri = parts[1];
                    
                    info!("Forward proxy request: {} {}", method, uri);
                    
                    // Handle CONNECT method
                    if method == "CONNECT" {
                        if let Err(e) = self.handle_raw_connect(&mut stream, uri).await {
                            error!("CONNECT handling failed: {}", e);
                            break;
                        }
                    } else {
                        // Handle regular HTTP request
                        if let Err(e) = self.handle_raw_http_request(&mut stream, request_data).await {
                            error!("HTTP request handling failed: {}", e);
                            break;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    async fn handle_raw_connect(&self, client_stream: &mut TcpStream, target: &str) -> Result<()> {
        // Connect to target
        match TcpStream::connect(target).await {
            Ok(target_stream) => {
                // Send 200 Connection Established
                let response = b"HTTP/1.1 200 Connection Established\r\n\r\n";
                client_stream.write_all(response).await?;

                // Start bidirectional tunneling
                self.tunnel_streams(client_stream, target_stream).await?;
            }
            Err(e) => {
                error!("Failed to connect to target {}: {}", target, e);
                let response = b"HTTP/1.1 502 Bad Gateway\r\n\r\n";
                client_stream.write_all(response).await?;
            }
        }

        Ok(())
    }

    async fn handle_raw_http_request(&self, client_stream: &mut TcpStream, request_data: &[u8]) -> Result<()> {
        // Parse request and forward to target
        // This is a simplified implementation
        let response = b"HTTP/1.1 501 Not Implemented\r\nContent-Length: 0\r\n\r\n";
        client_stream.write_all(response).await?;
        Ok(())
    }

    async fn tunnel_streams(&self, client_stream: &mut TcpStream, target_stream: TcpStream) -> Result<()> {
        let (mut target_read, mut target_write) = target_stream.into_split();
        let (mut client_read, mut client_write) = client_stream.split();

        // Bidirectional copy
        let client_to_target = tokio::spawn(async move {
            tokio::io::copy(&mut client_read, &mut target_write).await
        });

        let target_to_client = tokio::spawn(async move {
            tokio::io::copy(&mut target_read, &mut client_write).await
        });

        // Wait for either direction to complete
        tokio::select! {
            result = client_to_target => {
                match result {
                    Ok(Ok(bytes)) => info!("Client to target: {} bytes", bytes),
                    Ok(Err(e)) => error!("Client to target error: {}", e),
                    Err(e) => error!("Client to target task error: {}", e),
                }
            }
            result = target_to_client => {
                match result {
                    Ok(Ok(bytes)) => info!("Target to client: {} bytes", bytes),
                    Ok(Err(e)) => error!("Target to client error: {}", e),
                    Err(e) => error!("Target to client task error: {}", e),
                }
            }
        }

        Ok(())
    }
}

impl Clone for ForwardProxy {
    fn clone(&self) -> Self {
        ForwardProxy {
            config: self.config.clone(),
            client: self.client.clone(),
        }
    }
}

use base64;