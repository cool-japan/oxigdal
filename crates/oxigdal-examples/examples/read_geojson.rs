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
    println!();

    // Validate the collection
    println!("--- Validation ---");
    let mut validator = Validator::new();
    match validator.validate_feature_collection(&collection) {
        Ok(()) => println!("Valid GeoJSON"),
        Err(e) => println!("Validation failed: {}", e),
    }
    println!();

    // Print each feature
    println!("--- Features ---");
    for (i, feature) in collection.features.iter().enumerate() {
        println!("Feature {}", i);

        if let Some(id) = &feature.id {
            println!("  ID: {}", id.as_string());
        }

        if let Some(geometry) = &feature.geometry {
            println!("  Geometry type: {}", geometry_type_name(geometry));
            println!("  Coordinate count: {}", count_coordinates(geometry));
        } else {
            println!("  No geometry");
        }

        if let Some(properties) = &feature.properties
            && !properties.is_empty()
        {
            println!("  Properties:");
            for (key, value) in properties {
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
            *geometry_types.entry(geometry_type_name(geom)).or_insert(0) += 1;
        }
    }

    println!("Geometry type distribution:");
    for (geom_type, count) in geometry_types {
        println!("  {}: {}", geom_type, count);
    }

    println!();
    println!("=== Done ===");

    Ok(())
}

/// Returns a human-readable name for a geometry's variant
fn geometry_type_name(geometry: &oxigdal_geojson::Geometry) -> &'static str {
    use oxigdal_geojson::Geometry;
    match geometry {
        Geometry::Point(_) => "Point",
        Geometry::LineString(_) => "LineString",
        Geometry::Polygon(_) => "Polygon",
        Geometry::MultiPoint(_) => "MultiPoint",
        Geometry::MultiLineString(_) => "MultiLineString",
        Geometry::MultiPolygon(_) => "MultiPolygon",
        Geometry::GeometryCollection(_) => "GeometryCollection",
    }
}

/// Count total coordinates in a geometry
fn count_coordinates(geometry: &oxigdal_geojson::Geometry) -> usize {
    use oxigdal_geojson::Geometry;
    match geometry {
        Geometry::Point(_) => 1,
        Geometry::LineString(ls) => ls.coordinates.len(),
        Geometry::Polygon(poly) => poly.coordinates.iter().map(std::vec::Vec::len).sum(),
        Geometry::MultiPoint(mp) => mp.coordinates.len(),
        Geometry::MultiLineString(mls) => mls.coordinates.iter().map(std::vec::Vec::len).sum(),
        Geometry::MultiPolygon(mpoly) => mpoly
            .coordinates
            .iter()
            .flat_map(|rings| rings.iter().map(std::vec::Vec::len))
            .sum(),
        Geometry::GeometryCollection(gc) => gc.geometries.iter().map(count_coordinates).sum(),
    }
}

/// Create a test GeoJSON file
fn create_test_geojson(path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    use oxigdal_geojson::types::Point;
    use oxigdal_geojson::{Feature, FeatureCollection, GeoJsonWriter, Geometry, Properties};
    use std::fs::File;
    use std::io::BufWriter;

    let mut features = Vec::new();

    // Create Point feature
    let mut props1 = Properties::new();
    props1.insert("name".to_string(), "San Francisco".into());
    props1.insert("type".to_string(), "city".into());
    props1.insert("population".to_string(), 874_961.into());

    let mut point_feature = Feature::new(
        Some(Geometry::Point(Point::new_2d(-122.4194, 37.7749)?)),
        Some(props1),
    );
    point_feature.id = Some("sf".into());
    features.push(point_feature);

    // Create LineString feature
    let mut props2 = Properties::new();
    props2.insert("name".to_string(), "Route".into());
    props2.insert("type".to_string(), "road".into());

    let route_coords = vec![vec![-122.4, 37.7], vec![-122.3, 37.8], vec![-122.2, 37.9]];

    let mut line_feature = Feature::new(
        Some(Geometry::LineString(
            oxigdal_geojson::types::LineString::new(route_coords)?,
        )),
        Some(props2),
    );
    line_feature.id = Some("route1".into());
    features.push(line_feature);

    // Create Polygon feature
    let mut props3 = Properties::new();
    props3.insert("name".to_string(), "Park".into());
    props3.insert("type".to_string(), "park".into());
    props3.insert("area".to_string(), 12500.0.into());

    let polygon_rings = vec![vec![
        vec![-122.5, 37.7],
        vec![-122.4, 37.7],
        vec![-122.4, 37.8],
        vec![-122.5, 37.8],
        vec![-122.5, 37.7],
    ]];

    let mut poly_feature = Feature::new(
        Some(Geometry::Polygon(oxigdal_geojson::types::Polygon::new(
            polygon_rings,
        )?)),
        Some(props3),
    );
    poly_feature.id = Some("park1".into());
    features.push(poly_feature);

    let collection = FeatureCollection::new(features);

    // Write to file
    let file = File::create(path)?;
    let writer = BufWriter::new(file);
    let mut geojson_writer = GeoJsonWriter::pretty(writer);
    geojson_writer.write_feature_collection(&collection)?;

    Ok(())
}
