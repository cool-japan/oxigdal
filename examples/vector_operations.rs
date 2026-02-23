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

use oxigdal_core::types::BoundingBox;
use oxigdal_core::vector::{Geometry, Point};
use oxigdal_geojson::{
    Feature, FeatureCollection, GeoJsonWriter, Geometry as GeoJsonGeometry, GeometryType,
    Properties,
};
use std::env;
use std::fs::File;
use std::io::BufWriter;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Vector Operations Example ===");
    println!();

    // Create geometries
    println!("--- Creating Geometries ---");

    // Points
    let point1 = Point::new(-122.4194, 37.7749); // San Francisco
    let point2 = Point::new(-118.2437, 34.0522); // Los Angeles

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
    let test_point = Point::new(-122.4, 37.8);
    println!(
        "Contains ({}, {}): {}",
        test_point.x(),
        test_point.y(),
        bbox.contains(test_point.x(), test_point.y())
    );

    // Expand bbox
    let expanded = bbox.expand(0.1, 0.1);
    println!("Expanded by 0.1: {:?}", expanded);

    // Intersection
    let bbox2 = BoundingBox::new(-122.6, 37.6, -122.4, 37.8)?;
    if let Some(intersection) = bbox.intersection(&bbox2) {
        println!("Intersection: {:?}", intersection);
    }
    println!();

    // Create GeoJSON features
    println!("--- Creating GeoJSON Features ---");

    let mut collection = FeatureCollection {
        features: vec![],
        bbox: None,
        foreign_members: Default::default(),
    };

    // Add point feature
    let mut props = Properties::new();
    props.insert("name".to_string(), "City Hall".into());
    props.insert("type".to_string(), "landmark".into());

    collection.features.push(Feature {
        geometry: Some(GeoJsonGeometry {
            geometry_type: GeometryType::Point,
            coordinates: vec![vec![-122.4194, 37.7749]],
            bbox: None,
        }),
        properties: props,
        id: Some("city_hall".to_string()),
        bbox: None,
        foreign_members: Default::default(),
    });

    // Add linestring feature (route)
    let mut route_props = Properties::new();
    route_props.insert("name".to_string(), "Market Street".into());
    route_props.insert("type".to_string(), "street".into());

    let route_coords = vec![
        vec![-122.4194, 37.7749],
        vec![-122.4083, 37.7855],
        vec![-122.3972, 37.7961],
    ];

    collection.features.push(Feature {
        geometry: Some(GeoJsonGeometry {
            geometry_type: GeometryType::LineString,
            coordinates: route_coords.clone(),
            bbox: None,
        }),
        properties: route_props,
        id: Some("market_st".to_string()),
        bbox: None,
        foreign_members: Default::default(),
    });

    // Add polygon feature (park)
    let mut park_props = Properties::new();
    park_props.insert("name".to_string(), "Golden Gate Park".into());
    park_props.insert("type".to_string(), "park".into());
    park_props.insert("area_hectares".to_string(), 412.0.into());

    collection.features.push(Feature {
        geometry: Some(GeoJsonGeometry {
            geometry_type: GeometryType::Polygon,
            coordinates: vec![vec![
                vec![-122.5100, 37.7694],
                vec![-122.4548, 37.7694],
                vec![-122.4548, 37.7756],
                vec![-122.5100, 37.7756],
                vec![-122.5100, 37.7694],
            ]],
            bbox: None,
        }),
        properties: park_props,
        id: Some("ggp".to_string()),
        bbox: None,
        foreign_members: Default::default(),
    });

    println!("Created {} features", collection.features.len());
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

    let file = File::create(&output_path)?;
    let writer = BufWriter::new(file);
    let mut geojson_writer = GeoJsonWriter::new(writer);
    geojson_writer.set_pretty(true);
    geojson_writer.write_feature_collection(&collection)?;

    println!("Wrote: {:?}", output_path);
    println!();

    println!("=== Done ===");

    Ok(())
}
