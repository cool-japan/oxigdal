//! Latency percentile tracking and analysis.
//!
//! This module provides comprehensive latency monitoring with support for
//! multiple percentiles (p50, p95, p99, p999) and statistical analysis.

use crate::error::{ObservabilityError, Result};
use chrono::{DateTime, Duration, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;

/// Maximum number of samples to retain in the sliding window.
const DEFAULT_WINDOW_SIZE: usize = 10000;

/// Latency percentile configuration.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PercentileConfig {
    /// Percentile value (0.0 - 1.0).
    pub percentile: f64,
    /// Target latency threshold in milliseconds.
    pub target_ms: f64,
    /// Label for this percentile (e.g., "p50", "p95").
    pub label: &'static str,
}

impl PercentileConfig {
    /// P50 (median) configuration.
    pub const fn p50(target_ms: f64) -> Self {
        Self {
            percentile: 0.50,
            target_ms,
            label: "p50",
        }
    }

    /// P90 configuration.
    pub const fn p90(target_ms: f64) -> Self {
        Self {
            percentile: 0.90,
            target_ms,
            label: "p90",
        }
    }

    /// P95 configuration.
    pub const fn p95(target_ms: f64) -> Self {
        Self {
            percentile: 0.95,
            target_ms,
            label: "p95",
        }
    }

    /// P99 configuration.
    pub const fn p99(target_ms: f64) -> Self {
        Self {
            percentile: 0.99,
            target_ms,
            label: "p99",
        }
    }

    /// P999 configuration.
    pub const fn p999(target_ms: f64) -> Self {
        Self {
            percentile: 0.999,
            target_ms,
            label: "p999",
        }
    }
}

/// Latency sample with timestamp.
#[derive(Debug, Clone, Copy)]
struct LatencySample {
    /// Latency value in milliseconds.
    value_ms: f64,
    /// Timestamp when the sample was recorded.
    timestamp: DateTime<Utc>,
}

/// Latency percentile result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PercentileResult {
    /// Percentile label (e.g., "p95").
    pub label: String,
    /// Percentile value (0.0 - 1.0).
    pub percentile: f64,
    /// Actual latency value in milliseconds.
    pub actual_ms: f64,
    /// Target latency threshold in milliseconds.
    pub target_ms: f64,
    /// Whether the SLO is met.
    pub is_met: bool,
    /// Sample count used for calculation.
    pub sample_count: usize,
}

/// Latency statistics summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyStats {
    /// Minimum latency in milliseconds.
    pub min_ms: f64,
    /// Maximum latency in milliseconds.
    pub max_ms: f64,
    /// Mean latency in milliseconds.
    pub mean_ms: f64,
    /// Standard deviation in milliseconds.
    pub std_dev_ms: f64,
    /// Variance in milliseconds squared.
    pub variance_ms2: f64,
    /// Number of samples.
    pub sample_count: usize,
    /// Timestamp of oldest sample.
    pub oldest_sample: Option<DateTime<Utc>>,
    /// Timestamp of newest sample.
    pub newest_sample: Option<DateTime<Utc>>,
}

impl Default for LatencyStats {
    fn default() -> Self {
        Self {
            min_ms: f64::INFINITY,
            max_ms: f64::NEG_INFINITY,
            mean_ms: 0.0,
            std_dev_ms: 0.0,
            variance_ms2: 0.0,
            sample_count: 0,
            oldest_sample: None,
            newest_sample: None,
        }
    }
}

/// Latency tracker for monitoring service latencies.
pub struct LatencyTracker {
    /// Name of the service or endpoint being tracked.
    name: String,
    /// Sliding window of latency samples.
    samples: Arc<RwLock<VecDeque<LatencySample>>>,
    /// Maximum window size.
    window_size: usize,
    /// Configured percentiles to track.
    percentiles: Vec<PercentileConfig>,
    /// Time window for sample retention.
    retention_duration: Option<Duration>,
}

impl LatencyTracker {
    /// Create a new latency tracker with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            samples: Arc::new(RwLock::new(VecDeque::with_capacity(DEFAULT_WINDOW_SIZE))),
            window_size: DEFAULT_WINDOW_SIZE,
            percentiles: vec![
                PercentileConfig::p50(50.0),
                PercentileConfig::p95(100.0),
                PercentileConfig::p99(500.0),
            ],
            retention_duration: None,
        }
    }

    /// Set the maximum window size.
    pub fn with_window_size(mut self, size: usize) -> Self {
        self.window_size = size;
        self
    }

    /// Set the configured percentiles.
    pub fn with_percentiles(mut self, percentiles: Vec<PercentileConfig>) -> Self {
        self.percentiles = percentiles;
        self
    }

    /// Set the retention duration for samples.
    pub fn with_retention(mut self, duration: Duration) -> Self {
        self.retention_duration = Some(duration);
        self
    }

    /// Get the tracker name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Record a new latency sample.
    pub fn record(&self, latency_ms: f64) {
        let sample = LatencySample {
            value_ms: latency_ms,
            timestamp: Utc::now(),
        };

        let mut samples = self.samples.write();

        // Remove old samples if retention is configured
        if let Some(retention) = self.retention_duration {
            let cutoff = Utc::now() - retention;
            while samples.front().map_or(false, |s| s.timestamp < cutoff) {
                samples.pop_front();
            }
        }

        // Ensure we don't exceed the window size
        while samples.len() >= self.window_size {
            samples.pop_front();
        }

        samples.push_back(sample);
    }

    /// Record a latency sample with a specific timestamp.
    pub fn record_with_timestamp(&self, latency_ms: f64, timestamp: DateTime<Utc>) {
        let sample = LatencySample {
            value_ms: latency_ms,
            timestamp,
        };

        let mut samples = self.samples.write();

        // Ensure we don't exceed the window size
        while samples.len() >= self.window_size {
            samples.pop_front();
        }

        samples.push_back(sample);
    }

    /// Calculate a specific percentile.
    pub fn calculate_percentile(&self, percentile: f64) -> Result<f64> {
        if !(0.0..=1.0).contains(&percentile) {
            return Err(ObservabilityError::InvalidMetricValue(format!(
                "Percentile must be between 0 and 1, got {}",
                percentile
            )));
        }

        let samples = self.samples.read();
        if samples.is_empty() {
            return Err(ObservabilityError::SloCalculationError(
                "No samples available for percentile calculation".to_string(),
            ));
        }

        let mut values: Vec<f64> = samples.iter().map(|s| s.value_ms).collect();
        values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let index = ((values.len() as f64) * percentile).ceil() as usize;
        let index = index.saturating_sub(1).min(values.len() - 1);

        Ok(values[index])
    }

    /// Calculate all configured percentiles.
    pub fn calculate_all_percentiles(&self) -> Result<Vec<PercentileResult>> {
        let samples = self.samples.read();
        if samples.is_empty() {
            return Err(ObservabilityError::SloCalculationError(
                "No samples available for percentile calculation".to_string(),
            ));
        }

        let mut values: Vec<f64> = samples.iter().map(|s| s.value_ms).collect();
        values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let sample_count = values.len();

        let results = self
            .percentiles
            .iter()
            .map(|config| {
                let index = ((sample_count as f64) * config.percentile).ceil() as usize;
                let index = index.saturating_sub(1).min(sample_count - 1);
                let actual_ms = values[index];

                PercentileResult {
                    label: config.label.to_string(),
                    percentile: config.percentile,
                    actual_ms,
                    target_ms: config.target_ms,
                    is_met: actual_ms <= config.target_ms,
                    sample_count,
                }
            })
            .collect();

        Ok(results)
    }

    /// Calculate latency statistics.
    pub fn calculate_stats(&self) -> LatencyStats {
        let samples = self.samples.read();

        if samples.is_empty() {
            return LatencyStats::default();
        }

        let values: Vec<f64> = samples.iter().map(|s| s.value_ms).collect();
        let n = values.len() as f64;

        let min_ms = values
            .iter()
            .cloned()
            .fold(f64::INFINITY, |a, b| a.min(b));
        let max_ms = values
            .iter()
            .cloned()
            .fold(f64::NEG_INFINITY, |a, b| a.max(b));
        let sum: f64 = values.iter().sum();
        let mean_ms = sum / n;

        let variance_ms2 = values.iter().map(|v| (v - mean_ms).powi(2)).sum::<f64>() / n;
        let std_dev_ms = variance_ms2.sqrt();

        let oldest_sample = samples.front().map(|s| s.timestamp);
        let newest_sample = samples.back().map(|s| s.timestamp);

        LatencyStats {
            min_ms,
            max_ms,
            mean_ms,
            std_dev_ms,
            variance_ms2,
            sample_count: values.len(),
            oldest_sample,
            newest_sample,
        }
    }

    /// Get the sample count.
    pub fn sample_count(&self) -> usize {
        self.samples.read().len()
    }

    /// Clear all samples.
    pub fn clear(&self) {
        self.samples.write().clear();
    }

    /// Check if all configured SLOs are met.
    pub fn all_slos_met(&self) -> Result<bool> {
        let results = self.calculate_all_percentiles()?;
        Ok(results.iter().all(|r| r.is_met))
    }

    /// Get samples within a time range.
    pub fn samples_in_range(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Vec<(DateTime<Utc>, f64)> {
        self.samples
            .read()
            .iter()
            .filter(|s| s.timestamp >= start && s.timestamp <= end)
            .map(|s| (s.timestamp, s.value_ms))
            .collect()
    }
}

/// Latency histogram for bucketed latency tracking.
pub struct LatencyHistogram {
    /// Histogram name.
    name: String,
    /// Bucket boundaries in milliseconds.
    buckets: Vec<f64>,
    /// Counts per bucket.
    counts: Arc<RwLock<Vec<u64>>>,
    /// Total count.
    total_count: Arc<RwLock<u64>>,
    /// Sum of all values.
    sum: Arc<RwLock<f64>>,
}

impl LatencyHistogram {
    /// Create a new histogram with default buckets.
    pub fn new(name: impl Into<String>) -> Self {
        let default_buckets = vec![
            1.0, 5.0, 10.0, 25.0, 50.0, 75.0, 100.0, 250.0, 500.0, 750.0, 1000.0, 2500.0, 5000.0,
            10000.0,
        ];
        Self::with_buckets(name, default_buckets)
    }

    /// Create a histogram with custom buckets.
    pub fn with_buckets(name: impl Into<String>, buckets: Vec<f64>) -> Self {
        let bucket_count = buckets.len() + 1; // +1 for infinity bucket
        Self {
            name: name.into(),
            buckets,
            counts: Arc::new(RwLock::new(vec![0; bucket_count])),
            total_count: Arc::new(RwLock::new(0)),
            sum: Arc::new(RwLock::new(0.0)),
        }
    }

    /// Get the histogram name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Record a latency value.
    pub fn observe(&self, value_ms: f64) {
        let bucket_index = self
            .buckets
            .iter()
            .position(|&b| value_ms <= b)
            .unwrap_or(self.buckets.len());

        let mut counts = self.counts.write();
        counts[bucket_index] += 1;

        *self.total_count.write() += 1;
        *self.sum.write() += value_ms;
    }

    /// Get bucket counts.
    pub fn bucket_counts(&self) -> Vec<(f64, u64)> {
        let counts = self.counts.read();
        self.buckets
            .iter()
            .enumerate()
            .map(|(i, &boundary)| (boundary, counts[i]))
            .chain(std::iter::once((f64::INFINITY, counts[self.buckets.len()])))
            .collect()
    }

    /// Get total count.
    pub fn total_count(&self) -> u64 {
        *self.total_count.read()
    }

    /// Get sum of all values.
    pub fn sum(&self) -> f64 {
        *self.sum.read()
    }

    /// Get mean latency.
    pub fn mean(&self) -> Option<f64> {
        let count = *self.total_count.read();
        if count == 0 {
            return None;
        }
        Some(*self.sum.read() / count as f64)
    }

    /// Estimate percentile from histogram.
    pub fn estimate_percentile(&self, percentile: f64) -> Option<f64> {
        let total = *self.total_count.read();
        if total == 0 {
            return None;
        }

        let target_count = (total as f64 * percentile).ceil() as u64;
        let counts = self.counts.read();

        let mut cumulative = 0u64;
        for (i, &count) in counts.iter().enumerate() {
            cumulative += count;
            if cumulative >= target_count {
                return Some(if i < self.buckets.len() {
                    self.buckets[i]
                } else {
                    // Last bucket (infinity), use the last defined boundary
                    self.buckets.last().copied().unwrap_or(f64::INFINITY)
                });
            }
        }

        None
    }

    /// Reset the histogram.
    pub fn reset(&self) {
        let mut counts = self.counts.write();
        for count in counts.iter_mut() {
            *count = 0;
        }
        *self.total_count.write() = 0;
        *self.sum.write() = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_percentile_config() {
        let p50 = PercentileConfig::p50(50.0);
        assert_eq!(p50.percentile, 0.50);
        assert_eq!(p50.target_ms, 50.0);
        assert_eq!(p50.label, "p50");

        let p99 = PercentileConfig::p99(500.0);
        assert_eq!(p99.percentile, 0.99);
        assert_eq!(p99.target_ms, 500.0);
    }

    #[test]
    fn test_latency_tracker() {
        let tracker = LatencyTracker::new("test_service");

        // Record some samples
        for i in 1..=100 {
            tracker.record(i as f64);
        }

        assert_eq!(tracker.sample_count(), 100);

        // Test percentile calculation
        let p50 = tracker.calculate_percentile(0.50);
        assert!(p50.is_ok());
        let p50_value = p50.expect("p50 calculation failed");
        assert!(p50_value >= 50.0 && p50_value <= 51.0);

        let p99 = tracker.calculate_percentile(0.99);
        assert!(p99.is_ok());
        let p99_value = p99.expect("p99 calculation failed");
        assert!(p99_value >= 99.0);
    }

    #[test]
    fn test_latency_stats() {
        let tracker = LatencyTracker::new("test_stats");

        for i in 1..=100 {
            tracker.record(i as f64);
        }

        let stats = tracker.calculate_stats();
        assert_eq!(stats.sample_count, 100);
        assert_eq!(stats.min_ms, 1.0);
        assert_eq!(stats.max_ms, 100.0);
        assert!((stats.mean_ms - 50.5).abs() < 0.01);
    }

    #[test]
    fn test_latency_histogram() {
        let histogram = LatencyHistogram::new("test_histogram");

        // Record values
        for _ in 0..50 {
            histogram.observe(10.0);
        }
        for _ in 0..30 {
            histogram.observe(100.0);
        }
        for _ in 0..20 {
            histogram.observe(1000.0);
        }

        assert_eq!(histogram.total_count(), 100);

        // Test percentile estimation
        let p50 = histogram.estimate_percentile(0.50);
        assert!(p50.is_some());
        let p50_value = p50.expect("p50 estimation failed");
        assert!(p50_value <= 25.0, "p50 should be in the first bucket");
    }

    #[test]
    fn test_all_percentiles() {
        let tracker = LatencyTracker::new("multi_percentile")
            .with_percentiles(vec![
                PercentileConfig::p50(50.0),
                PercentileConfig::p95(95.0),
                PercentileConfig::p99(99.0),
            ]);

        for i in 1..=100 {
            tracker.record(i as f64);
        }

        let results = tracker.calculate_all_percentiles();
        assert!(results.is_ok());
        let results = results.expect("percentile calculation failed");
        assert_eq!(results.len(), 3);

        // All SLOs should be met with generous targets
        assert!(tracker.all_slos_met().expect("all_slos_met check failed"));
    }
}
