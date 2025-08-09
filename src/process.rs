use anyhow::{anyhow, Result};
use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use tokio::process::{Child, Command};
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};

use crate::config::{AppType, BackendConfig, ProcessConfig};

#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: u32,
    pub app_type: AppType,
    pub config: ProcessConfig,
    pub restart_count: u32,
    pub last_restart: Option<std::time::Instant>,
}

pub struct ProcessManager {
    processes: Arc<RwLock<HashMap<String, ProcessInfo>>>,
    children: Arc<RwLock<HashMap<String, Child>>>,
}

impl ProcessManager {
    pub fn new() -> Self {
        Self {
            processes: Arc::new(RwLock::new(HashMap::new())),
            children: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn start_backend(&self, name: String, backend: &BackendConfig) -> Result<()> {
        if let Some(process_config) = &backend.process {
            match backend.app_type {
                AppType::NodeJS => {
                    self.start_nodejs_app(&name, process_config).await?;
                }
                AppType::Python => {
                    self.start_python_app(&name, process_config).await?;
                }
                AppType::Tomcat => {
                    self.start_tomcat_app(&name, process_config).await?;
                }
                _ => {
                    info!("No process management needed for {:?} backend", backend.app_type);
                }
            }
        }
        Ok(())
    }

    async fn start_nodejs_app(&self, name: &str, config: &ProcessConfig) -> Result<()> {
        info!("Starting Node.js application: {}", name);
        
        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        if let Some(working_dir) = &config.working_dir {
            cmd.current_dir(working_dir);
        }

        for (key, value) in &config.env {
            cmd.env(key, value);
        }

        let child = cmd.spawn()?;
        let pid = child.id().ok_or_else(|| anyhow!("Failed to get process ID"))?;

        info!("Node.js app {} started with PID: {}", name, pid);

        let process_info = ProcessInfo {
            pid,
            app_type: AppType::NodeJS,
            config: config.clone(),
            restart_count: 0,
            last_restart: None,
        };

        let mut processes = self.processes.write().await;
        processes.insert(name.to_string(), process_info);

        let mut children = self.children.write().await;
        children.insert(name.to_string(), child);

        if config.auto_restart {
            self.monitor_process(name.to_string());
        }

        Ok(())
    }

    async fn start_python_app(&self, name: &str, config: &ProcessConfig) -> Result<()> {
        info!("Starting Python application: {}", name);
        
        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        if let Some(working_dir) = &config.working_dir {
            cmd.current_dir(working_dir);
        }

        for (key, value) in &config.env {
            cmd.env(key, value);
        }

        // Python-specific environment setup
        cmd.env("PYTHONUNBUFFERED", "1");

        let child = cmd.spawn()?;
        let pid = child.id().ok_or_else(|| anyhow!("Failed to get process ID"))?;

        info!("Python app {} started with PID: {}", name, pid);

        let process_info = ProcessInfo {
            pid,
            app_type: AppType::Python,
            config: config.clone(),
            restart_count: 0,
            last_restart: None,
        };

        let mut processes = self.processes.write().await;
        processes.insert(name.to_string(), process_info);

        let mut children = self.children.write().await;
        children.insert(name.to_string(), child);

        if config.auto_restart {
            self.monitor_process(name.to_string());
        }

        Ok(())
    }

    async fn start_tomcat_app(&self, name: &str, config: &ProcessConfig) -> Result<()> {
        info!("Starting Tomcat application: {}", name);
        
        // Tomcat-specific setup
        let catalina_home = config.env.get("CATALINA_HOME")
            .ok_or_else(|| anyhow!("CATALINA_HOME not set for Tomcat"))?;
        
        let catalina_base = config.env.get("CATALINA_BASE")
            .unwrap_or(catalina_home);

        let mut cmd = Command::new(format!("{}/bin/catalina.sh", catalina_home));
        cmd.arg("run")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        // Set Tomcat environment variables
        cmd.env("CATALINA_HOME", catalina_home);
        cmd.env("CATALINA_BASE", catalina_base);
        cmd.env("JAVA_HOME", config.env.get("JAVA_HOME")
            .ok_or_else(|| anyhow!("JAVA_HOME not set for Tomcat"))?);

        // Set JVM options if provided
        if let Some(java_opts) = config.env.get("JAVA_OPTS") {
            cmd.env("JAVA_OPTS", java_opts);
        } else {
            cmd.env("JAVA_OPTS", "-Xms512m -Xmx1024m -XX:MaxMetaspaceSize=256m");
        }

        // Add custom environment variables
        for (key, value) in &config.env {
            if !key.starts_with("CATALINA_") && key != "JAVA_HOME" && key != "JAVA_OPTS" {
                cmd.env(key, value);
            }
        }

        let child = cmd.spawn()?;
        let pid = child.id().ok_or_else(|| anyhow!("Failed to get process ID"))?;

        info!("Tomcat app {} started with PID: {}", name, pid);

        let process_info = ProcessInfo {
            pid,
            app_type: AppType::Tomcat,
            config: config.clone(),
            restart_count: 0,
            last_restart: None,
        };

        let mut processes = self.processes.write().await;
        processes.insert(name.to_string(), process_info);

        let mut children = self.children.write().await;
        children.insert(name.to_string(), child);

        if config.auto_restart {
            self.monitor_process(name.to_string());
        }

        Ok(())
    }

    fn monitor_process(&self, name: String) {
        let manager = self.clone();
        
        tokio::spawn(async move {
            loop {
                sleep(Duration::from_secs(5)).await;

                let should_restart = {
                    let mut children = manager.children.write().await;
                    if let Some(child) = children.get_mut(&name) {
                        match child.try_wait() {
                            Ok(Some(status)) => {
                                warn!("Process {} exited with status: {:?}", name, status);
                                children.remove(&name);
                                true
                            }
                            Ok(None) => false, // Still running
                            Err(e) => {
                                error!("Error checking process {}: {}", name, e);
                                false
                            }
                        }
                    } else {
                        false
                    }
                };

                if should_restart {
                    let processes = manager.processes.read().await;
                    if let Some(info) = processes.get(&name) {
                        if info.config.auto_restart {
                            info!("Restarting process: {}", name);
                            if let Err(e) = manager.restart_process(&name).await {
                                error!("Failed to restart {}: {}", name, e);
                            }
                        }
                    }
                }
            }
        });
    }

    async fn restart_process(&self, name: &str) -> Result<()> {
        let mut processes = self.processes.write().await;
        if let Some(info) = processes.get_mut(name) {
            info.restart_count += 1;
            info.last_restart = Some(std::time::Instant::now());

            // Check for restart throttling
            if info.restart_count > 5 {
                if let Some(last_restart) = info.last_restart {
                    if last_restart.elapsed() < Duration::from_secs(60) {
                        return Err(anyhow!("Process {} restarting too frequently", name));
                    }
                }
            }

            let config = info.config.clone();
            let app_type = info.app_type.clone();
            drop(processes);

            match app_type {
                AppType::NodeJS => self.start_nodejs_app(name, &config).await?,
                AppType::Python => self.start_python_app(name, &config).await?,
                AppType::Tomcat => self.start_tomcat_app(name, &config).await?,
                _ => {}
            }
        }

        Ok(())
    }

    pub async fn stop_process(&self, name: &str) -> Result<()> {
        let mut children = self.children.write().await;
        if let Some(mut child) = children.remove(name) {
            info!("Stopping process: {}", name);
            
            // Try graceful shutdown first
            if let Some(pid) = child.id() {
                let _ = signal::kill(Pid::from_raw(pid as i32), Signal::SIGTERM);
                
                // Wait for graceful shutdown
                tokio::select! {
                    _ = child.wait() => {
                        info!("Process {} stopped gracefully", name);
                    }
                    _ = sleep(Duration::from_secs(10)) => {
                        warn!("Process {} didn't stop gracefully, forcing kill", name);
                        child.kill().await?;
                    }
                }
            }

            let mut processes = self.processes.write().await;
            processes.remove(name);
        }

        Ok(())
    }

    pub async fn stop_all(&self) -> Result<()> {
        let names: Vec<String> = {
            let processes = self.processes.read().await;
            processes.keys().cloned().collect()
        };

        for name in names {
            if let Err(e) = self.stop_process(&name).await {
                error!("Failed to stop process {}: {}", name, e);
            }
        }

        Ok(())
    }

    pub async fn get_status(&self) -> HashMap<String, ProcessInfo> {
        let processes = self.processes.read().await;
        processes.clone()
    }
}

impl Clone for ProcessManager {
    fn clone(&self) -> Self {
        Self {
            processes: self.processes.clone(),
            children: self.children.clone(),
        }
    }
}