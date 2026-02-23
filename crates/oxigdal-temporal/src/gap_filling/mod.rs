//! Gap Filling Module
//!
//! Implements gap filling methods for temporal data including interpolation,
//! harmonic regression, and other techniques to fill missing values in time series.

use crate::error::{Result, TemporalError};
use crate::timeseries::TimeSeriesRaster;
use scirs2_core::ndarray::Array3;
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;
use tracing::info;

pub mod harmonic;
pub mod interpolation;

/// Gap filling method
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GapFillMethod {
    /// Linear interpolation
    LinearInterpolation,
    /// Spline interpolation
    SplineInterpolation,
    /// Nearest neighbor
    NearestNeighbor,
    /// Harmonic regression
    HarmonicRegression,
    /// Moving average
    MovingAverage,
    /// Forward fill (propagate last valid value)
    ForwardFill,
    /// Backward fill (propagate next valid value)
    BackwardFill,
}

/// Gap filling result
#[derive(Debug, Clone)]
pub struct GapFillResult {
    /// Filled data
    pub data: Array3<f64>,
    /// Filled count per pixel
    pub filled_count: Array3<usize>,
    /// Quality/confidence of fill
    pub quality: Option<Array3<f64>>,
}

impl GapFillResult {
    /// Create new gap fill result
    #[must_use]
    pub fn new(data: Array3<f64>, filled_count: Array3<usize>) -> Self {
        Self {
            data,
            filled_count,
            quality: None,
        }
    }

    /// Add quality scores
    #[must_use]
    pub fn with_quality(mut self, quality: Array3<f64>) -> Self {
        self.quality = Some(quality);
        self
    }
}

/// Gap filler
pub struct GapFiller;

impl GapFiller {
    /// Fill gaps in time series
    ///
    /// # Errors
    /// Returns error if gap filling fails
    pub fn fill_gaps(
        ts: &TimeSeriesRaster,
        method: GapFillMethod,
        params: Option<GapFillParams>,
    ) -> Result<TimeSeriesRaster> {
        match method {
            GapFillMethod::LinearInterpolation => Self::linear_interpolation(ts),
            GapFillMethod::SplineInterpolation => Self::spline_interpolation(ts),
            GapFillMethod::NearestNeighbor => Self::nearest_neighbor(ts),
            GapFillMethod::HarmonicRegression => {
                let period = params.map_or(12, |p| p.harmonic_period);
                Self::harmonic_regression(ts, period)
            }
            GapFillMethod::MovingAverage => {
                let window = params.map_or(3, |p| p.window_size);
                Self::moving_average(ts, window)
            }
            GapFillMethod::ForwardFill => Self::forward_fill(ts),
            GapFillMethod::BackwardFill => Self::backward_fill(ts),
        }
    }

    /// Linear interpolation gap filling
    fn linear_interpolation(ts: &TimeSeriesRaster) -> Result<TimeSeriesRaster> {
        if ts.len() < 2 {
            return Err(TemporalError::insufficient_data(
                "Need at least 2 observations",
            ));
        }

        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        let mut filled_ts = ts.clone();

        for i in 0..height {
            for j in 0..width {
                for k in 0..n_bands {
                    let values = ts.extract_pixel_timeseries(i, j, k)?;
                    let filled = Self::interpolate_linear(&values);

                    // Update time series with filled values
                    for (t, entry) in filled_ts.entries_mut().values_mut().enumerate() {
                        if let Some(data) = &mut entry.data {
                            data[[i, j, k]] = filled[t];
                        }
                    }
                }
            }
        }

        info!("Completed linear interpolation gap filling");
        Ok(filled_ts)
    }

    /// Interpolate gaps linearly
    fn interpolate_linear(values: &[f64]) -> Vec<f64> {
        let mut result = values.to_vec();

        for i in 0..result.len() {
            if result[i].is_nan() {
                // Find previous valid value
                let mut prev_idx = None;
                for j in (0..i).rev() {
                    if !result[j].is_nan() {
                        prev_idx = Some(j);
                        break;
                    }
                }

                // Find next valid value
                let next_idx = result[(i + 1)..]
                    .iter()
                    .position(|&v| !v.is_nan())
                    .map(|idx| idx + i + 1);

                // Interpolate
                if let (Some(prev), Some(next)) = (prev_idx, next_idx) {
                    let prev_val = result[prev];
                    let next_val = result[next];
                    let weight = (i - prev) as f64 / (next - prev) as f64;
                    result[i] = prev_val + weight * (next_val - prev_val);
                }
            }
        }

        result
    }

    /// Spline interpolation gap filling
    fn spline_interpolation(ts: &TimeSeriesRaster) -> Result<TimeSeriesRaster> {
        // Use linear interpolation as approximation
        info!("Using linear interpolation approximation for spline");
        Self::linear_interpolation(ts)
    }

    /// Nearest neighbor gap filling
    fn nearest_neighbor(ts: &TimeSeriesRaster) -> Result<TimeSeriesRaster> {
        if ts.is_empty() {
            return Err(TemporalError::insufficient_data("Empty time series"));
        }

        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        let mut filled_ts = ts.clone();

        for i in 0..height {
            for j in 0..width {
                for k in 0..n_bands {
                    let values = ts.extract_pixel_timeseries(i, j, k)?;
                    let filled = Self::fill_nearest(&values);

                    for (t, entry) in filled_ts.entries_mut().values_mut().enumerate() {
                        if let Some(data) = &mut entry.data {
                            data[[i, j, k]] = filled[t];
                        }
                    }
                }
            }
        }

        info!("Completed nearest neighbor gap filling");
        Ok(filled_ts)
    }

    /// Fill with nearest valid value
    fn fill_nearest(values: &[f64]) -> Vec<f64> {
        let mut result = values.to_vec();

        for i in 0..result.len() {
            if result[i].is_nan() {
                let nearest_val = result
                    .iter()
                    .enumerate()
                    .filter(|(_, v)| !v.is_nan())
                    .min_by_key(|(j, _)| i.abs_diff(*j))
                    .map(|(_, v)| *v)
                    .unwrap_or(f64::NAN);

                result[i] = nearest_val;
            }
        }

        result
    }

    /// Harmonic regression gap filling
    fn harmonic_regression(ts: &TimeSeriesRaster, period: usize) -> Result<TimeSeriesRaster> {
        if ts.len() < period {
            return Err(TemporalError::insufficient_data(format!(
                "Need at least {} observations for period {}",
                period, period
            )));
        }

        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        let mut filled_ts = ts.clone();

        for i in 0..height {
            for j in 0..width {
                for k in 0..n_bands {
                    let values = ts.extract_pixel_timeseries(i, j, k)?;
                    let filled = Self::fit_harmonic(&values, period);

                    for (t, entry) in filled_ts.entries_mut().values_mut().enumerate() {
                        if let Some(data) = &mut entry.data {
                            data[[i, j, k]] = filled[t];
                        }
                    }
                }
            }
        }

        info!("Completed harmonic regression gap filling");
        Ok(filled_ts)
    }

    /// Fit harmonic function to data
    fn fit_harmonic(values: &[f64], period: usize) -> Vec<f64> {
        let n = values.len();

        // Simple harmonic model: y = a + b*sin(2*pi*t/P) + c*cos(2*pi*t/P)
        let valid_data: Vec<(usize, f64)> = values
            .iter()
            .enumerate()
            .filter(|(_, v)| !v.is_nan())
            .map(|(i, &v)| (i, v))
            .collect();

        if valid_data.is_empty() {
            return values.to_vec();
        }

        // Least squares fit
        let mut sum_y = 0.0;
        let mut sum_sin = 0.0;
        let mut sum_cos = 0.0;
        let mut sum_sin2 = 0.0;
        let mut sum_cos2 = 0.0;
        let mut sum_y_sin = 0.0;
        let mut sum_y_cos = 0.0;

        for &(t, y) in &valid_data {
            let phase = 2.0 * PI * (t as f64) / (period as f64);
            let sin_val = phase.sin();
            let cos_val = phase.cos();

            sum_y += y;
            sum_sin += sin_val;
            sum_cos += cos_val;
            sum_sin2 += sin_val * sin_val;
            sum_cos2 += cos_val * cos_val;
            sum_y_sin += y * sin_val;
            sum_y_cos += y * cos_val;
        }

        let n_valid = valid_data.len() as f64;
        let a = sum_y / n_valid;
        let b = (sum_y_sin - sum_sin * sum_y / n_valid) / (sum_sin2 - sum_sin * sum_sin / n_valid);
        let c = (sum_y_cos - sum_cos * sum_y / n_valid) / (sum_cos2 - sum_cos * sum_cos / n_valid);

        // Generate fitted values
        values
            .iter()
            .enumerate()
            .take(n)
            .map(|(t, val)| {
                let phase = 2.0 * PI * (t as f64) / (period as f64);
                let fitted = a + b * phase.sin() + c * phase.cos();
                if val.is_nan() { fitted } else { *val }
            })
            .collect()
    }

    /// Moving average gap filling
    fn moving_average(ts: &TimeSeriesRaster, window: usize) -> Result<TimeSeriesRaster> {
        if ts.len() < window {
            return Err(TemporalError::insufficient_data(format!(
                "Need at least {} observations",
                window
            )));
        }

        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        let mut filled_ts = ts.clone();

        for i in 0..height {
            for j in 0..width {
                for k in 0..n_bands {
                    let values = ts.extract_pixel_timeseries(i, j, k)?;
                    let filled = Self::fill_moving_average(&values, window);

                    for (t, entry) in filled_ts.entries_mut().values_mut().enumerate() {
                        if let Some(data) = &mut entry.data {
                            data[[i, j, k]] = filled[t];
                        }
                    }
                }
            }
        }

        info!("Completed moving average gap filling");
        Ok(filled_ts)
    }

    /// Fill with moving average
    fn fill_moving_average(values: &[f64], window: usize) -> Vec<f64> {
        let mut result = values.to_vec();
        let half_window = window / 2;

        for i in 0..result.len() {
            if result[i].is_nan() {
                let start = i.saturating_sub(half_window);
                let end = (i + half_window + 1).min(result.len());

                let valid_values: Vec<f64> = result[start..end]
                    .iter()
                    .filter(|v| !v.is_nan())
                    .copied()
                    .collect();

                if !valid_values.is_empty() {
                    result[i] = valid_values.iter().sum::<f64>() / valid_values.len() as f64;
                }
            }
        }

        result
    }

    /// Forward fill
    fn forward_fill(ts: &TimeSeriesRaster) -> Result<TimeSeriesRaster> {
        if ts.is_empty() {
            return Err(TemporalError::insufficient_data("Empty time series"));
        }

        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        let mut filled_ts = ts.clone();

        for i in 0..height {
            for j in 0..width {
                for k in 0..n_bands {
                    let values = ts.extract_pixel_timeseries(i, j, k)?;
                    let mut last_valid = f64::NAN;
                    let mut filled = Vec::with_capacity(values.len());

                    for &value in &values {
                        let v: f64 = value;
                        if !v.is_nan() && v.is_finite() {
                            last_valid = v;
                            filled.push(v);
                        } else {
                            filled.push(last_valid);
                        }
                    }

                    for (t, entry) in filled_ts.entries_mut().values_mut().enumerate() {
                        if let Some(data) = &mut entry.data {
                            data[[i, j, k]] = filled[t];
                        }
                    }
                }
            }
        }

        info!("Completed forward fill");
        Ok(filled_ts)
    }

    /// Backward fill
    fn backward_fill(ts: &TimeSeriesRaster) -> Result<TimeSeriesRaster> {
        if ts.is_empty() {
            return Err(TemporalError::insufficient_data("Empty time series"));
        }

        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        let mut filled_ts = ts.clone();

        for i in 0..height {
            for j in 0..width {
                for k in 0..n_bands {
                    let values = ts.extract_pixel_timeseries(i, j, k)?;
                    let mut filled = values.clone();
                    let mut next_valid = f64::NAN;

                    for t in (0..values.len()).rev() {
                        if !values[t].is_nan() {
                            next_valid = values[t];
                        } else {
                            filled[t] = next_valid;
                        }
                    }

                    for (t, entry) in filled_ts.entries_mut().values_mut().enumerate() {
                        if let Some(data) = &mut entry.data {
                            data[[i, j, k]] = filled[t];
                        }
                    }
                }
            }
        }

        info!("Completed backward fill");
        Ok(filled_ts)
    }
}

/// Gap filling parameters
#[derive(Debug, Clone, Copy)]
pub struct GapFillParams {
    /// Window size for moving average
    pub window_size: usize,
    /// Period for harmonic regression
    pub harmonic_period: usize,
    /// Maximum gap size to fill
    pub max_gap_size: Option<usize>,
}

impl Default for GapFillParams {
    fn default() -> Self {
        Self {
            window_size: 3,
            harmonic_period: 12,
            max_gap_size: None,
        }
    }
}
