//! Time-series data types for IoT

use crate::error::Result;
use crate::iot::IotMessage;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// Time-series data point
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeSeriesPoint {
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Value
    pub value: f64,
    /// Tags (optional metadata)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<serde_json::Map<String, serde_json::Value>>,
}

impl TimeSeriesPoint {
    /// Create a new time-series point
    pub fn new(timestamp: DateTime<Utc>, value: f64) -> Self {
        Self {
            timestamp,
            value,
            tags: None,
        }
    }

    /// Create a new point with current timestamp
    pub fn now(value: f64) -> Self {
        Self::new(Utc::now(), value)
    }

    /// Add a tag
    pub fn with_tag(mut self, key: impl Into<String>, value: impl Into<serde_json::Value>) -> Self {
        if self.tags.is_none() {
            self.tags = Some(serde_json::Map::new());
        }
        if let Some(ref mut tags) = self.tags {
            tags.insert(key.into(), value.into());
        }
        self
    }
}

/// Time-series message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeSeriesMessage {
    /// Device ID
    pub device_id: String,
    /// Metric name
    pub metric: String,
    /// Data points
    pub points: Vec<TimeSeriesPoint>,
    /// Unit of measurement
    pub unit: String,
}

impl TimeSeriesMessage {
    /// Create a new time-series message
    pub fn new(
        device_id: impl Into<String>,
        metric: impl Into<String>,
        unit: impl Into<String>,
    ) -> Self {
        Self {
            device_id: device_id.into(),
            metric: metric.into(),
            points: Vec::new(),
            unit: unit.into(),
        }
    }

    /// Add a point
    pub fn add_point(mut self, point: TimeSeriesPoint) -> Self {
        self.points.push(point);
        self
    }

    /// Add a point with timestamp and value
    pub fn add_value(mut self, timestamp: DateTime<Utc>, value: f64) -> Self {
        self.points.push(TimeSeriesPoint::new(timestamp, value));
        self
    }

    /// Add a point with current timestamp
    pub fn add_value_now(mut self, value: f64) -> Self {
        self.points.push(TimeSeriesPoint::now(value));
        self
    }

    /// Convert to IoT message
    pub fn to_iot_message(&self) -> Result<IotMessage> {
        let payload = serde_json::to_value(self)?;
        Ok(IotMessage::new(
            self.device_id.clone(),
            "timeseries",
            payload,
        ))
    }

    /// Get time range
    pub fn time_range(&self) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
        if self.points.is_empty() {
            return None;
        }

        let mut min_time = self.points[0].timestamp;
        let mut max_time = self.points[0].timestamp;

        for point in &self.points {
            if point.timestamp < min_time {
                min_time = point.timestamp;
            }
            if point.timestamp > max_time {
                max_time = point.timestamp;
            }
        }

        Some((min_time, max_time))
    }

    /// Get statistics
    pub fn statistics(&self) -> Option<TimeSeriesStats> {
        if self.points.is_empty() {
            return None;
        }

        let values: Vec<f64> = self.points.iter().map(|p| p.value).collect();
        let count = values.len();
        let sum: f64 = values.iter().sum();
        let mean = sum / count as f64;

        let mut min = values[0];
        let mut max = values[0];

        for &v in &values {
            if v < min {
                min = v;
            }
            if v > max {
                max = v;
            }
        }

        // Calculate standard deviation
        let variance: f64 = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / count as f64;
        let std_dev = variance.sqrt();

        Some(TimeSeriesStats {
            count,
            sum,
            mean,
            min,
            max,
            std_dev,
        })
    }
}

/// Time-series statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeSeriesStats {
    /// Number of points
    pub count: usize,
    /// Sum of values
    pub sum: f64,
    /// Mean value
    pub mean: f64,
    /// Minimum value
    pub min: f64,
    /// Maximum value
    pub max: f64,
    /// Standard deviation
    pub std_dev: f64,
}

/// Aggregation type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Aggregation {
    /// Mean/average
    Mean,
    /// Sum
    Sum,
    /// Minimum
    Min,
    /// Maximum
    Max,
    /// Count
    Count,
    /// First value
    First,
    /// Last value
    Last,
}

/// Aggregated time-series data
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct AggregatedTimeSeries {
    /// Device ID
    pub device_id: String,
    /// Metric name
    pub metric: String,
    /// Aggregation type
    pub aggregation: Aggregation,
    /// Window duration (in seconds)
    pub window_seconds: i64,
    /// Data points
    pub points: Vec<AggregatedPoint>,
    /// Unit of measurement
    pub unit: String,
}

// Public API for time-series aggregation
#[allow(dead_code)]
impl AggregatedTimeSeries {
    /// Create a new aggregated time-series
    pub fn new(
        device_id: impl Into<String>,
        metric: impl Into<String>,
        aggregation: Aggregation,
        window_seconds: i64,
        unit: impl Into<String>,
    ) -> Self {
        Self {
            device_id: device_id.into(),
            metric: metric.into(),
            aggregation,
            window_seconds,
            points: Vec::new(),
            unit: unit.into(),
        }
    }

    /// Aggregate raw time-series data
    pub fn from_timeseries(
        ts: &TimeSeriesMessage,
        aggregation: Aggregation,
        window_seconds: i64,
    ) -> Self {
        let mut result = Self::new(
            ts.device_id.clone(),
            ts.metric.clone(),
            aggregation,
            window_seconds,
            ts.unit.clone(),
        );

        if ts.points.is_empty() {
            return result;
        }

        // Group points by window
        let window_duration =
            Duration::try_seconds(window_seconds).unwrap_or(Duration::seconds(60)); // Fallback to 60 seconds
        let mut windows: Vec<Vec<&TimeSeriesPoint>> = Vec::new();
        let mut current_window: Vec<&TimeSeriesPoint> = Vec::new();
        let mut window_start = ts.points[0].timestamp;

        for point in &ts.points {
            if point.timestamp >= window_start + window_duration {
                if !current_window.is_empty() {
                    windows.push(current_window);
                    current_window = Vec::new();
                }
                window_start = point.timestamp;
            }
            current_window.push(point);
        }

        if !current_window.is_empty() {
            windows.push(current_window);
        }

        // Aggregate each window
        for window in windows {
            if let Some(point) = Self::aggregate_window(&window, aggregation) {
                result.points.push(point);
            }
        }

        result
    }

    /// Aggregate a single window
    fn aggregate_window(
        window: &[&TimeSeriesPoint],
        aggregation: Aggregation,
    ) -> Option<AggregatedPoint> {
        if window.is_empty() {
            return None;
        }

        let timestamp = window[0].timestamp;
        let values: Vec<f64> = window.iter().map(|p| p.value).collect();

        let value = match aggregation {
            Aggregation::Mean => values.iter().sum::<f64>() / values.len() as f64,
            Aggregation::Sum => values.iter().sum(),
            Aggregation::Min => values.iter().copied().fold(f64::INFINITY, f64::min),
            Aggregation::Max => values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            Aggregation::Count => values.len() as f64,
            Aggregation::First => values[0],
            Aggregation::Last => *values.last().unwrap_or(&0.0),
        };

        Some(AggregatedPoint {
            timestamp,
            value,
            count: values.len(),
        })
    }

    /// Convert to IoT message
    pub fn to_iot_message(&self) -> Result<IotMessage> {
        let payload = serde_json::to_value(self)?;
        Ok(IotMessage::new(
            self.device_id.clone(),
            "timeseries_agg",
            payload,
        ))
    }
}

/// Aggregated data point
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct AggregatedPoint {
    /// Window start timestamp
    pub timestamp: DateTime<Utc>,
    /// Aggregated value
    pub value: f64,
    /// Number of points in window
    pub count: usize,
}

#[cfg(test)]
#[allow(clippy::panic, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_timeseries_point() {
        let point = TimeSeriesPoint::now(42.0)
            .with_tag("sensor", "temp1")
            .with_tag("location", "room1");

        assert_eq!(point.value, 42.0);
        assert!(point.tags.is_some());
    }

    #[test]
    fn test_timeseries_message() {
        let now = Utc::now();
        let msg = TimeSeriesMessage::new("device-001", "temperature", "celsius")
            .add_value(now, 25.0)
            .add_value(
                now + Duration::try_minutes(1).expect("Duration should be valid"),
                25.5,
            )
            .add_value(
                now + Duration::try_minutes(2).expect("Duration should be valid"),
                26.0,
            );

        assert_eq!(msg.points.len(), 3);
        assert_eq!(msg.metric, "temperature");
        assert_eq!(msg.unit, "celsius");
    }

    #[test]
    fn test_timeseries_stats() {
        let now = Utc::now();
        let msg = TimeSeriesMessage::new("device-001", "temperature", "celsius")
            .add_value(now, 25.0)
            .add_value(
                now + Duration::try_minutes(1).expect("Duration should be valid"),
                30.0,
            )
            .add_value(
                now + Duration::try_minutes(2).expect("Duration should be valid"),
                35.0,
            );

        let stats = msg.statistics().expect("Statistics should be available");
        assert_eq!(stats.count, 3);
        assert_eq!(stats.mean, 30.0);
        assert_eq!(stats.min, 25.0);
        assert_eq!(stats.max, 35.0);
    }

    #[test]
    fn test_time_range() {
        let now = Utc::now();
        let later = now + Duration::try_hours(1).expect("Duration should be valid");

        let msg = TimeSeriesMessage::new("device-001", "temperature", "celsius")
            .add_value(now, 25.0)
            .add_value(later, 30.0);

        let (min, max) = msg.time_range().expect("Time range should be available");
        assert_eq!(min, now);
        assert_eq!(max, later);
    }

    #[test]
    fn test_aggregation() {
        let now = Utc::now();
        let msg = TimeSeriesMessage::new("device-001", "temperature", "celsius")
            .add_value(now, 10.0)
            .add_value(now, 20.0)
            .add_value(now, 30.0);

        let agg = AggregatedTimeSeries::from_timeseries(&msg, Aggregation::Mean, 60);
        assert_eq!(agg.points.len(), 1);
        assert_eq!(agg.points[0].value, 20.0); // Mean of 10, 20, 30
        assert_eq!(agg.points[0].count, 3);
    }

    #[test]
    fn test_aggregation_types() {
        let now = Utc::now();
        let msg = TimeSeriesMessage::new("device-001", "value", "units")
            .add_value(now, 10.0)
            .add_value(now, 20.0)
            .add_value(now, 30.0);

        let mean_agg = AggregatedTimeSeries::from_timeseries(&msg, Aggregation::Mean, 60);
        assert_eq!(mean_agg.points[0].value, 20.0);

        let sum_agg = AggregatedTimeSeries::from_timeseries(&msg, Aggregation::Sum, 60);
        assert_eq!(sum_agg.points[0].value, 60.0);

        let min_agg = AggregatedTimeSeries::from_timeseries(&msg, Aggregation::Min, 60);
        assert_eq!(min_agg.points[0].value, 10.0);

        let max_agg = AggregatedTimeSeries::from_timeseries(&msg, Aggregation::Max, 60);
        assert_eq!(max_agg.points[0].value, 30.0);

        let count_agg = AggregatedTimeSeries::from_timeseries(&msg, Aggregation::Count, 60);
        assert_eq!(count_agg.points[0].value, 3.0);
    }
}
