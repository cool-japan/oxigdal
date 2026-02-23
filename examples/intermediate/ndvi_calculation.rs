//! Intermediate example: NDVI (Normalized Difference Vegetation Index) Calculation
//!
//! This example demonstrates how to:
//! - Read multi-band satellite imagery
//! - Perform band math (NDVI calculation)
//! - Handle NoData values
//! - Create output with color table
//! - Calculate statistics

use oxigdal_core::{Dataset, DataType};
use oxigdal_proj::SpatialRef;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("NDVI Calculation Example");
    println!("========================");
    println!();

    // Input file should have at least 4 bands (for Landsat/Sentinel-2)
    // Band 3 = Red, Band 4 = NIR (Near Infrared)
    let input_path = "examples/data/multispectral.tif";

    println!("Opening multispectral image: {}", input_path);

    let dataset = Dataset::open(input_path).await?;

    println!("  Dimensions: {} x {}", dataset.width(), dataset.height());
    println!("  Bands: {}", dataset.band_count());

    if dataset.band_count() < 4 {
        return Err("Input must have at least 4 bands (Red=3, NIR=4)".into());
    }

    let width = dataset.width() as usize;
    let height = dataset.height() as usize;

    // Read Red band (Band 3 for Landsat)
    println!();
    println!("Reading Red band (Band 3)...");
    let red_band = dataset.band(3)?;
    let mut red = vec![0.0f32; width * height];
    red_band.read_block(0, 0, width, height, &mut red).await?;

    // Read NIR band (Band 4 for Landsat)
    println!("Reading NIR band (Band 4)...");
    let nir_band = dataset.band(4)?;
    let mut nir = vec![0.0f32; width * height];
    nir_band.read_block(0, 0, width, height, &mut nir).await?;

    // Calculate NDVI: (NIR - Red) / (NIR + Red)
    println!();
    println!("Calculating NDVI...");

    let no_data_value = -1.0;
    let mut ndvi = Vec::with_capacity(width * height);
    let mut valid_count = 0;
    let mut vegetation_count = 0;

    for (n, r) in nir.iter().zip(red.iter()) {
        let sum = n + r;

        let ndvi_value = if sum.abs() > f32::EPSILON && *n >= 0.0 && *r >= 0.0 {
            valid_count += 1;
            let value = (n - r) / sum;

            // Count vegetation pixels (NDVI > 0.2)
            if value > 0.2 {
                vegetation_count += 1;
            }

            value
        } else {
            no_data_value
        };

        ndvi.push(ndvi_value);
    }

    println!("  Total pixels: {}", width * height);
    println!("  Valid pixels: {}", valid_count);
    println!("  Vegetation pixels (NDVI > 0.2): {}", vegetation_count);
    println!("  Vegetation coverage: {:.2}%",
             (vegetation_count as f64 / valid_count as f64) * 100.0);

    // Calculate NDVI statistics
    let valid_ndvi: Vec<f32> = ndvi
        .iter()
        .filter(|&&v| (v - no_data_value).abs() > f32::EPSILON)
        .copied()
        .collect();

    if !valid_ndvi.is_empty() {
        let sum: f32 = valid_ndvi.iter().sum();
        let mean = sum / valid_ndvi.len() as f32;

        let min = valid_ndvi
            .iter()
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .copied()
            .unwrap_or(0.0);

        let max = valid_ndvi
            .iter()
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .copied()
            .unwrap_or(0.0);

        let variance: f32 = valid_ndvi
            .iter()
            .map(|v| (v - mean).powi(2))
            .sum::<f32>()
            / valid_ndvi.len() as f32;

        let std_dev = variance.sqrt();

        println!();
        println!("NDVI Statistics:");
        println!("  Min:     {:.4}", min);
        println!("  Max:     {:.4}", max);
        println!("  Mean:    {:.4}", mean);
        println!("  Std Dev: {:.4}", std_dev);
    }

    // Create output dataset
    println!();
    println!("Creating output NDVI file...");

    let output_path = "output_ndvi.tif";

    let mut output = Dataset::create_with_type(
        output_path,
        width as u32,
        height as u32,
        1,
        DataType::Float32,
    )
    .await?;

    // Copy geotransform and projection from input
    output.set_geo_transform(dataset.geo_transform()?)?;
    output.set_spatial_ref(&dataset.spatial_ref()?)?;

    // Set NoData value
    let out_band = output.band_mut(1)?;
    out_band.set_no_data_value(no_data_value)?;

    // Write NDVI data
    out_band.write_block(0, 0, width, height, &ndvi).await?;

    // Create color interpretation
    create_ndvi_color_table(&mut output)?;

    output.flush().await?;

    println!("  Output saved to: {}", output_path);

    // Create classified output
    println!();
    println!("Creating classified NDVI (vegetation classes)...");

    let classified = classify_ndvi(&ndvi, no_data_value);

    let classified_path = "output_ndvi_classified.tif";

    let mut classified_output = Dataset::create_with_type(
        classified_path,
        width as u32,
        height as u32,
        1,
        DataType::UInt8,
    )
    .await?;

    classified_output.set_geo_transform(dataset.geo_transform()?)?;
    classified_output.set_spatial_ref(&dataset.spatial_ref()?)?;

    let classified_band = classified_output.band_mut(1)?;
    classified_band.set_no_data_value(0.0)?;

    classified_band
        .write_block_as::<u8>(0, 0, width, height, &classified)
        .await?;

    classified_output.flush().await?;

    println!("  Classified output saved to: {}", classified_path);

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

/// Create an NDVI color table
fn create_ndvi_color_table(dataset: &mut Dataset) -> Result<(), Box<dyn std::error::Error>> {
    // NDVI typically ranges from -1 to 1
    // Negative values: Water
    // 0 - 0.2: Barren/rock
    // 0.2 - 0.4: Sparse vegetation
    // 0.4 - 0.6: Moderate vegetation
    // > 0.6: Dense vegetation

    // This would set up a color ramp (implementation depends on driver capabilities)
    Ok(())
}

/// Classify NDVI into discrete categories
fn classify_ndvi(ndvi: &[f32], no_data: f32) -> Vec<u8> {
    ndvi.iter()
        .map(|&value| {
            if (value - no_data).abs() < f32::EPSILON || value <= 0.0 {
                0  // NoData or water
            } else if value <= 0.2 {
                1  // Barren/rock
            } else if value <= 0.4 {
                2  // Sparse vegetation
            } else if value <= 0.6 {
                3  // Moderate vegetation
            } else {
                4  // Dense vegetation
            }
        })
        .collect()
}
