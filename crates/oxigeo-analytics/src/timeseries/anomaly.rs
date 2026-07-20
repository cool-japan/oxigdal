//! Anomaly Detection for Time Series
//!
//! This module provides various anomaly detection methods including:
//! - Z-score based detection
//! - IQR (Interquartile Range) based detection
//! - Modified Z-score (using median absolute deviation)

use crate::error::{AnalyticsError, Result};
use scirs2_core::ndarray::{Array1, ArrayView1};

/// Anomaly detection methods
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnomalyMethod {
    /// Z-score based detection (assumes normal distribution)
    ZScore,
    /// IQR (Interquartile Range) based detection (robust to outliers)
    IQR,
    /// Modified Z-score using Median Absolute Deviation
    ModifiedZScore,
}

/// Result of anomaly detection
#[derive(Debug, Clone)]
pub struct AnomalyResult {
    /// Indices of detected anomalies
    pub anomaly_indices: Vec<usize>,
    /// Anomaly scores for each data point
    pub scores: Array1<f64>,
    /// Threshold used for detection
    pub threshold: f64,
    /// Detection method used
    pub method: AnomalyMethod,
}

/// Anomaly detector for time series
pub struct AnomalyDetector {
    method: AnomalyMethod,
    threshold: f64,
}

impl AnomalyDetector {
    /// Create a new anomaly detector
    ///
    /// # Arguments
    /// * `method` - Anomaly detection method
    /// * `threshold` - Detection threshold (e.g., 3.0 for Z-score, 1.5 for IQR)
    pub fn new(method: AnomalyMethod, threshold: f64) -> Self {
        Self { method, threshold }
    }

    /// Detect anomalies in time series
    ///
    /// # Arguments
    /// * `values` - Time series values
    ///
    /// # Errors
    /// Returns error if computation fails or insufficient data
    pub fn detect(&self, values: &ArrayView1<f64>) -> Result<AnomalyResult> {
        let scores = match self.method {
            AnomalyMethod::ZScore => self.z_score(values)?,
            AnomalyMethod::IQR => self.iqr_score(values)?,
            AnomalyMethod::ModifiedZScore => self.modified_z_score(values)?,
        };

        // Find anomalies based on threshold
        let anomaly_indices: Vec<usize> = scores
            .iter()
            .enumerate()
            .filter_map(|(i, &score)| {
                // Use >= to include boundary cases where score equals threshold
                if score.abs() >= self.threshold {
                    Some(i)
                } else {
                    None
                }
            })
            .collect();

        Ok(AnomalyResult {
            anomaly_indices,
            scores,
            threshold: self.threshold,
            method: self.method,
        })
    }

    /// Z-score based anomaly detection
    ///
    /// Calculates standardized scores: z = (x - mean) / std
    fn z_score(&self, values: &ArrayView1<f64>) -> Result<Array1<f64>> {
        if values.len() < 2 {
            return Err(AnalyticsError::insufficient_data(
                "Z-score requires at least 2 data points",
            ));
        }

        let n = values.len() as f64;
        let mean = values.sum() / n;
        let variance = values.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n;
        let std = variance.sqrt();

        if std < f64::EPSILON {
            return Err(AnalyticsError::numerical_instability(
                "Standard deviation is too small",
            ));
        }

        let mut scores = Array1::zeros(values.len());
        for (i, &value) in values.iter().enumerate() {
            scores[i] = (value - mean) / std;
        }

        Ok(scores)
    }

    /// IQR-based anomaly detection
    ///
    /// Uses interquartile range to detect outliers
    fn iqr_score(&self, values: &ArrayView1<f64>) -> Result<Array1<f64>> {
        if values.len() < 4 {
            return Err(AnalyticsError::insufficient_data(
                "IQR requires at least 4 data points",
            ));
        }

        // Sort values for percentile calculation
        let mut sorted = values.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let q1 = percentile(&sorted, 25.0)?;
        let q3 = percentile(&sorted, 75.0)?;
        let iqr = q3 - q1;

        if iqr < f64::EPSILON {
            return Err(AnalyticsError::numerical_instability("IQR is too small"));
        }

        let median = percentile(&sorted, 50.0)?;

        // Calculate scores based on distance from median in units of IQR
        let mut scores = Array1::zeros(values.len());
        for (i, &value) in values.iter().enumerate() {
            scores[i] = (value - median).abs() / iqr;
        }

        Ok(scores)
    }

    /// Modified Z-score using Median Absolute Deviation (MAD)
    ///
    /// More robust to outliers than standard Z-score
    fn modified_z_score(&self, values: &ArrayView1<f64>) -> Result<Array1<f64>> {
        if values.len() < 2 {
            return Err(AnalyticsError::insufficient_data(
                "Modified Z-score requires at least 2 data points",
            ));
        }

        // Sort values for median calculation
        let mut sorted = values.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let median = percentile(&sorted, 50.0)?;

        // Calculate MAD
        let mut abs_deviations: Vec<f64> = values.iter().map(|x| (x - median).abs()).collect();
        abs_deviations.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mad = percentile(&abs_deviations, 50.0)?;

        // Consistency constant for normal distribution
        let consistency_constant = 1.4826;
        let normalized_mad = consistency_constant * mad;

        if normalized_mad < f64::EPSILON {
            return Err(AnalyticsError::numerical_instability("MAD is too small"));
        }

        // Calculate modified Z-scores
        let mut scores = Array1::zeros(values.len());
        for (i, &value) in values.iter().enumerate() {
            scores[i] = 0.6745 * (value - median) / normalized_mad;
        }

        Ok(scores)
    }
}

/// Calculate percentile of sorted data
///
/// # Arguments
/// * `sorted_data` - Sorted array of values
/// * `percentile` - Percentile to calculate (0-100)
///
/// # Errors
/// Returns error if data is empty or percentile is out of range
fn percentile(sorted_data: &[f64], percentile: f64) -> Result<f64> {
    if sorted_data.is_empty() {
        return Err(AnalyticsError::insufficient_data("Data is empty"));
    }

    if !(0.0..=100.0).contains(&percentile) {
        return Err(AnalyticsError::invalid_parameter(
            "percentile",
            "must be between 0 and 100",
        ));
    }

    let n = sorted_data.len();
    if n == 1 {
        return Ok(sorted_data[0]);
    }

    // Linear interpolation between closest ranks
    let rank = (percentile / 100.0) * ((n - 1) as f64);
    let lower_idx = rank.floor() as usize;
    let upper_idx = rank.ceil() as usize;
    let fraction = rank - (lower_idx as f64);

    Ok(sorted_data[lower_idx] + fraction * (sorted_data[upper_idx] - sorted_data[lower_idx]))
}

/// Change point detection using cumulative sum (CUSUM)
///
/// # Arguments
/// * `values` - Time series values
/// * `threshold` - Detection threshold
///
/// # Errors
/// Returns error if computation fails
pub fn detect_change_points(values: &ArrayView1<f64>, threshold: f64) -> Result<Vec<usize>> {
    if values.len() < 3 {
        return Err(AnalyticsError::insufficient_data(
            "Change point detection requires at least 3 data points",
        ));
    }

    let n = values.len() as f64;
    let mean = values.sum() / n;

    // Calculate cumulative sum of deviations
    let mut cusum_pos = 0.0;
    let mut cusum_neg = 0.0;
    let mut change_points = Vec::new();

    for (i, &value) in values.iter().enumerate() {
        let deviation = value - mean;
        cusum_pos = (cusum_pos + deviation).max(0.0);
        cusum_neg = (cusum_neg - deviation).max(0.0);

        if cusum_pos > threshold || cusum_neg > threshold {
            change_points.push(i);
            cusum_pos = 0.0;
            cusum_neg = 0.0;
        }
    }

    Ok(change_points)
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;
    use scirs2_core::ndarray::array;

    #[test]
    fn test_z_score_detection() {
        let values = array![1.0, 2.0, 3.0, 4.0, 100.0]; // 100.0 is an outlier
        // Use threshold of 1.9 instead of 2.0 to account for the actual z-score being ~1.999
        // (just under 2.0 due to how the outlier affects both mean and std dev)
        let detector = AnomalyDetector::new(AnomalyMethod::ZScore, 1.9);
        let result = detector
            .detect(&values.view())
            .expect("Z-score detection should succeed with valid data");

        assert!(!result.anomaly_indices.is_empty());
        assert!(result.anomaly_indices.contains(&4));
    }

    #[test]
    fn test_iqr_detection() {
        let values = array![1.0, 2.0, 3.0, 4.0, 5.0, 100.0]; // 100.0 is an outlier
        let detector = AnomalyDetector::new(AnomalyMethod::IQR, 1.5);
        let result = detector
            .detect(&values.view())
            .expect("IQR detection should succeed with valid data");

        assert!(!result.anomaly_indices.is_empty());
        assert!(result.anomaly_indices.contains(&5));
    }

    #[test]
    fn test_modified_z_score() {
        let values = array![1.0, 2.0, 3.0, 4.0, 5.0, 100.0]; // 100.0 is an outlier
        let detector = AnomalyDetector::new(AnomalyMethod::ModifiedZScore, 3.5);
        let result = detector
            .detect(&values.view())
            .expect("Modified Z-score detection should succeed with valid data");

        assert!(!result.anomaly_indices.is_empty());
    }

    #[test]
    fn test_percentile() {
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let p50 = percentile(&data, 50.0).expect("50th percentile calculation should succeed");
        assert_abs_diff_eq!(p50, 3.0, epsilon = 1e-10);

        let p25 = percentile(&data, 25.0).expect("25th percentile calculation should succeed");
        assert_abs_diff_eq!(p25, 2.0, epsilon = 1e-10);

        let p75 = percentile(&data, 75.0).expect("75th percentile calculation should succeed");
        assert_abs_diff_eq!(p75, 4.0, epsilon = 1e-10);
    }

    #[test]
    fn test_change_point_detection() {
        let values = array![1.0, 1.0, 1.0, 5.0, 5.0, 5.0]; // Change at index 3
        let change_points = detect_change_points(&values.view(), 1.0)
            .expect("Change point detection should succeed with valid data");

        assert!(!change_points.is_empty());
    }

    #[test]
    fn test_no_anomalies() {
        let values = array![1.0, 2.0, 3.0, 4.0, 5.0];
        let detector = AnomalyDetector::new(AnomalyMethod::ZScore, 3.0);
        let result = detector
            .detect(&values.view())
            .expect("Anomaly detection should succeed with valid data");

        assert!(result.anomaly_indices.is_empty());
    }
}
