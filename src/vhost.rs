use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use regex::Regex;
use tracing::{debug, info, warn};

use crate::rewrite::{RewriteRule, RewriteEngine};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VirtualHost {
    pub domains: Vec<String>,
    pub priority: i32,
    pub ssl: Option<VHostSSL>,
    pub root: Option<PathBuf>,
    pub backend: Option<VHostBackend>,
    pub logging: Option<VHostLogging>,
    pub limits: Option<VHostLimits>,
    pub headers: Option<HashMap<String, String>>,
    pub error_pages: Option<HashMap<u16, String>>,
    pub redirects: Option<Vec<Redirect>>,
    pub rewrites: Option<Vec<RewriteRule>>,
    pub access_control: Option<AccessControl>,
    #[serde(skip)]
    pub rewrite_engine: Option<Arc<RewriteEngine>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VHostSSL {
    pub cert_path: Option<String>,
    pub key_path: Option<String>,
    pub client_auth: Option<ClientAuth>,
    pub protocols: Option<Vec<String>>,
    pub ciphers: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VHostBackend {
    pub urls: Vec<String>,
    pub strategy: LoadBalanceStrategy,
    pub health_check: Option<String>,
    pub timeout: Option<u64>,
    pub retry: Option<RetryConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VHostLogging {
    pub access_log: Option<String>,
    pub error_log: Option<String>,
    pub format: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VHostLimits {
    pub rate_limit: Option<u32>,
    pub max_connections: Option<u32>,
    pub max_request_size: Option<String>,
    pub timeout: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Redirect {
    pub from: String,
    pub to: String,
    pub status: u16,
    pub permanent: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AccessControl {
    pub allow: Option<Vec<String>>,
    pub deny: Option<Vec<String>>,
    pub auth: Option<AuthConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuthConfig {
    pub auth_type: AuthType,
    pub realm: String,
    pub users: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthType {
    Basic,
    Bearer,
    Digest,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LoadBalanceStrategy {
    RoundRobin,
    LeastConn,
    IpHash,
    Random,
    Weighted,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RetryConfig {
    pub attempts: u32,
    pub delay_ms: u64,
    pub backoff: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ClientAuth {
    None,
    Optional,
    Required,
}

pub struct VHostManager {
    vhosts: Vec<Arc<VirtualHost>>,
    domain_map: HashMap<String, Arc<VirtualHost>>,
    wildcard_patterns: Vec<(Regex, Arc<VirtualHost>)>,
    default_vhost: Option<Arc<VirtualHost>>,
}

impl VHostManager {
    pub fn new(vhosts: Vec<VirtualHost>) -> Result<Self> {
        let mut manager = VHostManager {
            vhosts: Vec::new(),
            domain_map: HashMap::new(),
            wildcard_patterns: Vec::new(),
            default_vhost: None,
        };

        // Sort vhosts by priority (higher priority first)
        let mut sorted_vhosts = vhosts;
        sorted_vhosts.sort_by(|a, b| b.priority.cmp(&a.priority));

        for vhost in sorted_vhosts {
            manager.add_vhost(vhost)?;
        }

        Ok(manager)
    }

    fn add_vhost(&mut self, vhost: VirtualHost) -> Result<()> {
        let vhost_arc = Arc::new(vhost.clone());
        
        for domain in &vhost.domains {
            if domain == "_" || domain == "default" {
                // Default vhost
                if self.default_vhost.is_none() {
                    info!("Setting default vhost");
                    self.default_vhost = Some(vhost_arc.clone());
                }
            } else if domain.contains('*') {
                // Wildcard domain
                let pattern = self.domain_to_regex(domain)?;
                self.wildcard_patterns.push((pattern, vhost_arc.clone()));
                info!("Added wildcard vhost: {}", domain);
            } else {
                // Exact domain match
                self.domain_map.insert(domain.clone(), vhost_arc.clone());
                info!("Added vhost: {}", domain);
            }
        }
        
        self.vhosts.push(vhost_arc);
        Ok(())
    }

    fn domain_to_regex(&self, domain: &str) -> Result<Regex> {
        // Convert wildcard domain to regex
        // *.example.com -> ^[^.]+\.example\.com$
        // *.*.example.com -> ^[^.]+\.[^.]+\.example\.com$
        let escaped = regex::escape(domain);
        let pattern = escaped.replace("\\*", "[^.]+");
        let full_pattern = format!("^{}$", pattern);
        
        Regex::new(&full_pattern)
            .map_err(|e| anyhow!("Invalid domain pattern {}: {}", domain, e))
    }

    pub fn get_vhost(&self, hostname: &str) -> Option<Arc<VirtualHost>> {
        // 1. Try exact match
        if let Some(vhost) = self.domain_map.get(hostname) {
            debug!("Found exact vhost match for {}", hostname);
            return Some(vhost.clone());
        }

        // 2. Try wildcard patterns (in priority order)
        for (pattern, vhost) in &self.wildcard_patterns {
            if pattern.is_match(hostname) {
                debug!("Found wildcard vhost match for {}", hostname);
                return Some(vhost.clone());
            }
        }

        // 3. Return default vhost if configured
        if let Some(ref default) = self.default_vhost {
            debug!("Using default vhost for {}", hostname);
            return Some(default.clone());
        }

        warn!("No vhost found for {}", hostname);
        None
    }

    pub fn get_ssl_config(&self, hostname: &str) -> Option<VHostSSL> {
        self.get_vhost(hostname)
            .and_then(|vhost| vhost.ssl.clone())
    }

    pub fn get_backend_urls(&self, hostname: &str) -> Option<Vec<String>> {
        self.get_vhost(hostname)
            .and_then(|vhost| vhost.backend.as_ref())
            .map(|backend| backend.urls.clone())
    }

    pub fn get_rate_limit(&self, hostname: &str) -> Option<u32> {
        self.get_vhost(hostname)
            .and_then(|vhost| vhost.limits.as_ref())
            .and_then(|limits| limits.rate_limit)
    }

    pub fn check_access(&self, hostname: &str, client_ip: &str) -> bool {
        let vhost = match self.get_vhost(hostname) {
            Some(v) => v,
            None => return false,
        };

        if let Some(ref access) = vhost.access_control {
            // Check deny list first
            if let Some(ref deny_list) = access.deny {
                for pattern in deny_list {
                    if self.matches_ip_pattern(client_ip, pattern) {
                        return false;
                    }
                }
            }

            // Check allow list
            if let Some(ref allow_list) = access.allow {
                for pattern in allow_list {
                    if self.matches_ip_pattern(client_ip, pattern) {
                        return true;
                    }
                }
                // If allow list exists but IP doesn't match, deny
                return false;
            }
        }

        // No access control configured, allow by default
        true
    }

    fn matches_ip_pattern(&self, ip: &str, pattern: &str) -> bool {
        // Simple IP pattern matching
        // Supports: exact IP, CIDR notation, wildcards
        if pattern == "*" {
            return true;
        }
        
        if pattern.contains('/') {
            // CIDR notation - simplified check
            // TODO: Implement proper CIDR matching
            return ip.starts_with(&pattern.split('/').next().unwrap_or(""));
        }
        
        if pattern.contains('*') {
            // Wildcard matching
            let regex_pattern = pattern.replace('.', r"\.").replace('*', r"\d+");
            if let Ok(re) = Regex::new(&format!("^{}$", regex_pattern)) {
                return re.is_match(ip);
            }
        }
        
        // Exact match
        ip == pattern
    }

    pub fn get_error_page(&self, hostname: &str, status_code: u16) -> Option<String> {
        self.get_vhost(hostname)
            .and_then(|vhost| vhost.error_pages.as_ref())
            .and_then(|pages| pages.get(&status_code))
            .cloned()
    }

    pub fn get_custom_headers(&self, hostname: &str) -> Option<HashMap<String, String>> {
        self.get_vhost(hostname)
            .and_then(|vhost| vhost.headers.clone())
    }

    pub fn find_redirect(&self, hostname: &str, path: &str) -> Option<Redirect> {
        self.get_vhost(hostname)
            .and_then(|vhost| vhost.redirects.as_ref())
            .and_then(|redirects| {
                redirects.iter()
                    .find(|r| path.starts_with(&r.from))
                    .cloned()
            })
    }

    pub fn list_vhosts(&self) -> Vec<String> {
        self.vhosts.iter()
            .flat_map(|v| v.domains.clone())
            .collect()
    }

    pub fn get_vhost_count(&self) -> usize {
        self.vhosts.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_domain_match() {
        let vhost = VirtualHost {
            domains: vec!["example.com".to_string()],
            priority: 100,
            ssl: None,
            root: None,
            backend: None,
            logging: None,
            limits: None,
            headers: None,
            error_pages: None,
            redirects: None,
            access_control: None,
        };

        let manager = VHostManager::new(vec![vhost]).unwrap();
        assert!(manager.get_vhost("example.com").is_some());
        assert!(manager.get_vhost("other.com").is_none());
    }

    #[test]
    fn test_wildcard_domain_match() {
        let vhost = VirtualHost {
            domains: vec!["*.example.com".to_string()],
            priority: 100,
            ssl: None,
            root: None,
            backend: None,
            logging: None,
            limits: None,
            headers: None,
            error_pages: None,
            redirects: None,
            access_control: None,
        };

        let manager = VHostManager::new(vec![vhost]).unwrap();
        assert!(manager.get_vhost("sub.example.com").is_some());
        assert!(manager.get_vhost("another.example.com").is_some());
        assert!(manager.get_vhost("example.com").is_none());
    }

    #[test]
    fn test_priority_ordering() {
        let vhost1 = VirtualHost {
            domains: vec!["*.example.com".to_string()],
            priority: 50,
            ssl: None,
            root: None,
            backend: None,
            logging: None,
            limits: None,
            headers: None,
            error_pages: None,
            redirects: None,
            access_control: None,
        };

        let vhost2 = VirtualHost {
            domains: vec!["specific.example.com".to_string()],
            priority: 100,
            ssl: None,
            root: None,
            backend: None,
            logging: None,
            limits: None,
            headers: None,
            error_pages: None,
            redirects: None,
            access_control: None,
        };

        let manager = VHostManager::new(vec![vhost1, vhost2]).unwrap();
        
        // Exact match should win despite lower priority wildcard
        assert!(manager.get_vhost("specific.example.com").is_some());
    }
}