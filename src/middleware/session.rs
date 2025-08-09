use axum::{
    body::Body,
    extract::{Request, State},
    http::{header::SET_COOKIE, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::sync::Arc;
use tracing::{debug, error};

use crate::session::{Session, SessionManager, extract_csrf_token, validate_csrf_token};

#[derive(Clone)]
pub struct SessionState {
    pub manager: Arc<SessionManager>,
}

// Session extension for request
#[derive(Clone)]
pub struct SessionData {
    pub session: Option<Session>,
    pub is_new: bool,
}

pub async fn session_middleware(
    State(state): State<SessionState>,
    mut request: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let headers = request.headers();
    
    // Try to load existing session
    let (session, is_new) = if let Some(session_id) = state.manager.extract_session_id(headers) {
        match state.manager.load_session(&session_id, headers).await {
            Ok(Some(session)) => {
                debug!("Loaded session: {}", session_id);
                (Some(session), false)
            }
            Ok(None) => {
                debug!("Session not found or invalid: {}", session_id);
                // Create new session if old one is invalid
                match state.manager.create_session(headers).await {
                    Ok(session) => (Some(session), true),
                    Err(e) => {
                        error!("Failed to create session: {}", e);
                        (None, false)
                    }
                }
            }
            Err(e) => {
                error!("Failed to load session: {}", e);
                (None, false)
            }
        }
    } else {
        // No session cookie, create new session
        match state.manager.create_session(headers).await {
            Ok(session) => {
                debug!("Created new session: {}", session.id);
                (Some(session), true)
            }
            Err(e) => {
                error!("Failed to create session: {}", e);
                (None, false)
            }
        }
    };

    // Add session to request extensions
    request.extensions_mut().insert(SessionData {
        session: session.clone(),
        is_new,
    });

    // Call the next middleware/handler
    let mut response = next.run(request).await;

    // Set session cookie if new or updated
    if let Some(session) = session {
        if is_new {
            let cookie = state.manager.create_cookie(&session.id);
            response.headers_mut().insert(
                SET_COOKIE,
                HeaderValue::from_str(&cookie).unwrap_or_else(|_| HeaderValue::from_static("")),
            );
        }
    }

    Ok(response)
}

// CSRF protection middleware
pub async fn csrf_middleware(
    request: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let method = request.method();
    
    // Only check CSRF for state-changing methods
    if method == "POST" || method == "PUT" || method == "DELETE" || method == "PATCH" {
        let session_data = request.extensions().get::<SessionData>();
        
        if let Some(session_data) = session_data {
            if let Some(ref session) = session_data.session {
                // Extract CSRF token from request
                let provided_token = extract_csrf_token(request.headers());
                
                if let Some(token) = provided_token {
                    if !validate_csrf_token(session, &token) {
                        error!("CSRF token validation failed");
                        return Err(StatusCode::FORBIDDEN);
                    }
                } else {
                    error!("Missing CSRF token");
                    return Err(StatusCode::FORBIDDEN);
                }
            }
        }
    }
    
    Ok(next.run(request).await)
}

// Helper extractors for handlers
use axum::extract::FromRequestParts;
use axum::http::request::Parts;

pub struct SessionExtractor(pub Option<Session>);

#[axum::async_trait]
impl<S> FromRequestParts<S> for SessionExtractor
where
    S: Send + Sync,
{
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let session_data = parts.extensions.get::<SessionData>();
        
        if let Some(session_data) = session_data {
            Ok(SessionExtractor(session_data.session.clone()))
        } else {
            Ok(SessionExtractor(None))
        }
    }
}

pub struct RequireSession(pub Session);

#[axum::async_trait]
impl<S> FromRequestParts<S> for RequireSession
where
    S: Send + Sync,
{
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let session_data = parts.extensions.get::<SessionData>();
        
        if let Some(session_data) = session_data {
            if let Some(session) = &session_data.session {
                return Ok(RequireSession(session.clone()));
            }
        }
        
        Err(StatusCode::UNAUTHORIZED)
    }
}

pub struct RequireAuth {
    pub session: Session,
    pub user_id: String,
}

#[axum::async_trait]
impl<S> FromRequestParts<S> for RequireAuth
where
    S: Send + Sync,
{
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let session_data = parts.extensions.get::<SessionData>();
        
        if let Some(session_data) = session_data {
            if let Some(session) = &session_data.session {
                if let Some(user_id) = &session.user_id {
                    return Ok(RequireAuth {
                        session: session.clone(),
                        user_id: user_id.clone(),
                    });
                }
            }
        }
        
        Err(StatusCode::UNAUTHORIZED)
    }
}