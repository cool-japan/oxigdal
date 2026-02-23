//! Tutorial 05: Temporal Analysis
//!
//! This tutorial demonstrates time-series analysis of geospatial data:
//! - Loading multi-temporal datasets
//! - Change detection algorithms
//! - Trend analysis
//! - Anomaly detection
//! - Time-series visualization
//! - Temporal aggregation
//!
//! Run with:
//! ```bash
//! cargo run --example 05_temporal_analysis
//! ```

use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use oxigdal_analytics::change::{ChangeDetectionMethod, ChangeDetector};
use oxigdal_analytics::timeseries::{AnomalyDetector, TrendAnalyzer};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::{BoundingBox, GeoTransform, RasterDataType};
use oxigdal_geotiff::writer::{CompressionType, GeoTiffWriter, GeoTiffWriterOptions};
use oxigdal_temporal::{TemporalCollection, TemporalRaster};
use std::env;
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Tutorial 05: Temporal Analysis ===\n");

    let temp_dir = env::temp_dir();

    // Step 1: Creating a Temporal Dataset
    println!("Step 1: Creating Temporal Dataset");
    println!("----------------------------------");

    // Simulate NDVI time series for a year (monthly)
    let start_date = NaiveDate::from_ymd_opt(2023, 1, 1).ok_or("Invalid date")?;
    let width = 256;
    let height = 256;

    let mut temporal_collection = TemporalCollection::new("NDVI Time Series");

    println!("Creating monthly NDVI data for 2023...");

    for month in 0..12 {
        let date = start_date + Duration::days(month * 30);
        let datetime: DateTime<Utc> = DateTime::from_naive_utc_and_offset(
            date.and_hms_opt(0, 0, 0).ok_or("Invalid time")?,
            Utc
        );

        // Create synthetic NDVI data with seasonal variation
        let buffer = create_seasonal_ndvi(width, height, month)?;

        let temporal_raster = TemporalRaster {
            timestamp: datetime,
            buffer,
            metadata: serde_json::json!({
                "sensor": "Simulated",
                "product": "NDVI",
                "month": month + 1,
            }),
        };

        temporal_collection.add_raster(temporal_raster)?;

        println!("  Added: {} (month {})", date, month + 1);
    }

    println!("\nCollection created:");
    println!("  Name: {}", temporal_collection.name());
    println!("  Time steps: {}", temporal_collection.count());
    println!("  Start: {}", temporal_collection.start_time()?);
    println!("  End: {}", temporal_collection.end_time()?);
    println!("  Temporal resolution: ~30 days");

    // Step 2: Temporal Statistics
    println!("\n\nStep 2: Temporal Statistics");
    println!("----------------------------");

    // Compute pixel-wise statistics over time
    println!("Computing temporal statistics (per-pixel)...");

    let mean_buffer = temporal_collection.temporal_mean()?;
    let std_buffer = temporal_collection.temporal_std()?;
    let min_buffer = temporal_collection.temporal_min()?;
    let max_buffer = temporal_collection.temporal_max()?;

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

    // Save temporal statistics
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

    // Compare two time periods
    let t1_buffer = temporal_collection.get_raster(0)?.buffer.clone();
    let t2_buffer = temporal_collection.get_raster(6)?.buffer.clone();

    println!("Detecting changes between January and July 2023...");

    // Method 1: Simple difference
    println!("\nMethod 1: Simple Difference");
    let diff_buffer = subtract_buffers(&t2_buffer, &t1_buffer)?;
    let diff_stats = diff_buffer.compute_statistics()?;

    println!("  Change statistics:");
    println!("    Min change: {:.4}", diff_stats.min);
    println!("    Max change: {:.4}", diff_stats.max);
    println!("    Mean change: {:.4}", diff_stats.mean);

    save_raster(&diff_buffer, &temp_dir.join("ndvi_change_diff.tif"), &gt)?;

    // Method 2: Ratio
    println!("\nMethod 2: Ratio");
    let ratio_buffer = divide_buffers(&t2_buffer, &t1_buffer)?;
    let ratio_stats = ratio_buffer.compute_statistics()?;

    println!("  Ratio statistics:");
    println!("    Min: {:.4}", ratio_stats.min);
    println!("    Max: {:.4}", ratio_stats.max);
    println!("    Mean: {:.4}", ratio_stats.mean);

    save_raster(&ratio_buffer, &temp_dir.join("ndvi_change_ratio.tif"), &gt)?;

    // Method 3: Advanced change detection
    println!("\nMethod 3: Statistical Change Detection");
    let detector = ChangeDetector::new(ChangeDetectionMethod::PrincipalComponents);
    let change_map = detector.detect(&t1_buffer, &t2_buffer)?;

    let change_stats = change_map.compute_statistics()?;
    println!("  Change magnitude:");
    println!("    Min: {:.4}", change_stats.min);
    println!("    Max: {:.4}", change_stats.max);
    println!("    Mean: {:.4}", change_stats.mean);

    save_raster(&change_map, &temp_dir.join("ndvi_change_pca.tif"), &gt)?;

    // Count significant changes (threshold at 2 std dev)
    let threshold = mean_stats.mean + 2.0 * std_stats.mean;
    let mut significant_pixels = 0;

    for y in 0..change_map.height() {
        for x in 0..change_map.width() {
            let value = change_map.get_pixel(x, y)?;
            if value.abs() > threshold {
                significant_pixels += 1;
            }
        }
    }

    let total_pixels = change_map.width() * change_map.height();
    let change_percentage = (significant_pixels as f64 / total_pixels as f64) * 100.0;

    println!("  Significant changes (>2σ): {:.2}%", change_percentage);

    // Step 4: Trend Analysis
    println!("\n\nStep 4: Trend Analysis");
    println!("----------------------");

    println!("Computing trends over time series...");

    let trend_analyzer = TrendAnalyzer::new();

    // Extract time series for a sample pixel
    let sample_x = width / 2;
    let sample_y = height / 2;

    let mut time_series = Vec::new();
    for i in 0..temporal_collection.count() {
        let raster = temporal_collection.get_raster(i)?;
        let value = raster.buffer.get_pixel(sample_x, sample_y)?;
        time_series.push(value);
    }

    println!("\nTime series at pixel ({}, {}):", sample_x, sample_y);
    for (i, value) in time_series.iter().enumerate() {
        println!("  Month {:2}: NDVI = {:.4}", i + 1, value);
    }

    // Compute trend
    let trend = trend_analyzer.linear_trend(&time_series)?;

    println!("\nLinear trend analysis:");
    println!("  Slope: {:.6} NDVI/month", trend.slope);
    println!("  Intercept: {:.4}", trend.intercept);
    println!("  R²: {:.4}", trend.r_squared);
    println!("  Trend: {}", if trend.slope > 0.0 { "Increasing" } else { "Decreasing" });

    // Compute trend for all pixels
    println!("\nComputing spatial trend map...");
    let trend_map = temporal_collection.compute_trend_map()?;

    let trend_stats = trend_map.compute_statistics()?;
    println!("  Trend slope statistics:");
    println!("    Min: {:.6}", trend_stats.min);
    println!("    Max: {:.6}", trend_stats.max);
    println!("    Mean: {:.6}", trend_stats.mean);

    save_raster(&trend_map, &temp_dir.join("ndvi_trend.tif"), &gt)?;

    // Step 5: Anomaly Detection
    println!("\n\nStep 5: Anomaly Detection");
    println!("-------------------------");

    println!("Detecting temporal anomalies...");

    let anomaly_detector = AnomalyDetector::new();

    // Detect anomalies in the time series
    let anomalies = anomaly_detector.detect_z_score(&time_series, 2.0)?;

    println!("\nAnomaly detection results (Z-score > 2.0):");
    for (i, is_anomaly) in anomalies.iter().enumerate() {
        if *is_anomaly {
            println!("  Month {:2}: NDVI = {:.4} (ANOMALY)", i + 1, time_series[i]);
        }
    }

    let anomaly_count = anomalies.iter().filter(|&&x| x).count();
    println!("\nTotal anomalies: {} out of {} months", anomaly_count, time_series.len());

    // Create anomaly map
    println!("\nComputing spatial anomaly map...");
    let anomaly_map = temporal_collection.detect_anomalies(2.0)?;

    let anomaly_stats = anomaly_map.compute_statistics()?;
    println!("  Anomaly count per pixel:");
    println!("    Min: {:.0}", anomaly_stats.min);
    println!("    Max: {:.0}", anomaly_stats.max);
    println!("    Mean: {:.2}", anomaly_stats.mean);

    save_raster(&anomaly_map, &temp_dir.join("ndvi_anomalies.tif"), &gt)?;

    // Step 6: Temporal Aggregation
    println!("\n\nStep 6: Temporal Aggregation");
    println!("-----------------------------");

    // Aggregate by season
    println!("Aggregating by season...");

    let winter = temporal_collection.aggregate_by_months(&[12, 1, 2], |buffers| {
        aggregate_mean(buffers)
    })?;

    let spring = temporal_collection.aggregate_by_months(&[3, 4, 5], |buffers| {
        aggregate_mean(buffers)
    })?;

    let summer = temporal_collection.aggregate_by_months(&[6, 7, 8], |buffers| {
        aggregate_mean(buffers)
    })?;

    let fall = temporal_collection.aggregate_by_months(&[9, 10, 11], |buffers| {
        aggregate_mean(buffers)
    })?;

    println!("\nSeasonal NDVI statistics:");

    let seasons = vec![
        ("Winter", &winter),
        ("Spring", &spring),
        ("Summer", &summer),
        ("Fall", &fall),
    ];

    for (name, buffer) in &seasons {
        let stats = buffer.compute_statistics()?;
        println!("  {}: mean = {:.4}, std = {:.4}", name, stats.mean, stats.std_dev);
    }

    save_raster(&winter, &temp_dir.join("ndvi_winter.tif"), &gt)?;
    save_raster(&spring, &temp_dir.join("ndvi_spring.tif"), &gt)?;
    save_raster(&summer, &temp_dir.join("ndvi_summer.tif"), &gt)?;
    save_raster(&fall, &temp_dir.join("ndvi_fall.tif"), &gt)?;

    // Step 7: Phenology Analysis
    println!("\n\nStep 7: Phenology Analysis");
    println!("--------------------------");

    println!("Extracting phenological metrics...");

    // Find peak NDVI timing
    let peak_timing = temporal_collection.find_peak_timing()?;

    println!("\nPhenological metrics:");
    println!("  Peak NDVI occurs on average in month: {}", peak_timing.mean_month);
    println!("  Standard deviation: {:.2} months", peak_timing.std_months);

    // Find growing season length
    let growing_season = temporal_collection.compute_growing_season(0.3)?; // NDVI > 0.3

    let gs_stats = growing_season.compute_statistics()?;
    println!("\nGrowing season length:");
    println!("  Min: {:.0} months", gs_stats.min);
    println!("  Max: {:.0} months", gs_stats.max);
    println!("  Mean: {:.2} months", gs_stats.mean);

    save_raster(&growing_season, &temp_dir.join("growing_season_length.tif"), &gt)?;

    // Summary
    println!("\n\n=== Tutorial Complete! ===");
    println!("\nOperations Covered:");
    println!("  1. Creating temporal raster collections");
    println!("  2. Computing temporal statistics (mean, std, min, max)");
    println!("  3. Change detection (difference, ratio, PCA)");
    println!("  4. Trend analysis (linear regression)");
    println!("  5. Anomaly detection (Z-score)");
    println!("  6. Temporal aggregation (seasonal)");
    println!("  7. Phenology analysis (peak timing, growing season)");

    println!("\nKey Points:");
    println!("  - Temporal analysis reveals patterns invisible in single images");
    println!("  - Multiple change detection methods provide different insights");
    println!("  - Trend analysis quantifies long-term changes");
    println!("  - Anomaly detection identifies unusual events");
    println!("  - Seasonal aggregation reduces noise");

    println!("\nOutput Files:");
    println!("  - ndvi_temporal_mean.tif");
    println!("  - ndvi_temporal_std.tif");
    println!("  - ndvi_change_*.tif");
    println!("  - ndvi_trend.tif");
    println!("  - ndvi_anomalies.tif");
    println!("  - ndvi_*season.tif");
    println!("  - growing_season_length.tif");

    println!("\nNext Tutorial:");
    println!("  - Try tutorial 06 for ML inference");

    Ok(())
}

/// Create synthetic seasonal NDVI data
fn create_seasonal_ndvi(
    width: u32,
    height: u32,
    month: i64,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let mut buffer = RasterBuffer::zeros(width, height, RasterDataType::Float32);

    // Seasonal variation (peaks in summer)
    let seasonal_factor = ((month as f64 * std::f64::consts::PI / 6.0).sin() * 0.3 + 0.5).max(0.2);

    for y in 0..height {
        for x in 0..width {
            // Spatial variation (decreases from center)
            let dx = (x as f64) - (width as f64) / 2.0;
            let dy = (y as f64) - (height as f64) / 2.0;
            let dist = (dx * dx + dy * dy).sqrt() / (width.min(height) as f64 / 2.0);

            // Base NDVI decreases with distance from center
            let base_ndvi = (1.0 - dist * 0.5).max(0.1);

            // Apply seasonal variation
            let ndvi = base_ndvi * seasonal_factor;

            // Add some noise
            let noise = (((x + y + month as u32) as f64).sin() * 0.05);

            buffer.set_pixel(x, y, ndvi + noise)?;
        }
    }

    Ok(buffer)
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
            let ratio = if val_b.abs() > 1e-10 { val_a / val_b } else { 0.0 };
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

/// Save a raster buffer to GeoTIFF
fn save_raster(
    buffer: &RasterBuffer,
    path: &std::path::Path,
    geo_transform: &GeoTransform,
) -> Result<(), Box<dyn std::error::Error>> {
    let options = GeoTiffWriterOptions {
        geo_transform: Some(*geo_transform),
        epsg_code: Some(4326),
        tile_width: Some(256),
        tile_height: Some(256),
        compression: Some(CompressionType::Lzw),
        ..Default::default()
    };

    let file = File::create(path)?;
    let writer = GeoTiffWriter::new(file, options)?;
    writer.write_buffer(buffer)?;

    Ok(())
}
