//! Tutorial 01: Quick Start with OxiGeo
//!
//! This tutorial covers the basics of OxiGeo:
//! - Reading raster and vector data
//! - Writing to various formats
//! - Basic metadata inspection
//! - Simple data transformations
//!
//! Run with:
//! ```bash
//! cargo run --example tutorial_01_quickstart
//! ```

use oxigeo_core::buffer::RasterBuffer;
use oxigeo_core::io::FileDataSource;
use oxigeo_core::types::{BoundingBox, GeoTransform, RasterDataType};
use oxigeo_geotiff::GeoTiffReader;
use oxigeo_geotiff::tiff::Compression;
use oxigeo_geotiff::writer::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};
use std::env;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== OxiGeo Quick Start Tutorial ===\n");

    // Part 1: Creating and Writing a Simple Raster
    println!("Part 1: Creating a Simple Raster");
    println!("-----------------------------------");

    let temp_dir = env::temp_dir();
    let output_path = temp_dir.join("quickstart_example.tif");

    // Create a 256x256 raster with some interesting data
    println!("Creating 256x256 raster buffer...");
    let mut buffer = RasterBuffer::zeros(256, 256, RasterDataType::Float32);

    // Fill with a distance-from-center pattern
    for y in 0..256 {
        for x in 0..256 {
            let dx = (x as f64) - 128.0;
            let dy = (y as f64) - 128.0;
            let distance = (dx * dx + dy * dy).sqrt();
            let value = (distance / 180.0 * 255.0).min(255.0);
            buffer.set_pixel(x, y, value)?;
        }
    }

    println!("Buffer created with distance-from-center pattern");

    // Compute and print statistics
    let stats = buffer.compute_statistics()?;
    println!("\nBuffer statistics:");
    println!("  Min: {:.2}", stats.min);
    println!("  Max: {:.2}", stats.max);
    println!("  Mean: {:.2}", stats.mean);
    println!("  Std Dev: {:.2}", stats.std_dev);
    println!("  Valid pixels: {}", stats.valid_count);

    // Create geospatial metadata (WGS84, covering a small area)
    let bbox = BoundingBox::new(-1.0, -1.0, 1.0, 1.0)?;
    let geo_transform = GeoTransform::from_bounds(&bbox, 256, 256)?;

    println!("\nGeospatial extent:");
    println!("  West: {}", bbox.min_x);
    println!("  South: {}", bbox.min_y);
    println!("  East: {}", bbox.max_x);
    println!("  North: {}", bbox.max_y);
    println!("  Pixel size: {:?}", geo_transform.resolution());

    // Write to GeoTIFF with tiling and compression
    println!("\nWriting to GeoTIFF: {:?}", output_path);
    let config = WriterConfig::new(256, 256, 1, RasterDataType::Float32)
        .with_compression(Compression::Lzw)
        .with_tile_size(256, 256)
        .with_geo_transform(geo_transform)
        .with_epsg_code(4326);

    let mut writer = GeoTiffWriter::create(&output_path, config, GeoTiffWriterOptions::default())?;
    writer.write(buffer.as_bytes())?;

    println!("File written successfully!");

    // Part 2: Reading and Inspecting the Raster
    println!("\n\nPart 2: Reading the Raster Back");
    println!("--------------------------------");

    let source = FileDataSource::open(&output_path)?;
    let reader = GeoTiffReader::open(source)?;

    println!("File opened: {:?}", output_path);
    println!("\nImage properties:");
    println!(
        "  Dimensions: {}x{} pixels",
        reader.width(),
        reader.height()
    );
    println!("  Bands: {}", reader.band_count());
    println!("  Data type: {:?}", reader.data_type());
    println!("  Compression: {}", reader.compression().name());

    if let Some((tile_w, tile_h)) = reader.tile_size() {
        println!("  Tile size: {}x{}", tile_w, tile_h);
        let (tiles_x, tiles_y) = reader.tile_count();
        println!(
            "  Tile count: {}x{} = {} tiles",
            tiles_x,
            tiles_y,
            tiles_x * tiles_y
        );
    }

    if let Some(gt) = reader.geo_transform() {
        println!("\nGeospatial metadata:");
        println!("  Origin: ({}, {})", gt.origin_x, gt.origin_y);
        println!("  Pixel size: {:?}", gt.resolution());

        let bounds = gt.compute_bounds(reader.width(), reader.height());
        println!(
            "  Bounds: [{}, {}, {}, {}]",
            bounds.min_x, bounds.min_y, bounds.max_x, bounds.max_y
        );
    }

    if let Some(epsg) = reader.epsg_code() {
        println!("  EPSG: {}", epsg);
    }

    // Part 3: Reading and Processing Tiles
    println!("\n\nPart 3: Reading Tile Data");
    println!("-------------------------");

    let tile_buffer = reader.read_tile_buffer(0, 0, 0)?;
    println!(
        "Read tile (0, 0) - {}x{} pixels",
        tile_buffer.width(),
        tile_buffer.height()
    );

    // Get corner pixel values
    let top_left = tile_buffer.get_pixel(0, 0)?;
    let top_right = tile_buffer.get_pixel(tile_buffer.width() - 1, 0)?;
    let bottom_left = tile_buffer.get_pixel(0, tile_buffer.height() - 1)?;
    let bottom_right = tile_buffer.get_pixel(tile_buffer.width() - 1, tile_buffer.height() - 1)?;

    println!("\nCorner pixel values:");
    println!("  Top-left: {:.2}", top_left);
    println!("  Top-right: {:.2}", top_right);
    println!("  Bottom-left: {:.2}", bottom_left);
    println!("  Bottom-right: {:.2}", bottom_right);

    // Compute statistics on the tile
    let tile_stats = tile_buffer.compute_statistics()?;
    println!("\nTile statistics:");
    println!("  Min: {:.2}", tile_stats.min);
    println!("  Max: {:.2}", tile_stats.max);
    println!("  Mean: {:.2}", tile_stats.mean);

    // Part 4: Basic Raster Operations
    println!("\n\nPart 4: Basic Operations");
    println!("------------------------");

    // Create a scaled version
    let mut scaled_buffer = tile_buffer.clone();
    println!("Scaling values by 0.5...");

    for y in 0..scaled_buffer.height() {
        for x in 0..scaled_buffer.width() {
            let value = scaled_buffer.get_pixel(x, y)?;
            scaled_buffer.set_pixel(x, y, value * 0.5)?;
        }
    }

    let scaled_stats = scaled_buffer.compute_statistics()?;
    println!("Scaled buffer statistics:");
    println!("  Min: {:.2}", scaled_stats.min);
    println!("  Max: {:.2}", scaled_stats.max);
    println!("  Mean: {:.2}", scaled_stats.mean);

    // Write the scaled version
    let scaled_path = temp_dir.join("quickstart_scaled.tif");
    println!("\nWriting scaled version to: {:?}", scaled_path);

    let scaled_config = WriterConfig::new(
        scaled_buffer.width(),
        scaled_buffer.height(),
        1,
        scaled_buffer.data_type(),
    )
    .with_compression(Compression::Lzw)
    .with_tile_size(256, 256);
    let mut scaled_writer =
        GeoTiffWriter::create(&scaled_path, scaled_config, GeoTiffWriterOptions::default())?;
    scaled_writer.write(scaled_buffer.as_bytes())?;

    println!("Scaled file written successfully!");

    // Summary
    println!("\n\n=== Tutorial Complete! ===");
    println!("\nKey Concepts Covered:");
    println!("  1. Creating raster buffers");
    println!("  2. Computing statistics");
    println!("  3. Setting geospatial metadata");
    println!("  4. Writing tiled, compressed GeoTIFFs");
    println!("  5. Reading raster data and metadata");
    println!("  6. Accessing tile data");
    println!("  7. Basic raster transformations");

    println!("\nOutput files:");
    println!("  - {:?}", output_path);
    println!("  - {:?}", scaled_path);

    println!("\nNext Steps:");
    println!("  - Try tutorial 02 for raster operations (reprojection, resampling)");
    println!("  - Try tutorial 03 for vector operations");
    println!("  - Explore the API documentation for more features");

    Ok(())
}
