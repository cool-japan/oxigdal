//! Integration tests for the `operations` module.

use oxigdal_index::{
    Coord, Polygon, Ring, area, buffer_bbox, centroid, convex_hull, distance, is_convex, perimeter,
    point_in_polygon, ring_bbox, simplify,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn unit_square() -> Polygon {
    Polygon::simple(Ring::new(vec![
        Coord::new(0.0, 0.0),
        Coord::new(1.0, 0.0),
        Coord::new(1.0, 1.0),
        Coord::new(0.0, 1.0),
        Coord::new(0.0, 0.0),
    ]))
}

fn right_triangle() -> Polygon {
    // 3-4-5 right triangle: area = 6.0
    Polygon::simple(Ring::new(vec![
        Coord::new(0.0, 0.0),
        Coord::new(4.0, 0.0),
        Coord::new(0.0, 3.0),
        Coord::new(0.0, 0.0),
    ]))
}

// ---------------------------------------------------------------------------
// Centroid
// ---------------------------------------------------------------------------

#[test]
fn centroid_unit_square() {
    let c = centroid(&unit_square());
    assert!((c.x - 0.5).abs() < 1e-10);
    assert!((c.y - 0.5).abs() < 1e-10);
}

#[test]
fn centroid_triangle() {
    let c = centroid(&right_triangle());
    // Centroid of triangle with vertices (0,0), (4,0), (0,3) is (4/3, 1).
    assert!((c.x - 4.0 / 3.0).abs() < 1e-10);
    assert!((c.y - 1.0).abs() < 1e-10);
}

#[test]
fn centroid_rectangle() {
    let poly = Polygon::simple(Ring::new(vec![
        Coord::new(0.0, 0.0),
        Coord::new(6.0, 0.0),
        Coord::new(6.0, 2.0),
        Coord::new(0.0, 2.0),
        Coord::new(0.0, 0.0),
    ]));
    let c = centroid(&poly);
    assert!((c.x - 3.0).abs() < 1e-10);
    assert!((c.y - 1.0).abs() < 1e-10);
}

// ---------------------------------------------------------------------------
// Area
// ---------------------------------------------------------------------------

#[test]
fn area_unit_square() {
    assert!((area(&unit_square()) - 1.0).abs() < 1e-10);
}

#[test]
fn area_right_triangle() {
    assert!((area(&right_triangle()) - 6.0).abs() < 1e-10);
}

#[test]
fn area_with_hole() {
    let exterior = Ring::new(vec![
        Coord::new(0.0, 0.0),
        Coord::new(4.0, 0.0),
        Coord::new(4.0, 4.0),
        Coord::new(0.0, 4.0),
        Coord::new(0.0, 0.0),
    ]);
    // Hole is 2x2 = 4 area, exterior is 4x4 = 16, net = 12.
    let hole = Ring::new(vec![
        Coord::new(1.0, 1.0),
        Coord::new(1.0, 3.0),
        Coord::new(3.0, 3.0),
        Coord::new(3.0, 1.0),
        Coord::new(1.0, 1.0),
    ]);
    let poly = Polygon::new(exterior, vec![hole]);
    assert!((area(&poly) - 12.0).abs() < 1e-10);
}

// ---------------------------------------------------------------------------
// Perimeter
// ---------------------------------------------------------------------------

#[test]
fn perimeter_unit_square() {
    assert!((perimeter(&unit_square()) - 4.0).abs() < 1e-10);
}

#[test]
fn perimeter_right_triangle() {
    // 3 + 4 + 5 = 12
    assert!((perimeter(&right_triangle()) - 12.0).abs() < 1e-10);
}

// ---------------------------------------------------------------------------
// Point-in-polygon
// ---------------------------------------------------------------------------

#[test]
fn pip_inside() {
    assert!(point_in_polygon(&Coord::new(0.5, 0.5), &unit_square()));
}

#[test]
fn pip_outside() {
    assert!(!point_in_polygon(&Coord::new(2.0, 2.0), &unit_square()));
}

#[test]
fn pip_in_hole_is_outside() {
    let exterior = Ring::new(vec![
        Coord::new(0.0, 0.0),
        Coord::new(10.0, 0.0),
        Coord::new(10.0, 10.0),
        Coord::new(0.0, 10.0),
        Coord::new(0.0, 0.0),
    ]);
    let hole = Ring::new(vec![
        Coord::new(3.0, 3.0),
        Coord::new(3.0, 7.0),
        Coord::new(7.0, 7.0),
        Coord::new(7.0, 3.0),
        Coord::new(3.0, 3.0),
    ]);
    let poly = Polygon::new(exterior, vec![hole]);
    // Point inside the hole.
    assert!(!point_in_polygon(&Coord::new(5.0, 5.0), &poly));
    // Point between exterior and hole.
    assert!(point_in_polygon(&Coord::new(1.0, 1.0), &poly));
}

#[test]
fn pip_far_away() {
    assert!(!point_in_polygon(&Coord::new(100.0, 100.0), &unit_square()));
}

// ---------------------------------------------------------------------------
// Simplification (Douglas-Peucker)
// ---------------------------------------------------------------------------

#[test]
fn simplify_straight_line() {
    let coords = vec![
        Coord::new(0.0, 0.0),
        Coord::new(1.0, 0.0),
        Coord::new(2.0, 0.0),
        Coord::new(3.0, 0.0),
    ];
    let simplified = simplify(&coords, 0.1);
    // All intermediate points are within epsilon of the line, so only first+last remain.
    assert_eq!(simplified.len(), 2);
}

#[test]
fn simplify_l_shape() {
    let coords = vec![
        Coord::new(0.0, 0.0),
        Coord::new(1.0, 0.0),
        Coord::new(2.0, 0.0),
        Coord::new(2.0, 1.0),
        Coord::new(2.0, 2.0),
    ];
    let simplified = simplify(&coords, 0.1);
    // The corner at (2,0) must be retained.
    assert!(simplified.len() >= 3);
    assert!(simplified.contains(&Coord::new(2.0, 0.0)));
}

#[test]
fn simplify_preserves_endpoints() {
    let coords = vec![
        Coord::new(0.0, 0.0),
        Coord::new(0.5, 0.01),
        Coord::new(1.0, 0.0),
    ];
    let simplified = simplify(&coords, 0.1);
    assert_eq!(simplified[0], Coord::new(0.0, 0.0));
    assert_eq!(simplified[simplified.len() - 1], Coord::new(1.0, 0.0));
}

#[test]
fn simplify_complex_curve() {
    // Zigzag pattern
    let coords = vec![
        Coord::new(0.0, 0.0),
        Coord::new(1.0, 2.0),
        Coord::new(2.0, 0.0),
        Coord::new(3.0, 2.0),
        Coord::new(4.0, 0.0),
    ];
    // With a large epsilon, should collapse to endpoints.
    let simplified = simplify(&coords, 3.0);
    assert_eq!(simplified.len(), 2);
    // With a tiny epsilon, all points kept.
    let simplified2 = simplify(&coords, 0.001);
    assert_eq!(simplified2.len(), 5);
}

#[test]
fn simplify_two_points_unchanged() {
    let coords = vec![Coord::new(0.0, 0.0), Coord::new(5.0, 5.0)];
    let simplified = simplify(&coords, 1.0);
    assert_eq!(simplified.len(), 2);
}

// ---------------------------------------------------------------------------
// Convex hull
// ---------------------------------------------------------------------------

#[test]
fn convex_hull_square() {
    let points = vec![
        Coord::new(0.0, 0.0),
        Coord::new(1.0, 0.0),
        Coord::new(1.0, 1.0),
        Coord::new(0.0, 1.0),
    ];
    let hull = convex_hull(&points);
    assert_eq!(hull.len(), 4);
}

#[test]
fn convex_hull_with_interior() {
    let points = vec![
        Coord::new(0.0, 0.0),
        Coord::new(4.0, 0.0),
        Coord::new(4.0, 4.0),
        Coord::new(0.0, 4.0),
        Coord::new(2.0, 2.0), // interior
        Coord::new(1.0, 1.0), // interior
    ];
    let hull = convex_hull(&points);
    assert_eq!(hull.len(), 4);
}

#[test]
fn convex_hull_triangle() {
    let points = vec![
        Coord::new(0.0, 0.0),
        Coord::new(2.0, 0.0),
        Coord::new(1.0, 2.0),
    ];
    let hull = convex_hull(&points);
    assert_eq!(hull.len(), 3);
}

#[test]
fn convex_hull_collinear() {
    let points = vec![
        Coord::new(0.0, 0.0),
        Coord::new(1.0, 0.0),
        Coord::new(2.0, 0.0),
    ];
    let hull = convex_hull(&points);
    // Collinear points don't form a polygon — result is the endpoints.
    assert!(hull.len() <= 3);
}

#[test]
fn convex_hull_single_point() {
    let hull = convex_hull(&[Coord::new(5.0, 5.0)]);
    assert_eq!(hull.len(), 1);
}

// ---------------------------------------------------------------------------
// is_convex
// ---------------------------------------------------------------------------

#[test]
fn is_convex_square_yes() {
    let ring = [
        Coord::new(0.0, 0.0),
        Coord::new(1.0, 0.0),
        Coord::new(1.0, 1.0),
        Coord::new(0.0, 1.0),
        Coord::new(0.0, 0.0),
    ];
    assert!(is_convex(&ring));
}

#[test]
fn is_convex_concave_no() {
    // L-shape is concave.
    let ring = [
        Coord::new(0.0, 0.0),
        Coord::new(2.0, 0.0),
        Coord::new(2.0, 1.0),
        Coord::new(1.0, 1.0),
        Coord::new(1.0, 2.0),
        Coord::new(0.0, 2.0),
        Coord::new(0.0, 0.0),
    ];
    assert!(!is_convex(&ring));
}

#[test]
fn is_convex_triangle() {
    let ring = [
        Coord::new(0.0, 0.0),
        Coord::new(3.0, 0.0),
        Coord::new(1.5, 2.0),
        Coord::new(0.0, 0.0),
    ];
    assert!(is_convex(&ring));
}

#[test]
fn is_convex_too_few() {
    let ring = [Coord::new(0.0, 0.0), Coord::new(1.0, 1.0)];
    assert!(!is_convex(&ring));
}

// ---------------------------------------------------------------------------
// Distance
// ---------------------------------------------------------------------------

#[test]
fn distance_3_4_5() {
    let d = distance(&Coord::new(0.0, 0.0), &Coord::new(3.0, 4.0));
    assert!((d - 5.0).abs() < 1e-10);
}

#[test]
fn distance_zero() {
    let d = distance(&Coord::new(1.0, 1.0), &Coord::new(1.0, 1.0));
    assert!(d.abs() < 1e-10);
}

#[test]
fn distance_negative_coords() {
    let d = distance(&Coord::new(-1.0, -1.0), &Coord::new(2.0, 3.0));
    assert!((d - 5.0).abs() < 1e-10);
}

// ---------------------------------------------------------------------------
// ring_bbox
// ---------------------------------------------------------------------------

#[test]
fn ring_bbox_basic() {
    let ring = [
        Coord::new(1.0, 2.0),
        Coord::new(5.0, -1.0),
        Coord::new(3.0, 4.0),
    ];
    let (min, max) = ring_bbox(&ring).expect("non-empty");
    assert!((min.x - 1.0).abs() < 1e-10);
    assert!((min.y - (-1.0)).abs() < 1e-10);
    assert!((max.x - 5.0).abs() < 1e-10);
    assert!((max.y - 4.0).abs() < 1e-10);
}

#[test]
fn ring_bbox_empty() {
    let ring: [Coord; 0] = [];
    assert!(ring_bbox(&ring).is_none());
}

#[test]
fn ring_bbox_single_point() {
    let ring = [Coord::new(3.0, 7.0)];
    let (min, max) = ring_bbox(&ring).expect("one point");
    assert!((min.x - 3.0).abs() < 1e-10);
    assert!((max.x - 3.0).abs() < 1e-10);
}

// ---------------------------------------------------------------------------
// buffer_bbox
// ---------------------------------------------------------------------------

#[test]
fn buffer_bbox_expand() {
    let (min, max) = buffer_bbox(&Coord::new(0.0, 0.0), &Coord::new(2.0, 2.0), 1.0);
    assert!((min.x - (-1.0)).abs() < 1e-10);
    assert!((min.y - (-1.0)).abs() < 1e-10);
    assert!((max.x - 3.0).abs() < 1e-10);
    assert!((max.y - 3.0).abs() < 1e-10);
}

#[test]
fn buffer_bbox_shrink() {
    let (min, max) = buffer_bbox(&Coord::new(0.0, 0.0), &Coord::new(4.0, 4.0), -1.0);
    assert!((min.x - 1.0).abs() < 1e-10);
    assert!((max.x - 3.0).abs() < 1e-10);
}

#[test]
fn buffer_bbox_zero() {
    let (min, max) = buffer_bbox(&Coord::new(1.0, 2.0), &Coord::new(3.0, 4.0), 0.0);
    assert!((min.x - 1.0).abs() < 1e-10);
    assert!((max.y - 4.0).abs() < 1e-10);
}
