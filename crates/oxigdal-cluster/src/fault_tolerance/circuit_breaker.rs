//! Circuit breaker pattern for fault tolerance.
//!
//! Implements the circuit breaker pattern to prevent cascading failures.
//! The circuit breaker has three states:
//! - Closed: Normal operation, requests pass through
//! - Open: Circuit tripped, requests fail fast
//! - HalfOpen: Testing if system has recovered

use crate::error::{ClusterError, Result};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

/// Circuit breaker state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CircuitState {
    /// Circuit is closed, requests pass through normally
    #[default]
    Closed,
    /// Circuit is open, requests fail immediately
    Open,
    /// Circuit is testing if the system has recovered
    HalfOpen,
}

impl std::fmt::Display for CircuitState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed => write!(f, "Closed"),
            Self::Open => write!(f, "Open"),
            Self::HalfOpen => write!(f, "HalfOpen"),
        }
    }
}

/// Circuit breaker configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerConfig {
    /// Failure threshold to open the circuit
    pub failure_threshold: u32,
    /// Success threshold to close the circuit from half-open
    pub success_threshold: u32,
    /// Duration to keep circuit open before transitioning to half-open
    pub open_duration: Duration,
    /// Window duration for counting failures
    pub failure_window: Duration,
    /// Maximum concurrent requests in half-open state
    pub half_open_max_requests: u32,
    /// Enable automatic reset after open duration
    pub auto_reset: bool,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            success_threshold: 3,
            open_duration: Duration::from_secs(30),
            failure_window: Duration::from_secs(60),
            half_open_max_requests: 3,
            auto_reset: true,
        }
    }
}

/// Circuit breaker statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CircuitBreakerStats {
    /// Total successful calls
    pub total_success: u64,
    /// Total failed calls
    pub total_failures: u64,
    /// Total rejected calls (circuit open)
    pub total_rejected: u64,
    /// Times circuit opened
    pub times_opened: u64,
    /// Times circuit closed from half-open
    pub times_closed: u64,
    /// Current consecutive failures
    pub consecutive_failures: u32,
    /// Current consecutive successes
    pub consecutive_successes: u32,
    /// Current state
    pub current_state: CircuitState,
    /// Milliseconds since last state change (for serialization)
    #[serde(skip)]
    pub last_state_change: Option<Instant>,
}

/// Internal state tracking for circuit breaker.
struct CircuitBreakerInner {
    /// Configuration
    config: CircuitBreakerConfig,
    /// Current state
    state: RwLock<CircuitState>,
    /// Failure timestamps within window
    failure_times: RwLock<Vec<Instant>>,
    /// Consecutive failures count
    consecutive_failures: AtomicU64,
    /// Consecutive successes count
    consecutive_successes: AtomicU64,
    /// Time circuit was opened
    opened_at: RwLock<Option<Instant>>,
    /// Requests in half-open state
    half_open_requests: AtomicU64,
    /// Statistics
    stats: RwLock<CircuitBreakerStats>,
}

/// Circuit breaker for protecting external calls.
#[derive(Clone)]
pub struct CircuitBreaker {
    inner: Arc<CircuitBreakerInner>,
}

impl CircuitBreaker {
    /// Create a new circuit breaker with the given configuration.
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            inner: Arc::new(CircuitBreakerInner {
                config,
                state: RwLock::new(CircuitState::Closed),
                failure_times: RwLock::new(Vec::new()),
                consecutive_failures: AtomicU64::new(0),
                consecutive_successes: AtomicU64::new(0),
                opened_at: RwLock::new(None),
                half_open_requests: AtomicU64::new(0),
                stats: RwLock::new(CircuitBreakerStats::default()),
            }),
        }
    }

    /// Create a circuit breaker with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(CircuitBreakerConfig::default())
    }

    /// Get the current state of the circuit breaker.
    pub fn state(&self) -> CircuitState {
        self.maybe_transition_state();
        *self.inner.state.read()
    }

    /// Check if the circuit breaker allows a request.
    pub fn allow_request(&self) -> Result<CircuitBreakerGuard> {
        self.maybe_transition_state();

        let state = *self.inner.state.read();

        match state {
            CircuitState::Closed => Ok(CircuitBreakerGuard {
                breaker: self.clone(),
                started: Instant::now(),
            }),
            CircuitState::Open => {
                self.inner.stats.write().total_rejected += 1;
                Err(ClusterError::FaultToleranceError(
                    "Circuit breaker is open".to_string(),
                ))
            }
            CircuitState::HalfOpen => {
                let current = self.inner.half_open_requests.load(Ordering::SeqCst);
                if current < self.inner.config.half_open_max_requests as u64 {
                    self.inner.half_open_requests.fetch_add(1, Ordering::SeqCst);
                    Ok(CircuitBreakerGuard {
                        breaker: self.clone(),
                        started: Instant::now(),
                    })
                } else {
                    self.inner.stats.write().total_rejected += 1;
                    Err(ClusterError::FaultToleranceError(
                        "Circuit breaker half-open limit reached".to_string(),
                    ))
                }
            }
        }
    }

    /// Execute a function with circuit breaker protection.
    pub async fn call<F, T, E>(&self, f: F) -> Result<T>
    where
        F: std::future::Future<Output = std::result::Result<T, E>>,
        E: std::fmt::Display,
    {
        let _guard = self.allow_request()?;

        match f.await {
            Ok(result) => {
                self.record_success();
                Ok(result)
            }
            Err(e) => {
                self.record_failure();
                Err(ClusterError::ExecutionError(e.to_string()))
            }
        }
    }

    /// Record a successful operation.
    pub fn record_success(&self) {
        let state = *self.inner.state.read();

        // Reset consecutive failures
        self.inner.consecutive_failures.store(0, Ordering::SeqCst);

        // Increment consecutive successes
        let successes = self
            .inner
            .consecutive_successes
            .fetch_add(1, Ordering::SeqCst)
            + 1;

        // Update stats
        {
            let mut stats = self.inner.stats.write();
            stats.total_success += 1;
            stats.consecutive_failures = 0;
            stats.consecutive_successes = successes as u32;
        }

        // If in half-open state and enough successes, close the circuit
        if state == CircuitState::HalfOpen
            && successes >= self.inner.config.success_threshold as u64
        {
            self.transition_to_closed();
        }

        // Decrement half-open requests if applicable
        if state == CircuitState::HalfOpen {
            self.inner.half_open_requests.fetch_sub(1, Ordering::SeqCst);
        }

        debug!("Circuit breaker: recorded success, state={}", state);
    }

    /// Record a failed operation.
    pub fn record_failure(&self) {
        let now = Instant::now();
        let state = *self.inner.state.read();

        // Add failure timestamp
        {
            let mut failures = self.inner.failure_times.write();
            failures.push(now);

            // Remove failures outside the window
            let window_start = now - self.inner.config.failure_window;
            failures.retain(|&t| t >= window_start);
        }

        // Increment consecutive failures
        let failures = self
            .inner
            .consecutive_failures
            .fetch_add(1, Ordering::SeqCst)
            + 1;

        // Reset consecutive successes
        self.inner.consecutive_successes.store(0, Ordering::SeqCst);

        // Update stats
        {
            let mut stats = self.inner.stats.write();
            stats.total_failures += 1;
            stats.consecutive_failures = failures as u32;
            stats.consecutive_successes = 0;
        }

        // Check if we should open the circuit
        let should_open = match state {
            CircuitState::Closed => failures >= self.inner.config.failure_threshold as u64,
            CircuitState::HalfOpen => true, // Any failure in half-open opens the circuit
            CircuitState::Open => false,
        };

        if should_open {
            self.transition_to_open();
        }

        // Decrement half-open requests if applicable
        if state == CircuitState::HalfOpen {
            self.inner.half_open_requests.fetch_sub(1, Ordering::SeqCst);
        }

        warn!(
            "Circuit breaker: recorded failure, state={}, consecutive={}",
            state, failures
        );
    }

    /// Check and perform state transitions based on timing.
    fn maybe_transition_state(&self) {
        let state = *self.inner.state.read();

        if state == CircuitState::Open && self.inner.config.auto_reset {
            if let Some(opened_at) = *self.inner.opened_at.read() {
                if opened_at.elapsed() >= self.inner.config.open_duration {
                    self.transition_to_half_open();
                }
            }
        }
    }

    /// Transition to closed state.
    fn transition_to_closed(&self) {
        let mut state = self.inner.state.write();
        if *state != CircuitState::Closed {
            *state = CircuitState::Closed;

            // Reset counters
            self.inner.consecutive_failures.store(0, Ordering::SeqCst);
            self.inner.half_open_requests.store(0, Ordering::SeqCst);
            self.inner.failure_times.write().clear();
            *self.inner.opened_at.write() = None;

            // Update stats
            let mut stats = self.inner.stats.write();
            stats.times_closed += 1;
            stats.current_state = CircuitState::Closed;
            stats.last_state_change = Some(Instant::now());

            info!("Circuit breaker: transitioned to CLOSED");
        }
    }

    /// Transition to open state.
    fn transition_to_open(&self) {
        let mut state = self.inner.state.write();
        if *state != CircuitState::Open {
            *state = CircuitState::Open;

            // Record when circuit was opened
            *self.inner.opened_at.write() = Some(Instant::now());

            // Update stats
            let mut stats = self.inner.stats.write();
            stats.times_opened += 1;
            stats.current_state = CircuitState::Open;
            stats.last_state_change = Some(Instant::now());

            warn!("Circuit breaker: transitioned to OPEN");
        }
    }

    /// Transition to half-open state.
    fn transition_to_half_open(&self) {
        let mut state = self.inner.state.write();
        if *state == CircuitState::Open {
            *state = CircuitState::HalfOpen;

            // Reset counters for testing
            self.inner.consecutive_successes.store(0, Ordering::SeqCst);
            self.inner.half_open_requests.store(0, Ordering::SeqCst);

            // Update stats
            let mut stats = self.inner.stats.write();
            stats.current_state = CircuitState::HalfOpen;
            stats.last_state_change = Some(Instant::now());

            info!("Circuit breaker: transitioned to HALF-OPEN");
        }
    }

    /// Force the circuit to open.
    pub fn force_open(&self) {
        self.transition_to_open();
    }

    /// Force the circuit to close.
    pub fn force_close(&self) {
        self.transition_to_closed();
    }

    /// Reset the circuit breaker to its initial state.
    pub fn reset(&self) {
        self.transition_to_closed();
        self.inner.consecutive_successes.store(0, Ordering::SeqCst);

        let mut stats = self.inner.stats.write();
        *stats = CircuitBreakerStats::default();
    }

    /// Get circuit breaker statistics.
    pub fn get_stats(&self) -> CircuitBreakerStats {
        let mut stats = self.inner.stats.read().clone();
        stats.current_state = self.state();
        stats
    }

    /// Get failure rate within the window.
    pub fn failure_rate(&self) -> f64 {
        let stats = self.inner.stats.read();
        let total = stats.total_success + stats.total_failures;
        if total == 0 {
            0.0
        } else {
            stats.total_failures as f64 / total as f64
        }
    }
}

/// Guard that tracks circuit breaker request lifecycle.
pub struct CircuitBreakerGuard {
    breaker: CircuitBreaker,
    #[allow(dead_code)]
    started: Instant,
}

impl CircuitBreakerGuard {
    /// Mark the request as successful.
    pub fn success(self) {
        self.breaker.record_success();
    }

    /// Mark the request as failed.
    pub fn failure(self) {
        self.breaker.record_failure();
    }
}

/// Circuit breaker registry for managing multiple circuit breakers.
#[derive(Clone)]
pub struct CircuitBreakerRegistry {
    breakers: Arc<RwLock<HashMap<String, CircuitBreaker>>>,
    default_config: CircuitBreakerConfig,
}

impl CircuitBreakerRegistry {
    /// Create a new circuit breaker registry.
    pub fn new(default_config: CircuitBreakerConfig) -> Self {
        Self {
            breakers: Arc::new(RwLock::new(HashMap::new())),
            default_config,
        }
    }

    /// Create with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(CircuitBreakerConfig::default())
    }

    /// Get or create a circuit breaker for the given key.
    pub fn get_or_create(&self, key: &str) -> CircuitBreaker {
        let breakers = self.breakers.read();
        if let Some(breaker) = breakers.get(key) {
            return breaker.clone();
        }
        drop(breakers);

        let mut breakers = self.breakers.write();
        breakers
            .entry(key.to_string())
            .or_insert_with(|| CircuitBreaker::new(self.default_config.clone()))
            .clone()
    }

    /// Get a circuit breaker by key.
    pub fn get(&self, key: &str) -> Option<CircuitBreaker> {
        self.breakers.read().get(key).cloned()
    }

    /// Register a circuit breaker with custom configuration.
    pub fn register(&self, key: &str, config: CircuitBreakerConfig) -> CircuitBreaker {
        let breaker = CircuitBreaker::new(config);
        self.breakers
            .write()
            .insert(key.to_string(), breaker.clone());
        breaker
    }

    /// Remove a circuit breaker.
    pub fn remove(&self, key: &str) -> Option<CircuitBreaker> {
        self.breakers.write().remove(key)
    }

    /// Get all circuit breaker stats.
    pub fn get_all_stats(&self) -> HashMap<String, CircuitBreakerStats> {
        self.breakers
            .read()
            .iter()
            .map(|(k, v)| (k.clone(), v.get_stats()))
            .collect()
    }

    /// Reset all circuit breakers.
    pub fn reset_all(&self) {
        for breaker in self.breakers.read().values() {
            breaker.reset();
        }
    }
}

impl Default for CircuitBreakerRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_circuit_breaker_creation() {
        let cb = CircuitBreaker::with_defaults();
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn test_circuit_breaker_opens_on_failures() {
        let config = CircuitBreakerConfig {
            failure_threshold: 3,
            ..Default::default()
        };
        let cb = CircuitBreaker::new(config);

        // Record failures
        for _ in 0..3 {
            cb.record_failure();
        }

        assert_eq!(cb.state(), CircuitState::Open);
    }

    #[test]
    fn test_circuit_breaker_rejects_when_open() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            auto_reset: false,
            ..Default::default()
        };
        let cb = CircuitBreaker::new(config);

        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);

        let result = cb.allow_request();
        assert!(result.is_err());
    }

    #[test]
    fn test_circuit_breaker_closes_on_successes() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            success_threshold: 2,
            auto_reset: false,
            ..Default::default()
        };
        let cb = CircuitBreaker::new(config);

        // Open the circuit
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);

        // Force to half-open
        cb.force_close();
        cb.force_open();
        {
            let mut state = cb.inner.state.write();
            *state = CircuitState::HalfOpen;
        }

        // Record successes
        cb.record_success();
        cb.record_success();

        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn test_circuit_breaker_stats() {
        let cb = CircuitBreaker::with_defaults();

        cb.record_success();
        cb.record_success();
        cb.record_failure();

        let stats = cb.get_stats();
        assert_eq!(stats.total_success, 2);
        assert_eq!(stats.total_failures, 1);
    }

    #[test]
    fn test_circuit_breaker_registry() {
        let registry = CircuitBreakerRegistry::with_defaults();

        let cb1 = registry.get_or_create("service_a");
        let cb2 = registry.get_or_create("service_a");

        // Should be the same circuit breaker
        cb1.record_failure();
        assert_eq!(cb2.get_stats().total_failures, 1);
    }

    #[test]
    fn test_circuit_breaker_force_operations() {
        let cb = CircuitBreaker::with_defaults();

        cb.force_open();
        assert_eq!(cb.state(), CircuitState::Open);

        cb.force_close();
        assert_eq!(cb.state(), CircuitState::Closed);
    }
}
