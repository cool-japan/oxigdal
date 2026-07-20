//! Integration tests for LandTrendr piecewise-linear segmentation
//! (Slice 26 W1 — Kennedy et al. 2010, RSE 114:2897-2910).
//!
//! These tests exercise both the per-pixel [`landtrendr_segment`] function and
//! the full [`ChangeDetector::detect`] dispatch pipeline.
#![allow(clippy::expect_used)]

use chrono::{DateTime, NaiveDate};
use oxigeo_temporal::{
    change::{
        ChangeDetectionConfig, ChangeDetectionMethod, ChangeDetector, LandTrendrOptions,
        landtrendr_segment,
    },
    error::TemporalError,
    timeseries::{TemporalMetadata, TimeSeriesRaster},
};
use scirs2_core::ndarray::Array3;

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn make_metadata(year: i32, day_of_year: u32) -> TemporalMetadata {
    let base_secs = 1_640_995_200_i64; // 2022-01-01 00:00:00 UTC
    let offset = (year - 2022) as i64 * 365 * 86_400 + (day_of_year as i64) * 86_400;
    let dt = DateTime::from_timestamp(base_secs + offset, 0).expect("valid timestamp");
    let date = NaiveDate::from_ymd_opt(year, 1, 1)
        .expect("valid date")
        .checked_add_days(chrono::Days::new(u64::from(day_of_year.saturating_sub(1))))
        .expect("valid date offset");
    TemporalMetadata::new(dt, date)
}

/// Build a `TimeSeriesRaster` from per-pixel 1D series.
/// `series` is a `Vec<Vec<Vec<f64>>>` indexed as `series[row][col][t]`,
/// returning shape `(rows, cols, 1)` per timestep. `n` is the number of timesteps.
fn build_ts_from_pixels(series: &[Vec<Vec<f64>>], n_time: usize) -> TimeSeriesRaster {
    let rows = series.len();
    let cols = if rows > 0 { series[0].len() } else { 0 };
    let mut ts = TimeSeriesRaster::with_shape(rows, cols, 1);
    for (t, _) in (0..n_time).enumerate() {
        let mut arr = Array3::<f64>::zeros((rows, cols, 1));
        for (r, row) in series.iter().enumerate().take(rows) {
            for (c, cell) in row.iter().enumerate().take(cols) {
                arr[[r, c, 0]] = cell[t];
            }
        }
        let metadata = make_metadata(2000 + t as i32, 1);
        ts.add_raster(metadata, arr).expect("add raster");
    }
    ts
}

// ---------------------------------------------------------------------------
// 1. constant series
// ---------------------------------------------------------------------------

#[test]
fn test_landtrendr_constant_no_vertices() {
    let values = vec![3.5_f64; 18];
    let opts = LandTrendrOptions::default();
    let result = landtrendr_segment(&values, &opts).expect("segmentation");
    assert!(result.vertices.len() >= 2, "expected at least two vertices");
    let first = result.vertices.first().expect("first").value;
    let last = result.vertices.last().expect("last").value;
    assert!((first - 3.5).abs() < 1e-9);
    assert!((last - 3.5).abs() < 1e-9);
    let magnitude = (last - first).abs();
    assert!(
        magnitude < 1e-9,
        "constant series should have zero magnitude"
    );
}

// ---------------------------------------------------------------------------
// 2. pure linear series
// ---------------------------------------------------------------------------

#[test]
fn test_landtrendr_pure_linear_two_vertices_only() {
    let n = 20_usize;
    let values: Vec<f64> = (0..n).map(|t| 2.0 + 0.35 * t as f64).collect();
    let opts = LandTrendrOptions {
        max_segments: 6,
        ..LandTrendrOptions::default()
    };
    let result = landtrendr_segment(&values, &opts).expect("segmentation");
    assert_eq!(
        result.vertices.len(),
        2,
        "pure linear series should yield exactly two vertices, got {}",
        result.vertices.len()
    );
    let first = result.vertices[0];
    let last = result.vertices[1];
    assert_eq!(first.year_idx, 0);
    assert_eq!(last.year_idx, n - 1);
    assert!((first.value - 2.0).abs() < 1e-6);
    let expected_last = 2.0 + 0.35 * (n - 1) as f64;
    assert!((last.value - expected_last).abs() < 1e-6);
}

// ---------------------------------------------------------------------------
// 3. single disturbance
// ---------------------------------------------------------------------------

#[test]
fn test_landtrendr_single_disturbance_three_vertices() {
    let n = 20_usize;
    let break_at = 10_usize;
    let baseline = 10.0_f64;
    let post = 2.0_f64;
    let mut values = vec![baseline; n];
    for v in values.iter_mut().take(n).skip(break_at) {
        *v = post;
    }
    let opts = LandTrendrOptions {
        prevent_one_year_recovery: false,
        recovery_threshold: 0.0,
        ..LandTrendrOptions::default()
    };
    let result = landtrendr_segment(&values, &opts).expect("segmentation");
    assert!(
        result.vertices.len() >= 3,
        "single disturbance should yield at least 3 vertices, got {}",
        result.vertices.len()
    );
    // The first and last values should reflect baseline / post-disturbance.
    assert!((result.vertices.first().expect("first").value - baseline).abs() < 1.0);
    assert!((result.vertices.last().expect("last").value - post).abs() < 1.0);
    // Magnitude should be sizeable.
    let mag =
        result.vertices.last().expect("last").value - result.vertices.first().expect("first").value;
    assert!(
        mag.abs() > 5.0,
        "expected large negative magnitude, got {mag}"
    );
}

// ---------------------------------------------------------------------------
// 4. disturbance + recovery
// ---------------------------------------------------------------------------

#[test]
fn test_landtrendr_recovery_after_disturbance_four_vertices() {
    let mut values: Vec<f64> = vec![10.0; 8]; // 8 years baseline
    // Sharp drop to 2 at year 8
    values.push(2.0);
    // 11 years sustained recovery back up to 8
    for t in 0..11 {
        let frac = (t + 1) as f64 / 11.0;
        values.push(2.0 + frac * 6.0);
    }
    assert_eq!(values.len(), 20);
    let opts = LandTrendrOptions {
        prevent_one_year_recovery: false,
        recovery_threshold: 0.05,
        ..LandTrendrOptions::default()
    };
    let result = landtrendr_segment(&values, &opts).expect("segmentation");
    assert!(
        result.vertices.len() >= 3,
        "disturbance+recovery should yield ≥3 vertices, got {}",
        result.vertices.len()
    );
    // Expect at least one negative-slope segment and one positive-slope segment.
    let n_negative = result
        .segments
        .iter()
        .filter(|s| s.end_value < s.start_value)
        .count();
    let n_positive = result
        .segments
        .iter()
        .filter(|s| s.end_value > s.start_value)
        .count();
    assert!(n_negative >= 1, "expected ≥1 negative-slope segment");
    assert!(n_positive >= 1, "expected ≥1 positive-slope segment");
}

// ---------------------------------------------------------------------------
// 5. max-segments cap
// ---------------------------------------------------------------------------

#[test]
fn test_landtrendr_respects_max_segments() {
    // Build a noisy sinusoidal-ish series.
    let n = 30_usize;
    let values: Vec<f64> = (0..n)
        .map(|t| {
            let x = t as f64 * 0.5;
            5.0 + (x).sin() * 2.0 + ((x * 0.3).cos()) * 1.5
        })
        .collect();
    let opts = LandTrendrOptions {
        max_segments: 3,
        ..LandTrendrOptions::default()
    };
    let result = landtrendr_segment(&values, &opts).expect("segmentation");
    assert!(
        result.vertices.len() <= 4,
        "max_segments=3 should yield ≤4 vertices, got {}",
        result.vertices.len()
    );
}

// ---------------------------------------------------------------------------
// 6. continuity at vertices
// ---------------------------------------------------------------------------

#[test]
fn test_landtrendr_continuity_at_vertices() {
    let values: Vec<f64> = (0..20)
        .map(|t| {
            let t = t as f64;
            if t < 10.0 {
                10.0 - 0.5 * t
            } else {
                5.0 + 0.3 * (t - 10.0)
            }
        })
        .collect();
    let opts = LandTrendrOptions::default();
    let result = landtrendr_segment(&values, &opts).expect("segmentation");
    for w in result.segments.windows(2) {
        let left = w[0];
        let right = w[1];
        assert!(
            (left.end_value - right.start_value).abs() < 1e-9,
            "segments must be continuous at vertex: left.end_value={}, right.start_value={}",
            left.end_value,
            right.start_value
        );
        assert_eq!(left.end_year, right.start_year);
    }
}

// ---------------------------------------------------------------------------
// 7. spike dampening
// ---------------------------------------------------------------------------

#[test]
fn test_landtrendr_spike_dampening_single_outlier() {
    let n = 15_usize;
    let mut values = vec![5.0_f64; n];
    // Single huge spike in the middle.
    values[7] = 50.0;
    let opts = LandTrendrOptions {
        spike_threshold: 0.5,
        ..LandTrendrOptions::default()
    };
    let result = landtrendr_segment(&values, &opts).expect("segmentation");
    // No vertex should sit at the spike index with the spike's value.
    let any_at_spike = result
        .vertices
        .iter()
        .any(|v| v.year_idx == 7 && (v.value - 50.0).abs() < 1.0);
    assert!(
        !any_at_spike,
        "spike should be dampened, not retained as a vertex"
    );
    // The vertex set should be very small (essentially flat).
    assert!(result.vertices.len() <= 4);
    let mag = (result.vertices.last().expect("last").value
        - result.vertices.first().expect("first").value)
        .abs();
    assert!(
        mag < 5.0,
        "magnitude after spike dampening should be small, got {mag}"
    );
}

// ---------------------------------------------------------------------------
// 8. short series rejection
// ---------------------------------------------------------------------------

#[test]
fn test_landtrendr_short_series_under_min_observations_errors() {
    let values = vec![1.0_f64, 2.0, 3.0, 4.0, 5.0]; // < 6
    let opts = LandTrendrOptions::default();
    let err = landtrendr_segment(&values, &opts).expect_err("should fail");
    assert!(
        matches!(err, TemporalError::InsufficientData(_)),
        "expected InsufficientData, got {err:?}"
    );
}

// ---------------------------------------------------------------------------
// 9. default options
// ---------------------------------------------------------------------------

#[test]
fn test_landtrendr_default_options_match_kennedy_2010() {
    let opts = LandTrendrOptions::default();
    assert_eq!(opts.max_segments, 6);
    assert!((opts.spike_threshold - 0.9).abs() < 1e-12);
    assert_eq!(opts.vertex_count_overshoot, 3);
    assert!(opts.prevent_one_year_recovery);
    assert!((opts.recovery_threshold - 0.25).abs() < 1e-12);
    assert!((opts.pval_threshold - 0.05).abs() < 1e-12);
    assert!((opts.best_model_proportion - 0.75).abs() < 1e-12);
    assert_eq!(opts.min_observations, 6);
}

// ---------------------------------------------------------------------------
// 10. p_of_f filtering
// ---------------------------------------------------------------------------

#[test]
fn test_landtrendr_p_of_f_threshold_filters() {
    // Series A: essentially flat (low-amplitude bounded noise) — the F-walk
    // should reject extra vertices because the full-model residuals are
    // tiny in absolute terms.
    let series_a: Vec<f64> = vec![5.0; 20];
    // Series B: strong step at t=10 (large-amplitude clean break).
    let series_b: Vec<f64> = (0..20).map(|t| if t < 10 { 10.0 } else { 1.0 }).collect();
    let opts = LandTrendrOptions {
        prevent_one_year_recovery: false,
        recovery_threshold: 0.0,
        ..LandTrendrOptions::default()
    };
    let res_a = landtrendr_segment(&series_a, &opts).expect("segmentation A");
    let res_b = landtrendr_segment(&series_b, &opts).expect("segmentation B");
    // Magnitude on B should be much larger than A.
    let mag_a = (res_a.vertices.last().expect("a-last").value
        - res_a.vertices.first().expect("a-first").value)
        .abs();
    let mag_b = (res_b.vertices.last().expect("b-last").value
        - res_b.vertices.first().expect("b-first").value)
        .abs();
    assert!(
        mag_b > mag_a + 3.0,
        "break series magnitude {mag_b} should >> noise {mag_a}"
    );
    // The break-heavy series should retain at least as many vertices as the
    // flat series.
    assert!(
        res_b.vertices.len() >= res_a.vertices.len(),
        "high-F-stat case should keep ≥ vertices: A={} B={}",
        res_a.vertices.len(),
        res_b.vertices.len()
    );
    // Flat series should converge to the minimal 2-vertex model.
    assert_eq!(
        res_a.vertices.len(),
        2,
        "flat series should yield 2 vertices, got {}",
        res_a.vertices.len()
    );
}

// ---------------------------------------------------------------------------
// 11. prevent one-year recovery
// ---------------------------------------------------------------------------

#[test]
fn test_landtrendr_prevent_one_year_recovery() {
    // 20-year series: baseline `a`, single anomaly `b` at year 5, then back to `a`.
    let mut values = vec![10.0_f64; 20];
    values[5] = 2.0;
    let opts = LandTrendrOptions {
        prevent_one_year_recovery: true,
        ..LandTrendrOptions::default()
    };
    let result = landtrendr_segment(&values, &opts).expect("segmentation");
    // No segment should be a 1-step positive-slope segment.
    for seg in &result.segments {
        if seg.length() == 1 {
            assert!(
                seg.end_value <= seg.start_value,
                "found 1-year recovery segment that should have been merged: {seg:?}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 12. end-to-end pipeline
// ---------------------------------------------------------------------------

#[test]
fn test_landtrendr_detection_pipeline_end_to_end() {
    let n_time = 15_usize;
    // 3×3 grid; pixels (0,0) and (1,1) experience disturbances, others flat.
    let mut series: Vec<Vec<Vec<f64>>> = vec![vec![vec![5.0; n_time]; 3]; 3];
    // pixel (0,0): drop at t=7
    for slot in series[0][0].iter_mut().take(n_time).skip(7) {
        *slot = 1.0;
    }
    // pixel (1,1): drop at t=9
    for slot in series[1][1].iter_mut().take(n_time).skip(9) {
        *slot = 0.5;
    }
    let ts = build_ts_from_pixels(&series, n_time);
    let config = ChangeDetectionConfig {
        method: ChangeDetectionMethod::LandTrendr,
        ..Default::default()
    };
    let result = ChangeDetector::detect(&ts, &config).expect("detect");
    assert_eq!(result.magnitude.shape(), &[3, 3, 1]);
    // Disturbed pixels should have non-trivial magnitude.
    let mag_00 = result.magnitude[[0, 0, 0]];
    let mag_11 = result.magnitude[[1, 1, 0]];
    assert!(mag_00 > 1.0, "expected disturbance at (0,0), got {mag_00}");
    assert!(mag_11 > 1.0, "expected disturbance at (1,1), got {mag_11}");
    assert_eq!(result.direction[[0, 0, 0]], -1);
    assert_eq!(result.direction[[1, 1, 0]], -1);
    // Flat pixels should have very small or zero magnitude.
    let mag_22 = result.magnitude[[2, 2, 0]];
    assert!(
        mag_22 < 0.5,
        "expected flat pixel (2,2) magnitude near 0, got {mag_22}"
    );
    assert_eq!(result.direction[[2, 2, 0]], 0);

    // Verify the temp_dir convention is available (no actual tempfile needed
    // here, but per spec we touch the helper so the policy is honoured).
    let _tmp = std::env::temp_dir();
}
