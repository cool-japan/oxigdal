//! Retry mechanisms with exponential backoff

use crate::error::{Error, Result};
use core::time::Duration;

/// Retry policy with exponential backoff
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// Maximum number of retry attempts
    pub max_attempts: usize,
    /// Initial delay between retries
    pub initial_delay: Duration,
    /// Maximum delay between retries
    pub max_delay: Duration,
    /// Backoff multiplier
    pub multiplier: f64,
    /// Jitter factor (0.0 to 1.0)
    pub jitter_factor: f64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            initial_delay: Duration::from_millis(1000),
            max_delay: Duration::from_millis(60_000),
            multiplier: 2.0,
            jitter_factor: 0.1,
        }
    }
}

impl RetryPolicy {
    /// Create a new retry policy
    pub fn new(
        max_attempts: usize,
        initial_delay: Duration,
        max_delay: Duration,
        multiplier: f64,
        jitter_factor: f64,
    ) -> Result<Self> {
        if max_attempts == 0 {
            return Err(Error::config("max_attempts must be > 0"));
        }
        if multiplier <= 1.0 {
            return Err(Error::config("multiplier must be > 1.0"));
        }
        if !(0.0..=1.0).contains(&jitter_factor) {
            return Err(Error::config("jitter_factor must be between 0.0 and 1.0"));
        }
        if initial_delay > max_delay {
            return Err(Error::config("initial_delay must be <= max_delay"));
        }

        Ok(Self {
            max_attempts,
            initial_delay,
            max_delay,
            multiplier,
            jitter_factor,
        })
    }

    /// Calculate delay for a given attempt number (0-indexed)
    pub fn calculate_delay(&self, attempt: usize) -> Duration {
        if attempt >= self.max_attempts {
            return self.max_delay;
        }

        // Calculate exponential backoff
        let delay_ms = self.initial_delay.as_millis() as f64 * self.multiplier.powi(attempt as i32);
        let delay_ms = delay_ms.min(self.max_delay.as_millis() as f64);

        // Add jitter
        let jitter = self.calculate_jitter(delay_ms);
        let final_delay = delay_ms + jitter;

        Duration::from_millis(final_delay as u64)
    }

    /// Calculate jitter
    fn calculate_jitter(&self, delay_ms: f64) -> f64 {
        if self.jitter_factor == 0.0 {
            return 0.0;
        }

        // Use a simple pseudo-random jitter based on timestamp
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);

        // Simple hash-like function
        let hash = (timestamp % 1000) as f64 / 1000.0; // 0.0 to 1.0

        // Scale to [-jitter_factor, +jitter_factor]
        let jitter_range = delay_ms * self.jitter_factor;
        (hash - 0.5) * 2.0 * jitter_range
    }

    /// Check if should retry based on attempt count
    pub fn should_retry(&self, attempt: usize) -> bool {
        attempt < self.max_attempts
    }

    /// Get next attempt delay
    pub fn next_delay(&self, current_attempt: usize) -> Option<Duration> {
        if self.should_retry(current_attempt) {
            Some(self.calculate_delay(current_attempt))
        } else {
            None
        }
    }
}

/// Retry manager for tracking and executing retries
pub struct RetryManager {
    policy: RetryPolicy,
}

impl RetryManager {
    /// Create a new retry manager
    pub fn new(policy: RetryPolicy) -> Self {
        Self { policy }
    }

    /// Create with default policy
    pub fn default_policy() -> Self {
        Self::new(RetryPolicy::default())
    }

    /// Execute a function with retry logic
    #[cfg(feature = "native")]
    pub async fn execute<F, Fut, T, E>(&self, mut operation: F) -> Result<T>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = core::result::Result<T, E>>,
        E: std::fmt::Display,
    {
        let mut attempt = 0;

        loop {
            match operation().await {
                Ok(result) => {
                    if attempt > 0 {
                        tracing::info!(attempt, "Operation succeeded after retry");
                    }
                    return Ok(result);
                }
                Err(err) => {
                    if !self.policy.should_retry(attempt) {
                        tracing::error!(
                            attempt,
                            error = %err,
                            "Operation failed after max retries"
                        );
                        return Err(Error::RetryExhausted {
                            attempts: attempt + 1,
                            message: err.to_string(),
                        });
                    }

                    let delay = self.policy.calculate_delay(attempt);
                    tracing::warn!(
                        attempt,
                        delay_ms = delay.as_millis(),
                        error = %err,
                        "Operation failed, retrying"
                    );

                    tokio::time::sleep(delay).await;
                    attempt += 1;
                }
            }
        }
    }

    /// Execute with retry and timeout
    #[cfg(feature = "native")]
    pub async fn execute_with_timeout<F, Fut, T, E>(
        &self,
        operation: F,
        timeout: Duration,
    ) -> Result<T>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = core::result::Result<T, E>>,
        E: std::fmt::Display,
    {
        tokio::time::timeout(timeout, self.execute(operation))
            .await
            .map_err(|_| Error::Timeout(format!("Operation timed out after {timeout:?}")))?
    }

    /// Get the retry policy
    pub fn policy(&self) -> &RetryPolicy {
        &self.policy
    }
}

/// Retry statistics
#[derive(Debug, Clone, Default)]
pub struct RetryStatistics {
    /// Total number of operations
    pub total_operations: usize,
    /// Number of operations that succeeded on first try
    pub first_try_success: usize,
    /// Number of operations that succeeded after retry
    pub retry_success: usize,
    /// Number of operations that failed after all retries
    pub exhausted: usize,
    /// Total number of retry attempts
    pub total_retries: usize,
    /// Average retry count for successful operations
    pub avg_retries: f64,
}

impl RetryStatistics {
    /// Create new statistics
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a successful operation
    pub fn record_success(&mut self, retry_count: usize) {
        self.total_operations += 1;
        if retry_count == 0 {
            self.first_try_success += 1;
        } else {
            self.retry_success += 1;
            self.total_retries += retry_count;
        }
        self.update_average();
    }

    /// Record a failed operation
    pub fn record_failure(&mut self, retry_count: usize) {
        self.total_operations += 1;
        self.exhausted += 1;
        self.total_retries += retry_count;
        self.update_average();
    }

    /// Update average retry count
    fn update_average(&mut self) {
        let successful = self.first_try_success + self.retry_success;
        if successful > 0 {
            self.avg_retries = self.total_retries as f64 / successful as f64;
        }
    }

    /// Get success rate
    pub fn success_rate(&self) -> f64 {
        if self.total_operations == 0 {
            return 0.0;
        }
        let successful = self.first_try_success + self.retry_success;
        successful as f64 / self.total_operations as f64
    }

    /// Get retry rate
    pub fn retry_rate(&self) -> f64 {
        if self.total_operations == 0 {
            return 0.0;
        }
        self.retry_success as f64 / self.total_operations as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retry_policy_default() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.max_attempts, 5);
        assert!(policy.should_retry(0));
        assert!(policy.should_retry(4));
        assert!(!policy.should_retry(5));
    }

    #[test]
    fn test_calculate_delay() {
        let policy = RetryPolicy::new(
            5,
            Duration::from_millis(1000),
            Duration::from_millis(10_000),
            2.0,
            0.0, // No jitter for deterministic test
        )
        .expect("failed to create policy");

        let delay0 = policy.calculate_delay(0);
        let delay1 = policy.calculate_delay(1);
        let delay2 = policy.calculate_delay(2);

        assert_eq!(delay0.as_millis(), 1000);
        assert_eq!(delay1.as_millis(), 2000);
        assert_eq!(delay2.as_millis(), 4000);
    }

    #[test]
    fn test_max_delay_cap() {
        let policy = RetryPolicy::new(
            10,
            Duration::from_millis(1000),
            Duration::from_millis(5000),
            2.0,
            0.0,
        )
        .expect("failed to create policy");

        let delay5 = policy.calculate_delay(5); // Would be 32000 without cap
        assert!(delay5.as_millis() <= 5000);
    }

    #[test]
    fn test_invalid_policy() {
        let result = RetryPolicy::new(
            0, // Invalid: max_attempts = 0
            Duration::from_millis(1000),
            Duration::from_millis(10_000),
            2.0,
            0.1,
        );
        assert!(result.is_err());

        let result = RetryPolicy::new(
            5,
            Duration::from_millis(1000),
            Duration::from_millis(10_000),
            0.5, // Invalid: multiplier <= 1.0
            0.1,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_retry_statistics() {
        let mut stats = RetryStatistics::new();

        stats.record_success(0); // Success on first try
        stats.record_success(2); // Success after 2 retries
        stats.record_failure(3); // Failed after 3 retries

        assert_eq!(stats.total_operations, 3);
        assert_eq!(stats.first_try_success, 1);
        assert_eq!(stats.retry_success, 1);
        assert_eq!(stats.exhausted, 1);
        assert_eq!(stats.total_retries, 5);

        let success_rate = stats.success_rate();
        assert!((success_rate - 0.666).abs() < 0.01);
    }

    #[tokio::test]
    #[cfg(feature = "native")]
    async fn test_retry_manager_success() {
        let policy = RetryPolicy::new(
            3,
            Duration::from_millis(10),
            Duration::from_millis(100),
            2.0,
            0.0,
        )
        .expect("failed to create policy");

        let manager = RetryManager::new(policy);

        let mut attempt_count = 0;
        let result = manager
            .execute(|| {
                attempt_count += 1;
                async move {
                    if attempt_count < 2 {
                        Err("temporary error")
                    } else {
                        Ok(42)
                    }
                }
            })
            .await;

        assert!(result.is_ok());
        assert_eq!(result.expect("failed"), 42);
        assert_eq!(attempt_count, 2);
    }

    #[tokio::test]
    #[cfg(feature = "native")]
    async fn test_retry_manager_exhausted() {
        let policy = RetryPolicy::new(
            2,
            Duration::from_millis(10),
            Duration::from_millis(100),
            2.0,
            0.0,
        )
        .expect("failed to create policy");

        let manager = RetryManager::new(policy);

        let result = manager
            .execute(|| async { Err::<(), _>("persistent error") })
            .await;

        let err = result.expect_err("Expected retry to fail");
        assert!(matches!(err, Error::RetryExhausted { .. }));
    }
}
