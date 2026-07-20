//! Integration tests for `GapFillMethod::SplineInterpolation` and
//! `GapFillMethod::HarmonicRegression`.
//!
//! Regression coverage for two production-hardening fixes:
//!
//! 1. `SplineInterpolation` used to silently alias `LinearInterpolation`
//!    (identical output). It now fits a real natural cubic spline through the
//!    valid anchor points, so its output must diverge from a straight-line
//!    fill on curved signals.
//! 2. `HarmonicRegression`'s `fit_harmonic` used to compute the sin/cos
//!    coefficients via independent marginal regressions, which is biased
//!    whenever the valid samples are not exactly orthogonal over a full
//!    period (e.g. irregularly-gappy series). It now solves the proper 3x3
//!    normal-equation system, so the fitted curve must closely reconstruct a
//!    known harmonic signal even when sampled with irregular gaps.

#![allow(clippy::expect_used)]

use chrono::{DateTime, NaiveDate};
use oxigdal_temporal::{
    gap_filling::{GapFillMethod, GapFillParams, GapFiller},
    timeseries::{TemporalMetadata, TimeSeriesRaster},
};
use scirs2_core::ndarray::Array3;
use std::f64::consts::PI;

/// Create a `TimeSeriesRaster` from a slice of 1-D signal values.
///
/// Each element becomes a separate time step. The raster is 1×1 with 1 band,
/// so `values[t]` maps exactly to the pixel time series at (0, 0, 0).
fn ts_from_signal(values: &[f64]) -> TimeSeriesRaster {
    let mut ts = TimeSeriesRaster::new();
    for (i, &v) in values.iter().enumerate() {
        let dt = DateTime::from_timestamp(1_640_995_200 + i as i64 * 86_400, 0)
            .expect("valid timestamp");
        let date = NaiveDate::from_ymd_opt(2022, 1, 1).expect("valid date");
        let meta = TemporalMetadata::new(dt, date);
        let mut data = Array3::<f64>::zeros((1, 1, 1));
        data[[0, 0, 0]] = v;
        ts.add_raster(meta, data).expect("should add");
    }
    ts
}

/// Extract the per-pixel (0, 0, 0) time series from a `TimeSeriesRaster`.
fn extract_signal(ts: &TimeSeriesRaster) -> Vec<f64> {
    ts.extract_pixel_timeseries(0, 0, 0)
        .expect("should extract")
}

// ─────────────────────────────────────────────────────────────────────────────
// Spline interpolation
// ─────────────────────────────────────────────────────────────────────────────

/// The spline fill must differ from the linear fill on a curved (quadratic)
/// signal with an interior gap — a plain alias to linear interpolation would
/// produce byte-for-byte identical output, which this test forbids.
#[test]
fn test_spline_differs_from_linear_on_curved_signal() {
    let full: Vec<f64> = (0..15).map(|i| (i as f64 - 7.0).powi(2)).collect();
    let mut gappy = full.clone();
    for v in gappy.iter_mut().take(9).skip(5) {
        *v = f64::NAN;
    }

    let spline_filled = GapFiller::fill_gaps(
        &ts_from_signal(&gappy),
        GapFillMethod::SplineInterpolation,
        None,
    )
    .expect("spline fill should succeed");
    let linear_filled = GapFiller::fill_gaps(
        &ts_from_signal(&gappy),
        GapFillMethod::LinearInterpolation,
        None,
    )
    .expect("linear fill should succeed");

    let spline_signal = extract_signal(&spline_filled);
    let linear_signal = extract_signal(&linear_filled);

    let mut max_diff = 0.0_f64;
    for idx in 5..9 {
        max_diff = max_diff.max((spline_signal[idx] - linear_signal[idx]).abs());
    }
    assert!(
        max_diff > 0.5,
        "spline output should visibly differ from linear output on a curved signal, max_diff={max_diff}"
    );
}

/// A cubic spline should reconstruct a quadratic signal (which is itself a
/// degree-2 polynomial, well within cubic capacity) far more accurately than
/// piecewise-linear interpolation across a multi-point gap.
#[test]
fn test_spline_reconstructs_quadratic_more_accurately_than_linear() {
    let full: Vec<f64> = (0..15)
        .map(|i| 0.5 * (i as f64 - 7.0).powi(2) + 3.0)
        .collect();
    let mut gappy = full.clone();
    for v in gappy.iter_mut().take(9).skip(5) {
        *v = f64::NAN;
    }

    let spline_filled = GapFiller::fill_gaps(
        &ts_from_signal(&gappy),
        GapFillMethod::SplineInterpolation,
        None,
    )
    .expect("spline fill should succeed");
    let linear_filled = GapFiller::fill_gaps(
        &ts_from_signal(&gappy),
        GapFillMethod::LinearInterpolation,
        None,
    )
    .expect("linear fill should succeed");

    let spline_signal = extract_signal(&spline_filled);
    let linear_signal = extract_signal(&linear_filled);

    let spline_err: f64 = (5..9)
        .map(|idx| (spline_signal[idx] - full[idx]).powi(2))
        .sum();
    let linear_err: f64 = (5..9)
        .map(|idx| (linear_signal[idx] - full[idx]).powi(2))
        .sum();

    assert!(
        spline_err < linear_err,
        "spline reconstruction error ({spline_err}) should be lower than linear ({linear_err}) on a curved signal"
    );
}

/// Leading and trailing NaN gaps (outside the range of any valid anchor)
/// must be left unfilled by the spline, matching the documented behaviour of
/// linear interpolation.
#[test]
fn test_spline_leaves_leading_trailing_gaps_unfilled() {
    let mut values = vec![f64::NAN; 10];
    for (i, v) in values.iter_mut().enumerate().take(7).skip(2) {
        *v = (i as f64).sin();
    }

    let filled = GapFiller::fill_gaps(
        &ts_from_signal(&values),
        GapFillMethod::SplineInterpolation,
        None,
    )
    .expect("spline fill should succeed");
    let signal = extract_signal(&filled);

    assert!(signal[0].is_nan());
    assert!(signal[1].is_nan());
    assert!(signal[9].is_nan());
    assert!(signal[3].is_finite());
}

// ─────────────────────────────────────────────────────────────────────────────
// Harmonic regression
// ─────────────────────────────────────────────────────────────────────────────

/// With an irregular (non-orthogonal) gap pattern, the fitted harmonic curve
/// must closely reconstruct the true signal at the gap positions. The old
/// marginal-regression implementation is measurably biased in this scenario
/// because the valid-sample sin/cos cross term is not negligible.
#[test]
fn test_harmonic_regression_reconstructs_signal_with_irregular_gaps() {
    let period = 12usize;
    let amplitude = 5.0_f64;
    let offset = 20.0_f64;
    let phase_shift = 0.7_f64;

    // Two full periods of a harmonic signal.
    let full: Vec<f64> = (0..24)
        .map(|t| {
            let phase = 2.0 * PI * (t as f64) / (period as f64);
            offset + amplitude * (phase + phase_shift).sin()
        })
        .collect();

    // Knock out an irregular (non-symmetric, non-orthogonal) subset of
    // samples so that sum(sin*cos), sum(sin), sum(cos) over the *valid*
    // samples are all measurably non-zero.
    let mut gappy = full.clone();
    for &idx in &[1usize, 2, 3, 4, 14, 15] {
        gappy[idx] = f64::NAN;
    }

    let params = GapFillParams {
        harmonic_period: period,
        ..Default::default()
    };
    let filled = GapFiller::fill_gaps(
        &ts_from_signal(&gappy),
        GapFillMethod::HarmonicRegression,
        Some(params),
    )
    .expect("harmonic regression should succeed");
    let signal = extract_signal(&filled);

    for &idx in &[1usize, 2, 3, 4, 14, 15] {
        let err = (signal[idx] - full[idx]).abs();
        assert!(
            err < 1.0,
            "index {idx}: fitted {} vs truth {} (err={err}) should be within amplitude tolerance",
            signal[idx],
            full[idx]
        );
    }
}

/// Harmonic regression with fewer than 3 valid samples cannot uniquely
/// determine (a, b, c); the implementation must fall back gracefully to the
/// sample mean rather than producing NaN/Inf or panicking.
#[test]
fn test_harmonic_regression_falls_back_gracefully_with_too_few_samples() {
    let period = 12usize;
    let mut values = vec![f64::NAN; period];
    values[0] = 10.0;
    values[1] = 12.0;

    let params = GapFillParams {
        harmonic_period: period,
        ..Default::default()
    };
    let filled = GapFiller::fill_gaps(
        &ts_from_signal(&values),
        GapFillMethod::HarmonicRegression,
        Some(params),
    )
    .expect("harmonic regression should succeed even with sparse data");
    let signal = extract_signal(&filled);

    assert!(signal.iter().all(|v| v.is_finite()));
}
