//! Time Series Analysis Module
//!
//! This module provides comprehensive time series analysis capabilities for geospatial data,
//! including trend detection, anomaly detection, seasonal decomposition, and gap filling.

pub mod anomaly;
pub mod trend;

pub use anomaly::{AnomalyDetector, AnomalyMethod, AnomalyResult};
pub use trend::{TrendDetector, TrendMethod, TrendResult};

use crate::error::{AnalyticsError, Result};
use scirs2_core::ndarray::Array1;

/// Time series data point
#[derive(Debug, Clone, Copy)]
pub struct TimePoint {
    /// Time index or timestamp
    pub time: f64,
    /// Value at this time point
    pub value: f64,
}

/// Time series with temporal metadata
#[derive(Debug, Clone)]
pub struct TimeSeries {
    /// Time points
    pub times: Array1<f64>,
    /// Values
    pub values: Array1<f64>,
}

impl TimeSeries {
    /// Create a new time series
    ///
    /// # Arguments
    /// * `times` - Time indices or timestamps
    /// * `values` - Corresponding values
    ///
    /// # Errors
    /// Returns error if dimensions don't match or data is empty
    pub fn new(times: Array1<f64>, values: Array1<f64>) -> Result<Self> {
        if times.len() != values.len() {
            return Err(AnalyticsError::dimension_mismatch(
                format!("{}", times.len()),
                format!("{}", values.len()),
            ));
        }

        if times.is_empty() {
            return Err(AnalyticsError::insufficient_data(
                "Time series must have at least one data point",
            ));
        }

        Ok(Self { times, values })
    }

    /// Get the length of the time series
    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Check if time series is empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Get the time range
    #[must_use]
    pub fn time_range(&self) -> Option<(f64, f64)> {
        if self.is_empty() {
            return None;
        }

        let min = self
            .times
            .iter()
            .copied()
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))?;
        let max = self
            .times
            .iter()
            .copied()
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))?;

        Some((min, max))
    }

    /// Calculate simple moving average
    ///
    /// # Arguments
    /// * `window_size` - Size of the moving window
    ///
    /// # Errors
    /// Returns error if window size is invalid
    pub fn moving_average(&self, window_size: usize) -> Result<Array1<f64>> {
        if window_size == 0 {
            return Err(AnalyticsError::invalid_parameter(
                "window_size",
                "must be greater than 0",
            ));
        }

        if window_size > self.len() {
            return Err(AnalyticsError::invalid_parameter(
                "window_size",
                "must not exceed series length",
            ));
        }

        let mut result = Array1::zeros(self.len() - window_size + 1);

        for i in 0..result.len() {
            let window = self.values.slice(s![i..i + window_size]);
            result[i] = window.sum() / (window_size as f64);
        }

        Ok(result)
    }

    /// Calculate exponential moving average
    ///
    /// # Arguments
    /// * `alpha` - Smoothing factor (0 < alpha <= 1)
    ///
    /// # Errors
    /// Returns error if alpha is out of range
    pub fn exponential_moving_average(&self, alpha: f64) -> Result<Array1<f64>> {
        if !(0.0 < alpha && alpha <= 1.0) {
            return Err(AnalyticsError::invalid_parameter(
                "alpha",
                "must be in range (0, 1]",
            ));
        }

        let mut result = Array1::zeros(self.len());
        result[0] = self.values[0];

        for i in 1..self.len() {
            result[i] = alpha * self.values[i] + (1.0 - alpha) * result[i - 1];
        }

        Ok(result)
    }

    /// Linear interpolation for gap filling
    ///
    /// # Arguments
    /// * `mask` - Boolean mask where true indicates missing values
    ///
    /// # Errors
    /// Returns error if all values are missing or interpolation fails
    pub fn linear_interpolate(&self, mask: &Array1<bool>) -> Result<Array1<f64>> {
        if mask.len() != self.len() {
            return Err(AnalyticsError::dimension_mismatch(
                format!("{}", self.len()),
                format!("{}", mask.len()),
            ));
        }

        let mut result = self.values.clone();

        // Find first and last valid indices
        let first_valid = mask
            .iter()
            .position(|&x| !x)
            .ok_or_else(|| AnalyticsError::insufficient_data("All values are missing"))?;
        let last_valid = mask
            .iter()
            .rposition(|&x| !x)
            .ok_or_else(|| AnalyticsError::insufficient_data("All values are missing"))?;

        // Interpolate gaps
        let mut last_valid_idx = first_valid;
        for i in (first_valid + 1)..=last_valid {
            if mask[i] {
                // Find next valid index
                let next_valid_idx =
                    ((i + 1)..=last_valid).find(|&j| !mask[j]).ok_or_else(|| {
                        AnalyticsError::insufficient_data("Cannot interpolate at end")
                    })?;

                // Linear interpolation
                let x0 = self.times[last_valid_idx];
                let x1 = self.times[next_valid_idx];
                let y0 = self.values[last_valid_idx];
                let y1 = self.values[next_valid_idx];
                let x = self.times[i];

                result[i] = y0 + (y1 - y0) * (x - x0) / (x1 - x0);
            } else {
                last_valid_idx = i;
            }
        }

        Ok(result)
    }
}

// Import ndarray slice macro
use scirs2_core::ndarray::s;

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;
    use scirs2_core::ndarray::array;

    #[test]
    fn test_time_series_creation() {
        let times = array![1.0, 2.0, 3.0, 4.0, 5.0];
        let values = array![10.0, 20.0, 15.0, 25.0, 30.0];
        let ts = TimeSeries::new(times, values)
            .expect("TimeSeries creation should succeed with matching dimensions");

        assert_eq!(ts.len(), 5);
        assert!(!ts.is_empty());
    }

    #[test]
    fn test_time_series_dimension_mismatch() {
        let times = array![1.0, 2.0, 3.0];
        let values = array![10.0, 20.0];
        let result = TimeSeries::new(times, values);

        assert!(result.is_err());
    }

    #[test]
    fn test_moving_average() {
        let times = array![1.0, 2.0, 3.0, 4.0, 5.0];
        let values = array![10.0, 20.0, 30.0, 40.0, 50.0];
        let ts = TimeSeries::new(times, values)
            .expect("TimeSeries creation should succeed with matching dimensions");

        let ma = ts
            .moving_average(3)
            .expect("Moving average calculation should succeed with valid window size");
        assert_eq!(ma.len(), 3);
        assert_abs_diff_eq!(ma[0], 20.0, epsilon = 1e-10);
        assert_abs_diff_eq!(ma[1], 30.0, epsilon = 1e-10);
        assert_abs_diff_eq!(ma[2], 40.0, epsilon = 1e-10);
    }

    #[test]
    fn test_exponential_moving_average() {
        let times = array![1.0, 2.0, 3.0, 4.0, 5.0];
        let values = array![10.0, 20.0, 30.0, 40.0, 50.0];
        let ts = TimeSeries::new(times, values)
            .expect("TimeSeries creation should succeed with matching dimensions");

        let ema = ts
            .exponential_moving_average(0.5)
            .expect("EMA calculation should succeed with valid alpha");
        assert_eq!(ema.len(), 5);
        assert_abs_diff_eq!(ema[0], 10.0, epsilon = 1e-10);
    }

    #[test]
    fn test_linear_interpolate() {
        let times = array![1.0, 2.0, 3.0, 4.0, 5.0];
        let values = array![10.0, 20.0, 0.0, 40.0, 50.0]; // 0.0 is placeholder for missing
        let mask = array![false, false, true, false, false];
        let ts = TimeSeries::new(times, values)
            .expect("TimeSeries creation should succeed with matching dimensions");

        let interpolated = ts
            .linear_interpolate(&mask)
            .expect("Linear interpolation should succeed with valid mask");
        assert_abs_diff_eq!(interpolated[2], 30.0, epsilon = 1e-10);
    }
}
