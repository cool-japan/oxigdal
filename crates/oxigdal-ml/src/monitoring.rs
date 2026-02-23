//! Model monitoring and performance tracking
//!
//! This module provides comprehensive monitoring capabilities for ML models
//! in production, including performance metrics, drift detection, and alerting.

use std::collections::VecDeque;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

/// Model performance metrics
#[derive(Debug, Clone)]
pub struct PerformanceMetrics {
    /// Total inference count
    pub total_inferences: u64,
    /// Average latency in milliseconds
    pub avg_latency_ms: f32,
    /// P50 latency in milliseconds
    pub p50_latency_ms: f32,
    /// P95 latency in milliseconds
    pub p95_latency_ms: f32,
    /// P99 latency in milliseconds
    pub p99_latency_ms: f32,
    /// Throughput (inferences per second)
    pub throughput: f32,
    /// Error rate (0.0 to 1.0)
    pub error_rate: f32,
}

/// Model drift metrics
#[derive(Debug, Clone)]
pub struct DriftMetrics {
    /// Input distribution drift score
    pub input_drift: f32,
    /// Output distribution drift score
    pub output_drift: f32,
    /// Concept drift detected
    pub concept_drift: bool,
    /// Data quality score (0.0 to 1.0)
    pub data_quality: f32,
}

/// Alert severity level
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertSeverity {
    /// Informational
    Info,
    /// Warning - needs attention
    Warning,
    /// Critical - immediate action required
    Critical,
}

/// Model alert
#[derive(Debug, Clone)]
pub struct ModelAlert {
    /// Alert timestamp
    pub timestamp: Instant,
    /// Alert severity
    pub severity: AlertSeverity,
    /// Alert message
    pub message: String,
    /// Metric name that triggered the alert
    pub metric: String,
    /// Threshold value
    pub threshold: f32,
    /// Actual value
    pub actual: f32,
}

/// Monitoring configuration
#[derive(Debug, Clone)]
pub struct MonitoringConfig {
    /// Enable performance monitoring
    pub enable_performance: bool,
    /// Enable drift detection
    pub enable_drift: bool,
    /// Enable alerting
    pub enable_alerting: bool,
    /// Latency alert threshold (ms)
    pub latency_threshold_ms: f32,
    /// Error rate alert threshold
    pub error_rate_threshold: f32,
    /// Drift alert threshold
    pub drift_threshold: f32,
    /// Metric retention period
    pub retention_period: Duration,
}

impl Default for MonitoringConfig {
    fn default() -> Self {
        Self {
            enable_performance: true,
            enable_drift: true,
            enable_alerting: true,
            latency_threshold_ms: 1000.0,
            error_rate_threshold: 0.05,
            drift_threshold: 0.2,
            retention_period: Duration::from_secs(3600), // 1 hour
        }
    }
}

/// Model monitor
pub struct ModelMonitor {
    config: MonitoringConfig,
    latencies: VecDeque<f32>,
    errors: VecDeque<bool>,
    alerts: Vec<ModelAlert>,
    start_time: Instant,
}

impl ModelMonitor {
    /// Creates a new model monitor
    #[must_use]
    pub fn new(config: MonitoringConfig) -> Self {
        info!("Initializing model monitor");
        Self {
            config,
            latencies: VecDeque::new(),
            errors: VecDeque::new(),
            alerts: Vec::new(),
            start_time: Instant::now(),
        }
    }

    /// Records an inference latency
    pub fn record_latency(&mut self, latency_ms: f32) {
        if !self.config.enable_performance {
            return;
        }

        self.latencies.push_back(latency_ms);
        self.trim_old_metrics();

        // Check for latency alerts
        if self.config.enable_alerting && latency_ms > self.config.latency_threshold_ms {
            self.add_alert(ModelAlert {
                timestamp: Instant::now(),
                severity: if latency_ms > self.config.latency_threshold_ms * 2.0 {
                    AlertSeverity::Critical
                } else {
                    AlertSeverity::Warning
                },
                message: format!("High latency detected: {:.1}ms", latency_ms),
                metric: "latency_ms".to_string(),
                threshold: self.config.latency_threshold_ms,
                actual: latency_ms,
            });
        }
    }

    /// Records an inference error
    pub fn record_error(&mut self, is_error: bool) {
        if !self.config.enable_performance {
            return;
        }

        self.errors.push_back(is_error);
        self.trim_old_metrics();

        // Check for error rate alerts
        if self.config.enable_alerting && is_error {
            let error_rate = self.calculate_error_rate();
            if error_rate > self.config.error_rate_threshold {
                self.add_alert(ModelAlert {
                    timestamp: Instant::now(),
                    severity: AlertSeverity::Critical,
                    message: format!("High error rate: {:.1}%", error_rate * 100.0),
                    metric: "error_rate".to_string(),
                    threshold: self.config.error_rate_threshold,
                    actual: error_rate,
                });
            }
        }
    }

    /// Records drift metrics
    pub fn record_drift(&mut self, metrics: DriftMetrics) {
        if !self.config.enable_drift {
            return;
        }

        debug!(
            "Drift metrics: input={:.3}, output={:.3}, concept={}",
            metrics.input_drift, metrics.output_drift, metrics.concept_drift
        );

        // Check for drift alerts
        if self.config.enable_alerting {
            if metrics.input_drift > self.config.drift_threshold {
                self.add_alert(ModelAlert {
                    timestamp: Instant::now(),
                    severity: AlertSeverity::Warning,
                    message: "Input distribution drift detected".to_string(),
                    metric: "input_drift".to_string(),
                    threshold: self.config.drift_threshold,
                    actual: metrics.input_drift,
                });
            }

            if metrics.output_drift > self.config.drift_threshold {
                self.add_alert(ModelAlert {
                    timestamp: Instant::now(),
                    severity: AlertSeverity::Warning,
                    message: "Output distribution drift detected".to_string(),
                    metric: "output_drift".to_string(),
                    threshold: self.config.drift_threshold,
                    actual: metrics.output_drift,
                });
            }

            if metrics.concept_drift {
                self.add_alert(ModelAlert {
                    timestamp: Instant::now(),
                    severity: AlertSeverity::Critical,
                    message: "Concept drift detected - model retraining recommended".to_string(),
                    metric: "concept_drift".to_string(),
                    threshold: 0.0,
                    actual: 1.0,
                });
            }
        }
    }

    /// Calculates performance metrics
    #[must_use]
    pub fn performance_metrics(&self) -> PerformanceMetrics {
        let total_inferences = self.latencies.len() as u64;
        let avg_latency = self.calculate_average_latency();
        let percentiles = self.calculate_latency_percentiles();
        let throughput = self.calculate_throughput();
        let error_rate = self.calculate_error_rate();

        PerformanceMetrics {
            total_inferences,
            avg_latency_ms: avg_latency,
            p50_latency_ms: percentiles.0,
            p95_latency_ms: percentiles.1,
            p99_latency_ms: percentiles.2,
            throughput,
            error_rate,
        }
    }

    /// Returns all alerts
    #[must_use]
    pub fn alerts(&self) -> &[ModelAlert] {
        &self.alerts
    }

    /// Returns alerts by severity
    #[must_use]
    pub fn alerts_by_severity(&self, severity: AlertSeverity) -> Vec<&ModelAlert> {
        self.alerts
            .iter()
            .filter(|a| a.severity == severity)
            .collect()
    }

    /// Clears all alerts
    pub fn clear_alerts(&mut self) {
        info!("Clearing {} alerts", self.alerts.len());
        self.alerts.clear();
    }

    /// Resets all metrics
    pub fn reset(&mut self) {
        info!("Resetting monitor metrics");
        self.latencies.clear();
        self.errors.clear();
        self.alerts.clear();
        self.start_time = Instant::now();
    }

    // Private helper methods

    fn trim_old_metrics(&mut self) {
        let max_samples = 10000; // Keep last 10k samples
        while self.latencies.len() > max_samples {
            self.latencies.pop_front();
        }
        while self.errors.len() > max_samples {
            self.errors.pop_front();
        }
    }

    fn add_alert(&mut self, alert: ModelAlert) {
        match alert.severity {
            AlertSeverity::Info => debug!("Alert: {}", alert.message),
            AlertSeverity::Warning => warn!("Alert: {}", alert.message),
            AlertSeverity::Critical => {
                warn!("CRITICAL Alert: {}", alert.message);
            }
        }
        self.alerts.push(alert);
    }

    fn calculate_average_latency(&self) -> f32 {
        if self.latencies.is_empty() {
            return 0.0;
        }
        let sum: f32 = self.latencies.iter().sum();
        sum / self.latencies.len() as f32
    }

    fn calculate_latency_percentiles(&self) -> (f32, f32, f32) {
        if self.latencies.is_empty() {
            return (0.0, 0.0, 0.0);
        }

        let mut sorted: Vec<_> = self.latencies.iter().copied().collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let p50 = sorted[sorted.len() * 50 / 100];
        let p95 = sorted[sorted.len() * 95 / 100];
        let p99 = sorted[sorted.len() * 99 / 100];

        (p50, p95, p99)
    }

    fn calculate_throughput(&self) -> f32 {
        let elapsed = self.start_time.elapsed().as_secs_f32();
        if elapsed > 0.0 {
            self.latencies.len() as f32 / elapsed
        } else {
            0.0
        }
    }

    fn calculate_error_rate(&self) -> f32 {
        if self.errors.is_empty() {
            return 0.0;
        }
        let error_count = self.errors.iter().filter(|&&e| e).count();
        error_count as f32 / self.errors.len() as f32
    }
}

/// Calculates input drift using KL divergence
#[must_use]
pub fn calculate_input_drift(reference_distribution: &[f32], current_distribution: &[f32]) -> f32 {
    if reference_distribution.len() != current_distribution.len() {
        return 1.0; // Maximum drift
    }

    let mut divergence = 0.0;
    for (p, q) in reference_distribution
        .iter()
        .zip(current_distribution.iter())
    {
        if *p > 0.0 && *q > 0.0 {
            divergence += p * (p / q).ln();
        }
    }

    divergence
}

/// Calculates output drift using distribution shift
#[must_use]
pub fn calculate_output_drift(reference_predictions: &[f32], current_predictions: &[f32]) -> f32 {
    if reference_predictions.len() != current_predictions.len() {
        return 1.0;
    }

    let ref_mean = reference_predictions.iter().sum::<f32>() / reference_predictions.len() as f32;
    let cur_mean = current_predictions.iter().sum::<f32>() / current_predictions.len() as f32;

    (ref_mean - cur_mean).abs() / ref_mean.max(1e-6)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_monitoring_config_default() {
        let config = MonitoringConfig::default();
        assert!(config.enable_performance);
        assert!(config.enable_drift);
        assert!(config.enable_alerting);
    }

    #[test]
    fn test_model_monitor_latency() {
        let config = MonitoringConfig::default();
        let mut monitor = ModelMonitor::new(config);

        monitor.record_latency(100.0);
        monitor.record_latency(150.0);
        monitor.record_latency(120.0);

        let metrics = monitor.performance_metrics();
        assert_eq!(metrics.total_inferences, 3);
        assert!((metrics.avg_latency_ms - 123.33).abs() < 1.0);
    }

    #[test]
    fn test_model_monitor_errors() {
        let config = MonitoringConfig::default();
        let mut monitor = ModelMonitor::new(config);

        monitor.record_error(false);
        monitor.record_error(false);
        monitor.record_error(true);
        monitor.record_error(false);

        let metrics = monitor.performance_metrics();
        assert!((metrics.error_rate - 0.25).abs() < 0.01);
    }

    #[test]
    fn test_alert_filtering() {
        let config = MonitoringConfig {
            latency_threshold_ms: 100.0,
            ..Default::default()
        };
        let mut monitor = ModelMonitor::new(config);

        monitor.record_latency(150.0); // Should trigger warning
        monitor.record_latency(250.0); // Should trigger critical

        let warnings = monitor.alerts_by_severity(AlertSeverity::Warning);
        let criticals = monitor.alerts_by_severity(AlertSeverity::Critical);

        assert_eq!(warnings.len(), 1);
        assert_eq!(criticals.len(), 1);
    }

    #[test]
    fn test_input_drift_calculation() {
        let reference = vec![0.25, 0.25, 0.25, 0.25];
        let current = vec![0.3, 0.2, 0.3, 0.2];

        let drift = calculate_input_drift(&reference, &current);
        assert!(drift > 0.0);
        assert!(drift < 1.0);
    }

    #[test]
    fn test_output_drift_calculation() {
        let reference = vec![0.8, 0.7, 0.9, 0.75];
        let current = vec![0.6, 0.5, 0.7, 0.55]; // 20% lower

        let drift = calculate_output_drift(&reference, &current);
        assert!(drift > 0.15);
        assert!(drift < 0.30);
    }
}
