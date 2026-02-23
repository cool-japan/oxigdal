//! Verify test Shapefile files created by create_test_shapefile_samples
//!
//! This example reads and validates the Shapefile files to ensure they were
//! created correctly.
//!
//! Usage:
//!     cargo run --package oxigdal-shapefile --example verify_shapefile_samples

use oxigdal_shapefile::ShapefileReader;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Verifying Shapefile samples...\n");

    verify_golden_triangle()?;
    verify_iron_belt()?;

    println!("\n✅ All Shapefile samples verified successfully!");

    Ok(())
}

fn verify_golden_triangle() -> Result<(), Box<dyn std::error::Error>> {
    println!("Verifying Golden Triangle Shapefile...");

    let path = PathBuf::from("demo/cog-viewer/golden-triangle");
    let reader = ShapefileReader::open(&path)?;
    let features = reader.read_features()?;

    println!("  - Feature count: {}", features.len());

    if features.is_empty() {
        return Err("Golden Triangle shapefile has no features".into());
    }

    let feature = &features[0];
    println!(
        "  - Geometry type: {:?}",
        feature.geometry.as_ref().map(|g| g.geometry_type())
    );
    println!("  - Attributes:");
    for (key, value) in &feature.attributes {
        println!("    - {}: {:?}", key, value);
    }

    // Verify attributes
    if let Some(name) = feature.attributes.get("NAME") {
        println!("  ✅ NAME attribute found: {:?}", name);
    } else {
        return Err("NAME attribute not found".into());
    }

    if let Some(area) = feature.attributes.get("AREA_KM2") {
        println!("  ✅ AREA_KM2 attribute found: {:?}", area);
    } else {
        return Err("AREA_KM2 attribute not found".into());
    }

    println!("  ✅ Golden Triangle verified");
    Ok(())
}

fn verify_iron_belt() -> Result<(), Box<dyn std::error::Error>> {
    println!("\nVerifying Basque Country Shapefile...");

    let path = PathBuf::from("demo/cog-viewer/iron-belt");
    let reader = ShapefileReader::open(&path)?;
    let features = reader.read_features()?;

    println!("  - Feature count: {}", features.len());

    if features.len() != 3 {
        return Err(format!("Expected 3 features, got {}", features.len()).into());
    }

    for (idx, feature) in features.iter().enumerate() {
        println!("  - Zone {}:", idx + 1);
        println!(
            "    - Geometry: {:?}",
            feature.geometry.as_ref().map(|g| g.geometry_type())
        );

        if let Some(name) = feature.attributes.get("NAME") {
            println!("    - NAME: {:?}", name);
        }

        if let Some(yield_mt) = feature.attributes.get("YIELD_MT") {
            println!("    - YIELD_MT: {:?}", yield_mt);
        }
    }

    println!("  ✅ Basque Country verified");
    Ok(())
}
