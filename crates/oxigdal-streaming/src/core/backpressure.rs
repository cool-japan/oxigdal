//! Backpressure handling for stream processing.

use crate::error::{Result, StreamingError};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Strategy for handling backpressure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackpressureStrategy {
    /// Block until capacity is available
    Block,

    /// Drop oldest elements
    DropOldest,

    /// Drop newest elements
    DropNewest,

    /// Fail the operation
    Fail,

    /// Adaptive strategy based on load
    Adaptive,
}

/// Configuration for backpressure management.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackpressureConfig {
    /// Strategy to use
    pub strategy: BackpressureStrategy,

    /// High watermark (percentage)
    pub high_watermark: f64,

    /// Low watermark (percentage)
    pub low_watermark: f64,

    /// Maximum latency threshold
    pub max_latency: Duration,

    /// Sample window for metrics
    pub sample_window: Duration,

    /// Enable adaptive backpressure
    pub adaptive: bool,
}

impl Default for BackpressureConfig {
    fn default() -> Self {
        Self {
            strategy: BackpressureStrategy::Block,
            high_watermark: 0.8,
            low_watermark: 0.2,
            max_latency: Duration::from_secs(1),
            sample_window: Duration::from_secs(10),
            adaptive: true,
        }
    }
}

/// Metrics for load monitoring.
#[derive(Debug, Clone)]
pub struct LoadMetrics {
    /// Current buffer utilization (0.0 to 1.0)
    pub buffer_utilization: f64,

    /// Average latency
    pub avg_latency: Duration,

    /// Peak latency
    pub peak_latency: Duration,

    /// Throughput (elements per second)
    pub throughput: f64,

    /// Number of dropped elements
    pub dropped_elements: u64,

    /// Number of backpressure events
    pub backpressure_events: u64,
}

impl Default for LoadMetrics {
    fn default() -> Self {
        Self {
            buffer_utilization: 0.0,
            avg_latency: Duration::ZERO,
            peak_latency: Duration::ZERO,
            throughput: 0.0,
            dropped_elements: 0,
            backpressure_events: 0,
        }
    }
}

/// Manages backpressure for a stream.
pub struct BackpressureManager {
    config: BackpressureConfig,
    metrics: Arc<RwLock<LoadMetrics>>,
    buffer_capacity: AtomicUsize,
    buffer_size: AtomicUsize,
    elements_processed: AtomicU64,
    elements_dropped: AtomicU64,
    backpressure_events: AtomicU64,
    last_sample: Arc<RwLock<Instant>>,
    sample_start: Instant,
}

impl BackpressureManager {
    /// Create a new backpressure manager.
    pub fn new(config: BackpressureConfig, buffer_capacity: usize) -> Self {
        Self {
            config,
            metrics: Arc::new(RwLock::new(LoadMetrics::default())),
            buffer_capacity: AtomicUsize::new(buffer_capacity),
            buffer_size: AtomicUsize::new(0),
            elements_processed: AtomicU64::new(0),
            elements_dropped: AtomicU64::new(0),
            backpressure_events: AtomicU64::new(0),
            last_sample: Arc::new(RwLock::new(Instant::now())),
            sample_start: Instant::now(),
        }
    }

    /// Check if backpressure should be applied.
    pub async fn should_apply_backpressure(&self) -> bool {
        let utilization = self.buffer_utilization();
        utilization >= self.config.high_watermark
    }

    /// Check if backpressure can be released.
    pub async fn can_release_backpressure(&self) -> bool {
        let utilization = self.buffer_utilization();
        utilization <= self.config.low_watermark
    }

    /// Handle a new element arrival.
    pub async fn handle_element_arrival(&self) -> Result<bool> {
        let current_size = self.buffer_size.load(Ordering::Relaxed);
        let capacity = self.buffer_capacity.load(Ordering::Relaxed);

        if current_size >= capacity {
            self.backpressure_events.fetch_add(1, Ordering::Relaxed);

            match self.config.strategy {
                BackpressureStrategy::Block => {
                    return Ok(false);
                }
                BackpressureStrategy::DropOldest | BackpressureStrategy::DropNewest => {
                    self.elements_dropped.fetch_add(1, Ordering::Relaxed);
                    return Ok(true);
                }
                BackpressureStrategy::Fail => {
                    return Err(StreamingError::BufferFull);
                }
                BackpressureStrategy::Adaptive => {
                    if self.should_apply_backpressure().await {
                        return Ok(false);
                    }
                }
            }
        }

        self.buffer_size.fetch_add(1, Ordering::Relaxed);
        Ok(true)
    }

    /// Handle element processing completion.
    pub async fn handle_element_processed(&self, latency: Duration) {
        self.buffer_size.fetch_sub(1, Ordering::Relaxed);
        self.elements_processed.fetch_add(1, Ordering::Relaxed);

        // Update metrics
        self.update_metrics(latency).await;
    }

    /// Update metrics based on current state.
    async fn update_metrics(&self, latency: Duration) {
        let now = Instant::now();
        let last_sample = *self.last_sample.read().await;

        if now.duration_since(last_sample) >= self.config.sample_window {
            let mut metrics = self.metrics.write().await;
            let mut last = self.last_sample.write().await;

            metrics.buffer_utilization = self.buffer_utilization();
            metrics.dropped_elements = self.elements_dropped.load(Ordering::Relaxed);
            metrics.backpressure_events = self.backpressure_events.load(Ordering::Relaxed);

            let elapsed = now.duration_since(self.sample_start).as_secs_f64();
            let processed = self.elements_processed.load(Ordering::Relaxed);
            metrics.throughput = processed as f64 / elapsed;

            if latency > metrics.peak_latency {
                metrics.peak_latency = latency;
            }

            // Simple moving average for latency
            let alpha = 0.1;
            let new_latency_secs = latency.as_secs_f64();
            let old_latency_secs = metrics.avg_latency.as_secs_f64();
            let avg_latency_secs = alpha * new_latency_secs + (1.0 - alpha) * old_latency_secs;
            metrics.avg_latency = Duration::from_secs_f64(avg_latency_secs);

            *last = now;
        }
    }

    /// Get current buffer utilization.
    fn buffer_utilization(&self) -> f64 {
        let size = self.buffer_size.load(Ordering::Relaxed);
        let capacity = self.buffer_capacity.load(Ordering::Relaxed);

        if capacity == 0 {
            0.0
        } else {
            size as f64 / capacity as f64
        }
    }

    /// Get current metrics.
    pub async fn metrics(&self) -> LoadMetrics {
        self.metrics.read().await.clone()
    }

    /// Set buffer capacity.
    pub fn set_capacity(&self, capacity: usize) {
        self.buffer_capacity.store(capacity, Ordering::Relaxed);
    }

    /// Get buffer capacity.
    pub fn capacity(&self) -> usize {
        self.buffer_capacity.load(Ordering::Relaxed)
    }

    /// Get current buffer size.
    pub fn size(&self) -> usize {
        self.buffer_size.load(Ordering::Relaxed)
    }

    /// Reset metrics.
    pub async fn reset_metrics(&self) {
        let mut metrics = self.metrics.write().await;
        *metrics = LoadMetrics::default();

        self.elements_processed.store(0, Ordering::Relaxed);
        self.elements_dropped.store(0, Ordering::Relaxed);
        self.backpressure_events.store(0, Ordering::Relaxed);
    }

    /// Adaptive capacity adjustment based on load.
    pub async fn adjust_capacity_adaptive(&self) {
        // Use real-time buffer utilization instead of cached metrics
        let utilization = self.buffer_utilization();
        let metrics = self.metrics().await;

        if utilization > self.config.high_watermark && metrics.avg_latency < self.config.max_latency
        {
            let current = self.buffer_capacity.load(Ordering::Relaxed);
            let new_capacity = (current as f64 * 1.2) as usize;
            self.buffer_capacity.store(new_capacity, Ordering::Relaxed);
        } else if utilization < self.config.low_watermark {
            let current = self.buffer_capacity.load(Ordering::Relaxed);
            let new_capacity = ((current as f64 * 0.8) as usize).max(64);
            self.buffer_capacity.store(new_capacity, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_backpressure_manager_creation() {
        let config = BackpressureConfig::default();
        let manager = BackpressureManager::new(config, 1000);

        assert_eq!(manager.capacity(), 1000);
        assert_eq!(manager.size(), 0);
    }

    #[tokio::test]
    async fn test_buffer_utilization() {
        let config = BackpressureConfig::default();
        let manager = BackpressureManager::new(config, 100);

        assert_eq!(manager.buffer_utilization(), 0.0);

        for _ in 0..50 {
            manager
                .handle_element_arrival()
                .await
                .expect("backpressure element arrival should succeed");
        }

        assert!((manager.buffer_utilization() - 0.5).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_backpressure_application() {
        let config = BackpressureConfig {
            high_watermark: 0.5,
            ..Default::default()
        };
        let manager = BackpressureManager::new(config, 100);

        for _ in 0..55 {
            manager
                .handle_element_arrival()
                .await
                .expect("backpressure element arrival should succeed");
        }

        assert!(manager.should_apply_backpressure().await);
    }

    #[tokio::test]
    async fn test_adaptive_capacity_adjustment() {
        let config = BackpressureConfig::default();
        let manager = BackpressureManager::new(config, 100);

        let initial_capacity = manager.capacity();

        for _ in 0..95 {
            manager
                .handle_element_arrival()
                .await
                .expect("backpressure element arrival should succeed");
        }

        manager.adjust_capacity_adaptive().await;
        let new_capacity = manager.capacity();

        assert!(new_capacity > initial_capacity);
    }
}
