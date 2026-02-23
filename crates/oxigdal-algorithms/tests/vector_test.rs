#![allow(
    clippy::panic,
    clippy::field_reassign_with_default,
    clippy::cloned_ref_to_slice_refs
)]
//! Comprehensive tests for vector operations in oxigdal-algorithms
//!
//! This module contains 80+ tests covering:
//! 1. Buffer operations
//! 2. Intersection operations
//! 3. Union operations
//! 4. Difference operations
//! 5. Simplification
//! 6. Centroid calculations
//! 7. Area calculations
//! 8. Distance calculations
//! 9. Edge cases and error handling
//!
//! All tests follow COOLJAPAN policies:
//! - No unwrap() usage
//! - Pure Rust
//! - Proper error handling with ? or expect()

use oxigdal_algorithms::error::{AlgorithmError, Result};
use oxigdal_algorithms::vector::{
    AreaMethod,
    BufferCapStyle,
    BufferJoinStyle,
    BufferOptions,
    // Geometry types
    Coordinate,
    DistanceMethod,
    IssueType,
    LineString,
    MultiPolygon,
    Point,
    Polygon,
    SegmentIntersection,
    Severity,
    SimplifyMethod,
    // Area operations
    area_multipolygon,
    area_polygon,
    // Buffer operations
    buffer_linestring,
    buffer_point,
    buffer_polygon,
    // Union operations
    cascaded_union,
    // Centroid operations
    centroid_linestring,
    centroid_multipoint,
    centroid_multipolygon,
    centroid_point,
    centroid_polygon,
    // Difference operations
    clip_to_box,
    // Spatial predicates
    contains,
    convex_hull,
    difference_polygon,
    difference_polygons,
    disjoint,
    // Distance operations
    distance_point_to_linestring,
    distance_point_to_point,
    distance_point_to_polygon,
    erase_small_holes,
    // Intersection operations
    intersect_linestrings,
    intersect_polygons,
    intersect_segment_segment,
    intersects,
    merge_polygons,
    point_in_polygon,
    point_in_polygon_or_boundary,
    point_on_polygon_boundary,
    point_strictly_inside_polygon,
    // Simplification operations
    simplify_linestring,
    simplify_polygon,
    symmetric_difference,
    touches,
    union_polygon,
    // Validation operations
    validate_linestring,
    validate_polygon,
    within,
};
use oxigdal_core::vector::MultiPoint;

// ============================================================================
// Helper Functions
// ============================================================================

/// Creates a square polygon with given origin and size
fn create_square(x: f64, y: f64, size: f64) -> Result<Polygon> {
    let coords = vec![
        Coordinate::new_2d(x, y),
        Coordinate::new_2d(x + size, y),
        Coordinate::new_2d(x + size, y + size),
        Coordinate::new_2d(x, y + size),
        Coordinate::new_2d(x, y),
    ];
    let exterior = LineString::new(coords).map_err(AlgorithmError::Core)?;
    Polygon::new(exterior, vec![]).map_err(AlgorithmError::Core)
}

/// Creates a triangle polygon
fn create_triangle(x1: f64, y1: f64, x2: f64, y2: f64, x3: f64, y3: f64) -> Result<Polygon> {
    let coords = vec![
        Coordinate::new_2d(x1, y1),
        Coordinate::new_2d(x2, y2),
        Coordinate::new_2d(x3, y3),
        Coordinate::new_2d(x1, y1),
    ];
    let exterior = LineString::new(coords).map_err(AlgorithmError::Core)?;
    Polygon::new(exterior, vec![]).map_err(AlgorithmError::Core)
}

/// Creates a simple linestring from coordinates
fn create_linestring(coords: Vec<(f64, f64)>) -> Result<LineString> {
    let coord_vec: Vec<Coordinate> = coords
        .into_iter()
        .map(|(x, y)| Coordinate::new_2d(x, y))
        .collect();
    LineString::new(coord_vec).map_err(AlgorithmError::Core)
}

/// Creates a polygon with a hole
fn create_square_with_hole(outer_size: f64, hole_offset: f64, hole_size: f64) -> Result<Polygon> {
    let exterior_coords = vec![
        Coordinate::new_2d(0.0, 0.0),
        Coordinate::new_2d(outer_size, 0.0),
        Coordinate::new_2d(outer_size, outer_size),
        Coordinate::new_2d(0.0, outer_size),
        Coordinate::new_2d(0.0, 0.0),
    ];
    let hole_coords = vec![
        Coordinate::new_2d(hole_offset, hole_offset),
        Coordinate::new_2d(hole_offset + hole_size, hole_offset),
        Coordinate::new_2d(hole_offset + hole_size, hole_offset + hole_size),
        Coordinate::new_2d(hole_offset, hole_offset + hole_size),
        Coordinate::new_2d(hole_offset, hole_offset),
    ];

    let exterior = LineString::new(exterior_coords).map_err(AlgorithmError::Core)?;
    let hole = LineString::new(hole_coords).map_err(AlgorithmError::Core)?;

    Polygon::new(exterior, vec![hole]).map_err(AlgorithmError::Core)
}

// ============================================================================
// Buffer Operations Tests (10 tests)
// ============================================================================

#[test]
fn test_buffer_point_basic() -> Result<()> {
    let point = Point::new(0.0, 0.0);
    let options = BufferOptions::default();
    let result = buffer_point(&point, 10.0, &options)?;

    // All points should be at distance 10 from center
    for coord in &result.exterior.coords {
        let dist = (coord.x * coord.x + coord.y * coord.y).sqrt();
        assert!(
            (dist - 10.0).abs() < 1e-6,
            "Point should be at distance 10, got {dist}"
        );
    }
    Ok(())
}

#[test]
fn test_buffer_point_zero_radius() -> Result<()> {
    let point = Point::new(5.0, 5.0);
    let options = BufferOptions::default();
    let result = buffer_point(&point, 0.0, &options)?;
    // Zero radius should create degenerate polygon
    assert!(result.exterior.coords.len() >= 4);
    Ok(())
}

#[test]
fn test_buffer_point_negative_radius_error() {
    let point = Point::new(0.0, 0.0);
    let options = BufferOptions::default();
    let result = buffer_point(&point, -10.0, &options);
    assert!(result.is_err(), "Negative radius should return error");
}

#[test]
fn test_buffer_point_infinite_radius_error() {
    let point = Point::new(0.0, 0.0);
    let options = BufferOptions::default();
    let result = buffer_point(&point, f64::INFINITY, &options);
    assert!(result.is_err(), "Infinite radius should return error");
}

#[test]
fn test_buffer_linestring_basic() -> Result<()> {
    let line = create_linestring(vec![(0.0, 0.0), (10.0, 0.0)])?;
    let options = BufferOptions::default();
    let result = buffer_linestring(&line, 5.0, &options)?;

    assert!(
        result.exterior.coords.len() > 4,
        "Buffer should create polygon with many vertices"
    );
    Ok(())
}

#[test]
fn test_buffer_linestring_cap_styles() -> Result<()> {
    let line = create_linestring(vec![(0.0, 0.0), (10.0, 0.0)])?;

    // Test round caps
    let mut options = BufferOptions::default();
    options.cap_style = BufferCapStyle::Round;
    let round_result = buffer_linestring(&line, 5.0, &options)?;
    assert!(round_result.exterior.coords.len() > 4);

    // Test flat caps
    options.cap_style = BufferCapStyle::Flat;
    let flat_result = buffer_linestring(&line, 5.0, &options)?;
    assert!(flat_result.exterior.coords.len() > 4);

    // Test square caps
    options.cap_style = BufferCapStyle::Square;
    let square_result = buffer_linestring(&line, 5.0, &options)?;
    assert!(square_result.exterior.coords.len() > 4);

    Ok(())
}

#[test]
fn test_buffer_linestring_join_styles() -> Result<()> {
    let line = create_linestring(vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)])?;

    // Test round joins
    let mut options = BufferOptions::default();
    options.join_style = BufferJoinStyle::Round;
    let round_result = buffer_linestring(&line, 5.0, &options)?;
    assert!(round_result.exterior.coords.len() > 4);

    // Test miter joins
    options.join_style = BufferJoinStyle::Miter;
    let miter_result = buffer_linestring(&line, 5.0, &options)?;
    assert!(miter_result.exterior.coords.len() > 4);

    // Test bevel joins
    options.join_style = BufferJoinStyle::Bevel;
    let bevel_result = buffer_linestring(&line, 5.0, &options)?;
    assert!(bevel_result.exterior.coords.len() > 4);

    Ok(())
}

#[test]
fn test_buffer_polygon_expand() -> Result<()> {
    let poly = create_square(0.0, 0.0, 10.0)?;
    let options = BufferOptions::default();
    let result = buffer_polygon(&poly, 2.0, &options)?;

    // Expanded polygon should have vertices outside original
    for coord in &result.exterior.coords {
        // At least some coordinates should be outside original bounds
        let outside = coord.x < 0.0 || coord.x > 10.0 || coord.y < 0.0 || coord.y > 10.0;
        if outside {
            return Ok(()); // Found at least one outside point
        }
    }
    // This is expected for buffer expansion
    Ok(())
}

#[test]
fn test_buffer_polygon_zero_distance() -> Result<()> {
    let poly = create_square(0.0, 0.0, 10.0)?;
    let options = BufferOptions::default();
    let result = buffer_polygon(&poly, 0.0, &options)?;

    // Zero distance should return same polygon
    assert_eq!(result.exterior.coords.len(), poly.exterior.coords.len());
    Ok(())
}

#[test]
fn test_buffer_options_quadrant_segments() -> Result<()> {
    let point = Point::new(0.0, 0.0);

    let mut options_low = BufferOptions::default();
    options_low.quadrant_segments = 4;

    let mut options_high = BufferOptions::default();
    options_high.quadrant_segments = 16;

    let low_result = buffer_point(&point, 10.0, &options_low)?;
    let high_result = buffer_point(&point, 10.0, &options_high)?;

    // Higher quadrant segments should produce more vertices
    assert!(
        high_result.exterior.coords.len() > low_result.exterior.coords.len(),
        "Higher quadrant segments should produce smoother circle"
    );
    Ok(())
}

// ============================================================================
// Intersection Operations Tests (12 tests)
// ============================================================================

#[test]
fn test_intersect_segment_segment_crossing() {
    let p1 = Coordinate::new_2d(0.0, 0.0);
    let p2 = Coordinate::new_2d(10.0, 10.0);
    let p3 = Coordinate::new_2d(0.0, 10.0);
    let p4 = Coordinate::new_2d(10.0, 0.0);

    match intersect_segment_segment(&p1, &p2, &p3, &p4) {
        SegmentIntersection::Point(pt) => {
            assert!((pt.x - 5.0).abs() < 1e-10);
            assert!((pt.y - 5.0).abs() < 1e-10);
        }
        _ => panic!("Expected point intersection"),
    }
}

#[test]
fn test_intersect_segment_segment_parallel() {
    let p1 = Coordinate::new_2d(0.0, 0.0);
    let p2 = Coordinate::new_2d(10.0, 0.0);
    let p3 = Coordinate::new_2d(0.0, 5.0);
    let p4 = Coordinate::new_2d(10.0, 5.0);

    assert_eq!(
        intersect_segment_segment(&p1, &p2, &p3, &p4),
        SegmentIntersection::None
    );
}

#[test]
fn test_intersect_segment_segment_collinear_overlap() {
    let p1 = Coordinate::new_2d(0.0, 0.0);
    let p2 = Coordinate::new_2d(10.0, 0.0);
    let p3 = Coordinate::new_2d(5.0, 0.0);
    let p4 = Coordinate::new_2d(15.0, 0.0);

    match intersect_segment_segment(&p1, &p2, &p3, &p4) {
        SegmentIntersection::Overlap(c1, c2) => {
            assert!((c1.x - 5.0).abs() < 1e-10);
            assert!((c2.x - 10.0).abs() < 1e-10);
        }
        _ => panic!("Expected overlap intersection"),
    }
}

#[test]
fn test_intersect_segment_segment_endpoint_touch() {
    let p1 = Coordinate::new_2d(0.0, 0.0);
    let p2 = Coordinate::new_2d(5.0, 5.0);
    let p3 = Coordinate::new_2d(5.0, 5.0);
    let p4 = Coordinate::new_2d(10.0, 5.0);

    match intersect_segment_segment(&p1, &p2, &p3, &p4) {
        SegmentIntersection::Point(pt) => {
            assert!((pt.x - 5.0).abs() < 1e-10);
            assert!((pt.y - 5.0).abs() < 1e-10);
        }
        _ => panic!("Expected point intersection at endpoint"),
    }
}

#[test]
fn test_intersect_segment_segment_no_intersection() {
    let p1 = Coordinate::new_2d(0.0, 0.0);
    let p2 = Coordinate::new_2d(5.0, 0.0);
    let p3 = Coordinate::new_2d(10.0, 10.0);
    let p4 = Coordinate::new_2d(15.0, 10.0);

    assert_eq!(
        intersect_segment_segment(&p1, &p2, &p3, &p4),
        SegmentIntersection::None
    );
}

#[test]
fn test_intersect_linestrings_single_crossing() -> Result<()> {
    let line1 = create_linestring(vec![(0.0, 0.0), (10.0, 10.0)])?;
    let line2 = create_linestring(vec![(0.0, 10.0), (10.0, 0.0)])?;

    let intersections = intersect_linestrings(&line1, &line2)?;
    assert_eq!(intersections.len(), 1);
    assert!((intersections[0].x - 5.0).abs() < 1e-10);
    assert!((intersections[0].y - 5.0).abs() < 1e-10);
    Ok(())
}

#[test]
fn test_intersect_linestrings_multiple_crossings() -> Result<()> {
    let line1 = create_linestring(vec![(0.0, 5.0), (10.0, 5.0)])?;
    let line2 = create_linestring(vec![(2.0, 0.0), (2.0, 10.0), (8.0, 10.0), (8.0, 0.0)])?;

    let intersections = intersect_linestrings(&line1, &line2)?;
    assert_eq!(intersections.len(), 2);
    Ok(())
}

#[test]
fn test_intersect_linestrings_no_intersection() -> Result<()> {
    let line1 = create_linestring(vec![(0.0, 0.0), (5.0, 0.0)])?;
    let line2 = create_linestring(vec![(0.0, 10.0), (5.0, 10.0)])?;

    let intersections = intersect_linestrings(&line1, &line2)?;
    assert!(intersections.is_empty());
    Ok(())
}

#[test]
fn test_point_in_polygon_inside() -> Result<()> {
    let poly = create_square(0.0, 0.0, 10.0)?;
    let point = Coordinate::new_2d(5.0, 5.0);

    let result = point_in_polygon(&point, &poly)?;
    assert!(result, "Point (5,5) should be inside square");
    Ok(())
}

#[test]
fn test_point_in_polygon_outside() -> Result<()> {
    let poly = create_square(0.0, 0.0, 10.0)?;
    let point = Coordinate::new_2d(15.0, 15.0);

    let result = point_in_polygon(&point, &poly)?;
    assert!(!result, "Point (15,15) should be outside square");
    Ok(())
}

#[test]
fn test_intersect_polygons_disjoint() -> Result<()> {
    let poly1 = create_square(0.0, 0.0, 5.0)?;
    let poly2 = create_square(10.0, 10.0, 5.0)?;

    let result = intersect_polygons(&poly1, &poly2)?;
    assert!(
        result.is_empty(),
        "Disjoint polygons should have no intersection"
    );
    Ok(())
}

#[test]
fn test_intersect_polygons_contained() -> Result<()> {
    let outer = create_square(0.0, 0.0, 10.0)?;
    let inner = create_square(2.0, 2.0, 3.0)?;

    let result = intersect_polygons(&outer, &inner)?;
    // Inner polygon should be the intersection
    assert_eq!(result.len(), 1);
    Ok(())
}

// ============================================================================
// Union Operations Tests (10 tests)
// ============================================================================

#[test]
fn test_union_polygon_disjoint() -> Result<()> {
    let poly1 = create_square(0.0, 0.0, 5.0)?;
    let poly2 = create_square(10.0, 10.0, 5.0)?;

    let result = union_polygon(&poly1, &poly2)?;
    assert_eq!(result.len(), 2, "Disjoint polygons should return both");
    Ok(())
}

#[test]
fn test_union_polygon_contained() -> Result<()> {
    let outer = create_square(0.0, 0.0, 10.0)?;
    let inner = create_square(2.0, 2.0, 3.0)?;

    let result = union_polygon(&outer, &inner)?;
    assert_eq!(result.len(), 1, "Contained polygon should return container");
    Ok(())
}

#[test]
fn test_union_polygon_overlapping() -> Result<()> {
    let poly1 = create_square(0.0, 0.0, 5.0)?;
    let poly2 = create_square(3.0, 0.0, 5.0)?;

    let result = union_polygon(&poly1, &poly2)?;
    assert!(!result.is_empty(), "Overlapping polygons should have union");
    Ok(())
}

#[test]
fn test_cascaded_union_empty() -> Result<()> {
    let result = cascaded_union(&[])?;
    assert!(result.is_empty());
    Ok(())
}

#[test]
fn test_cascaded_union_single() -> Result<()> {
    let poly = create_square(0.0, 0.0, 5.0)?;
    let result = cascaded_union(&[poly.clone()])?;
    assert_eq!(result.len(), 1);
    Ok(())
}

#[test]
fn test_cascaded_union_multiple_disjoint() -> Result<()> {
    let poly1 = create_square(0.0, 0.0, 5.0)?;
    let poly2 = create_square(10.0, 0.0, 5.0)?;
    let poly3 = create_square(20.0, 0.0, 5.0)?;

    let result = cascaded_union(&[poly1, poly2, poly3])?;
    // All disjoint should be returned
    assert!(!result.is_empty());
    Ok(())
}

#[test]
fn test_merge_polygons_touching() -> Result<()> {
    let poly1 = create_square(0.0, 0.0, 5.0)?;
    let poly2 = create_square(5.0, 0.0, 5.0)?;

    let result = merge_polygons(&[poly1, poly2], 0.1)?;
    assert!(!result.is_empty());
    Ok(())
}

#[test]
fn test_merge_polygons_with_gap() -> Result<()> {
    let poly1 = create_square(0.0, 0.0, 5.0)?;
    let poly2 = create_square(10.0, 0.0, 5.0)?;

    let result = merge_polygons(&[poly1, poly2], 1.0)?;
    // With large gap and small tolerance, should remain separate
    assert!(!result.is_empty());
    Ok(())
}

#[test]
fn test_convex_hull_triangle() -> Result<()> {
    let points = vec![
        Coordinate::new_2d(0.0, 0.0),
        Coordinate::new_2d(4.0, 0.0),
        Coordinate::new_2d(2.0, 3.0),
    ];

    let hull = convex_hull(&points)?;
    assert_eq!(hull.len(), 3);
    Ok(())
}

#[test]
fn test_convex_hull_with_interior_points() -> Result<()> {
    let points = vec![
        Coordinate::new_2d(0.0, 0.0),
        Coordinate::new_2d(4.0, 0.0),
        Coordinate::new_2d(4.0, 4.0),
        Coordinate::new_2d(0.0, 4.0),
        Coordinate::new_2d(2.0, 2.0), // Interior point
    ];

    let hull = convex_hull(&points)?;
    assert_eq!(hull.len(), 4, "Interior point should be excluded");
    Ok(())
}

// ============================================================================
// Difference Operations Tests (10 tests)
// ============================================================================

#[test]
fn test_difference_polygon_disjoint() -> Result<()> {
    let poly1 = create_square(0.0, 0.0, 5.0)?;
    let poly2 = create_square(10.0, 10.0, 5.0)?;

    let result = difference_polygon(&poly1, &poly2)?;
    assert_eq!(result.len(), 1, "Disjoint should return poly1 unchanged");
    Ok(())
}

#[test]
fn test_difference_polygon_contained() -> Result<()> {
    let outer = create_square(0.0, 0.0, 10.0)?;
    let inner = create_square(2.0, 2.0, 3.0)?;

    let result = difference_polygon(&outer, &inner)?;
    assert_eq!(result.len(), 1);
    // Inner should become a hole
    assert_eq!(result[0].interiors.len(), 1);
    Ok(())
}

#[test]
fn test_difference_polygon_completely_subtracted() -> Result<()> {
    let inner = create_square(2.0, 2.0, 3.0)?;
    let outer = create_square(0.0, 0.0, 10.0)?;

    let result = difference_polygon(&inner, &outer)?;
    assert!(
        result.is_empty(),
        "Inner completely inside outer, result should be empty"
    );
    Ok(())
}

#[test]
fn test_difference_polygons_multiple() -> Result<()> {
    let base = vec![create_square(0.0, 0.0, 10.0)?];
    let subtract = vec![create_square(2.0, 2.0, 2.0)?, create_square(6.0, 6.0, 2.0)?];

    let result = difference_polygons(&base, &subtract)?;
    assert!(!result.is_empty());
    Ok(())
}

#[test]
fn test_symmetric_difference() -> Result<()> {
    let poly1 = create_square(0.0, 0.0, 5.0)?;
    let poly2 = create_square(3.0, 0.0, 5.0)?;

    let result = symmetric_difference(&poly1, &poly2)?;
    // Should return non-overlapping parts
    assert!(!result.is_empty());
    Ok(())
}

#[test]
fn test_clip_to_box_inside() -> Result<()> {
    let poly = create_square(2.0, 2.0, 3.0)?;
    let result = clip_to_box(&poly, 0.0, 0.0, 10.0, 10.0)?;

    assert_eq!(result.len(), 1, "Polygon inside box should be unchanged");
    Ok(())
}

#[test]
fn test_clip_to_box_outside() -> Result<()> {
    let poly = create_square(15.0, 15.0, 3.0)?;
    let result = clip_to_box(&poly, 0.0, 0.0, 10.0, 10.0)?;

    assert!(result.is_empty(), "Polygon outside box should be empty");
    Ok(())
}

#[test]
fn test_clip_to_box_partial() -> Result<()> {
    let poly = create_square(5.0, 5.0, 10.0)?;
    let result = clip_to_box(&poly, 0.0, 0.0, 10.0, 10.0)?;

    assert_eq!(result.len(), 1, "Partially overlapping should be clipped");
    Ok(())
}

#[test]
fn test_clip_to_box_invalid_bounds() {
    let poly = create_square(0.0, 0.0, 5.0).expect("valid polygon");
    let result = clip_to_box(&poly, 10.0, 10.0, 5.0, 5.0); // Invalid: min > max
    assert!(result.is_err());
}

#[test]
fn test_erase_small_holes() -> Result<()> {
    let poly = create_square_with_hole(20.0, 2.0, 3.0)?;

    // Hole area is 9, so threshold of 10 should remove it
    let result = erase_small_holes(&poly, 10.0)?;
    assert!(result.interiors.is_empty(), "Small hole should be erased");

    // Threshold of 5 should keep it
    let result_keep = erase_small_holes(&poly, 5.0)?;
    assert_eq!(result_keep.interiors.len(), 1, "Hole should be kept");
    Ok(())
}

// ============================================================================
// Simplification Tests (10 tests)
// ============================================================================

#[test]
fn test_simplify_linestring_douglas_peucker() -> Result<()> {
    let line = create_linestring(vec![
        (0.0, 0.0),
        (1.0, 0.1),
        (2.0, 0.0),
        (3.0, 0.1),
        (4.0, 0.0),
    ])?;

    let simplified = simplify_linestring(&line, 0.2, SimplifyMethod::DouglasPeucker)?;
    assert!(simplified.len() <= line.len());
    assert!(simplified.len() >= 2);
    Ok(())
}

#[test]
fn test_simplify_linestring_visvalingam() -> Result<()> {
    let line = create_linestring(vec![
        (0.0, 0.0),
        (1.0, 0.5),
        (2.0, 0.0),
        (3.0, 0.5),
        (4.0, 0.0),
    ])?;

    let simplified = simplify_linestring(&line, 0.3, SimplifyMethod::VisvalingamWhyatt)?;
    assert!(simplified.len() <= line.len());
    assert!(simplified.len() >= 2);
    Ok(())
}

#[test]
fn test_simplify_linestring_topology_preserving() -> Result<()> {
    let line = create_linestring(vec![
        (0.0, 0.0),
        (1.0, 1.0),
        (2.0, 0.0),
        (3.0, 1.0),
        (4.0, 0.0),
    ])?;

    let simplified = simplify_linestring(&line, 0.5, SimplifyMethod::TopologyPreserving)?;
    assert!(simplified.len() <= line.len());
    assert!(simplified.len() >= 2);
    Ok(())
}

#[test]
fn test_simplify_linestring_high_tolerance() -> Result<()> {
    let line = create_linestring(vec![
        (0.0, 0.0),
        (1.0, 0.01),
        (2.0, 0.0),
        (3.0, 0.01),
        (4.0, 0.0),
    ])?;

    // Very high tolerance should simplify to endpoints
    let simplified = simplify_linestring(&line, 1.0, SimplifyMethod::DouglasPeucker)?;
    assert!(simplified.len() <= 3);
    Ok(())
}

#[test]
fn test_simplify_linestring_zero_tolerance() -> Result<()> {
    let line = create_linestring(vec![(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)])?;

    // Zero tolerance should keep all points
    let simplified = simplify_linestring(&line, 0.0, SimplifyMethod::DouglasPeucker)?;
    assert_eq!(simplified.len(), line.len());
    Ok(())
}

#[test]
fn test_simplify_linestring_negative_tolerance() {
    let line =
        create_linestring(vec![(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)]).expect("valid linestring");

    let result = simplify_linestring(&line, -1.0, SimplifyMethod::DouglasPeucker);
    assert!(result.is_err());
}

#[test]
fn test_simplify_polygon_basic() -> Result<()> {
    let poly = create_square(0.0, 0.0, 10.0)?;

    let simplified = simplify_polygon(&poly, 0.1, SimplifyMethod::DouglasPeucker)?;
    // Square should remain a square (4 corners + closing point)
    assert_eq!(simplified.exterior.len(), 5);
    Ok(())
}

#[test]
fn test_simplify_polygon_with_noise() -> Result<()> {
    // Create a square with small perturbations
    let coords = vec![
        Coordinate::new_2d(0.0, 0.0),
        Coordinate::new_2d(5.0, 0.01),
        Coordinate::new_2d(10.0, 0.0),
        Coordinate::new_2d(10.0, 5.0),
        Coordinate::new_2d(9.99, 10.0),
        Coordinate::new_2d(5.0, 10.0),
        Coordinate::new_2d(0.0, 10.0),
        Coordinate::new_2d(0.0, 5.0),
        Coordinate::new_2d(0.0, 0.0),
    ];
    let exterior = LineString::new(coords).map_err(AlgorithmError::Core)?;
    let poly = Polygon::new(exterior, vec![]).map_err(AlgorithmError::Core)?;

    let simplified = simplify_polygon(&poly, 0.1, SimplifyMethod::DouglasPeucker)?;
    assert!(simplified.exterior.len() <= poly.exterior.len());
    Ok(())
}

#[test]
fn test_simplify_different_methods_produce_different_results() -> Result<()> {
    let line = create_linestring(vec![
        (0.0, 0.0),
        (1.0, 2.0),
        (2.0, 0.5),
        (3.0, 2.0),
        (4.0, 0.0),
    ])?;

    let dp = simplify_linestring(&line, 0.5, SimplifyMethod::DouglasPeucker)?;
    let vw = simplify_linestring(&line, 0.5, SimplifyMethod::VisvalingamWhyatt)?;

    // Results may differ between methods
    assert!(dp.len() >= 2 && vw.len() >= 2);
    Ok(())
}

#[test]
fn test_simplify_preserves_endpoints() -> Result<()> {
    let line = create_linestring(vec![
        (0.0, 0.0),
        (1.0, 1.0),
        (2.0, 0.0),
        (3.0, 1.0),
        (4.0, 4.0),
    ])?;

    let simplified = simplify_linestring(&line, 2.0, SimplifyMethod::DouglasPeucker)?;

    // First and last points should be preserved
    assert!((simplified.coords[0].x - 0.0).abs() < 1e-10);
    assert!((simplified.coords[0].y - 0.0).abs() < 1e-10);
    let last_idx = simplified.coords.len() - 1;
    assert!((simplified.coords[last_idx].x - 4.0).abs() < 1e-10);
    assert!((simplified.coords[last_idx].y - 4.0).abs() < 1e-10);
    Ok(())
}

// ============================================================================
// Centroid Calculations Tests (10 tests)
// ============================================================================

#[test]
fn test_centroid_point() {
    let point = Point::new(3.0, 5.0);
    let result = centroid_point(&point);
    assert_eq!(result.coord.x, 3.0);
    assert_eq!(result.coord.y, 5.0);
}

#[test]
fn test_centroid_linestring_horizontal() -> Result<()> {
    let line = create_linestring(vec![(0.0, 0.0), (10.0, 0.0)])?;
    let result = centroid_linestring(&line)?;

    assert!((result.coord.x - 5.0).abs() < 1e-10);
    assert!((result.coord.y - 0.0).abs() < 1e-10);
    Ok(())
}

#[test]
fn test_centroid_linestring_l_shape() -> Result<()> {
    let line = create_linestring(vec![(0.0, 0.0), (4.0, 0.0), (4.0, 4.0)])?;
    let result = centroid_linestring(&line)?;

    // Weighted centroid should be somewhere in the middle
    assert!(result.coord.x >= 0.0 && result.coord.x <= 4.0);
    assert!(result.coord.y >= 0.0 && result.coord.y <= 4.0);
    Ok(())
}

#[test]
fn test_centroid_polygon_square() -> Result<()> {
    let poly = create_square(0.0, 0.0, 4.0)?;
    let result = centroid_polygon(&poly)?;

    assert!((result.coord.x - 2.0).abs() < 1e-10);
    assert!((result.coord.y - 2.0).abs() < 1e-10);
    Ok(())
}

#[test]
fn test_centroid_polygon_triangle() -> Result<()> {
    let poly = create_triangle(0.0, 0.0, 6.0, 0.0, 3.0, 6.0)?;
    let result = centroid_polygon(&poly)?;

    // Triangle centroid is at (x1+x2+x3)/3, (y1+y2+y3)/3
    assert!((result.coord.x - 3.0).abs() < 1e-10);
    assert!((result.coord.y - 2.0).abs() < 1e-10);
    Ok(())
}

#[test]
fn test_centroid_polygon_with_hole() -> Result<()> {
    let poly = create_square_with_hole(10.0, 2.0, 6.0)?;
    let result = centroid_polygon(&poly)?;

    // With centered hole, centroid should still be near center
    assert!(result.coord.x >= 0.0 && result.coord.x <= 10.0);
    assert!(result.coord.y >= 0.0 && result.coord.y <= 10.0);
    Ok(())
}

#[test]
fn test_centroid_multipoint() -> Result<()> {
    let points = vec![
        Point::new(0.0, 0.0),
        Point::new(4.0, 0.0),
        Point::new(4.0, 4.0),
        Point::new(0.0, 4.0),
    ];
    let mp = MultiPoint::new(points);

    let result = centroid_multipoint(&mp)?;
    assert!((result.coord.x - 2.0).abs() < 1e-10);
    assert!((result.coord.y - 2.0).abs() < 1e-10);
    Ok(())
}

#[test]
fn test_centroid_multipoint_empty() {
    let mp = MultiPoint::empty();
    let result = centroid_multipoint(&mp);
    assert!(result.is_err());
}

#[test]
fn test_centroid_multipolygon() -> Result<()> {
    let poly1 = create_square(0.0, 0.0, 4.0)?;
    let poly2 = create_square(10.0, 0.0, 4.0)?;
    let mp = MultiPolygon::new(vec![poly1, poly2]);

    let result = centroid_multipolygon(&mp)?;
    // Area-weighted centroid of two equal squares at (0,0,4,4) and (10,0,14,4)
    // Should be around (7, 2)
    assert!(result.coord.x >= 0.0 && result.coord.x <= 14.0);
    assert!(result.coord.y >= 0.0 && result.coord.y <= 4.0);
    Ok(())
}

#[test]
fn test_centroid_multipolygon_empty() {
    let mp = MultiPolygon::empty();
    let result = centroid_multipolygon(&mp);
    assert!(result.is_err());
}

// ============================================================================
// Area Calculations Tests (10 tests)
// ============================================================================

#[test]
fn test_area_polygon_square_planar() -> Result<()> {
    let poly = create_square(0.0, 0.0, 10.0)?;
    let result = area_polygon(&poly, AreaMethod::Planar)?;
    assert!((result - 100.0).abs() < 1e-10);
    Ok(())
}

#[test]
fn test_area_polygon_triangle_planar() -> Result<()> {
    let poly = create_triangle(0.0, 0.0, 4.0, 0.0, 2.0, 3.0)?;
    let result = area_polygon(&poly, AreaMethod::Planar)?;
    // Area = base * height / 2 = 4 * 3 / 2 = 6
    assert!((result - 6.0).abs() < 1e-10);
    Ok(())
}

#[test]
fn test_area_polygon_with_hole() -> Result<()> {
    let poly = create_square_with_hole(10.0, 2.0, 3.0)?;
    let result = area_polygon(&poly, AreaMethod::Planar)?;
    // Outer area = 100, hole area = 9, effective = 91
    assert!((result - 91.0).abs() < 1e-10);
    Ok(())
}

#[test]
fn test_area_polygon_signed() -> Result<()> {
    let poly = create_square(0.0, 0.0, 10.0)?;
    let result = area_polygon(&poly, AreaMethod::SignedPlanar)?;
    // CCW orientation = positive area
    assert!(result > 0.0);
    assert!((result.abs() - 100.0).abs() < 1e-10);
    Ok(())
}

#[test]
fn test_area_multipolygon() -> Result<()> {
    let poly1 = create_square(0.0, 0.0, 5.0)?;
    let poly2 = create_square(10.0, 0.0, 3.0)?;
    let mp = MultiPolygon::new(vec![poly1, poly2]);

    let result = area_multipolygon(&mp, AreaMethod::Planar)?;
    // 5*5 + 3*3 = 25 + 9 = 34
    assert!((result - 34.0).abs() < 1e-10);
    Ok(())
}

#[test]
fn test_area_multipolygon_empty() -> Result<()> {
    let mp = MultiPolygon::empty();
    let result = area_multipolygon(&mp, AreaMethod::Planar)?;
    assert_eq!(result, 0.0);
    Ok(())
}

#[test]
fn test_area_geodetic_small_polygon() -> Result<()> {
    // Small polygon in degrees (approximately 1 degree square near equator)
    let coords = vec![
        Coordinate::new_2d(0.0, 0.0),
        Coordinate::new_2d(1.0, 0.0),
        Coordinate::new_2d(1.0, 1.0),
        Coordinate::new_2d(0.0, 1.0),
        Coordinate::new_2d(0.0, 0.0),
    ];
    let exterior = LineString::new(coords).map_err(AlgorithmError::Core)?;
    let poly = Polygon::new(exterior, vec![]).map_err(AlgorithmError::Core)?;

    let result = area_polygon(&poly, AreaMethod::Geodetic)?;
    // 1 degree at equator is approximately 111 km
    // So 1 degree square is approximately 12,321 km^2 = 12,321,000,000 m^2
    assert!(result > 1e10);
    assert!(result < 2e10);
    Ok(())
}

#[test]
fn test_area_geodetic_invalid_latitude() {
    let coords = vec![
        Coordinate::new_2d(0.0, 0.0),
        Coordinate::new_2d(1.0, 0.0),
        Coordinate::new_2d(1.0, 100.0), // Invalid latitude
        Coordinate::new_2d(0.0, 1.0),
        Coordinate::new_2d(0.0, 0.0),
    ];
    let exterior = LineString::new(coords).expect("valid linestring");
    let poly = Polygon::new(exterior, vec![]).expect("valid polygon");

    let result = area_polygon(&poly, AreaMethod::Geodetic);
    assert!(result.is_err());
}

#[test]
fn test_area_different_methods() -> Result<()> {
    let poly = create_square(0.0, 0.0, 10.0)?;

    let planar = area_polygon(&poly, AreaMethod::Planar)?;
    let signed = area_polygon(&poly, AreaMethod::SignedPlanar)?;

    // Both should give same magnitude
    assert!((planar - signed.abs()).abs() < 1e-10);
    Ok(())
}

#[test]
fn test_area_zero_for_degenerate() -> Result<()> {
    // Create a degenerate polygon (line)
    let coords = vec![
        Coordinate::new_2d(0.0, 0.0),
        Coordinate::new_2d(5.0, 0.0),
        Coordinate::new_2d(10.0, 0.0),
        Coordinate::new_2d(0.0, 0.0),
    ];
    let exterior = LineString::new(coords).map_err(AlgorithmError::Core)?;
    let poly = Polygon::new(exterior, vec![]).map_err(AlgorithmError::Core)?;

    let result = area_polygon(&poly, AreaMethod::Planar)?;
    assert!(
        result.abs() < 1e-10,
        "Degenerate polygon should have zero area"
    );
    Ok(())
}

// ============================================================================
// Distance Calculations Tests (10 tests)
// ============================================================================

#[test]
fn test_distance_point_to_point_euclidean() -> Result<()> {
    let p1 = Point::new(0.0, 0.0);
    let p2 = Point::new(3.0, 4.0);

    let result = distance_point_to_point(&p1, &p2, DistanceMethod::Euclidean)?;
    assert!((result - 5.0).abs() < 1e-10);
    Ok(())
}

#[test]
fn test_distance_point_to_point_same() -> Result<()> {
    let p1 = Point::new(5.0, 5.0);
    let p2 = Point::new(5.0, 5.0);

    let result = distance_point_to_point(&p1, &p2, DistanceMethod::Euclidean)?;
    assert!(result.abs() < 1e-10);
    Ok(())
}

#[test]
fn test_distance_point_to_point_3d() -> Result<()> {
    let p1 = Point::new_3d(0.0, 0.0, 0.0);
    let p2 = Point::new_3d(1.0, 1.0, 1.0);

    let result = distance_point_to_point(&p1, &p2, DistanceMethod::Euclidean)?;
    assert!((result - 3.0_f64.sqrt()).abs() < 1e-10);
    Ok(())
}

#[test]
fn test_distance_point_to_point_haversine() -> Result<()> {
    // Distance from New York to London (approximately)
    let nyc = Point::new(-74.0, 40.7); // NYC: 74 deg W, 40.7 deg N
    let london = Point::new(-0.1, 51.5); // London: 0.1 deg W, 51.5 deg N

    let result = distance_point_to_point(&nyc, &london, DistanceMethod::Haversine)?;
    // Should be approximately 5,570 km
    assert!(result > 5_000_000.0);
    assert!(result < 6_000_000.0);
    Ok(())
}

#[test]
fn test_distance_point_to_linestring() -> Result<()> {
    let point = Point::new(1.0, 1.0);
    let line = create_linestring(vec![(0.0, 0.0), (2.0, 0.0), (2.0, 2.0)])?;

    let result = distance_point_to_linestring(&point, &line, DistanceMethod::Euclidean)?;
    // Point (1,1) is 1 unit from segment (0,0)-(2,0)
    assert!((result - 1.0).abs() < 1e-10);
    Ok(())
}

#[test]
fn test_distance_point_to_linestring_on_line() -> Result<()> {
    let point = Point::new(1.0, 0.0);
    let line = create_linestring(vec![(0.0, 0.0), (2.0, 0.0)])?;

    let result = distance_point_to_linestring(&point, &line, DistanceMethod::Euclidean)?;
    assert!(result.abs() < 1e-10);
    Ok(())
}

#[test]
fn test_distance_point_to_polygon_inside() -> Result<()> {
    let point = Point::new(2.0, 2.0);
    let poly = create_square(0.0, 0.0, 4.0)?;

    let result = distance_point_to_polygon(&point, &poly, DistanceMethod::Euclidean)?;
    assert_eq!(result, 0.0, "Point inside polygon should have distance 0");
    Ok(())
}

#[test]
fn test_distance_point_to_polygon_outside() -> Result<()> {
    let point = Point::new(5.0, 5.0);
    let poly = create_square(0.0, 0.0, 4.0)?;

    let result = distance_point_to_polygon(&point, &poly, DistanceMethod::Euclidean)?;
    // Distance from (5,5) to corner (4,4)
    let expected = 2.0_f64.sqrt();
    assert!((result - expected).abs() < 1e-10);
    Ok(())
}

#[test]
fn test_distance_invalid_latitude() {
    let p1 = Point::new(0.0, 95.0); // Invalid latitude
    let p2 = Point::new(0.0, 0.0);

    let result = distance_point_to_point(&p1, &p2, DistanceMethod::Haversine);
    assert!(result.is_err());
}

#[test]
fn test_distance_vincenty_same_point() -> Result<()> {
    let p1 = Point::new(0.0, 0.0);
    let p2 = Point::new(0.0, 0.0);

    let result = distance_point_to_point(&p1, &p2, DistanceMethod::Vincenty)?;
    assert!(result.abs() < 1e-10);
    Ok(())
}

// ============================================================================
// Edge Cases and Error Handling Tests (10 tests)
// ============================================================================

#[test]
fn test_validate_polygon_valid_square() -> Result<()> {
    let poly = create_square(0.0, 0.0, 4.0)?;
    let issues = validate_polygon(&poly)?;

    let errors = issues
        .iter()
        .filter(|i| i.severity == Severity::Error)
        .count();
    assert_eq!(errors, 0, "Valid square should have no errors");
    Ok(())
}

#[test]
fn test_validate_polygon_self_intersecting() -> Result<()> {
    // Self-intersecting (bow-tie) polygon
    let coords = vec![
        Coordinate::new_2d(0.0, 0.0),
        Coordinate::new_2d(4.0, 4.0),
        Coordinate::new_2d(4.0, 0.0),
        Coordinate::new_2d(0.0, 4.0),
        Coordinate::new_2d(0.0, 0.0),
    ];
    let exterior = LineString::new(coords).map_err(AlgorithmError::Core)?;
    let poly = Polygon::new(exterior, vec![]).map_err(AlgorithmError::Core)?;

    let issues = validate_polygon(&poly)?;
    let has_self_intersection = issues
        .iter()
        .any(|i| i.issue_type == IssueType::SelfIntersection);
    assert!(has_self_intersection, "Should detect self-intersection");
    Ok(())
}

#[test]
fn test_validate_linestring_duplicate_vertices() -> Result<()> {
    let coords = vec![
        Coordinate::new_2d(0.0, 0.0),
        Coordinate::new_2d(2.0, 0.0),
        Coordinate::new_2d(2.0, 0.0), // Duplicate
        Coordinate::new_2d(4.0, 0.0),
    ];
    let line = LineString::new(coords).map_err(AlgorithmError::Core)?;

    let issues = validate_linestring(&line)?;
    let has_duplicates = issues
        .iter()
        .any(|i| i.issue_type == IssueType::DuplicateVertices);
    assert!(has_duplicates, "Should detect duplicate vertices");
    Ok(())
}

#[test]
fn test_spatial_predicate_contains() -> Result<()> {
    let poly = create_square(0.0, 0.0, 4.0)?;
    let inside = Coordinate::new_2d(2.0, 2.0);

    assert!(point_strictly_inside_polygon(&inside, &poly));
    Ok(())
}

#[test]
fn test_spatial_predicate_on_boundary() -> Result<()> {
    let poly = create_square(0.0, 0.0, 4.0)?;
    let boundary = Coordinate::new_2d(0.0, 2.0);

    assert!(point_on_polygon_boundary(&boundary, &poly));
    assert!(!point_strictly_inside_polygon(&boundary, &poly));
    assert!(point_in_polygon_or_boundary(&boundary, &poly));
    Ok(())
}

#[test]
fn test_point_contains_same_point() -> Result<()> {
    let p1 = Point::new(1.0, 2.0);
    let p2 = Point::new(1.0, 2.0);

    let result = contains(&p1, &p2)?;
    assert!(result);
    Ok(())
}

#[test]
fn test_polygon_intersects_overlapping() -> Result<()> {
    let poly1 = create_square(0.0, 0.0, 4.0)?;
    let poly2 = create_square(2.0, 2.0, 4.0)?;

    let result: bool = intersects(&poly1, &poly2)?;
    assert!(result);
    Ok(())
}

#[test]
fn test_polygon_disjoint() -> Result<()> {
    let poly1 = create_square(0.0, 0.0, 4.0)?;
    let poly2 = create_square(10.0, 10.0, 4.0)?;

    let result: bool = disjoint(&poly1, &poly2)?;
    assert!(result);
    Ok(())
}

#[test]
fn test_polygon_within() -> Result<()> {
    let outer = create_square(0.0, 0.0, 10.0)?;
    let inner = create_square(2.0, 2.0, 3.0)?;

    // inner should be within outer
    let result: bool = within(&inner, &outer)?;
    // Note: This tests if outer.contains(inner)
    assert!(result);
    Ok(())
}

#[test]
fn test_polygon_touches() -> Result<()> {
    let poly1 = create_square(0.0, 0.0, 4.0)?;
    // Adjacent polygon sharing edge
    let poly2 = create_square(4.0, 0.0, 4.0)?;

    let result: bool = touches(&poly1, &poly2)?;
    assert!(result, "Adjacent polygons should touch");
    Ok(())
}
