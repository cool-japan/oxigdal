//! Alert system for Kinesis monitoring

use crate::monitoring::metrics::MetricType;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Alert
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    /// Alert ID
    pub id: String,
    /// Alert name
    pub name: String,
    /// Stream name
    pub stream_name: String,
    /// Shard ID (optional, for shard-level alerts)
    pub shard_id: Option<String>,
    /// Alert condition
    pub condition: AlertCondition,
    /// Alert actions
    pub actions: Vec<AlertAction>,
    /// Enabled state
    pub enabled: bool,
}

impl Alert {
    /// Creates a new alert
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        stream_name: impl Into<String>,
        condition: AlertCondition,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            stream_name: stream_name.into(),
            shard_id: None,
            condition,
            actions: Vec::new(),
            enabled: true,
        }
    }

    /// Sets the shard ID
    pub fn with_shard_id(mut self, shard_id: impl Into<String>) -> Self {
        self.shard_id = Some(shard_id.into());
        self
    }

    /// Adds an action
    pub fn add_action(mut self, action: AlertAction) -> Self {
        self.actions.push(action);
        self
    }

    /// Enables or disables the alert
    pub fn set_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Checks if the alert condition is met
    pub fn is_triggered(&self, metric_value: f64) -> bool {
        if !self.enabled {
            return false;
        }

        self.condition.evaluate(metric_value)
    }
}

/// Alert condition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertCondition {
    /// Threshold condition (metric > threshold)
    Threshold {
        /// Metric type
        metric_type: MetricType,
        /// Threshold value
        threshold: f64,
        /// Comparison operator
        operator: ComparisonOperator,
        /// Evaluation periods
        evaluation_periods: u32,
    },
    /// Rate of change condition
    RateOfChange {
        /// Metric type
        metric_type: MetricType,
        /// Change percentage threshold
        change_threshold_percent: f64,
        /// Time window in seconds
        time_window_seconds: i64,
    },
    /// Anomaly detection (simple statistical)
    Anomaly {
        /// Metric type
        metric_type: MetricType,
        /// Standard deviations from mean
        std_deviations: f64,
    },
}

impl AlertCondition {
    /// Evaluates the condition
    pub fn evaluate(&self, value: f64) -> bool {
        match self {
            Self::Threshold {
                threshold,
                operator,
                ..
            } => operator.compare(value, *threshold),
            Self::RateOfChange { .. } => {
                // Rate of change evaluation requires historical data
                // This is simplified
                false
            }
            Self::Anomaly { .. } => {
                // Anomaly detection requires statistical model
                // This is simplified
                false
            }
        }
    }

    /// Gets the metric type
    pub fn metric_type(&self) -> MetricType {
        match self {
            Self::Threshold { metric_type, .. }
            | Self::RateOfChange { metric_type, .. }
            | Self::Anomaly { metric_type, .. } => *metric_type,
        }
    }
}

/// Comparison operator
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComparisonOperator {
    /// Greater than
    GreaterThan,
    /// Greater than or equal
    GreaterThanOrEqual,
    /// Less than
    LessThan,
    /// Less than or equal
    LessThanOrEqual,
    /// Equal
    Equal,
    /// Not equal
    NotEqual,
}

impl ComparisonOperator {
    /// Compares two values
    pub fn compare(&self, value: f64, threshold: f64) -> bool {
        match self {
            Self::GreaterThan => value > threshold,
            Self::GreaterThanOrEqual => value >= threshold,
            Self::LessThan => value < threshold,
            Self::LessThanOrEqual => value <= threshold,
            Self::Equal => (value - threshold).abs() < f64::EPSILON,
            Self::NotEqual => (value - threshold).abs() >= f64::EPSILON,
        }
    }
}

/// Alert action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertAction {
    /// Send SNS notification
    SnsNotification {
        /// SNS topic ARN
        topic_arn: String,
    },
    /// Invoke Lambda function
    LambdaInvocation {
        /// Lambda function ARN
        function_arn: String,
    },
    /// Send email
    Email {
        /// Recipient email addresses
        recipients: Vec<String>,
        /// Subject
        subject: String,
    },
    /// Log to CloudWatch Logs
    CloudWatchLogs {
        /// Log group name
        log_group: String,
    },
    /// Auto-scaling action
    AutoScale {
        /// Scale up or down
        scale_up: bool,
        /// Number of shards to add/remove
        shard_count_adjustment: i32,
    },
}

/// Alert manager
pub struct AlertManager {
    alerts: parking_lot::RwLock<HashMap<String, Alert>>,
}

impl AlertManager {
    /// Creates a new alert manager
    pub fn new() -> Self {
        Self {
            alerts: parking_lot::RwLock::new(HashMap::new()),
        }
    }

    /// Registers an alert
    pub fn register_alert(&self, alert: Alert) {
        self.alerts.write().insert(alert.id.clone(), alert);
    }

    /// Unregisters an alert
    pub fn unregister_alert(&self, alert_id: &str) {
        self.alerts.write().remove(alert_id);
    }

    /// Gets an alert
    pub fn get_alert(&self, alert_id: &str) -> Option<Alert> {
        self.alerts.read().get(alert_id).cloned()
    }

    /// Lists all alerts
    pub fn list_alerts(&self) -> Vec<Alert> {
        self.alerts.read().values().cloned().collect()
    }

    /// Lists alerts for a stream
    pub fn list_alerts_for_stream(&self, stream_name: &str) -> Vec<Alert> {
        self.alerts
            .read()
            .values()
            .filter(|a| a.stream_name == stream_name)
            .cloned()
            .collect()
    }

    /// Evaluates alerts for a metric
    pub fn evaluate_alerts(
        &self,
        stream_name: &str,
        shard_id: Option<&str>,
        metric_type: MetricType,
        value: f64,
    ) -> Vec<Alert> {
        self.alerts
            .read()
            .values()
            .filter(|alert| {
                alert.stream_name == stream_name
                    && alert.shard_id.as_deref() == shard_id
                    && alert.condition.metric_type() == metric_type
                    && alert.is_triggered(value)
            })
            .cloned()
            .collect()
    }

    /// Enables an alert
    pub fn enable_alert(&self, alert_id: &str) {
        if let Some(alert) = self.alerts.write().get_mut(alert_id) {
            alert.enabled = true;
        }
    }

    /// Disables an alert
    pub fn disable_alert(&self, alert_id: &str) {
        if let Some(alert) = self.alerts.write().get_mut(alert_id) {
            alert.enabled = false;
        }
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
    fn test_comparison_operator() {
        assert!(ComparisonOperator::GreaterThan.compare(10.0, 5.0));
        assert!(!ComparisonOperator::GreaterThan.compare(5.0, 10.0));

        assert!(ComparisonOperator::LessThan.compare(5.0, 10.0));
        assert!(!ComparisonOperator::LessThan.compare(10.0, 5.0));

        assert!(ComparisonOperator::Equal.compare(5.0, 5.0));
        assert!(!ComparisonOperator::Equal.compare(5.0, 10.0));
    }

    #[test]
    fn test_alert_creation() {
        let alert = Alert::new(
            "alert-1",
            "High Iterator Age",
            "test-stream",
            AlertCondition::Threshold {
                metric_type: MetricType::GetRecordsIteratorAgeMilliseconds,
                threshold: 60000.0,
                operator: ComparisonOperator::GreaterThan,
                evaluation_periods: 2,
            },
        )
        .add_action(AlertAction::SnsNotification {
            topic_arn: "arn:aws:sns:us-east-1:123456789012:alerts".to_string(),
        });

        assert_eq!(alert.id, "alert-1");
        assert_eq!(alert.name, "High Iterator Age");
        assert_eq!(alert.actions.len(), 1);
    }

    #[test]
    fn test_alert_triggered() {
        let alert = Alert::new(
            "alert-1",
            "High Throughput",
            "test-stream",
            AlertCondition::Threshold {
                metric_type: MetricType::IncomingRecords,
                threshold: 1000.0,
                operator: ComparisonOperator::GreaterThan,
                evaluation_periods: 1,
            },
        );

        assert!(alert.is_triggered(1500.0));
        assert!(!alert.is_triggered(500.0));
    }

    #[test]
    fn test_alert_disabled() {
        let alert = Alert::new(
            "alert-1",
            "Test Alert",
            "test-stream",
            AlertCondition::Threshold {
                metric_type: MetricType::IncomingRecords,
                threshold: 1000.0,
                operator: ComparisonOperator::GreaterThan,
                evaluation_periods: 1,
            },
        )
        .set_enabled(false);

        // Should not trigger when disabled
        assert!(!alert.is_triggered(1500.0));
    }

    #[test]
    fn test_alert_manager() {
        let manager = AlertManager::new();

        let alert = Alert::new(
            "alert-1",
            "Test Alert",
            "test-stream",
            AlertCondition::Threshold {
                metric_type: MetricType::IncomingRecords,
                threshold: 1000.0,
                operator: ComparisonOperator::GreaterThan,
                evaluation_periods: 1,
            },
        );

        manager.register_alert(alert);

        assert!(manager.get_alert("alert-1").is_some());
        assert_eq!(manager.list_alerts().len(), 1);
        assert_eq!(manager.list_alerts_for_stream("test-stream").len(), 1);
    }

    #[test]
    fn test_alert_manager_evaluate() {
        let manager = AlertManager::new();

        let alert = Alert::new(
            "alert-1",
            "High Throughput",
            "test-stream",
            AlertCondition::Threshold {
                metric_type: MetricType::IncomingRecords,
                threshold: 1000.0,
                operator: ComparisonOperator::GreaterThan,
                evaluation_periods: 1,
            },
        );

        manager.register_alert(alert);

        let triggered =
            manager.evaluate_alerts("test-stream", None, MetricType::IncomingRecords, 1500.0);

        assert_eq!(triggered.len(), 1);
    }

    #[test]
    fn test_alert_manager_enable_disable() {
        let manager = AlertManager::new();

        let alert = Alert::new(
            "alert-1",
            "Test Alert",
            "test-stream",
            AlertCondition::Threshold {
                metric_type: MetricType::IncomingRecords,
                threshold: 1000.0,
                operator: ComparisonOperator::GreaterThan,
                evaluation_periods: 1,
            },
        );

        manager.register_alert(alert);
        manager.disable_alert("alert-1");

        let alert = manager.get_alert("alert-1");
        assert!(!alert.as_ref().map(|a| a.enabled).unwrap_or(true));

        manager.enable_alert("alert-1");
        let alert = manager.get_alert("alert-1");
        assert!(alert.as_ref().map(|a| a.enabled).unwrap_or(false));
    }
}
