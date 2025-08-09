use anyhow::{anyhow, Result};
use regex::{Captures, Regex};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, trace};
use axum::http::{StatusCode, Uri, HeaderMap, Method};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RewriteRule {
    pub pattern: String,
    pub replacement: String,
    pub flags: Option<Vec<RewriteFlag>>,
    pub conditions: Option<Vec<RewriteCondition>>,
    #[serde(skip)]
    pub regex: Option<Regex>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RewriteFlag {
    Last,       // Stop processing after this rule (like nginx 'last')
    Break,      // Stop processing in current location (like nginx 'break')
    Redirect,   // 302 temporary redirect
    Permanent,  // 301 permanent redirect
    Proxy,      // Proxy pass to backend
    Cookie,     // Set cookie
    Forbidden,  // Return 403
    Gone,       // Return 410
    NoCase,     // Case-insensitive matching
    QSAppend,   // Append query string
    QSDiscard,  // Discard original query string
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RewriteCondition {
    pub test_string: String,
    pub pattern: String,
    pub flags: Option<Vec<ConditionFlag>>,
    #[serde(skip)]
    pub regex: Option<Regex>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ConditionFlag {
    NoCase,     // Case-insensitive
    Or,         // OR with next condition (default is AND)
    Not,        // Negate the match
    File,       // Check if file exists
    Dir,        // Check if directory exists
    Symlink,    // Check if symlink
    Size,       // Check if file has size > 0
    Exec,       // Check if file is executable
}

#[derive(Debug, Clone)]
pub struct RewriteContext {
    pub uri: Uri,
    pub method: Method,
    pub headers: HeaderMap,
    pub remote_addr: String,
    pub server_name: String,
    pub variables: HashMap<String, String>,
}

pub struct RewriteEngine {
    rules: Vec<Arc<RewriteRule>>,
}

impl RewriteEngine {
    pub fn new(rules: Vec<RewriteRule>) -> Result<Self> {
        let mut compiled_rules = Vec::new();
        
        for mut rule in rules {
            // Compile regex patterns
            let flags = if rule.flags.as_ref()
                .map(|f| f.contains(&RewriteFlag::NoCase))
                .unwrap_or(false) {
                "(?i)"
            } else {
                ""
            };
            
            let pattern = format!("{}{}", flags, rule.pattern);
            rule.regex = Some(Regex::new(&pattern)?);
            
            // Compile condition regexes
            if let Some(ref mut conditions) = rule.conditions {
                for condition in conditions {
                    let cond_flags = if condition.flags.as_ref()
                        .map(|f| f.contains(&ConditionFlag::NoCase))
                        .unwrap_or(false) {
                        "(?i)"
                    } else {
                        ""
                    };
                    
                    let cond_pattern = format!("{}{}", cond_flags, condition.pattern);
                    condition.regex = Some(Regex::new(&cond_pattern)?);
                }
            }
            
            compiled_rules.push(Arc::new(rule));
        }
        
        Ok(RewriteEngine {
            rules: compiled_rules,
        })
    }

    pub fn process(&self, context: &mut RewriteContext) -> Result<Option<RewriteAction>> {
        let original_uri = context.uri.to_string();
        trace!("Processing rewrites for: {}", original_uri);
        
        for rule in &self.rules {
            // Check conditions first
            if !self.check_conditions(rule, context)? {
                continue;
            }
            
            // Apply the rewrite rule
            if let Some(regex) = &rule.regex {
                if let Some(captures) = regex.captures(&original_uri) {
                    debug!("Rewrite rule matched: {} -> {}", rule.pattern, rule.replacement);
                    
                    let new_uri = self.apply_replacement(&rule.replacement, &captures, context);
                    
                    // Handle flags
                    if let Some(flags) = &rule.flags {
                        if flags.contains(&RewriteFlag::Redirect) {
                            return Ok(Some(RewriteAction::Redirect {
                                location: new_uri,
                                permanent: false,
                            }));
                        }
                        
                        if flags.contains(&RewriteFlag::Permanent) {
                            return Ok(Some(RewriteAction::Redirect {
                                location: new_uri,
                                permanent: true,
                            }));
                        }
                        
                        if flags.contains(&RewriteFlag::Forbidden) {
                            return Ok(Some(RewriteAction::Forbidden));
                        }
                        
                        if flags.contains(&RewriteFlag::Gone) {
                            return Ok(Some(RewriteAction::Gone));
                        }
                        
                        if flags.contains(&RewriteFlag::Proxy) {
                            return Ok(Some(RewriteAction::Proxy {
                                backend: new_uri,
                            }));
                        }
                        
                        // Internal rewrite
                        context.uri = new_uri.parse()?;
                        
                        if flags.contains(&RewriteFlag::Last) {
                            return Ok(Some(RewriteAction::Internal {
                                uri: context.uri.clone(),
                            }));
                        }
                        
                        if flags.contains(&RewriteFlag::Break) {
                            return Ok(Some(RewriteAction::Internal {
                                uri: context.uri.clone(),
                            }));
                        }
                    } else {
                        // No flags, just internal rewrite and continue
                        context.uri = new_uri.parse()?;
                    }
                }
            }
        }
        
        // Check if URI was modified
        if context.uri.to_string() != original_uri {
            Ok(Some(RewriteAction::Internal {
                uri: context.uri.clone(),
            }))
        } else {
            Ok(None)
        }
    }

    fn check_conditions(&self, rule: &RewriteRule, context: &RewriteContext) -> Result<bool> {
        let conditions = match &rule.conditions {
            Some(c) => c,
            None => return Ok(true), // No conditions means rule applies
        };
        
        let mut result = true;
        let mut use_or = false;
        
        for condition in conditions {
            let test_string = self.expand_variables(&condition.test_string, context);
            
            let matches = if let Some(regex) = &condition.regex {
                regex.is_match(&test_string)
            } else {
                false
            };
            
            // Handle NOT flag
            let matches = if condition.flags.as_ref()
                .map(|f| f.contains(&ConditionFlag::Not))
                .unwrap_or(false) {
                !matches
            } else {
                matches
            };
            
            // Handle file system checks
            let matches = if let Some(flags) = &condition.flags {
                if flags.contains(&ConditionFlag::File) {
                    std::path::Path::new(&test_string).is_file()
                } else if flags.contains(&ConditionFlag::Dir) {
                    std::path::Path::new(&test_string).is_dir()
                } else if flags.contains(&ConditionFlag::Symlink) {
                    std::fs::symlink_metadata(&test_string)
                        .map(|m| m.file_type().is_symlink())
                        .unwrap_or(false)
                } else if flags.contains(&ConditionFlag::Size) {
                    std::fs::metadata(&test_string)
                        .map(|m| m.len() > 0)
                        .unwrap_or(false)
                } else {
                    matches
                }
            } else {
                matches
            };
            
            // Apply OR/AND logic
            if use_or {
                result = result || matches;
            } else {
                result = result && matches;
            }
            
            // Check if next condition should be OR'd
            use_or = condition.flags.as_ref()
                .map(|f| f.contains(&ConditionFlag::Or))
                .unwrap_or(false);
        }
        
        Ok(result)
    }

    fn apply_replacement(&self, replacement: &str, captures: &Captures, context: &RewriteContext) -> String {
        let mut result = replacement.to_string();
        
        // Replace capture groups $1, $2, etc.
        for i in 0..captures.len() {
            if let Some(capture) = captures.get(i) {
                result = result.replace(&format!("${}", i), capture.as_str());
            }
        }
        
        // Replace server variables
        result = self.expand_variables(&result, context);
        
        result
    }

    fn expand_variables(&self, input: &str, context: &RewriteContext) -> String {
        let mut result = input.to_string();
        
        // Common nginx-style variables
        result = result.replace("$scheme", context.uri.scheme_str().unwrap_or("http"));
        result = result.replace("$host", &context.server_name);
        result = result.replace("$request_uri", context.uri.path());
        result = result.replace("$remote_addr", &context.remote_addr);
        result = result.replace("$request_method", context.method.as_str());
        
        // Query string
        if let Some(query) = context.uri.query() {
            result = result.replace("$query_string", query);
            result = result.replace("$args", query);
        } else {
            result = result.replace("$query_string", "");
            result = result.replace("$args", "");
        }
        
        // Headers
        if let Some(user_agent) = context.headers.get("user-agent") {
            if let Ok(ua) = user_agent.to_str() {
                result = result.replace("$http_user_agent", ua);
            }
        }
        
        if let Some(referer) = context.headers.get("referer") {
            if let Ok(ref_str) = referer.to_str() {
                result = result.replace("$http_referer", ref_str);
            }
        }
        
        if let Some(cookie) = context.headers.get("cookie") {
            if let Ok(cookie_str) = cookie.to_str() {
                result = result.replace("$http_cookie", cookie_str);
            }
        }
        
        // Custom variables
        for (key, value) in &context.variables {
            result = result.replace(&format!("${{{}}}", key), value);
        }
        
        result
    }
}

#[derive(Debug, Clone)]
pub enum RewriteAction {
    Internal { uri: Uri },
    Redirect { location: String, permanent: bool },
    Proxy { backend: String },
    Forbidden,
    Gone,
}

// Helper function to create common rewrite rules
pub fn common_rewrites() -> Vec<RewriteRule> {
    vec![
        // Remove trailing slash
        RewriteRule {
            pattern: r"^(.+)/$".to_string(),
            replacement: "$1".to_string(),
            flags: Some(vec![RewriteFlag::Permanent]),
            conditions: None,
            regex: None,
        },
        // Add www
        RewriteRule {
            pattern: r"^(.*)$".to_string(),
            replacement: "https://www.$host$1".to_string(),
            flags: Some(vec![RewriteFlag::Permanent]),
            conditions: Some(vec![
                RewriteCondition {
                    test_string: "$host".to_string(),
                    pattern: r"^(?!www\.)".to_string(),
                    flags: None,
                    regex: None,
                }
            ]),
            regex: None,
        },
        // Remove .html extension
        RewriteRule {
            pattern: r"^(.+)\.html$".to_string(),
            replacement: "$1".to_string(),
            flags: Some(vec![RewriteFlag::Permanent]),
            conditions: None,
            regex: None,
        },
        // Force HTTPS
        RewriteRule {
            pattern: r"^(.*)$".to_string(),
            replacement: "https://$host$1".to_string(),
            flags: Some(vec![RewriteFlag::Permanent]),
            conditions: Some(vec![
                RewriteCondition {
                    test_string: "$scheme".to_string(),
                    pattern: r"^http$".to_string(),
                    flags: None,
                    regex: None,
                }
            ]),
            regex: None,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Method;

    #[test]
    fn test_simple_rewrite() {
        let rules = vec![
            RewriteRule {
                pattern: r"^/old/(.*)$".to_string(),
                replacement: "/new/$1".to_string(),
                flags: None,
                conditions: None,
                regex: None,
            }
        ];
        
        let engine = RewriteEngine::new(rules).unwrap();
        let mut context = RewriteContext {
            uri: "/old/page".parse().unwrap(),
            method: Method::GET,
            headers: HeaderMap::new(),
            remote_addr: "127.0.0.1".to_string(),
            server_name: "example.com".to_string(),
            variables: HashMap::new(),
        };
        
        let action = engine.process(&mut context).unwrap();
        assert!(action.is_some());
        assert_eq!(context.uri.path(), "/new/page");
    }

    #[test]
    fn test_redirect_flag() {
        let rules = vec![
            RewriteRule {
                pattern: r"^/temp$".to_string(),
                replacement: "/permanent".to_string(),
                flags: Some(vec![RewriteFlag::Permanent]),
                conditions: None,
                regex: None,
            }
        ];
        
        let engine = RewriteEngine::new(rules).unwrap();
        let mut context = RewriteContext {
            uri: "/temp".parse().unwrap(),
            method: Method::GET,
            headers: HeaderMap::new(),
            remote_addr: "127.0.0.1".to_string(),
            server_name: "example.com".to_string(),
            variables: HashMap::new(),
        };
        
        let action = engine.process(&mut context).unwrap();
        match action {
            Some(RewriteAction::Redirect { location, permanent }) => {
                assert_eq!(location, "/permanent");
                assert!(permanent);
            }
            _ => panic!("Expected redirect action"),
        }
    }

    #[test]
    fn test_condition_matching() {
        let rules = vec![
            RewriteRule {
                pattern: r"^(.*)$".to_string(),
                replacement: "/mobile$1".to_string(),
                flags: None,
                conditions: Some(vec![
                    RewriteCondition {
                        test_string: "$http_user_agent".to_string(),
                        pattern: r"Mobile|Android|iPhone".to_string(),
                        flags: Some(vec![ConditionFlag::NoCase]),
                        regex: None,
                    }
                ]),
                regex: None,
            }
        ];
        
        let engine = RewriteEngine::new(rules).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("user-agent", "Mozilla/5.0 iPhone".parse().unwrap());
        
        let mut context = RewriteContext {
            uri: "/page".parse().unwrap(),
            method: Method::GET,
            headers,
            remote_addr: "127.0.0.1".to_string(),
            server_name: "example.com".to_string(),
            variables: HashMap::new(),
        };
        
        let action = engine.process(&mut context).unwrap();
        assert!(action.is_some());
        assert_eq!(context.uri.path(), "/mobile/page");
    }
}