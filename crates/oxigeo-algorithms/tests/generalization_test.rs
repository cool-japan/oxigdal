//! Integration tests for map generalization operators.
//!
//! Covers: collapse (polygon + linestring), exaggerate (coord, linestring, polygon),
//! displace (points + polygon centroid method).  All tests are pure – no I/O.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use oxigeo_algorithms::{
    CollapseOptions, Coordinate, DisplaceOptions, ExaggerateAnchor, ExaggerateOptions, LineString,
    Polygon, collapse_linestring_to_point, collapse_polygon_to_point, displace_points,
    displace_polygons_by_centroid, exaggerate_coord, exaggerate_polygon, should_collapse_polygon,
};

// ── Test helpers ──────────────────────────────────────────────────────────────

/// Constructs an axis-aligned square with lower-left corner at `(x0, y0)` and
/// side length `side`.
fn make_square(x0: f64, y0: f64, side: f64) -> Polygon {
    let coords = vec![
        Coordinate::new_2d(x0, y0),
        Coordinate::new_2d(x0 + side, y0),
        Coordinate::new_2d(x0 + side, y0 + side),
        Coordinate::new_2d(x0, y0 + side),
        Coordinate::new_2d(x0, y0),
    ];
    let ext = LineString::new(coords).expect("valid exterior ring");
    Polygon::new(ext, vec![]).expect("valid polygon")
}

/// Constructs a two-point horizontal linestring from `(x0,0)` to `(x1,0)`.
fn make_hline(x0: f64, x1: f64) -> LineString {
    let coords = vec![Coordinate::new_2d(x0, 0.0), Coordinate::new_2d(x1, 0.0)];
    LineString::new(coords).expect("valid linestring")
}

fn make_coord(x: f64, y: f64) -> Coordinate {
    Coordinate::new_2d(x, y)
}

// ── Test 1: Small polygon collapses to centroid ───────────────────────────────

#[test]
fn test_collapse_small_polygon_returns_centroid() {
    // Unit square: area = 1.0 < min_area = 2.0 → should collapse.
    let poly = make_square(0.0, 0.0, 1.0);
    let opts = CollapseOptions::new(2.0);

    let result = collapse_polygon_to_point(&poly, &opts);
    assert!(
        result.is_some(),
        "unit square should collapse when min_area = 2.0"
    );

    let c = result.unwrap();
    assert!(
        (c.x - 0.5).abs() < 1e-10,
        "centroid x should be 0.5, got {}",
        c.x
    );
    assert!(
        (c.y - 0.5).abs() < 1e-10,
        "centroid y should be 0.5, got {}",
        c.y
    );
}

// ── Test 2: Large polygon is not collapsed ────────────────────────────────────

#[test]
fn test_collapse_large_polygon_returns_none() {
    // 10×10 square: area = 100.0 ≥ min_area = 1.0 → should NOT collapse.
    let poly = make_square(0.0, 0.0, 10.0);
    let opts = CollapseOptions::new(1.0);

    let result = collapse_polygon_to_point(&poly, &opts);
    assert!(
        result.is_none(),
        "10×10 square should not collapse with min_area = 1.0"
    );
}

// ── Test 3: Zero threshold never collapses any polygon ────────────────────────

#[test]
fn test_collapse_threshold_zero_keeps_all() {
    // Any positive-area polygon should survive min_area = 0.0.
    let tiny = make_square(0.0, 0.0, 0.001);
    let opts = CollapseOptions::new(0.0);

    let result = collapse_polygon_to_point(&tiny, &opts);
    assert!(
        result.is_none(),
        "no polygon should collapse when min_area = 0.0"
    );
}

// ── Test 4: should_collapse_polygon below threshold ───────────────────────────

#[test]
fn test_should_collapse_polygon_below_threshold() {
    let poly = make_square(0.0, 0.0, 0.5); // area = 0.25
    let opts = CollapseOptions::new(1.0);

    assert!(
        should_collapse_polygon(&poly, &opts),
        "polygon with area 0.25 should be flagged for collapse at threshold 1.0"
    );
}

// ── Test 5: Short linestring collapses to midpoint ────────────────────────────

#[test]
fn test_collapse_linestring_short_returns_midpoint() {
    // Line from 0 to 1 has length 1.0; threshold is 2.0 → collapse.
    let line = make_hline(0.0, 1.0);
    let opts = CollapseOptions::default().with_min_linestring_length(2.0);

    let result = collapse_linestring_to_point(&line, &opts);
    assert!(
        result.is_some(),
        "line of length 1.0 should collapse when min_len = 2.0"
    );

    let mid = result.unwrap();
    assert!((mid.x - 0.5).abs() < 1e-10, "midpoint x should be 0.5");
    assert!((mid.y - 0.0).abs() < 1e-10, "midpoint y should be 0.0");
}

// ── Test 6: Long linestring is not collapsed ──────────────────────────────────

#[test]
fn test_collapse_linestring_long_returns_none() {
    // Line from 0 to 10 has length 10.0; threshold is 0.5 → no collapse.
    let line = make_hline(0.0, 10.0);
    let opts = CollapseOptions::default().with_min_linestring_length(0.5);

    let result = collapse_linestring_to_point(&line, &opts);
    assert!(
        result.is_none(),
        "line of length 10.0 should not collapse with min_len = 0.5"
    );
}

// ── Test 7: exaggerate_coord doubles offset from anchor ───────────────────────

#[test]
fn test_exaggerate_coord_scale_2x_doubles_offset_from_anchor() {
    let anchor = make_coord(0.0, 0.0);
    let point = make_coord(1.0, 0.0);
    let result = exaggerate_coord(&point, &anchor, 2.0);

    assert!(
        (result.x - 2.0).abs() < 1e-12,
        "x should be 2.0, got {}",
        result.x
    );
    assert!(
        (result.y - 0.0).abs() < 1e-12,
        "y should be 0.0, got {}",
        result.y
    );
}

// ── Test 8: exaggerate_coord at scale 1 is identity ──────────────────────────

#[test]
fn test_exaggerate_coord_scale_1_returns_input() {
    let anchor = make_coord(3.0, 7.0);
    let point = make_coord(5.0, 2.0);
    let result = exaggerate_coord(&point, &anchor, 1.0);

    assert!((result.x - point.x).abs() < 1e-12, "x unchanged at scale=1");
    assert!((result.y - point.y).abs() < 1e-12, "y unchanged at scale=1");
}

// ── Test 9: exaggerate_polygon preserves ring count ───────────────────────────

#[test]
fn test_exaggerate_polygon_preserves_ring_count() {
    // Outer square 0..10, inner hole 2..8 (4×4 inner square closed ring).
    let ext_coords = vec![
        Coordinate::new_2d(0.0, 0.0),
        Coordinate::new_2d(10.0, 0.0),
        Coordinate::new_2d(10.0, 10.0),
        Coordinate::new_2d(0.0, 10.0),
        Coordinate::new_2d(0.0, 0.0),
    ];
    let hole_coords = vec![
        Coordinate::new_2d(2.0, 2.0),
        Coordinate::new_2d(8.0, 2.0),
        Coordinate::new_2d(8.0, 8.0),
        Coordinate::new_2d(2.0, 8.0),
        Coordinate::new_2d(2.0, 2.0),
    ];
    let exterior = LineString::new(ext_coords).expect("valid exterior");
    let hole = LineString::new(hole_coords).expect("valid hole");
    let poly = Polygon::new(exterior, vec![hole]).expect("valid donut polygon");

    let opts = ExaggerateOptions {
        scale_factor: 1.5,
        anchor: ExaggerateAnchor::Centroid,
    };
    let result = exaggerate_polygon(&poly, &opts);

    assert_eq!(
        result.interiors().len(),
        1,
        "exaggerated polygon must retain exactly one interior ring"
    );
    assert_eq!(
        result.exterior().coords.len(),
        poly.exterior().coords.len(),
        "exterior ring coordinate count must be unchanged"
    );
}

// ── Test 10: exaggerate_polygon centroid anchor doubles extent ────────────────

#[test]
fn test_exaggerate_polygon_centroid_anchor_doubles_extent() {
    // Unit square centred at (0.5, 0.5).  With scale=2 the corners should be
    // at distance 2× from the centroid, giving range [-0.5, 1.5] in each axis.
    let poly = make_square(0.0, 0.0, 1.0);
    let opts = ExaggerateOptions {
        scale_factor: 2.0,
        anchor: ExaggerateAnchor::Centroid,
    };
    let result = exaggerate_polygon(&poly, &opts);

    // Compute bounding box of the exaggerated exterior ring.
    let coords = result.exterior().coords();
    let min_x = coords.iter().map(|c| c.x).fold(f64::INFINITY, f64::min);
    let max_x = coords.iter().map(|c| c.x).fold(f64::NEG_INFINITY, f64::max);
    let min_y = coords.iter().map(|c| c.y).fold(f64::INFINITY, f64::min);
    let max_y = coords.iter().map(|c| c.y).fold(f64::NEG_INFINITY, f64::max);

    // Original extent [0, 1]; doubled gives [-0.5, 1.5].
    assert!(
        (min_x - (-0.5)).abs() < 1e-10,
        "min_x should be -0.5, got {}",
        min_x
    );
    assert!(
        (max_x - 1.5).abs() < 1e-10,
        "max_x should be 1.5, got {}",
        max_x
    );
    assert!(
        (min_y - (-0.5)).abs() < 1e-10,
        "min_y should be -0.5, got {}",
        min_y
    );
    assert!(
        (max_y - 1.5).abs() < 1e-10,
        "max_y should be 1.5, got {}",
        max_y
    );
}

// ── Test 11: Two overlapping points are pushed apart ─────────────────────────

#[test]
fn test_displace_two_overlapping_points_pushed_apart() {
    let mut positions = vec![
        make_coord(0.0, 0.0),
        make_coord(0.1, 0.0), // distance = 0.1, violates min_distance = 1.0
    ];
    let opts = DisplaceOptions {
        min_distance: 1.0,
        max_iterations: 200,
        damping: 0.5,
        convergence_tol: 1e-9,
    };

    displace_points(&mut positions, &opts);

    let dx = positions[1].x - positions[0].x;
    let dy = positions[1].y - positions[0].y;
    let dist = (dx * dx + dy * dy).sqrt();

    assert!(
        dist >= 0.99, // allow tiny floating-point undershoot
        "after displacement, distance should be ≥ 1.0; got {dist:.6}"
    );
}

// ── Test 12: Already-separated points are not displaced ───────────────────────

#[test]
fn test_displace_already_separated_points_unchanged() {
    let initial = vec![
        make_coord(0.0, 0.0),
        make_coord(10.0, 0.0), // distance = 10.0 >> min_distance = 1.0
    ];
    let mut positions = initial.clone();
    let opts = DisplaceOptions {
        min_distance: 1.0,
        ..Default::default()
    };

    displace_points(&mut positions, &opts);

    // Positions should be essentially unchanged (no repulsion force applied).
    for (orig, moved) in initial.iter().zip(positions.iter()) {
        let d = ((moved.x - orig.x).powi(2) + (moved.y - orig.y).powi(2)).sqrt();
        assert!(
            d < 1e-12,
            "already-separated point should not move; displacement was {d:.2e}"
        );
    }
}

// ── Test 13: Displacement converges within max iterations ────────────────────

#[test]
fn test_displace_converges_within_max_iter() {
    // Three points in a tight cluster; solver should converge.
    let mut positions = vec![
        make_coord(0.0, 0.0),
        make_coord(0.05, 0.0),
        make_coord(0.0, 0.05),
    ];
    let opts = DisplaceOptions {
        min_distance: 1.0,
        max_iterations: 500,
        damping: 0.3,
        convergence_tol: 1e-6,
    };

    let stats = displace_points(&mut positions, &opts);

    // The solver must not have used more iterations than allowed.
    assert!(
        stats.iterations <= 500,
        "solver must not exceed max_iterations; got {}",
        stats.iterations
    );

    // All pairwise distances should be >= min_distance (with small tolerance).
    for i in 0..positions.len() {
        for j in (i + 1)..positions.len() {
            let dx = positions[j].x - positions[i].x;
            let dy = positions[j].y - positions[i].y;
            let dist = (dx * dx + dy * dy).sqrt();
            assert!(
                dist >= 0.95,
                "after displacement, pair ({i},{j}) distance should be ≥ 1.0; got {dist:.4}"
            );
        }
    }
}

// ── Test 14: displace_polygons_by_centroid separates overlapping polygons ─────

#[test]
fn test_displace_polygons_translates_by_centroid_delta() {
    // Two unit squares whose centroids are only 0.1 apart (violates min_distance=1.0).
    let mut polys = vec![
        make_square(0.0, 0.0, 1.0), // centroid at (0.5, 0.5)
        make_square(0.1, 0.0, 1.0), // centroid at (0.6, 0.5)
    ];

    let opts = DisplaceOptions {
        min_distance: 1.0,
        max_iterations: 200,
        damping: 0.5,
        convergence_tol: 1e-9,
    };

    displace_polygons_by_centroid(&mut polys, &opts);

    // Recompute centroids of the (now displaced) polygons.
    let c0 = {
        let ext = polys[0].exterior().coords();
        let n = ext.len() as f64;
        let sx: f64 = ext.iter().map(|c| c.x).sum();
        let sy: f64 = ext.iter().map(|c| c.y).sum();
        (sx / n, sy / n)
    };
    let c1 = {
        let ext = polys[1].exterior().coords();
        let n = ext.len() as f64;
        let sx: f64 = ext.iter().map(|c| c.x).sum();
        let sy: f64 = ext.iter().map(|c| c.y).sum();
        (sx / n, sy / n)
    };

    let dx = c1.0 - c0.0;
    let dy = c1.1 - c0.1;
    let centroid_dist = (dx * dx + dy * dy).sqrt();

    assert!(
        centroid_dist >= 0.95,
        "centroid distance after displacement should be ≥ 1.0; got {centroid_dist:.4}"
    );
}
