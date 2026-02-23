//! Flow control mechanisms for stream processing.

use crate::error::{Result, StreamingError};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time::sleep;

/// Configuration for flow control.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowControlConfig {
    /// Maximum rate (elements per second)
    pub max_rate: Option<f64>,

    /// Burst size
    pub burst_size: usize,

    /// Enable rate limiting
    pub enable_rate_limiting: bool,

    /// Smoothing factor for rate adjustment
    pub smoothing_factor: f64,

    /// Target latency for adaptive control
    pub target_latency: Duration,

    /// Adjustment interval
    pub adjustment_interval: Duration,
}

impl Default for FlowControlConfig {
    fn default() -> Self {
        Self {
            max_rate: None,
            burst_size: 100,
            enable_rate_limiting: false,
            smoothing_factor: 0.1,
            target_latency: Duration::from_millis(100),
            adjustment_interval: Duration::from_secs(5),
        }
    }
}

/// Metrics for flow control.
#[derive(Debug, Clone, Default)]
pub struct FlowControlMetrics {
    /// Current rate (elements per second)
    pub current_rate: f64,

    /// Target rate (elements per second)
    pub target_rate: Option<f64>,

    /// Number of throttled operations
    pub throttled_count: u64,

    /// Total delay introduced (milliseconds)
    pub total_delay_ms: u64,

    /// Average processing latency
    pub avg_latency: Duration,
}

/// Token bucket for rate limiting.
struct TokenBucket {
    /// Available tokens
    tokens: Arc<RwLock<f64>>,

    /// Maximum tokens (burst size)
    max_tokens: f64,

    /// Refill rate (tokens per second)
    refill_rate: f64,

    /// Last refill time
    last_refill: Arc<RwLock<Instant>>,
}

impl TokenBucket {
    fn new(max_tokens: usize, refill_rate: f64) -> Self {
        Self {
            tokens: Arc::new(RwLock::new(max_tokens as f64)),
            max_tokens: max_tokens as f64,
            refill_rate,
            last_refill: Arc::new(RwLock::new(Instant::now())),
        }
    }

    async fn try_acquire(&self, count: usize) -> bool {
        self.refill().await;

        let mut tokens = self.tokens.write().await;
        if *tokens >= count as f64 {
            *tokens -= count as f64;
            true
        } else {
            false
        }
    }

    async fn refill(&self) {
        let now = Instant::now();
        let mut last_refill = self.last_refill.write().await;
        let elapsed = now.duration_since(*last_refill).as_secs_f64();

        if elapsed > 0.0 {
            let mut tokens = self.tokens.write().await;
            let new_tokens = elapsed * self.refill_rate;
            *tokens = (*tokens + new_tokens).min(self.max_tokens);
            *last_refill = now;
        }
    }

    async fn wait_for_tokens(&self, count: usize) -> Duration {
        self.refill().await;

        let tokens = self.tokens.read().await;
        if *tokens >= count as f64 {
            return Duration::ZERO;
        }

        let needed = count as f64 - *tokens;
        let wait_time = needed / self.refill_rate;
        Duration::from_secs_f64(wait_time)
    }
}

/// Flow controller for managing stream throughput.
pub struct FlowController {
    config: FlowControlConfig,
    token_bucket: Option<TokenBucket>,
    metrics: Arc<RwLock<FlowControlMetrics>>,
    elements_processed: AtomicU64,
    throttled_count: AtomicU64,
    total_delay_ms: AtomicU64,
    start_time: Instant,
    last_adjustment: Arc<RwLock<Instant>>,
}

impl FlowController {
    /// Create a new flow controller.
    pub fn new(config: FlowControlConfig) -> Self {
        let token_bucket = if config.enable_rate_limiting && config.max_rate.is_some() {
            Some(TokenBucket::new(
                config.burst_size,
                config.max_rate.unwrap_or(1000.0),
            ))
        } else {
            None
        };

        Self {
            config,
            token_bucket,
            metrics: Arc::new(RwLock::new(FlowControlMetrics::default())),
            elements_processed: AtomicU64::new(0),
            throttled_count: AtomicU64::new(0),
            total_delay_ms: AtomicU64::new(0),
            start_time: Instant::now(),
            last_adjustment: Arc::new(RwLock::new(Instant::now())),
        }
    }

    /// Acquire permission to process elements.
    pub async fn acquire(&self, count: usize) -> Result<()> {
        if !self.config.enable_rate_limiting {
            self.elements_processed
                .fetch_add(count as u64, Ordering::Relaxed);
            return Ok(());
        }

        if let Some(ref bucket) = self.token_bucket {
            if !bucket.try_acquire(count).await {
                let wait_time = bucket.wait_for_tokens(count).await;

                if wait_time > Duration::ZERO {
                    self.throttled_count.fetch_add(1, Ordering::Relaxed);
                    self.total_delay_ms
                        .fetch_add(wait_time.as_millis() as u64, Ordering::Relaxed);

                    sleep(wait_time).await;

                    // Try again after waiting
                    if !bucket.try_acquire(count).await {
                        return Err(StreamingError::Other(
                            "Failed to acquire tokens after waiting".to_string(),
                        ));
                    }
                }
            }
        }

        self.elements_processed
            .fetch_add(count as u64, Ordering::Relaxed);
        Ok(())
    }

    /// Try to acquire without blocking.
    pub async fn try_acquire(&self, count: usize) -> bool {
        if !self.config.enable_rate_limiting {
            self.elements_processed
                .fetch_add(count as u64, Ordering::Relaxed);
            return true;
        }

        if let Some(ref bucket) = self.token_bucket {
            if bucket.try_acquire(count).await {
                self.elements_processed
                    .fetch_add(count as u64, Ordering::Relaxed);
                true
            } else {
                false
            }
        } else {
            self.elements_processed
                .fetch_add(count as u64, Ordering::Relaxed);
            true
        }
    }

    /// Record processing latency.
    pub async fn record_latency(&self, latency: Duration) {
        let mut metrics = self.metrics.write().await;

        let alpha = self.config.smoothing_factor;
        let new_latency_secs = latency.as_secs_f64();
        let old_latency_secs = metrics.avg_latency.as_secs_f64();
        let avg_latency_secs = alpha * new_latency_secs + (1.0 - alpha) * old_latency_secs;
        metrics.avg_latency = Duration::from_secs_f64(avg_latency_secs);
    }

    /// Adjust rate based on observed latency.
    pub async fn adjust_rate_adaptive(&self) {
        let now = Instant::now();
        let last_adjustment = *self.last_adjustment.read().await;

        if now.duration_since(last_adjustment) < self.config.adjustment_interval {
            return;
        }

        let metrics = self.metrics.read().await;
        let current_latency = metrics.avg_latency;
        let target_latency = self.config.target_latency;

        drop(metrics);

        if let Some(ref bucket) = self.token_bucket {
            let current_rate = bucket.refill_rate;
            let latency_ratio = current_latency.as_secs_f64() / target_latency.as_secs_f64();

            let new_rate = if latency_ratio > 1.2 {
                current_rate * 0.9
            } else if latency_ratio < 0.8 {
                current_rate * 1.1
            } else {
                current_rate
            };

            // Update metrics
            let mut metrics = self.metrics.write().await;
            metrics.target_rate = Some(new_rate);

            *self.last_adjustment.write().await = now;
        }
    }

    /// Get current metrics.
    pub async fn metrics(&self) -> FlowControlMetrics {
        let mut metrics = self.metrics.read().await.clone();

        let elapsed = self.start_time.elapsed().as_secs_f64();
        let processed = self.elements_processed.load(Ordering::Relaxed);
        metrics.current_rate = processed as f64 / elapsed;
        metrics.throttled_count = self.throttled_count.load(Ordering::Relaxed);
        metrics.total_delay_ms = self.total_delay_ms.load(Ordering::Relaxed);

        metrics
    }

    /// Reset metrics.
    pub async fn reset_metrics(&self) {
        let mut metrics = self.metrics.write().await;
        *metrics = FlowControlMetrics::default();

        self.elements_processed.store(0, Ordering::Relaxed);
        self.throttled_count.store(0, Ordering::Relaxed);
        self.total_delay_ms.store(0, Ordering::Relaxed);
    }

    /// Get current rate.
    pub async fn current_rate(&self) -> f64 {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        let processed = self.elements_processed.load(Ordering::Relaxed);

        if elapsed > 0.0 {
            processed as f64 / elapsed
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_flow_controller_creation() {
        let config = FlowControlConfig::default();
        let controller = FlowController::new(config);

        assert_eq!(controller.current_rate().await, 0.0);
    }

    #[tokio::test]
    async fn test_flow_controller_acquire() {
        let config = FlowControlConfig::default();
        let controller = FlowController::new(config);

        controller
            .acquire(10)
            .await
            .expect("flow controller acquire should succeed");

        let metrics = controller.metrics().await;
        assert!(metrics.current_rate > 0.0);
    }

    #[tokio::test]
    async fn test_token_bucket() {
        let bucket = TokenBucket::new(100, 10.0);

        assert!(bucket.try_acquire(50).await);
        assert!(bucket.try_acquire(50).await);
        assert!(!bucket.try_acquire(1).await);
    }

    #[tokio::test]
    async fn test_rate_limiting() {
        let config = FlowControlConfig {
            enable_rate_limiting: true,
            max_rate: Some(100.0),
            burst_size: 50,
            ..Default::default()
        };

        let controller = FlowController::new(config);

        // First acquire should succeed using burst capacity
        controller
            .acquire(50)
            .await
            .expect("flow controller acquire should succeed");

        // After consuming all burst tokens, wait for refill at 100 tokens/sec
        // Wait 20ms to allow ~2 tokens to be refilled (100 tokens/sec = 0.1 tokens/ms)
        tokio::time::sleep(Duration::from_millis(20)).await;

        // Now try_acquire should succeed as tokens have been refilled
        assert!(controller.try_acquire(1).await);
    }

    #[tokio::test]
    async fn test_latency_recording() {
        let config = FlowControlConfig::default();
        let controller = FlowController::new(config);

        controller.record_latency(Duration::from_millis(100)).await;
        controller.record_latency(Duration::from_millis(200)).await;

        let metrics = controller.metrics().await;
        assert!(metrics.avg_latency > Duration::ZERO);
    }
}
