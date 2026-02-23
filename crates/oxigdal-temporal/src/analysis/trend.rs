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

        // Collect time indices
        let times: Vec<f64> = (0..ts.len()).map(|i| i as f64).collect();
        let n = times.len() as f64;
        let sum_t: f64 = times.iter().sum();
        let sum_t2: f64 = times.iter().map(|&t| t * t).sum();

        // Compute OLS for each pixel
        #[cfg(feature = "parallel")]
        {
            use std::sync::Mutex;
            let slope_mutex = Mutex::new(&mut slope);
            let intercept_mutex = Mutex::new(&mut intercept);

            (0..height).into_par_iter().for_each(|i| {
                for j in 0..width {
                    for k in 0..n_bands {
                        let values = ts.extract_pixel_timeseries(i, j, k).ok();
                        if let Some(values) = values {
                            let sum_y: f64 = values.iter().copied().sum();
                            let sum_ty: f64 =
                                times.iter().zip(values.iter()).map(|(t, y)| t * y).sum();

                            let slope_val =
                                (n * sum_ty - sum_t * sum_y) / (n * sum_t2 - sum_t * sum_t);
                            let intercept_val = (sum_y - slope_val * sum_t) / n;

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
                        let sum_y: f64 = values.iter().sum();
                        let sum_ty: f64 = times.iter().zip(values.iter()).map(|(t, y)| t * y).sum();

                        let slope_val = (n * sum_ty - sum_t * sum_y) / (n * sum_t2 - sum_t * sum_t);
                        let intercept_val = (sum_y - slope_val * sum_t) / n;

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

        let n = ts.len();

        for i in 0..height {
            for j in 0..width {
                for k in 0..n_bands {
                    let values = ts.extract_pixel_timeseries(i, j, k)?;

                    // Calculate Mann-Kendall S statistic
                    let mut s = 0i32;
                    for m in 0..n {
                        for l in (m + 1)..n {
                            s += Self::sign(values[l] - values[m]);
                        }
                    }

                    // Calculate variance
                    let var_s = (n * (n - 1) * (2 * n + 5)) as f64 / 18.0;

                    // Calculate Z-score
                    let z = if s > 0 {
                        (s as f64 - 1.0) / var_s.sqrt()
                    } else if s < 0 {
                        (s as f64 + 1.0) / var_s.sqrt()
                    } else {
                        0.0
                    };

                    // Calculate p-value (two-tailed test)
                    let p = 2.0 * (1.0 - Self::normal_cdf(z.abs()));

                    // Calculate Sen's slope for magnitude
                    let mut slopes = Vec::new();
                    for m in 0..n {
                        for l in (m + 1)..n {
                            if l != m {
                                slopes.push((values[l] - values[m]) / ((l - m) as f64));
                            }
                        }
                    }
                    slopes.sort_by(|a: &f64, b: &f64| {
                        a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
                    });
                    let median_slope = if slopes.len() % 2 == 0 {
                        (slopes[slopes.len() / 2 - 1] + slopes[slopes.len() / 2]) / 2.0
                    } else {
                        slopes[slopes.len() / 2]
                    };

                    slope[[i, j, k]] = median_slope;
                    pvalue[[i, j, k]] = p;

                    // Compute intercept
                    let median_intercept = Self::compute_intercept(&values, median_slope);
                    intercept[[i, j, k]] = median_intercept;
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

                    // Compute all pairwise slopes
                    let mut slopes = Vec::new();
                    for m in 0..values.len() {
                        for n in (m + 1)..values.len() {
                            let slope_mn = (values[n] - values[m]) / ((n - m) as f64);
                            slopes.push(slope_mn);
                        }
                    }

                    // Median slope
                    slopes.sort_by(|a: &f64, b: &f64| {
                        a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
                    });
                    let median_slope = if slopes.len() % 2 == 0 {
                        (slopes[slopes.len() / 2 - 1] + slopes[slopes.len() / 2]) / 2.0
                    } else {
                        slopes[slopes.len() / 2]
                    };

                    slope[[i, j, k]] = median_slope;

                    // Compute intercept as median of (y - slope * x)
                    let median_intercept = Self::compute_intercept(&values, median_slope);
                    intercept[[i, j, k]] = median_intercept;
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

    /// Compute intercept from values and slope
    fn compute_intercept(values: &[f64], slope: f64) -> f64 {
        let mut intercepts: Vec<f64> = values
            .iter()
            .enumerate()
            .map(|(idx, &y)| y - slope * idx as f64)
            .collect();

        intercepts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        if intercepts.len() % 2 == 0 {
            (intercepts[intercepts.len() / 2 - 1] + intercepts[intercepts.len() / 2]) / 2.0
        } else {
            intercepts[intercepts.len() / 2]
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
}
