//! Change Detection Algorithms
//!
//! Implements various change detection algorithms including simple differencing,
//! statistical tests, and advanced methods like BFAST and LandTrendr.

use crate::error::{Result, TemporalError};
use crate::stack::RasterStack;
use crate::timeseries::TimeSeriesRaster;
use scirs2_core::ndarray::Array3;
use serde::{Deserialize, Serialize};
use tracing::info;

/// Change detection method
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChangeDetectionMethod {
    /// Simple differencing (end - start)
    SimpleDifference,
    /// Absolute change
    AbsoluteChange,
    /// Relative change (percentage)
    RelativeChange,
    /// Z-score based change
    ZScore,
    /// Threshold-based change detection
    Threshold,
    /// Cumulative sum (CUSUM)
    CUSUM,
    /// BFAST (Breaks For Additive Season and Trend)
    BFAST,
    /// LandTrendr approximation
    LandTrendr,
}

/// Change detection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeDetectionConfig {
    /// Detection method
    pub method: ChangeDetectionMethod,
    /// Threshold for threshold-based detection
    pub threshold: Option<f64>,
    /// Minimum change magnitude to report
    pub min_magnitude: Option<f64>,
    /// NoData value
    pub nodata: Option<f64>,
    /// Confidence level for statistical tests
    pub confidence_level: Option<f64>,
}

impl Default for ChangeDetectionConfig {
    fn default() -> Self {
        Self {
            method: ChangeDetectionMethod::SimpleDifference,
            threshold: Some(0.1),
            min_magnitude: None,
            nodata: Some(f64::NAN),
            confidence_level: Some(0.95),
        }
    }
}

/// Change detection result
#[derive(Debug, Clone)]
pub struct ChangeDetectionResult {
    /// Change magnitude
    pub magnitude: Array3<f64>,
    /// Change direction (-1, 0, 1)
    pub direction: Array3<i8>,
    /// Timestamp of change (if applicable)
    pub change_time: Option<Array3<i64>>,
    /// Confidence/significance
    pub confidence: Option<Array3<f64>>,
}

impl ChangeDetectionResult {
    /// Create new change detection result
    #[must_use]
    pub fn new(magnitude: Array3<f64>, direction: Array3<i8>) -> Self {
        Self {
            magnitude,
            direction,
            change_time: None,
            confidence: None,
        }
    }

    /// Add change timestamps
    #[must_use]
    pub fn with_change_time(mut self, change_time: Array3<i64>) -> Self {
        self.change_time = Some(change_time);
        self
    }

    /// Add confidence scores
    #[must_use]
    pub fn with_confidence(mut self, confidence: Array3<f64>) -> Self {
        self.confidence = Some(confidence);
        self
    }
}

/// Change detector
pub struct ChangeDetector;

impl ChangeDetector {
    /// Detect changes in time series
    ///
    /// # Errors
    /// Returns error if detection fails
    pub fn detect(
        ts: &TimeSeriesRaster,
        config: &ChangeDetectionConfig,
    ) -> Result<ChangeDetectionResult> {
        match config.method {
            ChangeDetectionMethod::SimpleDifference => Self::simple_difference(ts, config),
            ChangeDetectionMethod::AbsoluteChange => Self::absolute_change(ts, config),
            ChangeDetectionMethod::RelativeChange => Self::relative_change(ts, config),
            ChangeDetectionMethod::ZScore => Self::zscore_change(ts, config),
            ChangeDetectionMethod::Threshold => Self::threshold_change(ts, config),
            ChangeDetectionMethod::CUSUM => Self::cusum_change(ts, config),
            ChangeDetectionMethod::BFAST => Self::bfast_change(ts, config),
            ChangeDetectionMethod::LandTrendr => Self::landtrendr_change(ts, config),
        }
    }

    /// Detect changes in raster stack
    ///
    /// # Errors
    /// Returns error if detection fails
    pub fn detect_stack(
        stack: &RasterStack,
        config: &ChangeDetectionConfig,
    ) -> Result<ChangeDetectionResult> {
        match config.method {
            ChangeDetectionMethod::SimpleDifference => Self::simple_difference_stack(stack, config),
            ChangeDetectionMethod::AbsoluteChange => Self::absolute_change_stack(stack, config),
            ChangeDetectionMethod::RelativeChange => Self::relative_change_stack(stack, config),
            _ => Err(TemporalError::change_detection_error(format!(
                "Method {:?} not supported for stack",
                config.method
            ))),
        }
    }

    /// Simple difference between first and last observation
    fn simple_difference(
        ts: &TimeSeriesRaster,
        _config: &ChangeDetectionConfig,
    ) -> Result<ChangeDetectionResult> {
        if ts.len() < 2 {
            return Err(TemporalError::insufficient_data(
                "Need at least 2 observations",
            ));
        }

        let first = ts.get_by_index(0)?;
        let last = ts.get_by_index(ts.len() - 1)?;

        let first_data = first
            .data
            .as_ref()
            .ok_or_else(|| TemporalError::invalid_input("Data not loaded"))?;
        let last_data = last
            .data
            .as_ref()
            .ok_or_else(|| TemporalError::invalid_input("Data not loaded"))?;

        let magnitude = last_data - first_data;
        let direction = Self::compute_direction(&magnitude);

        info!("Completed simple difference change detection");
        Ok(ChangeDetectionResult::new(magnitude, direction))
    }

    /// Absolute change
    fn absolute_change(
        ts: &TimeSeriesRaster,
        _config: &ChangeDetectionConfig,
    ) -> Result<ChangeDetectionResult> {
        if ts.len() < 2 {
            return Err(TemporalError::insufficient_data(
                "Need at least 2 observations",
            ));
        }

        let first = ts.get_by_index(0)?;
        let last = ts.get_by_index(ts.len() - 1)?;

        let first_data = first
            .data
            .as_ref()
            .ok_or_else(|| TemporalError::invalid_input("Data not loaded"))?;
        let last_data = last
            .data
            .as_ref()
            .ok_or_else(|| TemporalError::invalid_input("Data not loaded"))?;

        let magnitude = (last_data - first_data).mapv(f64::abs);
        let direction = Self::compute_direction(&(last_data - first_data));

        info!("Completed absolute change detection");
        Ok(ChangeDetectionResult::new(magnitude, direction))
    }

    /// Relative change (percentage)
    fn relative_change(
        ts: &TimeSeriesRaster,
        _config: &ChangeDetectionConfig,
    ) -> Result<ChangeDetectionResult> {
        if ts.len() < 2 {
            return Err(TemporalError::insufficient_data(
                "Need at least 2 observations",
            ));
        }

        let first = ts.get_by_index(0)?;
        let last = ts.get_by_index(ts.len() - 1)?;

        let first_data = first
            .data
            .as_ref()
            .ok_or_else(|| TemporalError::invalid_input("Data not loaded"))?;
        let last_data = last
            .data
            .as_ref()
            .ok_or_else(|| TemporalError::invalid_input("Data not loaded"))?;

        let magnitude =
            (last_data - first_data) / first_data.mapv(|v| if v != 0.0 { v } else { 1.0 });
        let direction = Self::compute_direction(&(last_data - first_data));

        info!("Completed relative change detection");
        Ok(ChangeDetectionResult::new(magnitude, direction))
    }

    /// Z-score based change detection
    fn zscore_change(
        ts: &TimeSeriesRaster,
        config: &ChangeDetectionConfig,
    ) -> Result<ChangeDetectionResult> {
        if ts.len() < 3 {
            return Err(TemporalError::insufficient_data(
                "Need at least 3 observations",
            ));
        }

        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        let mut magnitude = Array3::zeros((height, width, n_bands));
        let mut direction = Array3::zeros((height, width, n_bands));

        let threshold = config.threshold.unwrap_or(2.0);

        for i in 0..height {
            for j in 0..width {
                for k in 0..n_bands {
                    let values = ts.extract_pixel_timeseries(i, j, k)?;

                    let mean = values.iter().sum::<f64>() / values.len() as f64;
                    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>()
                        / values.len() as f64;
                    let std_dev = variance.sqrt();

                    if std_dev > 0.0 {
                        let last_value = values[values.len() - 1];
                        let z_score = ((last_value - mean) / std_dev).abs();

                        magnitude[[i, j, k]] = z_score;
                        direction[[i, j, k]] = if z_score > threshold {
                            if last_value > mean { 1 } else { -1 }
                        } else {
                            0
                        };
                    }
                }
            }
        }

        info!("Completed Z-score change detection");
        Ok(ChangeDetectionResult::new(magnitude, direction))
    }

    /// Threshold-based change detection
    fn threshold_change(
        ts: &TimeSeriesRaster,
        config: &ChangeDetectionConfig,
    ) -> Result<ChangeDetectionResult> {
        if ts.len() < 2 {
            return Err(TemporalError::insufficient_data(
                "Need at least 2 observations",
            ));
        }

        let threshold = config.threshold.ok_or_else(|| {
            TemporalError::invalid_parameter("threshold", "threshold required for this method")
        })?;

        let first = ts.get_by_index(0)?;
        let last = ts.get_by_index(ts.len() - 1)?;

        let first_data = first
            .data
            .as_ref()
            .ok_or_else(|| TemporalError::invalid_input("Data not loaded"))?;
        let last_data = last
            .data
            .as_ref()
            .ok_or_else(|| TemporalError::invalid_input("Data not loaded"))?;

        let diff = last_data - first_data;
        let magnitude = diff.mapv(|v| if v.abs() > threshold { v.abs() } else { 0.0 });
        let direction = Self::compute_direction(&diff);

        info!("Completed threshold-based change detection");
        Ok(ChangeDetectionResult::new(magnitude, direction))
    }

    /// CUSUM change detection
    fn cusum_change(
        ts: &TimeSeriesRaster,
        config: &ChangeDetectionConfig,
    ) -> Result<ChangeDetectionResult> {
        if ts.len() < 3 {
            return Err(TemporalError::insufficient_data(
                "Need at least 3 observations",
            ));
        }

        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        let mut magnitude = Array3::zeros((height, width, n_bands));
        let mut direction = Array3::zeros((height, width, n_bands));

        let threshold = config.threshold.unwrap_or(1.0);

        for i in 0..height {
            for j in 0..width {
                for k in 0..n_bands {
                    let values = ts.extract_pixel_timeseries(i, j, k)?;

                    let mean = values.iter().sum::<f64>() / values.len() as f64;

                    // Calculate CUSUM
                    let mut cusum_pos: f64 = 0.0;
                    let mut cusum_neg: f64 = 0.0;
                    let mut max_cusum: f64 = 0.0;

                    for &value in &values {
                        cusum_pos = (cusum_pos + (value - mean)).max(0.0);
                        cusum_neg = (cusum_neg - (value - mean)).max(0.0);

                        max_cusum = max_cusum.max(cusum_pos).max(cusum_neg);
                    }

                    magnitude[[i, j, k]] = max_cusum;
                    direction[[i, j, k]] = if max_cusum > threshold {
                        if cusum_pos > cusum_neg { 1 } else { -1 }
                    } else {
                        0
                    };
                }
            }
        }

        info!("Completed CUSUM change detection");
        Ok(ChangeDetectionResult::new(magnitude, direction))
    }

    /// BFAST change detection (simplified approximation)
    fn bfast_change(
        ts: &TimeSeriesRaster,
        config: &ChangeDetectionConfig,
    ) -> Result<ChangeDetectionResult> {
        // BFAST is complex - use CUSUM as approximation for now
        info!("Using CUSUM approximation for BFAST");
        Self::cusum_change(ts, config)
    }

    /// LandTrendr change detection (simplified approximation)
    fn landtrendr_change(
        ts: &TimeSeriesRaster,
        _config: &ChangeDetectionConfig,
    ) -> Result<ChangeDetectionResult> {
        // LandTrendr is complex - use trend-based approach as approximation
        info!("Using trend-based approximation for LandTrendr");

        if ts.len() < 3 {
            return Err(TemporalError::insufficient_data(
                "Need at least 3 observations",
            ));
        }

        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        let mut magnitude = Array3::zeros((height, width, n_bands));
        let mut direction = Array3::zeros((height, width, n_bands));

        for i in 0..height {
            for j in 0..width {
                for k in 0..n_bands {
                    let values = ts.extract_pixel_timeseries(i, j, k)?;

                    // Calculate slope
                    let n = values.len() as f64;
                    let times: Vec<f64> = (0..values.len()).map(|t| t as f64).collect();
                    let sum_t: f64 = times.iter().sum();
                    let sum_y: f64 = values.iter().sum();
                    let sum_t2: f64 = times.iter().map(|t| t * t).sum();
                    let sum_ty: f64 = times.iter().zip(values.iter()).map(|(t, y)| t * y).sum();

                    let slope = (n * sum_ty - sum_t * sum_y) / (n * sum_t2 - sum_t * sum_t);

                    magnitude[[i, j, k]] = slope.abs();
                    direction[[i, j, k]] = if slope > 0.0 {
                        1
                    } else if slope < 0.0 {
                        -1
                    } else {
                        0
                    };
                }
            }
        }

        info!("Completed LandTrendr approximation change detection");
        Ok(ChangeDetectionResult::new(magnitude, direction))
    }

    /// Simple difference for raster stack
    fn simple_difference_stack(
        stack: &RasterStack,
        _config: &ChangeDetectionConfig,
    ) -> Result<ChangeDetectionResult> {
        let (n_time, _height, _width, _n_bands) = stack.shape();

        if n_time < 2 {
            return Err(TemporalError::insufficient_data(
                "Need at least 2 time steps",
            ));
        }

        let data = stack.data();
        let first = data.slice(s![0, .., .., ..]).to_owned();
        let last = data.slice(s![n_time - 1, .., .., ..]).to_owned();

        let magnitude = &last - &first;
        let direction = Self::compute_direction(&magnitude);

        Ok(ChangeDetectionResult::new(magnitude, direction))
    }

    /// Absolute change for raster stack
    fn absolute_change_stack(
        stack: &RasterStack,
        _config: &ChangeDetectionConfig,
    ) -> Result<ChangeDetectionResult> {
        let (n_time, _height, _width, _n_bands) = stack.shape();

        if n_time < 2 {
            return Err(TemporalError::insufficient_data(
                "Need at least 2 time steps",
            ));
        }

        let data = stack.data();
        let first = data.slice(s![0, .., .., ..]).to_owned();
        let last = data.slice(s![n_time - 1, .., .., ..]).to_owned();

        let diff = &last - &first;
        let magnitude = diff.mapv(f64::abs);
        let direction = Self::compute_direction(&diff);

        Ok(ChangeDetectionResult::new(magnitude, direction))
    }

    /// Relative change for raster stack
    fn relative_change_stack(
        stack: &RasterStack,
        _config: &ChangeDetectionConfig,
    ) -> Result<ChangeDetectionResult> {
        let (n_time, _height, _width, _n_bands) = stack.shape();

        if n_time < 2 {
            return Err(TemporalError::insufficient_data(
                "Need at least 2 time steps",
            ));
        }

        let data = stack.data();
        let first = data.slice(s![0, .., .., ..]).to_owned();
        let last = data.slice(s![n_time - 1, .., .., ..]).to_owned();

        let diff = &last - &first;
        let magnitude = &diff / &first.mapv(|v| if v != 0.0 { v } else { 1.0 });
        let direction = Self::compute_direction(&diff);

        Ok(ChangeDetectionResult::new(magnitude, direction))
    }

    /// Compute change direction
    fn compute_direction(change: &Array3<f64>) -> Array3<i8> {
        let shape = change.shape();
        let mut direction = Array3::zeros((shape[0], shape[1], shape[2]));

        for i in 0..shape[0] {
            for j in 0..shape[1] {
                for k in 0..shape[2] {
                    let c = change[[i, j, k]];
                    direction[[i, j, k]] = if c > 0.0 {
                        1
                    } else if c < 0.0 {
                        -1
                    } else {
                        0
                    };
                }
            }
        }

        direction
    }
}

// Import ndarray slice macro
use scirs2_core::ndarray::s;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeseries::{TemporalMetadata, TimeSeriesRaster};
    use chrono::{DateTime, NaiveDate};

    #[test]
    fn test_simple_difference() {
        let mut ts = TimeSeriesRaster::new();

        let dt1 = DateTime::from_timestamp(1640995200, 0).expect("valid");
        let date1 = NaiveDate::from_ymd_opt(2022, 1, 1).expect("valid");
        let metadata1 = TemporalMetadata::new(dt1, date1);
        ts.add_raster(metadata1, Array3::from_elem((5, 5, 1), 10.0))
            .expect("should add");

        let dt2 = DateTime::from_timestamp(1641081600, 0).expect("valid");
        let date2 = NaiveDate::from_ymd_opt(2022, 1, 2).expect("valid");
        let metadata2 = TemporalMetadata::new(dt2, date2);
        ts.add_raster(metadata2, Array3::from_elem((5, 5, 1), 20.0))
            .expect("should add");

        let config = ChangeDetectionConfig::default();
        let result = ChangeDetector::detect(&ts, &config).expect("should detect");

        assert_eq!(result.magnitude[[0, 0, 0]], 10.0);
        assert_eq!(result.direction[[0, 0, 0]], 1);
    }
}
