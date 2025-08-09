use anyhow::Result;
use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderMap, StatusCode, Uri},
    response::{Html, IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{error, warn, debug};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorConfig {
    pub mode: ErrorMode,
    pub custom_pages: HashMap<u16, String>,
    pub templates_dir: Option<PathBuf>,
    pub show_details: bool,
    pub log_errors: bool,
    pub notify_errors: Option<NotificationConfig>,
    pub rate_limit_errors: bool,
    pub error_tracking: Option<ErrorTrackingConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ErrorMode {
    Development, // Show full stack traces
    Production,  // Show user-friendly messages
    Maintenance, // Show maintenance page
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationConfig {
    pub email: Option<String>,
    pub webhook: Option<String>,
    pub threshold: u32, // Number of errors before notification
    pub interval: u64,  // Seconds between notifications
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorTrackingConfig {
    pub sentry_dsn: Option<String>,
    pub datadog_api_key: Option<String>,
    pub custom_endpoint: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AppError {
    pub id: String,
    pub status: StatusCode,
    pub code: Option<String>,
    pub message: String,
    pub details: Option<String>,
    pub source: Option<Box<dyn std::error::Error + Send + Sync>>,
    pub context: HashMap<String, String>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl AppError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            status,
            code: None,
            message: message.into(),
            details: None,
            source: None,
            context: HashMap::new(),
            timestamp: chrono::Utc::now(),
        }
    }

    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }

    pub fn with_source(mut self, source: Box<dyn std::error::Error + Send + Sync>) -> Self {
        self.source = Some(source);
        self
    }

    pub fn with_context(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.context.insert(key.into(), value.into());
        self
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}: {}", self.id, self.status, self.message)
    }
}

impl std::error::Error for AppError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
    }
}

// Common errors
impl AppError {
    pub fn not_found(resource: &str) -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            format!("Resource not found: {}", resource),
        )
        .with_code("RESOURCE_NOT_FOUND")
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
            .with_code("BAD_REQUEST")
    }

    pub fn unauthorized() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "Authentication required")
            .with_code("UNAUTHORIZED")
    }

    pub fn forbidden() -> Self {
        Self::new(StatusCode::FORBIDDEN, "Access denied")
            .with_code("FORBIDDEN")
    }

    pub fn internal_server_error() -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
            .with_code("INTERNAL_ERROR")
    }

    pub fn service_unavailable() -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, "Service temporarily unavailable")
            .with_code("SERVICE_UNAVAILABLE")
    }

    pub fn rate_limit_exceeded() -> Self {
        Self::new(StatusCode::TOO_MANY_REQUESTS, "Rate limit exceeded")
            .with_code("RATE_LIMIT_EXCEEDED")
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorResponse {
    pub error: ErrorDetails,
    pub request_id: String,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub debug: Option<DebugInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorDetails {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DebugInfo {
    pub stack_trace: Option<Vec<String>>,
    pub context: HashMap<String, String>,
    pub source: Option<String>,
}

pub struct ErrorHandler {
    config: ErrorConfig,
    templates: Arc<HashMap<u16, String>>,
    error_counts: Arc<tokio::sync::RwLock<HashMap<String, u32>>>,
}

impl ErrorHandler {
    pub async fn new(config: ErrorConfig) -> Result<Self> {
        let mut templates = HashMap::new();
        
        // Load custom error page templates
        if let Some(dir) = &config.templates_dir {
            for (status_code, template_file) in &config.custom_pages {
                let path = dir.join(template_file);
                if path.exists() {
                    let content = tokio::fs::read_to_string(&path).await?;
                    templates.insert(*status_code, content);
                }
            }
        }

        // Add default templates if not provided
        templates.entry(404).or_insert_with(|| DEFAULT_404_PAGE.to_string());
        templates.entry(500).or_insert_with(|| DEFAULT_500_PAGE.to_string());
        templates.entry(503).or_insert_with(|| DEFAULT_503_PAGE.to_string());

        Ok(Self {
            config,
            templates: Arc::new(templates),
            error_counts: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        })
    }

    pub async fn handle_error(&self, error: AppError, headers: &HeaderMap) -> Response {
        // Log the error
        if self.config.log_errors {
            match error.status.as_u16() {
                500..=599 => error!("Server error: {}", error),
                400..=499 => warn!("Client error: {}", error),
                _ => debug!("Error: {}", error),
            }
        }

        // Track error for notifications
        self.track_error(&error).await;

        // Determine response format based on Accept header
        let accept = headers
            .get(header::ACCEPT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("text/html");

        if accept.contains("application/json") {
            self.json_error_response(error)
        } else {
            self.html_error_response(error).await
        }
    }

    fn json_error_response(&self, error: AppError) -> Response {
        let debug_info = if self.config.mode == ErrorMode::Development && self.config.show_details {
            Some(DebugInfo {
                stack_trace: None, // Could capture actual stack trace
                context: error.context.clone(),
                source: error.source.as_ref().map(|e| e.to_string()),
            })
        } else {
            None
        };

        let response = ErrorResponse {
            error: ErrorDetails {
                code: error.code.unwrap_or_else(|| error.status.as_u16().to_string()),
                message: if self.config.mode == ErrorMode::Production {
                    self.get_user_friendly_message(error.status)
                } else {
                    error.message.clone()
                },
                details: if self.config.show_details {
                    error.details
                } else {
                    None
                },
            },
            request_id: error.id,
            timestamp: error.timestamp.to_rfc3339(),
            debug: debug_info,
        };

        (error.status, Json(response)).into_response()
    }

    async fn html_error_response(&self, error: AppError) -> Response {
        let status_code = error.status.as_u16();
        
        // Check for custom template
        if let Some(template) = self.templates.get(&status_code) {
            let html = self.render_template(template, &error);
            return (error.status, Html(html)).into_response();
        }

        // Fallback to generic error page
        let html = self.render_generic_error(&error);
        (error.status, Html(html)).into_response()
    }

    fn render_template(&self, template: &str, error: &AppError) -> String {
        template
            .replace("{{status}}", &error.status.as_u16().to_string())
            .replace("{{status_text}}", error.status.canonical_reason().unwrap_or("Error"))
            .replace("{{message}}", &error.message)
            .replace("{{details}}", error.details.as_deref().unwrap_or(""))
            .replace("{{error_id}}", &error.id)
            .replace("{{timestamp}}", &error.timestamp.to_rfc3339())
    }

    fn render_generic_error(&self, error: &AppError) -> String {
        format!(
            r#"<!DOCTYPE html>
<html>
<head>
    <title>Error {}</title>
    <style>
        body {{ font-family: system-ui, sans-serif; margin: 0; padding: 20px; background: #f5f5f5; }}
        .container {{ max-width: 600px; margin: 100px auto; background: white; padding: 40px; border-radius: 8px; box-shadow: 0 2px 10px rgba(0,0,0,0.1); }}
        h1 {{ color: #e74c3c; margin: 0 0 20px; }}
        .error-code {{ font-size: 72px; font-weight: bold; color: #e74c3c; }}
        .error-message {{ color: #555; margin: 20px 0; }}
        .error-id {{ color: #999; font-size: 12px; margin-top: 30px; }}
        .back-link {{ display: inline-block; margin-top: 20px; color: #3498db; text-decoration: none; }}
        .debug {{ background: #f8f8f8; padding: 15px; border-radius: 4px; margin-top: 20px; font-family: monospace; font-size: 12px; }}
    </style>
</head>
<body>
    <div class="container">
        <div class="error-code">{}</div>
        <h1>{}</h1>
        <div class="error-message">{}</div>
        {}
        <a href="/" class="back-link">← Back to Home</a>
        <div class="error-id">Error ID: {}</div>
    </div>
</body>
</html>"#,
            error.status.as_u16(),
            error.status.as_u16(),
            error.status.canonical_reason().unwrap_or("Error"),
            if self.config.mode == ErrorMode::Production {
                self.get_user_friendly_message(error.status)
            } else {
                &error.message
            },
            if self.config.mode == ErrorMode::Development && error.details.is_some() {
                format!(r#"<div class="debug">Debug: {}</div>"#, error.details.as_ref().unwrap())
            } else {
                String::new()
            },
            error.id
        )
    }

    fn get_user_friendly_message(&self, status: StatusCode) -> String {
        match status {
            StatusCode::NOT_FOUND => "The page you're looking for doesn't exist.".to_string(),
            StatusCode::UNAUTHORIZED => "Please sign in to continue.".to_string(),
            StatusCode::FORBIDDEN => "You don't have permission to access this resource.".to_string(),
            StatusCode::INTERNAL_SERVER_ERROR => "Something went wrong on our end. Please try again later.".to_string(),
            StatusCode::SERVICE_UNAVAILABLE => "Our service is temporarily unavailable. Please check back soon.".to_string(),
            StatusCode::BAD_REQUEST => "There was a problem with your request.".to_string(),
            StatusCode::TOO_MANY_REQUESTS => "You've made too many requests. Please slow down.".to_string(),
            _ => format!("An error occurred ({})", status.as_u16()),
        }
    }

    async fn track_error(&self, error: &AppError) {
        let mut counts = self.error_counts.write().await;
        let count = counts.entry(error.status.as_u16().to_string()).or_insert(0);
        *count += 1;

        // Check if we should send notifications
        if let Some(notify) = &self.config.notify_errors {
            if *count >= notify.threshold {
                self.send_error_notification(error, *count).await;
                *count = 0; // Reset counter after notification
            }
        }

        // Send to error tracking service
        if let Some(tracking) = &self.config.error_tracking {
            self.send_to_tracking_service(error, tracking).await;
        }
    }

    async fn send_error_notification(&self, error: &AppError, count: u32) {
        debug!("Sending error notification for {} errors", count);
        // Implementation would send email/webhook
    }

    async fn send_to_tracking_service(&self, error: &AppError, config: &ErrorTrackingConfig) {
        debug!("Sending error to tracking service: {}", error.id);
        // Implementation would send to Sentry/Datadog/etc
    }
}

// Default error pages
const DEFAULT_404_PAGE: &str = r#"<!DOCTYPE html>
<html>
<head>
    <title>404 - Page Not Found</title>
    <style>
        body { font-family: system-ui; display: flex; align-items: center; justify-content: center; height: 100vh; margin: 0; background: linear-gradient(135deg, #667eea 0%, #764ba2 100%); }
        .container { text-align: center; color: white; }
        h1 { font-size: 120px; margin: 0; }
        p { font-size: 24px; margin: 20px 0; }
        a { color: white; text-decoration: none; border: 2px solid white; padding: 10px 20px; border-radius: 25px; display: inline-block; margin-top: 20px; }
    </style>
</head>
<body>
    <div class="container">
        <h1>404</h1>
        <p>Oops! Page not found</p>
        <a href="/">Go Home</a>
    </div>
</body>
</html>"#;

const DEFAULT_500_PAGE: &str = r#"<!DOCTYPE html>
<html>
<head>
    <title>500 - Server Error</title>
    <style>
        body { font-family: system-ui; display: flex; align-items: center; justify-content: center; height: 100vh; margin: 0; background: linear-gradient(135deg, #f093fb 0%, #f5576c 100%); }
        .container { text-align: center; color: white; }
        h1 { font-size: 120px; margin: 0; }
        p { font-size: 24px; margin: 20px 0; }
    </style>
</head>
<body>
    <div class="container">
        <h1>500</h1>
        <p>Something went wrong</p>
        <p style="font-size: 16px;">We're working on fixing this</p>
    </div>
</body>
</html>"#;

const DEFAULT_503_PAGE: &str = r#"<!DOCTYPE html>
<html>
<head>
    <title>503 - Maintenance</title>
    <style>
        body { font-family: system-ui; display: flex; align-items: center; justify-content: center; height: 100vh; margin: 0; background: linear-gradient(135deg, #fa709a 0%, #fee140 100%); }
        .container { text-align: center; color: white; }
        h1 { font-size: 60px; margin: 0; }
        p { font-size: 24px; margin: 20px 0; }
    </style>
</head>
<body>
    <div class="container">
        <h1>Under Maintenance</h1>
        <p>We'll be back shortly</p>
    </div>
</body>
</html>"#;

// Error recovery middleware
use axum::middleware::Next;
use axum::extract::Request;

pub async fn error_recovery_middleware(
    State(handler): State<Arc<ErrorHandler>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let headers = request.headers().clone();
    let uri = request.uri().clone();
    let method = request.method().clone();
    
    let response = next.run(request).await;
    
    // Check if response is an error
    if response.status().is_server_error() || response.status().is_client_error() {
        let error = AppError::new(response.status(), "Request failed")
            .with_context("uri", uri.to_string())
            .with_context("method", method.to_string());
        
        return handler.handle_error(error, &headers).await;
    }
    
    response
}

// Panic handler
pub fn setup_panic_handler() {
    std::panic::set_hook(Box::new(|panic_info| {
        let msg = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic".to_string()
        };
        
        let location = if let Some(location) = panic_info.location() {
            format!("{}:{}:{}", location.file(), location.line(), location.column())
        } else {
            "Unknown location".to_string()
        };
        
        error!("PANIC at {}: {}", location, msg);
    }));
}

// Helper trait for converting errors to responses
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status;
        let body = Json(serde_json::json!({
            "error": {
                "id": self.id,
                "code": self.code,
                "message": self.message,
                "details": self.details,
            }
        }));
        
        (status, body).into_response()
    }
}

// From implementations for common error types
impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::internal_server_error()
            .with_details(err.to_string())
            .with_source(Box::new(err))
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        AppError::bad_request("Invalid JSON")
            .with_details(err.to_string())
            .with_source(Box::new(err))
    }
}

impl From<anyhow::Error> for AppError {
    fn from(err: anyhow::Error) -> Self {
        AppError::internal_server_error()
            .with_details(err.to_string())
    }
}