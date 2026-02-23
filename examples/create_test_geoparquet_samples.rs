//! Create test GeoParquet files for various geographic regions
//!
//! This example demonstrates creating GeoParquet files with spatial partitioning
//! for the COG viewer demo regions.
//!
//! Usage:
//!     cargo run --example create_test_geoparquet_samples
//!
//! Output:
//!     demo/cog-viewer/golden-triangle.parquet
//!     demo/cog-viewer/iron-belt.parquet

use oxigdal_core::types::BoundingBox;
use oxigdal_geoparquet::geometry::{Coordinate, Geometry, Point, Polygon, LineString};
use oxigdal_geoparquet::metadata::{Crs, GeometryColumnMetadata};
use oxigdal_geoparquet::spatial::PartitionStrategy;
use oxigdal_geoparquet::GeoParquetWriter;
use std::f64::consts::PI;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating test GeoParquet samples...\n");

    create_golden_triangle()?;
    create_iron_belt()?;

    println!("\n✅ All GeoParquet samples created successfully!");
    println!("Files ready for use in COG viewer at http://localhost:8080/");

    Ok(())
}

fn create_golden_triangle() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating Golden Triangle GeoParquet...");

    let center_lon = 100.08466749884738;
    let center_lat = 20.35223590060906;

    println!("  📍 Center: {:.6}, {:.6}", center_lat, center_lon);
    println!("  ℹ️  Generating sample geometries...");

    // Create metadata with WGS84 CRS
    let metadata = GeometryColumnMetadata::new_wkb()
        .with_crs(Crs::wgs84());

    // Create writer with spatial partitioning
    let output_path = "demo/cog-viewer/golden-triangle.parquet";
    let mut writer = GeoParquetWriter::new(output_path, "geometry", metadata)?
        .with_batch_size(100)
        .with_partitioning(PartitionStrategy::Hilbert);

    // Generate sample geometries in a grid pattern around the center
    let num_samples = 500;
    let extent = 0.5; // Degrees
    let mut geometries = Vec::with_capacity(num_samples);

    for i in 0..num_samples {
        let angle = (i as f64 / num_samples as f64) * 2.0 * PI;
        let radius = ((i % 100) as f64 / 100.0) * extent;

        let lon = center_lon + radius * angle.cos();
        let lat = center_lat + radius * angle.sin();

        geometries.push(Geometry::Point(Point::new_2d(lon, lat)));
    }

    println!("  ⚙️  Writing {} geometries with Hilbert curve partitioning...", num_samples);

    // Write all geometries
    for geom in &geometries {
        writer.add_geometry(geom)?;
    }

    // Finish and write metadata
    writer.finish()?;

    println!("  ✅ Created: {}", output_path);
    println!("  📊 Geometries: {}", num_samples);
    println!("  💾 Format: GeoParquet 1.0 with spatial partitioning");

    Ok(())
}

fn create_iron_belt() -> Result<(), Box<dyn std::error::Error>> {
    println!("\nCreating Iron Belt GeoParquet...");

    let center_lon = 28.6362;
    let center_lat = -12.9638;

    println!("  📍 Center: {:.6}, {:.6}", center_lat, center_lon);
    println!("  ℹ️  Generating sample geometries...");

    // Create metadata with WGS84 CRS
    let metadata = GeometryColumnMetadata::new_wkb()
        .with_crs(Crs::wgs84());

    // Create writer with grid-based spatial partitioning
    let output_path = "demo/cog-viewer/iron-belt.parquet";
    let mut writer = GeoParquetWriter::new(output_path, "geometry", metadata)?
        .with_batch_size(50)
        .with_partitioning(PartitionStrategy::Grid { cells_x: 10, cells_y: 10 });

    // Generate sample polygons representing mining areas
    let num_samples = 200;
    let extent = 1.0; // Degrees
    let mut geometries = Vec::with_capacity(num_samples);

    for i in 0..num_samples {
        let row = i / 20;
        let col = i % 20;

        let base_lon = center_lon - extent / 2.0 + (col as f64 / 20.0) * extent;
        let base_lat = center_lat - extent / 2.0 + (row as f64 / 10.0) * extent;

        // Create small polygons representing mining areas
        let size = 0.02; // Small polygon size
        let coords = vec![
            Coordinate::new_2d(base_lon, base_lat),
            Coordinate::new_2d(base_lon + size, base_lat),
            Coordinate::new_2d(base_lon + size, base_lat + size),
            Coordinate::new_2d(base_lon, base_lat + size),
            Coordinate::new_2d(base_lon, base_lat), // Close the ring
        ];

        let polygon = Polygon::new_simple(LineString::new(coords));
        geometries.push(Geometry::Polygon(polygon));
    }

    println!("  ⚙️  Writing {} geometries with grid partitioning...", num_samples);

    // Write all geometries
    for geom in &geometries {
        writer.add_geometry(geom)?;
    }

    // Finish and write metadata
    writer.finish()?;

    println!("  ✅ Created: {}", output_path);
    println!("  📊 Geometries: {}", num_samples);
    println!("  💾 Format: GeoParquet 1.0 with row group spatial partitioning");

    Ok(())
}
