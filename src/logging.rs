use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};
use tracing::{info, warn, error};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LogConfig {
    pub access_log: AccessLogConfig,
    pub error_log: ErrorLogConfig,
    pub rotation: LogRotationConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AccessLogConfig {
    pub enabled: bool,
    pub path: String,
    pub format: LogFormat,
    pub buffer_size: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ErrorLogConfig {
    pub enabled: bool,
    pub path: String,
    pub level: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LogRotationConfig {
    pub enabled: bool,
    pub max_size_mb: u64,
    pub max_age_days: u32,
    pub max_backups: u32,
    pub compress: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    Common,     // Common Log Format
    Combined,   // Combined Log Format
    Json,       // JSON structured logs
    Custom(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct AccessLogEntry {
    pub timestamp: DateTime<Utc>,
    pub remote_addr: String,
    pub method: String,
    pub path: String,
    pub status: u16,
    pub response_time_ms: u64,
    pub bytes_sent: u64,
    pub user_agent: Option<String>,
    pub referer: Option<String>,
    pub request_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorLogEntry {
    pub timestamp: DateTime<Utc>,
    pub level: String,
    pub message: String,
    pub request_id: Option<String>,
    pub stack_trace: Option<String>,
}

pub struct LogManager {
    config: LogConfig,
    access_writer: Arc<RwLock<Option<File>>>,
    error_writer: Arc<RwLock<Option<File>>>,
    access_buffer: Arc<RwLock<Vec<AccessLogEntry>>>,
    error_buffer: Arc<RwLock<Vec<ErrorLogEntry>>>,
}

impl LogManager {
    pub fn new(config: LogConfig) -> Result<Self> {
        let access_writer = if config.access_log.enabled {
            Some(Self::open_log_file(&config.access_log.path)?)
        } else {
            None
        };

        let error_writer = if config.error_log.enabled {
            Some(Self::open_log_file(&config.error_log.path)?)
        } else {
            None
        };

        let manager = Self {
            config,
            access_writer: Arc::new(RwLock::new(access_writer)),
            error_writer: Arc::new(RwLock::new(error_writer)),
            access_buffer: Arc::new(RwLock::new(Vec::new())),
            error_buffer: Arc::new(RwLock::new(Vec::new())),
        };

        // Start background tasks
        manager.start_flush_task();
        if manager.config.rotation.enabled {
            manager.start_rotation_task();
        }

        Ok(manager)
    }

    fn open_log_file(path: &str) -> Result<File> {
        let path = Path::new(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        
        Ok(file)
    }

    pub async fn log_access(&self, entry: AccessLogEntry) {
        if !self.config.access_log.enabled {
            return;
        }

        let mut buffer = self.access_buffer.write().await;
        buffer.push(entry);

        // Flush if buffer is full
        if buffer.len() >= self.config.access_log.buffer_size {
            drop(buffer);
            self.flush_access_logs().await;
        }
    }

    pub async fn log_error(&self, entry: ErrorLogEntry) {
        if !self.config.error_log.enabled {
            return;
        }

        let mut buffer = self.error_buffer.write().await;
        buffer.push(entry);

        // Errors are flushed immediately for critical issues
        if entry.level == "ERROR" || entry.level == "FATAL" {
            drop(buffer);
            self.flush_error_logs().await;
        }
    }

    async fn flush_access_logs(&self) {
        let mut buffer = self.access_buffer.write().await;
        if buffer.is_empty() {
            return;
        }

        let entries = buffer.drain(..).collect::<Vec<_>>();
        drop(buffer);

        let mut writer = self.access_writer.write().await;
        if let Some(file) = writer.as_mut() {
            for entry in entries {
                let line = self.format_access_log(&entry);
                if let Err(e) = writeln!(file, "{}", line) {
                    error!("Failed to write access log: {}", e);
                }
            }
            let _ = file.flush();
        }
    }

    async fn flush_error_logs(&self) {
        let mut buffer = self.error_buffer.write().await;
        if buffer.is_empty() {
            return;
        }

        let entries = buffer.drain(..).collect::<Vec<_>>();
        drop(buffer);

        let mut writer = self.error_writer.write().await;
        if let Some(file) = writer.as_mut() {
            for entry in entries {
                let line = self.format_error_log(&entry);
                if let Err(e) = writeln!(file, "{}", line) {
                    error!("Failed to write error log: {}", e);
                }
            }
            let _ = file.flush();
        }
    }

    fn format_access_log(&self, entry: &AccessLogEntry) -> String {
        match &self.config.access_log.format {
            LogFormat::Common => {
                // Common Log Format: host ident authuser date request status bytes
                format!(
                    "{} - - [{}] \"{} {}\" {} {}",
                    entry.remote_addr,
                    entry.timestamp.format("%d/%b/%Y:%H:%M:%S %z"),
                    entry.method,
                    entry.path,
                    entry.status,
                    entry.bytes_sent
                )
            }
            LogFormat::Combined => {
                // Combined Log Format: Common + referer + user-agent
                format!(
                    "{} - - [{}] \"{} {}\" {} {} \"{}\" \"{}\"",
                    entry.remote_addr,
                    entry.timestamp.format("%d/%b/%Y:%H:%M:%S %z"),
                    entry.method,
                    entry.path,
                    entry.status,
                    entry.bytes_sent,
                    entry.referer.as_deref().unwrap_or("-"),
                    entry.user_agent.as_deref().unwrap_or("-")
                )
            }
            LogFormat::Json => {
                serde_json::to_string(entry).unwrap_or_default()
            }
            LogFormat::Custom(format) => {
                // Simple template replacement
                format
                    .replace("{remote_addr}", &entry.remote_addr)
                    .replace("{timestamp}", &entry.timestamp.to_rfc3339())
                    .replace("{method}", &entry.method)
                    .replace("{path}", &entry.path)
                    .replace("{status}", &entry.status.to_string())
                    .replace("{response_time}", &entry.response_time_ms.to_string())
                    .replace("{bytes}", &entry.bytes_sent.to_string())
                    .replace("{request_id}", &entry.request_id)
            }
        }
    }

    fn format_error_log(&self, entry: &ErrorLogEntry) -> String {
        serde_json::to_string(entry).unwrap_or_else(|_| {
            format!(
                "[{}] {} - {}",
                entry.timestamp.to_rfc3339(),
                entry.level,
                entry.message
            )
        })
    }

    fn start_flush_task(&self) {
        let access_buffer = self.access_buffer.clone();
        let error_buffer = self.error_buffer.clone();
        let manager = self.clone();

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(1));
            loop {
                interval.tick().await;
                manager.flush_access_logs().await;
                manager.flush_error_logs().await;
            }
        });
    }

    fn start_rotation_task(&self) {
        let config = self.config.clone();
        let access_writer = self.access_writer.clone();
        let error_writer = self.error_writer.clone();

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(3600)); // Check hourly
            loop {
                interval.tick().await;
                
                // Rotate access log
                if config.access_log.enabled {
                    if let Err(e) = Self::rotate_log_file(
                        &config.access_log.path,
                        &config.rotation,
                        access_writer.clone()
                    ).await {
                        error!("Failed to rotate access log: {}", e);
                    }
                }

                // Rotate error log
                if config.error_log.enabled {
                    if let Err(e) = Self::rotate_log_file(
                        &config.error_log.path,
                        &config.rotation,
                        error_writer.clone()
                    ).await {
                        error!("Failed to rotate error log: {}", e);
                    }
                }
            }
        });
    }

    async fn rotate_log_file(
        path: &str,
        config: &LogRotationConfig,
        writer: Arc<RwLock<Option<File>>>
    ) -> Result<()> {
        let path = Path::new(path);
        let metadata = fs::metadata(path)?;
        
        // Check if rotation is needed
        let size_mb = metadata.len() / (1024 * 1024);
        if size_mb < config.max_size_mb {
            return Ok(());
        }

        info!("Rotating log file: {:?}", path);

        // Generate rotation filename
        let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
        let rotation_path = path.with_extension(format!("{}.log", timestamp));

        // Rename current file
        fs::rename(path, &rotation_path)?;

        // Compress if enabled
        if config.compress {
            Self::compress_log_file(&rotation_path)?;
        }

        // Create new log file
        let new_file = Self::open_log_file(path.to_str().unwrap())?;
        let mut writer_guard = writer.write().await;
        *writer_guard = Some(new_file);

        // Clean up old backups
        Self::cleanup_old_logs(path, config)?;

        Ok(())
    }

    fn compress_log_file(path: &Path) -> Result<()> {
        use flate2::write::GzEncoder;
        use flate2::Compression;
        
        let input = File::open(path)?;
        let output_path = path.with_extension("gz");
        let output = File::create(&output_path)?;
        
        let mut encoder = GzEncoder::new(output, Compression::default());
        std::io::copy(&mut &input, &mut encoder)?;
        encoder.finish()?;
        
        fs::remove_file(path)?;
        info!("Compressed log file to: {:?}", output_path);
        
        Ok(())
    }

    fn cleanup_old_logs(base_path: &Path, config: &LogRotationConfig) -> Result<()> {
        let parent = base_path.parent().unwrap_or(Path::new("."));
        let base_name = base_path.file_stem().unwrap_or_default();
        
        let mut rotated_files: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();
        
        for entry in fs::read_dir(parent)? {
            let entry = entry?;
            let path = entry.path();
            let file_name = path.file_name().unwrap_or_default().to_string_lossy();
            
            if file_name.starts_with(&base_name.to_string_lossy()) && 
               (file_name.contains(".2025") || file_name.contains(".2024")) {
                if let Ok(metadata) = entry.metadata() {
                    if let Ok(modified) = metadata.modified() {
                        rotated_files.push((path, modified));
                    }
                }
            }
        }
        
        // Sort by modification time (oldest first)
        rotated_files.sort_by_key(|k| k.1);
        
        // Remove old files exceeding max_backups
        while rotated_files.len() > config.max_backups as usize {
            if let Some((path, _)) = rotated_files.first() {
                fs::remove_file(path)?;
                info!("Removed old log file: {:?}", path);
                rotated_files.remove(0);
            }
        }
        
        Ok(())
    }
}

impl Clone for LogManager {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            access_writer: self.access_writer.clone(),
            error_writer: self.error_writer.clone(),
            access_buffer: self.access_buffer.clone(),
            error_buffer: self.error_buffer.clone(),
        }
    }
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            access_log: AccessLogConfig {
                enabled: true,
                path: "logs/access.log".to_string(),
                format: LogFormat::Combined,
                buffer_size: 100,
            },
            error_log: ErrorLogConfig {
                enabled: true,
                path: "logs/error.log".to_string(),
                level: "ERROR".to_string(),
            },
            rotation: LogRotationConfig {
                enabled: true,
                max_size_mb: 100,
                max_age_days: 30,
                max_backups: 10,
                compress: true,
            },
        }
    }
}