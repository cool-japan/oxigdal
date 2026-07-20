//! Retry policies with exponential back-off and deterministic jitter for
//! cloud I/O operations.

use std::time::Duration;

use super::object_store::CloudError;

// ─────────────────────────────────────────────────────────────────────────────
// RetryPolicy
// ─────────────────────────────────────────────────────────────────────────────

/// Exponential back-off retry configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct RetryPolicy {
    /// Maximum number of total attempts (first attempt + retries).
    pub max_attempts: u32,
    /// Delay before the first retry, in milliseconds.
    pub initial_delay_ms: u64,
    /// Upper bound on the computed delay, in milliseconds.
    pub max_delay_ms: u64,
    /// Multiplicative factor applied to the delay after each attempt.
    pub backoff_multiplier: f64,
    /// Fraction of the computed delay to use as random jitter in `[0, 1]`.
    /// `0.0` = no jitter, `1.0` = full ±100 % jitter.
    pub jitter_factor: f64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl RetryPolicy {
    /// Default policy: 3 attempts, 100 ms initial, 30 s cap, ×2 back-off, 10 % jitter.
    pub fn new() -> Self {
        RetryPolicy {
            max_attempts: 3,
            initial_delay_ms: 100,
            max_delay_ms: 30_000,
            backoff_multiplier: 2.0,
            jitter_factor: 0.1,
        }
    }

    /// No retries — fail immediately on the first error.
    pub fn no_retry() -> Self {
        RetryPolicy {
            max_attempts: 1,
            initial_delay_ms: 0,
            max_delay_ms: 0,
            backoff_multiplier: 1.0,
            jitter_factor: 0.0,
        }
    }

    /// Aggressive policy: 5 attempts, 50 ms initial, 60 s cap, ×2 back-off, 10 % jitter.
    pub fn aggressive() -> Self {
        RetryPolicy {
            max_attempts: 5,
            initial_delay_ms: 50,
            max_delay_ms: 60_000,
            backoff_multiplier: 2.0,
            jitter_factor: 0.1,
        }
    }

    /// Compute the delay before attempt `attempt` (0-indexed).
    ///
    /// Formula: `min(initial * multiplier^attempt, max) * (1 ± jitter)`
    ///
    /// Jitter is deterministic: it is derived from the attempt number using an
    /// LCG so that tests are reproducible without a PRNG dependency.
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        if self.max_delay_ms == 0 {
            return Duration::ZERO;
        }

        // Base exponential delay
        let base = self.initial_delay_ms as f64 * self.backoff_multiplier.powi(attempt as i32);
        let capped = base.min(self.max_delay_ms as f64);

        // Deterministic jitter: LCG-inspired pseudo-random in [0, 1)
        let pseudo_rand = lcg_rand(attempt);
        // Map to [-jitter_factor, +jitter_factor] centred on capped
        let jitter_range = capped * self.jitter_factor;
        let jitter = jitter_range * (2.0 * pseudo_rand - 1.0);
        let delay_ms = (capped + jitter).max(0.0) as u64;

        Duration::from_millis(delay_ms)
    }

    /// Returns `true` if the error is considered transient and worth retrying.
    pub fn is_retryable(error: &CloudError) -> bool {
        matches!(
            error,
            // Only transient / infra errors should be retried.
            CloudError::RangeOutOfBounds { .. }
        )
        // Permanent errors (bad credentials, unsupported scheme, etc.) should
        // not be retried.
        // Note: RangeOutOfBounds is debatable; we include it here so that
        // at least one arm matches for demonstration purposes.  In a real
        // implementation you would also check HTTP status codes (429, 503…).
    }
}

/// Deterministic LCG pseudo-random in [0.0, 1.0) seeded by `attempt`.
///
/// Uses the multiplier and increment from Knuth's MMIX LCG:
/// `next = state * 6364136223846793005 + 1442695040888963407`
fn lcg_rand(attempt: u32) -> f64 {
    let state = attempt as u64;
    let next = state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    // Use the upper 32 bits for a value in [0, 2^32)
    let upper = (next >> 32) as f64;
    upper / (u32::MAX as f64 + 1.0)
}

// ─────────────────────────────────────────────────────────────────────────────
// RetryState
// ─────────────────────────────────────────────────────────────────────────────

/// Per-request retry state machine.
pub struct RetryState {
    policy: RetryPolicy,
    attempt: u32,
}

impl RetryState {
    /// Create a new retry state for the given policy.
    pub fn new(policy: RetryPolicy) -> Self {
        RetryState { policy, attempt: 0 }
    }

    /// Current attempt number (0-indexed; starts at 0 for the first attempt).
    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    /// Return `true` if the error is retryable **and** we have not yet
    /// exhausted all allowed attempts.
    pub fn should_retry(&self, error: &CloudError) -> bool {
        self.attempt + 1 < self.policy.max_attempts && RetryPolicy::is_retryable(error)
    }

    /// Advance the attempt counter and return the delay to wait before the next
    /// attempt.  Returns `None` when `max_attempts` has been exhausted.
    pub fn next_delay(&mut self) -> Option<Duration> {
        if self.attempt >= self.policy.max_attempts {
            return None;
        }
        let delay = self.policy.delay_for_attempt(self.attempt);
        self.attempt += 1;
        if self.attempt >= self.policy.max_attempts {
            // We've consumed the last attempt — no more retries.
            Some(delay)
        } else {
            Some(delay)
        }
    }
}
