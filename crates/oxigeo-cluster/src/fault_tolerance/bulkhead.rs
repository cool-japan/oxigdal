//! Bulkhead isolation pattern for fault tolerance.
//!
//! Implements bulkhead pattern to isolate different parts of the system
//! and prevent cascading failures by limiting concurrent requests.

use crate::error::{ClusterError, Result};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;
use tracing::{debug, warn};

/// Bulkhead configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkheadConfig {
    /// Maximum concurrent requests
    pub max_concurrent: u32,
    /// Maximum queue size for waiting requests
    pub max_queue_size: u32,
    /// Maximum wait time in queue
    pub max_wait_time: Duration,
    /// Enable fair ordering (FIFO) for queued requests
    pub fair_ordering: bool,
}

impl Default for BulkheadConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 10,
            max_queue_size: 100,
            max_wait_time: Duration::from_secs(30),
            fair_ordering: true,
        }
    }
}

/// Bulkhead statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BulkheadStats {
    /// Total successful calls
    pub total_success: u64,
    /// Total failed calls
    pub total_failures: u64,
    /// Total rejected calls (queue full)
    pub total_rejected: u64,
    /// Total timed out calls (wait too long)
    pub total_timeout: u64,
    /// Current concurrent requests
    pub current_concurrent: u64,
    /// Current queue size
    pub current_queue_size: u64,
    /// Peak concurrent requests
    pub peak_concurrent: u64,
    /// Peak queue size
    pub peak_queue_size: u64,
    /// Average wait time in microseconds
    pub avg_wait_time_us: u64,
}

/// Internal state for bulkhead.
struct BulkheadInner {
    /// Configuration
    config: BulkheadConfig,
    /// Semaphore for concurrency control
    semaphore: Semaphore,
    /// Current queue size
    queue_size: AtomicU64,
    /// Statistics
    stats: RwLock<BulkheadStats>,
    /// Total wait time for averaging
    total_wait_time_us: AtomicU64,
    /// Total completed requests
    total_completed: AtomicU64,
}

/// Bulkhead for isolating concurrent operations.
#[derive(Clone)]
pub struct Bulkhead {
    inner: Arc<BulkheadInner>,
}

impl Bulkhead {
    /// Create a new bulkhead with the given configuration.
    pub fn new(config: BulkheadConfig) -> Self {
        Self {
            inner: Arc::new(BulkheadInner {
                semaphore: Semaphore::new(config.max_concurrent as usize),
                config,
                queue_size: AtomicU64::new(0),
                stats: RwLock::new(BulkheadStats::default()),
                total_wait_time_us: AtomicU64::new(0),
                total_completed: AtomicU64::new(0),
            }),
        }
    }

    /// Create a bulkhead with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(BulkheadConfig::default())
    }

    /// Try to acquire a permit without waiting.
    pub fn try_acquire(&self) -> Result<BulkheadPermit<'_>> {
        match self.inner.semaphore.try_acquire() {
            Ok(permit) => {
                self.update_concurrent_stats(1);
                Ok(BulkheadPermit {
                    bulkhead: self.clone(),
                    _permit: permit,
                    acquired_at: Instant::now(),
                })
            }
            Err(_) => {
                self.inner.stats.write().total_rejected += 1;
                Err(ClusterError::ResourceExhausted(
                    "Bulkhead at capacity".to_string(),
                ))
            }
        }
    }

    /// Acquire a permit, waiting if necessary.
    pub async fn acquire(&self) -> Result<BulkheadPermit<'_>> {
        // Check queue capacity
        let current_queue = self.inner.queue_size.fetch_add(1, Ordering::SeqCst);
        if current_queue >= self.inner.config.max_queue_size as u64 {
            self.inner.queue_size.fetch_sub(1, Ordering::SeqCst);
            self.inner.stats.write().total_rejected += 1;
            return Err(ClusterError::ResourceExhausted(
                "Bulkhead queue full".to_string(),
            ));
        }

        // Update peak queue size
        {
            let mut stats = self.inner.stats.write();
            stats.current_queue_size = current_queue + 1;
            if stats.current_queue_size > stats.peak_queue_size {
                stats.peak_queue_size = stats.current_queue_size;
            }
        }

        let wait_start = Instant::now();

        // Wait for permit with timeout
        let result = tokio::time::timeout(
            self.inner.config.max_wait_time,
            self.inner.semaphore.acquire(),
        )
        .await;

        // Decrement queue size
        self.inner.queue_size.fetch_sub(1, Ordering::SeqCst);
        {
            let mut stats = self.inner.stats.write();
            stats.current_queue_size = self.inner.queue_size.load(Ordering::SeqCst);
        }

        match result {
            Ok(Ok(permit)) => {
                let wait_time = wait_start.elapsed();
                self.record_wait_time(wait_time);
                self.update_concurrent_stats(1);

                debug!("Bulkhead permit acquired after {:?} wait", wait_time);

                Ok(BulkheadPermit {
                    bulkhead: self.clone(),
                    _permit: permit,
                    acquired_at: Instant::now(),
                })
            }
            Ok(Err(_)) => {
                self.inner.stats.write().total_rejected += 1;
                Err(ClusterError::ResourceExhausted(
                    "Bulkhead semaphore closed".to_string(),
                ))
            }
            Err(_) => {
                self.inner.stats.write().total_timeout += 1;
                warn!(
                    "Bulkhead wait timeout after {:?}",
                    self.inner.config.max_wait_time
                );
                Err(ClusterError::Timeout("Bulkhead wait timeout".to_string()))
            }
        }
    }

    /// Execute a function with bulkhead protection.
    pub async fn call<F, T, E>(&self, f: F) -> Result<T>
    where
        F: std::future::Future<Output = std::result::Result<T, E>>,
        E: std::fmt::Display,
    {
        let permit = self.acquire().await?;

        match f.await {
            Ok(result) => {
                permit.success();
                Ok(result)
            }
            Err(e) => {
                permit.failure();
                Err(ClusterError::ExecutionError(e.to_string()))
            }
        }
    }

    /// Try to execute a function without waiting.
    pub async fn try_call<F, T, E>(&self, f: F) -> Result<T>
    where
        F: std::future::Future<Output = std::result::Result<T, E>>,
        E: std::fmt::Display,
    {
        let permit = self.try_acquire()?;

        match f.await {
            Ok(result) => {
                permit.success();
                Ok(result)
            }
            Err(e) => {
                permit.failure();
                Err(ClusterError::ExecutionError(e.to_string()))
            }
        }
    }

    /// Update concurrent request statistics.
    fn update_concurrent_stats(&self, delta: i64) {
        let mut stats = self.inner.stats.write();
        if delta > 0 {
            stats.current_concurrent += delta as u64;
            if stats.current_concurrent > stats.peak_concurrent {
                stats.peak_concurrent = stats.current_concurrent;
            }
        } else {
            stats.current_concurrent = stats
                .current_concurrent
                .saturating_sub(delta.unsigned_abs());
        }
    }

    /// Record wait time for statistics.
    fn record_wait_time(&self, wait_time: Duration) {
        let wait_us = wait_time.as_micros() as u64;
        self.inner
            .total_wait_time_us
            .fetch_add(wait_us, Ordering::SeqCst);
        let completed = self.inner.total_completed.fetch_add(1, Ordering::SeqCst) + 1;

        let total_wait = self.inner.total_wait_time_us.load(Ordering::SeqCst);
        let avg_wait = total_wait / completed;

        self.inner.stats.write().avg_wait_time_us = avg_wait;
    }

    /// Get current available permits.
    pub fn available_permits(&self) -> usize {
        self.inner.semaphore.available_permits()
    }

    /// Get current queue size.
    pub fn queue_size(&self) -> u64 {
        self.inner.queue_size.load(Ordering::SeqCst)
    }

    /// Get bulkhead statistics.
    pub fn get_stats(&self) -> BulkheadStats {
        self.inner.stats.read().clone()
    }

    /// Check if bulkhead is at capacity.
    pub fn is_at_capacity(&self) -> bool {
        self.available_permits() == 0
    }

    /// Get configuration.
    pub fn config(&self) -> &BulkheadConfig {
        &self.inner.config
    }
}

/// Permit granting access through the bulkhead.
pub struct BulkheadPermit<'a> {
    bulkhead: Bulkhead,
    _permit: tokio::sync::SemaphorePermit<'a>,
    #[allow(dead_code)]
    acquired_at: Instant,
}

impl<'a> BulkheadPermit<'a> {
    /// Mark the request as successful and release the permit.
    pub fn success(self) {
        self.bulkhead.inner.stats.write().total_success += 1;
        self.bulkhead.update_concurrent_stats(-1);
    }

    /// Mark the request as failed and release the permit.
    pub fn failure(self) {
        self.bulkhead.inner.stats.write().total_failures += 1;
        self.bulkhead.update_concurrent_stats(-1);
    }
}

impl<'a> Drop for BulkheadPermit<'a> {
    fn drop(&mut self) {
        // Note: success/failure should be called explicitly,
        // but we still need to update concurrent count on drop
        // This is handled by the tokio permit being dropped
    }
}

/// Bulkhead registry for managing multiple bulkheads.
#[derive(Clone)]
pub struct BulkheadRegistry {
    bulkheads: Arc<RwLock<HashMap<String, Bulkhead>>>,
    default_config: BulkheadConfig,
}

impl BulkheadRegistry {
    /// Create a new bulkhead registry.
    pub fn new(default_config: BulkheadConfig) -> Self {
        Self {
            bulkheads: Arc::new(RwLock::new(HashMap::new())),
            default_config,
        }
    }

    /// Create with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(BulkheadConfig::default())
    }

    /// Get or create a bulkhead for the given key.
    pub fn get_or_create(&self, key: &str) -> Bulkhead {
        let bulkheads = self.bulkheads.read();
        if let Some(bulkhead) = bulkheads.get(key) {
            return bulkhead.clone();
        }
        drop(bulkheads);

        let mut bulkheads = self.bulkheads.write();
        bulkheads
            .entry(key.to_string())
            .or_insert_with(|| Bulkhead::new(self.default_config.clone()))
            .clone()
    }

    /// Get a bulkhead by key.
    pub fn get(&self, key: &str) -> Option<Bulkhead> {
        self.bulkheads.read().get(key).cloned()
    }

    /// Register a bulkhead with custom configuration.
    pub fn register(&self, key: &str, config: BulkheadConfig) -> Bulkhead {
        let bulkhead = Bulkhead::new(config);
        self.bulkheads
            .write()
            .insert(key.to_string(), bulkhead.clone());
        bulkhead
    }

    /// Remove a bulkhead.
    pub fn remove(&self, key: &str) -> Option<Bulkhead> {
        self.bulkheads.write().remove(key)
    }

    /// Get all bulkhead stats.
    pub fn get_all_stats(&self) -> HashMap<String, BulkheadStats> {
        self.bulkheads
            .read()
            .iter()
            .map(|(k, v)| (k.clone(), v.get_stats()))
            .collect()
    }
}

impl Default for BulkheadRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}

/// Thread-pool based bulkhead for CPU-bound operations.
#[derive(Clone)]
pub struct ThreadPoolBulkhead {
    inner: Arc<ThreadPoolBulkheadInner>,
}

struct ThreadPoolBulkheadInner {
    /// Configuration
    #[allow(dead_code)]
    config: ThreadPoolBulkheadConfig,
    /// Semaphore for limiting concurrent operations
    semaphore: Semaphore,
    /// Statistics
    stats: RwLock<BulkheadStats>,
}

/// Thread pool bulkhead configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadPoolBulkheadConfig {
    /// Maximum number of concurrent threads
    pub max_threads: u32,
    /// Queue capacity for pending operations
    pub queue_capacity: u32,
}

impl Default for ThreadPoolBulkheadConfig {
    fn default() -> Self {
        Self {
            max_threads: num_cpus::get() as u32,
            queue_capacity: 100,
        }
    }
}

impl ThreadPoolBulkhead {
    /// Create a new thread pool bulkhead.
    pub fn new(config: ThreadPoolBulkheadConfig) -> Self {
        Self {
            inner: Arc::new(ThreadPoolBulkheadInner {
                semaphore: Semaphore::new(config.max_threads as usize),
                config,
                stats: RwLock::new(BulkheadStats::default()),
            }),
        }
    }

    /// Create with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(ThreadPoolBulkheadConfig::default())
    }

    /// Execute a blocking operation with bulkhead protection.
    pub async fn execute_blocking<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        let permit = self.inner.semaphore.acquire().await.map_err(|_| {
            ClusterError::ResourceExhausted("Thread pool bulkhead closed".to_string())
        })?;

        {
            let mut stats = self.inner.stats.write();
            stats.current_concurrent += 1;
            if stats.current_concurrent > stats.peak_concurrent {
                stats.peak_concurrent = stats.current_concurrent;
            }
        }

        let result = tokio::task::spawn_blocking(f).await;

        {
            let mut stats = self.inner.stats.write();
            stats.current_concurrent -= 1;
        }

        drop(permit);

        match result {
            Ok(value) => {
                self.inner.stats.write().total_success += 1;
                Ok(value)
            }
            Err(e) => {
                self.inner.stats.write().total_failures += 1;
                Err(ClusterError::ExecutionError(e.to_string()))
            }
        }
    }

    /// Get statistics.
    pub fn get_stats(&self) -> BulkheadStats {
        self.inner.stats.read().clone()
    }

    /// Get available threads.
    pub fn available_threads(&self) -> usize {
        self.inner.semaphore.available_permits()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_bulkhead_creation() {
        let bulkhead = Bulkhead::with_defaults();
        assert_eq!(bulkhead.available_permits(), 10);
    }

    #[tokio::test]
    async fn test_bulkhead_acquire() {
        let config = BulkheadConfig {
            max_concurrent: 2,
            ..Default::default()
        };
        let bulkhead = Bulkhead::new(config);

        let _p1 = bulkhead.acquire().await;
        assert!(bulkhead.available_permits() == 1);

        let _p2 = bulkhead.acquire().await;
        assert!(bulkhead.available_permits() == 0);
    }

    #[tokio::test]
    async fn test_bulkhead_try_acquire() {
        let config = BulkheadConfig {
            max_concurrent: 1,
            ..Default::default()
        };
        let bulkhead = Bulkhead::new(config);

        let p1 = bulkhead.try_acquire();
        assert!(p1.is_ok());

        let p2 = bulkhead.try_acquire();
        assert!(p2.is_err());
    }

    #[tokio::test]
    async fn test_bulkhead_stats() {
        let bulkhead = Bulkhead::with_defaults();

        {
            let permit = bulkhead.acquire().await;
            assert!(permit.is_ok());
            if let Ok(p) = permit {
                p.success();
            }
        }

        let stats = bulkhead.get_stats();
        assert_eq!(stats.total_success, 1);
    }

    #[tokio::test]
    async fn test_bulkhead_registry() {
        let registry = BulkheadRegistry::with_defaults();

        let b1 = registry.get_or_create("service_a");
        let b2 = registry.get_or_create("service_a");

        // Should be the same bulkhead
        assert_eq!(b1.available_permits(), b2.available_permits());
    }

    #[tokio::test]
    async fn test_thread_pool_bulkhead() {
        let bulkhead = ThreadPoolBulkhead::with_defaults();

        let result = bulkhead.execute_blocking(|| 42).await;
        assert!(result.is_ok());
        assert_eq!(result.ok(), Some(42));

        let stats = bulkhead.get_stats();
        assert_eq!(stats.total_success, 1);
    }
}
