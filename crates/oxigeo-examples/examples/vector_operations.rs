//! Example: Vector Operations
//!
//! This example demonstrates how to:
//! - Work with vector geometries
//! - Perform spatial operations
//! - Transform coordinates
//! - Simplify geometries
//!
//! Run with:
//! ```bash
//! cargo run --example vector_operations
//! ```

use oxigeo_core::types::BoundingBox;
use oxigeo_core::vector::Point as CorePoint;
use oxigeo_geojson::types::Point as GeoJsonPoint;
use oxigeo_geojson::{Feature, FeatureCollection, GeoJsonWriter, Geometry, Properties};
use std::env;
use std::fs::File;
use std::io::BufWriter;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Vector Operations Example ===");
    println!();

    // Create geometries
    println!("--- Creating Geometries ---");

    // Points
    let point1 = CorePoint::new(-122.4194, 37.7749); // San Francisco
    let point2 = CorePoint::new(-118.2437, 34.0522); // Los Angeles

    println!("Point 1: ({}, {})", point1.x(), point1.y());
    println!("Point 2: ({}, {})", point2.x(), point2.y());

    // Calculate distance
    let dx = point2.x() - point1.x();
    let dy = point2.y() - point1.y();
    let distance = (dx * dx + dy * dy).sqrt();
    println!("Euclidean distance: {:.4}", distance);
    println!();

    // Create bounding box
    println!("--- Bounding Box Operations ---");
    let bbox = BoundingBox::new(-122.5, 37.7, -122.3, 37.9)?;

    println!("Bounding Box: {:?}", bbox);
    println!("Width: {}", bbox.width());
    println!("Height: {}", bbox.height());
    println!("Center: {:?}", bbox.center());
    println!("Area: {}", bbox.area());

    // Check containment
    let test_point = CorePoint::new(-122.4, 37.8);
    println!(
        "Contains ({}, {}): {}",
        test_point.x(),
        test_point.y(),
        bbox.contains_point(test_point.x(), test_point.y())
    );

    // Expand bbox
    let expanded = bbox.expand(0.1);
    println!("Expanded by 0.1: {:?}", expanded);

    // Intersection
    let bbox2 = BoundingBox::new(-122.6, 37.6, -122.4, 37.8)?;
    if let Some(intersection) = bbox.intersection(&bbox2) {
        println!("Intersection: {:?}", intersection);
    }
    println!();

    // Create GeoJSON features
    println!("--- Creating GeoJSON Features ---");

    let mut features = Vec::new();

    // Add point feature
    let mut props = Properties::new();
    props.insert("name".to_string(), "City Hall".into());
    props.insert("type".to_string(), "landmark".into());

    let mut city_hall = Feature::new(
        Some(Geometry::Point(GeoJsonPoint::new_2d(-122.4194, 37.7749)?)),
        Some(props),
    );
    city_hall.id = Some("city_hall".into());
    features.push(city_hall);

    // Add linestring feature (route)
    let mut route_props = Properties::new();
    route_props.insert("name".to_string(), "Market Street".into());
    route_props.insert("type".to_string(), "street".into());

    let route_coords = vec![
        vec![-122.4194, 37.7749],
        vec![-122.4083, 37.7855],
        vec![-122.3972, 37.7961],
    ];

    let mut market_st = Feature::new(
        Some(Geometry::LineString(
            oxigeo_geojson::types::LineString::new(route_coords.clone())?,
        )),
        Some(route_props),
    );
    market_st.id = Some("market_st".into());
    features.push(market_st);

    // Add polygon feature (park)
    let mut park_props = Properties::new();
    park_props.insert("name".to_string(), "Golden Gate Park".into());
    park_props.insert("type".to_string(), "park".into());
    park_props.insert("area_hectares".to_string(), 412.0.into());

    let park_rings = vec![vec![
        vec![-122.5100, 37.7694],
        vec![-122.4548, 37.7694],
        vec![-122.4548, 37.7756],
        vec![-122.5100, 37.7756],
        vec![-122.5100, 37.7694],
    ]];

    let mut ggp = Feature::new(
        Some(Geometry::Polygon(oxigeo_geojson::types::Polygon::new(
            park_rings,
        )?)),
        Some(park_props),
    );
    ggp.id = Some("ggp".into());
    features.push(ggp);

    println!("Created {} features", features.len());
    println!();

    // Calculate route length
    println!("--- Route Analysis ---");
    let mut route_length = 0.0;
    for i in 0..route_coords.len() - 1 {
        let p1 = &route_coords[i];
        let p2 = &route_coords[i + 1];

        let dx = p2[0] - p1[0];
        let dy = p2[1] - p1[1];
        route_length += (dx * dx + dy * dy).sqrt();
    }
    println!("Route length (degrees): {:.6}", route_length);
    println!("Route segments: {}", route_coords.len() - 1);
    println!();

    // Write to file
    println!("--- Writing GeoJSON ---");
    let temp_dir = env::temp_dir();
    let output_path = temp_dir.join("vector_output.geojson");

    let collection = FeatureCollection::new(features);
    let file = File::create(&output_path)?;
    let writer = BufWriter::new(file);
    let mut geojson_writer = GeoJsonWriter::pretty(writer);
    geojson_writer.write_feature_collection(&collection)?;

    println!("Wrote: {:?}", output_path);
    println!();

    println!("=== Done ===");

    Ok(())
}
