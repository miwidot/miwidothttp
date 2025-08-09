use std::collections::HashMap;
use std::process::{Command, Child, Stdio};
use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use tracing::{info, warn, error};
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProcessConfig {
    pub app_type: AppType,
    pub command: String,
    pub args: Vec<String>,
    pub working_dir: String,
    pub env: HashMap<String, String>,
    pub port: u16,
    pub health_check: Option<String>,
    pub auto_restart: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AppType {
    NodeJs,
    Python,
    Tomcat,
    PhpFpm,
    Static,
}

pub struct ProcessInfo {
    pub config: ProcessConfig,
    pub child: Option<Child>,
    pub status: ProcessStatus,
    pub restarts: u32,
    pub last_health_check: std::time::Instant,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProcessStatus {
    Starting,
    Running,
    Stopped,
    Failed,
    Restarting,
}

pub struct ProcessManager {
    processes: Arc<RwLock<HashMap<String, ProcessInfo>>>,
}

impl ProcessManager {
    pub fn new() -> Self {
        Self {
            processes: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn start_process(&self, name: String, config: ProcessConfig) -> Result<()> {
        info!("Starting {} process: {}", config.app_type.to_string(), name);
        
        let mut child = match config.app_type {
            AppType::NodeJs => self.start_nodejs(&config)?,
            AppType::Python => self.start_python(&config)?,
            AppType::Tomcat => self.start_tomcat(&config)?,
            AppType::PhpFpm => self.start_phpfpm(&config)?,
            AppType::Static => {
                // Static doesn't need a process
                let mut processes = self.processes.write().await;
                processes.insert(name.clone(), ProcessInfo {
                    config,
                    child: None,
                    status: ProcessStatus::Running,
                    restarts: 0,
                    last_health_check: std::time::Instant::now(),
                });
                return Ok(());
            }
        };

        let mut processes = self.processes.write().await;
        processes.insert(name.clone(), ProcessInfo {
            config,
            child: Some(child),
            status: ProcessStatus::Running,
            restarts: 0,
            last_health_check: std::time::Instant::now(),
        });

        info!("Process {} started successfully", name);
        Ok(())
    }

    fn start_nodejs(&self, config: &ProcessConfig) -> Result<Child> {
        let mut cmd = Command::new("node");
        
        // Add arguments
        for arg in &config.args {
            cmd.arg(arg);
        }
        
        // Set working directory
        cmd.current_dir(&config.working_dir);
        
        // Set environment variables
        for (key, value) in &config.env {
            cmd.env(key, value);
        }
        
        // Set PORT environment variable
        cmd.env("PORT", config.port.to_string());
        
        // Redirect stdout/stderr for logging
        cmd.stdout(Stdio::piped())
           .stderr(Stdio::piped());
        
        let child = cmd.spawn()
            .map_err(|e| anyhow!("Failed to start Node.js process: {}", e))?;
        
        Ok(child)
    }

    fn start_python(&self, config: &ProcessConfig) -> Result<Child> {
        let mut cmd = Command::new("python3");
        
        // Add arguments
        for arg in &config.args {
            cmd.arg(arg);
        }
        
        // Set working directory
        cmd.current_dir(&config.working_dir);
        
        // Set environment variables
        for (key, value) in &config.env {
            cmd.env(key, value);
        }
        
        // Set PORT environment variable
        cmd.env("PORT", config.port.to_string());
        
        // Common Python web app environment variables
        cmd.env("FLASK_RUN_PORT", config.port.to_string());
        cmd.env("DJANGO_PORT", config.port.to_string());
        
        // Redirect stdout/stderr for logging
        cmd.stdout(Stdio::piped())
           .stderr(Stdio::piped());
        
        let child = cmd.spawn()
            .map_err(|e| anyhow!("Failed to start Python process: {}", e))?;
        
        Ok(child)
    }

    fn start_tomcat(&self, config: &ProcessConfig) -> Result<Child> {
        // For Tomcat, we typically start it with catalina.sh
        let catalina_path = config.env.get("CATALINA_HOME")
            .map(|h| PathBuf::from(h).join("bin/catalina.sh"))
            .unwrap_or_else(|| PathBuf::from("/usr/local/tomcat/bin/catalina.sh"));
        
        let mut cmd = Command::new(catalina_path);
        cmd.arg("run"); // Run in foreground
        
        // Set working directory
        cmd.current_dir(&config.working_dir);
        
        // Set environment variables
        for (key, value) in &config.env {
            cmd.env(key, value);
        }
        
        // Set Tomcat port (modifies server.xml typically, but we'll use env var)
        cmd.env("CATALINA_OPTS", format!("-Dserver.port={}", config.port));
        
        // Redirect stdout/stderr for logging
        cmd.stdout(Stdio::piped())
           .stderr(Stdio::piped());
        
        let child = cmd.spawn()
            .map_err(|e| anyhow!("Failed to start Tomcat process: {}", e))?;
        
        Ok(child)
    }

    fn start_phpfpm(&self, config: &ProcessConfig) -> Result<Child> {
        let mut cmd = Command::new("php-fpm");
        
        // Run in foreground mode
        cmd.arg("-F");
        
        // Add config file if specified
        if let Some(config_file) = config.env.get("PHP_FPM_CONFIG") {
            cmd.arg("-y").arg(config_file);
        }
        
        // Set working directory
        cmd.current_dir(&config.working_dir);
        
        // Set environment variables
        for (key, value) in &config.env {
            cmd.env(key, value);
        }
        
        // Redirect stdout/stderr for logging
        cmd.stdout(Stdio::piped())
           .stderr(Stdio::piped());
        
        let child = cmd.spawn()
            .map_err(|e| anyhow!("Failed to start PHP-FPM process: {}", e))?;
        
        Ok(child)
    }

    pub async fn stop_process(&self, name: &str) -> Result<()> {
        let mut processes = self.processes.write().await;
        
        if let Some(mut process_info) = processes.remove(name) {
            if let Some(mut child) = process_info.child {
                info!("Stopping process: {}", name);
                
                // Try graceful shutdown first
                #[cfg(unix)]
                {
                    use nix::sys::signal::{self, Signal};
                    use nix::unistd::Pid;
                    
                    if let Ok(pid) = child.id().try_into() {
                        let _ = signal::kill(Pid::from_raw(pid), Signal::SIGTERM);
                    }
                }
                
                // Wait a bit for graceful shutdown
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                
                // Force kill if still running
                let _ = child.kill();
                let _ = child.wait();
                
                info!("Process {} stopped", name);
            }
            Ok(())
        } else {
            Err(anyhow!("Process {} not found", name))
        }
    }

    pub async fn restart_process(&self, name: &str) -> Result<()> {
        let config = {
            let processes = self.processes.read().await;
            processes.get(name)
                .map(|p| p.config.clone())
                .ok_or_else(|| anyhow!("Process {} not found", name))?
        };
        
        self.stop_process(name).await?;
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        self.start_process(name.to_string(), config).await?;
        
        Ok(())
    }

    pub async fn health_check(&self, name: &str) -> Result<bool> {
        let processes = self.processes.read().await;
        
        if let Some(process_info) = processes.get(name) {
            if process_info.status != ProcessStatus::Running {
                return Ok(false);
            }
            
            // Check if process is still alive
            if let Some(child) = &process_info.child {
                // This would check if process is still running
                // In real implementation, we'd also check the health endpoint
                return Ok(true);
            }
            
            // For static apps, always healthy
            if process_info.config.app_type == AppType::Static {
                return Ok(true);
            }
        }
        
        Ok(false)
    }

    pub async fn get_status(&self) -> HashMap<String, ProcessStatus> {
        let processes = self.processes.read().await;
        let mut result = HashMap::new();
        for (name, info) in processes.iter() {
            result.insert(name.clone(), info.status.clone());
        }
        result
    }

    pub async fn monitor_processes(&self) {
        let manager = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
                
                let names_to_restart = {
                    let processes = manager.processes.read().await;
                    let mut to_restart = Vec::new();
                    for (name, info) in processes.iter() {
                        if info.config.auto_restart && info.status == ProcessStatus::Failed {
                            to_restart.push(name.clone());
                        }
                    }
                    to_restart
                };
                
                for name in names_to_restart {
                    warn!("Process {} failed, attempting restart", name);
                    if let Err(e) = manager.restart_process(&name).await {
                        error!("Failed to restart process {}: {}", name, e);
                    }
                }
            }
        });
    }
}

impl Clone for ProcessManager {
    fn clone(&self) -> Self {
        Self {
            processes: self.processes.clone(),
        }
    }
}

impl AppType {
    fn to_string(&self) -> &str {
        match self {
            AppType::NodeJs => "Node.js",
            AppType::Python => "Python",
            AppType::Tomcat => "Tomcat",
            AppType::PhpFpm => "PHP-FPM",
            AppType::Static => "Static",
        }
    }
}