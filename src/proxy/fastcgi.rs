use anyhow::Result;
use axum::{
    body::{Body, Bytes},
    extract::Request,
    http::{HeaderMap, Method, StatusCode, Uri},
    response::Response,
};
use std::collections::HashMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::{debug, error, info, warn};

const FCGI_VERSION: u8 = 1;
const FCGI_BEGIN_REQUEST: u8 = 1;
const FCGI_ABORT_REQUEST: u8 = 2;
const FCGI_END_REQUEST: u8 = 3;
const FCGI_PARAMS: u8 = 4;
const FCGI_STDIN: u8 = 5;
const FCGI_STDOUT: u8 = 6;
const FCGI_STDERR: u8 = 7;
const FCGI_DATA: u8 = 8;
const FCGI_GET_VALUES: u8 = 9;
const FCGI_GET_VALUES_RESULT: u8 = 10;

const FCGI_RESPONDER: u16 = 1;
const FCGI_AUTHORIZER: u16 = 2;
const FCGI_FILTER: u16 = 3;

const FCGI_REQUEST_COMPLETE: u8 = 0;
const FCGI_CANT_MPX_CONN: u8 = 1;
const FCGI_OVERLOADED: u8 = 2;
const FCGI_UNKNOWN_ROLE: u8 = 3;

#[derive(Debug, Clone)]
pub struct FastCGIConfig {
    pub socket_path: Option<String>,
    pub tcp_addr: Option<String>,
    pub document_root: PathBuf,
    pub index_files: Vec<String>,
    pub script_filename: Option<String>,
    pub params: HashMap<String, String>,
    pub connect_timeout: u64,
    pub read_timeout: u64,
    pub write_timeout: u64,
}

impl Default for FastCGIConfig {
    fn default() -> Self {
        FastCGIConfig {
            socket_path: Some("/var/run/php/php8.3-fpm.sock".to_string()),
            tcp_addr: None,
            document_root: PathBuf::from("/var/www/html"),
            index_files: vec!["index.php".to_string(), "index.html".to_string()],
            script_filename: None,
            params: HashMap::new(),
            connect_timeout: 10,
            read_timeout: 30,
            write_timeout: 30,
        }
    }
}

pub struct FastCGIProxy {
    config: FastCGIConfig,
}

impl FastCGIProxy {
    pub fn new(config: FastCGIConfig) -> Self {
        FastCGIProxy { config }
    }

    pub async fn handle_request(&self, req: Request<Body>) -> Result<Response> {
        let method = req.method().clone();
        let uri = req.uri().clone();
        let headers = req.headers().clone();
        
        // Determine script to execute
        let script_path = self.resolve_script_path(&uri)?;
        
        // Connect to PHP-FPM
        let mut stream = self.connect_to_phpfpm().await?;
        
        // Prepare FastCGI request
        let request_id = 1u16;
        
        // Send BEGIN_REQUEST
        self.send_begin_request(&mut stream, request_id).await?;
        
        // Send PARAMS
        let params = self.build_params(&method, &uri, &headers, &script_path);
        self.send_params(&mut stream, request_id, params).await?;
        
        // Send empty PARAMS to indicate end
        self.send_empty_params(&mut stream, request_id).await?;
        
        // Send STDIN (request body)
        let body_bytes = axum::body::to_bytes(req.into_body(), usize::MAX).await?;
        if !body_bytes.is_empty() {
            self.send_stdin(&mut stream, request_id, &body_bytes).await?;
        }
        self.send_empty_stdin(&mut stream, request_id).await?;
        
        // Read response
        let (status, response_headers, response_body) = self.read_response(&mut stream, request_id).await?;
        
        // Build HTTP response
        let mut response = Response::builder().status(status);
        
        for (key, value) in response_headers {
            response = response.header(key, value);
        }
        
        Ok(response.body(Body::from(response_body))?)
    }

    async fn connect_to_phpfpm(&self) -> Result<TcpStream> {
        if let Some(tcp_addr) = &self.config.tcp_addr {
            info!("Connecting to PHP-FPM at {}", tcp_addr);
            Ok(TcpStream::connect(tcp_addr).await?)
        } else if let Some(socket_path) = &self.config.socket_path {
            // Unix domain socket support would go here
            // For now, fallback to TCP
            info!("Connecting to PHP-FPM at localhost:9000");
            Ok(TcpStream::connect("127.0.0.1:9000").await?)
        } else {
            Err(anyhow::anyhow!("No PHP-FPM connection configured"))
        }
    }

    async fn send_begin_request(&self, stream: &mut TcpStream, request_id: u16) -> Result<()> {
        let mut packet = vec![
            FCGI_VERSION,
            FCGI_BEGIN_REQUEST,
            (request_id >> 8) as u8,
            request_id as u8,
            0, 8, // Content length
            0, // Padding
            0, // Reserved
        ];
        
        // Role and flags
        packet.extend_from_slice(&[
            (FCGI_RESPONDER >> 8) as u8,
            FCGI_RESPONDER as u8,
            0, // Flags (0 = close connection after request)
            0, 0, 0, 0, 0, // Reserved
        ]);
        
        stream.write_all(&packet).await?;
        Ok(())
    }

    fn build_params(&self, method: &Method, uri: &Uri, headers: &HeaderMap, script_path: &Path) -> HashMap<String, String> {
        let mut params = HashMap::new();
        
        // Required CGI/FastCGI parameters
        params.insert("REQUEST_METHOD".to_string(), method.to_string());
        params.insert("SCRIPT_FILENAME".to_string(), script_path.to_string_lossy().to_string());
        params.insert("SCRIPT_NAME".to_string(), uri.path().to_string());
        params.insert("REQUEST_URI".to_string(), uri.to_string());
        params.insert("DOCUMENT_URI".to_string(), uri.path().to_string());
        params.insert("DOCUMENT_ROOT".to_string(), self.config.document_root.to_string_lossy().to_string());
        params.insert("SERVER_PROTOCOL".to_string(), "HTTP/1.1".to_string());
        params.insert("GATEWAY_INTERFACE".to_string(), "CGI/1.1".to_string());
        params.insert("SERVER_SOFTWARE".to_string(), "miwidothttp/1.0".to_string());
        
        // Query string
        if let Some(query) = uri.query() {
            params.insert("QUERY_STRING".to_string(), query.to_string());
        } else {
            params.insert("QUERY_STRING".to_string(), String::new());
        }
        
        // Server info
        params.insert("SERVER_NAME".to_string(), 
            headers.get("host")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("localhost")
                .to_string());
        params.insert("SERVER_PORT".to_string(), "80".to_string());
        
        // Remote info
        params.insert("REMOTE_ADDR".to_string(), "127.0.0.1".to_string());
        params.insert("REMOTE_PORT".to_string(), "0".to_string());
        
        // HTTP headers
        for (name, value) in headers {
            let header_name = format!("HTTP_{}", 
                name.as_str().to_uppercase().replace('-', "_"));
            if let Ok(header_value) = value.to_str() {
                params.insert(header_name, header_value.to_string());
            }
        }
        
        // Content type and length
        if let Some(content_type) = headers.get("content-type") {
            if let Ok(ct) = content_type.to_str() {
                params.insert("CONTENT_TYPE".to_string(), ct.to_string());
            }
        }
        
        if let Some(content_length) = headers.get("content-length") {
            if let Ok(cl) = content_length.to_str() {
                params.insert("CONTENT_LENGTH".to_string(), cl.to_string());
            }
        }
        
        // PHP-specific
        params.insert("PHP_SELF".to_string(), uri.path().to_string());
        
        // Add custom params from config
        for (key, value) in &self.config.params {
            params.insert(key.clone(), value.clone());
        }
        
        params
    }

    async fn send_params(&self, stream: &mut TcpStream, request_id: u16, params: HashMap<String, String>) -> Result<()> {
        let mut param_bytes = Vec::new();
        
        for (key, value) in params {
            // Encode key length
            if key.len() < 128 {
                param_bytes.push(key.len() as u8);
            } else {
                param_bytes.push(((key.len() >> 24) | 0x80) as u8);
                param_bytes.push((key.len() >> 16) as u8);
                param_bytes.push((key.len() >> 8) as u8);
                param_bytes.push(key.len() as u8);
            }
            
            // Encode value length
            if value.len() < 128 {
                param_bytes.push(value.len() as u8);
            } else {
                param_bytes.push(((value.len() >> 24) | 0x80) as u8);
                param_bytes.push((value.len() >> 16) as u8);
                param_bytes.push((value.len() >> 8) as u8);
                param_bytes.push(value.len() as u8);
            }
            
            // Add key and value
            param_bytes.extend_from_slice(key.as_bytes());
            param_bytes.extend_from_slice(value.as_bytes());
        }
        
        // Send params in chunks if necessary
        for chunk in param_bytes.chunks(65535) {
            let packet = self.build_packet(FCGI_PARAMS, request_id, chunk);
            stream.write_all(&packet).await?;
        }
        
        Ok(())
    }

    async fn send_empty_params(&self, stream: &mut TcpStream, request_id: u16) -> Result<()> {
        let packet = self.build_packet(FCGI_PARAMS, request_id, &[]);
        stream.write_all(&packet).await?;
        Ok(())
    }

    async fn send_stdin(&self, stream: &mut TcpStream, request_id: u16, data: &[u8]) -> Result<()> {
        for chunk in data.chunks(65535) {
            let packet = self.build_packet(FCGI_STDIN, request_id, chunk);
            stream.write_all(&packet).await?;
        }
        Ok(())
    }

    async fn send_empty_stdin(&self, stream: &mut TcpStream, request_id: u16) -> Result<()> {
        let packet = self.build_packet(FCGI_STDIN, request_id, &[]);
        stream.write_all(&packet).await?;
        Ok(())
    }

    fn build_packet(&self, packet_type: u8, request_id: u16, content: &[u8]) -> Vec<u8> {
        let content_length = content.len();
        let padding_length = (8 - (content_length % 8)) % 8;
        
        let mut packet = vec![
            FCGI_VERSION,
            packet_type,
            (request_id >> 8) as u8,
            request_id as u8,
            (content_length >> 8) as u8,
            content_length as u8,
            padding_length as u8,
            0, // Reserved
        ];
        
        packet.extend_from_slice(content);
        packet.extend(vec![0; padding_length]);
        
        packet
    }

    async fn read_response(&self, stream: &mut TcpStream, request_id: u16) -> Result<(StatusCode, HashMap<String, String>, Vec<u8>)> {
        let mut stdout_data = Vec::new();
        let mut stderr_data = Vec::new();
        
        loop {
            let mut header = [0u8; 8];
            stream.read_exact(&mut header).await?;
            
            let version = header[0];
            let record_type = header[1];
            let record_request_id = ((header[2] as u16) << 8) | (header[3] as u16);
            let content_length = ((header[4] as u16) << 8) | (header[5] as u16);
            let padding_length = header[6];
            
            if version != FCGI_VERSION {
                return Err(anyhow::anyhow!("Invalid FastCGI version"));
            }
            
            if record_request_id != request_id {
                warn!("Received record for different request ID");
                continue;
            }
            
            let mut content = vec![0u8; content_length as usize];
            stream.read_exact(&mut content).await?;
            
            let mut padding = vec![0u8; padding_length as usize];
            stream.read_exact(&mut padding).await?;
            
            match record_type {
                FCGI_STDOUT => {
                    stdout_data.extend_from_slice(&content);
                }
                FCGI_STDERR => {
                    stderr_data.extend_from_slice(&content);
                    let error = String::from_utf8_lossy(&content);
                    warn!("PHP-FPM stderr: {}", error);
                }
                FCGI_END_REQUEST => {
                    break;
                }
                _ => {
                    debug!("Received FastCGI record type: {}", record_type);
                }
            }
        }
        
        // Parse HTTP response from stdout
        let (status, headers, body) = self.parse_http_response(&stdout_data)?;
        
        Ok((status, headers, body))
    }

    fn parse_http_response(&self, data: &[u8]) -> Result<(StatusCode, HashMap<String, String>, Vec<u8>)> {
        let response_str = String::from_utf8_lossy(data);
        let parts: Vec<&str> = response_str.splitn(2, "\r\n\r\n").collect();
        
        if parts.len() != 2 {
            return Err(anyhow::anyhow!("Invalid HTTP response from PHP-FPM"));
        }
        
        let headers_str = parts[0];
        let body = parts[1].as_bytes().to_vec();
        
        let mut headers = HashMap::new();
        let mut status = StatusCode::OK;
        
        for line in headers_str.lines() {
            if let Some((key, value)) = line.split_once(':') {
                let key = key.trim();
                let value = value.trim();
                
                if key.to_lowercase() == "status" {
                    // Parse status code
                    if let Some(code_str) = value.split_whitespace().next() {
                        if let Ok(code) = code_str.parse::<u16>() {
                            status = StatusCode::from_u16(code).unwrap_or(StatusCode::OK);
                        }
                    }
                } else {
                    headers.insert(key.to_string(), value.to_string());
                }
            }
        }
        
        Ok((status, headers, body))
    }

    fn resolve_script_path(&self, uri: &Uri) -> Result<PathBuf> {
        let path = uri.path().trim_start_matches('/');
        let mut script_path = self.config.document_root.join(path);
        
        // Check if path is a directory
        if script_path.is_dir() {
            // Look for index files
            for index in &self.config.index_files {
                let index_path = script_path.join(index);
                if index_path.exists() && index_path.extension() == Some(std::ffi::OsStr::new("php")) {
                    return Ok(index_path);
                }
            }
        }
        
        // Check if it's a PHP file
        if script_path.extension() != Some(std::ffi::OsStr::new("php")) {
            // Try adding .php extension
            let php_path = PathBuf::from(format!("{}.php", script_path.display()));
            if php_path.exists() {
                return Ok(php_path);
            }
            
            // Not a PHP file, return 404
            return Err(anyhow::anyhow!("Not a PHP file"));
        }
        
        if !script_path.exists() {
            return Err(anyhow::anyhow!("Script not found"));
        }
        
        Ok(script_path)
    }
}

impl Clone for FastCGIProxy {
    fn clone(&self) -> Self {
        FastCGIProxy {
            config: self.config.clone(),
        }
    }
}