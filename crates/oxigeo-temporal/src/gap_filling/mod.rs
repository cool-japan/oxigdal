//! Gap Filling Module
//!
//! Implements gap filling methods for temporal data including interpolation,
//! harmonic regression, and other techniques to fill missing values in time series.

use crate::error::{Result, TemporalError};
use crate::timeseries::TimeSeriesRaster;
use scirs2_core::linalg::solve_ndarray;
use scirs2_core::ndarray::{Array1, Array2, Array3};
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;
use tracing::info;

pub mod harmonic;
pub mod interpolation;
pub mod savgol;
pub mod spline;
pub mod whittaker;

/// Gap filling method
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GapFillMethod {
    /// Linear interpolation
    LinearInterpolation,
    /// Natural cubic spline interpolation (see [`gap_filling::spline`](crate::gap_filling::spline)).
    /// Fits a `C²`-continuous piecewise cubic through all valid anchor points
    /// per pixel timeseries, producing curvature rather than the straight
    /// line segments of [`GapFillMethod::LinearInterpolation`].
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
    /// Whittaker smoother (Eilers 2003, Anal. Chem. 75:3631)
    Whittaker,
    /// Savitzky-Golay polynomial smoothing filter (Savitzky & Golay 1964)
    SavitzkyGolay,
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
        let mut filled = match method {
            GapFillMethod::LinearInterpolation => Self::linear_interpolation(ts)?,
            GapFillMethod::SplineInterpolation => Self::spline_interpolation(ts)?,
            GapFillMethod::NearestNeighbor => Self::nearest_neighbor(ts)?,
            GapFillMethod::HarmonicRegression => {
                let period = params.map_or(12, |p| p.harmonic_period);
                Self::harmonic_regression(ts, period)?
            }
            GapFillMethod::MovingAverage => {
                let window = params.map_or(3, |p| p.window_size);
                Self::moving_average(ts, window)?
            }
            GapFillMethod::ForwardFill => Self::forward_fill(ts)?,
            GapFillMethod::BackwardFill => Self::backward_fill(ts)?,
            GapFillMethod::Whittaker => {
                let lambda = params.map_or(100.0, |p| p.whittaker_lambda);
                let order = params.map_or(2, |p| p.whittaker_order);
                Self::whittaker_smooth(ts, lambda, order)?
            }
            GapFillMethod::SavitzkyGolay => {
                let win = params.map_or(7, |p| p.savgol_window);
                let poly = params.map_or(2, |p| p.savgol_poly_order);
                Self::savitzky_golay_smooth(ts, win, poly)?
            }
        };

        // Honor `max_gap_size`: any maximal run of consecutive missing
        // observations longer than the threshold is left unfilled (restored to
        // NaN) so a caller who sets, e.g., `max_gap_size: Some(3)` does not get
        // a multi-month outage silently interpolated across.
        if let Some(max_gap) = params.and_then(|p| p.max_gap_size) {
            Self::apply_max_gap_size(ts, &mut filled, max_gap)?;
        }

        Ok(filled)
    }

    /// Restore `NaN` at positions belonging to a gap (maximal run of
    /// consecutive missing observations in the *original* series) longer than
    /// `max_gap`, undoing any fill for those positions.
    ///
    /// A `max_gap` of 0 means no gap may be filled at all.
    fn apply_max_gap_size(
        original: &TimeSeriesRaster,
        filled: &mut TimeSeriesRaster,
        max_gap: usize,
    ) -> Result<()> {
        let (height, width, n_bands) = original
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        for i in 0..height {
            for j in 0..width {
                for k in 0..n_bands {
                    let orig = original.extract_pixel_timeseries(i, j, k)?;
                    let n = orig.len();
                    let mut t = 0;
                    while t < n {
                        if orig[t].is_nan() {
                            let start = t;
                            while t < n && orig[t].is_nan() {
                                t += 1;
                            }
                            let run_len = t - start;
                            if run_len > max_gap {
                                for (idx, entry) in filled.entries_mut().values_mut().enumerate() {
                                    if idx >= start
                                        && idx < t
                                        && let Some(data) = &mut entry.data
                                    {
                                        data[[i, j, k]] = f64::NAN;
                                    }
                                }
                            }
                        } else {
                            t += 1;
                        }
                    }
                }
            }
        }

        Ok(())
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

    /// Natural cubic spline interpolation gap filling.
    ///
    /// Fits a natural cubic spline (see [`spline::fill_natural_cubic_spline`])
    /// through the valid observations of each pixel timeseries, producing a
    /// smoothly-curved fill rather than the piecewise-linear segments used by
    /// [`Self::linear_interpolation`].
    fn spline_interpolation(ts: &TimeSeriesRaster) -> Result<TimeSeriesRaster> {
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
                    let filled = spline::fill_natural_cubic_spline(&values);

                    for (t, entry) in filled_ts.entries_mut().values_mut().enumerate() {
                        if let Some(data) = &mut entry.data {
                            data[[i, j, k]] = filled[t];
                        }
                    }
                }
            }
        }

        info!("Completed natural cubic spline gap filling");
        Ok(filled_ts)
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

    /// Fit harmonic function to data.
    ///
    /// Model: `y = a + b*sin(2*pi*t/P) + c*cos(2*pi*t/P)`, fitted by ordinary
    /// least squares over the valid (non-NaN) samples via the proper 3x3
    /// normal-equation system `(AᵀA)·β = Aᵀy` (design matrix columns
    /// `[1, sin(phase), cos(phase)]`), solved with
    /// [`scirs2_core::linalg::solve_ndarray`] — the same approach used by
    /// [`savgol::smooth_savgol`] and [`whittaker::smooth_whittaker`] in this
    /// module. Unlike independent marginal regressions of `y` on `sin` and
    /// `y` on `cos` separately, this accounts for the sin/cos cross-term and
    /// is exact even when the valid samples are not orthogonal over a full
    /// period (e.g. gappy or irregularly-sampled series).
    fn fit_harmonic(values: &[f64], period: usize) -> Vec<f64> {
        let valid_data: Vec<(usize, f64)> = values
            .iter()
            .enumerate()
            .filter(|(_, v)| !v.is_nan())
            .map(|(i, &v)| (i, v))
            .collect();

        if valid_data.is_empty() {
            return values.to_vec();
        }

        let n_valid = valid_data.len();
        let mean = || valid_data.iter().map(|&(_, y)| y).sum::<f64>() / n_valid as f64;

        // Fewer than 3 valid samples cannot uniquely determine 3 unknowns
        // (a, b, c); fall back to the sample mean (b = c = 0).
        let (a_coef, b, c) = if n_valid < 3 {
            (mean(), 0.0, 0.0)
        } else {
            let mut design = Array2::<f64>::zeros((n_valid, 3));
            let mut target = Array1::<f64>::zeros(n_valid);
            for (row, &(t, y)) in valid_data.iter().enumerate() {
                let phase = 2.0 * PI * (t as f64) / (period as f64);
                design[[row, 0]] = 1.0;
                design[[row, 1]] = phase.sin();
                design[[row, 2]] = phase.cos();
                target[row] = y;
            }

            let design_t = design.t().to_owned();
            let ata = design_t.dot(&design);
            let aty = design_t.dot(&target);

            match solve_ndarray(&ata, &aty) {
                Ok(beta) => (beta[0], beta[1], beta[2]),
                Err(_) => {
                    // Degenerate/singular normal-equation matrix (e.g. all
                    // valid samples land on the same phase); fall back to
                    // the sample mean rather than a biased estimate.
                    (mean(), 0.0, 0.0)
                }
            }
        };

        values
            .iter()
            .enumerate()
            .map(|(t, val)| {
                let phase = 2.0 * PI * (t as f64) / (period as f64);
                let fitted = a_coef + b * phase.sin() + c * phase.cos();
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

    /// Whittaker smoother gap filling (Eilers 2003).
    fn whittaker_smooth(
        ts: &TimeSeriesRaster,
        lambda: f64,
        order: usize,
    ) -> Result<TimeSeriesRaster> {
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
                    let smoothed = whittaker::smooth_whittaker(&values, lambda, order);

                    for (t, entry) in filled_ts.entries_mut().values_mut().enumerate() {
                        if let Some(data) = &mut entry.data {
                            data[[i, j, k]] = smoothed[t];
                        }
                    }
                }
            }
        }

        info!("Completed Whittaker smoother gap filling (lambda={lambda}, order={order})");
        Ok(filled_ts)
    }

    /// Savitzky-Golay smoothing filter gap filling.
    fn savitzky_golay_smooth(
        ts: &TimeSeriesRaster,
        window: usize,
        poly_order: usize,
    ) -> Result<TimeSeriesRaster> {
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
                    let smoothed = savgol::smooth_savgol(&values, window, poly_order);

                    for (t, entry) in filled_ts.entries_mut().values_mut().enumerate() {
                        if let Some(data) = &mut entry.data {
                            data[[i, j, k]] = smoothed[t];
                        }
                    }
                }
            }
        }

        info!("Completed Savitzky-Golay smoothing (window={window}, poly_order={poly_order})");
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
    /// Smoothness penalty weight λ for the Whittaker smoother.
    /// Larger values produce a smoother (less data-faithful) estimate.
    /// Typical range for NDVI: 10–10000. Default: 100.0.
    pub whittaker_lambda: f64,
    /// Order of the finite-difference penalty for the Whittaker smoother.
    /// 1 = penalise first differences (roughness), 2 = penalise second
    /// differences (curvature). Default: 2.
    pub whittaker_order: usize,
    /// Window size for the Savitzky-Golay filter (must be odd; if even it is
    /// incremented by one). Default: 7.
    pub savgol_window: usize,
    /// Polynomial order for the Savitzky-Golay filter (must be < window).
    /// Default: 2.
    pub savgol_poly_order: usize,
}

impl Default for GapFillParams {
    fn default() -> Self {
        Self {
            window_size: 3,
            harmonic_period: 12,
            max_gap_size: None,
            whittaker_lambda: 100.0,
            whittaker_order: 2,
            savgol_window: 7,
            savgol_poly_order: 2,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::timeseries::TemporalMetadata;
    use chrono::{DateTime, NaiveDate, Utc};
    use scirs2_core::ndarray::Array3;

    fn meta(day: u32) -> TemporalMetadata {
        let date = NaiveDate::from_ymd_opt(2024, 1, day).expect("valid date");
        let ndt = date.and_hms_opt(0, 0, 0).expect("valid time");
        let ts = DateTime::from_naive_utc_and_offset(ndt, Utc);
        TemporalMetadata::new(ts, date)
    }

    /// Build a 1x1x1 time series from a slice of per-timestep values
    /// (NaN = missing).
    fn ts_from(values: &[f64]) -> TimeSeriesRaster {
        let mut ts = TimeSeriesRaster::new();
        for (idx, &v) in values.iter().enumerate() {
            let raster = Array3::from_elem((1, 1, 1), v);
            ts.add_raster(meta(idx as u32 + 1), raster).unwrap();
        }
        ts
    }

    fn pixel_series(ts: &TimeSeriesRaster) -> Vec<f64> {
        ts.extract_pixel_timeseries(0, 0, 0).unwrap()
    }

    #[test]
    fn test_max_gap_size_leaves_long_gaps_unfilled() {
        // A single-step gap and a 3-step gap. With max_gap_size = 1, only the
        // single-step gap may be filled; the 3-step run must remain NaN.
        let values = vec![
            1.0,
            f64::NAN, // 1-step gap (fillable)
            3.0,
            f64::NAN,
            f64::NAN,
            f64::NAN, // 3-step gap (too long)
            7.0,
        ];
        let ts = ts_from(&values);

        let params = GapFillParams {
            max_gap_size: Some(1),
            ..GapFillParams::default()
        };

        let filled =
            GapFiller::fill_gaps(&ts, GapFillMethod::LinearInterpolation, Some(params)).unwrap();
        let out = pixel_series(&filled);

        // 1-step gap filled to 2.0 (linear between 1.0 and 3.0).
        assert!(
            (out[1] - 2.0).abs() < 1e-9,
            "short gap should be filled, got {}",
            out[1]
        );
        // The 3-step gap must stay NaN.
        assert!(out[3].is_nan(), "long gap position 3 must stay NaN");
        assert!(out[4].is_nan(), "long gap position 4 must stay NaN");
        assert!(out[5].is_nan(), "long gap position 5 must stay NaN");
        // Anchors preserved.
        assert_eq!(out[0], 1.0);
        assert_eq!(out[2], 3.0);
        assert_eq!(out[6], 7.0);
    }

    #[test]
    fn test_no_max_gap_size_fills_everything() {
        // Without max_gap_size the whole gap is interpolated.
        let values = vec![1.0, f64::NAN, f64::NAN, f64::NAN, 5.0];
        let ts = ts_from(&values);

        let filled = GapFiller::fill_gaps(&ts, GapFillMethod::LinearInterpolation, None).unwrap();
        let out = pixel_series(&filled);

        for (idx, v) in out.iter().enumerate() {
            assert!(!v.is_nan(), "position {idx} should be filled, got NaN");
        }
        assert!((out[2] - 3.0).abs() < 1e-9);
    }

    #[test]
    fn test_max_gap_size_zero_blocks_all_fills() {
        let values = vec![1.0, f64::NAN, 3.0];
        let ts = ts_from(&values);
        let params = GapFillParams {
            max_gap_size: Some(0),
            ..GapFillParams::default()
        };
        let filled =
            GapFiller::fill_gaps(&ts, GapFillMethod::LinearInterpolation, Some(params)).unwrap();
        let out = pixel_series(&filled);
        assert!(out[1].is_nan(), "max_gap_size=0 must block all fills");
    }
}
