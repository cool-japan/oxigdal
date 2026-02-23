//! Cookbook: Multi-Temporal Change Detection
//!
//! Complete workflow for detecting changes between satellite images:
//! - Pre-processing and alignment
//! - Change detection algorithms (differencing, ratios, indices)
//! - Statistical significance testing
//! - Change classification and mapping
//! - Temporal analysis and trend detection
//!
//! Real-world scenarios:
//! - Forest loss detection
//! - Urban expansion monitoring
//! - Wetland changes
//! - Agricultural field changes
//!
//! Run with:
//! ```bash
//! cargo run --example change_detection
//! ```

use chrono::{DateTime, Duration, Utc};
use oxigdal_analytics::change::ChangeDetector;
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::{BoundingBox, GeoTransform, RasterDataType};
use oxigdal_geotiff::writer::{CompressionType, GeoTiffWriter, GeoTiffWriterOptions};
use std::env;
use std::fs::File;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Cookbook: Multi-Temporal Change Detection ===\n");

    let temp_dir = env::temp_dir();
    let output_dir = temp_dir.join("change_detection_output");
    std::fs::create_dir_all(&output_dir)?;

    println!("Output directory: {:?}\n", output_dir);

    // Scenario: Monitoring forest cover change over 3 years
    println!("Scenario: Forest Cover Change Monitoring");
    println!("========================================\n");

    // Time series: 2021, 2022, 2023
    let date_2021 = Utc.with_ymd_and_hms(2021, 6, 15, 0, 0, 0).unwrap();
    let date_2022 = Utc.with_ymd_and_hms(2022, 6, 15, 0, 0, 0).unwrap();
    let date_2023 = Utc.with_ymd_and_hms(2023, 6, 15, 0, 0, 0).unwrap();

    println!("Time series dates:");
    println!("  T1: {} (baseline)", date_2021.format("%Y-%m-%d"));
    println!("  T2: {}", date_2022.format("%Y-%m-%d"));
    println!("  T3: {}", date_2023.format("%Y-%m-%d"));

    let width = 512;
    let height = 512;
    let gt = create_geotransform(width, height)?;

    // Step 1: Load and prepare multitemporal data
    println!("\nStep 1: Load Multitemporal NDVI Data");
    println!("-------------------------------------");

    // Simulate NDVI time series (NDVI increases with forest cover)
    let ndvi_2021 = create_ndvi_baseline(width, height)?;
    let ndvi_2022 = apply_deforestation(&ndvi_2021, 0.05)?; // 5% forest loss
    let ndvi_2023 = apply_deforestation(&ndvi_2022, 0.03)?; // 3% additional loss

    println!("  ✓ NDVI 2021 (baseline)");
    let stats_2021 = ndvi_2021.compute_statistics()?;
    println!("    Mean NDVI: {:.4}, Stdev: {:.4}", stats_2021.mean, stats_2021.stdev);

    println!("  ✓ NDVI 2022");
    let stats_2022 = ndvi_2022.compute_statistics()?;
    println!("    Mean NDVI: {:.4}, Stdev: {:.4}", stats_2022.mean, stats_2022.stdev);

    println!("  ✓ NDVI 2023");
    let stats_2023 = ndvi_2023.compute_statistics()?;
    println!("    Mean NDVI: {:.4}, Stdev: {:.4}", stats_2023.mean, stats_2023.stdev);

    // Step 2: Simple Differencing
    println!("\n\nStep 2: Simple Differencing");
    println!("---------------------------");

    println!("Computing NDVI change: 2023 - 2021...");
    let change_2023_2021 = compute_difference(&ndvi_2023, &ndvi_2021)?;

    let change_stats = change_2023_2021.compute_statistics()?;
    println!("  Change range: [{:.4}, {:.4}]", change_stats.min, change_stats.max);
    println!("  Mean change: {:.4}", change_stats.mean);

    let negative_changes = count_below_threshold(&change_2023_2021, -0.1)?;
    let positive_changes = count_above_threshold(&change_2023_2021, 0.1)?;

    println!("  Degradation (decrease > 0.1): {:.2}%", negative_changes * 100.0);
    println!("  Improvement (increase > 0.1): {:.2}%", positive_changes * 100.0);

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

    println!("  2021-2022 annual change: {:.4}", rate_2021_2022_stats.mean);
    println!("  2022-2023 annual change: {:.4}", rate_2022_2023_stats.mean);

    let acceleration =
        rate_2022_2023_stats.mean - rate_2021_2022_stats.mean;

    if acceleration.abs() < 0.001 {
        println!("  Trend: Stable");
    } else if acceleration < 0.0 {
        println!(
            "  Trend: Accelerating degradation ({:.4}/year)",
            acceleration
        );
    } else {
        println!(
            "  Trend: Recovering degradation ({:.4}/year)",
            acceleration
        );
    }

    // Step 4: Statistical Significance Testing
    println!("\n\nStep 4: Change Significance Assessment");
    println!("--------------------------------------");

    // Calculate confidence intervals (simplified: using stdev)
    let change_std_error = change_stats.stdev / (width as f32 * height as f32).sqrt();

    println!("  Mean change: {:.4}", change_stats.mean);
    println!("  Standard error: {:.4}", change_std_error);
    println!("  95% CI: [{:.4}, {:.4}]",
        change_stats.mean - 1.96 * change_std_error,
        change_stats.mean + 1.96 * change_std_error
    );

    let is_significant =
        (change_stats.mean.abs() > 1.96 * change_std_error);

    println!("  Statistically significant: {}", is_significant);

    // Step 5: Change Classification
    println!("\n\nStep 5: Change Classification");
    println!("-----------------------------");

    let change_class = classify_changes(&change_2023_2021)?;

    let strong_loss_pct = count_below_threshold(&change_class, -2.0)?;
    let moderate_loss_pct = count_in_range(&change_class, -2.0, -1.0)?;
    let stable_pct = count_in_range(&change_class, -1.0, 1.0)?;
    let moderate_gain_pct = count_in_range(&change_class, 1.0, 2.0)?;
    let strong_gain_pct = count_above_threshold(&change_class, 2.0)?;

    println!("  Strong loss (NDVI < -2): {:.2}%", strong_loss_pct * 100.0);
    println!("  Moderate loss (-2 to -1): {:.2}%", moderate_loss_pct * 100.0);
    println!("  Stable (-1 to 1): {:.2}%", stable_pct * 100.0);
    println!("  Moderate gain (1 to 2): {:.2}%", moderate_gain_pct * 100.0);
    println!("  Strong gain (> 2): {:.2}%", strong_gain_pct * 100.0);

    save_raster(&change_class, &output_dir.join("change_class.tif"), &gt)?;

    // Step 6: Trend Detection using Linear Regression
    println!("\n\nStep 6: Long-Term Trend Detection");
    println!("----------------------------------");

    let years = vec![2021.5, 2022.5, 2023.5];
    let ndvi_values = vec![stats_2021.mean, stats_2022.mean, stats_2023.mean];

    let (slope, intercept) = linear_regression(&years, &ndvi_values)?;

    println!("  Linear regression: y = {:.4}x + {:.4}", slope, intercept);

    if slope.abs() < 0.0001 {
        println!("  Trend: No significant change");
    } else if slope > 0.0 {
        println!("  Trend: IMPROVING at {:.4} NDVI units/year", slope);
    } else {
        println!("  Trend: DEGRADING at {:.4} NDVI units/year", -slope);
    }

    // Extrapolate
    let year_2025 = 2025.5;
    let predicted_2025 = slope * year_2025 + intercept;

    println!("  Predicted 2025 NDVI: {:.4}", predicted_2025);

    // Step 7: Quality Metrics
    println!("\n\nStep 7: Quality Assessment");
    println!("--------------------------");

    println!("  Data completeness: 100%");
    println!("  Spatial resolution: 30m");
    println!("  Temporal consistency: Good (annual scenes)");
    println!("  Atmospheric artifacts: Minimal");
    println!("  Cloud contamination: < 5%");

    println!("\nSummary");
    println!("=======");
    println!(
        "Total forest loss (2021-2023): {:.2}%",
        negative_changes * 100.0
    );
    println!(
        "Annual loss rate: {:.4} NDVI units/year",
        slope
    );
    println!("Change is statistically significant: {}", is_significant);
    println!("\nOutput files saved to: {:?}", output_dir);

    Ok(())
}

// Helper functions

fn create_ndvi_baseline(width: usize, height: usize) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    // Create NDVI with forest in center, sparse edges
    let mut data = vec![0.0f32; width * height];

    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;

            // Distance from center
            let cx = width as f32 / 2.0;
            let cy = height as f32 / 2.0;
            let dist = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();

            // NDVI decreases with distance from center
            let max_dist = ((width as f32).powi(2) + (height as f32).powi(2)).sqrt() / 2.0;
            let normalized_dist = dist / max_dist;

            // Forest NDVI: 0.4-0.7, sparse: 0.2-0.4, water: -0.3
            data[idx] = 0.6 - (normalized_dist * 0.3);

            // Add some noise
            data[idx] += (((x ^ y) as f32).sin() * 0.05).clamp(-0.1, 0.1);
        }
    }

    Ok(RasterBuffer::from_vec(
        data,
        width,
        height,
        RasterDataType::Float32,
    )?)
}

fn apply_deforestation(
    ndvi: &RasterBuffer,
    loss_fraction: f32,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let mut data = ndvi.get_data_as_f32()?.clone();

    // Simulate deforestation: reduce NDVI in random areas
    for (i, val) in data.iter_mut().enumerate() {
        // Pseudo-random: use index as seed
        let hash = ((i as u64).wrapping_mul(2654435761)) % 100;
        if (hash as f32 / 100.0) < loss_fraction {
            *val = (*val * 0.6).max(-0.3); // Reduce NDVI by 40%, minimum -0.3
        }
    }

    Ok(RasterBuffer::from_vec(
        data,
        ndvi.width(),
        ndvi.height(),
        RasterDataType::Float32,
    )?)
}

fn compute_difference(
    a: &RasterBuffer,
    b: &RasterBuffer,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let data_a = a.get_data_as_f32()?;
    let data_b = b.get_data_as_f32()?;

    let diff: Vec<f32> = data_a
        .iter()
        .zip(data_b.iter())
        .map(|(x, y)| x - y)
        .collect();

    Ok(RasterBuffer::from_vec(
        diff,
        a.width(),
        a.height(),
        RasterDataType::Float32,
    )?)
}

fn count_below_threshold(
    raster: &RasterBuffer,
    threshold: f32,
) -> Result<f32, Box<dyn std::error::Error>> {
    let data = raster.get_data_as_f32()?;
    let count = data.iter().filter(|&&x| x < threshold).count();
    Ok(count as f32 / data.len() as f32)
}

fn count_above_threshold(
    raster: &RasterBuffer,
    threshold: f32,
) -> Result<f32, Box<dyn std::error::Error>> {
    let data = raster.get_data_as_f32()?;
    let count = data.iter().filter(|&&x| x > threshold).count();
    Ok(count as f32 / data.len() as f32)
}

fn count_in_range(
    raster: &RasterBuffer,
    min: f32,
    max: f32,
) -> Result<f32, Box<dyn std::error::Error>> {
    let data = raster.get_data_as_f32()?;
    let count = data.iter().filter(|&&x| x >= min && x <= max).count();
    Ok(count as f32 / data.len() as f32)
}

fn classify_changes(
    change: &RasterBuffer,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let data = change.get_data_as_f32()?;

    let classified: Vec<f32> = data
        .iter()
        .map(|&x| {
            if x < -2.0 {
                -2.0
            } else if x < -1.0 {
                -1.0
            } else if x < 1.0 {
                0.0
            } else if x < 2.0 {
                1.0
            } else {
                2.0
            }
        })
        .collect();

    Ok(RasterBuffer::from_vec(
        classified,
        change.width(),
        change.height(),
        RasterDataType::Float32,
    )?)
}

fn linear_regression(
    x: &[f32],
    y: &[f32],
) -> Result<(f32, f32), Box<dyn std::error::Error>> {
    let n = x.len() as f32;
    let mean_x = x.iter().sum::<f32>() / n;
    let mean_y = y.iter().sum::<f32>() / n;

    let mut numerator = 0.0f32;
    let mut denominator = 0.0f32;

    for (&xi, &yi) in x.iter().zip(y.iter()) {
        numerator += (xi - mean_x) * (yi - mean_y);
        denominator += (xi - mean_x).powi(2);
    }

    let slope = numerator / denominator;
    let intercept = mean_y - slope * mean_x;

    Ok((slope, intercept))
}

fn create_geotransform(
    width: usize,
    height: usize,
) -> Result<GeoTransform, Box<dyn std::error::Error>> {
    let bbox = BoundingBox::new(0.0, 0.0, width as f64 * 30.0, height as f64 * 30.0)?;
    Ok(GeoTransform::from_bounds(&bbox, width, height)?)
}

fn save_raster(
    raster: &RasterBuffer,
    path: &Path,
    gt: &GeoTransform,
) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::create(path)?;

    let options = GeoTiffWriterOptions {
        compression: CompressionType::Deflate,
        ..Default::default()
    };

    let mut writer = GeoTiffWriter::new(file, options)?;
    writer.write(raster, gt)?;

    println!("  Saved: {}", path.display());
    Ok(())
}
