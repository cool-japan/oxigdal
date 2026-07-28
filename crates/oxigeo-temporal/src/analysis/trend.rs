//! Trend Analysis Module
//!
//! Implements trend detection algorithms including linear trends, Mann-Kendall test,
//! Sen's slope estimator, and Theil-Sen regression for robust trend analysis.

use crate::error::{Result, TemporalError};
use crate::timeseries::TimeSeriesRaster;
use scirs2_core::ndarray::Array3;
use serde::{Deserialize, Serialize};
use tracing::info;

#[cfg(feature = "parallel")]
use rayon::prelude::*;

/// Trend analysis method
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrendMethod {
    /// Linear trend (OLS regression)
    Linear,
    /// Mann-Kendall test for monotonic trend
    MannKendall,
    /// Sen's slope estimator (robust)
    SensSlope,
    /// Theil-Sen estimator
    TheilSen,
}

/// Trend analysis result
#[derive(Debug, Clone)]
pub struct TrendResult {
    /// Trend slope (change per time unit)
    pub slope: Array3<f64>,
    /// Trend intercept
    pub intercept: Array3<f64>,
    /// Statistical significance (p-value)
    pub pvalue: Option<Array3<f64>>,
    /// Trend direction (-1: negative, 0: no trend, 1: positive)
    pub direction: Array3<i8>,
    /// Trend strength/magnitude
    pub magnitude: Option<Array3<f64>>,
}

impl TrendResult {
    /// Create new trend result
    #[must_use]
    pub fn new(slope: Array3<f64>, intercept: Array3<f64>, direction: Array3<i8>) -> Self {
        Self {
            slope,
            intercept,
            pvalue: None,
            direction,
            magnitude: None,
        }
    }

    /// Add p-values
    #[must_use]
    pub fn with_pvalue(mut self, pvalue: Array3<f64>) -> Self {
        self.pvalue = Some(pvalue);
        self
    }

    /// Add magnitude
    #[must_use]
    pub fn with_magnitude(mut self, magnitude: Array3<f64>) -> Self {
        self.magnitude = Some(magnitude);
        self
    }
}

/// Trend analyzer
pub struct TrendAnalyzer;

impl TrendAnalyzer {
    /// Analyze trends in time series
    ///
    /// # Errors
    /// Returns error if analysis fails
    pub fn analyze(ts: &TimeSeriesRaster, method: TrendMethod) -> Result<TrendResult> {
        match method {
            TrendMethod::Linear => Self::linear_trend(ts),
            TrendMethod::MannKendall => Self::mann_kendall(ts),
            TrendMethod::SensSlope | TrendMethod::TheilSen => Self::sens_slope(ts),
        }
    }

    /// Linear trend analysis using OLS
    fn linear_trend(ts: &TimeSeriesRaster) -> Result<TrendResult> {
        if ts.len() < 3 {
            return Err(TemporalError::insufficient_data(
                "Need at least 3 observations",
            ));
        }

        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        let mut slope = Array3::zeros((height, width, n_bands));
        let mut intercept = Array3::zeros((height, width, n_bands));

        // Compute OLS for each pixel over its NaN-filtered valid observations.
        // Cloud-masked / missing dates carry NaN; without filtering a single
        // NaN would poison the whole pixel's regression.
        #[cfg(feature = "parallel")]
        {
            use std::sync::Mutex;
            let slope_mutex = Mutex::new(&mut slope);
            let intercept_mutex = Mutex::new(&mut intercept);

            (0..height).into_par_iter().for_each(|i| {
                for j in 0..width {
                    for k in 0..n_bands {
                        if let Ok(values) = ts.extract_pixel_timeseries(i, j, k) {
                            let pairs = Self::valid_pairs(&values);
                            let (slope_val, intercept_val) =
                                Self::ols_from_pairs(&pairs).unwrap_or((f64::NAN, f64::NAN));

                            if let Ok(mut s) = slope_mutex.lock() {
                                s[[i, j, k]] = slope_val;
                            }
                            if let Ok(mut int) = intercept_mutex.lock() {
                                int[[i, j, k]] = intercept_val;
                            }
                        }
                    }
                }
            });
        }

        #[cfg(not(feature = "parallel"))]
        {
            for i in 0..height {
                for j in 0..width {
                    for k in 0..n_bands {
                        let values = ts.extract_pixel_timeseries(i, j, k)?;
                        let pairs = Self::valid_pairs(&values);
                        let (slope_val, intercept_val) =
                            Self::ols_from_pairs(&pairs).unwrap_or((f64::NAN, f64::NAN));

                        slope[[i, j, k]] = slope_val;
                        intercept[[i, j, k]] = intercept_val;
                    }
                }
            }
        }

        let direction = Self::compute_direction(&slope);

        info!("Completed linear trend analysis");
        Ok(TrendResult::new(slope, intercept, direction))
    }

    /// Mann-Kendall trend test
    fn mann_kendall(ts: &TimeSeriesRaster) -> Result<TrendResult> {
        if ts.len() < 4 {
            return Err(TemporalError::insufficient_data(
                "Mann-Kendall requires at least 4 observations",
            ));
        }

        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        let mut slope = Array3::zeros((height, width, n_bands));
        let mut intercept = Array3::zeros((height, width, n_bands));
        let mut pvalue = Array3::zeros((height, width, n_bands));

        for i in 0..height {
            for j in 0..width {
                for k in 0..n_bands {
                    let values = ts.extract_pixel_timeseries(i, j, k)?;

                    // Use only valid (non-NaN) observations, preserving each
                    // observation's original time index so temporal spacing is
                    // respected in Sen's slope.
                    let pairs = Self::valid_pairs(&values);
                    let n_valid = pairs.len();

                    // Mann-Kendall needs a meaningful sample; if too few valid
                    // observations survive cloud masking, report undefined
                    // rather than a fabricated significant/insignificant result.
                    if n_valid < 4 {
                        slope[[i, j, k]] = f64::NAN;
                        intercept[[i, j, k]] = f64::NAN;
                        pvalue[[i, j, k]] = f64::NAN;
                        continue;
                    }

                    // Calculate Mann-Kendall S statistic over valid values.
                    let mut s = 0i32;
                    for m in 0..n_valid {
                        for l in (m + 1)..n_valid {
                            s += Self::sign(pairs[l].1 - pairs[m].1);
                        }
                    }

                    // Variance with the standard tie correction.
                    let var_s = Self::mann_kendall_variance(&pairs);
                    let std_s = var_s.sqrt();

                    // Calculate Z-score (continuity-corrected).
                    let z = if std_s <= 0.0 {
                        0.0
                    } else if s > 0 {
                        (s as f64 - 1.0) / std_s
                    } else if s < 0 {
                        (s as f64 + 1.0) / std_s
                    } else {
                        0.0
                    };

                    // Calculate p-value (two-tailed test).
                    let p = 2.0 * (1.0 - Self::normal_cdf(z.abs()));

                    // Sen's slope, using actual time-index differences.
                    let median_slope = Self::median_pairwise_slope(&pairs).unwrap_or(f64::NAN);

                    slope[[i, j, k]] = median_slope;
                    pvalue[[i, j, k]] = p;

                    // Compute intercept from valid pairs.
                    intercept[[i, j, k]] = Self::compute_intercept_pairs(&pairs, median_slope);
                }
            }
        }

        let direction = Self::compute_direction(&slope);

        info!("Completed Mann-Kendall trend analysis");
        Ok(TrendResult::new(slope, intercept, direction).with_pvalue(pvalue))
    }

    /// Sen's slope estimator (robust trend)
    fn sens_slope(ts: &TimeSeriesRaster) -> Result<TrendResult> {
        if ts.len() < 3 {
            return Err(TemporalError::insufficient_data(
                "Need at least 3 observations",
            ));
        }

        let (height, width, n_bands) = ts
            .expected_shape()
            .ok_or_else(|| TemporalError::insufficient_data("No shape information"))?;

        let mut slope = Array3::zeros((height, width, n_bands));
        let mut intercept = Array3::zeros((height, width, n_bands));

        for i in 0..height {
            for j in 0..width {
                for k in 0..n_bands {
                    let values = ts.extract_pixel_timeseries(i, j, k)?;

                    // Use only valid observations, preserving original time
                    // indices so pairwise-slope denominators reflect true
                    // temporal spacing rather than compacted positions.
                    let pairs = Self::valid_pairs(&values);
                    if pairs.len() < 2 {
                        slope[[i, j, k]] = f64::NAN;
                        intercept[[i, j, k]] = f64::NAN;
                        continue;
                    }

                    let median_slope = Self::median_pairwise_slope(&pairs).unwrap_or(f64::NAN);
                    slope[[i, j, k]] = median_slope;

                    // Compute intercept as median of (y - slope * t).
                    intercept[[i, j, k]] = Self::compute_intercept_pairs(&pairs, median_slope);
                }
            }
        }

        let direction = Self::compute_direction(&slope);

        info!("Completed Sen's slope trend analysis");
        Ok(TrendResult::new(slope, intercept, direction))
    }

    /// Compute trend direction from slope
    fn compute_direction(slope: &Array3<f64>) -> Array3<i8> {
        let shape = slope.shape();
        let mut direction = Array3::zeros((shape[0], shape[1], shape[2]));

        for i in 0..shape[0] {
            for j in 0..shape[1] {
                for k in 0..shape[2] {
                    let s = slope[[i, j, k]];
                    direction[[i, j, k]] = if s > 0.0 {
                        1
                    } else if s < 0.0 {
                        -1
                    } else {
                        0
                    };
                }
            }
        }

        direction
    }

    /// Collect `(time_index, value)` pairs, skipping NaN observations.
    ///
    /// The time index is the observation's original position in the series, so
    /// downstream slope estimators respect the true temporal spacing across
    /// masked gaps.
    fn valid_pairs(values: &[f64]) -> Vec<(f64, f64)> {
        values
            .iter()
            .enumerate()
            .filter_map(|(t, &v)| {
                if v.is_nan() {
                    None
                } else {
                    Some((t as f64, v))
                }
            })
            .collect()
    }

    /// Ordinary least-squares regression from valid `(time, value)` pairs.
    ///
    /// Returns `None` when there are fewer than two points or the design is
    /// degenerate (all identical time indices).
    fn ols_from_pairs(pairs: &[(f64, f64)]) -> Option<(f64, f64)> {
        let n = pairs.len();
        if n < 2 {
            return None;
        }
        let n_f = n as f64;
        let sum_t: f64 = pairs.iter().map(|(t, _)| *t).sum();
        let sum_t2: f64 = pairs.iter().map(|(t, _)| t * t).sum();
        let sum_y: f64 = pairs.iter().map(|(_, y)| *y).sum();
        let sum_ty: f64 = pairs.iter().map(|(t, y)| t * y).sum();
        let denom = n_f * sum_t2 - sum_t * sum_t;
        if denom.abs() < f64::EPSILON {
            return None;
        }
        let slope = (n_f * sum_ty - sum_t * sum_y) / denom;
        let intercept = (sum_y - slope * sum_t) / n_f;
        Some((slope, intercept))
    }

    /// Median of all pairwise slopes `(y_l - y_m) / (t_l - t_m)` (Sen's slope).
    ///
    /// Returns `None` if no pair with a non-zero time difference exists.
    fn median_pairwise_slope(pairs: &[(f64, f64)]) -> Option<f64> {
        let mut slopes = Vec::new();
        for m in 0..pairs.len() {
            for l in (m + 1)..pairs.len() {
                let dt = pairs[l].0 - pairs[m].0;
                if dt != 0.0 {
                    slopes.push((pairs[l].1 - pairs[m].1) / dt);
                }
            }
        }
        if slopes.is_empty() {
            return None;
        }
        slopes.sort_by(|a: &f64, b: &f64| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mid = slopes.len() / 2;
        Some(if slopes.len() % 2 == 0 {
            (slopes[mid - 1] + slopes[mid]) / 2.0
        } else {
            slopes[mid]
        })
    }

    /// Mann-Kendall variance of S with the standard tied-value correction
    /// (Kendall 1975 / Gilbert 1987):
    /// `Var(S) = [ n(n-1)(2n+5) - Σ t_p(t_p-1)(2t_p+5) ] / 18`,
    /// summed over each group of `t_p` tied observations.
    fn mann_kendall_variance(pairs: &[(f64, f64)]) -> f64 {
        use std::collections::HashMap;

        let n = pairs.len();
        let base = (n * (n - 1) * (2 * n + 5)) as f64;

        // Group by exact value (bit pattern), normalising -0.0 to 0.0.
        let mut counts: HashMap<u64, usize> = HashMap::new();
        for &(_, v) in pairs {
            let key = if v == 0.0 { 0u64 } else { v.to_bits() };
            *counts.entry(key).or_insert(0) += 1;
        }

        let tie_term: f64 = counts
            .values()
            .filter(|&&t| t > 1)
            .map(|&t| (t * (t - 1) * (2 * t + 5)) as f64)
            .sum();

        ((base - tie_term) / 18.0).max(0.0)
    }

    /// Median intercept `median(y - slope * t)` from valid `(time, value)` pairs.
    fn compute_intercept_pairs(pairs: &[(f64, f64)], slope: f64) -> f64 {
        if pairs.is_empty() {
            return f64::NAN;
        }
        let mut intercepts: Vec<f64> = pairs.iter().map(|(t, y)| y - slope * t).collect();
        intercepts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mid = intercepts.len() / 2;
        if intercepts.len().is_multiple_of(2) {
            (intercepts[mid - 1] + intercepts[mid]) / 2.0
        } else {
            intercepts[mid]
        }
    }

    /// Sign function for Mann-Kendall
    fn sign(x: f64) -> i32 {
        if x > 0.0 {
            1
        } else if x < 0.0 {
            -1
        } else {
            0
        }
    }

    /// Approximate normal CDF
    fn normal_cdf(x: f64) -> f64 {
        0.5 * (1.0 + Self::erf(x / 2.0_f64.sqrt()))
    }

    /// Error function approximation
    fn erf(x: f64) -> f64 {
        // Abramowitz and Stegun approximation
        let a1 = 0.254829592;
        let a2 = -0.284496736;
        let a3 = 1.421413741;
        let a4 = -1.453152027;
        let a5 = 1.061405429;
        let p = 0.3275911;

        let sign = if x < 0.0 { -1.0 } else { 1.0 };
        let x = x.abs();

        let t = 1.0 / (1.0 + p * x);
        let y = 1.0 - (((((a5 * t + a4) * t) + a3) * t + a2) * t + a1) * t * (-x * x).exp();

        sign * y
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::timeseries::{TemporalMetadata, TimeSeriesRaster};
    use chrono::{DateTime, NaiveDate};

    #[test]
    fn test_linear_trend() {
        let mut ts = TimeSeriesRaster::new();

        for i in 0..10 {
            let dt = DateTime::from_timestamp(1640995200 + i * 86400, 0).expect("valid");
            let date = NaiveDate::from_ymd_opt(2022, 1, 1 + i as u32).expect("valid");
            let metadata = TemporalMetadata::new(dt, date);
            let data = Array3::from_elem((5, 5, 1), i as f64);
            ts.add_raster(metadata, data).expect("should add");
        }

        let result = TrendAnalyzer::analyze(&ts, TrendMethod::Linear).expect("should analyze");

        // Slope should be positive (increasing trend)
        assert!(result.slope[[0, 0, 0]] > 0.0);
        assert_eq!(result.direction[[0, 0, 0]], 1);
    }

    #[test]
    fn test_sens_slope() {
        let mut ts = TimeSeriesRaster::new();

        for i in 0..10 {
            let dt = DateTime::from_timestamp(1640995200 + i * 86400, 0).expect("valid");
            let date = NaiveDate::from_ymd_opt(2022, 1, 1 + i as u32).expect("valid");
            let metadata = TemporalMetadata::new(dt, date);
            let data = Array3::from_elem((5, 5, 1), (i * 2) as f64);
            ts.add_raster(metadata, data).expect("should add");
        }

        let result = TrendAnalyzer::analyze(&ts, TrendMethod::SensSlope).expect("should analyze");

        assert!(result.slope[[0, 0, 0]] > 0.0);
        assert_eq!(result.direction[[0, 0, 0]], 1);
    }

    #[test]
    fn test_mann_kendall() {
        let mut ts = TimeSeriesRaster::new();

        for i in 0..10 {
            let dt = DateTime::from_timestamp(1640995200 + i * 86400, 0).expect("valid");
            let date = NaiveDate::from_ymd_opt(2022, 1, 1 + i as u32).expect("valid");
            let metadata = TemporalMetadata::new(dt, date);
            let data = Array3::from_elem((5, 5, 1), (i * i) as f64); // Non-linear trend
            ts.add_raster(metadata, data).expect("should add");
        }

        let result = TrendAnalyzer::analyze(&ts, TrendMethod::MannKendall).expect("should analyze");

        assert!(result.slope[[0, 0, 0]] > 0.0);
        assert_eq!(result.direction[[0, 0, 0]], 1);
        assert!(result.pvalue.is_some());
    }

    /// Build a 1x1x1 time series from a slice of per-timestep values.
    fn ts_from(values: &[f64]) -> TimeSeriesRaster {
        let mut ts = TimeSeriesRaster::new();
        for (i, &v) in values.iter().enumerate() {
            let dt = DateTime::from_timestamp(1640995200 + i as i64 * 86400, 0).expect("valid");
            let date = NaiveDate::from_ymd_opt(2022, 1, 1 + i as u32).expect("valid");
            let metadata = TemporalMetadata::new(dt, date);
            ts.add_raster(metadata, Array3::from_elem((1, 1, 1), v))
                .expect("should add");
        }
        ts
    }

    #[test]
    fn test_linear_trend_ignores_nan_gaps() {
        // A perfectly linear series y = 2t with a single cloud-masked gap.
        // Without NaN filtering the whole pixel would become NaN; with
        // filtering the slope must still recover ~2.0.
        let values = vec![0.0, 2.0, f64::NAN, 6.0, 8.0, 10.0, f64::NAN, 14.0];
        let ts = ts_from(&values);
        let result = TrendAnalyzer::analyze(&ts, TrendMethod::Linear).expect("analyze");
        let slope = result.slope[[0, 0, 0]];
        assert!(slope.is_finite(), "slope must be finite, got {slope}");
        assert!(
            (slope - 2.0).abs() < 1e-9,
            "slope should recover 2.0, got {slope}"
        );
    }

    #[test]
    fn test_sens_slope_ignores_nan_gaps() {
        let values = vec![0.0, 3.0, f64::NAN, 9.0, 12.0, f64::NAN, 18.0];
        let ts = ts_from(&values);
        let result = TrendAnalyzer::analyze(&ts, TrendMethod::SensSlope).expect("analyze");
        let slope = result.slope[[0, 0, 0]];
        assert!(slope.is_finite(), "slope must be finite, got {slope}");
        assert!(
            (slope - 3.0).abs() < 1e-9,
            "slope should recover 3.0, got {slope}"
        );
    }

    #[test]
    fn test_mann_kendall_ignores_nan_gaps() {
        let values = vec![1.0, 2.0, f64::NAN, 4.0, 5.0, 6.0, f64::NAN, 8.0];
        let ts = ts_from(&values);
        let result = TrendAnalyzer::analyze(&ts, TrendMethod::MannKendall).expect("analyze");
        let slope = result.slope[[0, 0, 0]];
        let p = result.pvalue.as_ref().expect("pvalue")[[0, 0, 0]];
        assert!(slope.is_finite(), "slope must be finite, got {slope}");
        assert!(slope > 0.0, "increasing trend expected, got {slope}");
        assert!(p.is_finite(), "p-value must be finite, got {p}");
    }

    #[test]
    fn test_mann_kendall_too_few_valid_is_nan() {
        // Only 3 valid observations survive: MK is undefined (needs >= 4).
        let values = vec![1.0, f64::NAN, f64::NAN, 4.0, f64::NAN, 6.0];
        let ts = ts_from(&values);
        let result = TrendAnalyzer::analyze(&ts, TrendMethod::MannKendall).expect("analyze");
        assert!(result.slope[[0, 0, 0]].is_nan());
        assert!(result.pvalue.as_ref().expect("pvalue")[[0, 0, 0]].is_nan());
    }

    #[test]
    fn test_mann_kendall_variance_tie_correction() {
        // No ties: variance equals the untied formula.
        let distinct: Vec<(f64, f64)> = (0..6).map(|i| (i as f64, i as f64)).collect();
        let n = distinct.len();
        let untied = (n * (n - 1) * (2 * n + 5)) as f64 / 18.0;
        let var_distinct = TrendAnalyzer::mann_kendall_variance(&distinct);
        assert!((var_distinct - untied).abs() < 1e-9);

        // With a tied group of 3 equal values, variance must be strictly lower
        // than the untied formula by the tie-correction term.
        let tied: Vec<(f64, f64)> = vec![
            (0.0, 5.0),
            (1.0, 5.0),
            (2.0, 5.0),
            (3.0, 1.0),
            (4.0, 2.0),
            (5.0, 3.0),
        ];
        let var_tied = TrendAnalyzer::mann_kendall_variance(&tied);
        // Expected tie term for t_p = 3: 3*2*11 = 66.
        let expected = (untied * 18.0 - 66.0) / 18.0;
        assert!(
            (var_tied - expected).abs() < 1e-9,
            "tie-corrected variance mismatch: got {var_tied}, expected {expected}"
        );
        assert!(var_tied < untied, "tie correction must reduce variance");
    }
}
