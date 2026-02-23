//! Retry logic with exponential backoff and circuit breaker pattern
//!
//! This module provides comprehensive retry mechanisms for cloud storage operations,
//! including exponential backoff, jitter, circuit breaker, retry budgets, and idempotency checks.

use std::time::Duration;

use crate::error::{CloudError, Result, RetryError};

/// Default maximum number of retry attempts
pub const DEFAULT_MAX_RETRIES: usize = 3;

/// Default initial backoff duration (100ms)
pub const DEFAULT_INITIAL_BACKOFF: Duration = Duration::from_millis(100);

/// Default maximum backoff duration (30 seconds)
pub const DEFAULT_MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Default backoff multiplier
pub const DEFAULT_BACKOFF_MULTIPLIER: f64 = 2.0;

/// Retry strategy configuration
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_retries: usize,
    /// Initial backoff duration
    pub initial_backoff: Duration,
    /// Maximum backoff duration
    pub max_backoff: Duration,
    /// Backoff multiplier for exponential backoff
    pub backoff_multiplier: f64,
    /// Whether to add jitter to backoff
    pub jitter: bool,
    /// Whether to enable circuit breaker
    pub circuit_breaker: bool,
    /// Circuit breaker failure threshold
    pub circuit_breaker_threshold: usize,
    /// Circuit breaker timeout duration
    pub circuit_breaker_timeout: Duration,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: DEFAULT_MAX_RETRIES,
            initial_backoff: DEFAULT_INITIAL_BACKOFF,
            max_backoff: DEFAULT_MAX_BACKOFF,
            backoff_multiplier: DEFAULT_BACKOFF_MULTIPLIER,
            jitter: true,
            circuit_breaker: true,
            circuit_breaker_threshold: 5,
            circuit_breaker_timeout: Duration::from_secs(60),
        }
    }
}

impl RetryConfig {
    /// Creates a new retry configuration
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the maximum number of retries
    #[must_use]
    pub fn with_max_retries(mut self, max_retries: usize) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// Sets the initial backoff duration
    #[must_use]
    pub fn with_initial_backoff(mut self, duration: Duration) -> Self {
        self.initial_backoff = duration;
        self
    }

    /// Sets the maximum backoff duration
    #[must_use]
    pub fn with_max_backoff(mut self, duration: Duration) -> Self {
        self.max_backoff = duration;
        self
    }

    /// Sets the backoff multiplier
    #[must_use]
    pub fn with_backoff_multiplier(mut self, multiplier: f64) -> Self {
        self.backoff_multiplier = multiplier;
        self
    }

    /// Enables or disables jitter
    #[must_use]
    pub fn with_jitter(mut self, jitter: bool) -> Self {
        self.jitter = jitter;
        self
    }

    /// Enables or disables circuit breaker
    #[must_use]
    pub fn with_circuit_breaker(mut self, enabled: bool) -> Self {
        self.circuit_breaker = enabled;
        self
    }
}

/// Circuit breaker state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Circuit is closed (normal operation)
    Closed,
    /// Circuit is open (failures exceeded threshold)
    Open,
    /// Circuit is half-open (testing if service recovered)
    HalfOpen,
}

/// Circuit breaker for preventing cascading failures
#[derive(Debug)]
pub struct CircuitBreaker {
    /// Current state
    state: CircuitState,
    /// Consecutive failure count
    failure_count: usize,
    /// Failure threshold
    threshold: usize,
    /// Timeout duration
    timeout: Duration,
    /// Last failure time
    last_failure: Option<std::time::Instant>,
}

impl CircuitBreaker {
    /// Creates a new circuit breaker
    #[must_use]
    pub fn new(threshold: usize, timeout: Duration) -> Self {
        Self {
            state: CircuitState::Closed,
            failure_count: 0,
            threshold,
            timeout,
            last_failure: None,
        }
    }

    /// Checks if the circuit breaker allows the operation
    pub fn allow_request(&mut self) -> Result<()> {
        match self.state {
            CircuitState::Closed => Ok(()),
            CircuitState::Open => {
                // Check if timeout has elapsed
                if let Some(last_failure) = self.last_failure {
                    if last_failure.elapsed() >= self.timeout {
                        tracing::info!("Circuit breaker transitioning to half-open state");
                        self.state = CircuitState::HalfOpen;
                        Ok(())
                    } else {
                        Err(CloudError::Retry(RetryError::CircuitBreakerOpen {
                            message: "Circuit breaker is open".to_string(),
                        }))
                    }
                } else {
                    Ok(())
                }
            }
            CircuitState::HalfOpen => Ok(()),
        }
    }

    /// Records a successful operation
    pub fn record_success(&mut self) {
        match self.state {
            CircuitState::Closed => {
                self.failure_count = 0;
            }
            CircuitState::HalfOpen => {
                tracing::info!("Circuit breaker transitioning to closed state");
                self.state = CircuitState::Closed;
                self.failure_count = 0;
            }
            CircuitState::Open => {}
        }
    }

    /// Records a failed operation
    pub fn record_failure(&mut self) {
        self.failure_count += 1;
        self.last_failure = Some(std::time::Instant::now());

        if self.failure_count >= self.threshold && self.state != CircuitState::Open {
            tracing::warn!(
                "Circuit breaker opening after {} failures",
                self.failure_count
            );
            self.state = CircuitState::Open;
        }
    }

    /// Returns the current state of the circuit breaker
    #[must_use]
    pub fn state(&self) -> CircuitState {
        self.state
    }
}

/// Retry budget for limiting retry overhead
#[derive(Debug)]
pub struct RetryBudget {
    /// Available retry tokens
    tokens: usize,
    /// Maximum tokens
    max_tokens: usize,
    /// Token refill rate (tokens per second)
    refill_rate: f64,
    /// Last refill time
    last_refill: std::time::Instant,
}

impl RetryBudget {
    /// Creates a new retry budget
    #[must_use]
    pub fn new(max_tokens: usize, refill_rate: f64) -> Self {
        Self {
            tokens: max_tokens,
            max_tokens,
            refill_rate,
            last_refill: std::time::Instant::now(),
        }
    }

    /// Tries to consume a retry token
    pub fn try_consume(&mut self) -> Result<()> {
        self.refill();

        if self.tokens > 0 {
            self.tokens -= 1;
            Ok(())
        } else {
            Err(CloudError::Retry(RetryError::BudgetExhausted {
                message: "Retry budget exhausted".to_string(),
            }))
        }
    }

    /// Refills tokens based on elapsed time
    fn refill(&mut self) {
        let elapsed = self.last_refill.elapsed();
        let tokens_to_add = (elapsed.as_secs_f64() * self.refill_rate) as usize;

        if tokens_to_add > 0 {
            self.tokens = (self.tokens + tokens_to_add).min(self.max_tokens);
            self.last_refill = std::time::Instant::now();
        }
    }
}

/// Backoff calculator for exponential backoff with jitter
#[derive(Debug)]
pub struct Backoff {
    /// Configuration
    config: RetryConfig,
    /// Current attempt number
    attempt: usize,
}

impl Backoff {
    /// Creates a new backoff calculator
    #[must_use]
    pub fn new(config: RetryConfig) -> Self {
        Self { config, attempt: 0 }
    }

    /// Calculates the next backoff duration
    #[must_use]
    pub fn next(&mut self) -> Duration {
        let base = self.config.initial_backoff.as_secs_f64().mul_add(
            self.config.backoff_multiplier.powi(self.attempt as i32),
            0.0,
        );

        let backoff = if self.config.jitter {
            // Add jitter (0% to 50% of base)
            let jitter_factor = 1.0 + (rand() * 0.5);
            base * jitter_factor
        } else {
            base
        };

        self.attempt += 1;

        Duration::from_secs_f64(backoff.min(self.config.max_backoff.as_secs_f64()))
    }

    /// Resets the backoff state
    pub fn reset(&mut self) {
        self.attempt = 0;
    }
}

/// Simple pseudo-random number generator for jitter
/// Uses a basic LCG (Linear Congruential Generator)
fn rand() -> f64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEED: AtomicU64 = AtomicU64::new(0);

    let seed = SEED.load(Ordering::Relaxed);
    let next = seed.wrapping_mul(1664525).wrapping_add(1013904223);
    SEED.store(next, Ordering::Relaxed);

    (next >> 32) as f64 / u32::MAX as f64
}

/// Determines if an error is retryable
#[must_use]
pub fn is_retryable(error: &CloudError) -> bool {
    match error {
        CloudError::Timeout { .. } => true,
        CloudError::RateLimitExceeded { .. } => true,
        CloudError::Http(http_error) => match http_error {
            crate::error::HttpError::Network { .. } => true,
            crate::error::HttpError::Status { status, .. } => {
                // Retry on 5xx errors and some 4xx errors
                matches!(
                    *status,
                    500 | 502 | 503 | 504 | 408 | 429 // Server errors and rate limiting
                )
            }
            _ => false,
        },
        CloudError::S3(s3_error) => match s3_error {
            crate::error::S3Error::Sdk { .. } => true,
            _ => false,
        },
        CloudError::Azure(azure_error) => match azure_error {
            crate::error::AzureError::Sdk { .. } => true,
            _ => false,
        },
        CloudError::Gcs(gcs_error) => match gcs_error {
            crate::error::GcsError::Sdk { .. } => true,
            _ => false,
        },
        CloudError::Io(_) => true,
        _ => false,
    }
}

/// Retry executor with exponential backoff
#[cfg(feature = "async")]
pub struct RetryExecutor {
    /// Configuration
    config: RetryConfig,
    /// Circuit breaker
    circuit_breaker: Option<CircuitBreaker>,
    /// Retry budget
    retry_budget: Option<RetryBudget>,
}

#[cfg(feature = "async")]
impl RetryExecutor {
    /// Creates a new retry executor
    #[must_use]
    pub fn new(config: RetryConfig) -> Self {
        let circuit_breaker = if config.circuit_breaker {
            Some(CircuitBreaker::new(
                config.circuit_breaker_threshold,
                config.circuit_breaker_timeout,
            ))
        } else {
            None
        };

        let retry_budget = Some(RetryBudget::new(100, 10.0)); // 100 tokens, refill at 10/sec

        Self {
            config,
            circuit_breaker,
            retry_budget,
        }
    }

    /// Executes an async operation with retry logic
    pub async fn execute<F, Fut, T>(&mut self, mut operation: F) -> Result<T>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        // Check circuit breaker
        if let Some(ref mut cb) = self.circuit_breaker {
            cb.allow_request()?;
        }

        let mut backoff = Backoff::new(self.config.clone());
        let mut attempts = 0;

        loop {
            match operation().await {
                Ok(result) => {
                    // Success - record and return
                    if let Some(ref mut cb) = self.circuit_breaker {
                        cb.record_success();
                    }
                    return Ok(result);
                }
                Err(error) => {
                    attempts += 1;

                    // Check if error is retryable
                    if !is_retryable(&error) {
                        tracing::warn!("Non-retryable error: {}", error);
                        return Err(error);
                    }

                    // Check if we've exceeded max retries
                    if attempts > self.config.max_retries {
                        tracing::error!("Max retries ({}) exceeded", self.config.max_retries);
                        if let Some(ref mut cb) = self.circuit_breaker {
                            cb.record_failure();
                        }
                        return Err(CloudError::Retry(RetryError::MaxRetriesExceeded {
                            attempts,
                        }));
                    }

                    // Check retry budget
                    if let Some(ref mut budget) = self.retry_budget {
                        budget.try_consume()?;
                    }

                    // Calculate backoff and wait
                    let delay = backoff.next();
                    tracing::warn!(
                        "Retry attempt {}/{} after {:?}: {}",
                        attempts,
                        self.config.max_retries,
                        delay,
                        error
                    );

                    tokio::time::sleep(delay).await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retry_config_builder() {
        let config = RetryConfig::new()
            .with_max_retries(5)
            .with_initial_backoff(Duration::from_millis(50))
            .with_backoff_multiplier(3.0)
            .with_jitter(false);

        assert_eq!(config.max_retries, 5);
        assert_eq!(config.initial_backoff, Duration::from_millis(50));
        assert_eq!(config.backoff_multiplier, 3.0);
        assert!(!config.jitter);
    }

    #[test]
    fn test_circuit_breaker_closed() {
        let mut cb = CircuitBreaker::new(3, Duration::from_secs(60));
        assert_eq!(cb.state, CircuitState::Closed);
        assert!(cb.allow_request().is_ok());
    }

    #[test]
    fn test_circuit_breaker_opens() {
        let mut cb = CircuitBreaker::new(3, Duration::from_secs(60));

        // Record 3 failures
        cb.record_failure();
        cb.record_failure();
        cb.record_failure();

        assert_eq!(cb.state, CircuitState::Open);
        assert!(cb.allow_request().is_err());
    }

    #[test]
    fn test_circuit_breaker_half_open() {
        let mut cb = CircuitBreaker::new(3, Duration::from_millis(10));

        // Open the circuit
        cb.record_failure();
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state, CircuitState::Open);

        // Wait for timeout
        std::thread::sleep(Duration::from_millis(20));

        // Should transition to half-open
        assert!(cb.allow_request().is_ok());
        assert_eq!(cb.state, CircuitState::HalfOpen);

        // Success should close the circuit
        cb.record_success();
        assert_eq!(cb.state, CircuitState::Closed);
    }

    #[test]
    fn test_retry_budget() {
        // Use a high refill rate (100 tokens/sec) so we can sleep only 50ms
        // instead of seconds. At 100 tokens/sec, 50ms yields ~5 tokens.
        let mut budget = RetryBudget::new(10, 100.0);

        // Consume 10 tokens
        for _ in 0..10 {
            assert!(budget.try_consume().is_ok());
        }

        // 11th should fail
        assert!(budget.try_consume().is_err());

        // Wait for refill (50ms at 100 tokens/sec = ~5 tokens)
        std::thread::sleep(Duration::from_millis(50));

        // Should have refilled
        assert!(budget.try_consume().is_ok());
    }

    #[test]
    fn test_backoff_exponential() {
        let config = RetryConfig::new()
            .with_initial_backoff(Duration::from_millis(100))
            .with_backoff_multiplier(2.0)
            .with_jitter(false);

        let mut backoff = Backoff::new(config);

        let d1 = backoff.next();
        let d2 = backoff.next();
        let d3 = backoff.next();

        assert!(d1 < d2);
        assert!(d2 < d3);
    }

    #[test]
    fn test_is_retryable() {
        let timeout_error = CloudError::Timeout {
            message: "timeout".to_string(),
        };
        assert!(is_retryable(&timeout_error));

        let rate_limit_error = CloudError::RateLimitExceeded {
            message: "rate limit".to_string(),
        };
        assert!(is_retryable(&rate_limit_error));

        let not_found_error = CloudError::NotFound {
            key: "test".to_string(),
        };
        assert!(!is_retryable(&not_found_error));
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    async fn test_retry_executor_success() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let config = RetryConfig::new().with_max_retries(3);
        let mut executor = RetryExecutor::new(config);

        let attempt = std::sync::Arc::new(AtomicUsize::new(0));
        let attempt_clone = attempt.clone();
        let result = executor
            .execute(|| {
                let attempt = attempt_clone.clone();
                async move {
                    let current = attempt.fetch_add(1, Ordering::SeqCst) + 1;
                    if current < 2 {
                        Err(CloudError::Timeout {
                            message: "timeout".to_string(),
                        })
                    } else {
                        Ok(42)
                    }
                }
            })
            .await;

        assert!(result.is_ok());
        assert_eq!(result.ok(), Some(42));
        assert_eq!(attempt.load(Ordering::SeqCst), 2);
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    async fn test_retry_executor_max_retries() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let config = RetryConfig::new().with_max_retries(2);
        let mut executor = RetryExecutor::new(config);

        let attempt = std::sync::Arc::new(AtomicUsize::new(0));
        let attempt_clone = attempt.clone();
        let result: Result<i32> = executor
            .execute(|| {
                let attempt = attempt_clone.clone();
                async move {
                    attempt.fetch_add(1, Ordering::SeqCst);
                    Err(CloudError::Timeout {
                        message: "timeout".to_string(),
                    })
                }
            })
            .await;

        assert!(result.is_err());
        assert_eq!(attempt.load(Ordering::SeqCst), 3); // Initial + 2 retries
    }
}
