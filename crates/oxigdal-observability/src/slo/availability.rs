//! Availability calculations and tracking for SLO/SLA monitoring.
//!
//! This module provides comprehensive availability tracking based on
//! request success/failure rates, uptime monitoring, and error tracking.

use crate::error::{ObservabilityError, Result};
use chrono::{DateTime, Duration, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Default availability target (99.9% - "three nines").
pub const DEFAULT_AVAILABILITY_TARGET: f64 = 99.9;

/// Availability sample representing a single measurement period.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AvailabilitySample {
    /// Timestamp of the sample.
    pub timestamp: DateTime<Utc>,
    /// Total requests in this period.
    pub total_requests: u64,
    /// Successful requests in this period.
    pub successful_requests: u64,
    /// Failed requests in this period.
    pub failed_requests: u64,
    /// Availability percentage for this period.
    pub availability_pct: f64,
}

impl AvailabilitySample {
    /// Create a new availability sample.
    pub fn new(
        timestamp: DateTime<Utc>,
        total_requests: u64,
        successful_requests: u64,
    ) -> Self {
        let failed_requests = total_requests.saturating_sub(successful_requests);
        let availability_pct = if total_requests > 0 {
            (successful_requests as f64 / total_requests as f64) * 100.0
        } else {
            100.0 // No requests = 100% available (no failures)
        };

        Self {
            timestamp,
            total_requests,
            successful_requests,
            failed_requests,
            availability_pct,
        }
    }
}

/// Availability status at a point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailabilityStatus {
    /// Service name.
    pub service_name: String,
    /// Current availability percentage.
    pub current_availability: f64,
    /// Target availability percentage.
    pub target_availability: f64,
    /// Whether the SLO is currently met.
    pub is_slo_met: bool,
    /// Total requests in the evaluation window.
    pub total_requests: u64,
    /// Successful requests in the evaluation window.
    pub successful_requests: u64,
    /// Failed requests in the evaluation window.
    pub failed_requests: u64,
    /// Time window for this evaluation.
    pub window_duration: Duration,
    /// Evaluation timestamp.
    pub evaluated_at: DateTime<Utc>,
}

/// Availability tracker for monitoring service availability.
pub struct AvailabilityTracker {
    /// Service name being tracked.
    service_name: String,
    /// Target availability percentage.
    target_availability: f64,
    /// Total request count.
    total_requests: AtomicU64,
    /// Successful request count.
    successful_requests: AtomicU64,
    /// Failed request count.
    failed_requests: AtomicU64,
    /// Availability samples over time.
    samples: Arc<RwLock<VecDeque<AvailabilitySample>>>,
    /// Maximum number of samples to retain.
    max_samples: usize,
    /// Time window for sample retention.
    retention_duration: Option<Duration>,
}

impl AvailabilityTracker {
    /// Create a new availability tracker with the given service name.
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            target_availability: DEFAULT_AVAILABILITY_TARGET,
            total_requests: AtomicU64::new(0),
            successful_requests: AtomicU64::new(0),
            failed_requests: AtomicU64::new(0),
            samples: Arc::new(RwLock::new(VecDeque::with_capacity(1000))),
            max_samples: 1000,
            retention_duration: Some(Duration::days(30)),
        }
    }

    /// Set the target availability percentage.
    pub fn with_target(mut self, target_pct: f64) -> Self {
        self.target_availability = target_pct;
        self
    }

    /// Set the maximum number of samples to retain.
    pub fn with_max_samples(mut self, max_samples: usize) -> Self {
        self.max_samples = max_samples;
        self
    }

    /// Set the retention duration.
    pub fn with_retention(mut self, duration: Duration) -> Self {
        self.retention_duration = Some(duration);
        self
    }

    /// Get the service name.
    pub fn service_name(&self) -> &str {
        &self.service_name
    }

    /// Get the target availability.
    pub fn target_availability(&self) -> f64 {
        self.target_availability
    }

    /// Record a successful request.
    pub fn record_success(&self) {
        self.total_requests.fetch_add(1, Ordering::SeqCst);
        self.successful_requests.fetch_add(1, Ordering::SeqCst);
    }

    /// Record a failed request.
    pub fn record_failure(&self) {
        self.total_requests.fetch_add(1, Ordering::SeqCst);
        self.failed_requests.fetch_add(1, Ordering::SeqCst);
    }

    /// Record a request with explicit success/failure status.
    pub fn record_request(&self, is_success: bool) {
        if is_success {
            self.record_success();
        } else {
            self.record_failure();
        }
    }

    /// Record multiple requests at once.
    pub fn record_batch(&self, total: u64, successful: u64) {
        self.total_requests.fetch_add(total, Ordering::SeqCst);
        self.successful_requests.fetch_add(successful, Ordering::SeqCst);
        let failed = total.saturating_sub(successful);
        self.failed_requests.fetch_add(failed, Ordering::SeqCst);
    }

    /// Take a snapshot and store it as a sample.
    pub fn take_snapshot(&self) -> AvailabilitySample {
        let total = self.total_requests.load(Ordering::SeqCst);
        let successful = self.successful_requests.load(Ordering::SeqCst);

        let sample = AvailabilitySample::new(Utc::now(), total, successful);

        let mut samples = self.samples.write();

        // Remove old samples based on retention
        if let Some(retention) = self.retention_duration {
            let cutoff = Utc::now() - retention;
            while samples.front().map_or(false, |s| s.timestamp < cutoff) {
                samples.pop_front();
            }
        }

        // Ensure we don't exceed max samples
        while samples.len() >= self.max_samples {
            samples.pop_front();
        }

        samples.push_back(sample);

        sample
    }

    /// Reset counters after taking a snapshot (for period-based tracking).
    pub fn reset_counters(&self) {
        self.total_requests.store(0, Ordering::SeqCst);
        self.successful_requests.store(0, Ordering::SeqCst);
        self.failed_requests.store(0, Ordering::SeqCst);
    }

    /// Calculate current availability percentage.
    pub fn current_availability(&self) -> f64 {
        let total = self.total_requests.load(Ordering::SeqCst);
        let successful = self.successful_requests.load(Ordering::SeqCst);

        if total == 0 {
            return 100.0;
        }

        (successful as f64 / total as f64) * 100.0
    }

    /// Check if the current availability meets the SLO.
    pub fn is_slo_met(&self) -> bool {
        self.current_availability() >= self.target_availability
    }

    /// Get the current availability status.
    pub fn get_status(&self) -> AvailabilityStatus {
        let total = self.total_requests.load(Ordering::SeqCst);
        let successful = self.successful_requests.load(Ordering::SeqCst);
        let failed = self.failed_requests.load(Ordering::SeqCst);
        let availability = self.current_availability();

        AvailabilityStatus {
            service_name: self.service_name.clone(),
            current_availability: availability,
            target_availability: self.target_availability,
            is_slo_met: availability >= self.target_availability,
            total_requests: total,
            successful_requests: successful,
            failed_requests: failed,
            window_duration: self.retention_duration.unwrap_or_else(|| Duration::days(30)),
            evaluated_at: Utc::now(),
        }
    }

    /// Calculate availability over a rolling window.
    pub fn availability_in_window(&self, window: Duration) -> Result<f64> {
        let samples = self.samples.read();
        let cutoff = Utc::now() - window;

        let (total, successful) = samples
            .iter()
            .filter(|s| s.timestamp >= cutoff)
            .fold((0u64, 0u64), |(t, s), sample| {
                (t + sample.total_requests, s + sample.successful_requests)
            });

        if total == 0 {
            return Err(ObservabilityError::SloCalculationError(
                "No samples available in the specified window".to_string(),
            ));
        }

        Ok((successful as f64 / total as f64) * 100.0)
    }

    /// Get historical samples.
    pub fn get_samples(&self) -> Vec<AvailabilitySample> {
        self.samples.read().iter().cloned().collect()
    }

    /// Get samples within a time range.
    pub fn samples_in_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Vec<AvailabilitySample> {
        self.samples
            .read()
            .iter()
            .filter(|s| s.timestamp >= start && s.timestamp <= end)
            .cloned()
            .collect()
    }

    /// Calculate downtime minutes based on failed requests.
    pub fn estimated_downtime_minutes(&self, total_window_minutes: f64) -> f64 {
        let availability = self.current_availability();
        let downtime_fraction = (100.0 - availability) / 100.0;
        total_window_minutes * downtime_fraction
    }

    /// Get error rate percentage.
    pub fn error_rate(&self) -> f64 {
        100.0 - self.current_availability()
    }

    /// Get the number of samples.
    pub fn sample_count(&self) -> usize {
        self.samples.read().len()
    }
}

impl Default for AvailabilityTracker {
    fn default() -> Self {
        Self::new("default_service")
    }
}

/// Composite availability calculator for multi-service scenarios.
pub struct CompositeAvailability {
    /// Individual service trackers.
    trackers: Vec<Arc<AvailabilityTracker>>,
}

impl CompositeAvailability {
    /// Create a new composite availability calculator.
    pub fn new() -> Self {
        Self {
            trackers: Vec::new(),
        }
    }

    /// Add a service tracker.
    pub fn add_tracker(&mut self, tracker: Arc<AvailabilityTracker>) {
        self.trackers.push(tracker);
    }

    /// Calculate overall availability (assuming serial dependency).
    pub fn serial_availability(&self) -> f64 {
        if self.trackers.is_empty() {
            return 100.0;
        }

        self.trackers
            .iter()
            .map(|t| t.current_availability() / 100.0)
            .product::<f64>()
            * 100.0
    }

    /// Calculate overall availability (assuming parallel redundancy).
    pub fn parallel_availability(&self) -> f64 {
        if self.trackers.is_empty() {
            return 100.0;
        }

        let failure_product: f64 = self
            .trackers
            .iter()
            .map(|t| 1.0 - (t.current_availability() / 100.0))
            .product();

        (1.0 - failure_product) * 100.0
    }

    /// Calculate weighted average availability.
    pub fn weighted_average(&self, weights: &[f64]) -> Result<f64> {
        if self.trackers.len() != weights.len() {
            return Err(ObservabilityError::SloCalculationError(
                "Weights count must match tracker count".to_string(),
            ));
        }

        if self.trackers.is_empty() {
            return Ok(100.0);
        }

        let total_weight: f64 = weights.iter().sum();
        if total_weight <= 0.0 {
            return Err(ObservabilityError::SloCalculationError(
                "Total weight must be positive".to_string(),
            ));
        }

        let weighted_sum: f64 = self
            .trackers
            .iter()
            .zip(weights.iter())
            .map(|(t, w)| t.current_availability() * w)
            .sum();

        Ok(weighted_sum / total_weight)
    }

    /// Get all service statuses.
    pub fn get_all_statuses(&self) -> Vec<AvailabilityStatus> {
        self.trackers.iter().map(|t| t.get_status()).collect()
    }
}

impl Default for CompositeAvailability {
    fn default() -> Self {
        Self::new()
    }
}

/// Uptime calculator based on time-based availability.
pub struct UptimeCalculator {
    /// Start time of the monitoring period.
    start_time: DateTime<Utc>,
    /// Total downtime duration.
    downtime_duration: Arc<RwLock<Duration>>,
    /// Current downtime start (if currently down).
    current_downtime_start: Arc<RwLock<Option<DateTime<Utc>>>>,
}

impl UptimeCalculator {
    /// Create a new uptime calculator.
    pub fn new() -> Self {
        Self {
            start_time: Utc::now(),
            downtime_duration: Arc::new(RwLock::new(Duration::zero())),
            current_downtime_start: Arc::new(RwLock::new(None)),
        }
    }

    /// Mark the start of a downtime period.
    pub fn mark_down(&self) {
        let mut start = self.current_downtime_start.write();
        if start.is_none() {
            *start = Some(Utc::now());
        }
    }

    /// Mark the end of a downtime period.
    pub fn mark_up(&self) {
        let mut start = self.current_downtime_start.write();
        if let Some(down_start) = start.take() {
            let downtime = Utc::now() - down_start;
            *self.downtime_duration.write() = *self.downtime_duration.read() + downtime;
        }
    }

    /// Check if currently down.
    pub fn is_down(&self) -> bool {
        self.current_downtime_start.read().is_some()
    }

    /// Get total downtime duration.
    pub fn total_downtime(&self) -> Duration {
        let mut total = *self.downtime_duration.read();
        if let Some(down_start) = *self.current_downtime_start.read() {
            total = total + (Utc::now() - down_start);
        }
        total
    }

    /// Get total uptime duration.
    pub fn total_uptime(&self) -> Duration {
        let total_duration = Utc::now() - self.start_time;
        total_duration - self.total_downtime()
    }

    /// Calculate uptime percentage.
    pub fn uptime_percentage(&self) -> f64 {
        let total_duration = (Utc::now() - self.start_time).num_seconds() as f64;
        if total_duration <= 0.0 {
            return 100.0;
        }

        let uptime_seconds = self.total_uptime().num_seconds() as f64;
        (uptime_seconds / total_duration) * 100.0
    }

    /// Reset the calculator.
    pub fn reset(&self) {
        *self.downtime_duration.write() = Duration::zero();
        *self.current_downtime_start.write() = None;
    }
}

impl Default for UptimeCalculator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_availability_sample() {
        let sample = AvailabilitySample::new(Utc::now(), 100, 99);
        assert_eq!(sample.total_requests, 100);
        assert_eq!(sample.successful_requests, 99);
        assert_eq!(sample.failed_requests, 1);
        assert_eq!(sample.availability_pct, 99.0);
    }

    #[test]
    fn test_availability_tracker() {
        let tracker = AvailabilityTracker::new("test_service").with_target(99.0);

        // Record 99 successes and 1 failure
        for _ in 0..99 {
            tracker.record_success();
        }
        tracker.record_failure();

        assert_eq!(tracker.current_availability(), 99.0);
        assert!(tracker.is_slo_met());

        let status = tracker.get_status();
        assert_eq!(status.total_requests, 100);
        assert_eq!(status.successful_requests, 99);
        assert_eq!(status.failed_requests, 1);
    }

    #[test]
    fn test_composite_availability() {
        let tracker1 = Arc::new(AvailabilityTracker::new("service1"));
        let tracker2 = Arc::new(AvailabilityTracker::new("service2"));

        // Service 1: 99% availability
        for _ in 0..99 {
            tracker1.record_success();
        }
        tracker1.record_failure();

        // Service 2: 99% availability
        for _ in 0..99 {
            tracker2.record_success();
        }
        tracker2.record_failure();

        let mut composite = CompositeAvailability::new();
        composite.add_tracker(tracker1);
        composite.add_tracker(tracker2);

        // Serial: 99% * 99% = 98.01%
        let serial = composite.serial_availability();
        assert!((serial - 98.01).abs() < 0.01);

        // Parallel: 1 - (0.01 * 0.01) = 99.99%
        let parallel = composite.parallel_availability();
        assert!((parallel - 99.99).abs() < 0.01);
    }

    #[test]
    fn test_uptime_calculator() {
        let calculator = UptimeCalculator::new();

        assert!(!calculator.is_down());
        assert!(calculator.uptime_percentage() > 99.9);

        calculator.mark_down();
        assert!(calculator.is_down());

        calculator.mark_up();
        assert!(!calculator.is_down());
    }

    #[test]
    fn test_batch_recording() {
        let tracker = AvailabilityTracker::new("batch_test");
        tracker.record_batch(1000, 950);

        assert_eq!(tracker.current_availability(), 95.0);
        assert_eq!(tracker.error_rate(), 5.0);
    }
}
