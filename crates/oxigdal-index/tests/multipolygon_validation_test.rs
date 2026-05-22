//! Integration tests for [`validate_multipolygon`].
//!
//! Covers:
//!
//! * Disjoint parts (valid)
//! * Edge-sharing parts (valid — typical coverage / tiling case)
//! * Single-vertex touching parts (valid)
//! * Interior-overlapping parts (invalid: `PartsOverlapInterior`)
//! * Empty multi-polygon (valid)
//! * Per-part forwarding of `validate_polygon` issues
//! * Multi-part disjoint configurations

use oxigdal_index::{
    Coord, MultiPolygon, Polygon, Ring, ValidationIssue, validate_multipolygon, validate_polygon,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a closed CCW axis-aligned square as a [`Polygon`] with corner
/// `(min_x, min_y)` and side length `side`.
fn ccw_square(min_x: f64, min_y: f64, side: f64) -> Polygon {
    let max_x = min_x + side;
    let max_y = min_y + side;
    let ring = Ring::new(vec![
        Coord::new(min_x, min_y),
        Coord::new(max_x, min_y),
        Coord::new(max_x, max_y),
        Coord::new(min_x, max_y),
        Coord::new(min_x, min_y),
    ]);
    Polygon::simple(ring)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn test_multipolygon_two_disjoint_squares_valid() {
    let a = ccw_square(0.0, 0.0, 1.0);
    let b = ccw_square(5.0, 5.0, 1.0);
    let mp = MultiPolygon::new(vec![a, b]);
    let res = validate_multipolygon(&mp);
    assert!(res.is_valid(), "issues: {:?}", res.issues());
}

#[test]
fn test_multipolygon_two_squares_sharing_edge_valid() {
    // Left square (0,0)-(1,1) and right square (1,0)-(2,1) share the edge x=1.
    let a = ccw_square(0.0, 0.0, 1.0);
    let b = ccw_square(1.0, 0.0, 1.0);
    let mp = MultiPolygon::new(vec![a, b]);
    let res = validate_multipolygon(&mp);
    // CCW exteriors traverse the shared edge in opposite directions, so no
    // PartsOverlapInterior / SharedEdgeUsesOppositeOrientation should fire.
    assert!(
        res.is_valid(),
        "edge-sharing CCW squares should validate; issues: {:?}",
        res.issues()
    );
}

#[test]
fn test_multipolygon_two_squares_overlapping_interiors_invalid() {
    // (0,0)-(2,2) and (1,1)-(3,3) overlap on (1,1)-(2,2).
    let a = ccw_square(0.0, 0.0, 2.0);
    let b = ccw_square(1.0, 1.0, 2.0);
    let mp = MultiPolygon::new(vec![a, b]);
    let res = validate_multipolygon(&mp);
    assert!(!res.is_valid(), "expected overlap to be detected");
    assert!(
        res.issues().iter().any(|i| matches!(
            i,
            ValidationIssue::PartsOverlapInterior {
                part_a: 0,
                part_b: 1
            }
        )),
        "expected PartsOverlapInterior(0, 1); got {:?}",
        res.issues()
    );
}

#[test]
fn test_multipolygon_two_squares_sharing_one_vertex_valid() {
    // Square at (0,0)-(1,1) and square at (1,1)-(2,2): they touch at (1,1).
    let a = ccw_square(0.0, 0.0, 1.0);
    let b = ccw_square(1.0, 1.0, 1.0);
    let mp = MultiPolygon::new(vec![a, b]);
    let res = validate_multipolygon(&mp);
    assert!(
        res.is_valid(),
        "single-vertex touch should validate; issues: {:?}",
        res.issues()
    );
}

#[test]
fn test_multipolygon_single_part_validates_via_validate_polygon() {
    let p = ccw_square(0.0, 0.0, 1.0);
    let p_clone = p.clone();
    let mp = MultiPolygon::new(vec![p]);
    let mp_res = validate_multipolygon(&mp);
    let poly_res = validate_polygon(&p_clone);
    assert_eq!(mp_res.len(), poly_res.len());
    assert_eq!(mp_res.is_valid(), poly_res.is_valid());
    assert_eq!(mp_res.issues(), poly_res.issues());
}

#[test]
fn test_multipolygon_empty_returns_valid() {
    let mp = MultiPolygon::new(Vec::new());
    let res = validate_multipolygon(&mp);
    assert!(res.is_valid());
    assert_eq!(res.len(), 0);
    assert!(mp.is_empty());
    assert_eq!(mp.len(), 0);
    assert!(mp.parts().is_empty());
}

#[test]
fn test_multipolygon_part_with_invalid_ring_fails() {
    // Unclosed exterior on the second part.
    let good = ccw_square(0.0, 0.0, 1.0);
    let bad_ring = Ring::new(vec![
        Coord::new(5.0, 5.0),
        Coord::new(6.0, 5.0),
        Coord::new(6.0, 6.0),
        Coord::new(5.0, 6.0),
        // missing closure
    ]);
    let bad = Polygon::simple(bad_ring);
    let mp = MultiPolygon::new(vec![good, bad]);
    let res = validate_multipolygon(&mp);
    assert!(!res.is_valid());
    assert!(
        res.issues()
            .iter()
            .any(|i| matches!(i, ValidationIssue::UnclosedRing)),
        "expected UnclosedRing; got {:?}",
        res.issues()
    );
}

#[test]
fn test_multipolygon_three_parts_no_overlap_valid() {
    let a = ccw_square(0.0, 0.0, 1.0);
    let b = ccw_square(5.0, 0.0, 1.0);
    let c = ccw_square(0.0, 5.0, 1.0);
    let mp = MultiPolygon::new(vec![a, b, c]);
    let res = validate_multipolygon(&mp);
    assert!(res.is_valid(), "issues: {:?}", res.issues());
}

#[test]
fn test_multipolygon_one_part_contains_another_invalid() {
    // Outer square completely contains the inner square: classic interior
    // overlap.
    let outer = ccw_square(0.0, 0.0, 10.0);
    let inner = ccw_square(2.0, 2.0, 3.0);
    let mp = MultiPolygon::new(vec![outer, inner]);
    let res = validate_multipolygon(&mp);
    assert!(!res.is_valid(), "containment should be flagged");
    assert!(
        res.issues()
            .iter()
            .any(|i| matches!(i, ValidationIssue::PartsOverlapInterior { .. })),
        "expected PartsOverlapInterior; got {:?}",
        res.issues()
    );
}

#[test]
fn test_multipolygon_parts_accessor_round_trip() {
    let a = ccw_square(0.0, 0.0, 1.0);
    let b = ccw_square(2.0, 2.0, 1.0);
    let mp = MultiPolygon::new(vec![a.clone(), b.clone()]);
    assert_eq!(mp.len(), 2);
    assert_eq!(mp.parts().len(), 2);
    assert_eq!(mp.parts()[0], a);
    assert_eq!(mp.parts()[1], b);
}
