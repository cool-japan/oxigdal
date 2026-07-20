//! Integration tests for the real BFAST change-detection implementation.
//!
//! Covers the OLS-MOSUM detector exposed through
//! [`ChangeDetector::detect`] with [`ChangeDetectionMethod::BFAST`] as well as
//! the public helpers in [`oxigeo_temporal::change::bfast`].

use chrono::{DateTime, Utc};
use oxigeo_temporal::change::bfast::{
    bfast_detect, fit_harmonic_season_trend, inferred_period, mosum_critical_value,
    selected_harmonic_order,
};
use oxigeo_temporal::change::{ChangeDetectionConfig, ChangeDetectionMethod, ChangeDetector};
use oxigeo_temporal::timeseries::{TemporalMetadata, TemporalResolution, TimeSeriesRaster};
use scirs2_core::ndarray::Array3;

const PERIOD: usize = 12; // months per year — the annual seasonal cycle

/// Epoch (2015-01-01T00:00:00Z) for the synthetic monthly series, in seconds.
const BASE_EPOCH_SECS: i64 = 1_420_070_400;
/// 30-day spacing in seconds, consistent with [`TemporalResolution::Monthly`].
const STEP_SECS: i64 = 30 * 86_400;

/// Build a single-pixel monthly time series from a sequence of values.
///
/// Uses [`TemporalResolution::Monthly`] so the BFAST period inference yields an
/// annual cycle (~12 samples), and 30-day spacing consistent with that. Built
/// without `expect`/`unwrap` so the helper stays clippy-clean outside `#[test]`.
fn build_series(values: &[f64]) -> TimeSeriesRaster {
    let mut ts = TimeSeriesRaster::with_resolution(TemporalResolution::Monthly);
    for (i, &v) in values.iter().enumerate() {
        let secs = BASE_EPOCH_SECS + STEP_SECS * i as i64;
        let dt: DateTime<Utc> = DateTime::from_timestamp(secs, 0).unwrap_or_default();
        let metadata = TemporalMetadata::new(dt, dt.date_naive());
        // Single-pixel rasters always match the (1,1,1) shape, so this never
        // errors; ignore the Result rather than panicking in a helper.
        let _ = ts.add_raster(metadata, Array3::from_elem((1, 1, 1), v));
    }
    ts
}

/// Seasonal value: amplitude-`amp` annual sinusoid plus an offset.
fn seasonal(i: usize, amp: f64, offset: f64) -> f64 {
    offset + amp * (2.0 * std::f64::consts::PI * i as f64 / PERIOD as f64).sin()
}

fn bfast_config() -> ChangeDetectionConfig {
    ChangeDetectionConfig {
        method: ChangeDetectionMethod::BFAST,
        threshold: None,
        min_magnitude: None,
        nodata: Some(f64::NAN),
        confidence_level: Some(0.95),
    }
}

#[test]
fn test_bfast_no_break_flat_series() {
    // Six years of a perfectly constant signal — no structural change.
    let values: Vec<f64> = (0..72).map(|_| 0.5).collect();
    let ts = build_series(&values);

    let result = ChangeDetector::detect(&ts, &bfast_config()).expect("detect should succeed");

    assert_eq!(result.direction[[0, 0, 0]], 0, "flat series must not break");
    assert_eq!(result.magnitude[[0, 0, 0]], 0.0);
    let conf = result.confidence.as_ref().expect("confidence present");
    assert_eq!(conf[[0, 0, 0]], 0.0);
}

#[test]
fn test_bfast_no_break_pure_seasonal() {
    // Stationary seasonal signal with no trend and no level shift.
    let values: Vec<f64> = (0..72).map(|i| seasonal(i, 0.3, 0.5)).collect();
    let ts = build_series(&values);

    let result = ChangeDetector::detect(&ts, &bfast_config()).expect("detect should succeed");

    assert_eq!(
        result.direction[[0, 0, 0]],
        0,
        "pure seasonal signal must not be flagged as a break"
    );
    let conf = result.confidence.as_ref().expect("confidence present");
    assert_eq!(conf[[0, 0, 0]], 0.0);
}

#[test]
fn test_bfast_detects_single_abrupt_break() {
    // Seasonal signal with a large abrupt downward level shift at month 36.
    let break_at = 36;
    let values: Vec<f64> = (0..72)
        .map(|i| {
            let level = if i < break_at { 0.8 } else { 0.2 };
            seasonal(i, 0.15, level)
        })
        .collect();
    let ts = build_series(&values);

    let result = bfast_detect(&ts, &bfast_config()).expect("detect should succeed");

    assert_ne!(
        result.direction[[0, 0, 0]],
        0,
        "an abrupt level shift must be detected as a break"
    );
    let conf = result.confidence.as_ref().expect("confidence present");
    assert!(
        conf[[0, 0, 0]] > 0.0,
        "confidence must be positive for a detected break"
    );
    assert!(
        result.change_time.is_some(),
        "change_time must be populated"
    );
}

#[test]
fn test_bfast_break_time_within_one_period() {
    // Abrupt break at a known month; the localised break time should land
    // within one seasonal period of the truth.
    let break_at = 40usize;
    let values: Vec<f64> = (0..84)
        .map(|i| {
            let level = if i < break_at { 0.7 } else { 0.15 };
            seasonal(i, 0.1, level)
        })
        .collect();
    let ts = build_series(&values);
    let timestamps: Vec<i64> = ts.timestamps().iter().map(|d| d.timestamp()).collect();
    let true_break_ts = timestamps[break_at];

    let result = bfast_detect(&ts, &bfast_config()).expect("detect should succeed");
    let detected_ts = result.change_time.as_ref().expect("change_time present")[[0, 0, 0]];

    assert_ne!(result.direction[[0, 0, 0]], 0, "break must be detected");

    // One period in seconds (~12 months of 30-day spacing) plus a small slack.
    let one_period_secs = (PERIOD as i64) * 30 * 86_400;
    let delta = (detected_ts - true_break_ts).abs();
    assert!(
        delta <= one_period_secs,
        "break time {detected_ts} should be within one period ({one_period_secs}s) of the truth {true_break_ts}; delta={delta}"
    );
}

#[test]
fn test_bfast_detects_trend_shift() {
    // Flat-then-rising trend: a gradual change in the trend component.
    let break_at = 36usize;
    let values: Vec<f64> = (0..72)
        .map(|i| {
            let trend = if i < break_at {
                0.2
            } else {
                0.2 + 0.02 * (i - break_at) as f64
            };
            seasonal(i, 0.05, trend)
        })
        .collect();
    let ts = build_series(&values);

    let result = bfast_detect(&ts, &bfast_config()).expect("detect should succeed");

    assert_ne!(
        result.direction[[0, 0, 0]],
        0,
        "a trend shift must be detected as a structural break"
    );
}

#[test]
fn test_bfast_short_series_graceful_no_panic() {
    // Only 8 observations — far fewer than two annual periods. Must return a
    // graceful no-break result rather than panicking.
    let values: Vec<f64> = (0..8).map(|i| seasonal(i, 0.3, 0.5)).collect();
    let ts = build_series(&values);

    let result = bfast_detect(&ts, &bfast_config()).expect("short series should not error");

    assert_eq!(
        result.direction[[0, 0, 0]],
        0,
        "short series must not break"
    );
    assert_eq!(result.magnitude[[0, 0, 0]], 0.0);
    let conf = result.confidence.as_ref().expect("confidence present");
    assert_eq!(conf[[0, 0, 0]], 0.0);
    let ct = result.change_time.as_ref().expect("change_time present");
    assert_eq!(ct[[0, 0, 0]], 0);
}

#[test]
fn test_bfast_critical_value_5pct() {
    // The documented 5% OLS-MOSUM critical value for the default h = 0.15 is
    // approximately 1.85 (Chu, Hornik & Kuan 1995).
    let crit = mosum_critical_value(0.15);
    assert!(
        (crit - 1.85).abs() < 1e-9,
        "h=0.15 critical value should be 1.85, got {crit}"
    );

    // Monotone non-increasing in h across the tabulated range.
    let c05 = mosum_critical_value(0.05);
    let c10 = mosum_critical_value(0.10);
    let c30 = mosum_critical_value(0.30);
    assert!(c05 > c10, "critical value should decrease as h grows");
    assert!(c10 > crit);
    assert!(crit > c30);

    // Out-of-range clamps to the table endpoints.
    assert!((mosum_critical_value(0.0) - c05).abs() < 1e-9);
    assert!((mosum_critical_value(1.0) - mosum_critical_value(0.5)).abs() < 1e-9);
}

#[test]
fn test_bfast_magnitude_sign_matches_direction() {
    // Upward abrupt shift: magnitude must be positive and direction must be +1.
    let break_at = 36usize;
    let up: Vec<f64> = (0..72)
        .map(|i| {
            let level = if i < break_at { 0.2 } else { 0.8 };
            seasonal(i, 0.1, level)
        })
        .collect();
    let ts_up = build_series(&up);
    let res_up = bfast_detect(&ts_up, &bfast_config()).expect("detect should succeed");
    let m_up = res_up.magnitude[[0, 0, 0]];
    let d_up = res_up.direction[[0, 0, 0]];
    assert_ne!(d_up, 0, "upward break must be detected");
    assert_eq!(
        d_up,
        if m_up > 0.0 { 1 } else { -1 },
        "direction must equal sign(magnitude)"
    );
    assert!(m_up > 0.0, "upward shift should yield positive magnitude");

    // Downward abrupt shift: magnitude negative and direction -1.
    let down: Vec<f64> = (0..72)
        .map(|i| {
            let level = if i < break_at { 0.8 } else { 0.2 };
            seasonal(i, 0.1, level)
        })
        .collect();
    let ts_down = build_series(&down);
    let res_down = bfast_detect(&ts_down, &bfast_config()).expect("detect should succeed");
    let m_down = res_down.magnitude[[0, 0, 0]];
    let d_down = res_down.direction[[0, 0, 0]];
    assert_ne!(d_down, 0, "downward break must be detected");
    assert_eq!(d_down, if m_down > 0.0 { 1 } else { -1 });
    assert!(
        m_down < 0.0,
        "downward shift should yield negative magnitude"
    );
}

#[test]
fn test_bfast_harmonic_fit_recovers_known_coeffs() {
    // Construct a series from a known season + trend model and check the OLS
    // fit recovers the coefficients.
    let intercept = 0.40;
    let slope = 0.01;
    let sin1 = 0.30;
    let cos1 = -0.20;
    let period = PERIOD as f64;
    let n = 72;

    let values: Vec<f64> = (0..n)
        .map(|i| {
            let t = i as f64;
            let angle = 2.0 * std::f64::consts::PI * t / period;
            intercept + slope * t + sin1 * angle.sin() + cos1 * angle.cos()
        })
        .collect();

    let fit = fit_harmonic_season_trend(&values, period, 1)
        .expect("fit should not error")
        .expect("fit should be available for 72 samples");

    assert!(
        (fit.intercept - intercept).abs() < 1e-6,
        "intercept {} != {}",
        fit.intercept,
        intercept
    );
    assert!(
        (fit.slope - slope).abs() < 1e-6,
        "slope {} != {}",
        fit.slope,
        slope
    );
    assert!((fit.sin_amplitudes[0] - sin1).abs() < 1e-6);
    assert!((fit.cos_amplitudes[0] - cos1).abs() < 1e-6);
    // A noise-free reconstruction has essentially zero residual error.
    assert!(
        fit.sigma < 1e-6,
        "sigma should be ~0 for exact data, got {}",
        fit.sigma
    );
}

#[test]
fn test_bfast_period_and_order_inference() {
    // Sanity-check the public period/order helpers used internally.
    let values: Vec<f64> = (0..72).map(|i| seasonal(i, 0.2, 0.5)).collect();
    let ts = build_series(&values);

    let period = inferred_period(&ts);
    assert!(
        (10.0..=13.0).contains(&period),
        "monthly resolution should infer ~12-sample annual period, got {period}"
    );

    let order = selected_harmonic_order(72, period).expect("order should be feasible");
    assert!(
        (1..=3).contains(&order),
        "order should be in 1..=3, got {order}"
    );

    // Too few samples for any model.
    assert!(selected_harmonic_order(3, period).is_none());
}
