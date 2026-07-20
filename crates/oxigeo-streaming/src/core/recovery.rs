//! Error recovery strategies for stream processing.

use crate::error::{Result, StreamingError};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::sleep;

/// Strategy for recovering from failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecoveryStrategy {
    /// No recovery, fail immediately
    Fail,

    /// Retry with exponential backoff
    ExponentialBackoff,

    /// Retry with fixed delay
    FixedDelay,

    /// Skip failed elements
    Skip,

    /// Dead letter queue
    DeadLetter,
}

/// Configuration for recovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryConfig {
    /// Recovery strategy
    pub strategy: RecoveryStrategy,

    /// Maximum retry attempts
    pub max_retries: usize,

    /// Initial retry delay
    pub initial_delay: Duration,

    /// Maximum retry delay
    pub max_delay: Duration,

    /// Backoff multiplier
    pub backoff_multiplier: f64,

    /// Enable failure tracking
    pub track_failures: bool,

    /// Maximum failure history size
    pub max_failure_history: usize,
}

impl Default for RecoveryConfig {
    fn default() -> Self {
        Self {
            strategy: RecoveryStrategy::ExponentialBackoff,
            max_retries: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(60),
            backoff_multiplier: 2.0,
            track_failures: true,
            max_failure_history: 1000,
        }
    }
}

/// Record of a failure event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureRecord {
    /// Timestamp of failure
    pub timestamp: DateTime<Utc>,

    /// Error message
    pub error: String,

    /// Retry attempt number
    pub attempt: usize,

    /// Element that failed (if available)
    pub element_id: Option<String>,

    /// Recovery action taken
    pub action: String,
}

impl FailureRecord {
    /// Create a new failure record.
    pub fn new(error: String, attempt: usize) -> Self {
        Self {
            timestamp: Utc::now(),
            error,
            attempt,
            element_id: None,
            action: "pending".to_string(),
        }
    }

    /// Set the element ID.
    pub fn with_element_id(mut self, id: String) -> Self {
        self.element_id = Some(id);
        self
    }

    /// Set the recovery action.
    pub fn with_action(mut self, action: String) -> Self {
        self.action = action;
        self
    }
}

/// Manages recovery from failures.
pub struct RecoveryManager {
    config: RecoveryConfig,
    failure_history: Arc<RwLock<VecDeque<FailureRecord>>>,
    total_failures: Arc<RwLock<u64>>,
    total_retries: Arc<RwLock<u64>>,
    successful_recoveries: Arc<RwLock<u64>>,
}

impl RecoveryManager {
    /// Create a new recovery manager.
    pub fn new(config: RecoveryConfig) -> Self {
        Self {
            config,
            failure_history: Arc::new(RwLock::new(VecDeque::new())),
            total_failures: Arc::new(RwLock::new(0)),
            total_retries: Arc::new(RwLock::new(0)),
            successful_recoveries: Arc::new(RwLock::new(0)),
        }
    }

    /// Execute an operation with recovery.
    pub async fn execute_with_recovery<F, Fut, T>(&self, mut operation: F) -> Result<T>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let mut attempt = 0;
        let mut last_error_msg: Option<String> = None;

        while attempt <= self.config.max_retries {
            match operation().await {
                Ok(result) => {
                    if attempt > 0 {
                        let mut recoveries = self.successful_recoveries.write().await;
                        *recoveries += 1;
                    }
                    return Ok(result);
                }
                Err(e) => {
                    let error_msg = e.to_string();
                    last_error_msg = Some(error_msg.clone());
                    attempt += 1;

                    if attempt > self.config.max_retries {
                        break;
                    }

                    let delay = self.calculate_delay(attempt);

                    if self.config.track_failures {
                        let record = FailureRecord::new(error_msg, attempt)
                            .with_action(format!("retry after {:?}", delay));
                        self.record_failure(record).await;
                    }

                    let mut retries = self.total_retries.write().await;
                    *retries += 1;

                    sleep(delay).await;
                }
            }
        }

        let mut failures = self.total_failures.write().await;
        *failures += 1;

        if let Some(error_msg) = last_error_msg {
            if self.config.track_failures {
                let record = FailureRecord::new(error_msg.clone(), attempt)
                    .with_action("max retries exceeded".to_string());
                self.record_failure(record).await;
            }

            match self.config.strategy {
                RecoveryStrategy::Fail => Err(StreamingError::Other(error_msg)),
                RecoveryStrategy::Skip => {
                    tracing::warn!("Skipping failed operation after {} attempts", attempt);
                    Err(StreamingError::Other(error_msg))
                }
                RecoveryStrategy::DeadLetter => {
                    tracing::warn!("Moving to dead letter queue after {} attempts", attempt);
                    Err(StreamingError::Other(error_msg))
                }
                _ => Err(StreamingError::Other(error_msg)),
            }
        } else {
            Err(StreamingError::Other("Unknown error".to_string()))
        }
    }

    /// Calculate delay for retry based on strategy.
    fn calculate_delay(&self, attempt: usize) -> Duration {
        match self.config.strategy {
            RecoveryStrategy::FixedDelay => self.config.initial_delay,
            RecoveryStrategy::ExponentialBackoff => {
                let multiplier = self.config.backoff_multiplier.powi(attempt as i32 - 1);
                let delay_ms = self.config.initial_delay.as_millis() as f64 * multiplier;
                let delay = Duration::from_millis(delay_ms as u64);
                delay.min(self.config.max_delay)
            }
            _ => Duration::ZERO,
        }
    }

    /// Record a failure.
    async fn record_failure(&self, record: FailureRecord) {
        let mut history = self.failure_history.write().await;

        history.push_back(record);

        while history.len() > self.config.max_failure_history {
            history.pop_front();
        }
    }

    /// Get failure history.
    pub async fn get_failure_history(&self) -> Vec<FailureRecord> {
        self.failure_history.read().await.iter().cloned().collect()
    }

    /// Get total failures count.
    pub async fn total_failures(&self) -> u64 {
        *self.total_failures.read().await
    }

    /// Get total retries count.
    pub async fn total_retries(&self) -> u64 {
        *self.total_retries.read().await
    }

    /// Get successful recoveries count.
    pub async fn successful_recoveries(&self) -> u64 {
        *self.successful_recoveries.read().await
    }

    /// Calculate recovery success rate.
    pub async fn success_rate(&self) -> f64 {
        let failures = *self.total_failures.read().await;
        let recoveries = *self.successful_recoveries.read().await;

        if failures + recoveries == 0 {
            1.0
        } else {
            recoveries as f64 / (failures + recoveries) as f64
        }
    }

    /// Clear failure history.
    pub async fn clear_history(&self) {
        let mut history = self.failure_history.write().await;
        history.clear();

        *self.total_failures.write().await = 0;
        *self.total_retries.write().await = 0;
        *self.successful_recoveries.write().await = 0;
    }

    /// Get recent failures.
    pub async fn recent_failures(&self, count: usize) -> Vec<FailureRecord> {
        let history = self.failure_history.read().await;
        history.iter().rev().take(count).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[tokio::test]
    async fn test_recovery_manager_success() {
        let config = RecoveryConfig::default();
        let manager = RecoveryManager::new(config);

        let result = manager
            .execute_with_recovery(|| async { Ok::<_, StreamingError>(42) })
            .await
            .expect("recovery execution should succeed");

        assert_eq!(result, 42);
        assert_eq!(manager.total_failures().await, 0);
    }

    #[tokio::test]
    async fn test_recovery_manager_retry_success() {
        let config = RecoveryConfig::default();
        let manager = RecoveryManager::new(config);
        let counter = Arc::new(AtomicU32::new(0));

        let result = manager
            .execute_with_recovery(|| {
                let c = counter.clone();
                async move {
                    let count = c.fetch_add(1, Ordering::Relaxed);
                    if count < 2 {
                        Err(StreamingError::Other("temporary error".to_string()))
                    } else {
                        Ok(42)
                    }
                }
            })
            .await
            .expect("retry should eventually succeed");

        assert_eq!(result, 42);
        assert_eq!(manager.total_retries().await, 2);
        assert_eq!(manager.successful_recoveries().await, 1);
    }

    #[tokio::test]
    async fn test_recovery_manager_max_retries() {
        let config = RecoveryConfig {
            max_retries: 2,
            initial_delay: Duration::from_millis(10),
            ..Default::default()
        };

        let manager = RecoveryManager::new(config);

        let result = manager
            .execute_with_recovery(|| async {
                Err::<i32, _>(StreamingError::Other("persistent error".to_string()))
            })
            .await;

        assert!(result.is_err());
        assert_eq!(manager.total_failures().await, 1);
        assert_eq!(manager.total_retries().await, 2);
    }

    #[tokio::test]
    async fn test_exponential_backoff() {
        let config = RecoveryConfig {
            strategy: RecoveryStrategy::ExponentialBackoff,
            initial_delay: Duration::from_millis(100),
            backoff_multiplier: 2.0,
            max_delay: Duration::from_secs(1),
            ..Default::default()
        };

        let manager = RecoveryManager::new(config);

        let delay1 = manager.calculate_delay(1);
        let delay2 = manager.calculate_delay(2);
        let delay3 = manager.calculate_delay(3);

        assert_eq!(delay1, Duration::from_millis(100));
        assert_eq!(delay2, Duration::from_millis(200));
        assert_eq!(delay3, Duration::from_millis(400));
    }

    #[tokio::test]
    async fn test_failure_history() {
        let config = RecoveryConfig::default();
        let manager = RecoveryManager::new(config);

        let record = FailureRecord::new("test error".to_string(), 1);
        manager.record_failure(record).await;

        let history = manager.get_failure_history().await;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].error, "test error");
    }

    #[tokio::test]
    async fn test_success_rate() {
        let config = RecoveryConfig::default();
        let manager = RecoveryManager::new(config);

        *manager.total_failures.write().await = 2;
        *manager.successful_recoveries.write().await = 8;

        let rate = manager.success_rate().await;
        assert!((rate - 0.8).abs() < 0.01);
    }
}
