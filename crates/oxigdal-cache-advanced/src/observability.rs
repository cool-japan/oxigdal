//! Cache observability and monitoring
//!
//! Provides comprehensive monitoring capabilities:
//! - Detailed metrics (latency percentiles, throughput)
//! - Cache heat map visualization
//! - Performance regression detection
//! - Real-time alerting
//! - Distributed tracing integration

use crate::multi_tier::CacheKey;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Latency percentile statistics
#[derive(Debug, Clone)]
pub struct LatencyStats {
    /// Sorted latency samples (microseconds)
    samples: VecDeque<u64>,
    /// Maximum samples to keep
    max_samples: usize,
}

impl LatencyStats {
    /// Create new latency stats
    pub fn new(max_samples: usize) -> Self {
        Self {
            samples: VecDeque::with_capacity(max_samples),
            max_samples,
        }
    }

    /// Record latency sample
    pub fn record(&mut self, latency_us: u64) {
        if self.samples.len() >= self.max_samples {
            self.samples.pop_front();
        }
        self.samples.push_back(latency_us);
    }

    /// Calculate percentile
    pub fn percentile(&self, p: f64) -> Option<u64> {
        if self.samples.is_empty() {
            return None;
        }

        let mut sorted: Vec<u64> = self.samples.iter().copied().collect();
        sorted.sort_unstable();

        let index = ((sorted.len() as f64 * p / 100.0).ceil() as usize).saturating_sub(1);
        sorted.get(index).copied()
    }

    /// Get p50 (median)
    pub fn p50(&self) -> Option<u64> {
        self.percentile(50.0)
    }

    /// Get p95
    pub fn p95(&self) -> Option<u64> {
        self.percentile(95.0)
    }

    /// Get p99
    pub fn p99(&self) -> Option<u64> {
        self.percentile(99.0)
    }

    /// Get minimum latency
    pub fn min(&self) -> Option<u64> {
        self.samples.iter().min().copied()
    }

    /// Get maximum latency
    pub fn max(&self) -> Option<u64> {
        self.samples.iter().max().copied()
    }

    /// Get average latency
    pub fn avg(&self) -> Option<f64> {
        if self.samples.is_empty() {
            None
        } else {
            let sum: u64 = self.samples.iter().sum();
            Some(sum as f64 / self.samples.len() as f64)
        }
    }
}

/// Throughput tracker
#[derive(Debug, Clone)]
pub struct ThroughputTracker {
    /// Request counts in time windows
    windows: VecDeque<(Instant, u64)>,
    /// Window duration
    window_duration: Duration,
    /// Maximum windows to keep
    max_windows: usize,
}

impl ThroughputTracker {
    /// Create new throughput tracker
    pub fn new(window_duration: Duration, max_windows: usize) -> Self {
        Self {
            windows: VecDeque::with_capacity(max_windows),
            window_duration,
            max_windows,
        }
    }

    /// Record request
    pub fn record(&mut self) {
        let now = Instant::now();

        // Clean old windows
        while let Some((ts, _)) = self.windows.front() {
            if now.duration_since(*ts) > self.window_duration * self.max_windows as u32 {
                self.windows.pop_front();
            } else {
                break;
            }
        }

        // Update or create current window
        if let Some((ts, count)) = self.windows.back_mut() {
            if now.duration_since(*ts) < self.window_duration {
                *count += 1;
                return;
            }
        }

        if self.windows.len() >= self.max_windows {
            self.windows.pop_front();
        }
        self.windows.push_back((now, 1));
    }

    /// Calculate requests per second
    pub fn requests_per_second(&self) -> f64 {
        if self.windows.is_empty() {
            return 0.0;
        }

        let total_requests: u64 = self.windows.iter().map(|(_, count)| count).sum();
        let total_duration =
            if let (Some(first), Some(last)) = (self.windows.front(), self.windows.back()) {
                last.0.duration_since(first.0).as_secs_f64()
            } else {
                return 0.0;
            };

        if total_duration > 0.0 {
            total_requests as f64 / total_duration
        } else {
            total_requests as f64
        }
    }

    /// Get peak throughput
    pub fn peak_throughput(&self) -> u64 {
        self.windows
            .iter()
            .map(|(_, count)| count)
            .max()
            .copied()
            .unwrap_or(0)
    }
}

/// Cache heat map for visualizing access patterns
#[derive(Debug, Clone)]
pub struct HeatMapEntry {
    /// Access count
    pub access_count: u64,
    /// Last access time
    pub last_access: Instant,
    /// Total bytes accessed
    pub bytes_accessed: u64,
}

impl HeatMapEntry {
    /// Create new entry
    pub fn new() -> Self {
        Self {
            access_count: 0,
            last_access: Instant::now(),
            bytes_accessed: 0,
        }
    }

    /// Record access
    pub fn record_access(&mut self, bytes: u64) {
        self.access_count += 1;
        self.last_access = Instant::now();
        self.bytes_accessed += bytes;
    }

    /// Calculate heat score (0.0 to 1.0)
    pub fn heat_score(&self, max_accesses: u64, max_age: Duration) -> f64 {
        let frequency_score = if max_accesses > 0 {
            (self.access_count as f64 / max_accesses as f64).min(1.0)
        } else {
            0.0
        };

        let age = self.last_access.elapsed();
        let recency_score = if age < max_age {
            1.0 - (age.as_secs_f64() / max_age.as_secs_f64())
        } else {
            0.0
        };

        (frequency_score * 0.6 + recency_score * 0.4).min(1.0)
    }
}

impl Default for HeatMapEntry {
    fn default() -> Self {
        Self::new()
    }
}

/// Cache heat map
pub struct CacheHeatMap {
    /// Heat map entries
    entries: Arc<RwLock<HashMap<CacheKey, HeatMapEntry>>>,
    /// Maximum age for recency calculation
    max_age: Duration,
}

impl CacheHeatMap {
    /// Create new heat map
    pub fn new(max_age: Duration) -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
            max_age,
        }
    }

    /// Record access
    pub async fn record_access(&self, key: CacheKey, bytes: u64) {
        let mut entries = self.entries.write().await;
        entries
            .entry(key)
            .or_insert_with(HeatMapEntry::new)
            .record_access(bytes);
    }

    /// Get heat scores for all keys
    pub async fn get_heat_scores(&self) -> HashMap<CacheKey, f64> {
        let entries = self.entries.read().await;

        let max_accesses = entries.values().map(|e| e.access_count).max().unwrap_or(1);

        entries
            .iter()
            .map(|(k, e)| (k.clone(), e.heat_score(max_accesses, self.max_age)))
            .collect()
    }

    /// Get hottest keys
    pub async fn get_hot_keys(&self, limit: usize) -> Vec<(CacheKey, f64)> {
        let scores = self.get_heat_scores().await;
        let mut sorted: Vec<_> = scores.into_iter().collect();
        sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        sorted.truncate(limit);
        sorted
    }

    /// Clear old entries
    pub async fn cleanup(&self, max_age: Duration) {
        let mut entries = self.entries.write().await;
        entries.retain(|_, e| e.last_access.elapsed() < max_age);
    }
}

/// Performance regression detector
pub struct RegressionDetector {
    /// Historical baseline (metric -> value)
    baseline: Arc<RwLock<HashMap<String, f64>>>,
    /// Recent measurements
    recent: Arc<RwLock<HashMap<String, VecDeque<f64>>>>,
    /// Regression threshold (percentage)
    threshold: f64,
    /// Window size for recent measurements
    window_size: usize,
}

impl RegressionDetector {
    /// Create new regression detector
    pub fn new(threshold: f64, window_size: usize) -> Self {
        Self {
            baseline: Arc::new(RwLock::new(HashMap::new())),
            recent: Arc::new(RwLock::new(HashMap::new())),
            threshold,
            window_size,
        }
    }

    /// Set baseline for metric
    pub async fn set_baseline(&self, metric: String, value: f64) {
        self.baseline.write().await.insert(metric, value);
    }

    /// Record measurement
    pub async fn record(&self, metric: String, value: f64) {
        let mut recent = self.recent.write().await;
        let measurements = recent
            .entry(metric)
            .or_insert_with(|| VecDeque::with_capacity(self.window_size));

        if measurements.len() >= self.window_size {
            measurements.pop_front();
        }
        measurements.push_back(value);
    }

    /// Detect regression
    pub async fn detect_regression(&self, metric: &str) -> Option<f64> {
        let baseline = self.baseline.read().await;
        let recent = self.recent.read().await;

        let baseline_value = baseline.get(metric)?;
        let measurements = recent.get(metric)?;

        if measurements.is_empty() {
            return None;
        }

        // Calculate recent average
        let recent_avg: f64 = measurements.iter().sum::<f64>() / measurements.len() as f64;

        // Calculate regression percentage
        let regression = if *baseline_value > 0.0 {
            ((recent_avg - baseline_value) / baseline_value) * 100.0
        } else {
            0.0
        };

        // Return regression if it exceeds threshold
        if regression > self.threshold {
            Some(regression)
        } else {
            None
        }
    }

    /// Get all detected regressions
    pub async fn get_regressions(&self) -> HashMap<String, f64> {
        let baseline = self.baseline.read().await;
        let mut regressions = HashMap::new();

        for metric in baseline.keys() {
            if let Some(regression) = self.detect_regression(metric).await {
                regressions.insert(metric.clone(), regression);
            }
        }

        regressions
    }
}

/// Alert rule
#[derive(Debug, Clone)]
pub struct AlertRule {
    /// Metric name
    pub metric: String,
    /// Threshold value
    pub threshold: f64,
    /// Comparison operator
    pub operator: ComparisonOp,
    /// Minimum duration before alerting
    pub duration: Duration,
}

/// Comparison operator
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComparisonOp {
    /// Greater than
    GreaterThan,
    /// Less than
    LessThan,
    /// Equal to
    EqualTo,
}

impl ComparisonOp {
    /// Evaluate comparison
    pub fn evaluate(&self, value: f64, threshold: f64) -> bool {
        match self {
            ComparisonOp::GreaterThan => value > threshold,
            ComparisonOp::LessThan => value < threshold,
            ComparisonOp::EqualTo => (value - threshold).abs() < f64::EPSILON,
        }
    }
}

/// Real-time alerting system
pub struct AlertManager {
    /// Alert rules
    rules: Arc<RwLock<Vec<AlertRule>>>,
    /// Active alerts (metric -> start time)
    active_alerts: Arc<RwLock<HashMap<String, Instant>>>,
}

impl AlertManager {
    /// Create new alert manager
    pub fn new() -> Self {
        Self {
            rules: Arc::new(RwLock::new(Vec::new())),
            active_alerts: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add alert rule
    pub async fn add_rule(&self, rule: AlertRule) {
        self.rules.write().await.push(rule);
    }

    /// Evaluate metrics against rules
    pub async fn evaluate(&self, metrics: &HashMap<String, f64>) -> Vec<String> {
        let rules = self.rules.read().await;
        let mut active = self.active_alerts.write().await;
        let mut triggered = Vec::new();

        for rule in rules.iter() {
            if let Some(&value) = metrics.get(&rule.metric) {
                if rule.operator.evaluate(value, rule.threshold) {
                    // Check duration
                    let start_time = active
                        .entry(rule.metric.clone())
                        .or_insert_with(Instant::now);

                    if start_time.elapsed() >= rule.duration {
                        triggered.push(format!(
                            "Alert: {} = {} (threshold: {})",
                            rule.metric, value, rule.threshold
                        ));
                    }
                } else {
                    // Condition no longer met, clear alert
                    active.remove(&rule.metric);
                }
            }
        }

        triggered
    }

    /// Get active alerts
    pub async fn get_active_alerts(&self) -> Vec<String> {
        self.active_alerts.read().await.keys().cloned().collect()
    }
}

impl Default for AlertManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_latency_stats() {
        let mut stats = LatencyStats::new(100);

        for i in 1..=100 {
            stats.record(i * 10);
        }

        assert_eq!(stats.min(), Some(10));
        assert_eq!(stats.max(), Some(1000));
        assert!(stats.p50().is_some());
        assert!(stats.p95().is_some());
        assert!(stats.p99().is_some());
    }

    #[test]
    fn test_throughput_tracker() {
        let mut tracker = ThroughputTracker::new(Duration::from_secs(1), 10);

        for _ in 0..100 {
            tracker.record();
        }

        let rps = tracker.requests_per_second();
        assert!(rps > 0.0);
    }

    #[tokio::test]
    async fn test_heat_map() {
        let heat_map = CacheHeatMap::new(Duration::from_secs(60));

        heat_map.record_access("key1".to_string(), 1024).await;
        heat_map.record_access("key1".to_string(), 1024).await;
        heat_map.record_access("key2".to_string(), 512).await;

        let hot_keys = heat_map.get_hot_keys(2).await;
        assert!(!hot_keys.is_empty());
    }

    #[tokio::test]
    async fn test_regression_detector() {
        let detector = RegressionDetector::new(10.0, 5);

        detector.set_baseline("latency".to_string(), 100.0).await;

        for _ in 0..5 {
            detector.record("latency".to_string(), 120.0).await;
        }

        let regression = detector.detect_regression("latency").await;
        assert!(regression.is_some());
    }

    #[tokio::test]
    async fn test_alert_manager() {
        let manager = AlertManager::new();

        let rule = AlertRule {
            metric: "error_rate".to_string(),
            threshold: 5.0,
            operator: ComparisonOp::GreaterThan,
            duration: Duration::from_secs(0),
        };

        manager.add_rule(rule).await;

        let mut metrics = HashMap::new();
        metrics.insert("error_rate".to_string(), 10.0);

        let alerts = manager.evaluate(&metrics).await;
        assert!(!alerts.is_empty());
    }
}
