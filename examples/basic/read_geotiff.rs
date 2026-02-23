//! Basic example: Reading a GeoTIFF file
//!
//! This example demonstrates how to:
//! - Open a GeoTIFF dataset
//! - Read raster metadata
//! - Read raster data into a buffer
//! - Access spatial reference information

use oxigdal_core::Dataset;
use std::path::Path;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Open a GeoTIFF file
    let path = Path::new("examples/data/sample.tif");

    let dataset = match Dataset::open(path).await {
        Ok(ds) => ds,
        Err(e) => {
            eprintln!("Failed to open dataset: {}", e);
            eprintln!("Make sure the file exists at: {:?}", path);
            return Err(e.into());
        }
    };

    println!("Successfully opened: {:?}", path);
    println!();

    // Get raster dimensions
    let width = dataset.width();
    let height = dataset.height();
    let band_count = dataset.band_count();

    println!("Raster Dimensions:");
    println!("  Width:  {} pixels", width);
    println!("  Height: {} pixels", height);
    println!("  Bands:  {}", band_count);
    println!();

    // Get geotransform
    let geo_transform = dataset.geo_transform()?;
    println!("Geotransform:");
    println!("  Origin X: {}", geo_transform[0]);
    println!("  Pixel Width: {}", geo_transform[1]);
    println!("  Rotation X: {}", geo_transform[2]);
    println!("  Origin Y: {}", geo_transform[3]);
    println!("  Rotation Y: {}", geo_transform[4]);
    println!("  Pixel Height: {}", geo_transform[5]);
    println!();

    // Get spatial reference
    let srs = dataset.spatial_ref()?;
    println!("Spatial Reference:");

    if let Some(epsg) = srs.authority_code()? {
        println!("  EPSG Code: {}", epsg);
    }

    if let Ok(proj_name) = srs.projection_name() {
        println!("  Projection: {}", proj_name);
    }

    if srs.is_geographic()? {
        println!("  Type: Geographic");
    } else if srs.is_projected()? {
        println!("  Type: Projected");
    }

    println!();

    // Access first band
    let band = dataset.band(1)?;
    println!("Band 1 Information:");
    println!("  Data Type: {:?}", band.data_type());

    if let Some(no_data) = band.no_data_value() {
        println!("  NoData Value: {}", no_data);
    }

    println!("  Block Size: {} x {}", band.block_width(), band.block_height());
    println!();

    // Read a small sample of data (top-left 100x100 pixels)
    let sample_size = 100;
    let read_width = sample_size.min(width as usize);
    let read_height = sample_size.min(height as usize);

    let mut buffer = vec![0.0f32; read_width * read_height];

    println!("Reading {}x{} sample from top-left corner...", read_width, read_height);

    band.read_block(0, 0, read_width, read_height, &mut buffer).await?;

    // Calculate statistics for the sample
    let no_data = band.no_data_value();
    let valid_pixels: Vec<f32> = buffer
        .iter()
        .filter(|&&v| no_data.map_or(true, |nd| (v - nd).abs() > f32::EPSILON))
        .copied()
        .collect();

    if !valid_pixels.is_empty() {
        let sum: f32 = valid_pixels.iter().sum();
        let mean = sum / valid_pixels.len() as f32;

        let min = valid_pixels
            .iter()
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .copied()
            .unwrap_or(0.0);

        let max = valid_pixels
            .iter()
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .copied()
            .unwrap_or(0.0);

        println!();
        println!("Sample Statistics:");
        println!("  Valid Pixels: {}", valid_pixels.len());
        println!("  Min Value: {:.2}", min);
        println!("  Max Value: {:.2}", max);
        println!("  Mean Value: {:.2}", mean);
    } else {
        println!("No valid pixels found in sample");
    }

    println!();
    println!("Example completed successfully!");

    Ok(())
}
