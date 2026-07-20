//! Integration tests for minimum bounding geometry (W6, Slice 6).
//!
//! Seven tests cover:
//!  1. Axis-aligned unit square → MAR area ≈ 1.0
//!  2. Diamond (square rotated 45°) → MAR area ≈ 2.0
//!  3. Circle approximation → MAR area ≤ AABB area
//!  4. Three collinear points → SEC is diameter circle
//!  5. Unit square → SEC radius = √2 / 2
//!  6. 100 arithmetic-sequence points → all inside SEC
//!  7. Empty input → zero circle

use oxigeo_algorithms::vector::{
    Circle, Coordinate, RotatedRect, aabb, min_area_rotated_rect, smallest_enclosing_circle,
};

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn coords(pairs: &[(f64, f64)]) -> Vec<Coordinate> {
    pairs
        .iter()
        .map(|&(x, y)| Coordinate::new_2d(x, y))
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 1 — axis-aligned unit square
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_min_rect_axis_aligned_square_returns_square() {
    let pts = coords(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]);
    let result: RotatedRect =
        min_area_rotated_rect(&pts).expect("should return Some for 4-point square");

    assert!(
        result.area().is_finite(),
        "area must be finite, got {}",
        result.area()
    );
    assert!(
        (result.area() - 1.0).abs() < 1e-9,
        "expected area ≈ 1.0, got {}",
        result.area()
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2 — diamond (square rotated 45°)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_min_rect_rotated_45deg_recovers_orientation() {
    // Diamond vertices: (1,0), (2,1), (1,2), (0,1)
    // Side length = √2, area = 2
    let pts = coords(&[(1.0, 0.0), (2.0, 1.0), (1.0, 2.0), (0.0, 1.0)]);
    let result: RotatedRect = min_area_rotated_rect(&pts).expect("should return Some for diamond");

    assert!(
        (result.area() - 2.0).abs() < 1e-9,
        "expected area ≈ 2.0, got {}",
        result.area()
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3 — 50-point circle approximation: MAR area ≤ AABB area
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_min_rect_circle_approximation_area_no_greater_than_aabb() {
    let n = 50usize;
    let pts: Vec<Coordinate> = (0..n)
        .map(|i| {
            let angle = (i as f64) * 2.0 * std::f64::consts::PI / (n as f64);
            Coordinate::new_2d(angle.cos(), angle.sin())
        })
        .collect();

    let result: RotatedRect =
        min_area_rotated_rect(&pts).expect("should return Some for circle approximation");

    // AABB of the unit circle is 2×2 = 4.0
    let (min_x, min_y, max_x, max_y) = aabb(&pts).expect("non-empty");
    let aabb_area = (max_x - min_x) * (max_y - min_y);

    assert!(
        result.area() <= aabb_area + 1e-9,
        "MAR area {} exceeds AABB area {}",
        result.area(),
        aabb_area
    );
    // A tight sanity upper bound
    assert!(
        result.area() <= 4.01,
        "MAR area {} exceeds expected upper bound 4.01",
        result.area()
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 4 — three collinear points: SEC is the outer-diameter circle
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_welzl_circle_three_collinear_points_is_diameter() {
    let pts = coords(&[(0.0, 0.0), (1.0, 0.0), (2.0, 0.0)]);
    let result: Circle = smallest_enclosing_circle(&pts);

    assert!(
        (result.radius - 1.0).abs() < 1e-9,
        "expected radius ≈ 1.0, got {}",
        result.radius
    );
    assert!(
        (result.center.x - 1.0).abs() < 1e-9,
        "expected center.x ≈ 1.0, got {}",
        result.center.x
    );
    assert!(
        result.center.y.abs() < 1e-9,
        "expected center.y ≈ 0.0, got {}",
        result.center.y
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 5 — unit square: SEC has radius √2 / 2
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_welzl_circle_unit_square_radius_sqrt_half() {
    let pts = coords(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]);
    let result: Circle = smallest_enclosing_circle(&pts);

    let expected_radius = std::f64::consts::SQRT_2 * 0.5;
    assert!(
        (result.radius - expected_radius).abs() < 1e-9,
        "expected radius ≈ {expected_radius}, got {}",
        result.radius
    );
    assert!(
        (result.center.x - 0.5).abs() < 1e-9,
        "expected center.x ≈ 0.5, got {}",
        result.center.x
    );
    assert!(
        (result.center.y - 0.5).abs() < 1e-9,
        "expected center.y ≈ 0.5, got {}",
        result.center.y
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 6 — 100 arithmetic-sequence points: all inside SEC
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_welzl_circle_large_random_point_set_contains_all_points() {
    // Deterministic quasi-random points using golden ratio and e sequences.
    let pts: Vec<Coordinate> = (0..100)
        .map(|i| {
            let fi = i as f64;
            let x = (fi * 1.618_033_988_749_895) % 10.0 - 5.0;
            let y = (fi * std::f64::consts::E) % 10.0 - 5.0;
            Coordinate::new_2d(x, y)
        })
        .collect();

    let circle: Circle = smallest_enclosing_circle(&pts);

    for (idx, &p) in pts.iter().enumerate() {
        assert!(
            circle.contains(p),
            "point #{idx} ({}, {}) is not contained in circle with center ({}, {}) radius {}",
            p.x,
            p.y,
            circle.center.x,
            circle.center.y,
            circle.radius
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 7 — empty input returns zero circle
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_welzl_empty_input_returns_zero_circle() {
    let result: Circle = smallest_enclosing_circle(&[]);

    assert!(
        result.center.x.abs() < 1e-12 && result.center.y.abs() < 1e-12,
        "expected zero center, got ({}, {})",
        result.center.x,
        result.center.y
    );
    assert!(
        result.radius.abs() < 1e-12,
        "expected zero radius, got {}",
        result.radius
    );
}
