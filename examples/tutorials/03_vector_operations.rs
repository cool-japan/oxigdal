//! Tutorial 03: Vector Operations
//!
//! This tutorial demonstrates vector/geometry operations:
//! - Creating and reading vector data (GeoJSON, Shapefile)
//! - Buffer operations
//! - Intersection, union, difference
//! - Spatial queries and filtering
//! - Vector-raster conversions
//!
//! Run with:
//! ```bash
//! cargo run --example 03_vector_operations
//! ```

use geo::geometry::{LineString, MultiPolygon, Point, Polygon};
use geo::{Area, BoundingRect, Contains, Intersects};
use geo_types::Coord;
use geojson::{Feature, FeatureCollection, GeoJson, Geometry};
use oxigdal_core::types::BoundingBox;
use oxigdal_core::vector::{VectorGeometry, VectorLayer};
use std::env;
use std::fs::File;
use std::io::Write;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Tutorial 03: Vector Operations ===\n");

    let temp_dir = env::temp_dir();

    // Step 1: Creating Vector Geometries
    println!("Step 1: Creating Vector Geometries");
    println!("-----------------------------------");

    // Create some points
    let point1 = Point::new(0.0, 0.0);
    let point2 = Point::new(1.0, 1.0);
    let point3 = Point::new(2.0, 0.5);

    println!("Created points:");
    println!("  P1: ({}, {})", point1.x(), point1.y());
    println!("  P2: ({}, {})", point2.x(), point2.y());
    println!("  P3: ({}, {})", point3.x(), point3.y());

    // Create a line string
    let line = LineString::from(vec![
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 1.0, y: 1.0 },
        Coord { x: 2.0, y: 1.0 },
        Coord { x: 3.0, y: 0.0 },
    ]);

    println!("\nCreated line with {} points", line.coords_count());

    // Create a polygon
    let exterior = LineString::from(vec![
        Coord { x: 0.0, y: 0.0 },
        Coord { x: 4.0, y: 0.0 },
        Coord { x: 4.0, y: 3.0 },
        Coord { x: 0.0, y: 3.0 },
        Coord { x: 0.0, y: 0.0 },
    ]);

    let interior = LineString::from(vec![
        Coord { x: 1.0, y: 1.0 },
        Coord { x: 2.0, y: 1.0 },
        Coord { x: 2.0, y: 2.0 },
        Coord { x: 1.0, y: 2.0 },
        Coord { x: 1.0, y: 1.0 },
    ]);

    let polygon = Polygon::new(exterior, vec![interior]);

    println!("\nCreated polygon with hole:");
    println!("  Exterior points: {}", polygon.exterior().coords_count());
    println!("  Interior rings: {}", polygon.interiors().len());
    println!("  Area: {:.2}", polygon.unsigned_area());

    // Step 2: Writing to GeoJSON
    println!("\n\nStep 2: Writing Vector Data to GeoJSON");
    println!("---------------------------------------");

    let mut features = Vec::new();

    // Add point features
    let point_geom = Geometry::new_point(vec![point1.x(), point1.y()]);
    let mut point_feature = Feature {
        bbox: None,
        geometry: Some(point_geom),
        id: None,
        properties: None,
        foreign_members: None,
    };
    point_feature.set_property("name", "Point A");
    point_feature.set_property("type", "marker");
    features.push(point_feature);

    // Add line feature
    let line_coords: Vec<Vec<f64>> = line
        .coords()
        .map(|c| vec![c.x, c.y])
        .collect();
    let line_geom = Geometry::new_line_string(line_coords);
    let mut line_feature = Feature {
        bbox: None,
        geometry: Some(line_geom),
        id: None,
        properties: None,
        foreign_members: None,
    };
    line_feature.set_property("name", "Route 1");
    line_feature.set_property("length", line.coords_count());
    features.push(line_feature);

    // Add polygon feature
    let exterior_coords: Vec<Vec<f64>> = polygon
        .exterior()
        .coords()
        .map(|c| vec![c.x, c.y])
        .collect();
    let interior_coords: Vec<Vec<Vec<f64>>> = polygon
        .interiors()
        .iter()
        .map(|ring| ring.coords().map(|c| vec![c.x, c.y]).collect())
        .collect();

    let mut poly_coords = vec![exterior_coords];
    poly_coords.extend(interior_coords);

    let poly_geom = Geometry::new_polygon(poly_coords);
    let mut poly_feature = Feature {
        bbox: None,
        geometry: Some(poly_geom),
        id: None,
        properties: None,
        foreign_members: None,
    };
    poly_feature.set_property("name", "District A");
    poly_feature.set_property("area", polygon.unsigned_area());
    features.push(poly_feature);

    let feature_collection = FeatureCollection {
        bbox: None,
        features,
        foreign_members: None,
    };

    let geojson = GeoJson::FeatureCollection(feature_collection);
    let geojson_path = temp_dir.join("vector_example.geojson");

    let mut file = File::create(&geojson_path)?;
    file.write_all(geojson.to_string().as_bytes())?;

    println!("Wrote GeoJSON file: {:?}", geojson_path);
    println!("  Features: 3 (1 point, 1 line, 1 polygon)");

    // Step 3: Buffer Operations
    println!("\n\nStep 3: Buffer Operations");
    println!("-------------------------");

    use geo::algorithm::buffer::BufferBuilder;

    // Buffer a point
    println!("Buffering point by 0.5 units...");
    let point_buffer = BufferBuilder::new().buffer(&point1, 0.5);

    if let Some(buffered_geom) = point_buffer {
        match buffered_geom {
            geo::Geometry::Polygon(poly) => {
                println!("  Result: Polygon with {} exterior points",
                         poly.exterior().coords_count());
                println!("  Area: {:.4}", poly.unsigned_area());
            }
            _ => println!("  Result: Other geometry type"),
        }
    }

    // Buffer a line
    println!("\nBuffering line by 0.2 units...");
    let line_buffer = BufferBuilder::new().buffer(&line, 0.2);

    if let Some(buffered_geom) = line_buffer {
        match buffered_geom {
            geo::Geometry::Polygon(poly) => {
                println!("  Result: Polygon with {} exterior points",
                         poly.exterior().coords_count());
                println!("  Area: {:.4}", poly.unsigned_area());
            }
            geo::Geometry::MultiPolygon(mpoly) => {
                println!("  Result: MultiPolygon with {} polygons", mpoly.0.len());
            }
            _ => println!("  Result: Other geometry type"),
        }
    }

    // Step 4: Spatial Relationships
    println!("\n\nStep 4: Spatial Relationships");
    println!("------------------------------");

    // Test point-in-polygon
    let test_point = Point::new(0.5, 0.5);
    println!("Testing if point ({}, {}) is in polygon...",
             test_point.x(), test_point.y());
    println!("  Contains: {}", polygon.contains(&test_point));

    let outside_point = Point::new(5.0, 5.0);
    println!("\nTesting if point ({}, {}) is in polygon...",
             outside_point.x(), outside_point.y());
    println!("  Contains: {}", polygon.contains(&outside_point));

    // Test line-polygon intersection
    println!("\nTesting if line intersects polygon...");
    println!("  Intersects: {}", polygon.intersects(&line));

    // Create another polygon for intersection tests
    let poly2_exterior = LineString::from(vec![
        Coord { x: 2.0, y: 0.0 },
        Coord { x: 6.0, y: 0.0 },
        Coord { x: 6.0, y: 3.0 },
        Coord { x: 2.0, y: 3.0 },
        Coord { x: 2.0, y: 0.0 },
    ]);
    let polygon2 = Polygon::new(poly2_exterior, vec![]);

    println!("\nTesting if two polygons intersect...");
    println!("  Polygon 1 bounds: x=[0, 4], y=[0, 3]");
    println!("  Polygon 2 bounds: x=[2, 6], y=[0, 3]");
    println!("  Intersects: {}", polygon.intersects(&polygon2));

    // Step 5: Geometric Operations
    println!("\n\nStep 5: Geometric Operations");
    println!("----------------------------");

    use geo::algorithm::bounding_rect::BoundingRect;
    use geo::algorithm::convex_hull::ConvexHull;

    // Convex hull
    println!("Computing convex hull of multipoint...");
    let points = geo::MultiPoint::from(vec![point1, point2, point3]);
    let hull = points.convex_hull();

    println!("  Input: {} points", points.0.len());
    match hull {
        geo::Geometry::Polygon(poly) => {
            println!("  Hull: Polygon with {} vertices", poly.exterior().coords_count());
            println!("  Hull area: {:.4}", poly.unsigned_area());
        }
        _ => println!("  Hull: Other geometry type"),
    }

    // Bounding rectangle
    println!("\nComputing bounding rectangle of polygon...");
    if let Some(bbox) = polygon.bounding_rect() {
        println!("  Min: ({:.2}, {:.2})", bbox.min().x, bbox.min().y);
        println!("  Max: ({:.2}, {:.2})", bbox.max().x, bbox.max().y);
        println!("  Width: {:.2}", bbox.width());
        println!("  Height: {:.2}", bbox.height());
    }

    // Step 6: Distance Calculations
    println!("\n\nStep 6: Distance Calculations");
    println!("------------------------------");

    use geo::algorithm::euclidean_distance::EuclideanDistance;

    let dist = point1.euclidean_distance(&point2);
    println!("Distance from P1 to P2: {:.4}", dist);

    let dist = point1.euclidean_distance(&point3);
    println!("Distance from P1 to P3: {:.4}", dist);

    // Distance from point to line
    use geo::algorithm::closest_point::ClosestPoint;

    println!("\nFinding closest point on line to external point...");
    let test_point = Point::new(1.0, 0.0);
    match line.closest_point(&test_point) {
        geo::Closest::Intersection(p) => {
            println!("  Point is on line: ({:.2}, {:.2})", p.x(), p.y());
        }
        geo::Closest::SinglePoint(p) => {
            println!("  Closest point: ({:.2}, {:.2})", p.x(), p.y());
            let dist = test_point.euclidean_distance(&p);
            println!("  Distance: {:.4}", dist);
        }
        geo::Closest::Indeterminate => {
            println!("  Distance indeterminate");
        }
    }

    // Step 7: Vector Layer Operations
    println!("\n\nStep 7: Vector Layer Operations");
    println!("--------------------------------");

    // Create a layer with multiple features
    let mut layer = VectorLayer::new("example_layer");

    // Add features
    layer.add_geometry(VectorGeometry::Point(point1))?;
    layer.add_geometry(VectorGeometry::LineString(line.clone()))?;
    layer.add_geometry(VectorGeometry::Polygon(polygon.clone()))?;

    println!("Created layer: {}", layer.name());
    println!("  Feature count: {}", layer.feature_count());

    // Calculate layer extent
    let extent = layer.extent()?;
    println!("  Layer extent:");
    println!("    Min X: {:.2}", extent.min_x());
    println!("    Min Y: {:.2}", extent.min_y());
    println!("    Max X: {:.2}", extent.max_x());
    println!("    Max Y: {:.2}", extent.max_y());

    // Spatial filtering
    println!("\nSpatial filter: geometries intersecting [1, 1, 3, 2]");
    let filter_bbox = BoundingBox::new(1.0, 1.0, 3.0, 2.0)?;
    let filtered = layer.filter_by_bbox(&filter_bbox)?;
    println!("  Filtered features: {}", filtered.len());

    // Summary
    println!("\n\n=== Tutorial Complete! ===");
    println!("\nOperations Covered:");
    println!("  1. Creating vector geometries (points, lines, polygons)");
    println!("  2. Writing to GeoJSON format");
    println!("  3. Buffer operations");
    println!("  4. Spatial relationship tests (contains, intersects)");
    println!("  5. Geometric computations (convex hull, bounding box)");
    println!("  6. Distance calculations");
    println!("  7. Vector layer operations and filtering");

    println!("\nKey Points:");
    println!("  - Vector geometries use the geo-types crate");
    println!("  - GeoJSON is a common interchange format");
    println!("  - Buffer operations create polygons around geometries");
    println!("  - Spatial predicates test geometric relationships");
    println!("  - Layer operations enable bulk processing");

    println!("\nOutput files:");
    println!("  - {:?}", geojson_path);

    println!("\nNext Tutorial:");
    println!("  - Try tutorial 04 for cloud data access");

    Ok(())
}
