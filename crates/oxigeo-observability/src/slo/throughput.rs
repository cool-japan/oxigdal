//! Throughput monitoring for SLO/SLA tracking.
//!
//! This module provides comprehensive throughput monitoring including
//! requests per second (RPS), bytes per second, and operations per second.

use crate::error::{ObservabilityError, Result};
use chrono::{DateTime, Duration, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Default throughput measurement interval in seconds.
const DEFAULT_INTERVAL_SECONDS: i64 = 1;

/// Maximum samples to retain for rolling calculations.
const DEFAULT_MAX_SAMPLES: usize = 3600; // 1 hour at 1-second intervals

/// Throughput sample representing a single measurement period.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ThroughputSample {
    /// Timestamp of the sample.
    pub timestamp: DateTime<Utc>,
    /// Request count in this period.
    pub request_count: u64,
    /// Byte count in this period.
    pub byte_count: u64,
    /// Operation count in this period.
    pub operation_count: u64,
    /// Duration of the sample period in seconds.
    pub duration_seconds: f64,
    /// Requests per second.
    pub rps: f64,
    /// Bytes per second.
    pub bps: f64,
    /// Operations per second.
    pub ops: f64,
}

impl ThroughputSample {
    /// Create a new throughput sample.
    pub fn new(
        timestamp: DateTime<Utc>,
        request_count: u64,
        byte_count: u64,
        operation_count: u64,
        duration_seconds: f64,
    ) -> Self {
        let rps = if duration_seconds > 0.0 {
            request_count as f64 / duration_seconds
        } else {
            0.0
        };
        let bps = if duration_seconds > 0.0 {
            byte_count as f64 / duration_seconds
        } else {
            0.0
        };
        let ops = if duration_seconds > 0.0 {
            operation_count as f64 / duration_seconds
        } else {
            0.0
        };

        Self {
            timestamp,
            request_count,
            byte_count,
            operation_count,
            duration_seconds,
            rps,
            bps,
            ops,
        }
    }
}

/// Throughput SLO configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThroughputSlo {
    /// Minimum RPS target.
    pub min_rps: Option<f64>,
    /// Maximum RPS target (for rate limiting).
    pub max_rps: Option<f64>,
    /// Minimum bytes per second.
    pub min_bps: Option<f64>,
    /// Maximum bytes per second.
    pub max_bps: Option<f64>,
    /// Minimum operations per second.
    pub min_ops: Option<f64>,
    /// Maximum operations per second.
    pub max_ops: Option<f64>,
}

impl Default for ThroughputSlo {
    fn default() -> Self {
        Self {
            min_rps: None,
            max_rps: None,
            min_bps: None,
            max_bps: None,
            min_ops: None,
            max_ops: None,
        }
    }
}

impl ThroughputSlo {
    /// Create a new throughput SLO with minimum RPS.
    pub fn with_min_rps(mut self, min_rps: f64) -> Self {
        self.min_rps = Some(min_rps);
        self
    }

    /// Create a new throughput SLO with maximum RPS.
    pub fn with_max_rps(mut self, max_rps: f64) -> Self {
        self.max_rps = Some(max_rps);
        self
    }

    /// Create a new throughput SLO with minimum BPS.
    pub fn with_min_bps(mut self, min_bps: f64) -> Self {
        self.min_bps = Some(min_bps);
        self
    }

    /// Create a new throughput SLO with maximum BPS.
    pub fn with_max_bps(mut self, max_bps: f64) -> Self {
        self.max_bps = Some(max_bps);
        self
    }

    /// Check if a sample meets the SLO.
    pub fn is_met(&self, sample: &ThroughputSample) -> bool {
        if let Some(min_rps) = self.min_rps {
            if sample.rps < min_rps {
                return false;
            }
        }
        if let Some(max_rps) = self.max_rps {
            if sample.rps > max_rps {
                return false;
            }
        }
        if let Some(min_bps) = self.min_bps {
            if sample.bps < min_bps {
                return false;
            }
        }
        if let Some(max_bps) = self.max_bps {
            if sample.bps > max_bps {
                return false;
            }
        }
        if let Some(min_ops) = self.min_ops {
            if sample.ops < min_ops {
                return false;
            }
        }
        if let Some(max_ops) = self.max_ops {
            if sample.ops > max_ops {
                return false;
            }
        }
        true
    }
}

/// Throughput status summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThroughputStatus {
    /// Service name.
    pub service_name: String,
    /// Current RPS.
    pub current_rps: f64,
    /// Current BPS.
    pub current_bps: f64,
    /// Current OPS.
    pub current_ops: f64,
    /// Average RPS over the window.
    pub avg_rps: f64,
    /// Average BPS over the window.
    pub avg_bps: f64,
    /// Average OPS over the window.
    pub avg_ops: f64,
    /// Peak RPS in the window.
    pub peak_rps: f64,
    /// Peak BPS in the window.
    pub peak_bps: f64,
    /// Peak OPS in the window.
    pub peak_ops: f64,
    /// Whether SLO is met.
    pub slo_met: bool,
    /// Evaluation timestamp.
    pub evaluated_at: DateTime<Utc>,
}

/// Throughput tracker for monitoring request rates.
pub struct ThroughputTracker {
    /// Service name.
    service_name: String,
    /// Current period request count.
    request_count: AtomicU64,
    /// Current period byte count.
    byte_count: AtomicU64,
    /// Current period operation count.
    operation_count: AtomicU64,
    /// Start time of current measurement period.
    period_start: Arc<RwLock<DateTime<Utc>>>,
    /// Historical samples.
    samples: Arc<RwLock<VecDeque<ThroughputSample>>>,
    /// Maximum samples to retain.
    max_samples: usize,
    /// Throughput SLO configuration.
    slo: ThroughputSlo,
}

impl ThroughputTracker {
    /// Create a new throughput tracker.
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            request_count: AtomicU64::new(0),
            byte_count: AtomicU64::new(0),
            operation_count: AtomicU64::new(0),
            period_start: Arc::new(RwLock::new(Utc::now())),
            samples: Arc::new(RwLock::new(VecDeque::with_capacity(DEFAULT_MAX_SAMPLES))),
            max_samples: DEFAULT_MAX_SAMPLES,
            slo: ThroughputSlo::default(),
        }
    }

    /// Set the throughput SLO.
    pub fn with_slo(mut self, slo: ThroughputSlo) -> Self {
        self.slo = slo;
        self
    }

    /// Set the maximum samples to retain.
    pub fn with_max_samples(mut self, max_samples: usize) -> Self {
        self.max_samples = max_samples;
        self
    }

    /// Get the service name.
    pub fn service_name(&self) -> &str {
        &self.service_name
    }

    /// Record a request.
    pub fn record_request(&self) {
        self.request_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record bytes processed.
    pub fn record_bytes(&self, bytes: u64) {
        self.byte_count.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Record an operation.
    pub fn record_operation(&self) {
        self.operation_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a request with bytes.
    pub fn record_request_with_bytes(&self, bytes: u64) {
        self.record_request();
        self.record_bytes(bytes);
    }

    /// Record an operation with bytes.
    pub fn record_operation_with_bytes(&self, bytes: u64) {
        self.record_operation();
        self.record_bytes(bytes);
    }

    /// Take a sample and reset counters for the next period.
    pub fn take_sample(&self) -> ThroughputSample {
        let now = Utc::now();
        let request_count = self.request_count.swap(0, Ordering::SeqCst);
        let byte_count = self.byte_count.swap(0, Ordering::SeqCst);
        let operation_count = self.operation_count.swap(0, Ordering::SeqCst);

        let mut period_start = self.period_start.write();
        let duration = (now - *period_start).num_milliseconds() as f64 / 1000.0;
        *period_start = now;
        drop(period_start);

        let sample = ThroughputSample::new(now, request_count, byte_count, operation_count, duration);

        let mut samples = self.samples.write();
        while samples.len() >= self.max_samples {
            samples.pop_front();
        }
        samples.push_back(sample);

        sample
    }

    /// Get current instantaneous throughput (without resetting).
    pub fn current_throughput(&self) -> ThroughputSample {
        let now = Utc::now();
        let request_count = self.request_count.load(Ordering::Relaxed);
        let byte_count = self.byte_count.load(Ordering::Relaxed);
        let operation_count = self.operation_count.load(Ordering::Relaxed);

        let period_start = self.period_start.read();
        let duration = (now - *period_start).num_milliseconds() as f64 / 1000.0;

        ThroughputSample::new(now, request_count, byte_count, operation_count, duration)
    }

    /// Calculate average throughput over a time window.
    pub fn average_throughput(&self, window: Duration) -> Result<ThroughputSample> {
        let samples = self.samples.read();
        let cutoff = Utc::now() - window;

        let relevant_samples: Vec<_> = samples.iter().filter(|s| s.timestamp >= cutoff).collect();

        if relevant_samples.is_empty() {
            return Err(ObservabilityError::SloCalculationError(
                "No samples available in the specified window".to_string(),
            ));
        }

        let total_requests: u64 = relevant_samples.iter().map(|s| s.request_count).sum();
        let total_bytes: u64 = relevant_samples.iter().map(|s| s.byte_count).sum();
        let total_operations: u64 = relevant_samples.iter().map(|s| s.operation_count).sum();
        let total_duration: f64 = relevant_samples.iter().map(|s| s.duration_seconds).sum();

        Ok(ThroughputSample::new(
            Utc::now(),
            total_requests,
            total_bytes,
            total_operations,
            total_duration,
        ))
    }

    /// Get peak throughput in a time window.
    pub fn peak_throughput(&self, window: Duration) -> Result<ThroughputSample> {
        let samples = self.samples.read();
        let cutoff = Utc::now() - window;

        samples
            .iter()
            .filter(|s| s.timestamp >= cutoff)
            .max_by(|a, b| a.rps.partial_cmp(&b.rps).unwrap_or(std::cmp::Ordering::Equal))
            .cloned()
            .ok_or_else(|| {
                ObservabilityError::SloCalculationError(
                    "No samples available in the specified window".to_string(),
                )
            })
    }

    /// Get throughput status.
    pub fn get_status(&self) -> ThroughputStatus {
        let current = self.current_throughput();
        let samples = self.samples.read();

        let (avg_rps, avg_bps, avg_ops, peak_rps, peak_bps, peak_ops) = if samples.is_empty() {
            (
                current.rps,
                current.bps,
                current.ops,
                current.rps,
                current.bps,
                current.ops,
            )
        } else {
            let avg_rps = samples.iter().map(|s| s.rps).sum::<f64>() / samples.len() as f64;
            let avg_bps = samples.iter().map(|s| s.bps).sum::<f64>() / samples.len() as f64;
            let avg_ops = samples.iter().map(|s| s.ops).sum::<f64>() / samples.len() as f64;
            let peak_rps = samples
                .iter()
                .map(|s| s.rps)
                .fold(f64::NEG_INFINITY, f64::max);
            let peak_bps = samples
                .iter()
                .map(|s| s.bps)
                .fold(f64::NEG_INFINITY, f64::max);
            let peak_ops = samples
                .iter()
                .map(|s| s.ops)
                .fold(f64::NEG_INFINITY, f64::max);

            (avg_rps, avg_bps, avg_ops, peak_rps, peak_bps, peak_ops)
        };

        ThroughputStatus {
            service_name: self.service_name.clone(),
            current_rps: current.rps,
            current_bps: current.bps,
            current_ops: current.ops,
            avg_rps,
            avg_bps,
            avg_ops,
            peak_rps,
            peak_bps,
            peak_ops,
            slo_met: self.slo.is_met(&current),
            evaluated_at: Utc::now(),
        }
    }

    /// Check if current throughput meets SLO.
    pub fn is_slo_met(&self) -> bool {
        let current = self.current_throughput();
        self.slo.is_met(&current)
    }

    /// Get historical samples.
    pub fn get_samples(&self) -> Vec<ThroughputSample> {
        self.samples.read().iter().cloned().collect()
    }

    /// Get samples in a time range.
    pub fn samples_in_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Vec<ThroughputSample> {
        self.samples
            .read()
            .iter()
            .filter(|s| s.timestamp >= start && s.timestamp <= end)
            .cloned()
            .collect()
    }

    /// Calculate throughput trend (positive = increasing, negative = decreasing).
    pub fn calculate_trend(&self, window: Duration) -> Result<f64> {
        let samples = self.samples.read();
        let cutoff = Utc::now() - window;

        let relevant: Vec<_> = samples.iter().filter(|s| s.timestamp >= cutoff).collect();

        if relevant.len() < 2 {
            return Err(ObservabilityError::SloCalculationError(
                "Need at least 2 samples to calculate trend".to_string(),
            ));
        }

        // Simple linear regression on RPS
        let n = relevant.len() as f64;
        let x_values: Vec<f64> = (0..relevant.len()).map(|i| i as f64).collect();
        let y_values: Vec<f64> = relevant.iter().map(|s| s.rps).collect();

        let x_mean = x_values.iter().sum::<f64>() / n;
        let y_mean = y_values.iter().sum::<f64>() / n;

        let numerator: f64 = x_values
            .iter()
            .zip(y_values.iter())
            .map(|(x, y)| (x - x_mean) * (y - y_mean))
            .sum();

        let denominator: f64 = x_values.iter().map(|x| (x - x_mean).powi(2)).sum();

        if denominator.abs() < f64::EPSILON {
            return Ok(0.0);
        }

        Ok(numerator / denominator)
    }

    /// Get sample count.
    pub fn sample_count(&self) -> usize {
        self.samples.read().len()
    }

    /// Reset all counters and samples.
    pub fn reset(&self) {
        self.request_count.store(0, Ordering::SeqCst);
        self.byte_count.store(0, Ordering::SeqCst);
        self.operation_count.store(0, Ordering::SeqCst);
        *self.period_start.write() = Utc::now();
        self.samples.write().clear();
    }
}

impl Default for ThroughputTracker {
    fn default() -> Self {
        Self::new("default_service")
    }
}

/// Rate limiter based on throughput monitoring.
pub struct ThroughputRateLimiter {
    /// Tracker for monitoring throughput.
    tracker: Arc<ThroughputTracker>,
    /// Maximum allowed RPS.
    max_rps: f64,
    /// Burst allowance (requests that can exceed the rate).
    burst_allowance: u64,
    /// Remaining burst allowance.
    remaining_burst: AtomicU64,
}

impl ThroughputRateLimiter {
    /// Create a new rate limiter.
    pub fn new(tracker: Arc<ThroughputTracker>, max_rps: f64, burst_allowance: u64) -> Self {
        Self {
            tracker,
            max_rps,
            burst_allowance,
            remaining_burst: AtomicU64::new(burst_allowance),
        }
    }

    /// Check if a request should be allowed.
    pub fn should_allow(&self) -> bool {
        let current = self.tracker.current_throughput();

        if current.rps <= self.max_rps {
            // Replenish burst
            let current_burst = self.remaining_burst.load(Ordering::Relaxed);
            if current_burst < self.burst_allowance {
                self.remaining_burst
                    .store(self.burst_allowance, Ordering::Relaxed);
            }
            return true;
        }

        // Over limit, use burst if available
        let remaining = self.remaining_burst.load(Ordering::Relaxed);
        if remaining > 0 {
            self.remaining_burst.fetch_sub(1, Ordering::Relaxed);
            return true;
        }

        false
    }

    /// Get remaining burst capacity.
    pub fn remaining_burst(&self) -> u64 {
        self.remaining_burst.load(Ordering::Relaxed)
    }

    /// Get the maximum RPS limit.
    pub fn max_rps(&self) -> f64 {
        self.max_rps
    }

    /// Reset the rate limiter.
    pub fn reset(&self) {
        self.remaining_burst
            .store(self.burst_allowance, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_throughput_sample() {
        let sample = ThroughputSample::new(Utc::now(), 100, 1000, 50, 1.0);
        assert_eq!(sample.rps, 100.0);
        assert_eq!(sample.bps, 1000.0);
        assert_eq!(sample.ops, 50.0);
    }

    #[test]
    fn test_throughput_slo() {
        let slo = ThroughputSlo::default()
            .with_min_rps(50.0)
            .with_max_rps(1000.0);

        let good_sample = ThroughputSample::new(Utc::now(), 100, 0, 0, 1.0);
        assert!(slo.is_met(&good_sample));

        let low_sample = ThroughputSample::new(Utc::now(), 10, 0, 0, 1.0);
        assert!(!slo.is_met(&low_sample));

        let high_sample = ThroughputSample::new(Utc::now(), 2000, 0, 0, 1.0);
        assert!(!slo.is_met(&high_sample));
    }

    #[test]
    fn test_throughput_tracker() {
        let tracker = ThroughputTracker::new("test_service");

        // Record some requests
        for _ in 0..100 {
            tracker.record_request();
        }
        tracker.record_bytes(10000);

        let current = tracker.current_throughput();
        assert_eq!(current.request_count, 100);
        assert_eq!(current.byte_count, 10000);

        let sample = tracker.take_sample();
        assert_eq!(sample.request_count, 100);

        // After taking sample, counters should be reset
        let after = tracker.current_throughput();
        assert_eq!(after.request_count, 0);
    }

    #[test]
    fn test_throughput_status() {
        let tracker = ThroughputTracker::new("status_test").with_slo(
            ThroughputSlo::default()
                .with_min_rps(10.0)
                .with_max_rps(1000.0),
        );

        for _ in 0..50 {
            tracker.record_request();
        }

        let status = tracker.get_status();
        assert_eq!(status.service_name, "status_test");
    }

    #[test]
    fn test_rate_limiter() {
        let tracker = Arc::new(ThroughputTracker::new("rate_limited"));
        let limiter = ThroughputRateLimiter::new(tracker.clone(), 100.0, 10);

        // Should allow initially
        assert!(limiter.should_allow());

        // Should have burst capacity
        assert!(limiter.remaining_burst() > 0 || limiter.remaining_burst() == 10);
    }
}
