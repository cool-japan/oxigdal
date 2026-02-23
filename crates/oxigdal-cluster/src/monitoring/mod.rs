//! Advanced monitoring and alerting for cluster management.
//!
//! Provides real-time monitoring, custom metrics, alert rules, and anomaly detection.

use crate::error::{ClusterError, Result};
use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tracing::{debug, info, warn};

/// Metric identifier.
pub type MetricId = String;

/// Alert identifier.
pub type AlertId = uuid::Uuid;

/// Monitoring manager for comprehensive cluster monitoring.
pub struct MonitoringManager {
    /// Custom metrics registry
    metrics: Arc<DashMap<MetricId, RwLock<MetricSeries>>>,
    /// Alert rules
    alert_rules: Arc<DashMap<AlertId, AlertRule>>,
    /// Active alerts
    active_alerts: Arc<DashMap<AlertId, Alert>>,
    /// Alert history
    alert_history: Arc<RwLock<VecDeque<Alert>>>,
    /// Anomaly detector
    anomaly_detector: Arc<RwLock<AnomalyDetector>>,
    /// Statistics
    stats: Arc<RwLock<MonitoringStats>>,
}

/// Time series data for a metric.
#[derive(Debug, Clone)]
pub struct MetricSeries {
    /// Metric name
    pub name: String,
    /// Type of metric (counter, gauge, etc.)
    pub metric_type: MetricType,
    /// Stored data points
    pub datapoints: VecDeque<DataPoint>,
    /// Maximum number of data points to retain
    pub max_points: usize,
}

/// Data point in time series.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataPoint {
    /// Timestamp when the data point was recorded
    pub timestamp: SystemTime,
    /// Metric value
    pub value: f64,
    /// Key-value labels for the data point
    pub labels: HashMap<String, String>,
}

/// Metric type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MetricType {
    /// Monotonically increasing counter
    Counter,
    /// Value that can go up or down
    Gauge,
    /// Distribution of values in buckets
    Histogram,
    /// Quantile-based summary of values
    Summary,
}

/// Alert rule definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRule {
    /// Unique identifier for the alert rule
    pub id: AlertId,
    /// Human-readable name for the alert
    pub name: String,
    /// Metric ID this alert monitors
    pub metric: MetricId,
    /// Condition that triggers the alert
    pub condition: AlertCondition,
    /// Threshold value for the condition
    pub threshold: f64,
    /// Duration the condition must persist before alerting
    pub duration: Duration,
    /// Severity level of the alert
    pub severity: AlertSeverity,
    /// Whether the alert rule is active
    pub enabled: bool,
    /// Notification channels to use when alert triggers
    pub notify: Vec<NotificationChannel>,
}

/// Alert condition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertCondition {
    /// Alert when value exceeds threshold
    GreaterThan,
    /// Alert when value falls below threshold
    LessThan,
    /// Alert when value equals threshold
    Equal,
    /// Alert when rate of change exceeds threshold
    RateOfChange {
        /// Time period over which to calculate rate
        period: Duration,
    },
}

/// Alert severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum AlertSeverity {
    /// Informational alert
    Info,
    /// Warning-level alert
    Warning,
    /// Error-level alert
    Error,
    /// Critical alert requiring immediate attention
    Critical,
}

/// Notification channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NotificationChannel {
    /// Email notification
    Email {
        /// Email address to notify
        address: String,
    },
    /// Webhook notification
    Webhook {
        /// URL to send webhook request
        url: String,
    },
    /// Slack notification
    Slack {
        /// Slack webhook URL
        webhook_url: String,
    },
    /// PagerDuty notification
    PagerDuty {
        /// PagerDuty integration key
        integration_key: String,
    },
}

/// Active alert.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    /// Unique identifier for this alert instance
    pub id: AlertId,
    /// The rule that triggered this alert
    pub rule_id: AlertId,
    /// When the alert was triggered
    pub triggered_at: SystemTime,
    /// When the alert was resolved (if resolved)
    pub resolved_at: Option<SystemTime>,
    /// Severity level of the alert
    pub severity: AlertSeverity,
    /// Human-readable alert message
    pub message: String,
    /// Value that triggered the alert
    pub value: f64,
}

/// Anomaly detector using simple statistical methods.
#[derive(Debug, Clone)]
pub struct AnomalyDetector {
    /// Sensitivity (number of standard deviations)
    sensitivity: f64,
    /// Metric history for analysis
    metric_history: HashMap<MetricId, VecDeque<f64>>,
    /// Window size
    window_size: usize,
}

impl AnomalyDetector {
    fn new(sensitivity: f64, window_size: usize) -> Self {
        Self {
            sensitivity,
            metric_history: HashMap::new(),
            window_size,
        }
    }

    fn record(&mut self, metric_id: MetricId, value: f64) {
        let history = self.metric_history.entry(metric_id).or_default();
        history.push_back(value);

        if history.len() > self.window_size {
            history.pop_front();
        }
    }

    fn detect_anomaly(&self, metric_id: &MetricId, value: f64) -> bool {
        if let Some(history) = self.metric_history.get(metric_id) {
            if history.len() < 10 {
                return false;
            }

            let mean = history.iter().sum::<f64>() / history.len() as f64;
            let variance =
                history.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / history.len() as f64;
            let std_dev = variance.sqrt();

            let z_score = (value - mean).abs() / std_dev.max(0.001);
            z_score > self.sensitivity
        } else {
            false
        }
    }
}

/// Monitoring statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MonitoringStats {
    /// Total number of registered metrics
    pub total_metrics: usize,
    /// Total number of data points recorded
    pub total_datapoints: u64,
    /// Total number of alerts triggered
    pub total_alerts: u64,
    /// Number of currently active alerts
    pub active_alerts: usize,
    /// Number of anomalies detected
    pub anomalies_detected: u64,
}

impl MonitoringManager {
    /// Create a new monitoring manager.
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(DashMap::new()),
            alert_rules: Arc::new(DashMap::new()),
            active_alerts: Arc::new(DashMap::new()),
            alert_history: Arc::new(RwLock::new(VecDeque::new())),
            anomaly_detector: Arc::new(RwLock::new(AnomalyDetector::new(3.0, 100))),
            stats: Arc::new(RwLock::new(MonitoringStats::default())),
        }
    }

    /// Register a custom metric.
    pub fn register_metric(&self, name: MetricId, metric_type: MetricType) -> Result<()> {
        let series = MetricSeries {
            name: name.clone(),
            metric_type,
            datapoints: VecDeque::new(),
            max_points: 10000,
        };

        self.metrics.insert(name, RwLock::new(series));

        let mut stats = self.stats.write();
        stats.total_metrics = self.metrics.len();

        Ok(())
    }

    /// Record a metric value.
    pub fn record_metric(
        &self,
        metric_id: MetricId,
        value: f64,
        labels: HashMap<String, String>,
    ) -> Result<()> {
        let entry = self
            .metrics
            .get(&metric_id)
            .ok_or_else(|| ClusterError::MetricNotFound(metric_id.clone()))?;

        let mut series = entry.write();

        let datapoint = DataPoint {
            timestamp: SystemTime::now(),
            value,
            labels,
        };

        series.datapoints.push_back(datapoint);

        if series.datapoints.len() > series.max_points {
            series.datapoints.pop_front();
        }

        // Record for anomaly detection
        let mut detector = self.anomaly_detector.write();
        detector.record(metric_id.clone(), value);

        // Check for anomalies
        if detector.detect_anomaly(&metric_id, value) {
            warn!("Anomaly detected in metric {}: {}", metric_id, value);
            let mut stats = self.stats.write();
            stats.anomalies_detected += 1;
        }

        // Update stats - IMPORTANT: Must release this lock before evaluate_alerts
        // to avoid deadlock (evaluate_alerts may call trigger_alert which also locks stats)
        {
            let mut stats = self.stats.write();
            stats.total_datapoints += 1;
        } // Lock is released here

        // Evaluate alert rules
        self.evaluate_alerts(&metric_id, value)?;

        Ok(())
    }

    /// Create an alert rule.
    pub fn create_alert_rule(&self, rule: AlertRule) -> Result<AlertId> {
        let id = rule.id;
        self.alert_rules.insert(id, rule);
        Ok(id)
    }

    /// Evaluate alert rules for a metric.
    fn evaluate_alerts(&self, metric_id: &MetricId, value: f64) -> Result<()> {
        for entry in self.alert_rules.iter() {
            let rule = entry.value();

            if !rule.enabled || rule.metric != *metric_id {
                continue;
            }

            let triggered = match rule.condition {
                AlertCondition::GreaterThan => value > rule.threshold,
                AlertCondition::LessThan => value < rule.threshold,
                AlertCondition::Equal => (value - rule.threshold).abs() < 0.001,
                AlertCondition::RateOfChange { .. } => false, // Simplified
            };

            if triggered && !self.active_alerts.contains_key(&rule.id) {
                self.trigger_alert(rule.id, value)?;
            } else if !triggered && self.active_alerts.contains_key(&rule.id) {
                self.resolve_alert(rule.id)?;
            }
        }

        Ok(())
    }

    fn trigger_alert(&self, rule_id: AlertId, value: f64) -> Result<()> {
        let rule = self
            .alert_rules
            .get(&rule_id)
            .ok_or_else(|| ClusterError::AlertNotFound(rule_id.to_string()))?;

        let alert = Alert {
            id: uuid::Uuid::new_v4(),
            rule_id,
            triggered_at: SystemTime::now(),
            resolved_at: None,
            severity: rule.severity,
            message: format!("Alert triggered: {} (value: {})", rule.name, value),
            value,
        };

        info!("Alert triggered: {} - {}", rule.name, alert.message);

        // Send notifications
        for channel in &rule.notify {
            self.send_notification(channel, &alert)?;
        }

        self.active_alerts.insert(rule_id, alert.clone());
        self.alert_history.write().push_back(alert);

        let mut stats = self.stats.write();
        stats.total_alerts += 1;
        stats.active_alerts = self.active_alerts.len();

        Ok(())
    }

    fn resolve_alert(&self, rule_id: AlertId) -> Result<()> {
        if let Some((_, mut alert)) = self.active_alerts.remove(&rule_id) {
            alert.resolved_at = Some(SystemTime::now());
            info!("Alert resolved: {}", alert.message);

            let mut stats = self.stats.write();
            stats.active_alerts = self.active_alerts.len();
        }

        Ok(())
    }

    fn send_notification(&self, channel: &NotificationChannel, alert: &Alert) -> Result<()> {
        match channel {
            NotificationChannel::Email { address } => {
                debug!("Would send email to {}: {}", address, alert.message);
            }
            NotificationChannel::Webhook { url } => {
                debug!("Would send webhook to {}: {}", url, alert.message);
            }
            NotificationChannel::Slack { webhook_url } => {
                debug!(
                    "Would send Slack notification to {}: {}",
                    webhook_url, alert.message
                );
            }
            NotificationChannel::PagerDuty { integration_key } => {
                debug!(
                    "Would trigger PagerDuty with key {}: {}",
                    integration_key, alert.message
                );
            }
        }
        Ok(())
    }

    /// Get metric series.
    pub fn get_metric(&self, metric_id: &MetricId) -> Option<Vec<DataPoint>> {
        self.metrics
            .get(metric_id)
            .map(|s| s.read().datapoints.iter().cloned().collect())
    }

    /// Get active alerts.
    pub fn get_active_alerts(&self) -> Vec<Alert> {
        self.active_alerts
            .iter()
            .map(|e| e.value().clone())
            .collect()
    }

    /// Get alert history.
    pub fn get_alert_history(&self, limit: usize) -> Vec<Alert> {
        let history = self.alert_history.read();
        history.iter().rev().take(limit).cloned().collect()
    }

    /// Get monitoring statistics.
    pub fn get_stats(&self) -> MonitoringStats {
        self.stats.read().clone()
    }
}

impl Default for MonitoringManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_metric_registration() {
        let manager = MonitoringManager::new();
        let result = manager.register_metric("test_metric".to_string(), MetricType::Gauge);
        assert!(result.is_ok());

        let stats = manager.get_stats();
        assert_eq!(stats.total_metrics, 1);
    }

    #[test]
    fn test_metric_recording() {
        let manager = MonitoringManager::new();
        manager
            .register_metric("cpu_usage".to_string(), MetricType::Gauge)
            .ok();

        let mut labels = HashMap::new();
        labels.insert("host".to_string(), "worker1".to_string());

        manager
            .record_metric("cpu_usage".to_string(), 0.75, labels)
            .ok();

        let datapoints = manager.get_metric(&"cpu_usage".to_string());
        assert!(datapoints.is_some());
        assert_eq!(
            datapoints
                .expect("datapoints should be present for recorded metric")
                .len(),
            1
        );
    }

    #[test]
    fn test_alert_rule() {
        let manager = MonitoringManager::new();
        manager
            .register_metric("cpu_usage".to_string(), MetricType::Gauge)
            .expect("Failed to register metric");

        // Use a short duration for test - the duration field is metadata
        // and not currently enforced in the alert evaluation logic
        let rule = AlertRule {
            id: uuid::Uuid::new_v4(),
            name: "High CPU".to_string(),
            metric: "cpu_usage".to_string(),
            condition: AlertCondition::GreaterThan,
            threshold: 0.8,
            duration: Duration::from_millis(100), // Reduced from 60s to 100ms for fast testing
            severity: AlertSeverity::Warning,
            enabled: true,
            notify: vec![],
        };

        manager
            .create_alert_rule(rule.clone())
            .expect("Failed to create alert rule");

        // Trigger alert with high CPU
        manager
            .record_metric("cpu_usage".to_string(), 0.9, HashMap::new())
            .expect("Failed to record metric");

        let alerts = manager.get_active_alerts();
        assert!(!alerts.is_empty());
    }

    #[test]
    fn test_anomaly_detection() {
        let mut detector = AnomalyDetector::new(3.0, 100);

        // Record normal values
        for i in 0..50 {
            detector.record("metric1".to_string(), 100.0 + (i as f64 % 10.0));
        }

        // Normal value should not be anomaly
        assert!(!detector.detect_anomaly(&"metric1".to_string(), 105.0));

        // Abnormal value should be anomaly
        assert!(detector.detect_anomaly(&"metric1".to_string(), 500.0));
    }
}
