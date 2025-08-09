use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use anyhow::{Result, anyhow};
use tracing::{info, debug};

#[derive(Clone, Debug)]
pub struct RewriteRule {
    pub pattern: String,
    pub replacement: String,
    pub flags: Vec<RewriteFlag>,
    pub regex: Option<Regex>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "UPPERCASE")]
pub enum RewriteFlag {
    L,     // Last - stop processing after this rule
    R,     // Redirect - send HTTP redirect
    P,     // Proxy - proxy the request
    F,     // Forbidden - return 403
    G,     // Gone - return 410
    NC,    // No Case - case insensitive
    QSA,   // Query String Append
    R301,  // Permanent redirect
    R302,  // Temporary redirect
}

#[derive(Clone, Debug)]
pub struct RewriteConfig {
    pub rules: Vec<RewriteRule>,
    pub conditions: Vec<RewriteCondition>,
}

#[derive(Clone, Debug)]
pub struct RewriteCondition {
    pub test_string: String,
    pub pattern: String,
    pub flags: Vec<ConditionFlag>,
    pub regex: Option<Regex>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub enum ConditionFlag {
    NC,    // No Case
    OR,    // OR next condition (default is AND)
}

pub struct RewriteEngine {
    rules: Vec<RewriteRule>,
    conditions: Vec<RewriteCondition>,
}

impl RewriteEngine {
    pub fn new(mut config: RewriteConfig) -> Result<Self> {
        // Compile regex patterns
        for rule in &mut config.rules {
            let pattern = if rule.flags.contains(&RewriteFlag::NC) {
                format!("(?i){}", rule.pattern)
            } else {
                rule.pattern.clone()
            };
            
            rule.regex = Some(Regex::new(&pattern)
                .map_err(|e| anyhow!("Invalid regex pattern '{}': {}", pattern, e))?);
        }
        
        for condition in &mut config.conditions {
            let pattern = if condition.flags.contains(&ConditionFlag::NC) {
                format!("(?i){}", condition.pattern)
            } else {
                condition.pattern.clone()
            };
            
            condition.regex = Some(Regex::new(&pattern)
                .map_err(|e| anyhow!("Invalid condition pattern '{}': {}", pattern, e))?);
        }
        
        Ok(Self {
            rules: config.rules,
            conditions: config.conditions,
        })
    }

    pub fn process_url(&self, url: &str, query_string: Option<&str>, headers: &HashMap<String, String>) -> RewriteResult {
        debug!("Processing URL rewrite for: {}", url);
        
        for rule in &self.rules {
            // Check conditions first
            if !self.check_conditions(url, headers) {
                continue;
            }
            
            if let Some(regex) = &rule.regex {
                if let Some(captures) = regex.captures(url) {
                    let mut replacement = rule.replacement.clone();
                    
                    // Replace backreferences ($1, $2, etc.)
                    for (i, capture) in captures.iter().enumerate() {
                        if let Some(matched) = capture {
                            replacement = replacement.replace(&format!("${}", i), matched.as_str());
                        }
                    }
                    
                    // Handle query string
                    if rule.flags.contains(&RewriteFlag::QSA) && query_string.is_some() {
                        if replacement.contains('?') {
                            replacement.push('&');
                        } else {
                            replacement.push('?');
                        }
                        replacement.push_str(query_string.unwrap());
                    }
                    
                    info!("URL rewritten from '{}' to '{}'", url, replacement);
                    
                    // Process flags
                    if rule.flags.contains(&RewriteFlag::F) {
                        return RewriteResult::Forbidden;
                    }
                    
                    if rule.flags.contains(&RewriteFlag::G) {
                        return RewriteResult::Gone;
                    }
                    
                    if rule.flags.contains(&RewriteFlag::R) || 
                       rule.flags.contains(&RewriteFlag::R301) {
                        return RewriteResult::Redirect { 
                            url: replacement, 
                            permanent: true 
                        };
                    }
                    
                    if rule.flags.contains(&RewriteFlag::R302) {
                        return RewriteResult::Redirect { 
                            url: replacement, 
                            permanent: false 
                        };
                    }
                    
                    if rule.flags.contains(&RewriteFlag::P) {
                        return RewriteResult::Proxy { url: replacement };
                    }
                    
                    // Internal rewrite
                    let result = RewriteResult::Rewrite { url: replacement };
                    
                    // Stop processing if L flag is set
                    if rule.flags.contains(&RewriteFlag::L) {
                        return result;
                    }
                    
                    // Continue processing with rewritten URL
                    return self.process_url(&result.get_url(), query_string, headers);
                }
            }
        }
        
        RewriteResult::NoMatch
    }

    fn check_conditions(&self, url: &str, headers: &HashMap<String, String>) -> bool {
        if self.conditions.is_empty() {
            return true;
        }
        
        let mut result = true;
        let mut use_or = false;
        
        for condition in &self.conditions {
            let test_value = self.expand_variables(&condition.test_string, url, headers);
            
            let matches = if let Some(regex) = &condition.regex {
                regex.is_match(&test_value)
            } else {
                false
            };
            
            if use_or {
                result = result || matches;
                use_or = false;
            } else {
                result = result && matches;
            }
            
            if condition.flags.contains(&ConditionFlag::OR) {
                use_or = true;
            }
        }
        
        result
    }

    fn expand_variables(&self, template: &str, url: &str, headers: &HashMap<String, String>) -> String {
        let mut result = template.to_string();
        
        // Server variables
        result = result.replace("%{REQUEST_URI}", url);
        
        // HTTP headers
        for (key, value) in headers {
            result = result.replace(&format!("%{{HTTP:{}}}", key.to_uppercase()), value);
        }
        
        // Environment variables
        if result.contains("%{ENV:") {
            let env_regex = Regex::new(r"%\{ENV:([^}]+)\}").unwrap();
            result = env_regex.replace_all(&result, |caps: &regex::Captures| {
                std::env::var(&caps[1]).unwrap_or_default()
            }).to_string();
        }
        
        result
    }
}

#[derive(Debug, Clone)]
pub enum RewriteResult {
    NoMatch,
    Rewrite { url: String },
    Redirect { url: String, permanent: bool },
    Proxy { url: String },
    Forbidden,
    Gone,
}

impl RewriteResult {
    pub fn get_url(&self) -> String {
        match self {
            RewriteResult::Rewrite { url } => url.clone(),
            RewriteResult::Redirect { url, .. } => url.clone(),
            RewriteResult::Proxy { url } => url.clone(),
            _ => String::new(),
        }
    }
}

// Common rewrite rules
impl RewriteConfig {
    pub fn new() -> Self {
        Self {
            rules: Vec::new(),
            conditions: Vec::new(),
        }
    }

    // Remove trailing slash
    pub fn add_remove_trailing_slash(&mut self) {
        self.rules.push(RewriteRule {
            pattern: "^(.+)/$".to_string(),
            replacement: "$1".to_string(),
            flags: vec![RewriteFlag::R301, RewriteFlag::L],
            regex: None,
        });
    }

    // Force www
    pub fn add_force_www(&mut self, domain: &str) {
        self.conditions.push(RewriteCondition {
            test_string: "%{HTTP:Host}".to_string(),
            pattern: format!("^{}$", domain.replace(".", r"\.")),
            flags: vec![],
            regex: None,
        });
        
        self.rules.push(RewriteRule {
            pattern: "^(.*)$".to_string(),
            replacement: format!("https://www.{}$1", domain),
            flags: vec![RewriteFlag::R301, RewriteFlag::L],
            regex: None,
        });
    }

    // Force HTTPS
    pub fn add_force_https(&mut self) {
        self.conditions.push(RewriteCondition {
            test_string: "%{HTTP:X-Forwarded-Proto}".to_string(),
            pattern: "^http$".to_string(),
            flags: vec![],
            regex: None,
        });
        
        self.rules.push(RewriteRule {
            pattern: "^(.*)$".to_string(),
            replacement: "https://%{HTTP:Host}$1".to_string(),
            flags: vec![RewriteFlag::R301, RewriteFlag::L],
            regex: None,
        });
    }

    // Clean URLs (remove .html extension)
    pub fn add_clean_urls(&mut self) {
        self.rules.push(RewriteRule {
            pattern: r"^(.+)\.html$".to_string(),
            replacement: "$1".to_string(),
            flags: vec![RewriteFlag::R301, RewriteFlag::L],
            regex: None,
        });
        
        // Internal rewrite to add .html back
        self.rules.push(RewriteRule {
            pattern: r"^([^.]+)$".to_string(),
            replacement: "$1.html".to_string(),
            flags: vec![RewriteFlag::L],
            regex: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_rewrite() {
        let mut config = RewriteConfig::new();
        config.rules.push(RewriteRule {
            pattern: "^/old/(.*)$".to_string(),
            replacement: "/new/$1".to_string(),
            flags: vec![RewriteFlag::L],
            regex: None,
        });
        
        let engine = RewriteEngine::new(config).unwrap();
        let result = engine.process_url("/old/page.html", None, &HashMap::new());
        
        match result {
            RewriteResult::Rewrite { url } => assert_eq!(url, "/new/page.html"),
            _ => panic!("Expected rewrite result"),
        }
    }

    #[test]
    fn test_redirect() {
        let mut config = RewriteConfig::new();
        config.rules.push(RewriteRule {
            pattern: "^/temp$".to_string(),
            replacement: "/permanent".to_string(),
            flags: vec![RewriteFlag::R301],
            regex: None,
        });
        
        let engine = RewriteEngine::new(config).unwrap();
        let result = engine.process_url("/temp", None, &HashMap::new());
        
        match result {
            RewriteResult::Redirect { url, permanent } => {
                assert_eq!(url, "/permanent");
                assert!(permanent);
            }
            _ => panic!("Expected redirect result"),
        }
    }
}