//! Create a test COG file for Basque Country mining region
//!
//! This example demonstrates creating a Cloud Optimized GeoTIFF (COG) file
//! with geographic coordinates centered on the Basque Country/Pyrenees mining region (Northern Spain).
//!
//! Usage:
//!     cargo run --example create_test_cog_basque_country
//!
//! Output:
//!     demo/cog-viewer/iron-belt-test.tif (512x512, tiled COG)

use oxigdal_core::types::{GeoTransform, RasterDataType};
use oxigdal_geotiff::tiff::{ByteOrderType, Compression};
use oxigdal_geotiff::writer::{CogWriter, CogWriterOptions, OverviewResampling, WriterConfig};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating test COG for Basque Country mining region...");

    // Basque Country coordinates (Bilbao area, northern Spain)
    // This is one of Europe's historic iron and steel mining regions
    let center_lat = 43.2630; // North latitude
    let center_lon = -2.9253; // West longitude

    // Image dimensions
    let width = 512u64;
    let height = 512u64;

    // Pixel size in degrees (approximately 30 meters at this latitude)
    // 30m ≈ 0.00027 degrees at latitude ~43°N
    let pixel_size = 0.00027;

    // GeoTransform with tiepoint at pixel (0,0) -> geo (center_lon, center_lat)
    // This means pixel (0,0) maps to the center coordinates
    let geo_transform = GeoTransform {
        origin_x: center_lon,
        origin_y: center_lat,
        pixel_width: pixel_size,
        pixel_height: -pixel_size, // Negative for north-up
        row_rotation: 0.0,
        col_rotation: 0.0,
    };

    // Calculate actual bounds from tiepoint
    let min_lon = center_lon;
    let max_lon = center_lon + width as f64 * pixel_size;
    let max_lat = center_lat;
    let min_lat = center_lat - height as f64 * pixel_size;

    println!("Image dimensions: {}x{}", width, height);
    println!(
        "Center (at pixel 0,0): {:.6}, {:.6}",
        center_lat, center_lon
    );
    println!(
        "Bounds: {:.6} to {:.6} (lon), {:.6} to {:.6} (lat)",
        min_lon, max_lon, min_lat, max_lat
    );
    println!("Pixel size: {:.8} degrees (~30m)", pixel_size);

    // Create test data with a mining-themed pattern
    let data = create_mining_pattern(width as usize, height as usize);

    // Configure COG writer
    let config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
        .with_tile_size(256, 256)
        .with_compression(Compression::Deflate)
        .with_geo_transform(geo_transform)
        .with_epsg_code(4326) // WGS84 geographic
        .with_overviews(true, OverviewResampling::Average)
        .with_overview_levels(vec![2, 4]);

    let options = CogWriterOptions {
        byte_order: ByteOrderType::LittleEndian,
        validate_after_write: true,
    };

    // Create output path
    let output_path = "demo/cog-viewer/iron-belt-test.tif";

    // Create COG
    println!("\nWriting COG to {}...", output_path);
    let mut writer = CogWriter::create(output_path, config, options)?;
    let validation = writer.write(&data)?;

    println!("\nCOG created successfully!");
    println!(
        "Validation: {}",
        if validation.is_valid {
            "VALID"
        } else {
            "INVALID"
        }
    );
    println!("Has overviews: {}", validation.has_overviews);
    println!("Tiles ordered: {}", validation.tiles_ordered);

    if !validation.messages.is_empty() {
        println!("\nValidation messages:");
        for msg in &validation.messages {
            println!("  - {}", msg);
        }
    }

    println!("\nFile ready for use in COG viewer!");
    println!("Load this file by selecting it in the viewer at http://localhost:8080/");

    Ok(())
}

/// Creates a mining-themed test pattern with excavation zones and grid
fn create_mining_pattern(width: usize, height: usize) -> Vec<u8> {
    let mut data = vec![0u8; width * height];

    let center_x = width as f64 / 2.0;
    let center_y = height as f64 / 2.0;

    for y in 0..height {
        for x in 0..width {
            let dx = x as f64 - center_x;
            let dy = y as f64 - center_y;
            let distance = (dx * dx + dy * dy).sqrt();
            let angle = dy.atan2(dx);

            // Create mining pit pattern (spiral excavation)
            let spiral_value = ((distance * 0.02 + angle * 2.0).sin() * 127.5 + 127.5) as u8;

            // Grid pattern for mining roads/infrastructure
            let grid_x = (x / 32) % 2 == 0;
            let grid_y = (y / 32) % 2 == 0;
            let is_grid = (x % 32 < 2 || y % 32 < 2) && (grid_x || grid_y);

            // Radial zones (different extraction zones)
            let zone = (distance / 60.0) as usize % 4;
            let zone_value = match zone {
                0 => 200, // Fresh excavation
                1 => 150, // Processing area
                2 => 100, // Tailings
                _ => 50,  // Undisturbed
            };

            // Combine patterns
            let value = if is_grid {
                255 // Roads/infrastructure
            } else if distance < 100.0 {
                spiral_value // Central mining pit
            } else {
                let blend = (spiral_value as u32 + zone_value as u32) / 2;
                blend as u8
            };

            data[y * width + x] = value;
        }
    }

    // Add border markers
    for i in 0..width {
        data[i] = 255; // Top border
        data[(height - 1) * width + i] = 255; // Bottom border
    }
    for i in 0..height {
        data[i * width] = 255; // Left border
        data[i * width + width - 1] = 255; // Right border
    }

    // Add some "mining zones" as darker rectangles
    for zone in 0..3 {
        let start_x = 100 + zone * 120;
        let start_y = 100 + zone * 80;
        let end_x = (start_x + 80).min(width);
        let end_y = (start_y + 60).min(height);

        for y in start_y..end_y {
            for x in start_x..end_x {
                if y < height && x < width {
                    data[y * width + x] = data[y * width + x].saturating_sub(50);
                }
            }
        }
    }

    data
}
