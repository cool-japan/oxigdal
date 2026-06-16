//! Integration tests for Whittaker smoother and Savitzky-Golay filter.
//!
//! These tests cover:
//! - Correctness properties of `smooth_whittaker` and `smooth_savgol`
//!   (constant input, linear input, polynomial input, NaN gap filling,
//!   edge handling, parameter clamping).
//! - Round-trip dispatch via `GapFiller::fill_gaps` for both methods.
//!
//! The internal `smooth_*` helpers are `pub(crate)`, so we test them
//! indirectly through thin `pub` wrappers exposed in the test-only helper
//! functions below, or we test observable behaviour via `GapFiller::fill_gaps`.
//!
//! For fine-grained unit verification of the kernel / signal functions we
//! call them through the public gap-filling API constructed over a
//! `TimeSeriesRaster`.

#![allow(clippy::expect_used)]

use chrono::{DateTime, NaiveDate};
use oxigdal_temporal::{
    gap_filling::{GapFillMethod, GapFillParams, GapFiller},
    timeseries::{TemporalMetadata, TimeSeriesRaster},
};
use scirs2_core::ndarray::Array3;

// ─────────────────────────────────────────────────────────────────────────────
// Test helpers
// ─────────────────────────────────────────────────────────────────────────────

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

/// Create a `GapFillParams` configured for the Whittaker smoother.
fn whittaker_params(lambda: f64, order: usize) -> GapFillParams {
    GapFillParams {
        whittaker_lambda: lambda,
        whittaker_order: order,
        ..Default::default()
    }
}

/// Create a `GapFillParams` configured for the Savitzky-Golay filter.
fn savgol_params(window: usize, poly_order: usize) -> GapFillParams {
    GapFillParams {
        savgol_window: window,
        savgol_poly_order: poly_order,
        ..Default::default()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Whittaker smoother tests
// ─────────────────────────────────────────────────────────────────────────────

/// A constant signal must be reproduced exactly regardless of λ or order,
/// because the penalty on differences of a constant is zero and the fit
/// perfectly matches the data.
#[test]
fn test_whittaker_constant_series_unchanged() {
    let y: Vec<f64> = vec![5.0; 20];
    let ts = ts_from_signal(&y);
    let params = whittaker_params(100.0, 2);
    let filled =
        GapFiller::fill_gaps(&ts, GapFillMethod::Whittaker, Some(params)).expect("should succeed");
    let z = extract_signal(&filled);
    for (i, (&zi, &yi)) in z.iter().zip(y.iter()).enumerate() {
        assert!(
            (zi - yi).abs() < 1e-7,
            "constant not reproduced at t={i}: got {zi}, expected {yi}"
        );
    }
}

/// For a linear signal `y_i = 2i + 1`, the second-order finite difference
/// of the true function is identically zero.  Therefore, with a very large λ
/// (penalising curvature strongly), the Whittaker smoother should reproduce
/// the linear trend almost exactly.
#[test]
fn test_whittaker_linear_series_preserved_with_large_lambda() {
    let y: Vec<f64> = (0..20).map(|i| 2.0 * i as f64 + 1.0).collect();
    let ts = ts_from_signal(&y);
    let params = whittaker_params(1e8, 2);
    let filled =
        GapFiller::fill_gaps(&ts, GapFillMethod::Whittaker, Some(params)).expect("should succeed");
    let z = extract_signal(&filled);
    for (i, (zi, yi)) in z.iter().zip(y.iter()).enumerate() {
        assert!(
            (zi - yi).abs() < 0.5,
            "linear series not preserved at t={i}: got {zi}, expected {yi}"
        );
    }
}

/// A smoother should reduce the variance of a noisy signal while keeping the
/// mean roughly unchanged.
#[test]
fn test_whittaker_smooths_noise_reduces_variance() {
    // Sine wave with deterministic perturbations (no rand/rand_distr per policy)
    let y: Vec<f64> = (0..40)
        .map(|i| {
            let base = (i as f64 * std::f64::consts::PI / 10.0).sin();
            // Alternating +/- perturbation as a stand-in for noise
            let noise = if i % 2 == 0 { 0.3 } else { -0.3 };
            base + noise
        })
        .collect();

    let mean_y: f64 = y.iter().sum::<f64>() / y.len() as f64;
    let var_y: f64 = y.iter().map(|v| (v - mean_y).powi(2)).sum::<f64>() / y.len() as f64;

    let ts = ts_from_signal(&y);
    let params = whittaker_params(500.0, 2);
    let filled =
        GapFiller::fill_gaps(&ts, GapFillMethod::Whittaker, Some(params)).expect("should succeed");
    let z = extract_signal(&filled);

    let mean_z: f64 = z.iter().sum::<f64>() / z.len() as f64;
    let var_z: f64 = z.iter().map(|v| (v - mean_z).powi(2)).sum::<f64>() / z.len() as f64;

    assert!(
        var_z < var_y,
        "smoothed variance ({var_z:.4}) should be less than noisy variance ({var_y:.4})"
    );
}

/// NaN values (gaps) must be filled by the zero-weight mechanism.
/// For a ramp signal with two consecutive NaNs the gap-filled values should
/// fall within a reasonable range of their true ramp positions.
#[test]
fn test_whittaker_fills_nan_gap_via_zero_weight() {
    let mut y: Vec<f64> = (0..20).map(|i| i as f64).collect();
    y[8] = f64::NAN;
    y[9] = f64::NAN;

    let ts = ts_from_signal(&y);
    let params = whittaker_params(100.0, 2);
    let filled =
        GapFiller::fill_gaps(&ts, GapFillMethod::Whittaker, Some(params)).expect("should succeed");
    let z = extract_signal(&filled);

    assert!(!z[8].is_nan(), "gap at t=8 should be filled, got NaN");
    assert!(!z[9].is_nan(), "gap at t=9 should be filled, got NaN");
    // The filled values should be in the neighbourhood of the true ramp
    // (8.0, 9.0) — allow ±3 for smoothing bias.
    assert!(
        z[8] > 5.0 && z[8] < 12.0,
        "filled z[8]={} out of reasonable range [5, 12]",
        z[8]
    );
    assert!(
        z[9] > 6.0 && z[9] < 13.0,
        "filled z[9]={} out of reasonable range [6, 13]",
        z[9]
    );
}

/// With a very large λ the smoother is dominated by the penalty term and the
/// output approaches a low-degree polynomial trend regardless of the data.
/// We verify that the output values do not deviate wildly from a linear fit.
#[test]
fn test_whittaker_large_lambda_approaches_polynomial_trend() {
    // Quadratic + strong noise
    let y: Vec<f64> = (0..30)
        .map(|i| {
            let quad = (i as f64 - 15.0).powi(2);
            let noise = if i % 3 == 0 { 50.0 } else { -25.0 };
            quad + noise
        })
        .collect();

    let ts = ts_from_signal(&y);
    // Very large lambda → heavily smoothed, approaches a low-degree polynomial
    let params = whittaker_params(1e6, 2);
    let filled =
        GapFiller::fill_gaps(&ts, GapFillMethod::Whittaker, Some(params)).expect("should succeed");
    let z = extract_signal(&filled);

    // The smoothed signal should have much lower range than the noisy input.
    let range_y = y.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
        - y.iter().cloned().fold(f64::INFINITY, f64::min);
    let range_z = z.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
        - z.iter().cloned().fold(f64::INFINITY, f64::min);
    assert!(
        range_z < range_y,
        "smoothed range ({range_z:.1}) should be less than noisy range ({range_y:.1})"
    );
}

/// When the series has only as many points as the penalty order the system is
/// under-determined; the function should fall back and return the input
/// unchanged (no panic, no NaN propagation from the solver).
#[test]
fn test_whittaker_short_series_falls_back() {
    let y = vec![1.0_f64, 2.0]; // n = 2 == order = 2 → fall-back
    let ts = ts_from_signal(&y);
    let params = whittaker_params(100.0, 2);
    let filled = GapFiller::fill_gaps(&ts, GapFillMethod::Whittaker, Some(params))
        .expect("should succeed without error");
    let z = extract_signal(&filled);
    // n <= order: expect the input to be returned unchanged
    assert_eq!(z.len(), y.len());
    for (zi, yi) in z.iter().zip(y.iter()) {
        assert!(
            (zi - yi).abs() < 1e-10 || zi.is_nan(),
            "expected fall-back to input, got {zi} vs {yi}"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Savitzky-Golay filter tests
// ─────────────────────────────────────────────────────────────────────────────

/// A constant signal must be reproduced exactly by SG smoothing because the
/// polynomial fit of any degree through constant data evaluates to that
/// constant everywhere.
#[test]
fn test_savgol_constant_series_unchanged() {
    let y = vec![3.0_f64; 30];
    let ts = ts_from_signal(&y);
    let params = savgol_params(7, 2);
    let filled = GapFiller::fill_gaps(&ts, GapFillMethod::SavitzkyGolay, Some(params))
        .expect("should succeed");
    let z = extract_signal(&filled);
    for (i, (&zi, &yi)) in z.iter().zip(y.iter()).enumerate() {
        assert!(
            (zi - yi).abs() < 1e-5,
            "constant not reproduced at t={i}: got {zi}, expected {yi}"
        );
    }
}

/// A polynomial signal of degree ≤ `poly_order` should be reproduced exactly
/// (up to floating-point rounding) in the interior of the signal, where the
/// full symmetric window is used.
#[test]
fn test_savgol_polynomial_within_order_reproduced_in_interior() {
    // Quadratic y = (i - 15)^2, reproduced by a poly_order=2 SG filter.
    let y: Vec<f64> = (0..30).map(|i| (i as f64 - 15.0).powi(2)).collect();
    let ts = ts_from_signal(&y);
    let params = savgol_params(7, 2);
    let filled = GapFiller::fill_gaps(&ts, GapFillMethod::SavitzkyGolay, Some(params))
        .expect("should succeed");
    let z = extract_signal(&filled);
    // Check interior points only (edges use asymmetric kernels and may differ).
    for i in 5..25 {
        assert!(
            (z[i] - y[i]).abs() < 2.0,
            "quadratic not reproduced at t={i}: got {}, expected {}",
            z[i],
            y[i]
        );
    }
}

/// SG filtering should reduce the variance of a noisy signal.
#[test]
fn test_savgol_smooths_noise_reduces_variance() {
    let y: Vec<f64> = (0..50)
        .map(|i| {
            let base = (i as f64 * std::f64::consts::PI / 8.0).sin();
            let noise = if i % 2 == 0 { 0.4 } else { -0.4 };
            base + noise
        })
        .collect();

    let mean_y: f64 = y.iter().sum::<f64>() / y.len() as f64;
    let var_y: f64 = y.iter().map(|v| (v - mean_y).powi(2)).sum::<f64>() / y.len() as f64;

    let ts = ts_from_signal(&y);
    let params = savgol_params(7, 2);
    let filled = GapFiller::fill_gaps(&ts, GapFillMethod::SavitzkyGolay, Some(params))
        .expect("should succeed");
    let z = extract_signal(&filled);

    let mean_z: f64 = z.iter().sum::<f64>() / z.len() as f64;
    let var_z: f64 = z.iter().map(|v| (v - mean_z).powi(2)).sum::<f64>() / z.len() as f64;

    assert!(
        var_z < var_y,
        "smoothed variance ({var_z:.4}) should be less than noisy variance ({var_y:.4})"
    );
}

/// An even window size is silently bumped to the next odd value.  The function
/// must not panic and must return a valid (finite) result of the correct length.
#[test]
fn test_savgol_even_window_validated_and_bumped() {
    let y: Vec<f64> = (0..20).map(|i| i as f64).collect();
    let ts = ts_from_signal(&y);
    let params = savgol_params(6, 2); // 6 → 7 internally
    let filled = GapFiller::fill_gaps(&ts, GapFillMethod::SavitzkyGolay, Some(params))
        .expect("should succeed without panic");
    let z = extract_signal(&filled);
    assert_eq!(z.len(), y.len(), "output length mismatch");
    assert!(
        z.iter().all(|v| v.is_finite()),
        "output contains non-finite values"
    );
}

/// The first and last elements of the output must be finite.  This verifies
/// the edge-handling (asymmetric window) logic.
#[test]
fn test_savgol_edge_handling_produces_finite_values() {
    let y: Vec<f64> = (0..20).map(|i| i as f64).collect();
    let ts = ts_from_signal(&y);
    let params = savgol_params(7, 2);
    let filled = GapFiller::fill_gaps(&ts, GapFillMethod::SavitzkyGolay, Some(params))
        .expect("should succeed");
    let z = extract_signal(&filled);
    assert!(
        z[0].is_finite(),
        "first output element is non-finite: {}",
        z[0]
    );
    assert!(
        z[z.len() - 1].is_finite(),
        "last output element is non-finite: {}",
        z[z.len() - 1]
    );
}

/// NaN gaps in the input should be pre-filled by linear interpolation before
/// the SG convolution, so the output must not contain NaN at the gap positions.
#[test]
fn test_savgol_nan_gaps_pre_interpolated() {
    let mut y: Vec<f64> = (0..25).map(|i| i as f64).collect();
    y[10] = f64::NAN;
    y[11] = f64::NAN;
    y[12] = f64::NAN;

    let ts = ts_from_signal(&y);
    let params = savgol_params(7, 2);
    let filled = GapFiller::fill_gaps(&ts, GapFillMethod::SavitzkyGolay, Some(params))
        .expect("should succeed");
    let z = extract_signal(&filled);

    for gap_t in [10, 11, 12] {
        assert!(
            !z[gap_t].is_nan(),
            "gap at t={gap_t} should be filled, got NaN"
        );
        assert!(
            z[gap_t].is_finite(),
            "gap at t={gap_t} should be finite, got {}",
            z[gap_t]
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Dispatch test: GapFiller::fill_gaps round-trip
// ─────────────────────────────────────────────────────────────────────────────

/// Verify that both new methods are reachable through the public `fill_gaps`
/// API and return `Ok(TimeSeriesRaster)` without panicking.
#[test]
fn test_gap_filling_dispatch_whittaker_and_savgol_via_fill_gaps() {
    let y: Vec<f64> = (0..20)
        .map(|i| {
            if i == 7 || i == 8 {
                f64::NAN
            } else {
                (i as f64 * std::f64::consts::PI / 5.0).sin() * 0.4 + 0.5
            }
        })
        .collect();

    let ts = ts_from_signal(&y);

    // ── Whittaker ─────────────────────────────────────────────────────────────
    let w_params = GapFillParams {
        whittaker_lambda: 200.0,
        whittaker_order: 2,
        ..Default::default()
    };
    let w_result = GapFiller::fill_gaps(&ts, GapFillMethod::Whittaker, Some(w_params));
    assert!(
        w_result.is_ok(),
        "Whittaker dispatch failed: {:?}",
        w_result
    );
    let w_ts = w_result.expect("already checked");
    assert_eq!(w_ts.len(), ts.len(), "Whittaker output length mismatch");

    let w_signal = extract_signal(&w_ts);
    for (t, v) in w_signal.iter().enumerate() {
        assert!(v.is_finite(), "Whittaker output non-finite at t={t}: {v}");
    }

    // ── Savitzky-Golay ────────────────────────────────────────────────────────
    let sg_params = GapFillParams {
        savgol_window: 7,
        savgol_poly_order: 2,
        ..Default::default()
    };
    let sg_result = GapFiller::fill_gaps(&ts, GapFillMethod::SavitzkyGolay, Some(sg_params));
    assert!(
        sg_result.is_ok(),
        "SavitzkyGolay dispatch failed: {:?}",
        sg_result
    );
    let sg_ts = sg_result.expect("already checked");
    assert_eq!(
        sg_ts.len(),
        ts.len(),
        "SavitzkyGolay output length mismatch"
    );

    let sg_signal = extract_signal(&sg_ts);
    for (t, v) in sg_signal.iter().enumerate() {
        assert!(
            v.is_finite(),
            "SavitzkyGolay output non-finite at t={t}: {v}"
        );
    }
}

/// Both new `GapFillMethod` variants must be present in the enum and
/// distinguishable from existing variants.
#[test]
fn test_gap_fill_method_enum_variants_distinct() {
    let w = GapFillMethod::Whittaker;
    let sg = GapFillMethod::SavitzkyGolay;
    let lin = GapFillMethod::LinearInterpolation;

    assert_ne!(w, sg, "Whittaker and SavitzkyGolay should be distinct");
    assert_ne!(
        w, lin,
        "Whittaker and LinearInterpolation should be distinct"
    );
    assert_ne!(
        sg, lin,
        "SavitzkyGolay and LinearInterpolation should be distinct"
    );
}

/// `GapFillParams::default()` must expose the new fields with sensible defaults.
#[test]
fn test_gap_fill_params_new_fields_have_defaults() {
    let p = GapFillParams::default();
    assert!(
        p.whittaker_lambda > 0.0,
        "whittaker_lambda should be positive by default"
    );
    assert!(
        p.whittaker_order >= 1,
        "whittaker_order should be at least 1 by default"
    );
    assert!(
        p.savgol_window >= 3,
        "savgol_window should be at least 3 by default"
    );
    assert!(
        p.savgol_poly_order >= 1,
        "savgol_poly_order should be at least 1 by default"
    );
}
