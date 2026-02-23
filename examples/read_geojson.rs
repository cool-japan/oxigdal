//! Example: Reading GeoJSON files
//!
//! This example demonstrates how to:
//! - Read GeoJSON FeatureCollections
//! - Access geometry and properties
//! - Iterate over features
//! - Validate GeoJSON structure
//!
//! Run with:
//! ```bash
//! cargo run --example read_geojson
//! ```

use oxigdal_geojson::{GeoJsonReader, Validator};
use std::env;
use std::fs::File;
use std::io::BufReader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get file path from arguments or create test file
    let args: Vec<String> = env::args().collect();
    let file_path = if args.len() > 1 {
        args[1].clone()
    } else {
        // Create a test GeoJSON file
        let temp_dir = env::temp_dir();
        let test_file = temp_dir.join("test.geojson");
        create_test_geojson(&test_file)?;
        println!("Created test file: {:?}", test_file);
        println!("You can also run: cargo run --example read_geojson <path_to_geojson>");
        println!();
        test_file.to_string_lossy().to_string()
    };

    println!("=== Reading GeoJSON ===");
    println!("File: {}", file_path);
    println!();

    // Open and read the GeoJSON file
    let file = File::open(&file_path)?;
    let reader = BufReader::new(file);
    let mut geojson_reader = GeoJsonReader::new(reader);

    let collection = geojson_reader.read_feature_collection()?;

    println!("--- FeatureCollection Metadata ---");
    println!("Features: {}", collection.features.len());

    if let Some(bbox) = &collection.bbox {
        println!("Bounding box: {:?}", bbox);
    }

    if !collection.foreign_members.is_empty() {
        println!("Foreign members: {} entries", collection.foreign_members.len());
    }
    println!();

    // Validate the collection
    println!("--- Validation ---");
    let validator = Validator::new();
    let validation = validator.validate_collection(&collection);

    if validation.is_valid() {
        println!("✓ Valid GeoJSON");
    } else {
        println!("✗ Validation failed");
        for error in validation.errors() {
            println!("  Error: {}", error);
        }
        for warning in validation.warnings() {
            println!("  Warning: {}", warning);
        }
    }
    println!();

    // Print each feature
    println!("--- Features ---");
    for (i, feature) in collection.features.iter().enumerate() {
        println!("Feature {}", i);

        if let Some(id) = &feature.id {
            println!("  ID: {}", id);
        }

        if let Some(geometry) = &feature.geometry {
            println!("  Geometry type: {:?}", geometry.geometry_type);
            println!("  Coordinate count: {}", count_coordinates(&geometry.coordinates));

            if let Some(bbox) = &geometry.bbox {
                println!("  Bbox: {:?}", bbox);
            }
        } else {
            println!("  No geometry");
        }

        if !feature.properties.is_empty() {
            println!("  Properties:");
            for (key, value) in &feature.properties {
                println!("    {}: {:?}", key, value);
            }
        }

        println!();
    }

    // Statistics
    println!("--- Statistics ---");
    let mut geometry_types = std::collections::HashMap::new();
    for feature in &collection.features {
        if let Some(geom) = &feature.geometry {
            *geometry_types.entry(geom.geometry_type).or_insert(0) += 1;
        }
    }

    println!("Geometry type distribution:");
    for (geom_type, count) in geometry_types {
        println!("  {:?}: {}", geom_type, count);
    }

    println!();
    println!("=== Done ===");

    Ok(())
}

/// Count total coordinates in a geometry
fn count_coordinates(coords: &[Vec<f64>]) -> usize {
    coords.iter().map(|c| if c.len() >= 2 { 1 } else { 0 }).sum()
}

/// Create a test GeoJSON file
fn create_test_geojson(path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    use oxigdal_geojson::{
        Feature, FeatureCollection, GeoJsonWriter, Geometry, GeometryType, Properties,
    };
    use std::fs::File;
    use std::io::BufWriter;

    let mut collection = FeatureCollection {
        features: vec![],
        bbox: None,
        foreign_members: Default::default(),
    };

    // Create Point feature
    let mut props1 = Properties::new();
    props1.insert("name".to_string(), "San Francisco".into());
    props1.insert("type".to_string(), "city".into());
    props1.insert("population".to_string(), 874961.into());

    collection.features.push(Feature {
        geometry: Some(Geometry {
            geometry_type: GeometryType::Point,
            coordinates: vec![vec![-122.4194, 37.7749]],
            bbox: None,
        }),
        properties: props1,
        id: Some("sf".to_string()),
        bbox: None,
        foreign_members: Default::default(),
    });

    // Create LineString feature
    let mut props2 = Properties::new();
    props2.insert("name".to_string(), "Route".into());
    props2.insert("type".to_string(), "road".into());

    collection.features.push(Feature {
        geometry: Some(Geometry {
            geometry_type: GeometryType::LineString,
            coordinates: vec![
                vec![-122.4, 37.7],
                vec![-122.3, 37.8],
                vec![-122.2, 37.9],
            ],
            bbox: None,
        }),
        properties: props2,
        id: Some("route1".to_string()),
        bbox: None,
        foreign_members: Default::default(),
    });

    // Create Polygon feature
    let mut props3 = Properties::new();
    props3.insert("name".to_string(), "Park".into());
    props3.insert("type".to_string(), "park".into());
    props3.insert("area".to_string(), 12500.0.into());

    collection.features.push(Feature {
        geometry: Some(Geometry {
            geometry_type: GeometryType::Polygon,
            coordinates: vec![vec![
                vec![-122.5, 37.7],
                vec![-122.4, 37.7],
                vec![-122.4, 37.8],
                vec![-122.5, 37.8],
                vec![-122.5, 37.7],
            ]],
            bbox: None,
        }),
        properties: props3,
        id: Some("park1".to_string()),
        bbox: None,
        foreign_members: Default::default(),
    });

    // Write to file
    let file = File::create(path)?;
    let writer = BufWriter::new(file);
    let mut geojson_writer = GeoJsonWriter::new(writer);
    geojson_writer.set_pretty(true);
    geojson_writer.write_feature_collection(&collection)?;

    Ok(())
}
