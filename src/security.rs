use axum::{
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode},
    middleware::Next,
    response::Response,
};
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::collections::HashMap;
use tokio::sync::RwLock;
use tracing::{warn, info};
use std::net::IpAddr;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SecurityConfig {
    pub enable_hsts: bool,
    pub hsts_max_age: u32,
    pub enable_csp: bool,
    pub csp_policy: String,
    pub enable_rate_limiting: bool,
    pub rate_limit_requests: u32,
    pub rate_limit_window: Duration,
    pub max_body_size: usize,
    pub max_header_size: usize,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            enable_hsts: true,
            hsts_max_age: 31536000, // 1 year
            enable_csp: true,
            csp_policy: "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'".to_string(),
            enable_rate_limiting: true,
            rate_limit_requests: 100,
            rate_limit_window: Duration::from_secs(60),
            max_body_size: 10 * 1024 * 1024, // 10MB
            max_header_size: 8192, // 8KB
        }
    }
}

#[derive(Clone)]
pub struct RateLimiter {
    requests: Arc<RwLock<HashMap<IpAddr, Vec<Instant>>>>,
    config: SecurityConfig,
}

impl RateLimiter {
    pub fn new(config: SecurityConfig) -> Self {
        Self {
            requests: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    pub async fn check_rate_limit(&self, ip: IpAddr) -> bool {
        if !self.config.enable_rate_limiting {
            return true;
        }

        let mut requests = self.requests.write().await;
        let now = Instant::now();
        
        let timestamps = requests.entry(ip).or_insert_with(Vec::new);
        
        // Remove old timestamps outside the window
        timestamps.retain(|t| now.duration_since(*t) < self.config.rate_limit_window);
        
        if timestamps.len() >= self.config.rate_limit_requests as usize {
            warn!("Rate limit exceeded for IP: {}", ip);
            false
        } else {
            timestamps.push(now);
            true
        }
    }
}

pub async fn security_headers_middleware(
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    
    // Security headers
    headers.insert(
        HeaderName::from_static("x-frame-options"),
        HeaderValue::from_static("DENY"),
    );
    
    headers.insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    
    headers.insert(
        HeaderName::from_static("x-xss-protection"),
        HeaderValue::from_static("1; mode=block"),
    );
    
    headers.insert(
        HeaderName::from_static("referrer-policy"),
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    
    headers.insert(
        HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static("geolocation=(), microphone=(), camera=()"),
    );
    
    Ok(response)
}

pub async fn hsts_middleware(
    State(config): State<Arc<SecurityConfig>>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let mut response = next.run(request).await;
    
    if config.enable_hsts {
        response.headers_mut().insert(
            HeaderName::from_static("strict-transport-security"),
            HeaderValue::from_str(&format!(
                "max-age={}; includeSubDomains; preload",
                config.hsts_max_age
            )).unwrap(),
        );
    }
    
    Ok(response)
}

pub async fn csp_middleware(
    State(config): State<Arc<SecurityConfig>>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let mut response = next.run(request).await;
    
    if config.enable_csp {
        response.headers_mut().insert(
            HeaderName::from_static("content-security-policy"),
            HeaderValue::from_str(&config.csp_policy).unwrap(),
        );
    }
    
    Ok(response)
}

pub async fn rate_limit_middleware(
    State(limiter): State<Arc<RateLimiter>>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Extract client IP (in real implementation, handle X-Forwarded-For)
    let ip = request
        .extensions()
        .get::<std::net::SocketAddr>()
        .map(|addr| addr.ip())
        .unwrap_or_else(|| "127.0.0.1".parse().unwrap());
    
    if !limiter.check_rate_limit(ip).await {
        return Ok(Response::builder()
            .status(StatusCode::TOO_MANY_REQUESTS)
            .header("Retry-After", "60")
            .body(Body::from("Rate limit exceeded"))
            .unwrap());
    }
    
    Ok(next.run(request).await)
}

// Request size limiting
pub async fn size_limit_middleware(
    State(config): State<Arc<SecurityConfig>>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Check header size
    let header_size: usize = request.headers()
        .iter()
        .map(|(name, value)| name.as_str().len() + value.len())
        .sum();
    
    if header_size > config.max_header_size {
        warn!("Request headers too large: {} bytes", header_size);
        return Ok(Response::builder()
            .status(StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE)
            .body(Body::from("Request headers too large"))
            .unwrap());
    }
    
    // Check Content-Length
    if let Some(content_length) = request.headers().get("content-length") {
        if let Ok(length_str) = content_length.to_str() {
            if let Ok(length) = length_str.parse::<usize>() {
                if length > config.max_body_size {
                    warn!("Request body too large: {} bytes", length);
                    return Ok(Response::builder()
                        .status(StatusCode::PAYLOAD_TOO_LARGE)
                        .body(Body::from("Request body too large"))
                        .unwrap());
                }
            }
        }
    }
    
    Ok(next.run(request).await)
}

// CORS headers (already handled by tower_http, but we can add custom logic)
pub async fn cors_middleware(
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let origin = request.headers()
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "*".to_string());
    
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    
    // Add CORS headers if not already present
    if !headers.contains_key("access-control-allow-origin") {
        headers.insert(
            HeaderName::from_static("access-control-allow-origin"),
            HeaderValue::from_str(&origin).unwrap(),
        );
    }
    
    Ok(response)
}

// Anti-CSRF token validation (for state-changing requests)
pub fn generate_csrf_token() -> String {
    use rand::Rng;
    use base64::{Engine as _, engine::general_purpose};
    
    let mut rng = rand::thread_rng();
    let token: [u8; 32] = rng.gen();
    general_purpose::URL_SAFE_NO_PAD.encode(token)
}

pub async fn csrf_middleware(
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Only check for state-changing methods
    let method = request.method();
    if method == "POST" || method == "PUT" || method == "DELETE" || method == "PATCH" {
        // In real implementation, check CSRF token from header or form data
        // For now, we'll skip this check
    }
    
    Ok(next.run(request).await)
}

// IP-based access control
#[derive(Clone)]
pub struct IpFilter {
    whitelist: Vec<IpAddr>,
    blacklist: Vec<IpAddr>,
}

impl IpFilter {
    pub fn new() -> Self {
        Self {
            whitelist: Vec::new(),
            blacklist: Vec::new(),
        }
    }
    
    pub fn is_allowed(&self, ip: IpAddr) -> bool {
        // Check blacklist first
        if self.blacklist.contains(&ip) {
            return false;
        }
        
        // If whitelist is empty, allow all non-blacklisted
        if self.whitelist.is_empty() {
            return true;
        }
        
        // Otherwise, must be in whitelist
        self.whitelist.contains(&ip)
    }
}

// Slowloris attack protection
pub async fn slowloris_protection(
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Set a timeout for receiving the complete request
    // This is typically handled at the server level, but we can add additional checks
    
    // In production, you'd want to track slow clients and disconnect them
    
    Ok(next.run(request).await)
}