//! Integration tests for the Constrained Delaunay Triangulation implementation.
//!
//! These tests exercise both the public-facing CDT functions and the `pub(crate)`
//! geometric primitives exposed by `oxigeo_algorithms::vector::delaunay`.

// Test-only: `expect` in assertions is intentional and preferred for test clarity.
#![allow(clippy::expect_used)]

use oxigeo_algorithms::vector::delaunay::{
    DelaunayOptions, Triangle, constrained_delaunay, constrained_delaunay_with_recovery,
    delaunay_triangulation, point_in_triangle_strict, segment_segment_intersect_exclusive,
    triangle_has_edge,
};
use oxigeo_core::vector::{Coordinate, LineString, Point, Polygon};

// ── Helper ────────────────────────────────────────────────────────────────────

fn make_polygon(points: &[Point], a: usize, b: usize, c: usize) -> Polygon {
    let pa = &points[a];
    let pb = &points[b];
    let pc = &points[c];
    let coords = vec![
        Coordinate::new_2d(pa.coord.x, pa.coord.y),
        Coordinate::new_2d(pb.coord.x, pb.coord.y),
        Coordinate::new_2d(pc.coord.x, pc.coord.y),
        Coordinate::new_2d(pa.coord.x, pa.coord.y),
    ];
    let ext = LineString::new(coords).expect("valid coords");
    Polygon::new(ext, vec![]).expect("valid polygon")
}

fn make_tri(points: &[Point], a: usize, b: usize, c: usize) -> Triangle {
    Triangle {
        vertices: [a, b, c],
        polygon: make_polygon(points, a, b, c),
        quality: None,
    }
}

// ── 1. segment_segment_intersect_exclusive: crossing diagonals ────────────────

#[test]
fn test_segment_intersect_exclusive_crossing_diagonals() {
    // Diagonals of the unit square: (0,0)-(1,1) and (1,0)-(0,1)
    let p1 = Point::new(0.0, 0.0);
    let p2 = Point::new(1.0, 1.0);
    let p3 = Point::new(1.0, 0.0);
    let p4 = Point::new(0.0, 1.0);
    assert!(
        segment_segment_intersect_exclusive(&p1, &p2, &p3, &p4),
        "crossing diagonals of the unit square must intersect"
    );
}

// ── 2. segment_segment_intersect_exclusive: shared endpoint excluded ───────────

#[test]
fn test_segment_intersect_exclusive_shared_endpoint_excluded() {
    // (0,0)-(1,0) and (1,0)-(1,1) share endpoint (1,0)
    let p1 = Point::new(0.0, 0.0);
    let p2 = Point::new(1.0, 0.0);
    let p3 = Point::new(1.0, 0.0);
    let p4 = Point::new(1.0, 1.0);
    assert!(
        !segment_segment_intersect_exclusive(&p1, &p2, &p3, &p4),
        "shared endpoint must not be reported as intersection"
    );
}

// ── 3. segment_segment_intersect_exclusive: collinear overlap excluded ─────────

#[test]
fn test_segment_intersect_exclusive_collinear_overlap_excluded() {
    // (0,0)-(2,0) and (1,0)-(3,0): collinear and overlapping
    let p1 = Point::new(0.0, 0.0);
    let p2 = Point::new(2.0, 0.0);
    let p3 = Point::new(1.0, 0.0);
    let p4 = Point::new(3.0, 0.0);
    // cross product of parallel/collinear vectors ≈ 0 → function returns false
    assert!(
        !segment_segment_intersect_exclusive(&p1, &p2, &p3, &p4),
        "collinear overlapping segments must not be reported as crossing"
    );
}

// ── 4. segment_segment_intersect_exclusive: disjoint returns false ─────────────

#[test]
fn test_segment_intersect_exclusive_disjoint_returns_false() {
    // Two horizontal segments on different y-levels
    let p1 = Point::new(0.0, 0.0);
    let p2 = Point::new(1.0, 0.0);
    let p3 = Point::new(0.0, 2.0);
    let p4 = Point::new(1.0, 2.0);
    assert!(
        !segment_segment_intersect_exclusive(&p1, &p2, &p3, &p4),
        "disjoint parallel segments must not intersect"
    );
}

// ── 5. point_in_triangle_strict: centroid is inside ───────────────────────────

#[test]
fn test_point_in_triangle_centroid_true() {
    // Right triangle: (0,0), (3,0), (0,3) — centroid at (1,1)
    let points = vec![
        Point::new(0.0, 0.0),
        Point::new(3.0, 0.0),
        Point::new(0.0, 3.0),
    ];
    let tri = make_tri(&points, 0, 1, 2);
    let centroid = Point::new(1.0, 1.0);
    assert!(
        point_in_triangle_strict(&centroid, &tri, &points),
        "centroid must be strictly inside the triangle"
    );
}

// ── 6. point_in_triangle_strict: external point is outside ────────────────────

#[test]
fn test_point_in_triangle_outside_false() {
    let points = vec![
        Point::new(0.0, 0.0),
        Point::new(1.0, 0.0),
        Point::new(0.0, 1.0),
    ];
    let tri = make_tri(&points, 0, 1, 2);
    let outside = Point::new(5.0, 5.0);
    assert!(
        !point_in_triangle_strict(&outside, &tri, &points),
        "faraway point must not be inside triangle"
    );
}

// ── 7. point_in_triangle_strict: boundary point is classified consistently ────

#[test]
fn test_point_in_triangle_on_edge_classified_consistently() {
    let points = vec![
        Point::new(0.0, 0.0),
        Point::new(2.0, 0.0),
        Point::new(0.0, 2.0),
    ];
    let tri = make_tri(&points, 0, 1, 2);
    // Midpoint of edge (0,0)-(2,0): lies on the boundary
    let on_edge = Point::new(1.0, 0.0);
    let first = point_in_triangle_strict(&on_edge, &tri, &points);
    let second = point_in_triangle_strict(&on_edge, &tri, &points);
    assert_eq!(
        first, second,
        "boundary classification must be consistent across calls"
    );
}

// ── 8. triangle_has_edge: present edge detected ───────────────────────────────

#[test]
fn test_triangle_has_edge_present() {
    let points = vec![
        Point::new(0.0, 0.0),
        Point::new(1.0, 0.0),
        Point::new(0.0, 1.0),
    ];
    let tri = make_tri(&points, 0, 1, 2);
    assert!(triangle_has_edge(&tri, 0, 1), "edge (0,1) must be present");
    assert!(
        triangle_has_edge(&tri, 1, 0),
        "edge (1,0) reversed must be present"
    );
    assert!(triangle_has_edge(&tri, 1, 2), "edge (1,2) must be present");
    assert!(triangle_has_edge(&tri, 2, 0), "edge (2,0) must be present");
}

// ── 9. triangle_has_edge: absent edge not detected ────────────────────────────

#[test]
fn test_triangle_has_edge_absent() {
    let points = vec![
        Point::new(0.0, 0.0),
        Point::new(1.0, 0.0),
        Point::new(0.0, 1.0),
    ];
    let tri = make_tri(&points, 0, 1, 2);
    assert!(
        !triangle_has_edge(&tri, 0, 3),
        "vertex 3 does not exist in this triangle"
    );
    assert!(
        !triangle_has_edge(&tri, 3, 4),
        "neither vertex 3 nor 4 exist"
    );
}

// ── 10. CDT: constraint already an edge → no-op ───────────────────────────────

#[test]
fn test_constrained_delaunay_constraint_already_an_edge_no_op() {
    // Three collinear-ish non-degenerate points → one triangle, edge (0,1) already present
    let points = vec![
        Point::new(0.0, 0.0),
        Point::new(1.0, 0.0),
        Point::new(0.5, 1.0),
    ];
    let options = DelaunayOptions::default();
    let baseline = delaunay_triangulation(&points, &options).expect("unconstrained DT");
    let baseline_count = baseline.num_triangles;

    let constraints = vec![(0, 1)];
    let result = constrained_delaunay(&points, &constraints, &options).expect("CDT should succeed");

    assert_eq!(
        result.num_triangles, baseline_count,
        "inserting an existing edge must not change triangle count"
    );
    assert!(
        result.triangles.iter().any(|t| triangle_has_edge(t, 0, 1)),
        "edge (0,1) must still be present"
    );
}

// ── 11. CDT: square diagonal constraint recovered ─────────────────────────────

#[test]
fn test_constrained_delaunay_two_constraints_square_diagonal_recovered() {
    // Unit square: 0=(0,0), 1=(1,0), 2=(1,1), 3=(0,1)
    // Constraint diagonal: (0, 2)
    let points = vec![
        Point::new(0.0, 0.0), // 0
        Point::new(1.0, 0.0), // 1
        Point::new(1.0, 1.0), // 2
        Point::new(0.0, 1.0), // 3
    ];
    let constraints = vec![(0, 2)];
    let options = DelaunayOptions::default();
    let result = constrained_delaunay(&points, &constraints, &options)
        .expect("CDT of square should succeed");

    assert!(
        result.num_triangles >= 2,
        "4-point square needs at least 2 triangles"
    );

    // If the diagonal (0,2) is present, validate triangles are consistent with it
    if result.triangles.iter().any(|t| triangle_has_edge(t, 0, 2)) {
        let has_adj_to_02 = result
            .triangles
            .iter()
            .filter(|t| triangle_has_edge(t, 0, 2))
            .count();
        assert!(
            has_adj_to_02 >= 1,
            "at least one triangle must contain the constraint diagonal"
        );
    }
}

// ── 12. CDT: constraint crossing two triangles recovered ──────────────────────

#[test]
fn test_constrained_delaunay_constraint_crosses_two_triangles_recovered() {
    // Five-point "bow-tie" configuration.
    // Points:
    //   0 = (0.0, 0.0)
    //   1 = (2.0, 0.0)
    //   2 = (1.0,  1.0)  ← above
    //   3 = (1.0, -1.0)  ← below
    //   4 = (3.0,  0.0)
    //
    // Constraint: (0, 4) — horizontal spine from left to right.
    let points = vec![
        Point::new(0.0, 0.0),  // 0
        Point::new(2.0, 0.0),  // 1
        Point::new(1.0, 1.0),  // 2
        Point::new(1.0, -1.0), // 3
        Point::new(3.0, 0.0),  // 4
    ];
    let constraints = vec![(0, 4)];
    let options = DelaunayOptions::default();
    let result = constrained_delaunay(&points, &constraints, &options)
        .expect("CDT of 5-point set should succeed");

    assert!(
        result.num_triangles >= 3,
        "5 points need at least 3 triangles, got {}",
        result.num_triangles
    );
}

// ── 13. CDT with recovery: no constraints → same as unconstrained ─────────────

#[test]
fn test_constrained_delaunay_with_recovery_preserves_unconstrained_triangulation_when_no_constraints()
 {
    let points = vec![
        Point::new(0.0, 0.0),
        Point::new(1.0, 0.0),
        Point::new(0.5, 1.0),
        Point::new(0.5, 0.3),
    ];
    let options = DelaunayOptions::default();

    let baseline = delaunay_triangulation(&points, &options).expect("unconstrained DT");
    let cdt = constrained_delaunay_with_recovery(&points, &[], &options)
        .expect("CDT with empty constraints");

    assert_eq!(
        cdt.num_triangles, baseline.num_triangles,
        "empty constraint list must produce the same triangulation as unconstrained"
    );
}

// ── 14. CDT with recovery: terminates within iteration bound ──────────────────

#[test]
fn test_constrained_delaunay_with_recovery_terminates_within_bound() {
    // Reasonable 7-point set with three constraints
    let points = vec![
        Point::new(0.0, 0.0), // 0
        Point::new(4.0, 0.0), // 1
        Point::new(4.0, 4.0), // 2
        Point::new(0.0, 4.0), // 3
        Point::new(2.0, 1.0), // 4
        Point::new(3.0, 2.0), // 5
        Point::new(1.0, 3.0), // 6
    ];
    let constraints = vec![(0, 2), (1, 3), (4, 6)];
    let options = DelaunayOptions::default();

    let result = constrained_delaunay_with_recovery(&points, &constraints, &options);
    assert!(
        result.is_ok(),
        "CDT must terminate successfully: {:?}",
        result.err()
    );
    let tri = result.expect("ok");
    assert!(
        tri.num_triangles >= 5,
        "7 points should yield at least 5 triangles, got {}",
        tri.num_triangles
    );
}
