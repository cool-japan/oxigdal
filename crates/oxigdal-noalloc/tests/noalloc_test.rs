//! Integration tests for oxigdal-noalloc geometry primitives.
//!
//! All tests are designed to run in no_std + no_alloc environments.

use oxigdal_noalloc::{
    BBox2D, CoordTransform, FixedPolygon, GeoHashFixed, LineSegment2D, NoAllocError, Point2D,
    Point3D, Triangle2D,
};

// ── Point2D ──────────────────────────────────────────────────────────────────

#[test]
fn test_point2d_new() {
    let p = Point2D::new(3.0, 4.0);
    assert_eq!(p.x, 3.0);
    assert_eq!(p.y, 4.0);
}

#[test]
fn test_point2d_distance_pythagorean() {
    // 3-4-5 right triangle
    let a = Point2D::new(0.0, 0.0);
    let b = Point2D::new(3.0, 4.0);
    let dist = a.distance_to(&b);
    assert!((dist - 5.0).abs() < 1e-9, "Expected 5.0, got {}", dist);
}

#[test]
fn test_point2d_distance_same_point() {
    let p = Point2D::new(1.0, 2.0);
    assert!((p.distance_to(&p)).abs() < 1e-12);
}

#[test]
fn test_point2d_midpoint() {
    let a = Point2D::new(0.0, 0.0);
    let b = Point2D::new(4.0, 6.0);
    let mid = a.midpoint(&b);
    assert!((mid.x - 2.0).abs() < 1e-12);
    assert!((mid.y - 3.0).abs() < 1e-12);
}

#[test]
fn test_point2d_midpoint_negative() {
    let a = Point2D::new(-2.0, -4.0);
    let b = Point2D::new(2.0, 4.0);
    let mid = a.midpoint(&b);
    assert!((mid.x).abs() < 1e-12);
    assert!((mid.y).abs() < 1e-12);
}

// ── Point3D ──────────────────────────────────────────────────────────────────

#[test]
fn test_point3d_new() {
    let p = Point3D::new(1.0, 2.0, 3.0);
    assert_eq!(p.x, 1.0);
    assert_eq!(p.y, 2.0);
    assert_eq!(p.z, 3.0);
}

#[test]
fn test_point3d_distance() {
    // sqrt(1^2 + 2^2 + 2^2) = sqrt(9) = 3
    let a = Point3D::new(0.0, 0.0, 0.0);
    let b = Point3D::new(1.0, 2.0, 2.0);
    let dist = a.distance_to(&b);
    assert!((dist - 3.0).abs() < 1e-9, "Expected 3.0, got {}", dist);
}

#[test]
fn test_point3d_to_2d() {
    let p = Point3D::new(5.0, 7.0, 99.0);
    let p2 = p.to_2d();
    assert_eq!(p2.x, 5.0);
    assert_eq!(p2.y, 7.0);
}

// ── BBox2D ───────────────────────────────────────────────────────────────────

#[test]
fn test_bbox_contains_point_inside() {
    let b = BBox2D::new(0.0, 0.0, 10.0, 10.0);
    assert!(b.contains_point(Point2D::new(5.0, 5.0)));
}

#[test]
fn test_bbox_contains_point_on_boundary() {
    let b = BBox2D::new(0.0, 0.0, 10.0, 10.0);
    assert!(b.contains_point(Point2D::new(0.0, 0.0)));
    assert!(b.contains_point(Point2D::new(10.0, 10.0)));
}

#[test]
fn test_bbox_contains_point_outside() {
    let b = BBox2D::new(0.0, 0.0, 10.0, 10.0);
    assert!(!b.contains_point(Point2D::new(11.0, 5.0)));
    assert!(!b.contains_point(Point2D::new(-1.0, 5.0)));
}

#[test]
fn test_bbox_intersects_overlapping() {
    let a = BBox2D::new(0.0, 0.0, 5.0, 5.0);
    let b = BBox2D::new(3.0, 3.0, 8.0, 8.0);
    assert!(a.intersects(&b));
    assert!(b.intersects(&a));
}

#[test]
fn test_bbox_intersects_touching() {
    let a = BBox2D::new(0.0, 0.0, 5.0, 5.0);
    let b = BBox2D::new(5.0, 0.0, 10.0, 5.0);
    assert!(a.intersects(&b));
}

#[test]
fn test_bbox_intersects_disjoint() {
    let a = BBox2D::new(0.0, 0.0, 3.0, 3.0);
    let b = BBox2D::new(4.0, 4.0, 8.0, 8.0);
    assert!(!a.intersects(&b));
}

#[test]
fn test_bbox_union() {
    let a = BBox2D::new(0.0, 0.0, 5.0, 5.0);
    let b = BBox2D::new(3.0, 3.0, 8.0, 8.0);
    let u = a.union(&b);
    assert!((u.min_x).abs() < 1e-12);
    assert!((u.min_y).abs() < 1e-12);
    assert!((u.max_x - 8.0).abs() < 1e-12);
    assert!((u.max_y - 8.0).abs() < 1e-12);
}

#[test]
fn test_bbox_area() {
    let b = BBox2D::new(0.0, 0.0, 4.0, 3.0);
    assert!((b.area() - 12.0).abs() < 1e-12);
}

#[test]
fn test_bbox_area_zero() {
    let b = BBox2D::new(0.0, 0.0, 0.0, 0.0);
    assert!((b.area()).abs() < 1e-12);
}

#[test]
fn test_bbox_is_valid() {
    assert!(BBox2D::new(0.0, 0.0, 1.0, 1.0).is_valid());
    assert!(BBox2D::new(0.0, 0.0, 0.0, 0.0).is_valid()); // degenerate but valid
    assert!(!BBox2D::new(1.0, 0.0, 0.0, 1.0).is_valid()); // min_x > max_x
}

// ── LineSegment2D ─────────────────────────────────────────────────────────────

#[test]
fn test_line_segment_length() {
    let s = LineSegment2D::new(Point2D::new(0.0, 0.0), Point2D::new(3.0, 4.0));
    assert!((s.length() - 5.0).abs() < 1e-9);
}

#[test]
fn test_line_segment_midpoint() {
    let s = LineSegment2D::new(Point2D::new(0.0, 0.0), Point2D::new(6.0, 4.0));
    let m = s.midpoint();
    assert!((m.x - 3.0).abs() < 1e-12);
    assert!((m.y - 2.0).abs() < 1e-12);
}

#[test]
fn test_line_segment_point_on_segment_ends() {
    let s = LineSegment2D::new(Point2D::new(1.0, 2.0), Point2D::new(5.0, 6.0));
    let start = s.point_on_segment(0.0);
    let end = s.point_on_segment(1.0);
    assert!((start.x - 1.0).abs() < 1e-12);
    assert!((start.y - 2.0).abs() < 1e-12);
    assert!((end.x - 5.0).abs() < 1e-12);
    assert!((end.y - 6.0).abs() < 1e-12);
}

#[test]
fn test_line_segment_point_on_segment_midpoint() {
    let s = LineSegment2D::new(Point2D::new(0.0, 0.0), Point2D::new(4.0, 4.0));
    let mid = s.point_on_segment(0.5);
    assert!((mid.x - 2.0).abs() < 1e-12);
    assert!((mid.y - 2.0).abs() < 1e-12);
}

#[test]
fn test_line_segment_intersects_crossing() {
    let a = LineSegment2D::new(Point2D::new(0.0, 0.0), Point2D::new(4.0, 4.0));
    let b = LineSegment2D::new(Point2D::new(0.0, 4.0), Point2D::new(4.0, 0.0));
    let ix = a.intersects(&b);
    assert!(ix.is_some(), "Expected crossing intersection");
    let p = ix.expect("crossing intersection should exist");
    assert!((p.x - 2.0).abs() < 1e-9);
    assert!((p.y - 2.0).abs() < 1e-9);
}

#[test]
fn test_line_segment_intersects_parallel() {
    let a = LineSegment2D::new(Point2D::new(0.0, 0.0), Point2D::new(4.0, 0.0));
    let b = LineSegment2D::new(Point2D::new(0.0, 1.0), Point2D::new(4.0, 1.0));
    assert!(
        a.intersects(&b).is_none(),
        "Parallel lines should not intersect"
    );
}

#[test]
fn test_line_segment_intersects_t_shape() {
    // T-shaped: one segment runs from (0,2) to (4,2), other from (2,0) to (2,2)
    let a = LineSegment2D::new(Point2D::new(0.0, 2.0), Point2D::new(4.0, 2.0));
    let b = LineSegment2D::new(Point2D::new(2.0, 0.0), Point2D::new(2.0, 2.0));
    let ix = a.intersects(&b);
    assert!(ix.is_some());
    let p = ix.expect("T-shape intersection should exist");
    assert!((p.x - 2.0).abs() < 1e-9);
    assert!((p.y - 2.0).abs() < 1e-9);
}

#[test]
fn test_line_segment_no_intersection_non_overlapping() {
    let a = LineSegment2D::new(Point2D::new(0.0, 0.0), Point2D::new(1.0, 0.0));
    let b = LineSegment2D::new(Point2D::new(2.0, 0.0), Point2D::new(3.0, 0.0));
    assert!(a.intersects(&b).is_none());
}

// ── Triangle2D ───────────────────────────────────────────────────────────────

#[test]
fn test_triangle_area_unit() {
    // Right triangle with legs 1 and 2: area = 1.0
    let t = Triangle2D::new(
        Point2D::new(0.0, 0.0),
        Point2D::new(2.0, 0.0),
        Point2D::new(0.0, 1.0),
    );
    assert!((t.area() - 1.0).abs() < 1e-12);
}

#[test]
fn test_triangle_area_3_4_5() {
    // Right triangle 3-4-5: area = 6.0
    let t = Triangle2D::new(
        Point2D::new(0.0, 0.0),
        Point2D::new(3.0, 0.0),
        Point2D::new(0.0, 4.0),
    );
    assert!((t.area() - 6.0).abs() < 1e-12);
}

#[test]
fn test_triangle_is_clockwise() {
    let ccw = Triangle2D::new(
        Point2D::new(0.0, 0.0),
        Point2D::new(1.0, 0.0),
        Point2D::new(0.0, 1.0),
    );
    assert!(!ccw.is_clockwise());

    let cw = Triangle2D::new(
        Point2D::new(0.0, 0.0),
        Point2D::new(0.0, 1.0),
        Point2D::new(1.0, 0.0),
    );
    assert!(cw.is_clockwise());
}

#[test]
fn test_triangle_centroid() {
    let t = Triangle2D::new(
        Point2D::new(0.0, 0.0),
        Point2D::new(3.0, 0.0),
        Point2D::new(0.0, 3.0),
    );
    let c = t.centroid();
    assert!((c.x - 1.0).abs() < 1e-12);
    assert!((c.y - 1.0).abs() < 1e-12);
}

#[test]
fn test_triangle_contains_point_inside() {
    let t = Triangle2D::new(
        Point2D::new(0.0, 0.0),
        Point2D::new(4.0, 0.0),
        Point2D::new(2.0, 4.0),
    );
    assert!(t.contains_point(Point2D::new(2.0, 1.0)));
}

#[test]
fn test_triangle_contains_point_outside() {
    let t = Triangle2D::new(
        Point2D::new(0.0, 0.0),
        Point2D::new(4.0, 0.0),
        Point2D::new(2.0, 4.0),
    );
    assert!(!t.contains_point(Point2D::new(10.0, 10.0)));
    assert!(!t.contains_point(Point2D::new(-1.0, 0.0)));
}

#[test]
fn test_triangle_contains_vertex() {
    let t = Triangle2D::new(
        Point2D::new(0.0, 0.0),
        Point2D::new(4.0, 0.0),
        Point2D::new(2.0, 4.0),
    );
    // Vertex is on the boundary — should be inside
    assert!(t.contains_point(Point2D::new(0.0, 0.0)));
}

#[test]
fn test_triangle_perimeter() {
    // Equilateral triangle with side length 1.0: perimeter = 3.0
    let side = 1.0_f64;
    let h = (3.0_f64 * 0.25_f64 * side * side).sqrt(); // sqrt(3)/2 * side
    let t = Triangle2D::new(
        Point2D::new(0.0, 0.0),
        Point2D::new(1.0, 0.0),
        Point2D::new(0.5, h),
    );
    let p = t.perimeter();
    assert!((p - 3.0 * side).abs() < 1e-6, "Perimeter = {}", p);
}

// ── FixedPolygon ─────────────────────────────────────────────────────────────

#[test]
fn test_fixed_polygon_push_and_len() {
    let mut poly: FixedPolygon<4> = FixedPolygon::new();
    assert!(poly.is_empty());
    assert!(poly.try_push(Point2D::new(0.0, 0.0)));
    assert!(poly.try_push(Point2D::new(1.0, 0.0)));
    assert_eq!(poly.len(), 2);
}

#[test]
fn test_fixed_polygon_capacity_overflow() {
    let mut poly: FixedPolygon<4> = FixedPolygon::new();
    assert!(poly.try_push(Point2D::new(0.0, 0.0)));
    assert!(poly.try_push(Point2D::new(1.0, 0.0)));
    assert!(poly.try_push(Point2D::new(1.0, 1.0)));
    assert!(poly.try_push(Point2D::new(0.0, 1.0)));
    // 5th push should fail
    assert!(!poly.try_push(Point2D::new(0.5, 0.5)));
    assert_eq!(poly.len(), 4);
}

#[test]
fn test_fixed_polygon_square_area() {
    let mut poly: FixedPolygon<4> = FixedPolygon::new();
    poly.try_push(Point2D::new(0.0, 0.0));
    poly.try_push(Point2D::new(2.0, 0.0));
    poly.try_push(Point2D::new(2.0, 2.0));
    poly.try_push(Point2D::new(0.0, 2.0));
    assert!((poly.area() - 4.0).abs() < 1e-12, "Area = {}", poly.area());
}

#[test]
fn test_fixed_polygon_square_perimeter() {
    let mut poly: FixedPolygon<4> = FixedPolygon::new();
    poly.try_push(Point2D::new(0.0, 0.0));
    poly.try_push(Point2D::new(3.0, 0.0));
    poly.try_push(Point2D::new(3.0, 3.0));
    poly.try_push(Point2D::new(0.0, 3.0));
    assert!((poly.perimeter() - 12.0).abs() < 1e-12);
}

#[test]
fn test_fixed_polygon_bbox() {
    let mut poly: FixedPolygon<4> = FixedPolygon::new();
    poly.try_push(Point2D::new(-1.0, -2.0));
    poly.try_push(Point2D::new(3.0, -2.0));
    poly.try_push(Point2D::new(3.0, 5.0));
    poly.try_push(Point2D::new(-1.0, 5.0));
    let b = poly.bbox().expect("polygon bbox should exist");
    assert!((b.min_x - (-1.0)).abs() < 1e-12);
    assert!((b.min_y - (-2.0)).abs() < 1e-12);
    assert!((b.max_x - 3.0).abs() < 1e-12);
    assert!((b.max_y - 5.0).abs() < 1e-12);
}

#[test]
fn test_fixed_polygon_empty_bbox() {
    let poly: FixedPolygon<4> = FixedPolygon::new();
    assert!(poly.bbox().is_none());
}

#[test]
fn test_fixed_polygon_centroid() {
    let mut poly: FixedPolygon<4> = FixedPolygon::new();
    poly.try_push(Point2D::new(0.0, 0.0));
    poly.try_push(Point2D::new(4.0, 0.0));
    poly.try_push(Point2D::new(4.0, 4.0));
    poly.try_push(Point2D::new(0.0, 4.0));
    let c = poly.centroid().expect("polygon centroid should exist");
    assert!((c.x - 2.0).abs() < 1e-12);
    assert!((c.y - 2.0).abs() < 1e-12);
}

#[test]
fn test_fixed_polygon_vertices_slice() {
    let mut poly: FixedPolygon<8> = FixedPolygon::new();
    poly.try_push(Point2D::new(1.0, 2.0));
    poly.try_push(Point2D::new(3.0, 4.0));
    let verts = poly.vertices();
    assert_eq!(verts.len(), 2);
    assert_eq!(verts[0], Point2D::new(1.0, 2.0));
    assert_eq!(verts[1], Point2D::new(3.0, 4.0));
}

#[test]
fn test_fixed_polygon_default() {
    let poly: FixedPolygon<8> = FixedPolygon::default();
    assert!(poly.is_empty());
}

// ── CoordTransform ────────────────────────────────────────────────────────────

#[test]
fn test_coord_transform_identity() {
    let t = CoordTransform::identity();
    let p = Point2D::new(3.0, 7.0);
    let r = t.apply(p);
    assert!((r.x - 3.0).abs() < 1e-12);
    assert!((r.y - 7.0).abs() < 1e-12);
}

#[test]
fn test_coord_transform_scale_2x() {
    let t = CoordTransform::scale(2.0, 3.0);
    let p = Point2D::new(4.0, 5.0);
    let r = t.apply(p);
    assert!((r.x - 8.0).abs() < 1e-12);
    assert!((r.y - 15.0).abs() < 1e-12);
}

#[test]
fn test_coord_transform_translate() {
    let t = CoordTransform::translate(10.0, -5.0);
    let p = Point2D::new(1.0, 2.0);
    let r = t.apply(p);
    assert!((r.x - 11.0).abs() < 1e-12);
    assert!((r.y - (-3.0)).abs() < 1e-12);
}

#[test]
fn test_coord_transform_rotate_90_degrees() {
    let pi = core::f64::consts::PI;
    let t = CoordTransform::rotate(pi / 2.0);
    let p = Point2D::new(1.0, 0.0);
    let r = t.apply(p);
    // Rotate (1,0) by 90° CCW → (0,1)
    assert!((r.x).abs() < 1e-6, "x should be ~0, got {}", r.x);
    assert!((r.y - 1.0).abs() < 1e-6, "y should be ~1, got {}", r.y);
}

#[test]
fn test_coord_transform_rotate_180_degrees() {
    let pi = core::f64::consts::PI;
    let t = CoordTransform::rotate(pi);
    let p = Point2D::new(1.0, 1.0);
    let r = t.apply(p);
    // Rotate (1,1) by 180° → (-1,-1)
    assert!((r.x - (-1.0)).abs() < 1e-6, "x = {}", r.x);
    assert!((r.y - (-1.0)).abs() < 1e-6, "y = {}", r.y);
}

#[test]
fn test_coord_transform_compose_scale_then_translate() {
    let scale = CoordTransform::scale(2.0, 2.0);
    let translate = CoordTransform::translate(1.0, 1.0);
    let combined = scale.compose(&translate);
    let p = Point2D::new(3.0, 4.0);
    let r = combined.apply(p);
    // scale: (6, 8), then translate: (7, 9)
    assert!((r.x - 7.0).abs() < 1e-12, "x = {}", r.x);
    assert!((r.y - 9.0).abs() < 1e-12, "y = {}", r.y);
}

#[test]
fn test_coord_transform_apply3d_passthrough_z() {
    let t = CoordTransform::scale(3.0, 3.0);
    let p = Point3D::new(1.0, 2.0, 99.0);
    let r = t.apply3d(p);
    assert!((r.x - 3.0).abs() < 1e-12);
    assert!((r.y - 6.0).abs() < 1e-12);
    assert!((r.z - 99.0).abs() < 1e-12);
}

// ── GeoHashFixed ─────────────────────────────────────────────────────────────

#[test]
fn test_geohash_encode_origin() {
    // (lat=0, lon=0) at precision 1 → 's'
    let gh = GeoHashFixed::encode(0.0, 0.0, 1);
    assert_eq!(gh.precision(), 1);
    let bytes = gh.as_bytes();
    assert_eq!(bytes.len(), 1);
    assert_eq!(bytes[0], b's', "Expected 's', got '{}'", bytes[0] as char);
}

#[test]
fn test_geohash_encode_precision_range() {
    for prec in 1..=12 {
        let gh = GeoHashFixed::encode(48.8566, 2.3522, prec);
        assert_eq!(gh.precision(), prec);
        assert_eq!(gh.as_bytes().len(), prec as usize);
    }
}

#[test]
fn test_geohash_decode_round_trip() {
    // encode then decode should be within the cell error bounds
    for prec in 1..=8u8 {
        let lat_in = 35.6762_f64;
        let lon_in = 139.6503_f64; // Tokyo
        let gh = GeoHashFixed::encode(lat_in, lon_in, prec);
        let (lat_out, lon_out) = gh.decode();
        let bbox = gh.bbox();
        // Decoded centre must be inside the bounding box
        assert!(
            bbox.contains_point(crate_point2d(lon_out, lat_out)),
            "precision={}: ({},{}) not in bbox",
            prec,
            lat_out,
            lon_out
        );
        // The original point must also be inside the bounding box
        assert!(
            bbox.contains_point(crate_point2d(lon_in, lat_in)),
            "precision={}: original ({},{}) not in bbox",
            prec,
            lat_in,
            lon_in
        );
    }
}

#[test]
fn test_geohash_bbox_contains_decoded_center() {
    let gh = GeoHashFixed::encode(51.5074, -0.1278, 6); // London
    let bbox = gh.bbox();
    let (lat, lon) = gh.decode();
    assert!(bbox.contains_point(crate_point2d(lon, lat)));
}

#[test]
fn test_geohash_encode_north_pole_area() {
    let gh = GeoHashFixed::encode(90.0, 0.0, 3);
    assert_eq!(gh.precision(), 3);
    let (lat, _) = gh.decode();
    // Centre latitude should be in northern hemisphere
    assert!(lat > 0.0);
}

#[test]
fn test_geohash_encode_south_hemisphere() {
    let gh = GeoHashFixed::encode(-33.8688, 151.2093, 5); // Sydney
    let bbox = gh.bbox();
    assert!(bbox.min_y < 0.0, "Sydney should be in southern hemisphere");
}

#[test]
fn test_geohash_origin_precision_6() {
    // (0,0) at precision 6 should start with 's00000'
    let gh = GeoHashFixed::encode(0.0, 0.0, 6);
    let bytes = gh.as_bytes();
    assert_eq!(bytes[0], b's');
    assert_eq!(bytes[1], b'0');
    assert_eq!(bytes[2], b'0');
}

#[test]
fn test_geohash_bbox_valid() {
    let gh = GeoHashFixed::encode(40.7128, -74.0060, 8); // New York
    let bbox = gh.bbox();
    assert!(bbox.is_valid());
    assert!(bbox.min_x < bbox.max_x);
    assert!(bbox.min_y < bbox.max_y);
}

// ── NoAllocError ─────────────────────────────────────────────────────────────

#[test]
fn test_noalloc_error_variants() {
    let e = NoAllocError::CapacityExceeded;
    assert_eq!(e, NoAllocError::CapacityExceeded);

    let e2 = NoAllocError::InvalidPrecision;
    assert_ne!(e, e2);
}

// ── Helper ────────────────────────────────────────────────────────────────────

fn crate_point2d(x: f64, y: f64) -> Point2D {
    Point2D::new(x, y)
}
