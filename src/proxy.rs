use anyhow::Result;
use axum::{
    body::Body,
    extract::Request,
    http::{HeaderValue, StatusCode, Uri},
    response::Response,
};
use std::time::Duration;
use tracing::{debug, error};

use crate::config::BackendConfig;

pub struct ProxyManager {
    client: reqwest::Client,
}

impl ProxyManager {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .pool_idle_timeout(Duration::from_secs(90))
            .pool_max_idle_per_host(32)
            .build()
            .expect("Failed to build HTTP client");

        Self { client }
    }

    pub async fn proxy_request(
        &self,
        backend: &BackendConfig,
        mut req: Request<Body>,
    ) -> Result<Response> {
        let uri = req.uri();
        let path_and_query = uri
            .path_and_query()
            .map(|pq| pq.as_str())
            .unwrap_or("/");

        let backend_url = format!("{}{}", backend.url, path_and_query);
        debug!("Proxying to: {}", backend_url);

        *req.uri_mut() = backend_url.parse::<Uri>()?;

        req.headers_mut().insert(
            "X-Forwarded-For",
            HeaderValue::from_static("127.0.0.1"),
        );
        req.headers_mut().insert(
            "X-Real-IP",
            HeaderValue::from_static("127.0.0.1"),
        );
        req.headers_mut().insert(
            "X-Forwarded-Proto",
            HeaderValue::from_static("http"),
        );

        let method = req.method().clone();
        let headers = req.headers().clone();
        let body_bytes = axum::body::to_bytes(req.into_body(), usize::MAX).await?;

        let mut proxy_req = self.client
            .request(method, &backend_url)
            .body(body_bytes.to_vec());

        for (key, value) in headers.iter() {
            if key != "host" && key != "content-length" {
                proxy_req = proxy_req.header(key, value);
            }
        }

        match proxy_req.send().await {
            Ok(res) => {
                let status = res.status();
                let headers = res.headers().clone();
                let body = res.bytes().await?;

                let mut response = Response::builder().status(status);
                
                for (key, value) in headers.iter() {
                    response = response.header(key, value);
                }

                Ok(response.body(Body::from(body))?)
            }
            Err(e) => {
                error!("Proxy request failed: {}", e);
                Ok(Response::builder()
                    .status(StatusCode::BAD_GATEWAY)
                    .body(Body::from(format!("Proxy error: {}", e)))?)
            }
        }
    }

    pub async fn health_check(&self, backend: &BackendConfig) -> bool {
        if let Some(health_path) = &backend.health_check {
            let url = format!("{}{}", backend.url, health_path);
            match self.client.get(&url).send().await {
                Ok(res) => res.status().is_success(),
                Err(e) => {
                    error!("Health check failed for {}: {}", backend.url, e);
                    false
                }
            }
        } else {
            true
        }
    }
}