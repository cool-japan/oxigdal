#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use oxigdal_geojson_stream::types::GeoJsonGeometry;

// ── area ─────────────────────────────────────────────────────────────────────

#[test]
fn test_area_unit_square_polygon() {
    let g = GeoJsonGeometry::Polygon(vec![vec![
        [0.0, 0.0],
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 1.0],
        [0.0, 0.0],
    ]]);
    let a = g.area();
    assert!((a - 1.0).abs() < 1e-12, "expected 1.0, got {a}");
}

#[test]
fn test_area_polygon_with_hole() {
    // 4×4 square minus 2×2 interior = 12
    let exterior = vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0], [0.0, 0.0]];
    let hole = vec![[1.0, 1.0], [3.0, 1.0], [3.0, 3.0], [1.0, 3.0], [1.0, 1.0]];
    let g = GeoJsonGeometry::Polygon(vec![exterior, hole]);
    let a = g.area();
    assert!((a - 12.0).abs() < 1e-12, "expected 12.0, got {a}");
}

#[test]
fn test_area_multipolygon_sums_parts() {
    let sq = |offset: f64| {
        vec![vec![
            [offset, offset],
            [offset + 1.0, offset],
            [offset + 1.0, offset + 1.0],
            [offset, offset + 1.0],
            [offset, offset],
        ]]
    };
    let g = GeoJsonGeometry::MultiPolygon(vec![sq(0.0), sq(5.0)]);
    let a = g.area();
    assert!((a - 2.0).abs() < 1e-12, "expected 2.0, got {a}");
}

#[test]
fn test_area_point_and_linestring_zero() {
    assert_eq!(GeoJsonGeometry::Point([0.0, 0.0]).area(), 0.0);
    assert_eq!(
        GeoJsonGeometry::LineString(vec![[0.0, 0.0], [1.0, 1.0]]).area(),
        0.0
    );
}

// ── length ────────────────────────────────────────────────────────────────────

#[test]
fn test_length_unit_horizontal_line() {
    let g = GeoJsonGeometry::LineString(vec![[0.0, 0.0], [3.0, 0.0]]);
    assert!((g.length() - 3.0).abs() < 1e-12);
}

#[test]
fn test_length_3d_diagonal() {
    // 3-4-5 right triangle in 3-D
    let g = GeoJsonGeometry::LineStringZ(vec![[0.0, 0.0, 0.0], [3.0, 4.0, 0.0]]);
    assert!((g.length() - 5.0).abs() < 1e-12);
}

#[test]
fn test_length_polygon_perimeter() {
    // 1×1 square: perimeter = 4
    let g = GeoJsonGeometry::Polygon(vec![vec![
        [0.0, 0.0],
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 1.0],
        [0.0, 0.0],
    ]]);
    assert!((g.length() - 4.0).abs() < 1e-12);
}

#[test]
fn test_length_point_zero() {
    assert_eq!(GeoJsonGeometry::Point([1.0, 2.0]).length(), 0.0);
}

// ── centroid ─────────────────────────────────────────────────────────────────

#[test]
fn test_centroid_point() {
    let c = GeoJsonGeometry::Point([3.0, 7.0]).centroid().unwrap();
    assert!((c[0] - 3.0).abs() < 1e-12 && (c[1] - 7.0).abs() < 1e-12);
}

#[test]
fn test_centroid_unit_square_polygon() {
    let g = GeoJsonGeometry::Polygon(vec![vec![
        [0.0, 0.0],
        [2.0, 0.0],
        [2.0, 2.0],
        [0.0, 2.0],
        [0.0, 0.0],
    ]]);
    let c = g.centroid().unwrap();
    assert!((c[0] - 1.0).abs() < 1e-12 && (c[1] - 1.0).abs() < 1e-12);
}

#[test]
fn test_centroid_linestring_midpoint() {
    // Horizontal 6-unit segment → midpoint at x=3
    let g = GeoJsonGeometry::LineString(vec![[0.0, 0.0], [6.0, 0.0]]);
    let c = g.centroid().unwrap();
    assert!((c[0] - 3.0).abs() < 1e-12 && c[1].abs() < 1e-12);
}

#[test]
fn test_centroid_null_returns_none() {
    assert!(GeoJsonGeometry::Null.centroid().is_none());
}
