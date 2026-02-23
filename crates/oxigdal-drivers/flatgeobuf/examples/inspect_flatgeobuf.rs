//! Inspect FlatGeobuf files to verify R-tree index and structure
//!
//! Usage:
//!     cargo run --example inspect_flatgeobuf demo/cog-viewer/golden-triangle.fgb

use oxigdal_flatgeobuf::reader::FlatGeobufReader;
use std::env;
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <file.fgb>", args[0]);
        return Ok(());
    }

    let path = &args[1];
    println!("Inspecting FlatGeobuf file: {}\n", path);

    let file = File::open(path)?;
    let mut reader = FlatGeobufReader::new(file)?;

    let has_index = reader.header().has_index;
    let features_count = reader.header().features_count;

    println!("=== Header Information ===");
    println!("Geometry Type: {:?}", reader.header().geometry_type);
    println!("Has Z: {}", reader.header().has_z);
    println!("Has M: {}", reader.header().has_m);
    println!("Has Index: {} (R-tree)", has_index);
    println!("Features Count: {:?}", features_count);

    if let Some(extent) = reader.header().extent {
        println!(
            "Extent: [{:.6}, {:.6}, {:.6}, {:.6}]",
            extent[0], extent[1], extent[2], extent[3]
        );
    }

    if let Some(ref crs) = reader.header().crs {
        println!("\n=== CRS Information ===");
        if let Some(ref org) = crs.organization {
            println!("Organization: {}", org);
        }
        if let Some(code) = crs.organization_code {
            println!("EPSG Code: {}", code);
        }
    }

    println!("\n=== Columns ===");
    for (i, col) in reader.header().columns.iter().enumerate() {
        println!("  {}: {} ({})", i + 1, col.name, col.column_type.name());
    }

    println!("\n=== Features ===");
    let mut count = 0;
    while let Some(feature) = reader.read_feature()? {
        count += 1;
        if count <= 3 {
            println!("\nFeature {}:", count);
            if let Some(ref geom) = feature.geometry {
                println!("  Geometry: {:?}", geometry_type_name(geom));
            }
            println!("  Properties:");
            for (key, value) in &feature.properties {
                println!("    {}: {:?}", key, value);
            }
        }
    }

    println!("\nTotal features read: {}", count);

    println!("\n✅ File is valid FlatGeobuf format!");
    if has_index {
        println!("✅ R-tree spatial index is present!");
        println!("   This enables efficient HTTP Range Requests");
    }

    Ok(())
}

fn geometry_type_name(geom: &oxigdal_core::vector::Geometry) -> &'static str {
    match geom {
        oxigdal_core::vector::Geometry::Point(_) => "Point",
        oxigdal_core::vector::Geometry::LineString(_) => "LineString",
        oxigdal_core::vector::Geometry::Polygon(_) => "Polygon",
        oxigdal_core::vector::Geometry::MultiPoint(_) => "MultiPoint",
        oxigdal_core::vector::Geometry::MultiLineString(_) => "MultiLineString",
        oxigdal_core::vector::Geometry::MultiPolygon(_) => "MultiPolygon",
        oxigdal_core::vector::Geometry::GeometryCollection(_) => "GeometryCollection",
    }
}
