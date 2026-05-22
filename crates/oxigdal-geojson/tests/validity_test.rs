//! Integration tests for `oxigdal-geojson-stream` geometry validity module.

use oxigdal_geojson_stream::{
    GeoJsonGeometry, GeometryValidityIssue, WindingOrder, check_ring_self_intersection,
    fix_geometry_winding, fix_ring_winding, ring_signed_area, ring_winding_order,
    validate_geometry, validate_polygon_rings,
};

// ─── Fixtures ─────────────────────────────────────────────────────────────────

/// Unit square traversed counter-clockwise: (0,0)→(1,0)→(1,1)→(0,1)→(0,0).
///
/// Signed area = +1.0 → CCW.
fn ccw_square() -> Vec<[f64; 2]> {
    vec![[0., 0.], [1., 0.], [1., 1.], [0., 1.], [0., 0.]]
}

/// Unit square traversed clockwise: (0,0)→(0,1)→(1,1)→(1,0)→(0,0).
///
/// Signed area = -1.0 → CW.
fn cw_square() -> Vec<[f64; 2]> {
    vec![[0., 0.], [0., 1.], [1., 1.], [1., 0.], [0., 0.]]
}

/// Bowtie (figure-eight) ring: the two diagonal segments cross at (0.5, 0.5).
fn bowtie() -> Vec<[f64; 2]> {
    vec![[0., 0.], [1., 1.], [1., 0.], [0., 1.], [0., 0.]]
}

// ─── ring_winding_order ────────────────────────────────────────────────────────

#[test]
fn test_ring_winding_ccw_square() {
    assert_eq!(
        ring_winding_order(&ccw_square()),
        WindingOrder::CounterClockwise
    );
}

#[test]
fn test_ring_winding_cw_square() {
    assert_eq!(ring_winding_order(&cw_square()), WindingOrder::Clockwise);
}

#[test]
fn test_ring_winding_degenerate_collinear() {
    // All four distinct points are collinear on the x-axis → zero area.
    let collinear: Vec<[f64; 2]> = vec![[0., 0.], [1., 0.], [2., 0.], [3., 0.], [0., 0.]];
    assert_eq!(ring_winding_order(&collinear), WindingOrder::Degenerate);
}

// ─── ring_signed_area ─────────────────────────────────────────────────────────

#[test]
fn test_ring_signed_area_unit_square() {
    // The CCW unit square has a signed area of exactly +1.0.
    let area = ring_signed_area(&ccw_square());
    assert!((area - 1.0).abs() < 1e-10, "expected 1.0 but got {area}");
}

#[test]
fn test_ring_signed_area_cw_is_negative() {
    let area = ring_signed_area(&cw_square());
    assert!(
        area < 0.0,
        "CW ring must have negative signed area, got {area}"
    );
    assert!((area + 1.0).abs() < 1e-10, "expected -1.0 but got {area}");
}

// ─── check_ring_self_intersection ─────────────────────────────────────────────

#[test]
fn test_check_ring_self_intersection_simple_square_none() {
    assert!(
        check_ring_self_intersection(&ccw_square()).is_none(),
        "simple square must not self-intersect"
    );
}

#[test]
fn test_check_ring_self_intersection_bowtie_detected() {
    assert!(
        check_ring_self_intersection(&bowtie()).is_some(),
        "bowtie ring must be detected as self-intersecting"
    );
}

// ─── validate_polygon_rings ────────────────────────────────────────────────────

#[test]
fn test_validate_polygon_exterior_ccw_no_issues() {
    let issues = validate_polygon_rings(&[ccw_square()]);
    assert!(
        issues.is_empty(),
        "CCW exterior ring should produce no issues, got: {issues:?}"
    );
}

#[test]
fn test_validate_polygon_exterior_cw_reports_incorrect_winding() {
    let issues = validate_polygon_rings(&[cw_square()]);
    assert!(
        issues
            .iter()
            .any(|i| matches!(i, GeometryValidityIssue::IncorrectWinding { .. })),
        "CW exterior must report IncorrectWinding, got: {issues:?}"
    );
}

#[test]
fn test_validate_polygon_hole_ccw_reports_incorrect_winding() {
    // Exterior at ring_index 0 (CCW = correct), hole at ring_index 1 (CCW = wrong).
    let issues = validate_polygon_rings(&[ccw_square(), ccw_square()]);
    assert!(
        issues.iter().any(|i| matches!(
            i,
            GeometryValidityIssue::IncorrectWinding {
                ring_index: 1,
                expected: WindingOrder::Clockwise,
                ..
            }
        )),
        "CCW hole must report IncorrectWinding at ring_index 1, got: {issues:?}"
    );
}

#[test]
fn test_validate_polygon_unclosed_ring_reports() {
    let mut r = ccw_square();
    r.pop(); // remove the closing vertex — ring is now open
    let issues = validate_polygon_rings(&[r]);
    assert!(
        issues
            .iter()
            .any(|i| matches!(i, GeometryValidityIssue::UnclosedRing { ring_index: 0 })),
        "open ring must report UnclosedRing, got: {issues:?}"
    );
}

#[test]
fn test_validate_polygon_too_few_vertices_reports() {
    // 3 vertices is below the minimum of 4.
    let r: Vec<[f64; 2]> = vec![[0., 0.], [1., 1.], [0., 0.]];
    let issues = validate_polygon_rings(&[r]);
    assert!(
        issues
            .iter()
            .any(|i| matches!(i, GeometryValidityIssue::FewerThanMinimumVertices { .. })),
        "ring with 3 vertices must report FewerThanMinimumVertices, got: {issues:?}"
    );
}

// ─── validate_geometry ────────────────────────────────────────────────────────

#[test]
fn test_validate_geometry_point_always_valid() {
    let geom = GeoJsonGeometry::Point([1.0, 2.0]);
    let report = validate_geometry(&geom);
    assert!(report.is_valid, "Point geometry must always be valid");
}

#[test]
fn test_validate_geometry_linestring_always_valid() {
    let geom = GeoJsonGeometry::LineString(vec![[0., 0.], [1., 1.]]);
    assert!(validate_geometry(&geom).is_valid);
}

#[test]
fn test_validate_geometry_null_valid() {
    assert!(validate_geometry(&GeoJsonGeometry::Null).is_valid);
}

#[test]
fn test_validate_geometry_valid_polygon() {
    let geom = GeoJsonGeometry::Polygon(vec![ccw_square()]);
    let report = validate_geometry(&geom);
    assert!(
        report.is_valid,
        "CCW polygon must be valid, issues: {:?}",
        report.issues
    );
}

#[test]
fn test_validate_geometry_invalid_polygon_cw_exterior() {
    let geom = GeoJsonGeometry::Polygon(vec![cw_square()]);
    let report = validate_geometry(&geom);
    assert!(!report.is_valid, "CW exterior polygon must be invalid");
}

// ─── fix_ring_winding ─────────────────────────────────────────────────────────

#[test]
fn test_fix_ring_winding_reverses_when_mismatched() {
    let mut r = cw_square();
    fix_ring_winding(&mut r, WindingOrder::CounterClockwise);
    assert_eq!(
        ring_winding_order(&r),
        WindingOrder::CounterClockwise,
        "ring must become CCW after fix"
    );
}

#[test]
fn test_fix_ring_winding_no_op_when_already_correct() {
    let original = ccw_square();
    let mut r = original.clone();
    fix_ring_winding(&mut r, WindingOrder::CounterClockwise);
    assert_eq!(r, original, "ring must be unchanged if already CCW");
}

// ─── fix_geometry_winding ─────────────────────────────────────────────────────

#[test]
fn test_fix_geometry_winding_corrects_polygon_with_hole() {
    // CW exterior (wrong) + CCW hole (wrong)
    let exterior = cw_square();
    let hole = ccw_square();
    let mut geom = GeoJsonGeometry::Polygon(vec![exterior, hole]);

    fix_geometry_winding(&mut geom);

    let GeoJsonGeometry::Polygon(rings) = &geom else {
        unreachable!("we constructed a Polygon, so this branch is never reached");
    };
    assert_eq!(
        ring_winding_order(&rings[0]),
        WindingOrder::CounterClockwise,
        "exterior ring must be CCW after fix"
    );
    assert_eq!(
        ring_winding_order(&rings[1]),
        WindingOrder::Clockwise,
        "hole ring must be CW after fix"
    );
}

#[test]
fn test_fix_geometry_winding_multipolygon_corrects_all() {
    let poly1 = vec![cw_square()];
    let poly2 = vec![cw_square()];
    let mut geom = GeoJsonGeometry::MultiPolygon(vec![poly1, poly2]);

    fix_geometry_winding(&mut geom);

    let GeoJsonGeometry::MultiPolygon(polys) = &geom else {
        unreachable!("we constructed a MultiPolygon, so this branch is never reached");
    };
    for (i, poly) in polys.iter().enumerate() {
        assert_eq!(
            ring_winding_order(&poly[0]),
            WindingOrder::CounterClockwise,
            "polygon {i} exterior must be CCW after fix"
        );
    }
}
