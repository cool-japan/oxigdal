//! Load balancing module.
//!
//! Provides load balancing strategies, health checks, and circuit breaker functionality.
//!
//! # Features
//!
//! - **Basic Strategies**: Round-robin, weighted, least connections, IP hash
//! - **Advanced Features**: Connection pooling, consistent hashing, session affinity
//! - **Health Checking**: HTTP, TCP, gRPC health probes with configurable thresholds
//! - **Circuit Breaker**: Sliding window-based circuit breaker with automatic recovery
//! - **Failover**: Automatic retry with exponential backoff and jitter
//! - **Backend Management**: Priority-based selection, graceful draining

pub mod advanced;
pub mod health;
pub mod strategies;

use crate::error::{GatewayError, Result};
use std::sync::Arc;
use std::time::Duration;

/// Backend server configuration.
#[derive(Debug, Clone)]
pub struct Backend {
    /// Backend ID
    pub id: String,
    /// Backend URL
    pub url: String,
    /// Backend weight (for weighted strategies)
    pub weight: u32,
    /// Backend is healthy
    pub healthy: bool,
    /// Last health check timestamp
    pub last_check: Option<chrono::DateTime<chrono::Utc>>,
}

impl Backend {
    /// Creates a new backend.
    pub fn new(id: String, url: String, weight: u32) -> Self {
        Self {
            id,
            url,
            weight,
            healthy: true,
            last_check: None,
        }
    }
}

/// Load balancer configuration.
#[derive(Debug, Clone)]
pub struct LoadBalancerConfig {
    /// Load balancing strategy
    pub strategy: strategies::BalancingStrategy,
    /// Health check interval in seconds
    pub health_check_interval: u64,
    /// Health check timeout in seconds
    pub health_check_timeout: u64,
    /// Circuit breaker threshold
    pub circuit_breaker_threshold: u32,
    /// Circuit breaker timeout in seconds
    pub circuit_breaker_timeout: u64,
    /// Enable sticky sessions
    pub enable_sticky_sessions: bool,
    /// Retry attempts
    pub retry_attempts: u32,
}

impl Default for LoadBalancerConfig {
    fn default() -> Self {
        Self {
            strategy: strategies::BalancingStrategy::RoundRobin,
            health_check_interval: 30,
            health_check_timeout: 5,
            circuit_breaker_threshold: 5,
            circuit_breaker_timeout: 60,
            enable_sticky_sessions: false,
            retry_attempts: 3,
        }
    }
}

/// Circuit breaker state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Circuit is closed (healthy)
    Closed,
    /// Circuit is open (unhealthy)
    Open,
    /// Circuit is half-open (testing)
    HalfOpen,
}

/// Circuit breaker for a backend.
#[derive(Debug)]
pub struct CircuitBreaker {
    state: parking_lot::RwLock<CircuitState>,
    failure_count: std::sync::atomic::AtomicU32,
    threshold: u32,
    timeout: Duration,
    last_failure: parking_lot::RwLock<Option<chrono::DateTime<chrono::Utc>>>,
}

impl CircuitBreaker {
    /// Creates a new circuit breaker.
    pub fn new(threshold: u32, timeout: Duration) -> Self {
        Self {
            state: parking_lot::RwLock::new(CircuitState::Closed),
            failure_count: std::sync::atomic::AtomicU32::new(0),
            threshold,
            timeout,
            last_failure: parking_lot::RwLock::new(None),
        }
    }

    /// Records a successful request.
    pub fn record_success(&self) {
        self.failure_count
            .store(0, std::sync::atomic::Ordering::Relaxed);
        let mut state = self.state.write();
        // Reset circuit to Closed state on success from any state
        *state = CircuitState::Closed;
    }

    /// Records a failed request.
    pub fn record_failure(&self) {
        let count = self
            .failure_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;

        if count >= self.threshold {
            let mut state = self.state.write();
            *state = CircuitState::Open;
            *self.last_failure.write() = Some(chrono::Utc::now());
        }
    }

    /// Checks if requests are allowed.
    pub fn is_allowed(&self) -> bool {
        let state = *self.state.read();

        match state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                let last_failure = *self.last_failure.read();
                if let Some(last) = last_failure {
                    let elapsed = chrono::Utc::now() - last;
                    if elapsed
                        > chrono::Duration::from_std(self.timeout)
                            .ok()
                            .unwrap_or_default()
                    {
                        let mut state_write = self.state.write();
                        *state_write = CircuitState::HalfOpen;
                        return true;
                    }
                }
                false
            }
            CircuitState::HalfOpen => true,
        }
    }

    /// Gets the current state.
    pub fn state(&self) -> CircuitState {
        *self.state.read()
    }
}

/// Load balancer.
pub struct LoadBalancer {
    backends: Arc<parking_lot::RwLock<Vec<Backend>>>,
    strategy: Arc<dyn strategies::LoadBalancingStrategy>,
    circuit_breakers: Arc<dashmap::DashMap<String, Arc<CircuitBreaker>>>,
    health_checker: Arc<health::HealthChecker>,
    config: LoadBalancerConfig,
}

impl LoadBalancer {
    /// Creates a new load balancer.
    pub fn new(config: LoadBalancerConfig) -> Self {
        let strategy: Arc<dyn strategies::LoadBalancingStrategy> = match config.strategy {
            strategies::BalancingStrategy::RoundRobin => {
                Arc::new(strategies::RoundRobinStrategy::new())
            }
            strategies::BalancingStrategy::LeastConnections => {
                Arc::new(strategies::LeastConnectionsStrategy::new())
            }
            strategies::BalancingStrategy::Weighted => {
                Arc::new(strategies::WeightedStrategy::new())
            }
            strategies::BalancingStrategy::IpHash => Arc::new(strategies::IpHashStrategy::new()),
        };

        let health_checker = Arc::new(health::HealthChecker::new(
            Duration::from_secs(config.health_check_interval),
            Duration::from_secs(config.health_check_timeout),
        ));

        Self {
            backends: Arc::new(parking_lot::RwLock::new(Vec::new())),
            strategy,
            circuit_breakers: Arc::new(dashmap::DashMap::new()),
            health_checker,
            config,
        }
    }

    /// Adds a backend.
    pub fn add_backend(&self, backend: Backend) {
        let breaker = Arc::new(CircuitBreaker::new(
            self.config.circuit_breaker_threshold,
            Duration::from_secs(self.config.circuit_breaker_timeout),
        ));

        self.circuit_breakers.insert(backend.id.clone(), breaker);
        self.backends.write().push(backend);
    }

    /// Removes a backend.
    pub fn remove_backend(&self, backend_id: &str) {
        self.backends.write().retain(|b| b.id != backend_id);
        self.circuit_breakers.remove(backend_id);
    }

    /// Selects a backend for a request.
    pub fn select_backend(&self, client_ip: Option<&str>) -> Result<Backend> {
        let backends = self.backends.read();
        let healthy_backends: Vec<&Backend> = backends
            .iter()
            .filter(|b| {
                b.healthy
                    && self
                        .circuit_breakers
                        .get(&b.id)
                        .map(|cb| cb.is_allowed())
                        .unwrap_or(true)
            })
            .collect();

        if healthy_backends.is_empty() {
            return Err(GatewayError::BackendUnavailable(
                "No healthy backends available".to_string(),
            ));
        }

        self.strategy.select(&healthy_backends, client_ip)
    }

    /// Records a successful request to a backend.
    pub fn record_success(&self, backend_id: &str) {
        if let Some(breaker) = self.circuit_breakers.get(backend_id) {
            breaker.record_success();
        }
    }

    /// Records a failed request to a backend.
    pub fn record_failure(&self, backend_id: &str) {
        if let Some(breaker) = self.circuit_breakers.get(backend_id) {
            breaker.record_failure();
        }
    }

    /// Gets all backends.
    pub fn get_backends(&self) -> Vec<Backend> {
        self.backends.read().clone()
    }

    /// Starts health checking.
    pub async fn start_health_checks(&self) {
        let backends = Arc::clone(&self.backends);
        let health_checker = Arc::clone(&self.health_checker);

        tokio::spawn(async move {
            loop {
                // Collect backend info to check
                let backends_to_check: Vec<(String, String)> = {
                    let backends_read = backends.read();
                    backends_read
                        .iter()
                        .map(|b| (b.id.clone(), b.url.clone()))
                        .collect()
                };

                // Check each backend without holding the read lock
                for (backend_id, backend_url) in backends_to_check {
                    let is_healthy = health_checker.check(&backend_url).await;

                    // Update backend health
                    {
                        let mut backends_write = backends.write();
                        if let Some(b) = backends_write.iter_mut().find(|b| b.id == backend_id) {
                            b.healthy = is_healthy;
                            b.last_check = Some(chrono::Utc::now());
                        }
                    } // backends_write dropped here

                    tokio::time::sleep(Duration::from_millis(100)).await;
                }

                tokio::time::sleep(Duration::from_secs(health_checker.interval().as_secs())).await;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_creation() {
        let backend = Backend::new(
            "backend1".to_string(),
            "http://localhost:8080".to_string(),
            1,
        );

        assert_eq!(backend.id, "backend1");
        assert_eq!(backend.url, "http://localhost:8080");
        assert!(backend.healthy);
    }

    #[test]
    fn test_circuit_breaker() {
        let breaker = CircuitBreaker::new(3, Duration::from_secs(60));

        assert_eq!(breaker.state(), CircuitState::Closed);
        assert!(breaker.is_allowed());

        breaker.record_failure();
        breaker.record_failure();
        breaker.record_failure();

        assert_eq!(breaker.state(), CircuitState::Open);
        assert!(!breaker.is_allowed());

        breaker.record_success();
        assert_eq!(breaker.state(), CircuitState::Closed);
    }

    #[test]
    fn test_loadbalancer_creation() {
        let config = LoadBalancerConfig::default();
        let lb = LoadBalancer::new(config);

        assert_eq!(lb.get_backends().len(), 0);
    }

    #[test]
    fn test_add_backend() {
        let config = LoadBalancerConfig::default();
        let lb = LoadBalancer::new(config);

        let backend = Backend::new(
            "backend1".to_string(),
            "http://localhost:8080".to_string(),
            1,
        );

        lb.add_backend(backend);
        assert_eq!(lb.get_backends().len(), 1);
    }

    #[test]
    fn test_remove_backend() {
        let config = LoadBalancerConfig::default();
        let lb = LoadBalancer::new(config);

        let backend = Backend::new(
            "backend1".to_string(),
            "http://localhost:8080".to_string(),
            1,
        );

        lb.add_backend(backend);
        assert_eq!(lb.get_backends().len(), 1);

        lb.remove_backend("backend1");
        assert_eq!(lb.get_backends().len(), 0);
    }
}
