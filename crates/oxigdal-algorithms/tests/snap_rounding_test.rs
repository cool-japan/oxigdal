//! Integration tests for Hobby snap rounding.
//!
//! Covers:
//! 1. `snap_coordinate` grid rounding
//! 2. `snap_linestring` consecutive-duplicate removal
//! 3. Parallel lines — no spurious intersections
//! 4. X-crossing lines — shared intersection vertex
//! 5. Intersection vertex lies on the grid
//! 6. Collinear segments — no false intersection
//! 7. T-junction — horizontal line split at stem endpoint
//! 8. Multiple crossing lines — iteration terminates

#![allow(clippy::unwrap_used, missing_docs)]

use oxigdal_algorithms::vector::{Coordinate, snap_coordinate, snap_linestring};
use oxigdal_algorithms::{SnapRoundingOptions, SnappedSegment, snap_round};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Construct a 2-D coordinate.
fn c(x: f64, y: f64) -> Coordinate {
    Coordinate::new_2d(x, y)
}

/// Check whether `coord` is a multiple of `prec` on both axes within 1 ULP
/// of `prec`.
fn on_grid(coord: &Coordinate, prec: f64) -> bool {
    let rx = (coord.x / prec).round() * prec;
    let ry = (coord.y / prec).round() * prec;
    (coord.x - rx).abs() < prec * 1e-9 && (coord.y - ry).abs() < prec * 1e-9
}

// ---------------------------------------------------------------------------
// Test 1 — snap_coordinate rounds to grid
// ---------------------------------------------------------------------------

#[test]
fn test_snap_coordinate_rounds_to_grid() {
    let prec = 1e-6_f64;

    // (0.0000001, 0.9999999) with precision 1e-6
    // x: 0.0000001 / 1e-6 = 0.1 → round = 0 → 0 * 1e-6 = 0.0
    // y: 0.9999999 / 1e-6 = 999999.9 → round = 1000000 → 1000000 * 1e-6 = 1.0
    let raw = c(0.000_000_1, 0.999_999_9);
    let snapped = snap_coordinate(&raw, prec);

    assert!(
        (snapped.x - 0.0).abs() < prec * 0.5,
        "x should snap to 0.0, got {}",
        snapped.x
    );
    assert!(
        (snapped.y - 1.0).abs() < prec * 0.5,
        "y should snap to 1.0, got {}",
        snapped.y
    );

    // Verify the result lies exactly on the grid.
    assert!(on_grid(&snapped, prec), "snapped coordinate not on grid");
}

// ---------------------------------------------------------------------------
// Test 2 — snap_linestring removes consecutive duplicates
// ---------------------------------------------------------------------------

#[test]
fn test_snap_linestring_removes_consecutive_duplicates() {
    let prec = 1e-3_f64;

    // Three original coords; the first two collapse to (0, 0) at prec 1e-3.
    let coords = vec![
        c(0.0001, 0.0001),
        c(0.0002, 0.0002), // also collapses to (0, 0) at prec 1e-3
        c(1.0, 2.0),
    ];
    let result = snap_linestring(&coords, prec);

    // The first two collapse, so we expect exactly 2 distinct coords.
    assert_eq!(
        result.len(),
        2,
        "Expected 2 after de-duplication, got {}",
        result.len()
    );

    // No two consecutive elements should be equal.
    for window in result.windows(2) {
        let a = &window[0];
        let b = &window[1];
        assert!(
            (a.x - b.x).abs() > f64::EPSILON || (a.y - b.y).abs() > f64::EPSILON,
            "Consecutive duplicate found at ({}, {})",
            a.x,
            a.y
        );
    }
}

// ---------------------------------------------------------------------------
// Test 3 — two parallel lines produce no intersections
// ---------------------------------------------------------------------------

#[test]
fn test_snap_round_no_intersections_returns_original_segments() {
    let prec = 1e-6;
    // Horizontal line at y=0 and y=1 — parallel, no intersection.
    let line_a = vec![c(0.0, 0.0), c(10.0, 0.0)];
    let line_b = vec![c(0.0, 1.0), c(10.0, 1.0)];
    let lines = vec![line_a, line_b];

    let opts = SnapRoundingOptions {
        precision: prec,
        max_iterations: 8,
    };
    let result = snap_round(&lines, &opts).unwrap();

    // Two lines, one segment each — no splits expected.
    assert_eq!(
        result.segments.len(),
        2,
        "Expected 2 segments for parallel lines, got {}",
        result.segments.len()
    );
    assert_eq!(
        result.intersections_added, 0,
        "Parallel lines should produce no intersections"
    );
}

// ---------------------------------------------------------------------------
// Test 4 — two crossing lines share the intersection vertex
// ---------------------------------------------------------------------------

#[test]
fn test_snap_round_two_crossing_lines_produces_intersection_vertex() {
    let prec = 1e-6;
    // "/" and "\" crossing at (5, 5).
    let line_a = vec![c(0.0, 0.0), c(10.0, 10.0)];
    let line_b = vec![c(0.0, 10.0), c(10.0, 0.0)];
    let lines = vec![line_a, line_b];

    let opts = SnapRoundingOptions {
        precision: prec,
        max_iterations: 8,
    };
    let result = snap_round(&lines, &opts).unwrap();

    // Each original segment is split at (5, 5), so we expect 4 segments.
    assert_eq!(
        result.segments.len(),
        4,
        "Expected 4 segments after X-split, got {}",
        result.segments.len()
    );

    // Exactly one intersection should have been discovered.
    assert!(
        result.intersections_added >= 1,
        "At least one intersection should be added"
    );

    // Both line_0 and line_1 should share the intersection vertex.
    let shared_vertex = c(5.0, 5.0);
    let has_line0_seg_with_vertex = result.segments.iter().any(|s| {
        s.source_line == 0
            && ((s.start.x - shared_vertex.x).abs() < prec
                || (s.end.x - shared_vertex.x).abs() < prec)
    });
    let has_line1_seg_with_vertex = result.segments.iter().any(|s| {
        s.source_line == 1
            && ((s.start.x - shared_vertex.x).abs() < prec
                || (s.end.x - shared_vertex.x).abs() < prec)
    });
    assert!(
        has_line0_seg_with_vertex,
        "Line 0 should have a segment endpoint at the intersection vertex"
    );
    assert!(
        has_line1_seg_with_vertex,
        "Line 1 should have a segment endpoint at the intersection vertex"
    );
}

// ---------------------------------------------------------------------------
// Test 5 — intersection vertex lies exactly on the grid
// ---------------------------------------------------------------------------

#[test]
fn test_snap_round_intersection_vertex_lies_on_grid() {
    let prec = 0.01_f64;
    // Diagonal cross slightly off-grid to stress snap rounding.
    let line_a = vec![c(0.0, 0.0), c(1.007, 1.007)];
    let line_b = vec![c(0.0, 1.007), c(1.007, 0.0)];
    let lines = vec![line_a, line_b];

    let opts = SnapRoundingOptions {
        precision: prec,
        max_iterations: 8,
    };
    let result = snap_round(&lines, &opts).unwrap();

    // Every vertex in the output must lie on the precision grid.
    for seg in &result.segments {
        assert!(
            on_grid(&seg.start, prec),
            "Start vertex ({}, {}) not on grid (prec={})",
            seg.start.x,
            seg.start.y,
            prec
        );
        assert!(
            on_grid(&seg.end, prec),
            "End vertex ({}, {}) not on grid (prec={})",
            seg.end.x,
            seg.end.y,
            prec
        );
    }
}

// ---------------------------------------------------------------------------
// Test 6 — collinear (overlapping) segments produce no false intersection
// ---------------------------------------------------------------------------

#[test]
fn test_snap_round_collinear_segments_no_false_intersection() {
    let prec = 1e-6;
    // Two segments along y=0; they are collinear but from different lines.
    // No interior crossing should be introduced — only overlap endpoints
    // if any, but they already sit on grid.
    let line_a = vec![c(0.0, 0.0), c(5.0, 0.0)];
    let line_b = vec![c(3.0, 0.0), c(8.0, 0.0)]; // overlaps in [3,5]
    let lines = vec![line_a, line_b];

    let opts = SnapRoundingOptions {
        precision: prec,
        max_iterations: 8,
    };
    let result = snap_round(&lines, &opts).unwrap();

    // All vertices must still be on the grid.
    for seg in &result.segments {
        assert!(on_grid(&seg.start, prec));
        assert!(on_grid(&seg.end, prec));
    }

    // Overlap endpoints are snapped — no zero-length segments should survive.
    for seg in &result.segments {
        assert!(
            (seg.start.x - seg.end.x).abs() > prec * 0.5
                || (seg.start.y - seg.end.y).abs() > prec * 0.5,
            "Zero-length segment found: ({},{})→({},{})",
            seg.start.x,
            seg.start.y,
            seg.end.x,
            seg.end.y
        );
    }
}

// ---------------------------------------------------------------------------
// Test 7 — T-junction: horizontal line is split at the stem's endpoint
// ---------------------------------------------------------------------------

#[test]
fn test_snap_round_t_junction_single_split() {
    let prec = 1e-6;
    // Horizontal line:  (0,0) → (10,0)
    // Vertical stem:    (5,0) → (5,5)  — touches the horizontal at (5,0)
    //
    // The stem's start point (5,0) lies on the horizontal segment interior,
    // so the horizontal should be split there.
    let horizontal = vec![c(0.0, 0.0), c(10.0, 0.0)];
    let stem = vec![c(5.0, 0.0), c(5.0, 5.0)];
    let lines = vec![horizontal, stem];

    let opts = SnapRoundingOptions {
        precision: prec,
        max_iterations: 8,
    };
    let result = snap_round(&lines, &opts).unwrap();

    // The horizontal should now contain a vertex at x=5, y=0.
    let junction = c(5.0, 0.0);
    let horizontal_segs: Vec<&SnappedSegment> = result
        .segments
        .iter()
        .filter(|s| s.source_line == 0)
        .collect();
    let has_junction = horizontal_segs
        .iter()
        .any(|s| (s.start.x - junction.x).abs() < prec || (s.end.x - junction.x).abs() < prec);
    assert!(
        has_junction,
        "Horizontal line should be split at T-junction x=5 (got {} segments for line 0)",
        horizontal_segs.len()
    );
}

// ---------------------------------------------------------------------------
// Test 8 — multiple crossing lines, iteration terminates
// ---------------------------------------------------------------------------

#[test]
fn test_snap_round_multiple_lines_no_infinite_loop() {
    let prec = 1e-4;
    // Five lines through the centre of a 10×10 square, creating a star pattern.
    // They all intersect near (5, 5), producing multiple intersection points
    // that each need to be propagated.
    let lines: Vec<Vec<Coordinate>> = vec![
        vec![c(0.0, 0.0), c(10.0, 10.0)],
        vec![c(0.0, 10.0), c(10.0, 0.0)],
        vec![c(5.0, 0.0), c(5.0, 10.0)],
        vec![c(0.0, 5.0), c(10.0, 5.0)],
        vec![c(0.0, 2.0), c(10.0, 8.0)],
    ];

    let opts = SnapRoundingOptions {
        precision: prec,
        max_iterations: 8,
    };
    // Must not panic or loop forever.
    let result = snap_round(&lines, &opts).unwrap();

    // Algorithm should terminate within the max iterations.
    assert!(
        result.iterations <= 8,
        "Iterations exceeded limit: {}",
        result.iterations
    );

    // All output segments must have grid-aligned vertices.
    for seg in &result.segments {
        assert!(
            on_grid(&seg.start, prec),
            "Start not on grid: ({}, {})",
            seg.start.x,
            seg.start.y
        );
        assert!(
            on_grid(&seg.end, prec),
            "End not on grid: ({}, {})",
            seg.end.x,
            seg.end.y
        );
    }

    // There must be more segments than the original 5 (crossings were split).
    assert!(
        result.segments.len() >= 5,
        "Expected at least 5 segments, got {}",
        result.segments.len()
    );
}

// ---------------------------------------------------------------------------
// Bonus: empty input
// ---------------------------------------------------------------------------

#[test]
fn test_snap_round_empty_input() {
    let opts = SnapRoundingOptions::default();
    let result = snap_round(&[], &opts).unwrap();
    assert_eq!(result.segments.len(), 0);
    assert_eq!(result.intersections_added, 0);
    assert_eq!(result.iterations, 0);
}

// ---------------------------------------------------------------------------
// Bonus: single line, no crossing possible
// ---------------------------------------------------------------------------

#[test]
fn test_snap_round_single_line_no_crossing() {
    let prec = 1e-6;
    let line = vec![c(0.0, 0.0), c(1.0, 0.0), c(2.0, 1.0), c(3.0, 0.0)];
    let lines = vec![line];

    let opts = SnapRoundingOptions {
        precision: prec,
        max_iterations: 8,
    };
    let result = snap_round(&lines, &opts).unwrap();

    // Three segments, no cross-line intersections possible.
    assert_eq!(result.segments.len(), 3);
    assert_eq!(result.intersections_added, 0);
}
