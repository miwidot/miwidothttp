use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::config::CloudflareConfig;

#[derive(Debug, Serialize)]
struct CreateCertRequest {
    hostnames: Vec<String>,
    requested_validity: i32,
    request_type: String,
}

#[derive(Debug, Deserialize)]
struct CertResponse {
    success: bool,
    result: Option<CertResult>,
    errors: Vec<ApiError>,
}

#[derive(Debug, Deserialize)]
struct CertResult {
    id: String,
    certificate: String,
    private_key: String,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    code: i32,
    message: String,
}

pub struct CloudflareClient {
    client: Client,
    api_token: Option<String>,
    api_key: Option<String>,
    email: Option<String>,
    zone_id: Option<String>,
}

impl CloudflareClient {
    pub fn new(config: &CloudflareConfig) -> Result<Self> {
        if config.api_token.is_none() && (config.api_key.is_none() || config.email.is_none()) {
            return Err(anyhow!("Either API token or API key + email must be provided"));
        }

        Ok(Self {
            client: Client::new(),
            api_token: config.api_token.clone(),
            api_key: config.api_key.clone(),
            email: config.email.clone(),
            zone_id: config.zone_id.clone(),
        })
    }

    pub async fn get_or_create_origin_cert(&self, domains: &[String]) -> Result<(String, String)> {
        info!("Creating origin certificate for domains: {:?}", domains);
        
        let request = CreateCertRequest {
            hostnames: domains.to_vec(),
            requested_validity: 5475, // 15 years in days
            request_type: "origin-rsa".to_string(),
        };

        let mut req = self.client
            .post("https://api.cloudflare.com/client/v4/certificates")
            .json(&request);

        if let Some(token) = &self.api_token {
            req = req.header("Authorization", format!("Bearer {}", token));
        } else if let (Some(key), Some(email)) = (&self.api_key, &self.email) {
            req = req
                .header("X-Auth-Key", key)
                .header("X-Auth-Email", email);
        }

        let response = req.send().await?;
        let status = response.status();
        let body = response.text().await?;

        debug!("Cloudflare API response: {}", body);

        if !status.is_success() {
            return Err(anyhow!("Cloudflare API error: {} - {}", status, body));
        }

        let cert_response: CertResponse = serde_json::from_str(&body)?;
        
        if !cert_response.success {
            let errors = cert_response.errors
                .iter()
                .map(|e| format!("{}: {}", e.code, e.message))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(anyhow!("Failed to create certificate: {}", errors));
        }

        let result = cert_response.result
            .ok_or_else(|| anyhow!("No certificate in response"))?;

        info!("Successfully created origin certificate with ID: {}", result.id);
        
        Ok((result.certificate, result.private_key))
    }

    pub async fn revoke_certificate(&self, cert_id: &str) -> Result<()> {
        let url = format!("https://api.cloudflare.com/client/v4/certificates/{}", cert_id);
        
        let mut req = self.client.delete(&url);

        if let Some(token) = &self.api_token {
            req = req.header("Authorization", format!("Bearer {}", token));
        } else if let (Some(key), Some(email)) = (&self.api_key, &self.email) {
            req = req
                .header("X-Auth-Key", key)
                .header("X-Auth-Email", email);
        }

        let response = req.send().await?;
        
        if !response.status().is_success() {
            return Err(anyhow!("Failed to revoke certificate: {}", response.status()));
        }

        info!("Successfully revoked certificate: {}", cert_id);
        Ok(())
    }
}