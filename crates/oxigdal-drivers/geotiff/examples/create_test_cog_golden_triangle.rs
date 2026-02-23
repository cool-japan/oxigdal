//! Create a test COG file for Golden Triangle region
//!
//! This example demonstrates creating a Cloud Optimized GeoTIFF (COG) file
//! with geographic coordinates centered on the Golden Triangle (Thailand/Myanmar/Laos).
//!
//! Usage:
//!     cargo run --example create_test_cog_laos_cambodia
//!
//! Output:
//!     demo/cog-viewer/golden-triangle-test.tif (512x512, tiled COG)

use oxigdal_core::types::{GeoTransform, RasterDataType};
use oxigdal_geotiff::tiff::{ByteOrderType, Compression};
use oxigdal_geotiff::writer::{CogWriter, CogWriterOptions, OverviewResampling, WriterConfig};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating test COG for Golden Triangle region...");

    // Golden Triangle coordinates (Thailand/Myanmar/Laos border)
    let center_lat = 20.35223590060906;
    let center_lon = 100.08466749884738;

    // Image dimensions
    let width = 512u64;
    let height = 512u64;

    // Pixel size in degrees (approximately 30 meters at this latitude)
    // 30m ≈ 0.00027 degrees at latitude ~20°N
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

    // Create test data with a pattern
    let data = create_test_pattern(width as usize, height as usize);

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
    let output_path = "demo/cog-viewer/golden-triangle-test.tif";

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

/// Creates a test pattern with concentric circles and gradients
fn create_test_pattern(width: usize, height: usize) -> Vec<u8> {
    let mut data = vec![0u8; width * height];

    let center_x = width as f64 / 2.0;
    let center_y = height as f64 / 2.0;
    let max_radius = (width.min(height) as f64 / 2.0) * 0.9;

    for y in 0..height {
        for x in 0..width {
            let dx = x as f64 - center_x;
            let dy = y as f64 - center_y;
            let distance = (dx * dx + dy * dy).sqrt();

            // Concentric circles
            let circle_value = ((distance / max_radius * 10.0).sin() * 127.5 + 127.5) as u8;

            // Radial gradient
            let gradient_value = ((1.0 - distance / max_radius).max(0.0) * 255.0) as u8;

            // Checkerboard pattern
            let checker = if (x / 32 + y / 32) % 2 == 0 { 200 } else { 50 };

            // Combine patterns
            let value = if distance < max_radius * 0.3 {
                circle_value
            } else if distance < max_radius * 0.7 {
                gradient_value
            } else {
                checker
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

    data
}
