//! Time Series Forecasting Module
//!
//! Implements forecasting algorithms for predicting future values
//! in temporal data including simple methods and more advanced techniques.

use crate::error::{Result, TemporalError};
use crate::timeseries::TimeSeriesRaster;
use scirs2_core::ndarray::Array3;
use serde::{Deserialize, Serialize};
use tracing::info;

/// Forecasting method
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ForecastMethod {
    /// Last value propagation
    LastValue,
    /// Mean of historical values
    Mean,
    /// Linear extrapolation
    LinearExtrapolation,
    /// Exponential smoothing
    ExponentialSmoothing,
    /// Moving average
    MovingAverage,
}

/// Forecast result
#[derive(Debug, Clone)]
pub struct ForecastResult {
    /// Forecasted values
    pub forecast: Array3<f64>,
    /// Confidence intervals (lower bound)
    pub lower_bound: Option<Array3<f64>>,
    /// Confidence intervals (upper bound)
    pub upper_bound: Option<Array3<f64>>,
    /// Forecast horizon (number of steps ahead)
    pub horizon: usize,
}

impl ForecastResult {
    /// Create new forecast result
    #[must_use]
    pub fn new(forecast: Array3<f64>, horizon: usize) -> Self {
        Self {
            forecast,
            lower_bound: None,
            upper_bound: None,
            horizon,
        }
    }

    /// Add confidence intervals
    #[must_use]
    pub fn with_confidence(mut self, lower_bound: Array3<f64>, upper_bound: Array3<f64>) -> Self {
        self.lower_bound = Some(lower_bound);
        self.upper_bound = Some(upper_bound);
        self
    }
}

/// Time series forecaster
pub struct Forecaster;

impl Forecaster {
    /// Generate forecast for time series
    ///
    /// # Errors
    /// Returns error if forecasting fails
    pub fn forecast(
        ts: &TimeSeriesRaster,
        method: ForecastMethod,
        horizon: usize,
        params: Option<ForecastParams>,
    ) -> Result<ForecastResult> {
        if horizon == 0 {
            return Err(TemporalError::invalid_parameter(
                "horizon",
                "must be greater than 0",
            ));
        }

        match method {
            ForecastMethod::LastValue => Self::last_value_forecast(ts, horizon),
            ForecastMethod::Mean => Self::mean_forecast(ts, horizon),
            ForecastMethod::LinearExtrapolation => Self::linear_forecast(ts, horizon),
            ForecastMethod::ExponentialSmoothing => {
                let alpha = params.map_or(0.3, |p| p.alpha);
                Self::exponential_smoothing_forecast(ts, horizon, alpha)
            }
            ForecastMethod::MovingAverage => {
                let window = params.map_or(3, |p| p.window_size);
                Self::moving_average_forecast(ts, horizon, window)
            }
        }
    }

    /// Last value propagation forecast
    fn last_value_forecast(ts: &TimeSeriesRaster, horizon: usize) -> Result<ForecastResult> {
        if ts.is_empty() {
            return Err(TemporalError::insufficient_data("Empty time series"));
        }

        // Validate shape exists
        let (_height, _width, _n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        let last_entry = ts.get_by_index(ts.len() - 1)?;
        let last_data = last_entry
            .data
            .as_ref()
            .ok_or_else(|| TemporalError::invalid_input("Data not loaded"))?;

        let forecast = last_data.clone();

        info!("Generated last value forecast for {} steps", horizon);
        Ok(ForecastResult::new(forecast, horizon))
    }

    /// Mean forecast
    fn mean_forecast(ts: &TimeSeriesRaster, horizon: usize) -> Result<ForecastResult> {
        if ts.is_empty() {
            return Err(TemporalError::insufficient_data("Empty time series"));
        }

        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        let mut forecast = Array3::zeros((height, width, n_bands));

        for i in 0..height {
            for j in 0..width {
                for k in 0..n_bands {
                    let values = ts.extract_pixel_timeseries(i, j, k)?;
                    let mean = values.iter().sum::<f64>() / values.len() as f64;
                    forecast[[i, j, k]] = mean;
                }
            }
        }

        info!("Generated mean forecast for {} steps", horizon);
        Ok(ForecastResult::new(forecast, horizon))
    }

    /// Linear extrapolation forecast
    fn linear_forecast(ts: &TimeSeriesRaster, horizon: usize) -> Result<ForecastResult> {
        if ts.len() < 2 {
            return Err(TemporalError::insufficient_data(
                "Need at least 2 observations",
            ));
        }

        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        let mut forecast = Array3::zeros((height, width, n_bands));

        for i in 0..height {
            for j in 0..width {
                for k in 0..n_bands {
                    let values = ts.extract_pixel_timeseries(i, j, k)?;

                    // Fit linear trend
                    let n = values.len() as f64;
                    let times: Vec<f64> = (0..values.len()).map(|t| t as f64).collect();

                    let sum_t: f64 = times.iter().sum();
                    let sum_y: f64 = values.iter().sum();
                    let sum_t2: f64 = times.iter().map(|t| t * t).sum();
                    let sum_ty: f64 = times.iter().zip(values.iter()).map(|(t, y)| t * y).sum();

                    let slope = (n * sum_ty - sum_t * sum_y) / (n * sum_t2 - sum_t * sum_t);
                    let intercept = (sum_y - slope * sum_t) / n;

                    // Extrapolate
                    let forecast_time = (values.len() + horizon - 1) as f64;
                    forecast[[i, j, k]] = slope * forecast_time + intercept;
                }
            }
        }

        info!("Generated linear forecast for {} steps", horizon);
        Ok(ForecastResult::new(forecast, horizon))
    }

    /// Exponential smoothing forecast
    fn exponential_smoothing_forecast(
        ts: &TimeSeriesRaster,
        horizon: usize,
        alpha: f64,
    ) -> Result<ForecastResult> {
        if ts.is_empty() {
            return Err(TemporalError::insufficient_data("Empty time series"));
        }

        if !(0.0..=1.0).contains(&alpha) {
            return Err(TemporalError::invalid_parameter(
                "alpha",
                "must be between 0 and 1",
            ));
        }

        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        let mut forecast = Array3::zeros((height, width, n_bands));

        for i in 0..height {
            for j in 0..width {
                for k in 0..n_bands {
                    let values = ts.extract_pixel_timeseries(i, j, k)?;

                    // Initialize with first value
                    let mut smoothed = values[0];

                    // Apply exponential smoothing
                    for &value in &values[1..] {
                        smoothed = alpha * value + (1.0 - alpha) * smoothed;
                    }

                    forecast[[i, j, k]] = smoothed;
                }
            }
        }

        info!(
            "Generated exponential smoothing forecast (alpha={}) for {} steps",
            alpha, horizon
        );
        Ok(ForecastResult::new(forecast, horizon))
    }

    /// Moving average forecast
    fn moving_average_forecast(
        ts: &TimeSeriesRaster,
        horizon: usize,
        window: usize,
    ) -> Result<ForecastResult> {
        if ts.len() < window {
            return Err(TemporalError::insufficient_data(format!(
                "Need at least {} observations for window size {}",
                window, window
            )));
        }

        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        let mut forecast = Array3::zeros((height, width, n_bands));

        for i in 0..height {
            for j in 0..width {
                for k in 0..n_bands {
                    let values = ts.extract_pixel_timeseries(i, j, k)?;

                    // Calculate moving average from last window
                    let start_idx = values.len().saturating_sub(window);
                    let sum: f64 = values[start_idx..].iter().sum();
                    let avg = sum / (values.len() - start_idx) as f64;

                    forecast[[i, j, k]] = avg;
                }
            }
        }

        info!(
            "Generated moving average forecast (window={}) for {} steps",
            window, horizon
        );
        Ok(ForecastResult::new(forecast, horizon))
    }

    /// Calculate forecast confidence intervals
    ///
    /// # Errors
    /// Returns error if calculation fails
    pub fn calculate_confidence(
        ts: &TimeSeriesRaster,
        forecast: &Array3<f64>,
        confidence_level: f64,
    ) -> Result<(Array3<f64>, Array3<f64>)> {
        if !(0.0..=1.0).contains(&confidence_level) {
            return Err(TemporalError::invalid_parameter(
                "confidence_level",
                "must be between 0 and 1",
            ));
        }

        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        let mut lower = Array3::zeros((height, width, n_bands));
        let mut upper = Array3::zeros((height, width, n_bands));

        // Z-score for confidence level (approximation)
        let z = match confidence_level {
            x if x >= 0.99 => 2.576,
            x if x >= 0.95 => 1.96,
            x if x >= 0.90 => 1.645,
            _ => 1.0,
        };

        for i in 0..height {
            for j in 0..width {
                for k in 0..n_bands {
                    let values = ts.extract_pixel_timeseries(i, j, k)?;

                    // Calculate standard error
                    let mean = values.iter().sum::<f64>() / values.len() as f64;
                    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>()
                        / values.len() as f64;
                    let std_error = variance.sqrt() / (values.len() as f64).sqrt();

                    let margin = z * std_error;
                    lower[[i, j, k]] = forecast[[i, j, k]] - margin;
                    upper[[i, j, k]] = forecast[[i, j, k]] + margin;
                }
            }
        }

        Ok((lower, upper))
    }
}

/// Forecast parameters
#[derive(Debug, Clone, Copy)]
pub struct ForecastParams {
    /// Alpha parameter for exponential smoothing
    pub alpha: f64,
    /// Window size for moving average
    pub window_size: usize,
}

impl Default for ForecastParams {
    fn default() -> Self {
        Self {
            alpha: 0.3,
            window_size: 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeseries::{TemporalMetadata, TimeSeriesRaster};
    use chrono::{DateTime, NaiveDate};

    #[test]
    fn test_linear_forecast() {
        let mut ts = TimeSeriesRaster::new();

        for i in 0..10 {
            let dt = DateTime::from_timestamp(1640995200 + i * 86400, 0).expect("valid");
            let date = NaiveDate::from_ymd_opt(2022, 1, 1 + i as u32).expect("valid");
            let metadata = TemporalMetadata::new(dt, date);
            let data = Array3::from_elem((3, 3, 1), (i * 2) as f64);
            ts.add_raster(metadata, data).expect("should add");
        }

        let result = Forecaster::forecast(&ts, ForecastMethod::LinearExtrapolation, 5, None)
            .expect("should forecast");

        assert_eq!(result.horizon, 5);
        // Should extrapolate the linear trend
        assert!(result.forecast[[0, 0, 0]] > 18.0);
    }

    #[test]
    fn test_exponential_smoothing() {
        let mut ts = TimeSeriesRaster::new();

        for i in 0..10 {
            let dt = DateTime::from_timestamp(1640995200 + i * 86400, 0).expect("valid");
            let date = NaiveDate::from_ymd_opt(2022, 1, 1 + i as u32).expect("valid");
            let metadata = TemporalMetadata::new(dt, date);
            let data = Array3::from_elem((3, 3, 1), 10.0 + (i as f64));
            ts.add_raster(metadata, data).expect("should add");
        }

        let params = ForecastParams {
            alpha: 0.5,
            window_size: 3,
        };

        let result =
            Forecaster::forecast(&ts, ForecastMethod::ExponentialSmoothing, 3, Some(params))
                .expect("should forecast");

        assert_eq!(result.horizon, 3);
        assert!(result.forecast[[0, 0, 0]] > 10.0);
    }
}
