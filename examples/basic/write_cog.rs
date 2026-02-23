//! Basic example: Creating a Cloud Optimized GeoTIFF (COG)
//!
//! This example demonstrates how to:
//! - Create a new GeoTIFF dataset
//! - Generate synthetic data
//! - Set geotransform and projection
//! - Create a Cloud Optimized GeoTIFF with tiling and overviews

use oxigdal_core::{Dataset, DataType};
use oxigdal_geotiff::{GeoTiffDriver, CogOptions};
use oxigdal_proj::SpatialRef;
use std::f32::consts::PI;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating Cloud Optimized GeoTIFF (COG)...");
    println!();

    // Define raster parameters
    let width = 2048;
    let height = 2048;
    let bands = 1;

    // Generate synthetic elevation data (sine wave pattern)
    println!("Generating synthetic elevation data ({} x {} pixels)...", width, height);
    let data = generate_elevation_data(width, height);

    // Create temporary standard GeoTIFF first
    let temp_dir = std::env::temp_dir();
    let temp_path = temp_dir.join("temp_elevation.tif");
    let temp_path = temp_path.to_str().expect("valid temp path");

    println!("Creating temporary GeoTIFF...");
    let mut dataset = Dataset::create_with_type(
        temp_path,
        width,
        height,
        bands,
        DataType::Float32,
    )
    .await?;

    // Set geotransform (covering a 1 degree x 1 degree area)
    let geo_transform = [
        -180.0, // Origin X (longitude)
        1.0 / width as f64,  // Pixel width
        0.0,    // Rotation X
        90.0,   // Origin Y (latitude)
        0.0,    // Rotation Y
        -1.0 / height as f64, // Pixel height (negative for north-up)
    ];

    dataset.set_geo_transform(geo_transform)?;
    println!("  Set geotransform: [{:.6}, {:.6}, {:.6}, {:.6}, {:.6}, {:.6}]",
             geo_transform[0], geo_transform[1], geo_transform[2],
             geo_transform[3], geo_transform[4], geo_transform[5]);

    // Set WGS84 projection
    let srs = SpatialRef::from_epsg(4326)?;
    dataset.set_spatial_ref(&srs)?;
    println!("  Set projection: WGS84 (EPSG:4326)");

    // Write data to band
    let band = dataset.band_mut(1)?;
    band.set_no_data_value(-9999.0)?;
    band.write_block(0, 0, width as usize, height as usize, &data).await?;

    println!("  Wrote {} pixels", data.len());

    // Flush to ensure data is written
    dataset.flush().await?;
    drop(dataset);  // Close the temporary file

    // Convert to COG
    println!();
    println!("Converting to Cloud Optimized GeoTIFF...");

    let output_path = "output_elevation_cog.tif";

    let driver = GeoTiffDriver::new();

    let cog_options = CogOptions {
        tile_size: 512,
        compression: "DEFLATE".to_string(),
        compression_level: Some(6),
        overview_levels: vec![2, 4, 8, 16],
        resampling: "AVERAGE".to_string(),
        predictor: Some(2),  // Horizontal differencing for better compression
        ..Default::default()
    };

    println!("  Tile size: {} x {}", cog_options.tile_size, cog_options.tile_size);
    println!("  Compression: {} (level {})",
             cog_options.compression,
             cog_options.compression_level.unwrap_or(6));
    println!("  Overview levels: {:?}", cog_options.overview_levels);
    println!("  Resampling: {}", cog_options.resampling);

    // Open temp file and create COG
    let temp_dataset = Dataset::open(temp_path).await?;
    driver.create_cog(&temp_dataset, output_path, cog_options).await?;

    println!();
    println!("COG created successfully: {}", output_path);

    // Verify the output
    let cog_dataset = Dataset::open(output_path).await?;
    let overview_count = cog_dataset.overview_count()?;

    println!();
    println!("Verification:");
    println!("  Dimensions: {} x {}", cog_dataset.width(), cog_dataset.height());
    println!("  Bands: {}", cog_dataset.band_count());
    println!("  Overviews: {}", overview_count);

    // Check overview dimensions
    for i in 0..overview_count {
        let overview = cog_dataset.overview(i)?;
        println!("    Level {}: {} x {}", i + 1, overview.width(), overview.height());
    }

    // Check file sizes
    let temp_size = std::fs::metadata(temp_path)?.len();
    let cog_size = std::fs::metadata(output_path)?.len();

    println!();
    println!("File Sizes:");
    println!("  Original: {:.2} MB", temp_size as f64 / 1_000_000.0);
    println!("  COG: {:.2} MB", cog_size as f64 / 1_000_000.0);
    println!("  Compression ratio: {:.1}%",
             (1.0 - cog_size as f64 / temp_size as f64) * 100.0);

    // Clean up temporary file
    std::fs::remove_file(temp_path)?;

    println!();
    println!("Example completed successfully!");
    println!("You can now use this COG with cloud storage for efficient range requests.");

    Ok(())
}

/// Generate synthetic elevation data with a wave pattern
fn generate_elevation_data(width: u32, height: u32) -> Vec<f32> {
    let mut data = Vec::with_capacity((width * height) as usize);

    for y in 0..height {
        for x in 0..width {
            // Create a wave pattern
            let x_norm = x as f32 / width as f32;
            let y_norm = y as f32 / height as f32;

            let elevation = 1000.0
                + 500.0 * (2.0 * PI * x_norm * 3.0).sin()
                + 300.0 * (2.0 * PI * y_norm * 2.0).cos()
                + 200.0 * ((x_norm - 0.5).powi(2) + (y_norm - 0.5).powi(2)).sqrt();

            data.push(elevation);
        }
    }

    data
}
