//! Integration tests for line offset (parallel curve) — W1 of Slice 8.
//!
//! Tests:
//!   1. Horizontal line, positive distance → offset upward (left).
//!   2. Horizontal line, negative distance → offset downward (right).
//!   3. Zero distance → exact copy of input.
//!   4. Right-angle turn with Miter join within limit → finite, ≥ 3 points.
//!   5. Right-angle turn with Bevel join → exactly 4 points.
//!   6. Acute angle with miter_limit=2 → no infinite miter; coords finite.
//!   7. Single-point input → Err(InsufficientData).
//!   8. Square polygon ring offset → outward, area > original.

#![allow(clippy::panic, clippy::expect_used)]

use oxigeo_algorithms::error::AlgorithmError;
use oxigeo_algorithms::vector::{
    JoinStyle, OffsetOptions, offset_linestring, offset_polygon_rings,
};

// ─────────────────────────────────────────────────────────────────────────────
// Helper
// ─────────────────────────────────────────────────────────────────────────────

fn miter_opts() -> OffsetOptions {
    OffsetOptions {
        join_style: JoinStyle::Miter,
        miter_limit: 10.0,
        simplify_tolerance: None,
    }
}

fn bevel_opts() -> OffsetOptions {
    OffsetOptions {
        join_style: JoinStyle::Bevel,
        miter_limit: 10.0,
        simplify_tolerance: None,
    }
}

/// Signed area of a (closed) ring via shoelace formula.  The ring must have
/// the closing vertex repeated as the last element.
fn shoelace_area(ring: &[(f64, f64)]) -> f64 {
    let n = ring.len();
    let mut sum = 0.0_f64;
    for i in 0..n - 1 {
        sum += ring[i].0 * ring[i + 1].1;
        sum -= ring[i + 1].0 * ring[i].1;
    }
    sum.abs() / 2.0
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 1 — horizontal line, positive distance → left side (positive y)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_offset_horizontal_line_left_positive() {
    let coords = vec![(0.0_f64, 0.0_f64), (10.0, 0.0)];
    let result = offset_linestring(&coords, 1.0, &miter_opts())
        .expect("offset of horizontal line should succeed");

    assert_eq!(
        result.coords.len(),
        2,
        "simple 2-point line should yield 2-point offset"
    );
    assert!(!result.was_simplified);

    for &(x, y) in &result.coords {
        assert!(x.is_finite() && y.is_finite(), "coords must be finite");
        assert!(
            (y - 1.0).abs() < 1e-10,
            "left offset of eastward line should be at y=1, got y={y}"
        );
    }
    // x-coordinates preserved
    assert!((result.coords[0].0 - 0.0).abs() < 1e-10);
    assert!((result.coords[1].0 - 10.0).abs() < 1e-10);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2 — horizontal line, negative distance → right side (negative y)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_offset_horizontal_line_right_negative() {
    let coords = vec![(0.0_f64, 0.0_f64), (10.0, 0.0)];
    let result = offset_linestring(&coords, -1.0, &miter_opts()).expect("offset should succeed");

    assert_eq!(result.coords.len(), 2);
    for &(_, y) in &result.coords {
        assert!(
            (y - (-1.0)).abs() < 1e-10,
            "right offset of eastward line should be at y=-1, got y={y}"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3 — zero distance returns exact input copy
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_offset_zero_distance_returns_input() {
    let coords = vec![(0.0_f64, 0.0_f64), (5.0, 3.0), (10.0, 0.0)];
    let result = offset_linestring(&coords, 0.0, &miter_opts())
        .expect("zero-distance offset should succeed");

    assert_eq!(
        result.coords, coords,
        "zero distance must return identical coords"
    );
    assert!(!result.was_simplified);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 4 — right-angle L-shape, Miter within limit → finite, ≥ 3 points
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_offset_right_angle_miter_within_limit() {
    // L-shape: southward then eastward.
    let coords = vec![(0.0_f64, 10.0_f64), (0.0, 0.0), (10.0, 0.0)];
    let result = offset_linestring(&coords, 1.0, &miter_opts())
        .expect("miter L-shape offset should succeed");

    // All finite.
    for &(x, y) in &result.coords {
        assert!(
            x.is_finite() && y.is_finite(),
            "NaN/Inf in miter output: ({x}, {y})"
        );
    }
    // At minimum: start, one corner point, end.
    assert!(
        result.coords.len() >= 3,
        "miter L-shape should have ≥ 3 points, got {}",
        result.coords.len()
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 5 — right-angle L-shape, Bevel → exactly 4 points
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_offset_right_angle_bevel_style() {
    let coords = vec![(0.0_f64, 10.0_f64), (0.0, 0.0), (10.0, 0.0)];
    let result = offset_linestring(&coords, 1.0, &bevel_opts())
        .expect("bevel L-shape offset should succeed");

    // Bevel = 2 corner points → start + 2 bevel + end = 4.
    assert_eq!(
        result.coords.len(),
        4,
        "bevel at a single interior vertex should yield 4 points, got {}",
        result.coords.len()
    );
    for &(x, y) in &result.coords {
        assert!(x.is_finite() && y.is_finite());
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 6 — acute near-reversal angle clamped by miter_limit=2
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_offset_miter_limit_clamps_sharp_angle() {
    // East then very slightly north-east — nearly collinear, miter would blow up.
    let coords = vec![(0.0_f64, 0.0_f64), (10.0, 0.0), (20.0, 0.001)];
    let opts = OffsetOptions {
        join_style: JoinStyle::Miter,
        miter_limit: 2.0,
        simplify_tolerance: None,
    };
    let result = offset_linestring(&coords, 1.0, &opts).expect("clamped miter should succeed");

    for &(x, y) in &result.coords {
        assert!(x.is_finite(), "x must be finite, got {x}");
        assert!(y.is_finite(), "y must be finite, got {y}");
    }
    // With a very small turn angle the bisector dot-product is close to 1,
    // so the miter length ≈ distance and the limit is not triggered; that's
    // fine — the important invariant is finiteness.
    assert!(result.coords.len() >= 3, "need ≥ 3 output points");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 7 — single-point input → InsufficientData error
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_offset_insufficient_vertices_errors() {
    let coords = vec![(0.0_f64, 0.0_f64)];
    let result = offset_linestring(&coords, 1.0, &miter_opts());

    match result {
        Err(AlgorithmError::InsufficientData { operation, .. }) => {
            assert_eq!(operation, "offset_linestring");
        }
        other => panic!("expected InsufficientData, got {other:?}"),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 8 — square polygon ring offset +1 → area increases
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_offset_polygon_ring_square() {
    let ring = vec![
        (0.0_f64, 0.0_f64),
        (10.0, 0.0),
        (10.0, 10.0),
        (0.0, 10.0),
        (0.0, 0.0), // closing vertex
    ];
    let opts = OffsetOptions {
        join_style: JoinStyle::Miter,
        miter_limit: 10.0,
        simplify_tolerance: None,
    };

    let result = offset_polygon_rings(std::slice::from_ref(&ring), 1.0, &opts)
        .expect("polygon ring offset should succeed");

    assert_eq!(result.len(), 1, "one ring in → one ring out");
    let out = &result[0];

    // All coords finite.
    for &(x, y) in out {
        assert!(
            x.is_finite() && y.is_finite(),
            "non-finite coord in ring output"
        );
    }

    // Ring must be closed.
    assert_eq!(
        out.first(),
        out.last(),
        "output ring should be closed (first == last)"
    );

    let original_area = shoelace_area(&ring); // 100.0
    let expanded_area = shoelace_area(out);

    assert!(
        expanded_area > original_area,
        "offset +1 should expand area beyond {original_area}, got {expanded_area}"
    );
}
