use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::collections::HashMap;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};
use tracing::debug;

#[derive(Clone)]
pub struct MetricsCollector {
    requests_total: Arc<AtomicU64>,
    requests_by_method: Arc<RwLock<HashMap<String, u64>>>,
    requests_by_status: Arc<RwLock<HashMap<u16, u64>>>,
    response_times: Arc<RwLock<Vec<Duration>>>,
    active_connections: Arc<AtomicUsize>,
    bytes_sent: Arc<AtomicU64>,
    bytes_received: Arc<AtomicU64>,
    errors: Arc<AtomicU64>,
    start_time: Instant,
}

impl MetricsCollector {
    pub fn new() -> Self {
        Self {
            requests_total: Arc::new(AtomicU64::new(0)),
            requests_by_method: Arc::new(RwLock::new(HashMap::new())),
            requests_by_status: Arc::new(RwLock::new(HashMap::new())),
            response_times: Arc::new(RwLock::new(Vec::new())),
            active_connections: Arc::new(AtomicUsize::new(0)),
            bytes_sent: Arc::new(AtomicU64::new(0)),
            bytes_received: Arc::new(AtomicU64::new(0)),
            errors: Arc::new(AtomicU64::new(0)),
            start_time: Instant::now(),
        }
    }

    pub async fn record_request(&self, method: &str, status: u16, duration: Duration, bytes_in: u64, bytes_out: u64) {
        self.requests_total.fetch_add(1, Ordering::Relaxed);
        
        // Record by method
        let mut methods = self.requests_by_method.write().await;
        *methods.entry(method.to_string()).or_insert(0) += 1;
        drop(methods);
        
        // Record by status
        let mut statuses = self.requests_by_status.write().await;
        *statuses.entry(status).or_insert(0) += 1;
        drop(statuses);
        
        // Record response time
        let mut times = self.response_times.write().await;
        times.push(duration);
        // Keep only last 10000 samples to prevent unbounded growth
        if times.len() > 10000 {
            times.drain(0..5000);
        }
        drop(times);
        
        // Record bytes
        self.bytes_received.fetch_add(bytes_in, Ordering::Relaxed);
        self.bytes_sent.fetch_add(bytes_out, Ordering::Relaxed);
        
        // Record errors
        if status >= 500 {
            self.errors.fetch_add(1, Ordering::Relaxed);
        }
        
        debug!("Recorded request: {} {} {}ms", method, status, duration.as_millis());
    }

    pub fn increment_connections(&self) {
        self.active_connections.fetch_add(1, Ordering::Relaxed);
    }

    pub fn decrement_connections(&self) {
        self.active_connections.fetch_sub(1, Ordering::Relaxed);
    }

    pub async fn get_prometheus_metrics(&self) -> String {
        let total = self.requests_total.load(Ordering::Relaxed);
        let active = self.active_connections.load(Ordering::Relaxed);
        let bytes_in = self.bytes_received.load(Ordering::Relaxed);
        let bytes_out = self.bytes_sent.load(Ordering::Relaxed);
        let errors = self.errors.load(Ordering::Relaxed);
        let uptime = self.start_time.elapsed().as_secs();
        
        let methods = self.requests_by_method.read().await;
        let statuses = self.requests_by_status.read().await;
        let times = self.response_times.read().await;
        
        let mut output = String::new();
        
        // Total requests
        output.push_str("# HELP http_requests_total Total number of HTTP requests\n");
        output.push_str("# TYPE http_requests_total counter\n");
        
        for (method, count) in methods.iter() {
            for (status, status_count) in statuses.iter() {
                if *status_count > 0 {
                    output.push_str(&format!(
                        "http_requests_total{{method=\"{}\",status=\"{}\"}} {}\n",
                        method, status, count * status_count / total.max(1)
                    ));
                }
            }
        }
        
        // Response time histogram
        if !times.is_empty() {
            let mut sorted_times: Vec<_> = times.iter().map(|d| d.as_secs_f64()).collect();
            sorted_times.sort_by(|a, b| a.partial_cmp(b).unwrap());
            
            output.push_str("\n# HELP http_request_duration_seconds HTTP request latency\n");
            output.push_str("# TYPE http_request_duration_seconds histogram\n");
            
            let buckets = vec![0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0];
            for bucket in &buckets {
                let count = sorted_times.iter().filter(|&&t| t <= *bucket).count();
                output.push_str(&format!(
                    "http_request_duration_seconds_bucket{{le=\"{}\"}} {}\n",
                    bucket, count
                ));
            }
            output.push_str(&format!(
                "http_request_duration_seconds_bucket{{le=\"+Inf\"}} {}\n",
                sorted_times.len()
            ));
            
            let sum: f64 = sorted_times.iter().sum();
            output.push_str(&format!(
                "http_request_duration_seconds_sum {:.3}\n",
                sum
            ));
            output.push_str(&format!(
                "http_request_duration_seconds_count {}\n",
                sorted_times.len()
            ));
            
            // Percentiles
            let p50 = percentile(&sorted_times, 0.5);
            let p95 = percentile(&sorted_times, 0.95);
            let p99 = percentile(&sorted_times, 0.99);
            
            output.push_str(&format!(
                "\n# HELP http_request_duration_quantile Response time quantiles\n"
            ));
            output.push_str(&format!(
                "# TYPE http_request_duration_quantile gauge\n"
            ));
            output.push_str(&format!(
                "http_request_duration_quantile{{quantile=\"0.5\"}} {:.3}\n",
                p50
            ));
            output.push_str(&format!(
                "http_request_duration_quantile{{quantile=\"0.95\"}} {:.3}\n",
                p95
            ));
            output.push_str(&format!(
                "http_request_duration_quantile{{quantile=\"0.99\"}} {:.3}\n",
                p99
            ));
        }
        
        // Active connections
        output.push_str("\n# HELP http_connections_active Current number of active connections\n");
        output.push_str("# TYPE http_connections_active gauge\n");
        output.push_str(&format!("http_connections_active {}\n", active));
        
        // Bytes
        output.push_str("\n# HELP http_bytes_received_total Total bytes received\n");
        output.push_str("# TYPE http_bytes_received_total counter\n");
        output.push_str(&format!("http_bytes_received_total {}\n", bytes_in));
        
        output.push_str("\n# HELP http_bytes_sent_total Total bytes sent\n");
        output.push_str("# TYPE http_bytes_sent_total counter\n");
        output.push_str(&format!("http_bytes_sent_total {}\n", bytes_out));
        
        // Errors
        output.push_str("\n# HELP http_errors_total Total number of HTTP errors (5xx)\n");
        output.push_str("# TYPE http_errors_total counter\n");
        output.push_str(&format!("http_errors_total {}\n", errors));
        
        // Process metrics
        output.push_str("\n# HELP process_uptime_seconds Time since server start\n");
        output.push_str("# TYPE process_uptime_seconds gauge\n");
        output.push_str(&format!("process_uptime_seconds {}\n", uptime));
        
        // Memory usage (approximate)
        if let Ok(mem_info) = sys_info::mem_info() {
            let used = mem_info.total - mem_info.free;
            output.push_str("\n# HELP process_resident_memory_bytes Resident memory size\n");
            output.push_str("# TYPE process_resident_memory_bytes gauge\n");
            output.push_str(&format!("process_resident_memory_bytes {}\n", used * 1024));
        }
        
        // CPU usage (approximate)
        if let Ok(load) = sys_info::loadavg() {
            output.push_str("\n# HELP process_cpu_load_average System load average\n");
            output.push_str("# TYPE process_cpu_load_average gauge\n");
            output.push_str(&format!("process_cpu_load_average{{period=\"1m\"}} {:.2}\n", load.one));
            output.push_str(&format!("process_cpu_load_average{{period=\"5m\"}} {:.2}\n", load.five));
            output.push_str(&format!("process_cpu_load_average{{period=\"15m\"}} {:.2}\n", load.fifteen));
        }
        
        output
    }

    pub async fn get_json_metrics(&self) -> serde_json::Value {
        let total = self.requests_total.load(Ordering::Relaxed);
        let active = self.active_connections.load(Ordering::Relaxed);
        let bytes_in = self.bytes_received.load(Ordering::Relaxed);
        let bytes_out = self.bytes_sent.load(Ordering::Relaxed);
        let errors = self.errors.load(Ordering::Relaxed);
        let uptime = self.start_time.elapsed();
        
        let times = self.response_times.read().await;
        let mut sorted_times: Vec<_> = times.iter().map(|d| d.as_millis() as f64).collect();
        sorted_times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        
        let (p50, p95, p99, avg) = if !sorted_times.is_empty() {
            (
                percentile(&sorted_times, 0.5),
                percentile(&sorted_times, 0.95),
                percentile(&sorted_times, 0.99),
                sorted_times.iter().sum::<f64>() / sorted_times.len() as f64,
            )
        } else {
            (0.0, 0.0, 0.0, 0.0)
        };
        
        let rps = if uptime.as_secs() > 0 {
            total / uptime.as_secs()
        } else {
            0
        };
        
        serde_json::json!({
            "requests": {
                "total": total,
                "per_second": rps,
                "errors": errors,
                "error_rate": if total > 0 { errors as f64 / total as f64 } else { 0.0 },
            },
            "latency": {
                "p50": p50,
                "p95": p95,
                "p99": p99,
                "avg": avg,
            },
            "connections": {
                "active": active,
            },
            "throughput": {
                "bytes_in": bytes_in,
                "bytes_out": bytes_out,
                "in_rate": if uptime.as_secs() > 0 { bytes_in / uptime.as_secs() } else { 0 },
                "out_rate": if uptime.as_secs() > 0 { bytes_out / uptime.as_secs() } else { 0 },
            },
            "uptime": {
                "seconds": uptime.as_secs(),
                "formatted": format_duration(uptime),
            },
        })
    }
}

fn percentile(sorted_data: &[f64], p: f64) -> f64 {
    if sorted_data.is_empty() {
        return 0.0;
    }
    
    let index = ((sorted_data.len() - 1) as f64 * p) as usize;
    sorted_data[index]
}

fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    let days = seconds / 86400;
    let hours = (seconds % 86400) / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    
    if days > 0 {
        format!("{}d {}h {}m {}s", days, hours, minutes, secs)
    } else if hours > 0 {
        format!("{}h {}m {}s", hours, minutes, secs)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, secs)
    } else {
        format!("{}s", secs)
    }
}

// Middleware helper for tracking request metrics
pub struct RequestMetrics {
    pub start: Instant,
    pub method: String,
    pub bytes_in: u64,
}

impl RequestMetrics {
    pub fn new(method: String, bytes_in: u64) -> Self {
        Self {
            start: Instant::now(),
            method,
            bytes_in,
        }
    }
    
    pub fn duration(&self) -> Duration {
        self.start.elapsed()
    }
}