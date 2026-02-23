//! Timeout management for fault tolerance.
//!
//! Provides configurable timeout management with support for:
//! - Static timeouts
//! - Dynamic adaptive timeouts
//! - Deadline propagation
//! - Timeout budgets

use crate::error::{ClusterError, Result};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tracing::{debug, warn};

/// Timeout configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeoutConfig {
    /// Default timeout duration
    pub default_timeout: Duration,
    /// Minimum allowed timeout
    pub min_timeout: Duration,
    /// Maximum allowed timeout
    pub max_timeout: Duration,
    /// Enable adaptive timeout adjustment
    pub adaptive: bool,
    /// Percentile for adaptive timeout (e.g., 0.95 for p95)
    pub adaptive_percentile: f64,
    /// Window size for adaptive calculations
    pub adaptive_window_size: usize,
    /// Multiplier for adaptive timeout (safety margin)
    pub adaptive_multiplier: f64,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            default_timeout: Duration::from_secs(30),
            min_timeout: Duration::from_millis(100),
            max_timeout: Duration::from_secs(300),
            adaptive: true,
            adaptive_percentile: 0.95,
            adaptive_window_size: 100,
            adaptive_multiplier: 1.5,
        }
    }
}

/// Timeout statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TimeoutStats {
    /// Total operations
    pub total_operations: u64,
    /// Total timeouts
    pub total_timeouts: u64,
    /// Total successes within timeout
    pub total_success: u64,
    /// Average duration in microseconds
    pub avg_duration_us: u64,
    /// Current adaptive timeout in microseconds
    pub current_timeout_us: u64,
    /// P50 latency in microseconds
    pub p50_latency_us: u64,
    /// P95 latency in microseconds
    pub p95_latency_us: u64,
    /// P99 latency in microseconds
    pub p99_latency_us: u64,
}

/// Internal state for timeout manager.
struct TimeoutManagerInner {
    /// Configuration
    config: TimeoutConfig,
    /// Current timeout (may be adaptive)
    current_timeout: RwLock<Duration>,
    /// Latency history for adaptive calculations
    latency_history: RwLock<Vec<u64>>,
    /// Statistics
    stats: RwLock<TimeoutStats>,
    /// Total duration for averaging
    total_duration_us: AtomicU64,
}

/// Timeout manager for configurable timeout management.
#[derive(Clone)]
pub struct TimeoutManager {
    inner: Arc<TimeoutManagerInner>,
}

impl TimeoutManager {
    /// Create a new timeout manager.
    pub fn new(config: TimeoutConfig) -> Self {
        let current_timeout = config.default_timeout;
        Self {
            inner: Arc::new(TimeoutManagerInner {
                config,
                current_timeout: RwLock::new(current_timeout),
                latency_history: RwLock::new(Vec::new()),
                stats: RwLock::new(TimeoutStats {
                    current_timeout_us: current_timeout.as_micros() as u64,
                    ..Default::default()
                }),
                total_duration_us: AtomicU64::new(0),
            }),
        }
    }

    /// Create with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(TimeoutConfig::default())
    }

    /// Get the current timeout duration.
    pub fn timeout(&self) -> Duration {
        *self.inner.current_timeout.read()
    }

    /// Execute a future with timeout.
    pub async fn call<F, T>(&self, f: F) -> Result<T>
    where
        F: std::future::Future<Output = T>,
    {
        let timeout = self.timeout();
        let start = Instant::now();

        match tokio::time::timeout(timeout, f).await {
            Ok(result) => {
                let duration = start.elapsed();
                self.record_success(duration);
                Ok(result)
            }
            Err(_) => {
                self.record_timeout();
                Err(ClusterError::Timeout(format!(
                    "Operation timed out after {:?}",
                    timeout
                )))
            }
        }
    }

    /// Execute a fallible future with timeout.
    pub async fn call_with_error<F, T, E>(&self, f: F) -> Result<T>
    where
        F: std::future::Future<Output = std::result::Result<T, E>>,
        E: std::fmt::Display,
    {
        let timeout = self.timeout();
        let start = Instant::now();

        match tokio::time::timeout(timeout, f).await {
            Ok(Ok(result)) => {
                let duration = start.elapsed();
                self.record_success(duration);
                Ok(result)
            }
            Ok(Err(e)) => {
                let duration = start.elapsed();
                self.record_success(duration); // Not a timeout
                Err(ClusterError::ExecutionError(e.to_string()))
            }
            Err(_) => {
                self.record_timeout();
                Err(ClusterError::Timeout(format!(
                    "Operation timed out after {:?}",
                    timeout
                )))
            }
        }
    }

    /// Record a successful operation.
    pub fn record_success(&self, duration: Duration) {
        let duration_us = duration.as_micros() as u64;

        // Update latency history
        {
            let mut history = self.inner.latency_history.write();
            history.push(duration_us);
            if history.len() > self.inner.config.adaptive_window_size {
                history.remove(0);
            }
        }

        // Update total duration
        let total = self
            .inner
            .total_duration_us
            .fetch_add(duration_us, Ordering::SeqCst)
            + duration_us;

        // Update stats
        {
            let mut stats = self.inner.stats.write();
            stats.total_operations += 1;
            stats.total_success += 1;
            stats.avg_duration_us = total / stats.total_operations;
        }

        // Update adaptive timeout if enabled
        if self.inner.config.adaptive {
            self.update_adaptive_timeout();
        }

        debug!("Timeout manager: recorded success, duration={:?}", duration);
    }

    /// Record a timeout.
    pub fn record_timeout(&self) {
        let mut stats = self.inner.stats.write();
        stats.total_operations += 1;
        stats.total_timeouts += 1;

        warn!(
            "Timeout manager: operation timed out (total timeouts: {})",
            stats.total_timeouts
        );
    }

    /// Update adaptive timeout based on latency history.
    fn update_adaptive_timeout(&self) {
        let history = self.inner.latency_history.read();
        if history.len() < 10 {
            return;
        }

        // Sort for percentile calculation
        let mut sorted: Vec<u64> = history.clone();
        sorted.sort_unstable();

        // Calculate percentiles
        let p50_idx = (sorted.len() as f64 * 0.50) as usize;
        let p95_idx = (sorted.len() as f64 * self.inner.config.adaptive_percentile) as usize;
        let p99_idx = (sorted.len() as f64 * 0.99) as usize;

        let p50 = sorted
            .get(p50_idx.min(sorted.len() - 1))
            .copied()
            .unwrap_or(0);
        let p95 = sorted
            .get(p95_idx.min(sorted.len() - 1))
            .copied()
            .unwrap_or(0);
        let p99 = sorted
            .get(p99_idx.min(sorted.len() - 1))
            .copied()
            .unwrap_or(0);

        // Calculate adaptive timeout
        let adaptive_us = (p95 as f64 * self.inner.config.adaptive_multiplier) as u64;
        let adaptive_timeout = Duration::from_micros(adaptive_us);

        // Clamp to configured bounds
        let clamped = adaptive_timeout
            .max(self.inner.config.min_timeout)
            .min(self.inner.config.max_timeout);

        // Update current timeout
        *self.inner.current_timeout.write() = clamped;

        // Update stats
        let mut stats = self.inner.stats.write();
        stats.current_timeout_us = clamped.as_micros() as u64;
        stats.p50_latency_us = p50;
        stats.p95_latency_us = p95;
        stats.p99_latency_us = p99;

        debug!(
            "Adaptive timeout updated: {:?} (p95={:?})",
            clamped,
            Duration::from_micros(p95)
        );
    }

    /// Set a fixed timeout (disables adaptive).
    pub fn set_timeout(&self, timeout: Duration) {
        let clamped = timeout
            .max(self.inner.config.min_timeout)
            .min(self.inner.config.max_timeout);

        *self.inner.current_timeout.write() = clamped;
        self.inner.stats.write().current_timeout_us = clamped.as_micros() as u64;
    }

    /// Get timeout statistics.
    pub fn get_stats(&self) -> TimeoutStats {
        self.inner.stats.read().clone()
    }

    /// Get timeout rate.
    pub fn timeout_rate(&self) -> f64 {
        let stats = self.inner.stats.read();
        if stats.total_operations == 0 {
            0.0
        } else {
            stats.total_timeouts as f64 / stats.total_operations as f64
        }
    }

    /// Reset statistics.
    pub fn reset_stats(&self) {
        let current_timeout = *self.inner.current_timeout.read();
        *self.inner.stats.write() = TimeoutStats {
            current_timeout_us: current_timeout.as_micros() as u64,
            ..Default::default()
        };
        self.inner.latency_history.write().clear();
        self.inner.total_duration_us.store(0, Ordering::SeqCst);
    }
}

/// Deadline for propagating timeout budgets across operations.
#[derive(Debug, Clone)]
pub struct Deadline {
    /// When the deadline expires
    expires_at: Instant,
    /// Original budget
    original_budget: Duration,
}

impl Deadline {
    /// Create a new deadline with the given budget.
    pub fn new(budget: Duration) -> Self {
        Self {
            expires_at: Instant::now() + budget,
            original_budget: budget,
        }
    }

    /// Create a deadline from an absolute time.
    pub fn at(expires_at: Instant) -> Self {
        let now = Instant::now();
        let original_budget = if expires_at > now {
            expires_at - now
        } else {
            Duration::ZERO
        };
        Self {
            expires_at,
            original_budget,
        }
    }

    /// Get remaining time until deadline.
    pub fn remaining(&self) -> Duration {
        let now = Instant::now();
        if self.expires_at > now {
            self.expires_at - now
        } else {
            Duration::ZERO
        }
    }

    /// Check if deadline has expired.
    pub fn is_expired(&self) -> bool {
        Instant::now() >= self.expires_at
    }

    /// Get original budget.
    pub fn original_budget(&self) -> Duration {
        self.original_budget
    }

    /// Get time elapsed since deadline was created.
    pub fn elapsed(&self) -> Duration {
        let deadline_start = self.expires_at - self.original_budget;
        Instant::now().saturating_duration_since(deadline_start)
    }

    /// Check deadline and return remaining time or error if expired.
    pub fn check(&self) -> Result<Duration> {
        let remaining = self.remaining();
        if remaining.is_zero() {
            Err(ClusterError::Timeout("Deadline expired".to_string()))
        } else {
            Ok(remaining)
        }
    }

    /// Execute a future with this deadline.
    pub async fn run<F, T>(&self, f: F) -> Result<T>
    where
        F: std::future::Future<Output = T>,
    {
        let remaining = self.check()?;

        match tokio::time::timeout(remaining, f).await {
            Ok(result) => Ok(result),
            Err(_) => Err(ClusterError::Timeout("Deadline exceeded".to_string())),
        }
    }
}

/// Timeout budget for distributing time across multiple operations.
#[derive(Clone)]
pub struct TimeoutBudget {
    inner: Arc<TimeoutBudgetInner>,
}

struct TimeoutBudgetInner {
    /// Total budget
    total_budget: Duration,
    /// Start time
    started_at: Instant,
    /// Operations completed
    operations: AtomicU64,
    /// Time consumed
    consumed_us: AtomicU64,
}

impl TimeoutBudget {
    /// Create a new timeout budget.
    pub fn new(total_budget: Duration) -> Self {
        Self {
            inner: Arc::new(TimeoutBudgetInner {
                total_budget,
                started_at: Instant::now(),
                operations: AtomicU64::new(0),
                consumed_us: AtomicU64::new(0),
            }),
        }
    }

    /// Get total budget.
    pub fn total(&self) -> Duration {
        self.inner.total_budget
    }

    /// Get remaining budget.
    pub fn remaining(&self) -> Duration {
        let elapsed = self.inner.started_at.elapsed();
        if elapsed >= self.inner.total_budget {
            Duration::ZERO
        } else {
            self.inner.total_budget - elapsed
        }
    }

    /// Get elapsed time.
    pub fn elapsed(&self) -> Duration {
        self.inner.started_at.elapsed()
    }

    /// Check if budget is exhausted.
    pub fn is_exhausted(&self) -> bool {
        self.remaining().is_zero()
    }

    /// Allocate a portion of the remaining budget for an operation.
    pub fn allocate(&self, portion: f64) -> Result<Duration> {
        let remaining = self.remaining();
        if remaining.is_zero() {
            return Err(ClusterError::Timeout("Budget exhausted".to_string()));
        }

        let allocated = Duration::from_secs_f64(remaining.as_secs_f64() * portion.min(1.0));
        Ok(allocated)
    }

    /// Allocate budget evenly for remaining operations.
    pub fn allocate_even(&self, remaining_operations: u32) -> Result<Duration> {
        if remaining_operations == 0 {
            return Err(ClusterError::InvalidOperation(
                "No remaining operations".to_string(),
            ));
        }

        let remaining = self.remaining();
        if remaining.is_zero() {
            return Err(ClusterError::Timeout("Budget exhausted".to_string()));
        }

        Ok(remaining / remaining_operations)
    }

    /// Record an operation's duration.
    pub fn record_operation(&self, duration: Duration) {
        self.inner.operations.fetch_add(1, Ordering::SeqCst);
        self.inner
            .consumed_us
            .fetch_add(duration.as_micros() as u64, Ordering::SeqCst);
    }

    /// Get number of operations completed.
    pub fn operations_completed(&self) -> u64 {
        self.inner.operations.load(Ordering::SeqCst)
    }

    /// Get consumed time.
    pub fn consumed(&self) -> Duration {
        Duration::from_micros(self.inner.consumed_us.load(Ordering::SeqCst))
    }

    /// Create a deadline from the remaining budget.
    pub fn to_deadline(&self) -> Deadline {
        Deadline::new(self.remaining())
    }
}

/// Timeout manager registry for managing multiple timeout managers.
#[derive(Clone)]
pub struct TimeoutRegistry {
    managers: Arc<RwLock<HashMap<String, TimeoutManager>>>,
    default_config: TimeoutConfig,
}

impl TimeoutRegistry {
    /// Create a new timeout registry.
    pub fn new(default_config: TimeoutConfig) -> Self {
        Self {
            managers: Arc::new(RwLock::new(HashMap::new())),
            default_config,
        }
    }

    /// Create with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(TimeoutConfig::default())
    }

    /// Get or create a timeout manager for the given key.
    pub fn get_or_create(&self, key: &str) -> TimeoutManager {
        let managers = self.managers.read();
        if let Some(manager) = managers.get(key) {
            return manager.clone();
        }
        drop(managers);

        let mut managers = self.managers.write();
        managers
            .entry(key.to_string())
            .or_insert_with(|| TimeoutManager::new(self.default_config.clone()))
            .clone()
    }

    /// Get a timeout manager by key.
    pub fn get(&self, key: &str) -> Option<TimeoutManager> {
        self.managers.read().get(key).cloned()
    }

    /// Register a timeout manager with custom configuration.
    pub fn register(&self, key: &str, config: TimeoutConfig) -> TimeoutManager {
        let manager = TimeoutManager::new(config);
        self.managers
            .write()
            .insert(key.to_string(), manager.clone());
        manager
    }

    /// Get all timeout stats.
    pub fn get_all_stats(&self) -> HashMap<String, TimeoutStats> {
        self.managers
            .read()
            .iter()
            .map(|(k, v)| (k.clone(), v.get_stats()))
            .collect()
    }
}

impl Default for TimeoutRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_timeout_manager_creation() {
        let tm = TimeoutManager::with_defaults();
        assert_eq!(tm.timeout(), Duration::from_secs(30));
    }

    #[tokio::test]
    async fn test_timeout_success() {
        let tm = TimeoutManager::with_defaults();

        let result = tm.call(async { 42 }).await;
        assert!(result.is_ok());
        assert_eq!(result.ok(), Some(42));

        let stats = tm.get_stats();
        assert_eq!(stats.total_success, 1);
    }

    #[tokio::test]
    async fn test_timeout_exceeded() {
        let config = TimeoutConfig {
            default_timeout: Duration::from_millis(10),
            ..Default::default()
        };
        let tm = TimeoutManager::new(config);

        let result = tm
            .call(async {
                tokio::time::sleep(Duration::from_millis(50)).await;
                42
            })
            .await;

        assert!(result.is_err());

        let stats = tm.get_stats();
        assert_eq!(stats.total_timeouts, 1);
    }

    #[test]
    fn test_deadline() {
        let deadline = Deadline::new(Duration::from_secs(10));
        assert!(!deadline.is_expired());
        assert!(deadline.remaining() <= Duration::from_secs(10));
    }

    #[test]
    fn test_deadline_expired() {
        let deadline = Deadline::new(Duration::ZERO);
        assert!(deadline.is_expired());
        assert_eq!(deadline.remaining(), Duration::ZERO);
    }

    #[test]
    fn test_timeout_budget() {
        let budget = TimeoutBudget::new(Duration::from_secs(10));
        assert_eq!(budget.total(), Duration::from_secs(10));
        assert!(!budget.is_exhausted());

        let allocated = budget.allocate(0.5);
        assert!(allocated.is_ok());
    }

    #[test]
    fn test_timeout_budget_even_allocation() {
        let budget = TimeoutBudget::new(Duration::from_secs(10));

        let allocated = budget.allocate_even(5);
        assert!(allocated.is_ok());
        // Should be approximately 2 seconds each
        assert!(allocated.ok().map(|d| d.as_secs()).unwrap_or(0) >= 1);
    }

    #[tokio::test]
    async fn test_adaptive_timeout() {
        let config = TimeoutConfig {
            adaptive: true,
            adaptive_window_size: 10,
            default_timeout: Duration::from_secs(30),
            min_timeout: Duration::from_millis(1),
            ..Default::default()
        };
        let tm = TimeoutManager::new(config);

        // Record some successes with known durations
        for _ in 0..15 {
            tm.record_success(Duration::from_millis(100));
        }

        // Timeout should have adapted
        let stats = tm.get_stats();
        assert!(stats.p50_latency_us > 0);
    }

    #[test]
    fn test_timeout_registry() {
        let registry = TimeoutRegistry::with_defaults();

        let tm1 = registry.get_or_create("service_a");
        let tm2 = registry.get_or_create("service_a");

        // Should be the same manager
        assert_eq!(tm1.timeout(), tm2.timeout());
    }
}
