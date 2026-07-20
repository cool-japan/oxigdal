//! Intermediate example: NDVI (Normalized Difference Vegetation Index) Calculation
//!
//! This example demonstrates how to:
//! - Generate synthetic Red/NIR bands (standing in for multi-band satellite imagery)
//! - Perform band math (NDVI calculation)
//! - Handle NoData values
//! - Classify results into vegetation categories
//! - Calculate statistics and write the results as Cloud-friendly GeoTIFFs
//!
//! Run with:
//! ```bash
//! cargo run --example intermediate_ndvi_calculation
//! ```

use oxigeo_core::buffer::RasterBuffer;
use oxigeo_core::types::{BoundingBox, GeoTransform, NoDataValue, RasterDataType};
use oxigeo_geotiff::tiff::Compression;
use oxigeo_geotiff::writer::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};
use std::env;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("NDVI Calculation Example");
    println!("========================");
    println!();

    let width = 512u64;
    let height = 512u64;

    println!("Generating synthetic multispectral image ({width}x{height})...");
    let red = create_band(width, height, 0);
    let nir = create_band(width, height, 1);

    println!("  Dimensions: {width} x {height}");
    println!("  Bands: 2 (Red, NIR)");

    // Calculate NDVI: (NIR - Red) / (NIR + Red)
    println!();
    println!("Calculating NDVI...");

    let no_data_value = -1.0_f64;
    let mut ndvi = RasterBuffer::zeros(width, height, RasterDataType::Float32);
    let mut valid_count = 0u64;
    let mut vegetation_count = 0u64;
    let mut ndvi_values: Vec<f64> = Vec::with_capacity((width * height) as usize);

    for y in 0..height {
        for x in 0..width {
            let r = red.get_pixel(x, y)?;
            let n = nir.get_pixel(x, y)?;
            let sum = n + r;

            let value = if sum.abs() > f64::EPSILON && n >= 0.0 && r >= 0.0 {
                valid_count += 1;
                let value = (n - r) / sum;
                if value > 0.2 {
                    vegetation_count += 1;
                }
                value
            } else {
                no_data_value
            };

            ndvi.set_pixel(x, y, value)?;
            if (value - no_data_value).abs() > f64::EPSILON {
                ndvi_values.push(value);
            }
        }
    }

    println!("  Total pixels: {}", width * height);
    println!("  Valid pixels: {}", valid_count);
    println!("  Vegetation pixels (NDVI > 0.2): {}", vegetation_count);
    println!(
        "  Vegetation coverage: {:.2}%",
        (vegetation_count as f64 / valid_count.max(1) as f64) * 100.0
    );

    // Calculate NDVI statistics
    if !ndvi_values.is_empty() {
        let sum: f64 = ndvi_values.iter().sum();
        let mean = sum / ndvi_values.len() as f64;

        let min = ndvi_values.iter().copied().fold(f64::INFINITY, f64::min);
        let max = ndvi_values
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);

        let variance: f64 =
            ndvi_values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / ndvi_values.len() as f64;
        let std_dev = variance.sqrt();

        println!();
        println!("NDVI Statistics:");
        println!("  Min:     {:.4}", min);
        println!("  Max:     {:.4}", max);
        println!("  Mean:    {:.4}", mean);
        println!("  Std Dev: {:.4}", std_dev);
    }

    // Create output file
    println!();
    println!("Creating output NDVI file...");

    let temp_dir = env::temp_dir();
    let output_path = temp_dir.join("output_ndvi.tif");
    let bbox = BoundingBox::new(-122.5, 37.6, -122.3, 37.8)?;
    let geo_transform = GeoTransform::from_bounds(&bbox, width, height)?;

    let config = WriterConfig::new(width, height, 1, RasterDataType::Float32)
        .with_compression(Compression::Lzw)
        .with_tile_size(256, 256)
        .with_geo_transform(geo_transform)
        .with_nodata(NoDataValue::Float(no_data_value));

    let mut writer = GeoTiffWriter::create(&output_path, config, GeoTiffWriterOptions::default())?;
    writer.write(ndvi.as_bytes())?;

    println!("  Output saved to: {:?}", output_path);

    // Create classified output
    println!();
    println!("Creating classified NDVI (vegetation classes)...");

    let classified = classify_ndvi(&ndvi, no_data_value)?;

    let classified_path = temp_dir.join("output_ndvi_classified.tif");
    let classified_config = WriterConfig::new(width, height, 1, RasterDataType::UInt8)
        .with_compression(Compression::Lzw)
        .with_tile_size(256, 256)
        .with_geo_transform(geo_transform)
        .with_nodata(NoDataValue::Integer(0));

    let mut classified_writer = GeoTiffWriter::create(
        &classified_path,
        classified_config,
        GeoTiffWriterOptions::default(),
    )?;
    classified_writer.write(classified.as_bytes())?;

    println!("  Classified output saved to: {:?}", classified_path);

    println!();
    println!("Example completed successfully!");
    println!();
    println!("NDVI Classification:");
    println!("  Class 0: NoData/Water (NDVI <= 0)");
    println!("  Class 1: Barren/Rock (0 < NDVI <= 0.2)");
    println!("  Class 2: Sparse Vegetation (0.2 < NDVI <= 0.4)");
    println!("  Class 3: Moderate Vegetation (0.4 < NDVI <= 0.6)");
    println!("  Class 4: Dense Vegetation (NDVI > 0.6)");

    Ok(())
}

/// Create a synthetic band with a spatial pattern (band 0 = red-like, band 1 = NIR-like)
fn create_band(width: u64, height: u64, band: u32) -> RasterBuffer {
    let mut buffer = RasterBuffer::zeros(width, height, RasterDataType::Float32);

    for y in 0..height {
        for x in 0..width {
            let dx = x as f64 / width as f64;
            let dy = y as f64 / height as f64;

            let value = if band == 0 {
                // Red band: higher over bare ground, lower over vegetation
                0.15 + 0.10 * (dx * std::f64::consts::PI).sin().abs()
            } else {
                // NIR band: higher over vegetation
                0.10 + 0.55 * (dy * std::f64::consts::PI * 2.0).sin().abs()
            };

            let _ = buffer.set_pixel(x, y, value);
        }
    }

    buffer
}

/// Classify NDVI into discrete categories
fn classify_ndvi(
    ndvi: &RasterBuffer,
    no_data: f64,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let mut classified = RasterBuffer::zeros(ndvi.width(), ndvi.height(), RasterDataType::UInt8);

    for y in 0..ndvi.height() {
        for x in 0..ndvi.width() {
            let value = ndvi.get_pixel(x, y)?;

            let class = if (value - no_data).abs() < f64::EPSILON || value <= 0.0 {
                0.0 // NoData or water
            } else if value <= 0.2 {
                1.0 // Barren/rock
            } else if value <= 0.4 {
                2.0 // Sparse vegetation
            } else if value <= 0.6 {
                3.0 // Moderate vegetation
            } else {
                4.0 // Dense vegetation
            };

            classified.set_pixel(x, y, class)?;
        }
    }

    Ok(classified)
}
