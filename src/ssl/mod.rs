use anyhow::{anyhow, Result};
use axum_server::tls_rustls::RustlsConfig;
use rustls::ServerConfig;
use rustls::server::{ResolvesServerCert, ClientHello};
use rustls::sign::CertifiedKey;
use rustls_pemfile::{certs, pkcs8_private_keys};
use std::collections::HashMap;
use std::fs;
use std::io::BufReader;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn, debug};

use crate::config::Config;
use crate::vhost::VHostManager;

mod cloudflare;
use cloudflare::CloudflareClient;

pub struct SslManager {
    config: Config,
    tls_config: Arc<RwLock<Option<RustlsConfig>>>,
    cloudflare_client: Option<CloudflareClient>,
}

impl SslManager {
    pub fn new(config: Config) -> Self {
        let cloudflare_client = if config.ssl.auto_cert {
            CloudflareClient::new(&config.cloudflare).ok()
        } else {
            None
        };

        Self {
            config,
            tls_config: Arc::new(RwLock::new(None)),
            cloudflare_client,
        }
    }

    pub async fn get_tls_config(&self) -> Result<RustlsConfig> {
        let guard = self.tls_config.read().await;
        if let Some(config) = guard.as_ref() {
            return Ok(config.clone());
        }
        drop(guard);

        self.load_or_create_tls_config().await
    }

    async fn load_or_create_tls_config(&self) -> Result<RustlsConfig> {
        let tls_config = if self.config.ssl.auto_cert {
            info!("Auto-generating SSL certificate via Cloudflare");
            self.create_cloudflare_cert().await?
        } else if let (Some(cert_path), Some(key_path)) = 
            (&self.config.ssl.cert_path, &self.config.ssl.key_path) {
            info!("Loading SSL certificate from disk");
            self.load_cert_from_files(cert_path, key_path).await?
        } else {
            return Err(anyhow!("No SSL certificate configuration provided"));
        };

        let mut guard = self.tls_config.write().await;
        *guard = Some(tls_config.clone());

        Ok(tls_config)
    }

    async fn create_cloudflare_cert(&self) -> Result<RustlsConfig> {
        let client = self.cloudflare_client.as_ref()
            .ok_or_else(|| anyhow!("Cloudflare client not configured"))?;

        let (cert_pem, key_pem) = client.get_or_create_origin_cert(&self.config.ssl.domains).await?;

        // Save to temporary files for RustlsConfig
        let cert_path = "/tmp/cert.pem";
        let key_path = "/tmp/key.pem";
        
        fs::write(cert_path, cert_pem)?;
        fs::write(key_path, key_pem)?;

        let config = RustlsConfig::from_pem_file(cert_path, key_path).await?;

        Ok(config)
    }

    async fn load_cert_from_files(&self, cert_path: &str, key_path: &str) -> Result<RustlsConfig> {
        // RustlsConfig handles file loading internally
        let config = RustlsConfig::from_pem_file(cert_path, key_path)
            .await
            .map_err(|e| anyhow!("Failed to load certificates: {}", e))?;

        Ok(config)
    }

    pub async fn refresh_certificate(&self) -> Result<()> {
        if self.config.ssl.auto_cert {
            info!("Refreshing SSL certificate from Cloudflare");
            let new_config = self.create_cloudflare_cert().await?;
            let mut guard = self.tls_config.write().await;
            *guard = Some(new_config);
            info!("SSL certificate refreshed successfully");
        } else {
            warn!("Certificate refresh requested but auto_cert is disabled");
        }
        Ok(())
    }
}