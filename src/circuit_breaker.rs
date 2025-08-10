use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use anyhow::Result;
use tracing::{debug, warn};

#[derive(Debug, Clone)]
pub struct Config {
    pub failure_threshold: u32,
    pub success_threshold: u32,
    pub timeout: Duration,
    pub half_open_max_calls: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum State {
    Closed,
    Open,
    HalfOpen,
}

pub struct CircuitBreaker {
    config: Config,
    state: Arc<RwLock<State>>,
    failure_count: AtomicU32,
    success_count: AtomicU32,
    half_open_calls: AtomicU32,
    last_failure_time: Arc<RwLock<Option<Instant>>>,
    total_requests: AtomicU64,
    total_failures: AtomicU64,
}

impl CircuitBreaker {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            state: Arc::new(RwLock::new(State::Closed)),
            failure_count: AtomicU32::new(0),
            success_count: AtomicU32::new(0),
            half_open_calls: AtomicU32::new(0),
            last_failure_time: Arc::new(RwLock::new(None)),
            total_requests: AtomicU64::new(0),
            total_failures: AtomicU64::new(0),
        }
    }
    
    pub async fn call<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce() -> Result<T>,
    {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        
        let state = self.get_state().await;
        
        match state {
            State::Open => {
                // Check if we should transition to half-open
                if self.should_attempt_reset().await {
                    self.transition_to_half_open().await;
                } else {
                    warn!("Circuit breaker is open, rejecting request");
                    return Err(anyhow::anyhow!("Circuit breaker is open"));
                }
            }
            State::HalfOpen => {
                let calls = self.half_open_calls.fetch_add(1, Ordering::SeqCst);
                if calls >= self.config.half_open_max_calls {
                    warn!("Circuit breaker half-open limit reached");
                    return Err(anyhow::anyhow!("Circuit breaker half-open limit reached"));
                }
            }
            State::Closed => {
                // Normal operation
            }
        }
        
        // Execute the function
        match f() {
            Ok(result) => {
                self.on_success().await;
                Ok(result)
            }
            Err(e) => {
                self.on_failure().await;
                Err(e)
            }
        }
    }
    
    async fn get_state(&self) -> State {
        *self.state.read().await
    }
    
    async fn on_success(&self) {
        let state = self.get_state().await;
        
        match state {
            State::Closed => {
                self.failure_count.store(0, Ordering::Relaxed);
            }
            State::HalfOpen => {
                let successes = self.success_count.fetch_add(1, Ordering::SeqCst) + 1;
                if successes >= self.config.success_threshold {
                    self.transition_to_closed().await;
                }
            }
            State::Open => {
                // Shouldn't happen
            }
        }
    }
    
    async fn on_failure(&self) {
        self.total_failures.fetch_add(1, Ordering::Relaxed);
        let state = self.get_state().await;
        
        match state {
            State::Closed => {
                let failures = self.failure_count.fetch_add(1, Ordering::SeqCst) + 1;
                if failures >= self.config.failure_threshold {
                    self.transition_to_open().await;
                }
            }
            State::HalfOpen => {
                self.transition_to_open().await;
            }
            State::Open => {
                // Already open
            }
        }
        
        *self.last_failure_time.write().await = Some(Instant::now());
    }
    
    async fn transition_to_open(&self) {
        let mut state = self.state.write().await;
        *state = State::Open;
        self.failure_count.store(0, Ordering::Relaxed);
        self.success_count.store(0, Ordering::Relaxed);
        warn!("Circuit breaker transitioned to OPEN");
    }
    
    async fn transition_to_closed(&self) {
        let mut state = self.state.write().await;
        *state = State::Closed;
        self.failure_count.store(0, Ordering::Relaxed);
        self.success_count.store(0, Ordering::Relaxed);
        self.half_open_calls.store(0, Ordering::Relaxed);
        debug!("Circuit breaker transitioned to CLOSED");
    }
    
    async fn transition_to_half_open(&self) {
        let mut state = self.state.write().await;
        *state = State::HalfOpen;
        self.success_count.store(0, Ordering::Relaxed);
        self.half_open_calls.store(0, Ordering::Relaxed);
        debug!("Circuit breaker transitioned to HALF-OPEN");
    }
    
    async fn should_attempt_reset(&self) -> bool {
        if let Some(last_failure) = *self.last_failure_time.read().await {
            last_failure.elapsed() >= self.config.timeout
        } else {
            false
        }
    }
    
    pub fn get_stats(&self) -> CircuitBreakerStats {
        CircuitBreakerStats {
            total_requests: self.total_requests.load(Ordering::Relaxed),
            total_failures: self.total_failures.load(Ordering::Relaxed),
            current_failures: self.failure_count.load(Ordering::Relaxed),
            current_successes: self.success_count.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub struct CircuitBreakerStats {
    pub total_requests: u64,
    pub total_failures: u64,
    pub current_failures: u32,
    pub current_successes: u32,
}