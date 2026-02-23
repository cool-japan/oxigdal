//! Real-time performance dashboards with metric aggregation and visualization.
//!
//! This module provides comprehensive real-time performance monitoring capabilities:
//! - Real-time metric aggregation with sliding windows
//! - Time-series data structures for historical analysis
//! - Dashboard widget definitions for visualization
//! - Alert threshold configuration and monitoring
//! - Performance trend analysis with statistical methods
//! - Resource utilization tracking (CPU, memory, I/O)
//! - Custom metric visualization support

use crate::error::{ObservabilityError, Result};
use chrono::{DateTime, Duration, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

// ============================================================================
// Time Series Data Structures
// ============================================================================

/// A single data point in a time series.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataPoint {
    /// Timestamp of the data point.
    pub timestamp: DateTime<Utc>,
    /// Value at this timestamp.
    pub value: f64,
    /// Optional labels for this data point.
    pub labels: HashMap<String, String>,
}

impl DataPoint {
    /// Create a new data point with the current timestamp.
    pub fn now(value: f64) -> Self {
        Self {
            timestamp: Utc::now(),
            value,
            labels: HashMap::new(),
        }
    }

    /// Create a new data point with specific timestamp.
    pub fn with_timestamp(timestamp: DateTime<Utc>, value: f64) -> Self {
        Self {
            timestamp,
            value,
            labels: HashMap::new(),
        }
    }

    /// Add a label to this data point.
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }
}

/// Time series data structure with efficient storage and querying.
#[derive(Debug, Clone)]
pub struct TimeSeries {
    /// Name of the time series.
    pub name: String,
    /// Description of the metric.
    pub description: String,
    /// Unit of measurement.
    pub unit: MetricUnit,
    /// Data points stored in chronological order.
    data: VecDeque<DataPoint>,
    /// Maximum number of data points to retain.
    max_size: usize,
    /// Retention duration for data points.
    retention: Duration,
}

/// Unit of measurement for metrics.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MetricUnit {
    /// No unit (dimensionless).
    None,
    /// Count/number of items.
    Count,
    /// Bytes.
    Bytes,
    /// Kilobytes.
    Kilobytes,
    /// Megabytes.
    Megabytes,
    /// Gigabytes.
    Gigabytes,
    /// Milliseconds.
    Milliseconds,
    /// Seconds.
    Seconds,
    /// Percentage (0-100).
    Percent,
    /// Requests per second.
    RequestsPerSecond,
    /// Operations per second.
    OperationsPerSecond,
    /// Pixels per second.
    PixelsPerSecond,
}

impl TimeSeries {
    /// Create a new time series.
    pub fn new(name: impl Into<String>, description: impl Into<String>, unit: MetricUnit) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            unit,
            data: VecDeque::new(),
            max_size: 10000,
            retention: Duration::hours(24),
        }
    }

    /// Set the maximum number of data points.
    pub fn with_max_size(mut self, max_size: usize) -> Self {
        self.max_size = max_size;
        self
    }

    /// Set the retention duration.
    pub fn with_retention(mut self, retention: Duration) -> Self {
        self.retention = retention;
        self
    }

    /// Add a data point to the time series.
    pub fn add(&mut self, point: DataPoint) {
        // Remove old data points based on retention
        let cutoff = Utc::now() - self.retention;
        while let Some(front) = self.data.front() {
            if front.timestamp < cutoff {
                self.data.pop_front();
            } else {
                break;
            }
        }

        // Remove oldest if at max size
        if self.data.len() >= self.max_size {
            self.data.pop_front();
        }

        self.data.push_back(point);
    }

    /// Add a value with current timestamp.
    pub fn add_value(&mut self, value: f64) {
        self.add(DataPoint::now(value));
    }

    /// Get data points within a time range.
    pub fn range(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Vec<&DataPoint> {
        self.data
            .iter()
            .filter(|p| p.timestamp >= start && p.timestamp <= end)
            .collect()
    }

    /// Get the last N data points.
    pub fn last_n(&self, n: usize) -> Vec<&DataPoint> {
        self.data.iter().rev().take(n).rev().collect()
    }

    /// Get the latest data point.
    pub fn latest(&self) -> Option<&DataPoint> {
        self.data.back()
    }

    /// Get aggregate statistics for the time series.
    pub fn statistics(&self) -> Option<TimeSeriesStats> {
        if self.data.is_empty() {
            return None;
        }

        let values: Vec<f64> = self.data.iter().map(|p| p.value).collect();
        let count = values.len();
        let sum: f64 = values.iter().sum();
        let mean = sum / count as f64;

        let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / count as f64;
        let std_dev = variance.sqrt();

        let min = values.iter().copied().fold(f64::INFINITY, |a, b| a.min(b));
        let max = values
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, |a, b| a.max(b));

        // Calculate percentiles
        let mut sorted = values;
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let p50 = percentile(&sorted, 50.0);
        let p90 = percentile(&sorted, 90.0);
        let p95 = percentile(&sorted, 95.0);
        let p99 = percentile(&sorted, 99.0);

        Some(TimeSeriesStats {
            count,
            sum,
            mean,
            std_dev,
            min,
            max,
            p50,
            p90,
            p95,
            p99,
        })
    }

    /// Get the data length.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Check if the time series is empty.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Clear all data points.
    pub fn clear(&mut self) {
        self.data.clear();
    }
}

/// Calculate percentile from sorted values.
fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }

    let idx = (p / 100.0 * (sorted.len() - 1) as f64).round() as usize;
    let idx = idx.min(sorted.len() - 1);
    sorted[idx]
}

/// Statistics for a time series.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeSeriesStats {
    /// Number of data points.
    pub count: usize,
    /// Sum of all values.
    pub sum: f64,
    /// Mean value.
    pub mean: f64,
    /// Standard deviation.
    pub std_dev: f64,
    /// Minimum value.
    pub min: f64,
    /// Maximum value.
    pub max: f64,
    /// 50th percentile (median).
    pub p50: f64,
    /// 90th percentile.
    pub p90: f64,
    /// 95th percentile.
    pub p95: f64,
    /// 99th percentile.
    pub p99: f64,
}

// ============================================================================
// Real-time Metric Aggregation
// ============================================================================

/// Aggregation method for metrics.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AggregationType {
    /// Sum of values.
    Sum,
    /// Average of values.
    Average,
    /// Minimum value.
    Min,
    /// Maximum value.
    Max,
    /// Count of data points.
    Count,
    /// Rate of change per second.
    Rate,
    /// Percentile (specified in configuration).
    Percentile,
}

/// Configuration for metric aggregation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationConfig {
    /// Aggregation method.
    pub method: AggregationType,
    /// Window duration for aggregation.
    pub window: Duration,
    /// Step interval for rolling aggregation.
    pub step: Duration,
    /// Percentile value (if method is Percentile).
    pub percentile_value: Option<f64>,
}

impl Default for AggregationConfig {
    fn default() -> Self {
        Self {
            method: AggregationType::Average,
            window: Duration::minutes(1),
            step: Duration::seconds(10),
            percentile_value: None,
        }
    }
}

/// Real-time metric aggregator with sliding window.
#[derive(Debug)]
pub struct MetricAggregator {
    /// Configuration for aggregation.
    config: AggregationConfig,
    /// Underlying time series.
    series: TimeSeries,
    /// Aggregated results.
    aggregated: RwLock<VecDeque<DataPoint>>,
    /// Maximum aggregated results to keep.
    max_aggregated: usize,
}

impl MetricAggregator {
    /// Create a new metric aggregator.
    pub fn new(series: TimeSeries, config: AggregationConfig) -> Self {
        Self {
            config,
            series,
            aggregated: RwLock::new(VecDeque::new()),
            max_aggregated: 1000,
        }
    }

    /// Add a data point and trigger aggregation if needed.
    pub fn add(&mut self, point: DataPoint) {
        self.series.add(point);
        self.maybe_aggregate();
    }

    /// Add a value with current timestamp.
    pub fn add_value(&mut self, value: f64) {
        self.series.add_value(value);
        self.maybe_aggregate();
    }

    /// Perform aggregation if the step interval has passed.
    fn maybe_aggregate(&self) {
        let mut aggregated = self.aggregated.write();
        let now = Utc::now();

        // Check if we need to aggregate
        let should_aggregate = match aggregated.back() {
            Some(last) => (now - last.timestamp) >= self.config.step,
            None => !self.series.is_empty(),
        };

        if should_aggregate {
            let window_start = now - self.config.window;
            let points = self.series.range(window_start, now);

            if !points.is_empty() {
                let value = self.compute_aggregation(&points);
                let point = DataPoint::now(value);

                if aggregated.len() >= self.max_aggregated {
                    aggregated.pop_front();
                }
                aggregated.push_back(point);
            }
        }
    }

    /// Compute the aggregation for a set of points.
    fn compute_aggregation(&self, points: &[&DataPoint]) -> f64 {
        if points.is_empty() {
            return 0.0;
        }

        match self.config.method {
            AggregationType::Sum => points.iter().map(|p| p.value).sum(),
            AggregationType::Average => {
                let sum: f64 = points.iter().map(|p| p.value).sum();
                sum / points.len() as f64
            }
            AggregationType::Min => points
                .iter()
                .map(|p| p.value)
                .fold(f64::INFINITY, |a, b| a.min(b)),
            AggregationType::Max => points
                .iter()
                .map(|p| p.value)
                .fold(f64::NEG_INFINITY, |a, b| a.max(b)),
            AggregationType::Count => points.len() as f64,
            AggregationType::Rate => {
                if points.len() < 2 {
                    return 0.0;
                }
                let first = &points[0];
                let last = &points[points.len() - 1];
                let duration = (last.timestamp - first.timestamp).num_seconds() as f64;
                if duration <= 0.0 {
                    return 0.0;
                }
                (last.value - first.value) / duration
            }
            AggregationType::Percentile => {
                let p = self.config.percentile_value.unwrap_or(95.0);
                let mut values: Vec<f64> = points.iter().map(|pt| pt.value).collect();
                values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                percentile(&values, p)
            }
        }
    }

    /// Get aggregated data points.
    pub fn aggregated(&self) -> Vec<DataPoint> {
        self.aggregated.read().iter().cloned().collect()
    }

    /// Get the latest aggregated value.
    pub fn latest_aggregated(&self) -> Option<DataPoint> {
        self.aggregated.read().back().cloned()
    }

    /// Get underlying time series statistics.
    pub fn statistics(&self) -> Option<TimeSeriesStats> {
        self.series.statistics()
    }
}

// ============================================================================
// Dashboard Widget Definitions
// ============================================================================

/// Type of dashboard widget.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum WidgetType {
    /// Line chart for time series.
    LineChart,
    /// Area chart.
    AreaChart,
    /// Bar chart.
    BarChart,
    /// Gauge/dial display.
    Gauge,
    /// Single value display.
    SingleStat,
    /// Data table.
    Table,
    /// Heat map.
    Heatmap,
    /// Histogram.
    Histogram,
    /// Pie chart.
    PieChart,
    /// Sparkline.
    Sparkline,
    /// Text panel.
    Text,
}

/// Widget display configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WidgetDisplay {
    /// Widget title.
    pub title: String,
    /// Widget description.
    pub description: Option<String>,
    /// Position in grid (x, y).
    pub position: (u32, u32),
    /// Size in grid (width, height).
    pub size: (u32, u32),
    /// Background color (hex).
    pub background_color: Option<String>,
    /// Text color (hex).
    pub text_color: Option<String>,
}

/// Threshold configuration for widgets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdConfig {
    /// Value threshold.
    pub value: f64,
    /// Color for this threshold (hex).
    pub color: String,
    /// Label for this threshold.
    pub label: Option<String>,
}

/// Dashboard widget definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Widget {
    /// Unique widget ID.
    pub id: String,
    /// Widget type.
    pub widget_type: WidgetType,
    /// Display configuration.
    pub display: WidgetDisplay,
    /// Metric names to display.
    pub metrics: Vec<String>,
    /// Aggregation configuration.
    pub aggregation: Option<AggregationConfig>,
    /// Thresholds for coloring.
    pub thresholds: Vec<ThresholdConfig>,
    /// Refresh interval in seconds.
    pub refresh_interval: u32,
    /// Custom options.
    pub options: HashMap<String, serde_json::Value>,
}

impl Widget {
    /// Create a new widget.
    pub fn new(id: impl Into<String>, widget_type: WidgetType, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            widget_type,
            display: WidgetDisplay {
                title: title.into(),
                description: None,
                position: (0, 0),
                size: (4, 4),
                background_color: None,
                text_color: None,
            },
            metrics: Vec::new(),
            aggregation: None,
            thresholds: Vec::new(),
            refresh_interval: 30,
            options: HashMap::new(),
        }
    }

    /// Set widget position.
    pub fn with_position(mut self, x: u32, y: u32) -> Self {
        self.display.position = (x, y);
        self
    }

    /// Set widget size.
    pub fn with_size(mut self, width: u32, height: u32) -> Self {
        self.display.size = (width, height);
        self
    }

    /// Add a metric to display.
    pub fn with_metric(mut self, metric: impl Into<String>) -> Self {
        self.metrics.push(metric.into());
        self
    }

    /// Set aggregation configuration.
    pub fn with_aggregation(mut self, config: AggregationConfig) -> Self {
        self.aggregation = Some(config);
        self
    }

    /// Add a threshold.
    pub fn with_threshold(mut self, value: f64, color: impl Into<String>) -> Self {
        self.thresholds.push(ThresholdConfig {
            value,
            color: color.into(),
            label: None,
        });
        self
    }

    /// Set refresh interval.
    pub fn with_refresh_interval(mut self, seconds: u32) -> Self {
        self.refresh_interval = seconds;
        self
    }

    /// Add a custom option.
    pub fn with_option(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.options.insert(key.into(), value);
        self
    }
}

// ============================================================================
// Alert Threshold Configuration
// ============================================================================

/// Comparison operator for alerts.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ComparisonOperator {
    /// Greater than.
    GreaterThan,
    /// Greater than or equal.
    GreaterThanOrEqual,
    /// Less than.
    LessThan,
    /// Less than or equal.
    LessThanOrEqual,
    /// Equal to.
    Equal,
    /// Not equal to.
    NotEqual,
}

impl ComparisonOperator {
    /// Evaluate the comparison.
    pub fn evaluate(&self, actual: f64, threshold: f64) -> bool {
        match self {
            ComparisonOperator::GreaterThan => actual > threshold,
            ComparisonOperator::GreaterThanOrEqual => actual >= threshold,
            ComparisonOperator::LessThan => actual < threshold,
            ComparisonOperator::LessThanOrEqual => actual <= threshold,
            ComparisonOperator::Equal => (actual - threshold).abs() < f64::EPSILON,
            ComparisonOperator::NotEqual => (actual - threshold).abs() >= f64::EPSILON,
        }
    }
}

/// Severity level for alerts.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum AlertSeverity {
    /// Informational.
    Info,
    /// Warning level.
    Warning,
    /// Error level.
    Error,
    /// Critical level.
    Critical,
}

/// Alert threshold definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertThreshold {
    /// Threshold name.
    pub name: String,
    /// Metric name to monitor.
    pub metric: String,
    /// Comparison operator.
    pub operator: ComparisonOperator,
    /// Threshold value.
    pub threshold: f64,
    /// Alert severity.
    pub severity: AlertSeverity,
    /// Duration the condition must persist.
    pub duration: Duration,
    /// Message template for alerts.
    pub message: String,
    /// Whether threshold is enabled.
    pub enabled: bool,
}

impl AlertThreshold {
    /// Create a new alert threshold.
    pub fn new(
        name: impl Into<String>,
        metric: impl Into<String>,
        operator: ComparisonOperator,
        threshold: f64,
    ) -> Self {
        Self {
            name: name.into(),
            metric: metric.into(),
            operator,
            threshold,
            severity: AlertSeverity::Warning,
            duration: Duration::minutes(1),
            message: String::new(),
            enabled: true,
        }
    }

    /// Set alert severity.
    pub fn with_severity(mut self, severity: AlertSeverity) -> Self {
        self.severity = severity;
        self
    }

    /// Set duration condition.
    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    /// Set alert message.
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self
    }

    /// Check if a value triggers this threshold.
    pub fn check(&self, value: f64) -> bool {
        self.enabled && self.operator.evaluate(value, self.threshold)
    }
}

/// Alert instance when threshold is triggered.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    /// Alert ID.
    pub id: String,
    /// Threshold that triggered the alert.
    pub threshold_name: String,
    /// Metric name.
    pub metric: String,
    /// Actual value that triggered the alert.
    pub value: f64,
    /// Threshold value.
    pub threshold_value: f64,
    /// Alert severity.
    pub severity: AlertSeverity,
    /// Alert message.
    pub message: String,
    /// When the alert was triggered.
    pub triggered_at: DateTime<Utc>,
    /// When the alert was resolved (if applicable).
    pub resolved_at: Option<DateTime<Utc>>,
}

/// Alert threshold manager.
#[derive(Debug)]
pub struct AlertManager {
    /// Configured thresholds.
    thresholds: RwLock<Vec<AlertThreshold>>,
    /// Active alerts.
    active_alerts: RwLock<HashMap<String, Alert>>,
    /// Alert history.
    history: RwLock<VecDeque<Alert>>,
    /// Maximum history size.
    max_history: usize,
    /// Threshold violation start times.
    violation_starts: RwLock<HashMap<String, DateTime<Utc>>>,
}

impl Default for AlertManager {
    fn default() -> Self {
        Self::new()
    }
}

impl AlertManager {
    /// Create a new alert manager.
    pub fn new() -> Self {
        Self {
            thresholds: RwLock::new(Vec::new()),
            active_alerts: RwLock::new(HashMap::new()),
            history: RwLock::new(VecDeque::new()),
            max_history: 1000,
            violation_starts: RwLock::new(HashMap::new()),
        }
    }

    /// Add a threshold.
    pub fn add_threshold(&self, threshold: AlertThreshold) {
        self.thresholds.write().push(threshold);
    }

    /// Remove a threshold by name.
    pub fn remove_threshold(&self, name: &str) {
        self.thresholds.write().retain(|t| t.name != name);
    }

    /// Check a metric value against all relevant thresholds.
    pub fn check_metric(&self, metric: &str, value: f64) -> Vec<Alert> {
        let thresholds = self.thresholds.read();
        let now = Utc::now();
        let mut new_alerts = Vec::new();

        for threshold in thresholds.iter().filter(|t| t.metric == metric) {
            let alert_key = format!("{}:{}", threshold.name, threshold.metric);

            if threshold.check(value) {
                // Check duration condition
                let mut violation_starts = self.violation_starts.write();
                let start = violation_starts.entry(alert_key.clone()).or_insert(now);

                if now - *start >= threshold.duration {
                    // Create alert if not already active
                    let mut active = self.active_alerts.write();
                    if !active.contains_key(&alert_key) {
                        let alert = Alert {
                            id: uuid::Uuid::new_v4().to_string(),
                            threshold_name: threshold.name.clone(),
                            metric: metric.to_string(),
                            value,
                            threshold_value: threshold.threshold,
                            severity: threshold.severity,
                            message: threshold.message.clone(),
                            triggered_at: now,
                            resolved_at: None,
                        };
                        active.insert(alert_key.clone(), alert.clone());
                        new_alerts.push(alert);
                    }
                }
            } else {
                // Condition cleared - resolve alert if active
                self.violation_starts.write().remove(&alert_key);

                if let Some(mut alert) = self.active_alerts.write().remove(&alert_key) {
                    alert.resolved_at = Some(now);
                    let mut history = self.history.write();
                    if history.len() >= self.max_history {
                        history.pop_front();
                    }
                    history.push_back(alert);
                }
            }
        }

        new_alerts
    }

    /// Get all active alerts.
    pub fn active_alerts(&self) -> Vec<Alert> {
        self.active_alerts.read().values().cloned().collect()
    }

    /// Get alert history.
    pub fn history(&self) -> Vec<Alert> {
        self.history.read().iter().cloned().collect()
    }
}

// ============================================================================
// Performance Trend Analysis
// ============================================================================

/// Trend direction.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TrendDirection {
    /// Values are increasing.
    Increasing,
    /// Values are decreasing.
    Decreasing,
    /// Values are stable.
    Stable,
    /// Not enough data to determine.
    Unknown,
}

/// Trend analysis result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendAnalysis {
    /// Metric name.
    pub metric: String,
    /// Trend direction.
    pub direction: TrendDirection,
    /// Slope of the trend (rate of change).
    pub slope: f64,
    /// R-squared value (goodness of fit).
    pub r_squared: f64,
    /// Projected value at future time.
    pub projected_value: Option<f64>,
    /// Confidence interval (lower, upper).
    pub confidence_interval: Option<(f64, f64)>,
}

/// Performance trend analyzer.
#[derive(Debug)]
pub struct TrendAnalyzer {
    /// Minimum data points required for analysis.
    min_data_points: usize,
    /// Threshold for considering trend as stable.
    stability_threshold: f64,
}

impl Default for TrendAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl TrendAnalyzer {
    /// Create a new trend analyzer.
    pub fn new() -> Self {
        Self {
            min_data_points: 10,
            stability_threshold: 0.01,
        }
    }

    /// Set minimum data points.
    pub fn with_min_data_points(mut self, min: usize) -> Self {
        self.min_data_points = min;
        self
    }

    /// Set stability threshold.
    pub fn with_stability_threshold(mut self, threshold: f64) -> Self {
        self.stability_threshold = threshold;
        self
    }

    /// Analyze trend for a time series.
    pub fn analyze(&self, series: &TimeSeries) -> Result<TrendAnalysis> {
        let points = series.last_n(self.min_data_points.max(series.len()));

        if points.len() < self.min_data_points {
            return Ok(TrendAnalysis {
                metric: series.name.clone(),
                direction: TrendDirection::Unknown,
                slope: 0.0,
                r_squared: 0.0,
                projected_value: None,
                confidence_interval: None,
            });
        }

        // Perform linear regression
        let (slope, intercept, r_squared) = self.linear_regression(&points)?;

        // Determine trend direction
        let direction = if slope.abs() < self.stability_threshold {
            TrendDirection::Stable
        } else if slope > 0.0 {
            TrendDirection::Increasing
        } else {
            TrendDirection::Decreasing
        };

        // Project future value (1 step ahead)
        let n = points.len() as f64;
        let projected = intercept + slope * (n + 1.0);
        let projected_value = Some(projected);

        // Calculate confidence interval (95%)
        let values: Vec<f64> = points.iter().map(|p| p.value).collect();
        let mean: f64 = values.iter().sum::<f64>() / n;
        let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
        let std_dev = variance.sqrt();
        let margin = 1.96 * std_dev / n.sqrt(); // 95% CI
        let confidence_interval = Some((projected - margin, projected + margin));

        Ok(TrendAnalysis {
            metric: series.name.clone(),
            direction,
            slope,
            r_squared,
            projected_value,
            confidence_interval,
        })
    }

    /// Perform linear regression on data points.
    fn linear_regression(&self, points: &[&DataPoint]) -> Result<(f64, f64, f64)> {
        let n = points.len() as f64;
        if n < 2.0 {
            return Err(ObservabilityError::Other(
                "Insufficient data points for regression".to_string(),
            ));
        }

        // Use index as x values for simplicity
        let x_values: Vec<f64> = (0..points.len()).map(|i| i as f64).collect();
        let y_values: Vec<f64> = points.iter().map(|p| p.value).collect();

        let x_mean = x_values.iter().sum::<f64>() / n;
        let y_mean = y_values.iter().sum::<f64>() / n;

        let mut numerator = 0.0;
        let mut denominator = 0.0;

        for i in 0..points.len() {
            let x_diff = x_values[i] - x_mean;
            let y_diff = y_values[i] - y_mean;
            numerator += x_diff * y_diff;
            denominator += x_diff * x_diff;
        }

        let slope = if denominator.abs() < f64::EPSILON {
            0.0
        } else {
            numerator / denominator
        };
        let intercept = y_mean - slope * x_mean;

        // Calculate R-squared
        let ss_tot: f64 = y_values.iter().map(|y| (y - y_mean).powi(2)).sum();
        let ss_res: f64 = y_values
            .iter()
            .zip(x_values.iter())
            .map(|(y, x)| (y - (slope * x + intercept)).powi(2))
            .sum();

        let r_squared = if ss_tot.abs() < f64::EPSILON {
            0.0
        } else {
            1.0 - (ss_res / ss_tot)
        };

        Ok((slope, intercept, r_squared))
    }
}

// ============================================================================
// Resource Utilization Tracking
// ============================================================================

/// Resource type being tracked.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ResourceType {
    /// CPU utilization.
    Cpu,
    /// Memory usage.
    Memory,
    /// Disk I/O.
    DiskIo,
    /// Network I/O.
    NetworkIo,
    /// GPU utilization.
    Gpu,
    /// GPU memory.
    GpuMemory,
    /// File descriptors.
    FileDescriptors,
    /// Thread count.
    Threads,
}

/// Resource utilization snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceSnapshot {
    /// Timestamp of the snapshot.
    pub timestamp: DateTime<Utc>,
    /// Resource type.
    pub resource_type: ResourceType,
    /// Current utilization (0-100%).
    pub utilization: f64,
    /// Used amount.
    pub used: u64,
    /// Total available.
    pub total: u64,
    /// Additional metadata.
    pub metadata: HashMap<String, String>,
}

/// Resource utilization tracker.
#[derive(Debug)]
pub struct ResourceTracker {
    /// Resource history by type.
    resources: RwLock<HashMap<ResourceType, VecDeque<ResourceSnapshot>>>,
    /// Maximum history per resource.
    max_history: usize,
    /// Utilization thresholds for alerts.
    thresholds: RwLock<HashMap<ResourceType, f64>>,
}

impl Default for ResourceTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl ResourceTracker {
    /// Create a new resource tracker.
    pub fn new() -> Self {
        Self {
            resources: RwLock::new(HashMap::new()),
            max_history: 1000,
            thresholds: RwLock::new(HashMap::new()),
        }
    }

    /// Set utilization threshold for a resource type.
    pub fn set_threshold(&self, resource_type: ResourceType, threshold: f64) {
        self.thresholds.write().insert(resource_type, threshold);
    }

    /// Record a resource snapshot.
    pub fn record(&self, snapshot: ResourceSnapshot) {
        let mut resources = self.resources.write();
        let history = resources.entry(snapshot.resource_type).or_default();

        if history.len() >= self.max_history {
            history.pop_front();
        }
        history.push_back(snapshot);
    }

    /// Record CPU utilization.
    pub fn record_cpu(&self, utilization: f64, used_cores: u64, total_cores: u64) {
        self.record(ResourceSnapshot {
            timestamp: Utc::now(),
            resource_type: ResourceType::Cpu,
            utilization,
            used: used_cores,
            total: total_cores,
            metadata: HashMap::new(),
        });
    }

    /// Record memory usage.
    pub fn record_memory(&self, used_bytes: u64, total_bytes: u64) {
        let utilization = if total_bytes > 0 {
            (used_bytes as f64 / total_bytes as f64) * 100.0
        } else {
            0.0
        };
        self.record(ResourceSnapshot {
            timestamp: Utc::now(),
            resource_type: ResourceType::Memory,
            utilization,
            used: used_bytes,
            total: total_bytes,
            metadata: HashMap::new(),
        });
    }

    /// Get the latest snapshot for a resource type.
    pub fn latest(&self, resource_type: ResourceType) -> Option<ResourceSnapshot> {
        self.resources
            .read()
            .get(&resource_type)
            .and_then(|h| h.back().cloned())
    }

    /// Get history for a resource type.
    pub fn history(&self, resource_type: ResourceType) -> Vec<ResourceSnapshot> {
        self.resources
            .read()
            .get(&resource_type)
            .map(|h| h.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Get resources exceeding their threshold.
    pub fn exceeding_threshold(&self) -> Vec<(ResourceType, f64)> {
        let resources = self.resources.read();
        let thresholds = self.thresholds.read();

        let mut exceeding = Vec::new();
        for (resource_type, threshold) in thresholds.iter() {
            if let Some(history) = resources.get(resource_type)
                && let Some(latest) = history.back()
                && latest.utilization > *threshold
            {
                exceeding.push((*resource_type, latest.utilization));
            }
        }
        exceeding
    }

    /// Get average utilization for a resource type.
    pub fn average_utilization(&self, resource_type: ResourceType) -> Option<f64> {
        let resources = self.resources.read();
        resources.get(&resource_type).and_then(|history| {
            if history.is_empty() {
                None
            } else {
                let sum: f64 = history.iter().map(|s| s.utilization).sum();
                Some(sum / history.len() as f64)
            }
        })
    }
}

// ============================================================================
// Custom Metric Visualization
// ============================================================================

/// Visualization format for metrics.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum VisualizationFormat {
    /// JSON format.
    Json,
    /// Prometheus text format.
    PrometheusText,
    /// OpenMetrics format.
    OpenMetrics,
    /// CSV format.
    Csv,
    /// InfluxDB line protocol.
    InfluxLineProtocol,
}

/// Custom metric for visualization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomMetric {
    /// Metric name.
    pub name: String,
    /// Metric description.
    pub description: String,
    /// Metric type.
    pub metric_type: CustomMetricType,
    /// Unit of measurement.
    pub unit: MetricUnit,
    /// Labels for the metric.
    pub labels: HashMap<String, String>,
    /// Current value.
    pub value: f64,
    /// Timestamp.
    pub timestamp: DateTime<Utc>,
}

/// Type of custom metric.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CustomMetricType {
    /// Counter (monotonically increasing).
    Counter,
    /// Gauge (can go up and down).
    Gauge,
    /// Histogram.
    Histogram,
    /// Summary with percentiles.
    Summary,
}

impl CustomMetric {
    /// Create a new custom metric.
    pub fn new(name: impl Into<String>, metric_type: CustomMetricType, value: f64) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            metric_type,
            unit: MetricUnit::None,
            labels: HashMap::new(),
            value,
            timestamp: Utc::now(),
        }
    }

    /// Set description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Set unit.
    pub fn with_unit(mut self, unit: MetricUnit) -> Self {
        self.unit = unit;
        self
    }

    /// Add a label.
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    /// Format the metric for visualization.
    pub fn format(&self, format: VisualizationFormat) -> String {
        match format {
            VisualizationFormat::Json => {
                serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
            }
            VisualizationFormat::PrometheusText => {
                let labels = if self.labels.is_empty() {
                    String::new()
                } else {
                    let label_str: Vec<String> = self
                        .labels
                        .iter()
                        .map(|(k, v)| format!("{}=\"{}\"", k, v))
                        .collect();
                    format!("{{{}}}", label_str.join(","))
                };
                format!("{}{} {}", self.name, labels, self.value)
            }
            VisualizationFormat::OpenMetrics => {
                let type_str = match self.metric_type {
                    CustomMetricType::Counter => "counter",
                    CustomMetricType::Gauge => "gauge",
                    CustomMetricType::Histogram => "histogram",
                    CustomMetricType::Summary => "summary",
                };
                format!(
                    "# TYPE {} {}\n# HELP {} {}\n{} {}",
                    self.name, type_str, self.name, self.description, self.name, self.value
                )
            }
            VisualizationFormat::Csv => {
                format!(
                    "{},{},{},{}",
                    self.name,
                    self.value,
                    self.timestamp.to_rfc3339(),
                    serde_json::to_string(&self.labels).unwrap_or_else(|_| "{}".to_string())
                )
            }
            VisualizationFormat::InfluxLineProtocol => {
                let tags = if self.labels.is_empty() {
                    String::new()
                } else {
                    let tag_str: Vec<String> = self
                        .labels
                        .iter()
                        .map(|(k, v)| format!("{}={}", k, v))
                        .collect();
                    format!(",{}", tag_str.join(","))
                };
                format!(
                    "{}{} value={} {}",
                    self.name,
                    tags,
                    self.value,
                    self.timestamp.timestamp_nanos_opt().unwrap_or(0)
                )
            }
        }
    }
}

/// Custom metric registry for visualization.
#[derive(Debug)]
pub struct MetricRegistry {
    /// Registered metrics.
    metrics: RwLock<HashMap<String, CustomMetric>>,
    /// Metric history.
    history: RwLock<HashMap<String, VecDeque<CustomMetric>>>,
    /// Maximum history per metric.
    max_history: usize,
}

impl Default for MetricRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricRegistry {
    /// Create a new metric registry.
    pub fn new() -> Self {
        Self {
            metrics: RwLock::new(HashMap::new()),
            history: RwLock::new(HashMap::new()),
            max_history: 1000,
        }
    }

    /// Register or update a metric.
    pub fn register(&self, metric: CustomMetric) {
        let name = metric.name.clone();

        // Update current value
        self.metrics.write().insert(name.clone(), metric.clone());

        // Add to history
        let mut history = self.history.write();
        let hist = history.entry(name).or_default();
        if hist.len() >= self.max_history {
            hist.pop_front();
        }
        hist.push_back(metric);
    }

    /// Get a metric by name.
    pub fn get(&self, name: &str) -> Option<CustomMetric> {
        self.metrics.read().get(name).cloned()
    }

    /// Get all metrics.
    pub fn all(&self) -> Vec<CustomMetric> {
        self.metrics.read().values().cloned().collect()
    }

    /// Get metric history.
    pub fn get_history(&self, name: &str) -> Vec<CustomMetric> {
        self.history
            .read()
            .get(name)
            .map(|h| h.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Export all metrics in specified format.
    pub fn export(&self, format: VisualizationFormat) -> String {
        let metrics = self.metrics.read();
        match format {
            VisualizationFormat::Json => {
                let values: Vec<&CustomMetric> = metrics.values().collect();
                serde_json::to_string_pretty(&values).unwrap_or_else(|_| "[]".to_string())
            }
            VisualizationFormat::Csv => {
                let mut output = String::from("name,value,timestamp,labels\n");
                for metric in metrics.values() {
                    output.push_str(&metric.format(format));
                    output.push('\n');
                }
                output
            }
            _ => metrics
                .values()
                .map(|m| m.format(format))
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}

// ============================================================================
// Performance Dashboard
// ============================================================================

/// Complete performance dashboard with all components.
#[derive(Debug)]
pub struct PerformanceDashboard {
    /// Dashboard name.
    pub name: String,
    /// Dashboard description.
    pub description: String,
    /// Dashboard widgets.
    widgets: RwLock<Vec<Widget>>,
    /// Time series data.
    time_series: RwLock<HashMap<String, TimeSeries>>,
    /// Alert manager.
    pub alert_manager: AlertManager,
    /// Trend analyzer.
    pub trend_analyzer: TrendAnalyzer,
    /// Resource tracker.
    pub resource_tracker: ResourceTracker,
    /// Custom metric registry.
    pub metric_registry: MetricRegistry,
}

impl PerformanceDashboard {
    /// Create a new performance dashboard.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            widgets: RwLock::new(Vec::new()),
            time_series: RwLock::new(HashMap::new()),
            alert_manager: AlertManager::new(),
            trend_analyzer: TrendAnalyzer::new(),
            resource_tracker: ResourceTracker::new(),
            metric_registry: MetricRegistry::new(),
        }
    }

    /// Set description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Add a widget to the dashboard.
    pub fn add_widget(&self, widget: Widget) {
        self.widgets.write().push(widget);
    }

    /// Remove a widget by ID.
    pub fn remove_widget(&self, id: &str) {
        self.widgets.write().retain(|w| w.id != id);
    }

    /// Get all widgets.
    pub fn widgets(&self) -> Vec<Widget> {
        self.widgets.read().clone()
    }

    /// Create or get a time series.
    pub fn time_series(
        &self,
        name: impl Into<String>,
        description: impl Into<String>,
        unit: MetricUnit,
    ) -> String {
        let name = name.into();
        let mut series_map = self.time_series.write();
        if !series_map.contains_key(&name) {
            series_map.insert(name.clone(), TimeSeries::new(&name, description, unit));
        }
        name
    }

    /// Add a value to a time series.
    pub fn add_value(&self, series_name: &str, value: f64) {
        let mut series_map = self.time_series.write();
        if let Some(series) = series_map.get_mut(series_name) {
            series.add_value(value);
        }
    }

    /// Get time series statistics.
    pub fn get_statistics(&self, series_name: &str) -> Option<TimeSeriesStats> {
        self.time_series
            .read()
            .get(series_name)
            .and_then(|s| s.statistics())
    }

    /// Analyze trend for a time series.
    pub fn analyze_trend(&self, series_name: &str) -> Result<TrendAnalysis> {
        let series_map = self.time_series.read();
        let series = series_map.get(series_name).ok_or_else(|| {
            ObservabilityError::NotFound(format!("Time series '{}' not found", series_name))
        })?;
        self.trend_analyzer.analyze(series)
    }

    /// Get a summary of the dashboard state.
    pub fn summary(&self) -> DashboardSummary {
        let widgets = self.widgets.read();
        let series = self.time_series.read();
        let active_alerts = self.alert_manager.active_alerts();
        let exceeding = self.resource_tracker.exceeding_threshold();

        DashboardSummary {
            name: self.name.clone(),
            widget_count: widgets.len(),
            time_series_count: series.len(),
            active_alert_count: active_alerts.len(),
            resources_exceeding_threshold: exceeding.len(),
            critical_alerts: active_alerts
                .iter()
                .filter(|a| a.severity == AlertSeverity::Critical)
                .count(),
        }
    }
}

/// Dashboard summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardSummary {
    /// Dashboard name.
    pub name: String,
    /// Number of widgets.
    pub widget_count: usize,
    /// Number of time series.
    pub time_series_count: usize,
    /// Number of active alerts.
    pub active_alert_count: usize,
    /// Number of resources exceeding threshold.
    pub resources_exceeding_threshold: usize,
    /// Number of critical alerts.
    pub critical_alerts: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_time_series() {
        let mut series = TimeSeries::new("test_metric", "Test metric", MetricUnit::Milliseconds);

        for i in 0..100 {
            series.add_value(i as f64);
        }

        assert_eq!(series.len(), 100);
        assert!(series.statistics().is_some());

        let stats = series.statistics().expect("Stats should exist");
        assert_eq!(stats.count, 100);
        assert!((stats.mean - 49.5).abs() < 0.1);
    }

    #[test]
    fn test_data_point_with_labels() {
        let point = DataPoint::now(42.0)
            .with_label("region", "us-west")
            .with_label("service", "api");

        assert_eq!(point.value, 42.0);
        assert_eq!(point.labels.len(), 2);
        assert_eq!(point.labels.get("region"), Some(&"us-west".to_string()));
    }

    #[test]
    fn test_widget_builder() {
        let widget = Widget::new("cpu_gauge", WidgetType::Gauge, "CPU Usage")
            .with_position(0, 0)
            .with_size(4, 4)
            .with_metric("cpu_percent")
            .with_threshold(80.0, "#FFA500")
            .with_threshold(95.0, "#FF0000")
            .with_refresh_interval(5);

        assert_eq!(widget.id, "cpu_gauge");
        assert_eq!(widget.display.title, "CPU Usage");
        assert_eq!(widget.thresholds.len(), 2);
        assert_eq!(widget.refresh_interval, 5);
    }

    #[test]
    fn test_alert_threshold() {
        let threshold = AlertThreshold::new(
            "high_cpu",
            "cpu_percent",
            ComparisonOperator::GreaterThan,
            80.0,
        )
        .with_severity(AlertSeverity::Warning)
        .with_message("CPU usage is high");

        assert!(threshold.check(85.0));
        assert!(!threshold.check(75.0));
    }

    #[test]
    fn test_trend_analyzer() {
        let mut series = TimeSeries::new("increasing", "Increasing metric", MetricUnit::Count);

        for i in 0..50 {
            series.add(DataPoint::with_timestamp(
                Utc::now() - Duration::seconds(50 - i),
                i as f64 * 2.0,
            ));
        }

        let analyzer = TrendAnalyzer::new().with_min_data_points(5);
        let analysis = analyzer.analyze(&series).expect("Analysis should succeed");

        assert_eq!(analysis.direction, TrendDirection::Increasing);
        assert!(analysis.slope > 0.0);
    }

    #[test]
    fn test_resource_tracker() {
        let tracker = ResourceTracker::new();
        tracker.set_threshold(ResourceType::Cpu, 80.0);

        tracker.record_cpu(75.0, 6, 8);
        assert!(tracker.exceeding_threshold().is_empty());

        tracker.record_cpu(85.0, 7, 8);
        let exceeding = tracker.exceeding_threshold();
        assert_eq!(exceeding.len(), 1);
        assert_eq!(exceeding[0].0, ResourceType::Cpu);
    }

    #[test]
    fn test_custom_metric_formats() {
        let metric = CustomMetric::new("request_count", CustomMetricType::Counter, 42.0)
            .with_description("Total request count")
            .with_label("service", "api")
            .with_unit(MetricUnit::Count);

        let prometheus_text = metric.format(VisualizationFormat::PrometheusText);
        assert!(prometheus_text.contains("request_count"));
        assert!(prometheus_text.contains("42"));

        let json = metric.format(VisualizationFormat::Json);
        assert!(json.contains("request_count"));
    }

    #[test]
    fn test_metric_registry() {
        let registry = MetricRegistry::new();

        registry.register(CustomMetric::new(
            "cpu_usage",
            CustomMetricType::Gauge,
            75.5,
        ));
        registry.register(CustomMetric::new(
            "memory_usage",
            CustomMetricType::Gauge,
            8_589_934_592.0,
        ));

        assert_eq!(registry.all().len(), 2);
        assert!(registry.get("cpu_usage").is_some());

        let export = registry.export(VisualizationFormat::PrometheusText);
        assert!(export.contains("cpu_usage"));
        assert!(export.contains("memory_usage"));
    }

    #[test]
    fn test_performance_dashboard() {
        let dashboard = PerformanceDashboard::new("Test Dashboard")
            .with_description("Testing dashboard functionality");

        // Add widgets
        dashboard.add_widget(Widget::new("w1", WidgetType::Gauge, "CPU"));
        dashboard.add_widget(Widget::new("w2", WidgetType::LineChart, "Latency"));

        assert_eq!(dashboard.widgets().len(), 2);

        // Create and populate time series
        let series_name =
            dashboard.time_series("latency", "Request latency", MetricUnit::Milliseconds);
        for i in 0..20 {
            dashboard.add_value(&series_name, i as f64 * 10.0);
        }

        let stats = dashboard.get_statistics(&series_name);
        assert!(stats.is_some());

        // Get summary
        let summary = dashboard.summary();
        assert_eq!(summary.widget_count, 2);
        assert_eq!(summary.time_series_count, 1);
    }

    #[test]
    fn test_aggregation_config() {
        let config = AggregationConfig {
            method: AggregationType::Average,
            window: Duration::minutes(5),
            step: Duration::seconds(30),
            percentile_value: None,
        };

        assert_eq!(config.method, AggregationType::Average);
    }

    #[test]
    fn test_comparison_operators() {
        assert!(ComparisonOperator::GreaterThan.evaluate(10.0, 5.0));
        assert!(!ComparisonOperator::GreaterThan.evaluate(5.0, 10.0));

        assert!(ComparisonOperator::LessThan.evaluate(5.0, 10.0));
        assert!(!ComparisonOperator::LessThan.evaluate(10.0, 5.0));

        assert!(ComparisonOperator::Equal.evaluate(5.0, 5.0));
        assert!(ComparisonOperator::NotEqual.evaluate(5.0, 10.0));
    }

    #[test]
    fn test_percentile_calculation() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];

        assert_eq!(percentile(&values, 50.0), 6.0);
        assert_eq!(percentile(&values, 0.0), 1.0);
        assert_eq!(percentile(&values, 100.0), 10.0);
    }
}
