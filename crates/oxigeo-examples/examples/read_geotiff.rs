//! Example: Reading GeoTIFF files
//!
//! This example demonstrates how to:
//! - Open a GeoTIFF file
//! - Read metadata (size, bands, data type, geotransform)
//! - Read tiles
//! - Access geospatial information
//!
//! Run with:
//! ```bash
//! cargo run --example read_geotiff
//! ```

use oxigeo_core::io::FileDataSource;
use oxigeo_core::types::RasterDataType;
use oxigeo_geotiff::GeoTiffReader;
use std::env;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get file path from arguments or use a test file
    let args: Vec<String> = env::args().collect();
    let file_path: &str = if args.len() > 1 {
        args[1].as_str()
    } else {
        // Create a simple test GeoTIFF in temp directory
        let temp_dir = env::temp_dir();
        let test_file = temp_dir.join("test_geotiff.tif");
        create_test_geotiff(&test_file)?;
        println!("Created test file: {:?}", test_file);
        println!("You can also run: cargo run --example read_geotiff <path_to_geotiff>");
        println!();

        // Convert to String and leak to get &'static str
        // (This is fine for an example program that runs once)
        Box::leak(test_file.to_string_lossy().to_string().into_boxed_str())
    };

    println!("=== Reading GeoTIFF ===");
    println!("File: {}", file_path);
    println!();

    // Open the GeoTIFF file
    let source = FileDataSource::open(file_path)?;
    let reader = GeoTiffReader::open(source)?;

    // Print basic metadata
    println!("--- Image Metadata ---");
    println!("Size: {}x{} pixels", reader.width(), reader.height());
    println!("Bands: {}", reader.band_count());

    if let Some(data_type) = reader.data_type() {
        println!("Data type: {:?}", data_type);
        println!("Bytes per pixel: {}", data_type.size_bytes());
    }

    println!("Compression: {}", reader.compression().name());

    if let Some((tile_w, tile_h)) = reader.tile_size() {
        println!("Tile size: {}x{}", tile_w, tile_h);
    } else {
        println!("Layout: Stripped (not tiled)");
    }

    println!();

    // Print geospatial metadata
    println!("--- Geospatial Metadata ---");

    if let Some(gt) = reader.geo_transform() {
        println!("GeoTransform:");
        println!("  Origin: ({}, {})", gt.origin_x, gt.origin_y);
        println!("  Pixel size: {:?}", gt.resolution());
        if gt.has_rotation() {
            println!("  Rotation: {} degrees", gt.rotation_degrees());
        } else {
            println!("  North-Up: yes");
        }

        // Calculate bounds
        let bounds = gt.compute_bounds(reader.width(), reader.height());
        println!("Bounds:");
        println!("  West: {}", bounds.min_x);
        println!("  South: {}", bounds.min_y);
        println!("  East: {}", bounds.max_x);
        println!("  North: {}", bounds.max_y);
    } else {
        println!("No GeoTransform found");
    }

    if let Some(epsg) = reader.epsg_code() {
        println!("EPSG Code: {}", epsg);
    } else {
        println!("No EPSG code found");
    }

    let nodata = reader.nodata();
    if !nodata.is_none() {
        println!("NoData value: {:?}", nodata.as_f64());
    }
    println!();

    // Print overview information. Each level's real size comes from its own
    // IFD via `level_size`, so it is right even for a pyramid that is not a
    // strict power-of-two decimation.
    println!("--- Overviews ---");
    println!("Overview count: {}", reader.overview_count());
    for level in 0..=reader.overview_count() {
        match reader.level_size(level) {
            Ok((width, height)) => println!("  Level {}: {}x{}", level, width, height),
            Err(e) => eprintln!("  Level {}: unavailable ({})", level, e),
        }
    }
    println!();

    // Print tile information
    let (tiles_x, tiles_y) = reader.tile_count();
    println!("--- Tiles ---");
    println!(
        "Tiles: {}x{} = {} total",
        tiles_x,
        tiles_y,
        tiles_x * tiles_y
    );
    println!();

    // Read first tile
    if tiles_x > 0 && tiles_y > 0 {
        println!("--- Reading First Tile ---");
        match reader.read_tile(0, 0, 0) {
            Ok(tile_data) => {
                println!("Successfully read tile (0, 0)");
                println!("Tile data size: {} bytes", tile_data.len());

                // Print first few bytes
                let preview_len = tile_data.len().min(16);
                print!("First {} bytes: ", preview_len);
                for byte in &tile_data[..preview_len] {
                    print!("{:02X} ", byte);
                }
                println!();
            }
            Err(e) => {
                eprintln!("Failed to read tile: {}", e);
            }
        }
        println!();

        // Read the tile as a buffer, one band at a time. A `RasterBuffer` holds
        // a single band, so the band is part of the request;
        // `read_tile_buffer(level, tile_x, tile_y)` is the `band = 0` shorthand.
        println!("--- Reading Tile as RasterBuffer ---");
        for band in 0..reader.band_count() as usize {
            match reader.read_tile_band_buffer(0, band, 0, 0) {
                Ok(buffer) => {
                    println!("Band {}:", band);
                    println!("  Buffer size: {}x{}", buffer.width(), buffer.height());
                    println!("  Buffer data type: {:?}", buffer.data_type());

                    // Get a pixel value
                    if let Ok(value) = buffer.get_pixel(0, 0) {
                        println!("  Pixel (0, 0) value: {}", value);
                    }

                    // Compute statistics
                    if let Ok(stats) = buffer.compute_statistics() {
                        println!("  Statistics:");
                        println!("    Min: {}", stats.min);
                        println!("    Max: {}", stats.max);
                        println!("    Mean: {}", stats.mean);
                        println!("    Std Dev: {}", stats.std_dev);
                        println!("    Valid pixels: {}", stats.valid_count);
                    }
                }
                Err(e) => {
                    eprintln!("Failed to read tile buffer for band {}: {}", band, e);
                }
            }
        }
    }

    println!();
    println!("=== Done ===");

    Ok(())
}

/// Create a simple test GeoTIFF for demonstration
fn create_test_geotiff(path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    use oxigeo_core::buffer::RasterBuffer;
    use oxigeo_core::types::{BoundingBox, GeoTransform};
    use oxigeo_geotiff::tiff::Compression;
    use oxigeo_geotiff::writer::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};

    // Create a simple 512x512 raster
    let mut buffer = RasterBuffer::zeros(512, 512, RasterDataType::UInt8);

    // Fill with a pattern
    for y in 0..512 {
        for x in 0..512 {
            let value = ((x + y) % 256) as f64;
            buffer.set_pixel(x, y, value)?;
        }
    }

    // Create geotransform for WGS84
    let bbox = BoundingBox::new(-180.0, -90.0, 180.0, 90.0)?;
    let geo_transform = GeoTransform::from_bounds(&bbox, 512, 512)?;

    // Write GeoTIFF
    let config = WriterConfig::new(512, 512, 1, RasterDataType::UInt8)
        .with_compression(Compression::Lzw)
        .with_tile_size(256, 256)
        .with_geo_transform(geo_transform)
        .with_epsg_code(4326);

    let mut writer = GeoTiffWriter::create(path, config, GeoTiffWriterOptions::default())?;
    writer.write(buffer.as_bytes())?;

    Ok(())
}
