//! Integration tests for geometry operations
//!
//! Tests for simplification, centroid, convex hull, and envelope operations.

use oxigdal_algorithms::vector::{
    Coordinate, LineString, Point, Polygon, SimplifyMethod, centroid_linestring, centroid_polygon,
    convex_hull, envelope, envelope_linestring, envelope_polygon, simplify_linestring,
    simplify_polygon,
};
use oxigdal_core::vector::Geometry;

#[test]
fn test_linestring_operations() {
    // Create a complex linestring
    let coords = vec![
        Coordinate::new_2d(0.0, 0.0),
        Coordinate::new_2d(1.0, 0.1),
        Coordinate::new_2d(2.0, 0.0),
        Coordinate::new_2d(3.0, 0.1),
        Coordinate::new_2d(4.0, 0.0),
        Coordinate::new_2d(5.0, 0.1),
        Coordinate::new_2d(6.0, 0.0),
    ];

    let line = LineString::new(coords).expect("Failed to create linestring");

    // Test simplification
    let simplified =
        simplify_linestring(&line, 0.2, SimplifyMethod::DouglasPeucker).expect("Simplify failed");
    assert!(simplified.len() >= 2);
    assert!(simplified.len() <= line.len());

    // Test centroid
    let centroid = centroid_linestring(&line).expect("Centroid failed");
    assert!(centroid.coord.x >= 0.0 && centroid.coord.x <= 6.0);
    assert!(centroid.coord.y >= 0.0 && centroid.coord.y <= 0.1);

    // Test envelope
    let envelope = envelope_linestring(&line).expect("Envelope failed");
    let bounds = envelope.bounds().expect("Bounds failed");
    assert_eq!(bounds.0, 0.0); // min_x
    assert_eq!(bounds.2, 6.0); // max_x
}

#[test]
fn test_polygon_operations() {
    // Create a polygon
    let coords = vec![
        Coordinate::new_2d(0.0, 0.0),
        Coordinate::new_2d(10.0, 0.0),
        Coordinate::new_2d(10.0, 10.0),
        Coordinate::new_2d(5.0, 15.0),
        Coordinate::new_2d(0.0, 10.0),
        Coordinate::new_2d(0.0, 0.0),
    ];

    let exterior = LineString::new(coords).expect("Failed to create linestring");
    let polygon = Polygon::new(exterior, vec![]).expect("Failed to create polygon");

    // Test simplification
    let simplified =
        simplify_polygon(&polygon, 0.5, SimplifyMethod::DouglasPeucker).expect("Simplify failed");
    assert!(simplified.exterior.len() >= 4); // At least 4 points for a valid polygon

    // Test centroid
    let centroid = centroid_polygon(&polygon).expect("Centroid failed");
    assert!(centroid.coord.x >= 0.0 && centroid.coord.x <= 10.0);
    assert!(centroid.coord.y >= 0.0 && centroid.coord.y <= 15.0);

    // Test envelope
    let envelope = envelope_polygon(&polygon).expect("Envelope failed");
    let bounds = envelope.bounds().expect("Bounds failed");
    assert_eq!(bounds.0, 0.0); // min_x
    assert_eq!(bounds.1, 0.0); // min_y
    assert_eq!(bounds.2, 10.0); // max_x
    assert_eq!(bounds.3, 15.0); // max_y
}

#[test]
fn test_convex_hull_operations() {
    // Create a set of points that form a non-convex shape
    let points = vec![
        Coordinate::new_2d(0.0, 0.0),
        Coordinate::new_2d(2.0, 0.0),
        Coordinate::new_2d(1.0, 1.0), // Interior point
        Coordinate::new_2d(2.0, 2.0),
        Coordinate::new_2d(0.0, 2.0),
    ];

    // Compute convex hull
    let hull = convex_hull(&points).expect("Convex hull failed");

    // Hull should exclude the interior point
    assert_eq!(hull.len(), 4); // Four corners of the square

    // Create polygon from hull for further operations
    let mut hull_ring = hull.clone();
    hull_ring.push(hull[0]); // Close the ring

    let hull_line = LineString::new(hull_ring).expect("Failed to create hull linestring");
    let hull_polygon = Polygon::new(hull_line, vec![]).expect("Failed to create hull polygon");

    // Test centroid of convex hull
    let centroid = centroid_polygon(&hull_polygon).expect("Centroid failed");
    assert!((centroid.coord.x - 1.0).abs() < 0.1);
    assert!((centroid.coord.y - 1.0).abs() < 0.1);

    // Test envelope of convex hull
    let envelope = envelope_polygon(&hull_polygon).expect("Envelope failed");
    let bounds = envelope.bounds().expect("Bounds failed");
    assert_eq!(bounds.0, 0.0);
    assert_eq!(bounds.1, 0.0);
    assert_eq!(bounds.2, 2.0);
    assert_eq!(bounds.3, 2.0);
}

#[test]
fn test_geometry_dispatch() {
    // Test that envelope works with the generic Geometry enum
    let point = Point::new(5.0, 5.0);
    let geom = Geometry::Point(point);

    let env = envelope(&geom).expect("Envelope failed");
    let bounds = env.bounds().expect("Bounds failed");

    assert_eq!(bounds.0, 5.0);
    assert_eq!(bounds.1, 5.0);
    assert_eq!(bounds.2, 5.0);
    assert_eq!(bounds.3, 5.0);
}

#[test]
fn test_simplify_then_envelope() {
    // Test workflow: simplify geometry, then compute envelope
    let coords = vec![
        Coordinate::new_2d(0.0, 0.0),
        Coordinate::new_2d(1.0, 0.05),
        Coordinate::new_2d(2.0, 0.0),
        Coordinate::new_2d(3.0, 0.05),
        Coordinate::new_2d(4.0, 0.0),
    ];

    let line = LineString::new(coords).expect("Failed to create linestring");

    // Simplify first
    let simplified =
        simplify_linestring(&line, 0.1, SimplifyMethod::DouglasPeucker).expect("Simplify failed");

    // Then compute envelope
    let env = envelope_linestring(&simplified).expect("Envelope failed");
    let bounds = env.bounds().expect("Bounds failed");

    // Envelope should still cover the simplified geometry
    assert!(bounds.0 <= 0.0);
    assert!(bounds.2 >= 4.0);
}

#[test]
fn test_convex_hull_then_centroid() {
    // Test workflow: compute convex hull, then find centroid
    let points = vec![
        Coordinate::new_2d(0.0, 0.0),
        Coordinate::new_2d(4.0, 0.0),
        Coordinate::new_2d(4.0, 4.0),
        Coordinate::new_2d(0.0, 4.0),
        Coordinate::new_2d(2.0, 2.0), // Interior point
    ];

    // Compute convex hull
    let hull = convex_hull(&points).expect("Convex hull failed");

    // Create polygon from hull
    let mut hull_ring = hull.clone();
    hull_ring.push(hull[0]);

    let hull_line = LineString::new(hull_ring).expect("Failed to create hull linestring");
    let hull_polygon = Polygon::new(hull_line, vec![]).expect("Failed to create hull polygon");

    // Compute centroid
    let centroid = centroid_polygon(&hull_polygon).expect("Centroid failed");

    // For a square, centroid should be at (2, 2)
    assert!((centroid.coord.x - 2.0).abs() < 0.1);
    assert!((centroid.coord.y - 2.0).abs() < 0.1);
}

#[test]
fn test_all_operations_together() {
    // Complex workflow: create geometry, simplify, find convex hull, compute centroid and envelope

    // 1. Create a complex shape
    let coords = vec![
        Coordinate::new_2d(0.0, 0.0),
        Coordinate::new_2d(1.0, 0.1),
        Coordinate::new_2d(2.0, 0.0),
        Coordinate::new_2d(3.0, 1.0),
        Coordinate::new_2d(4.0, 0.0),
        Coordinate::new_2d(5.0, 0.1),
        Coordinate::new_2d(6.0, 0.0),
        Coordinate::new_2d(5.0, 3.0),
        Coordinate::new_2d(3.0, 4.0),
        Coordinate::new_2d(1.0, 3.0),
        Coordinate::new_2d(0.0, 0.0),
    ];

    let line = LineString::new(coords.clone()).expect("Failed to create linestring");

    // 2. Simplify
    let simplified =
        simplify_linestring(&line, 0.5, SimplifyMethod::DouglasPeucker).expect("Simplify failed");
    assert!(simplified.len() < line.len());

    // 3. Compute convex hull of original points
    let hull = convex_hull(&coords).expect("Convex hull failed");
    assert!(hull.len() <= coords.len());

    // 4. Create polygon from hull
    let mut hull_ring = hull.clone();
    hull_ring.push(hull[0]);
    let hull_line = LineString::new(hull_ring).expect("Failed to create hull linestring");
    let hull_polygon = Polygon::new(hull_line, vec![]).expect("Failed to create hull polygon");

    // 5. Compute centroid of hull
    let centroid = centroid_polygon(&hull_polygon).expect("Centroid failed");
    assert!(centroid.coord.x >= 0.0 && centroid.coord.x <= 6.0);
    assert!(centroid.coord.y >= 0.0 && centroid.coord.y <= 4.0);

    // 6. Compute envelope of hull
    let env = envelope_polygon(&hull_polygon).expect("Envelope failed");
    let bounds = env.bounds().expect("Bounds failed");

    // Envelope should contain the convex hull (which may be smaller than original shape)
    // Just verify it's a valid bounding box
    assert!(bounds.0 <= bounds.2); // min_x <= max_x
    assert!(bounds.1 <= bounds.3); // min_y <= max_y
    assert!(bounds.0 >= 0.0 - 0.1); // Allow small tolerance
    assert!(bounds.1 >= 0.0 - 0.1);
    assert!(bounds.2 <= 6.0 + 0.1);
    assert!(bounds.3 <= 4.0 + 0.1);

    // All operations completed successfully!
}

#[test]
fn test_error_handling() {
    // Test that operations properly handle error cases

    // Empty linestring should fail
    let empty_coords: Vec<Coordinate> = vec![];
    let empty_line = LineString::new(empty_coords);
    assert!(empty_line.is_err());

    // Insufficient points for convex hull
    let few_points = vec![Coordinate::new_2d(0.0, 0.0), Coordinate::new_2d(1.0, 1.0)];
    let hull = convex_hull(&few_points);
    assert!(hull.is_err());

    // Negative tolerance for simplification
    let coords = vec![
        Coordinate::new_2d(0.0, 0.0),
        Coordinate::new_2d(1.0, 1.0),
        Coordinate::new_2d(2.0, 0.0),
    ];
    let line = LineString::new(coords).expect("Failed to create linestring");
    let result = simplify_linestring(&line, -1.0, SimplifyMethod::DouglasPeucker);
    assert!(result.is_err());
}
