//! Condition evaluation engine

use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Arc;

use super::rules::{AggregationFunction, ConditionExpression};
/// Metric data point for evaluation.
#[derive(Debug, Clone)]
pub struct MetricDataPoint {
    /// Metric name.
    pub name: String,
    /// Metric value.
    pub value: f64,
    /// Labels.
    pub labels: HashMap<String, String>,
    /// Timestamp.
    pub timestamp: DateTime<Utc>,
}

/// Metric provider trait for retrieving metric values.
pub trait MetricProvider: Send + Sync {
    /// Get the current value of a metric.
    fn get_metric(&self, name: &str) -> Option<f64>;

    /// Get metric values over a time range.
    fn get_metric_range(&self, name: &str, duration_seconds: u64) -> Vec<MetricDataPoint>;

    /// Check if a metric exists.
    fn has_metric(&self, name: &str) -> bool {
        self.get_metric(name).is_some()
    }
}

/// Condition evaluation engine.
pub struct ConditionEvaluator<P: MetricProvider> {
    provider: Arc<P>,
}

impl<P: MetricProvider> ConditionEvaluator<P> {
    /// Create a new condition evaluator.
    pub fn new(provider: Arc<P>) -> Self {
        Self { provider }
    }

    /// Evaluate a condition expression with no label context.
    ///
    /// Equivalent to [`Self::evaluate_with_labels`] with an empty label map:
    /// a [`super::rules::ConditionExpression::LabelMatch`] condition will
    /// therefore never match (there are no labels to match against), which
    /// is the honest outcome when no context is available -- see
    /// [`Self::evaluate_with_labels`] for the real implementation used by
    /// [`super::manager::AlertEngine`], which passes the firing rule's
    /// static labels.
    pub fn evaluate(&self, condition: &ConditionExpression) -> bool {
        self.evaluate_with_labels(condition, &HashMap::new())
    }

    /// Evaluate a condition expression against a label context (typically
    /// an [`super::rules::AlertRuleDefinition`]'s static `labels`, or an
    /// in-flight [`super::instance::AlertInstance`]'s `labels`).
    ///
    /// This is what makes [`super::rules::ConditionExpression::LabelMatch`]
    /// a real check instead of the unconditional `true` it previously
    /// returned (which meant every rule using `LabelMatch` fired regardless
    /// of whether the label actually matched): the referenced label is
    /// looked up in `label_context` and matched against `pattern` as a
    /// regular expression.  An invalid regex pattern or a missing label
    /// means the condition does not match (fails closed, not open).
    pub fn evaluate_with_labels(
        &self,
        condition: &ConditionExpression,
        label_context: &HashMap<String, String>,
    ) -> bool {
        match condition {
            ConditionExpression::Threshold {
                metric,
                operator,
                value,
            } => self
                .provider
                .get_metric(metric)
                .map(|v| operator.evaluate(v, *value))
                .unwrap_or(false),
            ConditionExpression::AggregatedThreshold {
                metric,
                aggregation,
                window_seconds,
                operator,
                value,
            } => {
                let data_points = self.provider.get_metric_range(metric, *window_seconds);
                if data_points.is_empty() {
                    return false;
                }

                let aggregated_value = self.aggregate(&data_points, aggregation);
                operator.evaluate(aggregated_value, *value)
            }
            ConditionExpression::RateOfChange {
                metric,
                window_seconds,
                operator,
                rate_threshold,
            } => {
                let data_points = self.provider.get_metric_range(metric, *window_seconds);
                if data_points.len() < 2 {
                    return false;
                }

                let rate = self.calculate_rate(&data_points);
                operator.evaluate(rate, *rate_threshold)
            }
            ConditionExpression::Absent {
                metric,
                for_seconds: _,
            } => !self.provider.has_metric(metric),
            ConditionExpression::And(conditions) => conditions
                .iter()
                .all(|c| self.evaluate_with_labels(c, label_context)),
            ConditionExpression::Or(conditions) => conditions
                .iter()
                .any(|c| self.evaluate_with_labels(c, label_context)),
            ConditionExpression::Not(condition) => {
                !self.evaluate_with_labels(condition, label_context)
            }
            ConditionExpression::LabelMatch { label, pattern } => match label_context.get(label) {
                Some(value) => regex::Regex::new(pattern)
                    .map(|re| re.is_match(value))
                    .unwrap_or(false),
                None => false,
            },
        }
    }

    fn aggregate(&self, data_points: &[MetricDataPoint], aggregation: &AggregationFunction) -> f64 {
        if data_points.is_empty() {
            return 0.0;
        }

        let values: Vec<f64> = data_points.iter().map(|p| p.value).collect();

        match aggregation {
            AggregationFunction::Avg => values.iter().sum::<f64>() / values.len() as f64,
            AggregationFunction::Sum => values.iter().sum(),
            AggregationFunction::Min => values.iter().cloned().fold(f64::INFINITY, f64::min),
            AggregationFunction::Max => values.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
            AggregationFunction::Count => values.len() as f64,
            AggregationFunction::Rate => self.calculate_rate(data_points),
            AggregationFunction::Percentile(p) => self.calculate_percentile(&values, *p),
        }
    }

    fn calculate_rate(&self, data_points: &[MetricDataPoint]) -> f64 {
        if data_points.len() < 2 {
            return 0.0;
        }

        let first = &data_points[0];
        let last = &data_points[data_points.len() - 1];

        let value_diff = last.value - first.value;
        let time_diff = (last.timestamp - first.timestamp).num_seconds() as f64;

        if time_diff > 0.0 {
            value_diff / time_diff
        } else {
            0.0
        }
    }

    fn calculate_percentile(&self, values: &[f64], percentile: u8) -> f64 {
        if values.is_empty() {
            return 0.0;
        }

        let mut sorted = values.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let index = (percentile as f64 / 100.0 * (sorted.len() - 1) as f64).round() as usize;
        sorted.get(index).copied().unwrap_or(0.0)
    }
}
