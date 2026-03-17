//! Integration tests for the `validation` module.

use oxigdal_index::{
    Coord, Polygon, Ring, ValidationIssue, validate_no_self_intersection, validate_polygon,
    validate_ring_closure, validate_ring_orientation,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ccw_square() -> Ring {
    Ring::new(vec![
        Coord::new(0.0, 0.0),
        Coord::new(4.0, 0.0),
        Coord::new(4.0, 4.0),
        Coord::new(0.0, 4.0),
        Coord::new(0.0, 0.0),
    ])
}

fn cw_square() -> Ring {
    Ring::new(vec![
        Coord::new(0.0, 0.0),
        Coord::new(0.0, 4.0),
        Coord::new(4.0, 4.0),
        Coord::new(4.0, 0.0),
        Coord::new(0.0, 0.0),
    ])
}

fn ccw_triangle() -> Ring {
    Ring::new(vec![
        Coord::new(0.0, 0.0),
        Coord::new(4.0, 0.0),
        Coord::new(2.0, 3.0),
        Coord::new(0.0, 0.0),
    ])
}

// ---------------------------------------------------------------------------
// Valid polygons
// ---------------------------------------------------------------------------

#[test]
fn valid_square_polygon() {
    let poly = Polygon::simple(ccw_square());
    let res = validate_polygon(&poly);
    assert!(res.is_valid(), "issues: {:?}", res.issues());
}

#[test]
fn valid_triangle_polygon() {
    let poly = Polygon::simple(ccw_triangle());
    let res = validate_polygon(&poly);
    assert!(res.is_valid(), "issues: {:?}", res.issues());
}

#[test]
fn valid_polygon_with_hole() {
    let exterior = ccw_square();
    // CW hole inside the square.
    let hole = Ring::new(vec![
        Coord::new(1.0, 1.0),
        Coord::new(1.0, 3.0),
        Coord::new(3.0, 3.0),
        Coord::new(3.0, 1.0),
        Coord::new(1.0, 1.0),
    ]);
    let poly = Polygon::new(exterior, vec![hole]);
    let res = validate_polygon(&poly);
    assert!(res.is_valid(), "issues: {:?}", res.issues());
}

// ---------------------------------------------------------------------------
// Unclosed ring
// ---------------------------------------------------------------------------

#[test]
fn unclosed_ring_detected() {
    let ring = Ring::new(vec![
        Coord::new(0.0, 0.0),
        Coord::new(1.0, 0.0),
        Coord::new(1.0, 1.0),
        Coord::new(0.0, 1.0),
        // missing closure
    ]);
    assert_eq!(
        validate_ring_closure(&ring),
        Some(ValidationIssue::UnclosedRing)
    );
}

#[test]
fn unclosed_ring_in_polygon() {
    let ring = Ring::new(vec![
        Coord::new(0.0, 0.0),
        Coord::new(1.0, 0.0),
        Coord::new(1.0, 1.0),
        Coord::new(0.0, 1.0),
    ]);
    let poly = Polygon::simple(ring);
    let res = validate_polygon(&poly);
    assert!(res.issues().contains(&ValidationIssue::UnclosedRing));
}

// ---------------------------------------------------------------------------
// Too few points
// ---------------------------------------------------------------------------

#[test]
fn too_few_points_detected() {
    let ring = Ring::new(vec![
        Coord::new(0.0, 0.0),
        Coord::new(1.0, 0.0),
        Coord::new(0.0, 0.0),
    ]);
    let poly = Polygon::simple(ring);
    let res = validate_polygon(&poly);
    assert!(res.issues().contains(&ValidationIssue::TooFewPoints));
}

#[test]
fn too_few_points_single_coord() {
    let ring = Ring::new(vec![Coord::new(0.0, 0.0)]);
    let poly = Polygon::simple(ring);
    let res = validate_polygon(&poly);
    assert!(res.issues().contains(&ValidationIssue::TooFewPoints));
}

// ---------------------------------------------------------------------------
// Self-intersection
// ---------------------------------------------------------------------------

#[test]
fn figure_eight_self_intersection() {
    // Edges 0→1 and 2→3 cross.
    let ring = Ring::new(vec![
        Coord::new(0.0, 0.0),
        Coord::new(2.0, 2.0),
        Coord::new(2.0, 0.0),
        Coord::new(0.0, 2.0),
        Coord::new(0.0, 0.0),
    ]);
    let issues = validate_no_self_intersection(&ring);
    assert!(
        issues
            .iter()
            .any(|i| matches!(i, ValidationIssue::SelfIntersection { .. })),
        "expected self-intersection, got {:?}",
        issues
    );
}

#[test]
fn no_self_intersection_in_square() {
    let issues = validate_no_self_intersection(&ccw_square());
    assert!(issues.is_empty());
}

// ---------------------------------------------------------------------------
// Duplicate consecutive points
// ---------------------------------------------------------------------------

#[test]
fn duplicate_consecutive_detected() {
    let ring = Ring::new(vec![
        Coord::new(0.0, 0.0),
        Coord::new(1.0, 0.0),
        Coord::new(1.0, 0.0), // duplicate
        Coord::new(1.0, 1.0),
        Coord::new(0.0, 1.0),
        Coord::new(0.0, 0.0),
    ]);
    let poly = Polygon::simple(ring);
    let res = validate_polygon(&poly);
    assert!(
        res.issues()
            .iter()
            .any(|i| matches!(i, ValidationIssue::DuplicateConsecutivePoints { .. })),
        "issues: {:?}",
        res.issues()
    );
}

// ---------------------------------------------------------------------------
// Hole orientation
// ---------------------------------------------------------------------------

#[test]
fn invalid_hole_orientation_detected() {
    let exterior = ccw_square();
    // A CCW hole (same as exterior) is invalid.
    let hole = Ring::new(vec![
        Coord::new(1.0, 1.0),
        Coord::new(3.0, 1.0),
        Coord::new(3.0, 3.0),
        Coord::new(1.0, 3.0),
        Coord::new(1.0, 1.0),
    ]);
    let poly = Polygon::new(exterior, vec![hole]);
    let res = validate_polygon(&poly);
    assert!(
        res.issues()
            .contains(&ValidationIssue::InvalidHoleOrientation),
        "issues: {:?}",
        res.issues()
    );
}

// ---------------------------------------------------------------------------
// Hole outside exterior
// ---------------------------------------------------------------------------

#[test]
fn hole_outside_exterior_detected() {
    let exterior = ccw_square();
    // Hole entirely outside the exterior.
    let hole = Ring::new(vec![
        Coord::new(10.0, 10.0),
        Coord::new(10.0, 12.0),
        Coord::new(12.0, 12.0),
        Coord::new(12.0, 10.0),
        Coord::new(10.0, 10.0),
    ]);
    let poly = Polygon::new(exterior, vec![hole]);
    let res = validate_polygon(&poly);
    assert!(
        res.issues().contains(&ValidationIssue::HoleOutsideExterior),
        "issues: {:?}",
        res.issues()
    );
}

// ---------------------------------------------------------------------------
// Zero-area ring
// ---------------------------------------------------------------------------

#[test]
fn zero_area_ring_collinear() {
    let ring = Ring::new(vec![
        Coord::new(0.0, 0.0),
        Coord::new(1.0, 0.0),
        Coord::new(2.0, 0.0),
        Coord::new(0.0, 0.0),
    ]);
    let poly = Polygon::simple(ring);
    let res = validate_polygon(&poly);
    assert!(res.issues().contains(&ValidationIssue::ZeroAreaRing));
}

#[test]
fn zero_area_ring_repeated_point() {
    let ring = Ring::new(vec![
        Coord::new(5.0, 5.0),
        Coord::new(5.0, 5.0),
        Coord::new(5.0, 5.0),
        Coord::new(5.0, 5.0),
    ]);
    let poly = Polygon::simple(ring);
    let res = validate_polygon(&poly);
    assert!(res.issues().contains(&ValidationIssue::ZeroAreaRing));
}

// ---------------------------------------------------------------------------
// Ring orientation helpers
// ---------------------------------------------------------------------------

#[test]
fn orientation_ccw_is_true() {
    assert!(validate_ring_orientation(&ccw_square()));
}

#[test]
fn orientation_cw_is_false() {
    assert!(!validate_ring_orientation(&cw_square()));
}

// ---------------------------------------------------------------------------
// Ring closure helpers
// ---------------------------------------------------------------------------

#[test]
fn ring_closure_valid() {
    assert!(validate_ring_closure(&ccw_square()).is_none());
}

#[test]
fn ring_closure_empty() {
    let ring = Ring::new(vec![]);
    assert_eq!(
        validate_ring_closure(&ring),
        Some(ValidationIssue::UnclosedRing)
    );
}

#[test]
fn ring_closure_single() {
    let ring = Ring::new(vec![Coord::new(0.0, 0.0)]);
    assert_eq!(
        validate_ring_closure(&ring),
        Some(ValidationIssue::UnclosedRing)
    );
}

// ---------------------------------------------------------------------------
// Multiple issues in one polygon
// ---------------------------------------------------------------------------

#[test]
fn multiple_issues_detected() {
    // Unclosed, too few points, zero area — all at once.
    let ring = Ring::new(vec![Coord::new(0.0, 0.0), Coord::new(1.0, 0.0)]);
    let poly = Polygon::simple(ring);
    let res = validate_polygon(&poly);
    assert!(
        res.len() >= 3,
        "expected >=3 issues, got {:?}",
        res.issues()
    );
}

// ---------------------------------------------------------------------------
// Complex valid polygon
// ---------------------------------------------------------------------------

#[test]
fn complex_valid_polygon_two_holes() {
    let exterior = Ring::new(vec![
        Coord::new(0.0, 0.0),
        Coord::new(10.0, 0.0),
        Coord::new(10.0, 10.0),
        Coord::new(0.0, 10.0),
        Coord::new(0.0, 0.0),
    ]);
    // CW hole 1
    let hole1 = Ring::new(vec![
        Coord::new(1.0, 1.0),
        Coord::new(1.0, 4.0),
        Coord::new(4.0, 4.0),
        Coord::new(4.0, 1.0),
        Coord::new(1.0, 1.0),
    ]);
    // CW hole 2
    let hole2 = Ring::new(vec![
        Coord::new(6.0, 6.0),
        Coord::new(6.0, 9.0),
        Coord::new(9.0, 9.0),
        Coord::new(9.0, 6.0),
        Coord::new(6.0, 6.0),
    ]);
    let poly = Polygon::new(exterior, vec![hole1, hole2]);
    let res = validate_polygon(&poly);
    assert!(res.is_valid(), "issues: {:?}", res.issues());
}

// ---------------------------------------------------------------------------
// Pentagon valid polygon
// ---------------------------------------------------------------------------

#[test]
fn valid_pentagon() {
    let ring = Ring::new(vec![
        Coord::new(2.0, 0.0),
        Coord::new(4.0, 1.0),
        Coord::new(3.0, 3.0),
        Coord::new(1.0, 3.0),
        Coord::new(0.0, 1.0),
        Coord::new(2.0, 0.0),
    ]);
    let poly = Polygon::simple(ring);
    let res = validate_polygon(&poly);
    assert!(res.is_valid(), "issues: {:?}", res.issues());
}

// ---------------------------------------------------------------------------
// Validation result API
// ---------------------------------------------------------------------------

#[test]
fn validation_result_api() {
    let mut vr = oxigdal_index::ValidationResult::new();
    assert!(vr.is_valid());
    assert!(vr.is_empty());
    assert_eq!(vr.len(), 0);
    vr.push(ValidationIssue::UnclosedRing);
    assert!(!vr.is_valid());
    assert_eq!(vr.len(), 1);
}

// ---------------------------------------------------------------------------
// Coord basic ops
// ---------------------------------------------------------------------------

#[test]
fn coord_new() {
    let c = Coord::new(1.5, -2.3);
    assert!((c.x - 1.5).abs() < 1e-10);
    assert!((c.y - (-2.3)).abs() < 1e-10);
}

// ---------------------------------------------------------------------------
// Ring API
// ---------------------------------------------------------------------------

#[test]
fn ring_len_and_empty() {
    let r = Ring::new(vec![]);
    assert!(r.is_empty());
    assert_eq!(r.len(), 0);
    let r2 = Ring::new(vec![Coord::new(0.0, 0.0)]);
    assert!(!r2.is_empty());
    assert_eq!(r2.len(), 1);
}
