//! Alert rule definitions and condition expressions

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Threshold comparison operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThresholdOperator {
    /// Value is greater than threshold.
    GreaterThan,
    /// Value is greater than or equal to threshold.
    GreaterThanOrEqual,
    /// Value is less than threshold.
    LessThan,
    /// Value is less than or equal to threshold.
    LessThanOrEqual,
    /// Value equals threshold.
    Equal,
    /// Value does not equal threshold.
    NotEqual,
}

impl ThresholdOperator {
    /// Evaluate the operator with two values.
    #[must_use]
    pub fn evaluate(&self, value: f64, threshold: f64) -> bool {
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

/// Aggregation functions for metric queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AggregationFunction {
    /// Average of values.
    Avg,
    /// Sum of values.
    Sum,
    /// Minimum value.
    Min,
    /// Maximum value.
    Max,
    /// Count of values.
    Count,
    /// Rate of change.
    Rate,
    /// Percentile (e.g., 95th).
    Percentile(u8),
}

/// Condition expression for alert rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConditionExpression {
    /// Simple threshold condition.
    Threshold {
        /// Metric name to query.
        metric: String,
        /// Comparison operator.
        operator: ThresholdOperator,
        /// Threshold value.
        value: f64,
    },
    /// Threshold with aggregation over a time range.
    AggregatedThreshold {
        /// Metric name to query.
        metric: String,
        /// Aggregation function to apply.
        aggregation: AggregationFunction,
        /// Time window for aggregation in seconds.
        window_seconds: u64,
        /// Comparison operator.
        operator: ThresholdOperator,
        /// Threshold value.
        value: f64,
    },
    /// Rate of change condition.
    RateOfChange {
        /// Metric name to query.
        metric: String,
        /// Time window for rate calculation in seconds.
        window_seconds: u64,
        /// Comparison operator for rate.
        operator: ThresholdOperator,
        /// Rate threshold (units per second).
        rate_threshold: f64,
    },
    /// Absence of data condition.
    Absent {
        /// Metric name to check.
        metric: String,
        /// Time window to check for absence in seconds.
        for_seconds: u64,
    },
    /// Logical AND of multiple conditions.
    And(Vec<ConditionExpression>),
    /// Logical OR of multiple conditions.
    Or(Vec<ConditionExpression>),
    /// Logical NOT of a condition.
    Not(Box<ConditionExpression>),
    /// Label-based condition.
    LabelMatch {
        /// Label name.
        label: String,
        /// Expected value (regex pattern).
        pattern: String,
    },
}

impl ConditionExpression {
    /// Create a simple threshold condition.
    pub fn threshold(metric: impl Into<String>, operator: ThresholdOperator, value: f64) -> Self {
        Self::Threshold {
            metric: metric.into(),
            operator,
            value,
        }
    }

    /// Create an aggregated threshold condition.
    pub fn aggregated_threshold(
        metric: impl Into<String>,
        aggregation: AggregationFunction,
        window_seconds: u64,
        operator: ThresholdOperator,
        value: f64,
    ) -> Self {
        Self::AggregatedThreshold {
            metric: metric.into(),
            aggregation,
            window_seconds,
            operator,
            value,
        }
    }

    /// Create an AND condition.
    pub fn and(conditions: Vec<ConditionExpression>) -> Self {
        Self::And(conditions)
    }

    /// Create an OR condition.
    pub fn or(conditions: Vec<ConditionExpression>) -> Self {
        Self::Or(conditions)
    }

    /// Create a NOT condition.
    pub fn not(condition: ConditionExpression) -> Self {
        Self::Not(Box::new(condition))
    }
}

/// Complete alert rule definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRuleDefinition {
    /// Unique identifier for the rule.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Detailed description.
    pub description: String,
    /// Alert severity level.
    pub level: super::AlertLevel,
    /// Condition expression to evaluate.
    pub condition: Option<ConditionExpression>,
    /// Duration to wait before firing (pending period).
    pub pending_duration: std::time::Duration,
    /// Labels to attach to fired alerts.
    pub labels: HashMap<String, String>,
    /// Annotations for additional context.
    pub annotations: HashMap<String, String>,
    /// Grouping keys for alert aggregation.
    pub group_by: Vec<String>,
    /// Rule is enabled.
    pub enabled: bool,
    /// Evaluation interval in seconds.
    pub eval_interval_seconds: u64,
    /// Runbook URL for remediation steps.
    pub runbook_url: Option<String>,
    /// Dashboard URL for investigation.
    pub dashboard_url: Option<String>,
}

impl AlertRuleDefinition {
    /// Create a new alert rule with the given ID.
    pub fn new(id: impl Into<String>) -> Self {
        let id_str = id.into();
        Self {
            id: id_str.clone(),
            name: id_str,
            description: String::new(),
            level: super::AlertLevel::default(),
            condition: None,
            pending_duration: std::time::Duration::from_secs(0),
            labels: HashMap::new(),
            annotations: HashMap::new(),
            group_by: Vec::new(),
            enabled: true,
            eval_interval_seconds: 60,
            runbook_url: None,
            dashboard_url: None,
        }
    }

    /// Set the rule name.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Set the description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Set the alert level.
    #[must_use]
    pub const fn with_severity(mut self, level: super::AlertLevel) -> Self {
        self.level = level;
        self
    }

    /// Set the condition expression.
    #[must_use]
    pub fn with_condition(mut self, condition: ConditionExpression) -> Self {
        self.condition = Some(condition);
        self
    }

    /// Set the pending duration.
    #[must_use]
    pub const fn with_pending_duration(mut self, duration: std::time::Duration) -> Self {
        self.pending_duration = duration;
        self
    }

    /// Add a label.
    #[must_use]
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    /// Add an annotation.
    #[must_use]
    pub fn with_annotation(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.annotations.insert(key.into(), value.into());
        self
    }

    /// Set grouping keys.
    #[must_use]
    pub fn with_group_by(mut self, keys: Vec<String>) -> Self {
        self.group_by = keys;
        self
    }

    /// Set runbook URL.
    #[must_use]
    pub fn with_runbook_url(mut self, url: impl Into<String>) -> Self {
        self.runbook_url = Some(url.into());
        self
    }

    /// Set dashboard URL.
    #[must_use]
    pub fn with_dashboard_url(mut self, url: impl Into<String>) -> Self {
        self.dashboard_url = Some(url.into());
        self
    }

    /// Enable or disable the rule.
    #[must_use]
    pub const fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}
