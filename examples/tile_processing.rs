//! Example: Tile Processing
//!
//! This example demonstrates how to:
//! - Process large rasters tile-by-tile
//! - Read and write tiled GeoTIFFs
//! - Perform operations on individual tiles
//! - Handle tile boundaries correctly
//!
//! Run with:
//! ```bash
//! cargo run --example tile_processing
//! ```

use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::io::FileDataSource;
use oxigdal_core::types::{BoundingBox, GeoTransform, RasterDataType};
use oxigdal_geotiff::writer::{GeoTiffWriter, GeoTiffWriterOptions};
use oxigdal_geotiff::GeoTiffReader;
use std::env;
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Tile Processing Example ===");
    println!();

    let temp_dir = env::temp_dir();

    // Step 1: Create a tiled GeoTIFF
    println!("Step 1: Creating tiled GeoTIFF");
    let input_path = temp_dir.join("tiled_input.tif");
    create_tiled_geotiff(&input_path)?;
    println!("Created: {:?}", input_path);
    println!();

    // Step 2: Read and process tiles
    println!("Step 2: Processing tiles");
    let source = FileDataSource::open(&input_path)?;
    let reader = GeoTiffReader::open(source)?;

    println!("Image: {}x{} pixels", reader.width(), reader.height());

    if let Some((tile_w, tile_h)) = reader.tile_size() {
        println!("Tile size: {}x{} pixels", tile_w, tile_h);
    }

    let (tiles_x, tiles_y) = reader.tile_count();
    println!("Tiles: {}x{} = {} total", tiles_x, tiles_y, tiles_x * tiles_y);
    println!();

    // Process each tile
    let mut processed_tiles = Vec::new();

    for ty in 0..tiles_y {
        for tx in 0..tiles_x {
            // Read tile
            let tile_buffer = reader.read_tile_buffer(0, tx, ty)?;

            // Process tile (e.g., apply threshold)
            let processed = process_tile(&tile_buffer, 128.0)?;

            // Compute statistics
            let stats = processed.compute_statistics()?;
            println!(
                "Tile ({}, {}): mean={:.2}, min={:.2}, max={:.2}",
                tx, ty, stats.mean, stats.min, stats.max
            );

            processed_tiles.push(processed);
        }
    }
    println!();

    // Step 3: Reconstruct full image from tiles
    println!("Step 3: Reconstructing image from tiles");
    let full_image = reconstruct_from_tiles(
        &processed_tiles,
        reader.width(),
        reader.height(),
        tiles_x,
        tiles_y,
    )?;

    println!("Reconstructed image: {}x{}", full_image.width(), full_image.height());
    println!();

    // Step 4: Write processed result
    println!("Step 4: Writing processed result");
    let output_path = temp_dir.join("tiled_output.tif");
    write_result(&output_path, &full_image)?;
    println!("Wrote: {:?}", output_path);
    println!();

    // Step 5: Verify result
    println!("Step 5: Verifying result");
    let verify_source = FileDataSource::open(&output_path)?;
    let verify_reader = GeoTiffReader::open(verify_source)?;

    println!("Output image: {}x{}", verify_reader.width(), verify_reader.height());
    println!("Data type: {:?}", verify_reader.data_type());
    println!("Compression: {:?}", verify_reader.compression());

    println!();
    println!("=== Done ===");
    println!("Files created in: {:?}", temp_dir);

    Ok(())
}

/// Create a tiled GeoTIFF
fn create_tiled_geotiff(path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    // Create a 1024x1024 raster
    let mut buffer = RasterBuffer::zeros(1024, 1024, RasterDataType::UInt8);

    // Fill with a gradient pattern
    for y in 0..1024 {
        for x in 0..1024 {
            let value = ((x + y) / 8) % 256;
            buffer.set_pixel(x, y, value as f64)?;
        }
    }

    // Create geotransform
    let bbox = BoundingBox::new(-180.0, -90.0, 180.0, 90.0)?;
    let geo_transform = GeoTransform::from_bounds(&bbox, 1024, 1024)?;

    // Write as tiled GeoTIFF
    let options = GeoTiffWriterOptions {
        geo_transform: Some(geo_transform),
        epsg_code: Some(4326),
        tile_width: Some(256),
        tile_height: Some(256),
        ..Default::default()
    };

    let file = File::create(path)?;
    let writer = GeoTiffWriter::new(file, options)?;
    writer.write_buffer(&buffer)?;

    Ok(())
}

/// Process a single tile (apply threshold)
fn process_tile(
    tile: &RasterBuffer,
    threshold: f64,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let mut result = RasterBuffer::zeros(tile.width(), tile.height(), tile.data_type());

    for y in 0..tile.height() {
        for x in 0..tile.width() {
            let value = tile.get_pixel(x, y)?;

            // Apply threshold
            let new_value = if value > threshold { 255.0 } else { 0.0 };

            result.set_pixel(x, y, new_value)?;
        }
    }

    Ok(result)
}

/// Reconstruct full image from tiles
fn reconstruct_from_tiles(
    tiles: &[RasterBuffer],
    width: u64,
    height: u64,
    tiles_x: u32,
    tiles_y: u32,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let tile_width = tiles[0].width();
    let tile_height = tiles[0].height();
    let data_type = tiles[0].data_type();

    let mut result = RasterBuffer::zeros(width, height, data_type);

    for (i, tile) in tiles.iter().enumerate() {
        let tx = (i as u32) % tiles_x;
        let ty = (i as u32) / tiles_x;

        let x_offset = u64::from(tx) * tile_width;
        let y_offset = u64::from(ty) * tile_height;

        // Copy tile into result
        for y in 0..tile.height() {
            for x in 0..tile.width() {
                let dst_x = x_offset + x;
                let dst_y = y_offset + y;

                if dst_x < width && dst_y < height {
                    let value = tile.get_pixel(x, y)?;
                    result.set_pixel(dst_x, dst_y, value)?;
                }
            }
        }
    }

    Ok(result)
}

/// Write result to file
fn write_result(
    path: &std::path::Path,
    buffer: &RasterBuffer,
) -> Result<(), Box<dyn std::error::Error>> {
    let bbox = BoundingBox::new(-180.0, -90.0, 180.0, 90.0)?;
    let geo_transform = GeoTransform::from_bounds(&bbox, buffer.width(), buffer.height())?;

    let options = GeoTiffWriterOptions {
        geo_transform: Some(geo_transform),
        epsg_code: Some(4326),
        tile_width: Some(256),
        tile_height: Some(256),
        ..Default::default()
    };

    let file = File::create(path)?;
    let writer = GeoTiffWriter::new(file, options)?;
    writer.write_buffer(buffer)?;

    Ok(())
}
