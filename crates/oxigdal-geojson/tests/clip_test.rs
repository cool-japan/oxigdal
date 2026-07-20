//! Tests for bbox clipping (Cohen-Sutherland + Sutherland-Hodgman) in
//! `oxigdal-geojson-stream`.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs, clippy::panic)]

use oxigdal_geojson_stream::{
    ClipBox, GeoJsonGeometry, clip_geometry, clip_linestring, clip_polygon,
};

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Build a `ClipBox` for [0, 0, 10, 10].
fn unit_box() -> ClipBox {
    ClipBox::new(0.0, 0.0, 10.0, 10.0)
}

/// Assert that two f64 values are equal within `1e-9`.
macro_rules! assert_approx {
    ($a:expr, $b:expr) => {
        assert!(
            ($a - $b).abs() < 1e-9,
            "assertion failed: {} ≈ {} (diff = {})",
            $a,
            $b,
            ($a - $b).abs()
        );
    };
}

// ─── Test 1: Point inside clip box returns Some ───────────────────────────────

#[test]
fn test_clip_point_inside_returns_some() {
    let clip = unit_box();
    let geom = GeoJsonGeometry::Point([5.0, 5.0]);
    let result = clip_geometry(&geom, &clip);
    assert!(result.is_some(), "expected Some for point inside clip box");
    match result {
        Some(GeoJsonGeometry::Point([x, y])) => {
            assert_approx!(x, 5.0);
            assert_approx!(y, 5.0);
        }
        other => panic!("expected Point, got {:?}", other),
    }
}

// ─── Test 2: Point outside clip box returns None ──────────────────────────────

#[test]
fn test_clip_point_outside_returns_none() {
    let clip = unit_box();
    let geom = GeoJsonGeometry::Point([15.0, 5.0]);
    let result = clip_geometry(&geom, &clip);
    assert!(result.is_none(), "expected None for point outside clip box");
}

// ─── Test 3: LineString fully inside clip box ─────────────────────────────────

#[test]
fn test_clip_linestring_fully_inside() {
    let clip = unit_box();
    let coords = vec![[1.0, 1.0], [4.0, 4.0], [9.0, 2.0]];
    let parts = clip_linestring(&coords, &clip);

    // The line is entirely within the box — should produce exactly one
    // sub-string with the same coordinates.
    assert_eq!(parts.len(), 1, "should produce one output sub-string");
    assert_eq!(
        parts[0].len(),
        3,
        "output sub-string must preserve all 3 vertices"
    );
    assert_approx!(parts[0][0][0], 1.0);
    assert_approx!(parts[0][2][0], 9.0);
}

// ─── Test 4: LineString crosses boundary (clip to interior) ───────────────────

#[test]
fn test_clip_linestring_crosses_boundary() {
    let clip = unit_box();
    // Horizontal line from (-5, 5) to (15, 5) — crosses left (x=0) and right
    // (x=10) boundaries.
    let coords = vec![[-5.0, 5.0], [15.0, 5.0]];
    let parts = clip_linestring(&coords, &clip);

    assert_eq!(parts.len(), 1, "should produce exactly one clipped segment");
    assert_eq!(parts[0].len(), 2);

    assert_approx!(parts[0][0][0], 0.0);
    assert_approx!(parts[0][0][1], 5.0);
    assert_approx!(parts[0][1][0], 10.0);
    assert_approx!(parts[0][1][1], 5.0);
}

// ─── Test 5: LineString re-enters clip box produces two pieces ────────────────

#[test]
fn test_clip_linestring_two_segments_outside_produces_two_pieces() {
    let clip = unit_box();
    // A line that enters the box, exits through the right edge, passes outside,
    // and re-enters through the bottom-left corner area.
    // Segment 1: (2, 5) → (15, 5)  [exits right]
    // Segment 2: (15, 5) → (15, -5) [entirely right and below]
    // Segment 3: (15, -5) → (5, 5)  [this one re-enters the box]
    let coords = vec![[2.0, 5.0], [15.0, 5.0], [15.0, -5.0], [5.0, 5.0]];
    let parts = clip_linestring(&coords, &clip);

    // We should get at least 1 piece; typically 2 (enter–exit on segment 0,
    // then a new enter on segment 2).
    assert!(
        !parts.is_empty(),
        "expected at least one clipped piece, got {}",
        parts.len()
    );

    // Every vertex in every output part must be inside (or on) the clip box.
    for part in &parts {
        for [x, y] in part {
            assert!(*x >= -1e-9 && *x <= 10.0 + 1e-9, "x={} outside clip box", x);
            assert!(*y >= -1e-9 && *y <= 10.0 + 1e-9, "y={} outside clip box", y);
        }
    }
}

// ─── Test 6: Polygon square fully inside clip box ─────────────────────────────

#[test]
fn test_clip_polygon_square_fully_inside() {
    let clip = unit_box();
    // Unit square well inside the clip box.
    let exterior = vec![[1.0, 1.0], [9.0, 1.0], [9.0, 9.0], [1.0, 9.0], [1.0, 1.0]];
    let rings = vec![exterior];
    let result = clip_polygon(&rings, &clip);

    assert!(
        result.is_some(),
        "polygon fully inside should not be clipped to None"
    );
    let clipped = result.unwrap();
    assert_eq!(clipped.len(), 1, "should have only the exterior ring");
    // 4 corners + closure point = 5 vertices.
    assert_eq!(
        clipped[0].len(),
        5,
        "exterior ring should have 5 vertices (closed)"
    );
}

// ─── Test 7: Large polygon clipped to clip box boundary ──────────────────────

#[test]
fn test_clip_polygon_larger_than_clip_box() {
    let clip = unit_box();
    // 20×20 polygon (−5 to +15 in both dimensions) clipped to [0,0,10,10].
    let exterior = vec![
        [-5.0, -5.0],
        [15.0, -5.0],
        [15.0, 15.0],
        [-5.0, 15.0],
        [-5.0, -5.0],
    ];
    let rings = vec![exterior];
    let result = clip_polygon(&rings, &clip);

    assert!(
        result.is_some(),
        "large polygon should clip to the box, not None"
    );
    let clipped = result.unwrap();
    assert!(!clipped.is_empty());

    // The resulting exterior should be the clip box itself (10×10 square).
    // Every vertex must lie on or within the clip box boundary.
    for [x, y] in &clipped[0] {
        assert!(*x >= -1e-9 && *x <= 10.0 + 1e-9, "x={} out of range", x);
        assert!(*y >= -1e-9 && *y <= 10.0 + 1e-9, "y={} out of range", y);
    }

    // Approximate area check: the result should be close to 100 (10×10).
    let area = polygon_area(&clipped[0]);
    assert!(
        area > 90.0 && area < 110.0,
        "area of clipped polygon ({}) should be ~100",
        area
    );
}

// ─── Test 8: clip_geometry dispatch for outside Point returns None ─────────────

#[test]
fn test_clip_geometry_dispatch_point_outside() {
    let clip = unit_box();
    // Point far outside the clip box.
    let geom = GeoJsonGeometry::Point([-100.0, -100.0]);
    let result = clip_geometry(&geom, &clip);
    assert!(
        result.is_none(),
        "outside point via clip_geometry should return None"
    );
}

// ─── Test 9: MultiLineString — outside parts filtered ─────────────────────────

#[test]
fn test_clip_multilinestring_filters_outside_parts() {
    let clip = unit_box();
    // Three lines: one inside, one outside, one crossing.
    let inside_line = vec![[1.0, 1.0], [9.0, 9.0]];
    let outside_line = vec![[20.0, 20.0], [30.0, 30.0]];
    let crossing_line = vec![[-5.0, 5.0], [15.0, 5.0]];

    let geom = GeoJsonGeometry::MultiLineString(vec![inside_line, outside_line, crossing_line]);
    let result = clip_geometry(&geom, &clip);

    assert!(
        result.is_some(),
        "at least two lines overlap the box — should not be None"
    );

    match result {
        Some(GeoJsonGeometry::MultiLineString(parts)) => {
            // Outside line should have been dropped — expect 2 parts.
            assert_eq!(
                parts.len(),
                2,
                "should have 2 surviving parts (inside + crossing), got {}",
                parts.len()
            );
        }
        other => panic!("expected MultiLineString, got {:?}", other),
    }
}

// ─── Additional tests ─────────────────────────────────────────────────────────

#[test]
fn test_clip_null_geometry_returns_none() {
    let clip = unit_box();
    let geom = GeoJsonGeometry::Null;
    assert!(clip_geometry(&geom, &clip).is_none());
}

#[test]
fn test_clip_multipoint_filters_outside() {
    let clip = unit_box();
    let geom = GeoJsonGeometry::MultiPoint(vec![
        [5.0, 5.0],  // inside
        [15.0, 5.0], // outside
        [3.0, 7.0],  // inside
    ]);
    let result = clip_geometry(&geom, &clip);
    match result {
        Some(GeoJsonGeometry::MultiPoint(pts)) => {
            assert_eq!(pts.len(), 2, "only 2 points should be inside");
        }
        other => panic!("expected MultiPoint, got {:?}", other),
    }
}

#[test]
fn test_clip_multipolygon_drops_outside_polygons() {
    let clip = unit_box();
    let inside_poly = vec![vec![
        [1.0, 1.0],
        [9.0, 1.0],
        [9.0, 9.0],
        [1.0, 9.0],
        [1.0, 1.0],
    ]];
    let outside_poly = vec![vec![
        [20.0, 20.0],
        [30.0, 20.0],
        [30.0, 30.0],
        [20.0, 30.0],
        [20.0, 20.0],
    ]];
    let geom = GeoJsonGeometry::MultiPolygon(vec![inside_poly, outside_poly]);
    let result = clip_geometry(&geom, &clip);
    match result {
        Some(GeoJsonGeometry::MultiPolygon(polys)) => {
            assert_eq!(polys.len(), 1, "only the inside polygon should survive");
        }
        other => panic!("expected MultiPolygon, got {:?}", other),
    }
}

#[test]
fn test_clip_geometry_collection() {
    let clip = unit_box();
    let inside = GeoJsonGeometry::Point([5.0, 5.0]);
    let outside = GeoJsonGeometry::Point([50.0, 50.0]);
    let geom = GeoJsonGeometry::GeometryCollection(vec![inside, outside]);
    let result = clip_geometry(&geom, &clip);
    match result {
        Some(GeoJsonGeometry::GeometryCollection(children)) => {
            assert_eq!(children.len(), 1, "only 1 child should survive");
        }
        other => panic!("expected GeometryCollection, got {:?}", other),
    }
}

#[test]
fn test_clip_linestring_single_point_is_empty() {
    // A degenerate linestring with a single coordinate is not a segment and
    // should produce no output.
    let clip = unit_box();
    let coords = vec![[5.0, 5.0]];
    let parts = clip_linestring(&coords, &clip);
    assert!(
        parts.is_empty(),
        "single-point input should produce no segments"
    );
}

#[test]
fn test_clip_polygon_ring_degenerate_becomes_empty() {
    use oxigdal_geojson_stream::clip::clip_polygon_ring;
    let clip = unit_box();
    // A ring that collapses to a line segment after clipping.
    // Line along x = 0 (boundary), which produces a degenerate polygon.
    let ring = vec![
        [20.0, 5.0],
        [20.0, 6.0],
        [20.0, 5.0], // back to start — zero area
    ];
    let clipped = clip_polygon_ring(&ring, &clip);
    // Should be empty since the resulting polygon has no area.
    // (It's entirely outside or degenerate.)
    assert!(
        clipped.is_empty(),
        "degenerate / outside ring should be empty"
    );
}

#[test]
fn test_clip_point_on_boundary_is_inside() {
    let clip = unit_box();
    // Points exactly on the clip box boundary should be accepted.
    for pt in [[0.0, 5.0], [10.0, 5.0], [5.0, 0.0], [5.0, 10.0]] {
        let geom = GeoJsonGeometry::Point(pt);
        assert!(
            clip_geometry(&geom, &clip).is_some(),
            "boundary point {:?} should be inside",
            pt
        );
    }
}

// ─── Helper: signed area of a closed polygon ring ────────────────────────────

fn polygon_area(ring: &[[f64; 2]]) -> f64 {
    let n = ring.len();
    if n < 3 {
        return 0.0;
    }
    let mut area = 0.0_f64;
    for i in 0..n {
        let [x0, y0] = ring[i];
        let [x1, y1] = ring[(i + 1) % n];
        area += x0 * y1 - x1 * y0;
    }
    area.abs() / 2.0
}
