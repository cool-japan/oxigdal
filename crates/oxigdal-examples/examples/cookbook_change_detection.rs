//! Cookbook: Multi-Temporal Change Detection
//!
//! Complete workflow for detecting changes between satellite images:
//! - Pre-processing and alignment
//! - Change detection algorithms (differencing, classification)
//! - Statistical significance testing
//! - Long-term trend detection (linear regression)
//!
//! Real-world scenarios:
//! - Forest loss detection
//! - Urban expansion monitoring
//! - Wetland changes
//! - Agricultural field changes
//!
//! Run with:
//! ```bash
//! cargo run --example cookbook_change_detection
//! ```

use chrono::{TimeZone, Utc};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::{BoundingBox, GeoTransform, RasterDataType};
use oxigdal_geotiff::tiff::Compression;
use oxigdal_geotiff::writer::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};
use std::env;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Cookbook: Multi-Temporal Change Detection ===\n");

    let temp_dir = env::temp_dir();
    let output_dir = temp_dir.join("change_detection_output");
    std::fs::create_dir_all(&output_dir)?;

    println!("Output directory: {:?}\n", output_dir);

    println!("Scenario: Forest Cover Change Monitoring");
    println!("========================================\n");

    let date_2021 = Utc
        .with_ymd_and_hms(2021, 6, 15, 0, 0, 0)
        .single()
        .ok_or("bad date")?;
    let date_2022 = Utc
        .with_ymd_and_hms(2022, 6, 15, 0, 0, 0)
        .single()
        .ok_or("bad date")?;
    let date_2023 = Utc
        .with_ymd_and_hms(2023, 6, 15, 0, 0, 0)
        .single()
        .ok_or("bad date")?;

    println!("Time series dates:");
    println!("  T1: {} (baseline)", date_2021.format("%Y-%m-%d"));
    println!("  T2: {}", date_2022.format("%Y-%m-%d"));
    println!("  T3: {}", date_2023.format("%Y-%m-%d"));

    let width = 256u64;
    let height = 256u64;
    let gt = create_geotransform(width, height)?;

    // Step 1: Load and prepare multitemporal data
    println!("\nStep 1: Load Multitemporal NDVI Data");
    println!("-------------------------------------");

    let ndvi_2021 = create_ndvi_baseline(width, height);
    let ndvi_2022 = apply_deforestation(&ndvi_2021, 0.05)?; // 5% forest loss
    let ndvi_2023 = apply_deforestation(&ndvi_2022, 0.03)?; // 3% additional loss

    println!("  NDVI 2021 (baseline)");
    let stats_2021 = ndvi_2021.compute_statistics()?;
    println!(
        "    Mean NDVI: {:.4}, StdDev: {:.4}",
        stats_2021.mean, stats_2021.std_dev
    );

    println!("  NDVI 2022");
    let stats_2022 = ndvi_2022.compute_statistics()?;
    println!(
        "    Mean NDVI: {:.4}, StdDev: {:.4}",
        stats_2022.mean, stats_2022.std_dev
    );

    println!("  NDVI 2023");
    let stats_2023 = ndvi_2023.compute_statistics()?;
    println!(
        "    Mean NDVI: {:.4}, StdDev: {:.4}",
        stats_2023.mean, stats_2023.std_dev
    );

    // Step 2: Simple Differencing
    println!("\n\nStep 2: Simple Differencing");
    println!("---------------------------");

    println!("Computing NDVI change: 2023 - 2021...");
    let change_2023_2021 = compute_difference(&ndvi_2023, &ndvi_2021)?;

    let change_stats = change_2023_2021.compute_statistics()?;
    println!(
        "  Change range: [{:.4}, {:.4}]",
        change_stats.min, change_stats.max
    );
    println!("  Mean change: {:.4}", change_stats.mean);

    let negative_changes = count_below_threshold(&change_2023_2021, -0.1)?;
    let positive_changes = count_above_threshold(&change_2023_2021, 0.1)?;

    println!(
        "  Degradation (decrease > 0.1): {:.2}%",
        negative_changes * 100.0
    );
    println!(
        "  Improvement (increase > 0.1): {:.2}%",
        positive_changes * 100.0
    );

    save_raster(
        &change_2023_2021,
        &output_dir.join("change_2023_2021.tif"),
        &gt,
    )?;

    // Step 3: Annual Change Rate
    println!("\n\nStep 3: Annual Change Analysis");
    println!("-------------------------------");

    let annual_rate_2021_2022 = compute_difference(&ndvi_2022, &ndvi_2021)?;
    let annual_rate_2022_2023 = compute_difference(&ndvi_2023, &ndvi_2022)?;

    let rate_2021_2022_stats = annual_rate_2021_2022.compute_statistics()?;
    let rate_2022_2023_stats = annual_rate_2022_2023.compute_statistics()?;

    println!(
        "  2021-2022 annual change: {:.4}",
        rate_2021_2022_stats.mean
    );
    println!(
        "  2022-2023 annual change: {:.4}",
        rate_2022_2023_stats.mean
    );

    let acceleration = rate_2022_2023_stats.mean - rate_2021_2022_stats.mean;

    if acceleration.abs() < 0.001 {
        println!("  Trend: Stable");
    } else if acceleration < 0.0 {
        println!(
            "  Trend: Accelerating degradation ({:.4}/year)",
            acceleration
        );
    } else {
        println!("  Trend: Recovering degradation ({:.4}/year)", acceleration);
    }

    // Step 4: Statistical Significance Testing
    println!("\n\nStep 4: Change Significance Assessment");
    println!("--------------------------------------");

    let change_std_error = change_stats.std_dev / (width as f64 * height as f64).sqrt();

    println!("  Mean change: {:.4}", change_stats.mean);
    println!("  Standard error: {:.6}", change_std_error);
    println!(
        "  95% CI: [{:.4}, {:.4}]",
        change_stats.mean - 1.96 * change_std_error,
        change_stats.mean + 1.96 * change_std_error
    );

    let is_significant = change_stats.mean.abs() > 1.96 * change_std_error;

    println!("  Statistically significant: {}", is_significant);

    // Step 5: Change Classification
    println!("\n\nStep 5: Change Classification");
    println!("-----------------------------");

    let change_class = classify_changes(&change_2023_2021)?;

    let strong_loss_pct = count_below_threshold(&change_class, -0.15)?;
    let moderate_loss_pct = count_in_range(&change_class, -0.15, -0.05)?;
    let stable_pct = count_in_range(&change_class, -0.05, 0.05)?;
    let moderate_gain_pct = count_in_range(&change_class, 0.05, 0.15)?;
    let strong_gain_pct = count_above_threshold(&change_class, 0.15)?;

    println!("  Strong loss (< -0.15): {:.2}%", strong_loss_pct * 100.0);
    println!(
        "  Moderate loss (-0.15 to -0.05): {:.2}%",
        moderate_loss_pct * 100.0
    );
    println!("  Stable (-0.05 to 0.05): {:.2}%", stable_pct * 100.0);
    println!(
        "  Moderate gain (0.05 to 0.15): {:.2}%",
        moderate_gain_pct * 100.0
    );
    println!("  Strong gain (> 0.15): {:.2}%", strong_gain_pct * 100.0);

    save_raster(&change_class, &output_dir.join("change_class.tif"), &gt)?;

    // Step 6: Trend Detection using Linear Regression
    println!("\n\nStep 6: Long-Term Trend Detection");
    println!("----------------------------------");

    let years = [2021.5, 2022.5, 2023.5];
    let ndvi_values = [stats_2021.mean, stats_2022.mean, stats_2023.mean];

    let (slope, intercept) = linear_regression(&years, &ndvi_values);

    println!("  Linear regression: y = {:.4}x + {:.4}", slope, intercept);

    if slope.abs() < 0.0001 {
        println!("  Trend: No significant change");
    } else if slope > 0.0 {
        println!("  Trend: IMPROVING at {:.4} NDVI units/year", slope);
    } else {
        println!("  Trend: DEGRADING at {:.4} NDVI units/year", -slope);
    }

    let year_2025 = 2025.5;
    let predicted_2025 = slope * year_2025 + intercept;

    println!("  Predicted 2025 NDVI: {:.4}", predicted_2025);

    // Step 7: Quality Metrics
    println!("\n\nStep 7: Quality Assessment");
    println!("--------------------------");

    println!("  Data completeness: 100%");
    println!("  Spatial resolution: 30m");
    println!("  Temporal consistency: Good (annual scenes)");

    println!("\nSummary");
    println!("=======");
    println!(
        "Total forest loss (2021-2023): {:.2}%",
        negative_changes * 100.0
    );
    println!("Annual loss rate: {:.4} NDVI units/year", slope);
    println!("Change is statistically significant: {}", is_significant);
    println!("\nOutput files saved to: {:?}", output_dir);

    Ok(())
}

// Helper functions

fn create_ndvi_baseline(width: u64, height: u64) -> RasterBuffer {
    let mut buffer = RasterBuffer::zeros(width, height, RasterDataType::Float32);

    for y in 0..height {
        for x in 0..width {
            let cx = width as f64 / 2.0;
            let cy = height as f64 / 2.0;
            let dist = ((x as f64 - cx).powi(2) + (y as f64 - cy).powi(2)).sqrt();

            let max_dist = ((width as f64).powi(2) + (height as f64).powi(2)).sqrt() / 2.0;
            let normalized_dist = dist / max_dist;

            let mut value = 0.6 - (normalized_dist * 0.3);
            value += (((x ^ y) as f64).sin() * 0.05).clamp(-0.1, 0.1);

            let _ = buffer.set_pixel(x, y, value);
        }
    }

    buffer
}

fn apply_deforestation(
    ndvi: &RasterBuffer,
    loss_fraction: f64,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let width = ndvi.width();
    let height = ndvi.height();
    let mut data: Vec<f32> = ndvi.as_slice::<f32>()?.to_vec();

    for (i, val) in data.iter_mut().enumerate() {
        let hash = (i as u64).wrapping_mul(2_654_435_761) % 100;
        if (hash as f64 / 100.0) < loss_fraction {
            *val = (*val * 0.6).max(-0.3);
        }
    }

    Ok(RasterBuffer::from_typed_vec(
        width as usize,
        height as usize,
        data,
        RasterDataType::Float32,
    )?)
}

fn compute_difference(
    a: &RasterBuffer,
    b: &RasterBuffer,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let data_a = a.as_slice::<f32>()?;
    let data_b = b.as_slice::<f32>()?;

    let diff: Vec<f32> = data_a
        .iter()
        .zip(data_b.iter())
        .map(|(x, y)| x - y)
        .collect();

    Ok(RasterBuffer::from_typed_vec(
        a.width() as usize,
        a.height() as usize,
        diff,
        RasterDataType::Float32,
    )?)
}

fn count_below_threshold(
    raster: &RasterBuffer,
    threshold: f32,
) -> Result<f32, Box<dyn std::error::Error>> {
    let data = raster.as_slice::<f32>()?;
    let count = data.iter().filter(|&&x| x < threshold).count();
    Ok(count as f32 / data.len() as f32)
}

fn count_above_threshold(
    raster: &RasterBuffer,
    threshold: f32,
) -> Result<f32, Box<dyn std::error::Error>> {
    let data = raster.as_slice::<f32>()?;
    let count = data.iter().filter(|&&x| x > threshold).count();
    Ok(count as f32 / data.len() as f32)
}

fn count_in_range(
    raster: &RasterBuffer,
    min: f32,
    max: f32,
) -> Result<f32, Box<dyn std::error::Error>> {
    let data = raster.as_slice::<f32>()?;
    let count = data.iter().filter(|&&x| x >= min && x <= max).count();
    Ok(count as f32 / data.len() as f32)
}

fn classify_changes(change: &RasterBuffer) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let data = change.as_slice::<f32>()?;

    let classified: Vec<f32> = data
        .iter()
        .map(|&x| {
            if x < -0.15 {
                -0.2
            } else if x < -0.05 {
                -0.1
            } else if x < 0.05 {
                0.0
            } else if x < 0.15 {
                0.1
            } else {
                0.2
            }
        })
        .collect();

    Ok(RasterBuffer::from_typed_vec(
        change.width() as usize,
        change.height() as usize,
        classified,
        RasterDataType::Float32,
    )?)
}

fn linear_regression(x: &[f64], y: &[f64]) -> (f64, f64) {
    let n = x.len() as f64;
    let mean_x = x.iter().sum::<f64>() / n;
    let mean_y = y.iter().sum::<f64>() / n;

    let mut numerator = 0.0;
    let mut denominator = 0.0;

    for (&xi, &yi) in x.iter().zip(y.iter()) {
        numerator += (xi - mean_x) * (yi - mean_y);
        denominator += (xi - mean_x).powi(2);
    }

    let slope = if denominator.abs() > f64::EPSILON {
        numerator / denominator
    } else {
        0.0
    };
    let intercept = mean_y - slope * mean_x;

    (slope, intercept)
}

fn create_geotransform(
    width: u64,
    height: u64,
) -> Result<GeoTransform, Box<dyn std::error::Error>> {
    let bbox = BoundingBox::new(0.0, 0.0, width as f64 * 30.0, height as f64 * 30.0)?;
    Ok(GeoTransform::from_bounds(&bbox, width, height)?)
}

fn save_raster(
    raster: &RasterBuffer,
    path: &Path,
    gt: &GeoTransform,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = WriterConfig::new(raster.width(), raster.height(), 1, raster.data_type())
        .with_compression(Compression::Deflate)
        .with_tile_size(64, 64)
        .with_geo_transform(*gt);

    let mut writer = GeoTiffWriter::create(path, config, GeoTiffWriterOptions::default())?;
    writer.write(raster.as_bytes())?;

    println!("  Saved: {}", path.display());
    Ok(())
}
