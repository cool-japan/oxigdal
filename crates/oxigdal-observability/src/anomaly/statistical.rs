//! Statistical anomaly detection methods.

use super::{Anomaly, AnomalyDetector, AnomalySeverity, AnomalyType, Baseline, DataPoint};
use crate::error::Result;
use parking_lot::RwLock;

/// Z-score based anomaly detector.
pub struct ZScoreDetector {
    baseline: RwLock<Option<Baseline>>,
    threshold: f64,
}

impl ZScoreDetector {
    /// Create a new Z-score detector with the given threshold.
    pub fn new(threshold: f64) -> Self {
        Self {
            baseline: RwLock::new(None),
            threshold,
        }
    }
}

impl AnomalyDetector for ZScoreDetector {
    fn detect(&self, data: &[DataPoint]) -> Result<Vec<Anomaly>> {
        let baseline = self.baseline.read();
        let baseline = baseline.as_ref().ok_or_else(|| {
            crate::error::ObservabilityError::AnomalyDetectionError(
                "Baseline not established".to_string(),
            )
        })?;

        let mut anomalies = Vec::new();

        for point in data {
            let z_score = (point.value - baseline.mean).abs() / baseline.std_dev;

            if z_score > self.threshold {
                let score = (z_score / (self.threshold * 2.0)).min(1.0);
                let severity = if z_score > self.threshold * 3.0 {
                    AnomalySeverity::Critical
                } else if z_score > self.threshold * 2.0 {
                    AnomalySeverity::High
                } else if z_score > self.threshold * 1.5 {
                    AnomalySeverity::Medium
                } else {
                    AnomalySeverity::Low
                };

                let anomaly_type = if point.value > baseline.mean {
                    AnomalyType::Spike
                } else {
                    AnomalyType::Drop
                };

                anomalies.push(Anomaly {
                    timestamp: point.timestamp,
                    metric_name: "unknown".to_string(),
                    observed_value: point.value,
                    expected_value: baseline.mean,
                    score,
                    severity,
                    anomaly_type,
                    description: format!(
                        "Z-score: {:.2}, threshold: {:.2}",
                        z_score, self.threshold
                    ),
                });
            }
        }

        Ok(anomalies)
    }

    fn update_baseline(&mut self, data: &[DataPoint]) -> Result<()> {
        let new_baseline = Baseline::from_data(data)?;
        *self.baseline.write() = Some(new_baseline);
        Ok(())
    }
}

/// IQR (Interquartile Range) based anomaly detector.
pub struct IqrDetector {
    baseline: RwLock<Option<IqrBaseline>>,
    multiplier: f64,
}

#[derive(Debug, Clone)]
struct IqrBaseline {
    q1: f64,
    q3: f64,
    iqr: f64,
    lower_bound: f64,
    upper_bound: f64,
}

impl IqrDetector {
    /// Create a new IQR detector with the given multiplier.
    pub fn new(multiplier: f64) -> Self {
        Self {
            baseline: RwLock::new(None),
            multiplier,
        }
    }

    fn calculate_baseline(data: &[DataPoint], multiplier: f64) -> Result<IqrBaseline> {
        if data.is_empty() {
            return Err(crate::error::ObservabilityError::AnomalyDetectionError(
                "Cannot calculate baseline from empty data".to_string(),
            ));
        }

        let mut values: Vec<f64> = data.iter().map(|d| d.value).collect();
        values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let n = values.len();
        let q1_idx = n / 4;
        let q3_idx = (3 * n) / 4;

        let q1 = values[q1_idx];
        let q3 = values[q3_idx];
        let iqr = q3 - q1;

        let lower_bound = q1 - multiplier * iqr;
        let upper_bound = q3 + multiplier * iqr;

        Ok(IqrBaseline {
            q1,
            q3,
            iqr,
            lower_bound,
            upper_bound,
        })
    }
}

impl AnomalyDetector for IqrDetector {
    fn detect(&self, data: &[DataPoint]) -> Result<Vec<Anomaly>> {
        let baseline = self.baseline.read();
        let baseline = baseline.as_ref().ok_or_else(|| {
            crate::error::ObservabilityError::AnomalyDetectionError(
                "Baseline not established".to_string(),
            )
        })?;

        let mut anomalies = Vec::new();

        for point in data {
            if point.value < baseline.lower_bound || point.value > baseline.upper_bound {
                let distance = if point.value < baseline.lower_bound {
                    (baseline.lower_bound - point.value).abs()
                } else {
                    (point.value - baseline.upper_bound).abs()
                };

                let score = (distance / baseline.iqr).min(1.0);
                let severity = if score > 0.75 {
                    AnomalySeverity::Critical
                } else if score > 0.5 {
                    AnomalySeverity::High
                } else if score > 0.25 {
                    AnomalySeverity::Medium
                } else {
                    AnomalySeverity::Low
                };

                let anomaly_type = if point.value > baseline.upper_bound {
                    AnomalyType::Spike
                } else {
                    AnomalyType::Drop
                };

                anomalies.push(Anomaly {
                    timestamp: point.timestamp,
                    metric_name: "unknown".to_string(),
                    observed_value: point.value,
                    expected_value: (baseline.q1 + baseline.q3) / 2.0,
                    score,
                    severity,
                    anomaly_type,
                    description: format!(
                        "Value outside IQR bounds: [{:.2}, {:.2}]",
                        baseline.lower_bound, baseline.upper_bound
                    ),
                });
            }
        }

        Ok(anomalies)
    }

    fn update_baseline(&mut self, data: &[DataPoint]) -> Result<()> {
        let new_baseline = Self::calculate_baseline(data, self.multiplier)?;
        *self.baseline.write() = Some(new_baseline);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_zscore_detector() {
        let mut detector = ZScoreDetector::new(3.0);

        let data = vec![
            DataPoint::new(Utc::now(), 10.0),
            DataPoint::new(Utc::now(), 12.0),
            DataPoint::new(Utc::now(), 11.0),
        ];

        detector
            .update_baseline(&data)
            .expect("Failed to update baseline");

        let test_data = vec![
            DataPoint::new(Utc::now(), 50.0), // Anomaly
        ];

        let anomalies = detector.detect(&test_data).expect("Failed to detect");
        assert_eq!(anomalies.len(), 1);
    }
}
