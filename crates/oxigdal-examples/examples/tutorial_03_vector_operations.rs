//! Tutorial 03: Vector Operations
//!
//! This tutorial demonstrates vector/geometry operations:
//! - Creating and reading vector data (GeoJSON)
//! - Buffer operations
//! - Intersection, containment
//! - Spatial queries and filtering
//! - Vector-raster conversions
//!
//! Run with:
//! ```bash
//! cargo run --example tutorial_03_vector_operations
//! ```

use geo::geometry::{LineString, MultiPolygon, Point, Polygon};
use geo::{Area, BoundingRect, Buffer, Contains, CoordsIter, Distance, Euclidean, Intersects};
use geo_types::Coord;
use oxigdal_core::types::BoundingBox;
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

    let mut point_props = oxigdal_geojson::Properties::new();
    point_props.insert("name".to_string(), "Point A".into());
    point_props.insert("type".to_string(), "marker".into());
    let point_geom = oxigdal_geojson::types::Point::new_2d(point1.x(), point1.y())?;
    features.push(oxigdal_geojson::Feature::new(
        Some(oxigdal_geojson::Geometry::Point(point_geom)),
        Some(point_props),
    ));

    let line_coords: Vec<Vec<f64>> = line.coords().map(|c| vec![c.x, c.y]).collect();
    let mut line_props = oxigdal_geojson::Properties::new();
    line_props.insert("name".to_string(), "Route 1".into());
    line_props.insert("length".to_string(), (line.coords_count() as i64).into());
    let line_geom = oxigdal_geojson::types::LineString::new(line_coords)?;
    features.push(oxigdal_geojson::Feature::new(
        Some(oxigdal_geojson::Geometry::LineString(line_geom)),
        Some(line_props),
    ));

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

    let mut poly_props = oxigdal_geojson::Properties::new();
    poly_props.insert("name".to_string(), "District A".into());
    poly_props.insert("area".to_string(), polygon.unsigned_area().into());
    let poly_geom = oxigdal_geojson::types::Polygon::new(poly_coords)?;
    features.push(oxigdal_geojson::Feature::new(
        Some(oxigdal_geojson::Geometry::Polygon(poly_geom)),
        Some(poly_props),
    ));

    let feature_collection = oxigdal_geojson::FeatureCollection::new(features);
    let geojson_path = temp_dir.join("vector_example.geojson");

    let mut file = File::create(&geojson_path)?;
    let json = serde_json::to_string_pretty(&feature_collection)?;
    file.write_all(json.as_bytes())?;

    println!("Wrote GeoJSON file: {:?}", geojson_path);
    println!("  Features: 3 (1 point, 1 line, 1 polygon)");

    // Step 3: Buffer Operations
    println!("\n\nStep 3: Buffer Operations");
    println!("-------------------------");

    // Buffer a point
    println!("Buffering point by 0.5 units...");
    let point_buffer: MultiPolygon = point1.buffer(0.5);
    if let Some(first) = point_buffer.0.first() {
        println!(
            "  Result: Polygon with {} exterior points",
            first.exterior().coords_count()
        );
        println!("  Area: {:.4}", first.unsigned_area());
    }

    // Buffer a line
    println!("\nBuffering line by 0.2 units...");
    let line_buffer: MultiPolygon = line.buffer(0.2);
    println!(
        "  Result: MultiPolygon with {} polygon(s)",
        line_buffer.0.len()
    );
    if let Some(first) = line_buffer.0.first() {
        println!(
            "  First polygon exterior points: {}",
            first.exterior().coords_count()
        );
        println!("  Area: {:.4}", first.unsigned_area());
    }

    // Step 4: Spatial Relationships
    println!("\n\nStep 4: Spatial Relationships");
    println!("------------------------------");

    // Test point-in-polygon
    let test_point = Point::new(0.5, 0.5);
    println!(
        "Testing if point ({}, {}) is in polygon...",
        test_point.x(),
        test_point.y()
    );
    println!("  Contains: {}", polygon.contains(&test_point));

    let outside_point = Point::new(5.0, 5.0);
    println!(
        "\nTesting if point ({}, {}) is in polygon...",
        outside_point.x(),
        outside_point.y()
    );
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

    use geo::algorithm::convex_hull::ConvexHull;

    // Convex hull
    println!("Computing convex hull of multipoint...");
    let points = geo::MultiPoint::from(vec![point1, point2, point3]);
    let hull = points.convex_hull();

    println!("  Input: {} points", points.0.len());
    println!(
        "  Hull: Polygon with {} vertices",
        hull.exterior().coords_count()
    );
    println!("  Hull area: {:.4}", hull.unsigned_area());

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

    let dist = Euclidean.distance(point1, point2);
    println!("Distance from P1 to P2: {:.4}", dist);

    let dist = Euclidean.distance(point1, point3);
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
            let dist = Euclidean.distance(test_point, p);
            println!("  Distance: {:.4}", dist);
        }
        geo::Closest::Indeterminate => {
            println!("  Distance indeterminate");
        }
    }

    // Step 7: Vector Layer Extent and Filtering
    println!("\n\nStep 7: Vector Layer Extent and Filtering");
    println!("--------------------------------------------");

    // Compute the extent of every geometry we created
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    let point_rect: Option<geo::Rect> = point1.bounding_rect().into();
    let line_rect: Option<geo::Rect> = line.bounding_rect();
    let polygon_rect: Option<geo::Rect> = polygon.bounding_rect();

    for geom_bounds in [point_rect, line_rect, polygon_rect]
        .into_iter()
        .flatten()
        .map(|r| (r.min().x, r.min().y, r.max().x, r.max().y))
    {
        min_x = min_x.min(geom_bounds.0);
        min_y = min_y.min(geom_bounds.1);
        max_x = max_x.max(geom_bounds.2);
        max_y = max_y.max(geom_bounds.3);
    }

    println!("Created layer: example_layer");
    println!("  Feature count: 3");
    println!("  Layer extent:");
    println!("    Min X: {:.2}", min_x);
    println!("    Min Y: {:.2}", min_y);
    println!("    Max X: {:.2}", max_x);
    println!("    Max Y: {:.2}", max_y);

    // Spatial filtering
    println!("\nSpatial filter: geometries intersecting [1, 1, 3, 2]");
    let filter_bbox = BoundingBox::new(1.0, 1.0, 3.0, 2.0)?;
    let filter_rect = geo::Rect::new(
        Coord {
            x: filter_bbox.min_x,
            y: filter_bbox.min_y,
        },
        Coord {
            x: filter_bbox.max_x,
            y: filter_bbox.max_y,
        },
    );
    let mut filtered = 0;
    if line.intersects(&filter_rect) {
        filtered += 1;
    }
    if polygon.intersects(&filter_rect) {
        filtered += 1;
    }
    println!("  Filtered features: {}", filtered);

    // Summary
    println!("\n\n=== Tutorial Complete! ===");
    println!("\nOperations Covered:");
    println!("  1. Creating vector geometries (points, lines, polygons)");
    println!("  2. Writing to GeoJSON format");
    println!("  3. Buffer operations");
    println!("  4. Spatial relationship tests (contains, intersects)");
    println!("  5. Geometric computations (convex hull, bounding box)");
    println!("  6. Distance calculations");
    println!("  7. Extent computation and spatial filtering");

    println!("\nKey Points:");
    println!("  - Vector geometries use the geo-types crate");
    println!("  - GeoJSON is a common interchange format");
    println!("  - Buffer operations create polygons around geometries");
    println!("  - Spatial predicates test geometric relationships");

    println!("\nOutput files:");
    println!("  - {:?}", geojson_path);

    println!("\nNext Tutorial:");
    println!("  - Try tutorial 04 for cloud data access");

    Ok(())
}
