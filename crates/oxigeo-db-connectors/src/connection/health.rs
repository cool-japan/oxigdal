//! Connection health checking.

use std::time::{Duration, Instant};

/// Health check result.
#[derive(Debug, Clone)]
pub struct HealthCheckResult {
    /// Whether the connection is healthy.
    pub healthy: bool,
    /// Response time.
    pub response_time: Duration,
    /// Error message if unhealthy.
    pub error: Option<String>,
    /// Timestamp of the check.
    pub timestamp: Instant,
}

impl HealthCheckResult {
    /// Create a healthy result.
    pub fn healthy(response_time: Duration) -> Self {
        Self {
            healthy: true,
            response_time,
            error: None,
            timestamp: Instant::now(),
        }
    }

    /// Create an unhealthy result.
    pub fn unhealthy(error: String, response_time: Duration) -> Self {
        Self {
            healthy: false,
            response_time,
            error: Some(error),
            timestamp: Instant::now(),
        }
    }

    /// Check if the result is healthy.
    pub fn is_healthy(&self) -> bool {
        self.healthy
    }

    /// Get error message if unhealthy.
    pub fn error_message(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

/// Health check configuration.
#[derive(Debug, Clone)]
pub struct HealthCheckConfig {
    /// Interval between health checks.
    pub check_interval: Duration,
    /// Timeout for health checks.
    pub check_timeout: Duration,
    /// Number of consecutive failures before marking as unhealthy.
    pub failure_threshold: u32,
    /// Number of consecutive successes before marking as healthy.
    pub success_threshold: u32,
}

impl Default for HealthCheckConfig {
    fn default() -> Self {
        Self {
            check_interval: Duration::from_secs(30),
            check_timeout: Duration::from_secs(5),
            failure_threshold: 3,
            success_threshold: 2,
        }
    }
}

impl HealthCheckConfig {
    /// Create a new health check configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set check interval.
    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.check_interval = interval;
        self
    }

    /// Set check timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.check_timeout = timeout;
        self
    }

    /// Set failure threshold.
    pub fn with_failure_threshold(mut self, threshold: u32) -> Self {
        self.failure_threshold = threshold;
        self
    }

    /// Set success threshold.
    pub fn with_success_threshold(mut self, threshold: u32) -> Self {
        self.success_threshold = threshold;
        self
    }
}

/// Health tracker for monitoring connection health over time.
#[derive(Debug)]
pub struct HealthTracker {
    config: HealthCheckConfig,
    consecutive_failures: u32,
    consecutive_successes: u32,
    is_healthy: bool,
}

impl HealthTracker {
    /// Create a new health tracker.
    pub fn new(config: HealthCheckConfig) -> Self {
        Self {
            config,
            consecutive_failures: 0,
            consecutive_successes: 0,
            is_healthy: true,
        }
    }

    /// Record a successful health check.
    pub fn record_success(&mut self) {
        self.consecutive_successes += 1;
        self.consecutive_failures = 0;

        if self.consecutive_successes >= self.config.success_threshold {
            self.is_healthy = true;
        }
    }

    /// Record a failed health check.
    pub fn record_failure(&mut self) {
        self.consecutive_failures += 1;
        self.consecutive_successes = 0;

        if self.consecutive_failures >= self.config.failure_threshold {
            self.is_healthy = false;
        }
    }

    /// Check if currently healthy.
    pub fn is_healthy(&self) -> bool {
        self.is_healthy
    }

    /// Get consecutive failure count.
    pub fn consecutive_failures(&self) -> u32 {
        self.consecutive_failures
    }

    /// Get consecutive success count.
    pub fn consecutive_successes(&self) -> u32 {
        self.consecutive_successes
    }

    /// Reset the tracker.
    pub fn reset(&mut self) {
        self.consecutive_failures = 0;
        self.consecutive_successes = 0;
        self.is_healthy = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_tracker() {
        let config = HealthCheckConfig::default();
        let mut tracker = HealthTracker::new(config);

        assert!(tracker.is_healthy());

        // Record failures
        tracker.record_failure();
        tracker.record_failure();
        assert!(tracker.is_healthy()); // Still healthy (threshold is 3)

        tracker.record_failure();
        assert!(!tracker.is_healthy()); // Now unhealthy

        // Record successes
        tracker.record_success();
        tracker.record_success();
        assert!(tracker.is_healthy()); // Healthy again (threshold is 2)
    }
}
