//! Retry policy with exponential backoff

use crate::error::StorageError;
use std::time::{Duration, Instant};

// ============================================================================
// Retry Policy
// ============================================================================

/// Retry policy with exponential backoff
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// Maximum number of retries
    pub max_retries: u32,
    /// Base delay for exponential backoff
    pub base_delay: Duration,
    /// Maximum delay for exponential backoff
    pub max_delay: Duration,
    /// Jitter factor (0.0 - 1.0) for randomizing delays
    pub jitter_factor: f64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: super::config::DEFAULT_MAX_RETRIES,
            base_delay: Duration::from_millis(super::config::DEFAULT_BASE_DELAY_MS),
            max_delay: Duration::from_millis(super::config::DEFAULT_MAX_DELAY_MS),
            jitter_factor: 0.25,
        }
    }
}

impl RetryPolicy {
    /// Creates a new retry policy
    #[must_use]
    pub fn new(max_retries: u32, base_delay: Duration, max_delay: Duration) -> Self {
        Self {
            max_retries,
            base_delay,
            max_delay,
            jitter_factor: 0.25,
        }
    }

    /// Sets the jitter factor
    #[must_use]
    pub fn with_jitter(mut self, factor: f64) -> Self {
        self.jitter_factor = factor.clamp(0.0, 1.0);
        self
    }

    /// Calculates the delay for a given retry attempt
    #[must_use]
    pub fn calculate_delay(&self, attempt: u32) -> Duration {
        if attempt == 0 {
            return Duration::ZERO;
        }

        // Calculate exponential delay: base_delay * 2^(attempt - 1)
        let exp_factor = 1u64 << attempt.saturating_sub(1).min(31);
        let base_ms = self.base_delay.as_millis() as u64;
        let delay_ms = base_ms.saturating_mul(exp_factor);

        // Clamp to max delay
        let max_ms = self.max_delay.as_millis() as u64;
        let clamped_ms = delay_ms.min(max_ms);

        // Apply jitter using a simple deterministic approach
        // For true randomness, use a proper RNG
        let jitter_range = (clamped_ms as f64 * self.jitter_factor) as u64;
        let jitter_offset = if jitter_range > 0 {
            // Simple deterministic jitter based on attempt number
            (attempt as u64 * 17) % (jitter_range * 2)
        } else {
            0
        };

        let final_ms = if jitter_offset > jitter_range {
            clamped_ms.saturating_add(jitter_offset - jitter_range)
        } else {
            clamped_ms.saturating_sub(jitter_range - jitter_offset)
        };

        Duration::from_millis(final_ms.max(1))
    }

    /// Checks if an error is retryable
    #[must_use]
    pub fn is_retryable(&self, error: &StorageError) -> bool {
        match error {
            StorageError::Network { .. } => true,
            StorageError::Http { status, .. } => is_retryable_status(*status),
            _ => false,
        }
    }
}

/// Checks if an HTTP status code indicates a retryable error
#[must_use]
pub fn is_retryable_status(status: u16) -> bool {
    matches!(
        status,
        408 | // Request Timeout
        429 | // Too Many Requests
        500 | // Internal Server Error
        502 | // Bad Gateway
        503 | // Service Unavailable
        504 // Gateway Timeout
    )
}

/// Retry context for tracking retry state
#[derive(Debug)]
pub struct RetryContext {
    /// Retry policy
    policy: RetryPolicy,
    /// Current attempt number (0-based)
    attempt: u32,
    /// Time of first attempt
    started_at: Instant,
    /// Total retry time spent
    total_delay: Duration,
}

impl RetryContext {
    /// Creates a new retry context
    #[must_use]
    pub fn new(policy: RetryPolicy) -> Self {
        Self {
            policy,
            attempt: 0,
            started_at: Instant::now(),
            total_delay: Duration::ZERO,
        }
    }

    /// Checks if another retry should be attempted
    #[must_use]
    pub fn should_retry(&self, error: &StorageError) -> bool {
        self.attempt < self.policy.max_retries && self.policy.is_retryable(error)
    }

    /// Records a retry attempt and returns the delay to wait
    pub fn record_retry(&mut self) -> Duration {
        self.attempt += 1;
        let delay = self.policy.calculate_delay(self.attempt);
        self.total_delay += delay;
        delay
    }

    /// Returns the current attempt number
    #[must_use]
    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    /// Returns the total time elapsed since first attempt
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }

    /// Returns the total delay time spent on retries
    #[must_use]
    pub fn total_delay(&self) -> Duration {
        self.total_delay
    }
}
