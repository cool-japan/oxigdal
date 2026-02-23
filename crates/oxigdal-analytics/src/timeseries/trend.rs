//! Trend Detection for Time Series
//!
//! This module provides various trend detection methods including:
//! - Mann-Kendall test (non-parametric trend test)
//! - Linear regression trend
//! - Seasonal decomposition

use crate::error::{AnalyticsError, Result};
use scirs2_core::ndarray::{Array1, ArrayView1};

/// Trend detection methods
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrendMethod {
    /// Mann-Kendall non-parametric trend test
    MannKendall,
    /// Linear regression
    LinearRegression,
    /// Seasonal trend decomposition
    SeasonalDecomposition,
}

/// Result of trend detection
#[derive(Debug, Clone)]
pub struct TrendResult {
    /// Trend direction: positive (1), negative (-1), or no trend (0)
    pub direction: i8,
    /// Statistical significance (p-value)
    pub p_value: f64,
    /// Trend magnitude (slope or Kendall's tau)
    pub magnitude: f64,
    /// Confidence level (typically 0.05 or 0.01)
    pub confidence: f64,
    /// Whether trend is statistically significant
    pub significant: bool,
}

/// Trend detector for time series analysis
pub struct TrendDetector {
    method: TrendMethod,
    confidence: f64,
}

impl TrendDetector {
    /// Create a new trend detector
    ///
    /// # Arguments
    /// * `method` - Trend detection method
    /// * `confidence` - Confidence level for significance testing (e.g., 0.05 for 95%)
    pub fn new(method: TrendMethod, confidence: f64) -> Self {
        Self { method, confidence }
    }

    /// Detect trend in time series
    ///
    /// # Arguments
    /// * `values` - Time series values
    ///
    /// # Errors
    /// Returns error if computation fails or insufficient data
    pub fn detect(&self, values: &ArrayView1<f64>) -> Result<TrendResult> {
        match self.method {
            TrendMethod::MannKendall => self.mann_kendall(values),
            TrendMethod::LinearRegression => self.linear_regression(values),
            TrendMethod::SeasonalDecomposition => Err(AnalyticsError::time_series_error(
                "Seasonal decomposition not yet implemented",
            )),
        }
    }

    /// Mann-Kendall trend test
    ///
    /// Non-parametric test for monotonic trend detection.
    /// Null hypothesis: no trend
    /// Alternative: monotonic trend exists
    fn mann_kendall(&self, values: &ArrayView1<f64>) -> Result<TrendResult> {
        let n = values.len();
        if n < 3 {
            return Err(AnalyticsError::insufficient_data(
                "Mann-Kendall test requires at least 3 data points",
            ));
        }

        // Calculate S statistic
        let mut s = 0i64;
        for i in 0..n - 1 {
            for j in (i + 1)..n {
                let diff = values[j] - values[i];
                // Note: f64::signum() returns 1.0 for 0.0, so we need to check explicitly
                if diff.abs() > f64::EPSILON {
                    s += diff.signum() as i64;
                }
                // If diff is ~0, add nothing (no contribution to trend)
            }
        }

        // Calculate variance
        let n_f64 = n as f64;
        let var_s = (n_f64 * (n_f64 - 1.0) * (2.0 * n_f64 + 5.0)) / 18.0;

        // Calculate standardized test statistic Z
        let z = if s > 0 {
            ((s - 1) as f64) / var_s.sqrt()
        } else if s < 0 {
            ((s + 1) as f64) / var_s.sqrt()
        } else {
            0.0
        };

        // Calculate p-value (two-tailed test)
        let p_value = 2.0 * (1.0 - standard_normal_cdf(z.abs()));

        // Calculate Kendall's tau
        let tau = (2.0 * s as f64) / (n_f64 * (n_f64 - 1.0));

        Ok(TrendResult {
            direction: s.signum() as i8,
            p_value,
            magnitude: tau,
            confidence: self.confidence,
            significant: p_value < self.confidence,
        })
    }

    /// Linear regression trend
    ///
    /// Fits a linear trend line y = ax + b
    fn linear_regression(&self, values: &ArrayView1<f64>) -> Result<TrendResult> {
        let n = values.len();
        if n < 2 {
            return Err(AnalyticsError::insufficient_data(
                "Linear regression requires at least 2 data points",
            ));
        }

        // Create time indices
        let x: Vec<f64> = (0..n).map(|i| i as f64).collect();

        // Calculate means
        let x_mean = x.iter().sum::<f64>() / (n as f64);
        let y_mean = values.sum() / (n as f64);

        // Calculate slope and intercept
        let mut numerator = 0.0;
        let mut denominator = 0.0;

        for i in 0..n {
            let x_diff = x[i] - x_mean;
            let y_diff = values[i] - y_mean;
            numerator += x_diff * y_diff;
            denominator += x_diff * x_diff;
        }

        if denominator.abs() < f64::EPSILON {
            return Err(AnalyticsError::numerical_instability(
                "Cannot compute slope: zero denominator",
            ));
        }

        let slope = numerator / denominator;

        // Calculate residuals and standard error
        let intercept = y_mean - slope * x_mean;
        let mut ss_res = 0.0;
        let mut ss_tot = 0.0;

        for i in 0..n {
            let y_pred = slope * x[i] + intercept;
            let residual = values[i] - y_pred;
            ss_res += residual * residual;
            ss_tot += (values[i] - y_mean) * (values[i] - y_mean);
        }

        // Calculate R-squared
        let _r_squared = if ss_tot > f64::EPSILON {
            1.0 - (ss_res / ss_tot)
        } else {
            0.0
        };

        // Calculate standard error of slope
        let se = if n > 2 {
            (ss_res / ((n - 2) as f64) / denominator).sqrt()
        } else {
            f64::INFINITY
        };

        // Calculate t-statistic
        let t_stat = if se.is_finite() && se > f64::EPSILON {
            slope / se
        } else {
            0.0
        };

        // Approximate p-value using t-distribution (simplified)
        // For production use, should use proper t-distribution CDF
        let df = (n - 2) as f64;
        let p_value = if df > 0.0 {
            2.0 * (1.0 - standard_normal_cdf(t_stat.abs()))
        } else {
            1.0
        };

        Ok(TrendResult {
            direction: slope.signum() as i8,
            p_value,
            magnitude: slope,
            confidence: self.confidence,
            significant: p_value < self.confidence,
        })
    }
}

/// Standard normal cumulative distribution function
///
/// Approximation using the error function
fn standard_normal_cdf(x: f64) -> f64 {
    0.5 * (1.0 + erf(x / 2_f64.sqrt()))
}

/// Error function approximation
///
/// Uses Abramowitz and Stegun approximation (maximum error: 1.5e-7)
fn erf(x: f64) -> f64 {
    let sign = x.signum();
    let x = x.abs();

    // Constants
    let a1 = 0.254_829_592;
    let a2 = -0.284_496_736;
    let a3 = 1.421_413_741;
    let a4 = -1.453_152_027;
    let a5 = 1.061_405_429;
    let p = 0.327_591_100;

    let t = 1.0 / (1.0 + p * x);
    let t2 = t * t;
    let t3 = t2 * t;
    let t4 = t3 * t;
    let t5 = t4 * t;

    let result = 1.0 - (a1 * t + a2 * t2 + a3 * t3 + a4 * t4 + a5 * t5) * (-x * x).exp();

    sign * result
}

/// Seasonal decomposition result
#[derive(Debug, Clone)]
pub struct SeasonalDecomposition {
    /// Trend component
    pub trend: Array1<f64>,
    /// Seasonal component
    pub seasonal: Array1<f64>,
    /// Residual component
    pub residual: Array1<f64>,
}

/// Perform seasonal decomposition
///
/// # Arguments
/// * `values` - Time series values
/// * `period` - Period of seasonality
///
/// # Errors
/// Returns error if computation fails
pub fn seasonal_decompose(
    values: &ArrayView1<f64>,
    period: usize,
) -> Result<SeasonalDecomposition> {
    let n = values.len();
    if n < 2 * period {
        return Err(AnalyticsError::insufficient_data(format!(
            "Need at least {} data points for period {}",
            2 * period,
            period
        )));
    }

    // Calculate trend using centered moving average
    let mut trend = Array1::zeros(n);
    let half_window = period / 2;

    for i in half_window..(n - half_window) {
        let start = i - half_window;
        let end = i + half_window + 1;
        let window = values.slice(s![start..end]);
        trend[i] = window.sum() / (period as f64);
    }

    // Fill edges with simple extrapolation
    for i in 0..half_window {
        trend[i] = trend[half_window];
    }
    for i in (n - half_window)..n {
        trend[i] = trend[n - half_window - 1];
    }

    // Calculate detrended series
    let detrended = values - &trend;

    // Calculate seasonal component (average for each season)
    let mut seasonal = Array1::zeros(n);
    let mut season_sums = vec![0.0; period];
    let mut season_counts = vec![0; period];

    for (i, &value) in detrended.iter().enumerate() {
        let season_idx = i % period;
        season_sums[season_idx] += value;
        season_counts[season_idx] += 1;
    }

    // Average seasonal components
    let season_avgs: Vec<f64> = season_sums
        .iter()
        .zip(season_counts.iter())
        .map(|(sum, count)| {
            if *count > 0 {
                sum / (*count as f64)
            } else {
                0.0
            }
        })
        .collect();

    // Normalize seasonal component (sum to zero)
    let season_mean = season_avgs.iter().sum::<f64>() / (period as f64);
    let season_normalized: Vec<f64> = season_avgs.iter().map(|x| x - season_mean).collect();

    // Apply seasonal component
    for (i, value) in seasonal.iter_mut().enumerate() {
        *value = season_normalized[i % period];
    }

    // Calculate residuals
    let residual = values - &trend - &seasonal;

    Ok(SeasonalDecomposition {
        trend,
        seasonal,
        residual,
    })
}

// Import slice macro for ndarray
use scirs2_core::ndarray::s;

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;
    use scirs2_core::ndarray::array;

    #[test]
    fn test_mann_kendall_positive_trend() {
        let values = array![1.0, 2.0, 3.0, 4.0, 5.0];
        let detector = TrendDetector::new(TrendMethod::MannKendall, 0.05);
        let result = detector
            .detect(&values.view())
            .expect("Mann-Kendall detection should succeed for valid data");

        assert_eq!(result.direction, 1);
        assert!(result.p_value < 0.05);
        assert!(result.significant);
    }

    #[test]
    fn test_mann_kendall_negative_trend() {
        let values = array![5.0, 4.0, 3.0, 2.0, 1.0];
        let detector = TrendDetector::new(TrendMethod::MannKendall, 0.05);
        let result = detector
            .detect(&values.view())
            .expect("Mann-Kendall detection should succeed for negative trend");

        assert_eq!(result.direction, -1);
        assert!(result.p_value < 0.05);
        assert!(result.significant);
    }

    #[test]
    fn test_mann_kendall_no_trend() {
        let values = array![1.0, 1.0, 1.0, 1.0, 1.0];
        let detector = TrendDetector::new(TrendMethod::MannKendall, 0.05);
        let result = detector
            .detect(&values.view())
            .expect("Mann-Kendall detection should succeed for no trend data");

        assert_eq!(result.direction, 0);
        assert!(!result.significant);
    }

    #[test]
    fn test_linear_regression() {
        let values = array![1.0, 2.0, 3.0, 4.0, 5.0];
        let detector = TrendDetector::new(TrendMethod::LinearRegression, 0.05);
        let result = detector
            .detect(&values.view())
            .expect("Linear regression should succeed for valid data");

        assert_eq!(result.direction, 1);
        assert_abs_diff_eq!(result.magnitude, 1.0, epsilon = 1e-10);
    }

    #[test]
    fn test_seasonal_decompose() {
        // Create synthetic seasonal data
        let n = 24;
        let period = 6;
        let mut values = Array1::zeros(n);
        for i in 0..n {
            // Trend + seasonal component
            values[i] = (i as f64) + ((i % period) as f64);
        }

        let result = seasonal_decompose(&values.view(), period)
            .expect("Seasonal decomposition should succeed for valid data");
        assert_eq!(result.trend.len(), n);
        assert_eq!(result.seasonal.len(), n);
        assert_eq!(result.residual.len(), n);
    }

    #[test]
    fn test_standard_normal_cdf() {
        // Test known values
        assert_abs_diff_eq!(standard_normal_cdf(0.0), 0.5, epsilon = 1e-6);
        assert!(standard_normal_cdf(1.96) > 0.975);
        assert!(standard_normal_cdf(-1.96) < 0.025);
    }
}
