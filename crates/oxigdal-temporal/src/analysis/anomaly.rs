//! Anomaly Detection Module
//!
//! Implements anomaly detection algorithms for identifying unusual patterns
//! and outliers in temporal data.

use crate::error::{Result, TemporalError};
use crate::timeseries::TimeSeriesRaster;
use scirs2_core::ndarray::Array3;
use serde::{Deserialize, Serialize};
use tracing::info;

/// Anomaly detection method
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnomalyMethod {
    /// Z-score based detection
    ZScore,
    /// Interquartile range (IQR) method
    IQR,
    /// Modified Z-score using median
    ModifiedZScore,
    /// Isolation Forest (simplified)
    IsolationForest,
}

/// Anomaly detection result
#[derive(Debug, Clone)]
pub struct AnomalyResult {
    /// Anomaly scores (higher = more anomalous)
    pub scores: Array3<f64>,
    /// Binary anomaly mask (1 = anomaly, 0 = normal)
    pub mask: Array3<u8>,
    /// Anomaly count per pixel
    pub count: Array3<usize>,
    /// Threshold used for detection
    pub threshold: f64,
}

impl AnomalyResult {
    /// Create new anomaly result
    #[must_use]
    pub fn new(
        scores: Array3<f64>,
        mask: Array3<u8>,
        count: Array3<usize>,
        threshold: f64,
    ) -> Self {
        Self {
            scores,
            mask,
            count,
            threshold,
        }
    }
}

/// Anomaly detector
pub struct AnomalyDetector;

impl AnomalyDetector {
    /// Detect anomalies in time series
    ///
    /// # Errors
    /// Returns error if detection fails
    pub fn detect(
        ts: &TimeSeriesRaster,
        method: AnomalyMethod,
        threshold: f64,
    ) -> Result<AnomalyResult> {
        match method {
            AnomalyMethod::ZScore => Self::zscore_detection(ts, threshold),
            AnomalyMethod::ModifiedZScore => Self::modified_zscore_detection(ts, threshold),
            AnomalyMethod::IQR => Self::iqr_detection(ts, threshold),
            AnomalyMethod::IsolationForest => Self::isolation_forest_detection(ts, threshold),
        }
    }

    /// Z-score based anomaly detection
    fn zscore_detection(ts: &TimeSeriesRaster, threshold: f64) -> Result<AnomalyResult> {
        if ts.len() < 3 {
            return Err(TemporalError::insufficient_data(
                "Need at least 3 observations",
            ));
        }

        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        let mut scores = Array3::zeros((height, width, n_bands));
        let mut mask = Array3::zeros((height, width, n_bands));
        let mut count = Array3::zeros((height, width, n_bands));

        for i in 0..height {
            for j in 0..width {
                for k in 0..n_bands {
                    let values = ts.extract_pixel_timeseries(i, j, k)?;

                    // Calculate mean and std dev
                    let mean = values.iter().sum::<f64>() / values.len() as f64;
                    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>()
                        / values.len() as f64;
                    let std_dev = variance.sqrt();

                    // Calculate max absolute Z-score
                    let max_zscore = if std_dev > 0.0 {
                        values
                            .iter()
                            .map(|v| ((v - mean) / std_dev).abs())
                            .fold(0.0_f64, f64::max)
                    } else {
                        0.0
                    };

                    scores[[i, j, k]] = max_zscore;

                    // Count anomalies
                    if std_dev > 0.0 {
                        let anomaly_count = values
                            .iter()
                            .filter(|v| ((*v - mean) / std_dev).abs() > threshold)
                            .count();
                        count[[i, j, k]] = anomaly_count;
                        mask[[i, j, k]] = if anomaly_count > 0 { 1 } else { 0 };
                    }
                }
            }
        }

        info!("Completed Z-score anomaly detection");
        Ok(AnomalyResult::new(scores, mask, count, threshold))
    }

    /// Modified Z-score using median (robust to outliers)
    fn modified_zscore_detection(ts: &TimeSeriesRaster, threshold: f64) -> Result<AnomalyResult> {
        if ts.len() < 3 {
            return Err(TemporalError::insufficient_data(
                "Need at least 3 observations",
            ));
        }

        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        let mut scores = Array3::zeros((height, width, n_bands));
        let mut mask = Array3::zeros((height, width, n_bands));
        let mut count = Array3::zeros((height, width, n_bands));

        for i in 0..height {
            for j in 0..width {
                for k in 0..n_bands {
                    let mut values = ts.extract_pixel_timeseries(i, j, k)?;
                    values.sort_by(|a: &f64, b: &f64| {
                        a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
                    });

                    // Calculate median
                    let median = if values.len() % 2 == 0 {
                        (values[values.len() / 2 - 1] + values[values.len() / 2]) / 2.0
                    } else {
                        values[values.len() / 2]
                    };

                    // Calculate MAD (Median Absolute Deviation)
                    let mut deviations: Vec<f64> =
                        values.iter().map(|v: &f64| (v - median).abs()).collect();
                    deviations.sort_by(|a: &f64, b: &f64| {
                        a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
                    });

                    let mad = if deviations.len() % 2 == 0 {
                        (deviations[deviations.len() / 2 - 1] + deviations[deviations.len() / 2])
                            / 2.0
                    } else {
                        deviations[deviations.len() / 2]
                    };

                    // Modified Z-score: 0.6745 * (x - median) / MAD
                    let max_modified_zscore = if mad > 0.0 {
                        values
                            .iter()
                            .map(|v: &f64| (0.6745 * (v - median) / mad).abs())
                            .fold(0.0_f64, f64::max)
                    } else {
                        0.0
                    };

                    scores[[i, j, k]] = max_modified_zscore;

                    let anomaly_count = if mad > 0.0 {
                        values
                            .iter()
                            .filter(|v: &&f64| (0.6745 * (*v - median) / mad).abs() > threshold)
                            .count()
                    } else {
                        0
                    };

                    count[[i, j, k]] = anomaly_count;
                    mask[[i, j, k]] = if anomaly_count > 0 { 1 } else { 0 };
                }
            }
        }

        info!("Completed modified Z-score anomaly detection");
        Ok(AnomalyResult::new(scores, mask, count, threshold))
    }

    /// IQR-based anomaly detection
    fn iqr_detection(ts: &TimeSeriesRaster, threshold: f64) -> Result<AnomalyResult> {
        if ts.len() < 4 {
            return Err(TemporalError::insufficient_data(
                "Need at least 4 observations for IQR",
            ));
        }

        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        let mut scores = Array3::zeros((height, width, n_bands));
        let mut mask = Array3::zeros((height, width, n_bands));
        let mut count = Array3::zeros((height, width, n_bands));

        for i in 0..height {
            for j in 0..width {
                for k in 0..n_bands {
                    let mut values = ts.extract_pixel_timeseries(i, j, k)?;
                    values.sort_by(|a: &f64, b: &f64| {
                        a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
                    });

                    // Calculate Q1, Q3, and IQR
                    let q1_idx = values.len() / 4;
                    let q3_idx = 3 * values.len() / 4;

                    let q1 = values[q1_idx];
                    let q3 = values[q3_idx];
                    let iqr = q3 - q1;

                    // Outlier bounds
                    let lower_bound = q1 - threshold * iqr;
                    let upper_bound = q3 + threshold * iqr;

                    // Count outliers
                    let anomaly_count = values
                        .iter()
                        .filter(|&&v| v < lower_bound || v > upper_bound)
                        .count();

                    // Score as distance from bounds
                    let max_score = values
                        .iter()
                        .map(|&v| {
                            if v < lower_bound {
                                (lower_bound - v) / iqr
                            } else if v > upper_bound {
                                (v - upper_bound) / iqr
                            } else {
                                0.0
                            }
                        })
                        .fold(0.0_f64, f64::max);

                    scores[[i, j, k]] = max_score;
                    count[[i, j, k]] = anomaly_count;
                    mask[[i, j, k]] = if anomaly_count > 0 { 1 } else { 0 };
                }
            }
        }

        info!("Completed IQR anomaly detection");
        Ok(AnomalyResult::new(scores, mask, count, threshold))
    }

    /// Simplified isolation forest detection
    fn isolation_forest_detection(ts: &TimeSeriesRaster, threshold: f64) -> Result<AnomalyResult> {
        // For now, use modified Z-score as approximation
        // Full isolation forest would require tree-based implementation
        info!("Using modified Z-score approximation for isolation forest");
        Self::modified_zscore_detection(ts, threshold)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeseries::{TemporalMetadata, TimeSeriesRaster};
    use chrono::{DateTime, NaiveDate};

    #[test]
    fn test_zscore_detection() {
        let mut ts = TimeSeriesRaster::new();

        // Create data with an outlier
        for i in 0..10 {
            let dt = DateTime::from_timestamp(1640995200 + i * 86400, 0).expect("valid");
            let date = NaiveDate::from_ymd_opt(2022, 1, 1 + i as u32).expect("valid");
            let metadata = TemporalMetadata::new(dt, date);

            let value = if i == 5 { 100.0 } else { 10.0 }; // Outlier at i=5
            let data = Array3::from_elem((3, 3, 1), value);
            ts.add_raster(metadata, data).expect("should add");
        }

        let result =
            AnomalyDetector::detect(&ts, AnomalyMethod::ZScore, 2.0).expect("should detect");

        assert!(result.mask[[0, 0, 0]] == 1); // Should detect anomaly
        assert!(result.count[[0, 0, 0]] > 0);
    }

    #[test]
    fn test_iqr_detection() {
        let mut ts = TimeSeriesRaster::new();

        for i in 0..20 {
            let dt = DateTime::from_timestamp(1640995200 + i * 86400, 0).expect("valid");
            let date = NaiveDate::from_ymd_opt(2022, 1, 1 + i as u32).expect("valid");
            let metadata = TemporalMetadata::new(dt, date);

            let value = if i == 10 { 200.0 } else { 50.0 + (i as f64) };
            let data = Array3::from_elem((3, 3, 1), value);
            ts.add_raster(metadata, data).expect("should add");
        }

        let result = AnomalyDetector::detect(&ts, AnomalyMethod::IQR, 1.5).expect("should detect");

        assert!(result.mask[[0, 0, 0]] == 1);
    }
}
