//! Create test Shapefile files for various geographic regions
//!
//! This example demonstrates creating Shapefile files with different feature types
//! for the COG viewer demo regions.
//!
//! Usage:
//!     cargo run --package oxigdal-shapefile --example create_test_shapefile_samples
//!
//! Output:
//!     demo/cog-viewer/golden-triangle.shp (+ .shx, .dbf)
//!     demo/cog-viewer/iron-belt.shp (+ .shx, .dbf)

use oxigdal_core::vector::{Coordinate, Geometry, LineString, Polygon};
use oxigdal_shapefile::ShapefileFeature;
use oxigdal_shapefile::shp::shapes::ShapeType;
use oxigdal_shapefile::writer::{ShapefileSchemaBuilder, ShapefileWriter};
use std::collections::HashMap;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating test Shapefile samples...\n");

    create_golden_triangle()?;
    create_iron_belt()?;

    println!("\n✅ All Shapefile samples created successfully!");
    println!("Files ready for use in COG viewer at http://localhost:8080/");

    Ok(())
}

fn create_golden_triangle() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating Golden Triangle Shapefile...");

    let center_lon = 100.08466749884738;
    let center_lat = 20.35223590060906;

    // Create output path
    let output_path = PathBuf::from("demo/cog-viewer/golden-triangle");

    // Create schema for attributes
    let schema = ShapefileSchemaBuilder::new()
        .add_character_field("NAME", 50)?
        .add_character_field("TYPE", 30)?
        .add_numeric_field("AREA_KM2", 10, 2)?
        .build();

    // Create writer for Polygon shapefile
    let mut writer = ShapefileWriter::new(&output_path, ShapeType::Polygon, schema)?;

    // Create a polygon representing the Golden Triangle region
    // This is a simplified triangle-shaped polygon around the center
    let offset = 0.5; // degrees offset for the triangle
    let exterior_coords = vec![
        Coordinate::new_2d(center_lon, center_lat + offset), // North
        Coordinate::new_2d(center_lon - offset, center_lat - offset / 2.0), // Southwest
        Coordinate::new_2d(center_lon + offset, center_lat - offset / 2.0), // Southeast
        Coordinate::new_2d(center_lon, center_lat + offset), // Close the ring
    ];

    let exterior = LineString::new(exterior_coords)?;
    let polygon_geom = Polygon::new(exterior, vec![])?;

    // Create feature with attributes
    let mut attributes = HashMap::new();
    attributes.insert(
        "NAME".to_string(),
        oxigdal_core::vector::PropertyValue::String("Golden Triangle".to_string()),
    );
    attributes.insert(
        "TYPE".to_string(),
        oxigdal_core::vector::PropertyValue::String("Geographic Region".to_string()),
    );
    attributes.insert(
        "AREA_KM2".to_string(),
        oxigdal_core::vector::PropertyValue::Float(12500.0),
    );

    let feature = ShapefileFeature::new(1, Some(Geometry::Polygon(polygon_geom)), attributes);

    // Write feature
    writer.write_features(&[feature])?;

    println!("  ✅ Created Golden Triangle Shapefile");
    println!("     - {}.shp", output_path.display());
    println!("     - {}.shx", output_path.display());
    println!("     - {}.dbf", output_path.display());
    println!("  📍 Center: {:.6}, {:.6}", center_lat, center_lon);

    Ok(())
}

fn create_iron_belt() -> Result<(), Box<dyn std::error::Error>> {
    println!("\nCreating Basque Country Shapefile...");

    let center_lon = -2.9253;
    let center_lat = 43.2630;

    // Create output path
    let output_path = PathBuf::from("demo/cog-viewer/iron-belt");

    // Create schema for mining zones
    let schema = ShapefileSchemaBuilder::new()
        .add_character_field("NAME", 50)?
        .add_character_field("TYPE", 30)?
        .add_character_field("MINE_TYPE", 20)?
        .add_numeric_field("YIELD_MT", 10, 2)?
        .build();

    // Create writer for Polygon shapefile
    let mut writer = ShapefileWriter::new(&output_path, ShapeType::Polygon, schema)?;

    // Create multiple mining zone polygons
    let zones = [
        ("Bilbao Zone", -2.93, 43.26, 950.0),
        ("Vitoria Zone", -2.68, 42.85, 720.0),
        ("Pamplona Zone", -1.64, 42.82, 580.0),
    ];

    let mut features = Vec::new();

    for (idx, (name, lon, lat, yield_mt)) in zones.iter().enumerate() {
        // Create a rectangular polygon for each mining zone
        let size = 0.3; // degrees
        let exterior_coords = vec![
            Coordinate::new_2d(*lon - size / 2.0, *lat + size / 2.0),
            Coordinate::new_2d(*lon + size / 2.0, *lat + size / 2.0),
            Coordinate::new_2d(*lon + size / 2.0, *lat - size / 2.0),
            Coordinate::new_2d(*lon - size / 2.0, *lat - size / 2.0),
            Coordinate::new_2d(*lon - size / 2.0, *lat + size / 2.0), // Close the ring
        ];

        let exterior = LineString::new(exterior_coords)?;
        let polygon_geom = Polygon::new(exterior, vec![])?;

        // Create attributes
        let mut attributes = HashMap::new();
        attributes.insert(
            "NAME".to_string(),
            oxigdal_core::vector::PropertyValue::String(name.to_string()),
        );
        attributes.insert(
            "TYPE".to_string(),
            oxigdal_core::vector::PropertyValue::String("Mining Zone".to_string()),
        );
        attributes.insert(
            "MINE_TYPE".to_string(),
            oxigdal_core::vector::PropertyValue::String("Iron".to_string()),
        );
        attributes.insert(
            "YIELD_MT".to_string(),
            oxigdal_core::vector::PropertyValue::Float(*yield_mt),
        );

        let feature = ShapefileFeature::new(
            (idx + 1) as i32,
            Some(Geometry::Polygon(polygon_geom)),
            attributes,
        );

        features.push(feature);
    }

    // Write all features
    writer.write_features(&features)?;

    println!(
        "  ✅ Created Basque Country Shapefile with {} mining zones",
        zones.len()
    );
    println!("     - {}.shp", output_path.display());
    println!("     - {}.shx", output_path.display());
    println!("     - {}.dbf", output_path.display());
    println!("  📍 Center: {:.6}, {:.6}", center_lat, center_lon);

    Ok(())
}
