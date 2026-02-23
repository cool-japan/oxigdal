//! Trend-based anomaly detection.

use super::{Anomaly, AnomalyDetector, AnomalySeverity, AnomalyType, DataPoint};
use crate::error::Result;

/// Trend detector for identifying gradual changes.
pub struct TrendDetector {
    window_size: usize,
    threshold: f64,
}

impl TrendDetector {
    /// Create a new trend detector.
    pub fn new(window_size: usize, threshold: f64) -> Self {
        Self {
            window_size,
            threshold,
        }
    }

    /// Calculate linear regression slope for trend detection.
    fn calculate_slope(&self, values: &[f64]) -> f64 {
        let n = values.len() as f64;
        if n < 2.0 {
            return 0.0;
        }

        let x_mean = (n - 1.0) / 2.0;
        let y_mean: f64 = values.iter().sum::<f64>() / n;

        let mut numerator = 0.0;
        let mut denominator = 0.0;

        for (i, value) in values.iter().enumerate() {
            let x_diff = i as f64 - x_mean;
            let y_diff = value - y_mean;
            numerator += x_diff * y_diff;
            denominator += x_diff * x_diff;
        }

        if denominator == 0.0 {
            0.0
        } else {
            numerator / denominator
        }
    }
}

impl AnomalyDetector for TrendDetector {
    fn detect(&self, data: &[DataPoint]) -> Result<Vec<Anomaly>> {
        if data.len() < self.window_size {
            return Ok(Vec::new());
        }

        let mut anomalies = Vec::new();

        for i in self.window_size..data.len() {
            let window: Vec<f64> = data[i - self.window_size..i]
                .iter()
                .map(|d| d.value)
                .collect();

            let slope = self.calculate_slope(&window);

            if slope.abs() > self.threshold {
                let severity = if slope.abs() > self.threshold * 2.0 {
                    AnomalySeverity::High
                } else {
                    AnomalySeverity::Medium
                };

                let anomaly_type = if slope > 0.0 {
                    AnomalyType::UpwardTrend
                } else {
                    AnomalyType::DownwardTrend
                };

                anomalies.push(Anomaly {
                    timestamp: data[i].timestamp,
                    metric_name: "trend".to_string(),
                    observed_value: data[i].value,
                    expected_value: data[i - 1].value,
                    score: (slope.abs() / self.threshold).min(1.0),
                    severity,
                    anomaly_type,
                    description: format!("Trend detected with slope: {:.4}", slope),
                });
            }
        }

        Ok(anomalies)
    }

    fn update_baseline(&mut self, _data: &[DataPoint]) -> Result<()> {
        Ok(())
    }
}

/// Seasonal pattern detector.
pub struct SeasonalDetector {
    period: usize,
    threshold: f64,
}

impl SeasonalDetector {
    /// Create a new seasonal detector.
    pub fn new(period: usize, threshold: f64) -> Self {
        Self { period, threshold }
    }

    /// Calculate seasonal component.
    fn calculate_seasonal_component(&self, data: &[DataPoint]) -> Vec<f64> {
        let mut seasonal = vec![0.0; self.period];
        let mut counts = vec![0; self.period];

        for (i, point) in data.iter().enumerate() {
            let season_idx = i % self.period;
            seasonal[season_idx] += point.value;
            counts[season_idx] += 1;
        }

        for i in 0..self.period {
            if counts[i] > 0 {
                seasonal[i] /= counts[i] as f64;
            }
        }

        seasonal
    }
}

impl AnomalyDetector for SeasonalDetector {
    fn detect(&self, data: &[DataPoint]) -> Result<Vec<Anomaly>> {
        if data.len() < self.period * 2 {
            return Ok(Vec::new());
        }

        let seasonal = self.calculate_seasonal_component(data);
        let mut anomalies = Vec::new();

        for (i, point) in data.iter().enumerate() {
            let season_idx = i % self.period;
            let expected = seasonal[season_idx];
            let deviation = (point.value - expected).abs();

            if deviation > self.threshold {
                let score = (deviation / self.threshold).min(1.0);
                let severity = if score > 0.75 {
                    AnomalySeverity::High
                } else if score > 0.5 {
                    AnomalySeverity::Medium
                } else {
                    AnomalySeverity::Low
                };

                anomalies.push(Anomaly {
                    timestamp: point.timestamp,
                    metric_name: "seasonal".to_string(),
                    observed_value: point.value,
                    expected_value: expected,
                    score,
                    severity,
                    anomaly_type: AnomalyType::Pattern,
                    description: format!(
                        "Seasonal deviation: expected {:.2}, got {:.2}",
                        expected, point.value
                    ),
                });
            }
        }

        Ok(anomalies)
    }

    fn update_baseline(&mut self, _data: &[DataPoint]) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_trend_detector() {
        let detector = TrendDetector::new(5, 1.0);

        // Create data with upward trend
        let data: Vec<DataPoint> = (0..10)
            .map(|i| DataPoint::new(Utc::now(), (i * 2) as f64))
            .collect();

        let anomalies = detector.detect(&data).expect("Failed to detect");
        assert!(!anomalies.is_empty());
    }

    #[test]
    fn test_seasonal_detector() {
        let detector = SeasonalDetector::new(7, 5.0);

        // Create seasonal data (weekly pattern)
        let data: Vec<DataPoint> = (0..21)
            .map(|i| {
                let value = if i % 7 == 0 { 100.0 } else { 50.0 };
                DataPoint::new(Utc::now(), value)
            })
            .collect();

        let _anomalies = detector.detect(&data).expect("Failed to detect");
    }
}
