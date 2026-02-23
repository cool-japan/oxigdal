//! Anomaly detection engine for metrics.

pub mod ml;
pub mod rules;
pub mod statistical;
pub mod trend;

use crate::error::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Anomaly detection result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Anomaly {
    /// Timestamp of the anomaly.
    pub timestamp: DateTime<Utc>,

    /// Metric name.
    pub metric_name: String,

    /// Observed value.
    pub observed_value: f64,

    /// Expected value.
    pub expected_value: f64,

    /// Anomaly score (0.0 to 1.0).
    pub score: f64,

    /// Severity level.
    pub severity: AnomalySeverity,

    /// Anomaly type.
    pub anomaly_type: AnomalyType,

    /// Description.
    pub description: String,
}

/// Anomaly severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnomalySeverity {
    /// Low severity anomaly, minor deviation from baseline.
    Low,
    /// Medium severity anomaly, noticeable deviation requiring attention.
    Medium,
    /// High severity anomaly, significant deviation requiring immediate action.
    High,
    /// Critical severity anomaly, severe deviation requiring urgent response.
    Critical,
}

/// Anomaly type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnomalyType {
    /// Sudden spike in metric value.
    Spike,

    /// Sudden drop in metric value.
    Drop,

    /// Gradual increasing trend.
    UpwardTrend,

    /// Gradual decreasing trend.
    DownwardTrend,

    /// Unusual pattern.
    Pattern,

    /// Missing data.
    MissingData,
}

/// Anomaly detector trait.
pub trait AnomalyDetector: Send + Sync {
    /// Detect anomalies in the given data points.
    fn detect(&self, data: &[DataPoint]) -> Result<Vec<Anomaly>>;

    /// Update baseline with new data.
    fn update_baseline(&mut self, data: &[DataPoint]) -> Result<()>;
}

/// Data point for anomaly detection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataPoint {
    /// Timestamp when the data point was recorded.
    pub timestamp: DateTime<Utc>,
    /// Numeric value of the data point.
    pub value: f64,
}

impl DataPoint {
    /// Create a new data point.
    pub fn new(timestamp: DateTime<Utc>, value: f64) -> Self {
        Self { timestamp, value }
    }
}

/// Baseline statistics for anomaly detection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Baseline {
    /// Mean (average) value of the baseline.
    pub mean: f64,
    /// Standard deviation of the baseline values.
    pub std_dev: f64,
    /// Minimum value observed in the baseline.
    pub min: f64,
    /// Maximum value observed in the baseline.
    pub max: f64,
    /// Number of data points in the baseline.
    pub count: usize,
}

impl Baseline {
    /// Calculate baseline from data points.
    pub fn from_data(data: &[DataPoint]) -> Result<Self> {
        if data.is_empty() {
            return Err(crate::error::ObservabilityError::AnomalyDetectionError(
                "Cannot calculate baseline from empty data".to_string(),
            ));
        }

        let values: Vec<f64> = data.iter().map(|d| d.value).collect();
        let count = values.len();
        let sum: f64 = values.iter().sum();
        let mean = sum / count as f64;

        let variance: f64 = values
            .iter()
            .map(|v| {
                let diff = v - mean;
                diff * diff
            })
            .sum::<f64>()
            / count as f64;

        let std_dev = variance.sqrt();
        let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

        Ok(Self {
            mean,
            std_dev,
            min,
            max,
            count,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_baseline_calculation() {
        let data = vec![
            DataPoint::new(Utc::now(), 10.0),
            DataPoint::new(Utc::now(), 20.0),
            DataPoint::new(Utc::now(), 30.0),
        ];

        let baseline = Baseline::from_data(&data).expect("Failed to calculate baseline");
        assert_eq!(baseline.mean, 20.0);
        assert_eq!(baseline.min, 10.0);
        assert_eq!(baseline.max, 30.0);
    }
}
