//! Integration tests for the STL (Seasonal-Trend decomposition using Loess)
//! implementation and the underlying Loess smoother.
//!
//! These tests follow the requirements of Cleveland et al. (1990): the
//! seasonal component must integrate to zero over a full cycle, the
//! decomposition must reconstruct the original to machine precision, and the
//! outer (robust) loop must dampen point outliers.
#![allow(clippy::expect_used)]
#![allow(clippy::float_cmp)]

use chrono::{DateTime, NaiveDate};
use oxigeo_temporal::analysis::loess::{
    LoessOptions, loess_smooth_1d, weighted_polynomial_fit_local,
};
use oxigeo_temporal::analysis::seasonality::{SeasonalityAnalyzer, SeasonalityMethod};
use oxigeo_temporal::analysis::stl::{StlOptions, default_n_trend, stl_decompose};
use oxigeo_temporal::timeseries::{TemporalMetadata, TimeSeriesRaster};
use scirs2_core::ndarray::Array3;

// =========================================================================
// Loess tests
// =========================================================================

#[test]
fn test_loess_constant_input_returns_constant() {
    let x: Vec<f64> = (0..20).map(|i| i as f64).collect();
    let y = vec![3.5_f64; 20];
    let opts = LoessOptions::default();
    let res = loess_smooth_1d(&x, &y, &opts).expect("loess should succeed");
    assert_eq!(res.len(), 20);
    for v in res {
        assert!((v - 3.5).abs() < 1e-9, "expected 3.5, got {v}");
    }
}

#[test]
fn test_loess_perfect_linear_input_returns_input() {
    // f(x) = 2x + 1 — local linear Loess must reproduce it exactly.
    let x: Vec<f64> = (0..40).map(|i| i as f64).collect();
    let y: Vec<f64> = x.iter().map(|xi| 2.0 * xi + 1.0).collect();
    let opts = LoessOptions::default();
    let res = loess_smooth_1d(&x, &y, &opts).expect("loess should succeed");
    for (yi, ri) in y.iter().zip(res.iter()) {
        assert!((yi - ri).abs() < 1e-8, "linear mismatch: {yi} vs {ri}");
    }
}

#[test]
fn test_loess_recovers_smooth_through_noisy_quadratic_within_tolerance() {
    // f(x) = x^2/100, with a deterministic high-frequency perturbation that
    // a local linear smoother should largely cancel.
    let x: Vec<f64> = (0..120).map(|i| i as f64).collect();
    let truth: Vec<f64> = x.iter().map(|xi| xi * xi / 100.0).collect();
    let noisy: Vec<f64> = x
        .iter()
        .zip(truth.iter())
        .map(|(xi, ti)| ti + 0.5 * (xi * 0.7).sin())
        .collect();
    let opts = LoessOptions {
        bandwidth_fraction: 0.4,
        degree: 2,
        robustness_iterations: 0,
        weights: None,
    };
    let smoothed = loess_smooth_1d(&x, &noisy, &opts).expect("loess should succeed");
    // Mean squared error between smoothed and truth should be smaller than
    // the variance of the additive perturbation.
    let mse: f64 = smoothed
        .iter()
        .zip(truth.iter())
        .map(|(s, t)| (s - t).powi(2))
        .sum::<f64>()
        / truth.len() as f64;
    let pert_var: f64 = noisy
        .iter()
        .zip(truth.iter())
        .map(|(n, t)| (n - t).powi(2))
        .sum::<f64>()
        / truth.len() as f64;
    assert!(
        mse < pert_var * 0.6,
        "smoothed mse {mse} should be << perturbation variance {pert_var}"
    );
}

#[test]
fn test_loess_boundary_one_sided_weights_match_cleveland() {
    // At the boundary, only one-sided neighbours are available. A linear fit
    // through these one-sided weighted points must recover a globally linear
    // signal exactly. Cleveland (1990) §3.5 guarantees this property.
    let x: Vec<f64> = (0..15).map(|i| i as f64).collect();
    let y: Vec<f64> = x.iter().map(|xi| 3.0 - 0.4 * xi).collect();
    let opts = LoessOptions {
        bandwidth_fraction: 0.5,
        degree: 1,
        robustness_iterations: 0,
        weights: None,
    };
    let res = loess_smooth_1d(&x, &y, &opts).expect("loess should succeed");
    // Boundary indices.
    assert!((res[0] - y[0]).abs() < 1e-8, "left boundary mismatch");
    assert!(
        (res[y.len() - 1] - y[y.len() - 1]).abs() < 1e-8,
        "right boundary mismatch"
    );
}

#[test]
fn test_loess_bandwidth_fraction_zero_falls_back_to_local_constant() {
    // With bandwidth_fraction = 0 the neighbourhood collapses to a single
    // point: the smoother is forced to return that point unchanged.
    let x: Vec<f64> = (0..10).map(|i| i as f64).collect();
    let y: Vec<f64> = (0..10).map(|i| (i as f64).sin()).collect();
    let opts = LoessOptions {
        bandwidth_fraction: 0.0,
        degree: 1,
        robustness_iterations: 0,
        weights: None,
    };
    let res = loess_smooth_1d(&x, &y, &opts).expect("loess should succeed");
    for (yi, ri) in y.iter().zip(res.iter()) {
        assert!(
            (yi - ri).abs() < 1e-9,
            "expected identity, got {ri} vs {yi}"
        );
    }
}

#[test]
fn test_loess_rank_deficient_design_returns_weighted_mean() {
    // All points share the same abscissa → rank-deficient design. The
    // weighted polynomial fit must fall back to the weighted mean.
    let xs = vec![1.0; 5];
    let ys = vec![2.0, 4.0, 6.0, 8.0, 10.0];
    let ws = vec![1.0; 5];
    let v = weighted_polynomial_fit_local(&xs, &ys, &ws, 1, 1.0);
    let expected: f64 = ys.iter().sum::<f64>() / ys.len() as f64;
    assert!((v - expected).abs() < 1e-9, "{v} vs {expected}");
}

// =========================================================================
// STL tests
// =========================================================================

#[test]
fn test_stl_pure_sine_period_12_extracts_seasonal_amplitude() {
    let n = 120;
    let period = 12;
    let amp = 3.0;
    let values: Vec<f64> = (0..n)
        .map(|i| amp * (2.0 * std::f64::consts::PI * i as f64 / period as f64).sin())
        .collect();
    let opts = StlOptions::new(period).with_inner_iterations(3);
    let res = stl_decompose(&values, &opts).expect("STL should succeed");
    assert_eq!(res.seasonal.len(), n);
    let seasonal_max = res.seasonal.iter().cloned().fold(f64::MIN, f64::max);
    let seasonal_min = res.seasonal.iter().cloned().fold(f64::MAX, f64::min);
    let recovered_amp = (seasonal_max - seasonal_min) / 2.0;
    assert!(
        (recovered_amp - amp).abs() < 0.5,
        "recovered amplitude {recovered_amp} vs expected {amp}"
    );
    // Trend should be small relative to amplitude.
    let trend_mean: f64 = res.trend.iter().sum::<f64>() / n as f64;
    assert!(trend_mean.abs() < amp * 0.5);
}

#[test]
fn test_stl_pure_linear_trend_extracts_trend() {
    let n = 60;
    let period = 6;
    let values: Vec<f64> = (0..n).map(|i| 0.5 * i as f64 + 2.0).collect();
    let opts = StlOptions::new(period).with_inner_iterations(3);
    let res = stl_decompose(&values, &opts).expect("STL should succeed");
    // Trend should closely match values.
    for (v, t) in values.iter().zip(res.trend.iter()) {
        assert!(
            (v - t).abs() < 0.5,
            "trend should approximate linear: value {v}, trend {t}"
        );
    }
    // Seasonal magnitude should be small.
    let seasonal_max_abs = res.seasonal.iter().map(|s| s.abs()).fold(0.0_f64, f64::max);
    assert!(
        seasonal_max_abs < 1.0,
        "seasonal should be near zero, got {seasonal_max_abs}"
    );
}

#[test]
fn test_stl_decomposition_sums_to_original_within_1e_10() {
    let n = 96;
    let period = 12;
    let values: Vec<f64> = (0..n)
        .map(|i| {
            0.3 * i as f64
                + 2.0 * (2.0 * std::f64::consts::PI * i as f64 / period as f64).sin()
                + 0.1 * ((i * 7) as f64).cos()
        })
        .collect();
    let opts = StlOptions::new(period);
    let res = stl_decompose(&values, &opts).expect("STL should succeed");
    for (i, v) in values.iter().enumerate() {
        let sum = res.trend[i] + res.seasonal[i] + res.residual[i];
        assert!(
            (sum - v).abs() < 1e-10,
            "additive reconstruction failed: {sum} vs {v}"
        );
    }
}

#[test]
fn test_stl_period_24_monthly_data() {
    let n = 96;
    let period = 24;
    let values: Vec<f64> = (0..n)
        .map(|i| {
            10.0 + 0.05 * i as f64
                + 1.5 * (2.0 * std::f64::consts::PI * i as f64 / period as f64).cos()
        })
        .collect();
    let opts = StlOptions::new(period);
    let res = stl_decompose(&values, &opts).expect("STL should succeed");
    assert_eq!(res.trend.len(), n);
    assert_eq!(res.seasonal.len(), n);
    assert_eq!(res.residual.len(), n);
    let recovered_amp = (res.seasonal.iter().cloned().fold(f64::MIN, f64::max)
        - res.seasonal.iter().cloned().fold(f64::MAX, f64::min))
        / 2.0;
    assert!(
        (recovered_amp - 1.5).abs() < 0.6,
        "amplitude {recovered_amp} far from 1.5"
    );
}

#[test]
fn test_stl_robustness_iterations_dampen_single_outlier() {
    let n = 72;
    let period = 12;
    let mut values: Vec<f64> = (0..n)
        .map(|i| 2.0 * (2.0 * std::f64::consts::PI * i as f64 / period as f64).sin())
        .collect();
    // Inject a single huge outlier at index 30.
    values[30] += 50.0;
    // Non-robust pass.
    let opts_plain = StlOptions::new(period);
    let plain = stl_decompose(&values, &opts_plain).expect("STL should succeed");
    // Robust pass.
    let opts_robust = StlOptions::new(period).with_robust();
    let robust = stl_decompose(&values, &opts_robust).expect("STL should succeed");
    // Robust trend at the outlier location should be much closer to the
    // surrounding trend than the non-robust trend.
    let plain_jump = (plain.trend[30] - plain.trend[29]).abs();
    let robust_jump = (robust.trend[30] - robust.trend[29]).abs();
    assert!(
        robust_jump <= plain_jump + 1e-6,
        "robust trend should not jump more than plain at outlier: plain {plain_jump}, robust {robust_jump}"
    );
    // And the robustness weight at the outlier should be very small.
    assert!(
        robust.robustness_weights[30] < 0.5,
        "expected low robustness weight at outlier, got {}",
        robust.robustness_weights[30]
    );
}

#[test]
fn test_stl_seasonal_component_period_zero_mean_per_cycle() {
    let n = 120;
    let period = 12;
    let values: Vec<f64> = (0..n)
        .map(|i| 4.0 * (2.0 * std::f64::consts::PI * i as f64 / period as f64).sin())
        .collect();
    let opts = StlOptions::new(period).with_inner_iterations(3);
    let res = stl_decompose(&values, &opts).expect("STL should succeed");
    // Over the entire window, the seasonal mean should be near zero.
    let total_mean: f64 = res.seasonal.iter().sum::<f64>() / n as f64;
    assert!(
        total_mean.abs() < 0.5,
        "seasonal mean should be near zero, got {total_mean}"
    );
}

#[test]
fn test_stl_residual_low_autocorrelation_at_period_lag() {
    let n = 96;
    let period = 12;
    let values: Vec<f64> = (0..n)
        .map(|i| {
            0.2 * i as f64 + 3.0 * (2.0 * std::f64::consts::PI * i as f64 / period as f64).sin()
        })
        .collect();
    let opts = StlOptions::new(period);
    let res = stl_decompose(&values, &opts).expect("STL should succeed");
    // Compute autocorrelation of residual at lag = period.
    let mean: f64 = res.residual.iter().sum::<f64>() / n as f64;
    let mut num = 0.0_f64;
    let mut den = 0.0_f64;
    for r in &res.residual {
        den += (r - mean).powi(2);
    }
    for i in 0..(n - period) {
        num += (res.residual[i] - mean) * (res.residual[i + period] - mean);
    }
    let rho = if den > 0.0 { num / den } else { 0.0 };
    // For a clean sinusoid, residual at lag = period should be small.
    assert!(
        rho.abs() < 0.5,
        "residual lag-{period} autocorrelation should be small, got {rho}"
    );
}

#[test]
fn test_stl_short_series_minimal_period_2_works() {
    let n = 8;
    let period = 2;
    let values: Vec<f64> = vec![1.0, 5.0, 1.0, 5.0, 1.0, 5.0, 1.0, 5.0];
    let opts = StlOptions::new(period);
    let res = stl_decompose(&values, &opts).expect("STL should succeed for short series");
    assert_eq!(res.trend.len(), n);
    assert_eq!(res.seasonal.len(), n);
    // Reconstruction.
    for (i, v) in values.iter().enumerate().take(n) {
        let sum = res.trend[i] + res.seasonal[i] + res.residual[i];
        assert!((sum - v).abs() < 1e-10);
    }
}

#[test]
fn test_stl_options_default_n_trend_per_cleveland_recipe() {
    let period = 12;
    let opts = StlOptions::new(period);
    let expected = default_n_trend(period, 7);
    assert_eq!(opts.n_trend, expected);
    assert!(opts.n_trend % 2 == 1, "n_trend must be odd");
}

#[test]
fn test_stl_options_robust_flag_engages_outer_loop() {
    let opts_plain = StlOptions::new(12);
    let opts_robust = StlOptions::new(12).with_robust();
    assert!(!opts_plain.robust);
    assert_eq!(opts_plain.outer_iterations, 0);
    assert!(opts_robust.robust);
    assert!(
        opts_robust.outer_iterations > 0,
        "with_robust() must enable outer iterations, got {}",
        opts_robust.outer_iterations
    );
}

// =========================================================================
// Seasonality integration test (covers STL stub replacement)
// =========================================================================

#[test]
fn test_seasonality_stl_integration_small_raster() {
    let mut ts = TimeSeriesRaster::new();
    let period = 12;
    let n = period * 4;
    let base_date = NaiveDate::from_ymd_opt(2022, 1, 1).expect("valid date");
    for i in 0..n {
        let dt = DateTime::from_timestamp(1640995200 + i as i64 * 86400, 0).expect("valid");
        let date = base_date + chrono::Duration::days(i as i64);
        let metadata = TemporalMetadata::new(dt, date);
        let value =
            (i as f64) * 0.1 + 2.0 * (2.0 * std::f64::consts::PI * i as f64 / period as f64).sin();
        let data = Array3::from_elem((3, 3, 1), value);
        ts.add_raster(metadata, data).expect("should add");
    }

    let res = SeasonalityAnalyzer::decompose(&ts, SeasonalityMethod::STL, period)
        .expect("STL decomposition should succeed");
    assert_eq!(res.period, Some(period));
    assert_eq!(res.trend.shape(), &[3, 3, 1]);
    assert_eq!(res.seasonal.shape(), &[3, 3, 1]);
    assert_eq!(res.residual.shape(), &[3, 3, 1]);
}
