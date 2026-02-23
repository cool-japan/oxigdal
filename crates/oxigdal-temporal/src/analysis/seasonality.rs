//! Seasonality Analysis Module
//!
//! Implements seasonal decomposition algorithms for extracting seasonal patterns,
//! trends, and residuals from time series data.

use crate::error::{Result, TemporalError};
use crate::timeseries::TimeSeriesRaster;
use scirs2_core::ndarray::Array3;
use serde::{Deserialize, Serialize};
use tracing::info;

/// Seasonality decomposition method
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SeasonalityMethod {
    /// Additive decomposition (Y = T + S + R)
    Additive,
    /// Multiplicative decomposition (Y = T * S * R)
    Multiplicative,
    /// STL (Seasonal-Trend decomposition using Loess)
    STL,
}

/// Seasonal decomposition result
#[derive(Debug, Clone)]
pub struct SeasonalityResult {
    /// Trend component
    pub trend: Array3<f64>,
    /// Seasonal component
    pub seasonal: Array3<f64>,
    /// Residual component
    pub residual: Array3<f64>,
    /// Seasonal period detected
    pub period: Option<usize>,
}

impl SeasonalityResult {
    /// Create new seasonality result
    #[must_use]
    pub fn new(trend: Array3<f64>, seasonal: Array3<f64>, residual: Array3<f64>) -> Self {
        Self {
            trend,
            seasonal,
            residual,
            period: None,
        }
    }

    /// Add detected period
    #[must_use]
    pub fn with_period(mut self, period: usize) -> Self {
        self.period = Some(period);
        self
    }
}

/// Seasonality analyzer
pub struct SeasonalityAnalyzer;

impl SeasonalityAnalyzer {
    /// Decompose time series into seasonal components
    ///
    /// # Errors
    /// Returns error if decomposition fails
    pub fn decompose(
        ts: &TimeSeriesRaster,
        method: SeasonalityMethod,
        period: usize,
    ) -> Result<SeasonalityResult> {
        match method {
            SeasonalityMethod::Additive => Self::additive_decomposition(ts, period),
            SeasonalityMethod::Multiplicative => Self::multiplicative_decomposition(ts, period),
            SeasonalityMethod::STL => Self::stl_decomposition(ts, period),
        }
    }

    /// Additive seasonal decomposition
    fn additive_decomposition(ts: &TimeSeriesRaster, period: usize) -> Result<SeasonalityResult> {
        if ts.len() < period * 2 {
            return Err(TemporalError::insufficient_data(
                "Need at least 2 full periods",
            ));
        }

        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        let mut trend = Array3::zeros((height, width, n_bands));
        let mut seasonal = Array3::zeros((height, width, n_bands));
        let mut residual = Array3::zeros((height, width, n_bands));

        for i in 0..height {
            for j in 0..width {
                for k in 0..n_bands {
                    let values = ts.extract_pixel_timeseries(i, j, k)?;

                    // Compute trend using moving average
                    let trend_vals = Self::moving_average(&values, period);

                    // Detrend
                    let detrended: Vec<f64> = values
                        .iter()
                        .zip(trend_vals.iter())
                        .map(|(y, t)| y - t)
                        .collect();

                    // Compute seasonal component
                    let seasonal_vals = Self::extract_seasonal(&detrended, period);

                    // Compute residuals
                    let residual_vals: Vec<f64> = values
                        .iter()
                        .zip(trend_vals.iter())
                        .zip(seasonal_vals.iter())
                        .map(|((y, t), s)| y - t - s)
                        .collect();

                    // Store mean values (simplified for spatial data)
                    trend[[i, j, k]] = trend_vals.iter().sum::<f64>() / trend_vals.len() as f64;
                    seasonal[[i, j, k]] =
                        seasonal_vals.iter().sum::<f64>() / seasonal_vals.len() as f64;
                    residual[[i, j, k]] =
                        residual_vals.iter().sum::<f64>() / residual_vals.len() as f64;
                }
            }
        }

        info!("Completed additive seasonal decomposition");
        Ok(SeasonalityResult::new(trend, seasonal, residual).with_period(period))
    }

    /// Multiplicative seasonal decomposition
    fn multiplicative_decomposition(
        ts: &TimeSeriesRaster,
        period: usize,
    ) -> Result<SeasonalityResult> {
        if ts.len() < period * 2 {
            return Err(TemporalError::insufficient_data(
                "Need at least 2 full periods",
            ));
        }

        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        let mut trend = Array3::zeros((height, width, n_bands));
        let mut seasonal = Array3::zeros((height, width, n_bands));
        let mut residual = Array3::zeros((height, width, n_bands));

        for i in 0..height {
            for j in 0..width {
                for k in 0..n_bands {
                    let values = ts.extract_pixel_timeseries(i, j, k)?;

                    // Compute trend using moving average
                    let trend_vals = Self::moving_average(&values, period);

                    // Detrend (divide for multiplicative)
                    let detrended: Vec<f64> = values
                        .iter()
                        .zip(trend_vals.iter())
                        .map(|(y, t)| if *t != 0.0 { y / t } else { 1.0 })
                        .collect();

                    // Compute seasonal component
                    let seasonal_vals = Self::extract_seasonal(&detrended, period);

                    // Compute residuals
                    let residual_vals: Vec<f64> = values
                        .iter()
                        .zip(trend_vals.iter())
                        .zip(seasonal_vals.iter())
                        .map(|((y, t), s)| {
                            if *t != 0.0 && *s != 0.0 {
                                y / (t * s)
                            } else {
                                1.0
                            }
                        })
                        .collect();

                    trend[[i, j, k]] = trend_vals.iter().sum::<f64>() / trend_vals.len() as f64;
                    seasonal[[i, j, k]] =
                        seasonal_vals.iter().sum::<f64>() / seasonal_vals.len() as f64;
                    residual[[i, j, k]] =
                        residual_vals.iter().sum::<f64>() / residual_vals.len() as f64;
                }
            }
        }

        info!("Completed multiplicative seasonal decomposition");
        Ok(SeasonalityResult::new(trend, seasonal, residual).with_period(period))
    }

    /// STL decomposition (Seasonal-Trend using Loess)
    fn stl_decomposition(ts: &TimeSeriesRaster, period: usize) -> Result<SeasonalityResult> {
        // For now, use additive decomposition as a placeholder
        // Full STL implementation would require Loess smoothing
        info!("STL decomposition using additive approximation");
        Self::additive_decomposition(ts, period)
    }

    /// Moving average smoothing
    fn moving_average(values: &[f64], window: usize) -> Vec<f64> {
        let mut result = Vec::with_capacity(values.len());
        let half_window = window / 2;

        for i in 0..values.len() {
            let start = i.saturating_sub(half_window);
            let end = (i + half_window + 1).min(values.len());

            let sum: f64 = values[start..end].iter().sum();
            let count = (end - start) as f64;
            result.push(sum / count);
        }

        result
    }

    /// Extract seasonal component
    fn extract_seasonal(detrended: &[f64], period: usize) -> Vec<f64> {
        let mut seasonal = vec![0.0; detrended.len()];
        let n_periods = detrended.len() / period;

        if n_periods < 1 {
            return seasonal;
        }

        // Calculate average for each position in the period
        for i in 0..period {
            let mut sum = 0.0;
            let mut count = 0;

            for p in 0..n_periods {
                let idx = p * period + i;
                if idx < detrended.len() {
                    sum += detrended[idx];
                    count += 1;
                }
            }

            let avg = if count > 0 { sum / count as f64 } else { 0.0 };

            // Assign to all occurrences
            for p in 0..n_periods {
                let idx = p * period + i;
                if idx < seasonal.len() {
                    seasonal[idx] = avg;
                }
            }
        }

        // Handle remainder
        for i in (n_periods * period)..detrended.len() {
            let phase = i % period;
            seasonal[i] = seasonal[phase];
        }

        seasonal
    }

    /// Detect seasonal period using autocorrelation
    ///
    /// # Errors
    /// Returns error if detection fails
    pub fn detect_period(ts: &TimeSeriesRaster, max_lag: usize) -> Result<usize> {
        if ts.is_empty() {
            return Err(TemporalError::insufficient_data("Empty time series"));
        }

        // Use first pixel as representative
        let values = ts.extract_pixel_timeseries(0, 0, 0)?;

        let mut best_lag = 1;
        let mut max_corr = f64::NEG_INFINITY;

        for lag in 1..=max_lag.min(values.len() / 2) {
            let corr = Self::autocorrelation(&values, lag);
            if corr > max_corr {
                max_corr = corr;
                best_lag = lag;
            }
        }

        info!(
            "Detected seasonal period: {} (correlation: {})",
            best_lag, max_corr
        );
        Ok(best_lag)
    }

    /// Calculate autocorrelation at specific lag
    fn autocorrelation(values: &[f64], lag: usize) -> f64 {
        if lag >= values.len() {
            return 0.0;
        }

        let mean: f64 = values.iter().sum::<f64>() / values.len() as f64;
        let n = values.len() - lag;

        let mut numerator = 0.0;
        let mut denominator = 0.0;

        for i in 0..n {
            numerator += (values[i] - mean) * (values[i + lag] - mean);
        }

        for val in values {
            denominator += (val - mean).powi(2);
        }

        if denominator != 0.0 {
            numerator / denominator
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeseries::{TemporalMetadata, TimeSeriesRaster};
    use chrono::{DateTime, NaiveDate};

    #[test]
    fn test_additive_decomposition() {
        let mut ts = TimeSeriesRaster::new();

        // Create synthetic seasonal data
        for i in 0..24 {
            let dt = DateTime::from_timestamp(1640995200 + i * 86400, 0).expect("valid");
            let date = NaiveDate::from_ymd_opt(2022, 1, 1 + i as u32).expect("valid");
            let metadata = TemporalMetadata::new(dt, date);

            // Trend + seasonal pattern
            let value = (i as f64) + 10.0 * ((i as f64 * 2.0 * std::f64::consts::PI / 12.0).sin());
            let data = Array3::from_elem((5, 5, 1), value);
            ts.add_raster(metadata, data).expect("should add");
        }

        let result = SeasonalityAnalyzer::decompose(&ts, SeasonalityMethod::Additive, 12)
            .expect("should decompose");

        assert!(result.trend[[0, 0, 0]].abs() > 0.0);
        assert_eq!(result.period, Some(12));
    }

    #[test]
    fn test_detect_period() {
        let mut ts = TimeSeriesRaster::new();

        let base_date = NaiveDate::from_ymd_opt(2022, 1, 1).expect("valid base date");
        for i in 0..36 {
            let dt = DateTime::from_timestamp(1640995200 + i * 86400, 0).expect("valid");
            let date = base_date + chrono::Duration::days(i);
            let metadata = TemporalMetadata::new(dt, date);

            let value = 10.0 * ((i as f64 * 2.0 * std::f64::consts::PI / 7.0).sin());
            let data = Array3::from_elem((3, 3, 1), value);
            ts.add_raster(metadata, data).expect("should add");
        }

        let period = SeasonalityAnalyzer::detect_period(&ts, 15).expect("should detect");
        // Should detect weekly pattern (7 days)
        assert!((6..=8).contains(&period));
    }
}
