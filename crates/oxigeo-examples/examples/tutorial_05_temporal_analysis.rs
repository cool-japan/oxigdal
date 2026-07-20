//! Tutorial 05: Temporal Analysis
//!
//! This tutorial demonstrates time-series analysis of geospatial data:
//! - Loading multi-temporal datasets
//! - Change detection (difference, ratio)
//! - Trend analysis
//! - Anomaly detection
//! - Time-series visualization
//! - Temporal aggregation
//!
//! Run with:
//! ```bash
//! cargo run --example tutorial_05_temporal_analysis
//! ```

use chrono::{DateTime, Duration, NaiveDate, Utc};
use oxigeo_core::buffer::RasterBuffer;
use oxigeo_core::types::{BoundingBox, GeoTransform, RasterDataType};
use oxigeo_geotiff::tiff::Compression;
use oxigeo_geotiff::writer::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};
use std::env;

/// A single timestamped raster in a small in-memory time series
struct TemporalRaster {
    timestamp: DateTime<Utc>,
    buffer: RasterBuffer,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Tutorial 05: Temporal Analysis ===\n");

    let temp_dir = env::temp_dir();

    // Step 1: Creating a Temporal Dataset
    println!("Step 1: Creating Temporal Dataset");
    println!("----------------------------------");

    let start_date = NaiveDate::from_ymd_opt(2023, 1, 1).ok_or("Invalid date")?;
    let width = 128u64;
    let height = 128u64;

    println!("Creating monthly NDVI data for 2023...");

    let mut series: Vec<TemporalRaster> = Vec::new();
    for month in 0..12_i64 {
        let date = start_date + Duration::days(month * 30);
        let datetime: DateTime<Utc> = DateTime::from_naive_utc_and_offset(
            date.and_hms_opt(0, 0, 0).ok_or("Invalid time")?,
            Utc,
        );

        let buffer = create_seasonal_ndvi(width, height, month);
        println!("  Added: {} (month {})", date, month + 1);
        series.push(TemporalRaster {
            timestamp: datetime,
            buffer,
        });
    }

    println!("\nCollection created:");
    println!("  Name: NDVI Time Series");
    println!("  Time steps: {}", series.len());
    println!(
        "  Start: {}",
        series.first().ok_or("empty series")?.timestamp
    );
    println!("  End: {}", series.last().ok_or("empty series")?.timestamp);
    println!("  Temporal resolution: ~30 days");

    // Step 2: Temporal Statistics
    println!("\n\nStep 2: Temporal Statistics");
    println!("----------------------------");

    println!("Computing temporal statistics (per-pixel)...");

    let mean_buffer = temporal_mean(&series);
    let std_buffer = temporal_std(&series, &mean_buffer)?;

    let mean_stats = mean_buffer.compute_statistics()?;
    let std_stats = std_buffer.compute_statistics()?;

    println!("\nTemporal mean NDVI:");
    println!("  Min: {:.4}", mean_stats.min);
    println!("  Max: {:.4}", mean_stats.max);
    println!("  Mean: {:.4}", mean_stats.mean);

    println!("\nTemporal std dev NDVI:");
    println!("  Min: {:.4}", std_stats.min);
    println!("  Max: {:.4}", std_stats.max);
    println!("  Mean: {:.4}", std_stats.mean);

    let bbox = BoundingBox::new(-10.0, 40.0, 10.0, 50.0)?;
    let gt = GeoTransform::from_bounds(&bbox, width, height)?;

    save_raster(&mean_buffer, &temp_dir.join("ndvi_temporal_mean.tif"), &gt)?;
    save_raster(&std_buffer, &temp_dir.join("ndvi_temporal_std.tif"), &gt)?;

    println!("\nSaved temporal statistics:");
    println!("  - ndvi_temporal_mean.tif");
    println!("  - ndvi_temporal_std.tif");

    // Step 3: Change Detection
    println!("\n\nStep 3: Change Detection");
    println!("------------------------");

    let t1_buffer = &series[0].buffer;
    let t2_buffer = &series[6].buffer;

    println!("Detecting changes between January and July 2023...");

    // Method 1: Simple difference
    println!("\nMethod 1: Simple Difference");
    let diff_buffer = subtract_buffers(t2_buffer, t1_buffer)?;
    let diff_stats = diff_buffer.compute_statistics()?;

    println!("  Change statistics:");
    println!("    Min change: {:.4}", diff_stats.min);
    println!("    Max change: {:.4}", diff_stats.max);
    println!("    Mean change: {:.4}", diff_stats.mean);

    save_raster(&diff_buffer, &temp_dir.join("ndvi_change_diff.tif"), &gt)?;

    // Method 2: Ratio
    println!("\nMethod 2: Ratio");
    let ratio_buffer = divide_buffers(t2_buffer, t1_buffer)?;
    let ratio_stats = ratio_buffer.compute_statistics()?;

    println!("  Ratio statistics:");
    println!("    Min: {:.4}", ratio_stats.min);
    println!("    Max: {:.4}", ratio_stats.max);
    println!("    Mean: {:.4}", ratio_stats.mean);

    save_raster(&ratio_buffer, &temp_dir.join("ndvi_change_ratio.tif"), &gt)?;

    // Count significant changes (threshold at 2 std dev)
    let threshold = mean_stats.mean + 2.0 * std_stats.mean;
    let mut significant_pixels = 0;

    for y in 0..diff_buffer.height() {
        for x in 0..diff_buffer.width() {
            let value = diff_buffer.get_pixel(x, y)?;
            if value.abs() > threshold {
                significant_pixels += 1;
            }
        }
    }

    let total_pixels = diff_buffer.width() * diff_buffer.height();
    let change_percentage = (significant_pixels as f64 / total_pixels as f64) * 100.0;

    println!("  Significant changes (>2sigma): {:.2}%", change_percentage);

    // Step 4: Trend Analysis
    println!("\n\nStep 4: Trend Analysis");
    println!("----------------------");

    println!("Computing trends over time series...");

    // Extract time series for a sample pixel
    let sample_x = width / 2;
    let sample_y = height / 2;

    let mut time_series = Vec::new();
    for raster in &series {
        time_series.push(raster.buffer.get_pixel(sample_x, sample_y)?);
    }

    println!("\nTime series at pixel ({}, {}):", sample_x, sample_y);
    for (i, value) in time_series.iter().enumerate() {
        println!("  Month {:2}: NDVI = {:.4}", i + 1, value);
    }

    // Compute trend (simple linear regression)
    let trend = linear_trend(&time_series);

    println!("\nLinear trend analysis:");
    println!("  Slope: {:.6} NDVI/month", trend.0);
    println!("  Intercept: {:.4}", trend.1);
    println!(
        "  Trend: {}",
        if trend.0 > 0.0 {
            "Increasing"
        } else {
            "Decreasing"
        }
    );

    // Step 5: Anomaly Detection
    println!("\n\nStep 5: Anomaly Detection");
    println!("-------------------------");

    println!("Detecting temporal anomalies (Z-score)...");

    let series_mean = time_series.iter().sum::<f64>() / time_series.len() as f64;
    let series_var = time_series
        .iter()
        .map(|v| (v - series_mean).powi(2))
        .sum::<f64>()
        / time_series.len() as f64;
    let series_std = series_var.sqrt();

    println!("\nAnomaly detection results (|Z-score| > 2.0):");
    let mut anomaly_count = 0;
    for (i, value) in time_series.iter().enumerate() {
        let z_score = if series_std > f64::EPSILON {
            (value - series_mean) / series_std
        } else {
            0.0
        };
        if z_score.abs() > 2.0 {
            println!(
                "  Month {:2}: NDVI = {:.4} (ANOMALY, z={:.2})",
                i + 1,
                value,
                z_score
            );
            anomaly_count += 1;
        }
    }

    println!(
        "\nTotal anomalies: {} out of {} months",
        anomaly_count,
        time_series.len()
    );

    // Step 6: Temporal Aggregation
    println!("\n\nStep 6: Temporal Aggregation");
    println!("-----------------------------");

    println!("Aggregating by season...");

    let winter = aggregate_mean(&[&series[11].buffer, &series[0].buffer, &series[1].buffer])?;
    let spring = aggregate_mean(&[&series[2].buffer, &series[3].buffer, &series[4].buffer])?;
    let summer = aggregate_mean(&[&series[5].buffer, &series[6].buffer, &series[7].buffer])?;
    let fall = aggregate_mean(&[&series[8].buffer, &series[9].buffer, &series[10].buffer])?;

    println!("\nSeasonal NDVI statistics:");

    for (name, buffer) in [
        ("Winter", &winter),
        ("Spring", &spring),
        ("Summer", &summer),
        ("Fall", &fall),
    ] {
        let stats = buffer.compute_statistics()?;
        println!(
            "  {}: mean = {:.4}, std = {:.4}",
            name, stats.mean, stats.std_dev
        );
    }

    save_raster(&winter, &temp_dir.join("ndvi_winter.tif"), &gt)?;
    save_raster(&spring, &temp_dir.join("ndvi_spring.tif"), &gt)?;
    save_raster(&summer, &temp_dir.join("ndvi_summer.tif"), &gt)?;
    save_raster(&fall, &temp_dir.join("ndvi_fall.tif"), &gt)?;

    // Summary
    println!("\n\n=== Tutorial Complete! ===");
    println!("\nOperations Covered:");
    println!("  1. Creating temporal raster collections");
    println!("  2. Computing temporal statistics (mean, std)");
    println!("  3. Change detection (difference, ratio)");
    println!("  4. Trend analysis (linear regression)");
    println!("  5. Anomaly detection (Z-score)");
    println!("  6. Temporal aggregation (seasonal)");

    println!("\nKey Points:");
    println!("  - Temporal analysis reveals patterns invisible in single images");
    println!("  - Multiple change detection methods provide different insights");
    println!("  - Trend analysis quantifies long-term changes");
    println!("  - Anomaly detection identifies unusual events");
    println!("  - Seasonal aggregation reduces noise");

    println!("\nOutput Files:");
    println!("  - ndvi_temporal_mean.tif / ndvi_temporal_std.tif");
    println!("  - ndvi_change_diff.tif / ndvi_change_ratio.tif");
    println!("  - ndvi_{{winter,spring,summer,fall}}.tif");

    Ok(())
}

/// Create synthetic seasonal NDVI data
fn create_seasonal_ndvi(width: u64, height: u64, month: i64) -> RasterBuffer {
    let mut buffer = RasterBuffer::zeros(width, height, RasterDataType::Float32);

    // Seasonal variation (peaks in summer)
    let seasonal_factor = ((month as f64 * std::f64::consts::PI / 6.0).sin() * 0.3 + 0.5).max(0.2);

    for y in 0..height {
        for x in 0..width {
            let dx = (x as f64) - (width as f64) / 2.0;
            let dy = (y as f64) - (height as f64) / 2.0;
            let dist = (dx * dx + dy * dy).sqrt() / (width.min(height) as f64 / 2.0);

            let base_ndvi = (1.0 - dist * 0.5).max(0.1);
            let ndvi = base_ndvi * seasonal_factor;
            let noise = ((x + y) as f64 + month as f64).sin() * 0.05;

            let _ = buffer.set_pixel(x, y, ndvi + noise);
        }
    }

    buffer
}

/// Compute the pixel-wise mean over a temporal series
fn temporal_mean(series: &[TemporalRaster]) -> RasterBuffer {
    let width = series[0].buffer.width();
    let height = series[0].buffer.height();
    let mut result = RasterBuffer::zeros(width, height, RasterDataType::Float32);

    for y in 0..height {
        for x in 0..width {
            let mut sum = 0.0;
            for raster in series {
                sum += raster.buffer.get_pixel(x, y).unwrap_or(0.0);
            }
            let _ = result.set_pixel(x, y, sum / series.len() as f64);
        }
    }

    result
}

/// Compute the pixel-wise standard deviation over a temporal series
fn temporal_std(
    series: &[TemporalRaster],
    mean: &RasterBuffer,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let width = mean.width();
    let height = mean.height();
    let mut result = RasterBuffer::zeros(width, height, RasterDataType::Float32);

    for y in 0..height {
        for x in 0..width {
            let mean_value = mean.get_pixel(x, y)?;
            let mut sum_sq = 0.0;
            for raster in series {
                let value = raster.buffer.get_pixel(x, y)?;
                sum_sq += (value - mean_value).powi(2);
            }
            let variance = sum_sq / series.len() as f64;
            result.set_pixel(x, y, variance.sqrt())?;
        }
    }

    Ok(result)
}

/// Subtract two buffers
fn subtract_buffers(
    a: &RasterBuffer,
    b: &RasterBuffer,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let mut result = a.clone();

    for y in 0..result.height() {
        for x in 0..result.width() {
            let val_a = a.get_pixel(x, y)?;
            let val_b = b.get_pixel(x, y)?;
            result.set_pixel(x, y, val_a - val_b)?;
        }
    }

    Ok(result)
}

/// Divide two buffers
fn divide_buffers(
    a: &RasterBuffer,
    b: &RasterBuffer,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let mut result = a.clone();

    for y in 0..result.height() {
        for x in 0..result.width() {
            let val_a = a.get_pixel(x, y)?;
            let val_b = b.get_pixel(x, y)?;
            let ratio = if val_b.abs() > 1e-10 {
                val_a / val_b
            } else {
                0.0
            };
            result.set_pixel(x, y, ratio)?;
        }
    }

    Ok(result)
}

/// Aggregate multiple buffers by mean
fn aggregate_mean(buffers: &[&RasterBuffer]) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    if buffers.is_empty() {
        return Err("No buffers to aggregate".into());
    }

    let width = buffers[0].width();
    let height = buffers[0].height();
    let mut result = RasterBuffer::zeros(width, height, RasterDataType::Float32);

    for y in 0..height {
        for x in 0..width {
            let mut sum = 0.0;
            for buffer in buffers {
                sum += buffer.get_pixel(x, y)?;
            }
            result.set_pixel(x, y, sum / buffers.len() as f64)?;
        }
    }

    Ok(result)
}

/// Compute a simple ordinary-least-squares linear trend (slope, intercept)
fn linear_trend(values: &[f64]) -> (f64, f64) {
    let n = values.len() as f64;
    let xs: Vec<f64> = (0..values.len()).map(|i| i as f64).collect();

    let x_mean = xs.iter().sum::<f64>() / n;
    let y_mean = values.iter().sum::<f64>() / n;

    let mut numerator = 0.0;
    let mut denominator = 0.0;
    for (x, y) in xs.iter().zip(values.iter()) {
        numerator += (x - x_mean) * (y - y_mean);
        denominator += (x - x_mean).powi(2);
    }

    let slope = if denominator.abs() > f64::EPSILON {
        numerator / denominator
    } else {
        0.0
    };
    let intercept = y_mean - slope * x_mean;

    (slope, intercept)
}

/// Save a raster buffer to GeoTIFF
fn save_raster(
    buffer: &RasterBuffer,
    path: &std::path::Path,
    geo_transform: &GeoTransform,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = WriterConfig::new(buffer.width(), buffer.height(), 1, buffer.data_type())
        .with_compression(Compression::Lzw)
        .with_tile_size(256, 256)
        .with_geo_transform(*geo_transform)
        .with_epsg_code(4326);

    let mut writer = GeoTiffWriter::create(path, config, GeoTiffWriterOptions::default())?;
    writer.write(buffer.as_bytes())?;

    Ok(())
}
