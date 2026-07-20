//! Time-Series Analysis Example
//!
//! This example demonstrates temporal raster analysis built on the real
//! `oxigdal-temporal` crate:
//! - Building a `TimeSeriesRaster` from synthetic monthly NDVI observations
//! - Trend analysis (Mann-Kendall)
//! - Anomaly detection (Z-score)
//! - Temporal compositing via `RasterStack` (mean/median/std)
//! - Exporting analysis results as GeoTIFFs
//!
//! Run with:
//! ```bash
//! cargo run --example timeseries_analysis
//! ```

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::{BoundingBox, GeoTransform, RasterDataType};
use oxigdal_geotiff::tiff::Compression;
use oxigdal_geotiff::writer::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};
use oxigdal_temporal::analysis::anomaly::{AnomalyDetector, AnomalyMethod};
use oxigdal_temporal::analysis::trend::{TrendAnalyzer, TrendMethod};
use oxigdal_temporal::{RasterStack, TemporalMetadata, TimeSeriesRaster};
use scirs2_core::ndarray::Array3;
use std::path::Path;
use tracing::info;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("timeseries_analysis=info")
        .init();

    info!("Starting Time-Series Analysis");

    let output_dir = std::env::temp_dir().join("timeseries_analysis_output");
    std::fs::create_dir_all(&output_dir)?;

    let height = 64usize;
    let width = 64usize;

    // Step 1: Build a synthetic monthly NDVI time series
    info!("Step 1: Building synthetic monthly NDVI time series");

    let mut series = TimeSeriesRaster::with_shape(height, width, 1);

    for month in 1..=12u32 {
        let acquisition_date = NaiveDate::from_ymd_opt(2023, month, 15).ok_or("bad date")?;
        let timestamp: DateTime<Utc> = Utc
            .with_ymd_and_hms(2023, month, 15, 0, 0, 0)
            .single()
            .ok_or("bad timestamp")?;

        let data = create_seasonal_ndvi_array(height, width, month);

        let mut metadata = TemporalMetadata::new(timestamp, acquisition_date);
        metadata.sensor = Some("Simulated".to_string());
        metadata.cloud_cover = Some(5.0);

        series.add_raster(metadata, data)?;
        info!(
            "  Added observation: {} (month {})",
            acquisition_date, month
        );
    }

    info!("  Time series length: {}", series.len());

    // Step 2: Trend analysis (Mann-Kendall)
    info!("\nStep 2: Trend Analysis (Mann-Kendall)");

    let trend = TrendAnalyzer::analyze(&series, TrendMethod::MannKendall)?;

    let (slope_min, slope_max, slope_mean) = array3_stats(&trend.slope);
    info!(
        "  Slope range: [{:.6}, {:.6}], mean: {:.6}",
        slope_min, slope_max, slope_mean
    );

    let positive = trend.direction.iter().filter(|&&d| d > 0).count();
    let negative = trend.direction.iter().filter(|&&d| d < 0).count();
    let stable = trend.direction.len() - positive - negative;
    info!(
        "  Direction: {} increasing, {} decreasing, {} stable",
        positive, negative, stable
    );

    // Step 3: Anomaly detection (Z-score)
    info!("\nStep 3: Anomaly Detection (Z-score)");

    let anomalies = AnomalyDetector::detect(&series, AnomalyMethod::ZScore, 2.0)?;
    let anomaly_count: usize = anomalies.mask.iter().map(|&v| v as usize).sum();
    let total_cells = anomalies.mask.len();
    info!(
        "  Anomalous cells: {} / {} ({:.2}%)",
        anomaly_count,
        total_cells,
        anomaly_count as f64 / total_cells as f64 * 100.0
    );
    info!("  Detection threshold: {:.2}", anomalies.threshold);

    // Step 4: Temporal compositing via RasterStack
    info!("\nStep 4: Temporal Compositing");

    let stack = RasterStack::from_timeseries(&series)?;
    let (n_time, n_bands, stack_h, stack_w) = stack.shape();
    info!(
        "  Stack shape: time={}, bands={}, height={}, width={}",
        n_time, n_bands, stack_h, stack_w
    );

    let mean_composite = stack.mean_temporal()?;
    let median_composite = stack.median_temporal()?;
    let std_composite = stack.std_temporal()?;

    let (mean_min, mean_max, mean_mean) = array3_stats(&mean_composite);
    info!(
        "  Mean composite: range [{:.4}, {:.4}], mean {:.4}",
        mean_min, mean_max, mean_mean
    );

    let (std_min, std_max, std_mean) = array3_stats(&std_composite);
    info!(
        "  Std composite: range [{:.4}, {:.4}], mean {:.4}",
        std_min, std_max, std_mean
    );

    // Step 5: Export results
    info!("\nStep 5: Exporting results");

    let bbox = BoundingBox::new(-10.0, 40.0, -9.0, 41.0)?;
    let gt = GeoTransform::from_bounds(&bbox, width as u64, height as u64)?;

    save_array3_band(&trend.slope, 0, &output_dir.join("trend_slope.tif"), &gt)?;
    save_array3_band(
        &mean_composite,
        0,
        &output_dir.join("mean_composite.tif"),
        &gt,
    )?;
    save_array3_band(
        &median_composite,
        0,
        &output_dir.join("median_composite.tif"),
        &gt,
    )?;
    save_array3_band(
        &std_composite,
        0,
        &output_dir.join("std_composite.tif"),
        &gt,
    )?;

    info!("  Wrote 4 output rasters to {:?}", output_dir);

    // Step 6: Report
    info!("\nStep 6: Generating summary report");

    let report = AnalysisReport {
        observations: series.len(),
        spatial_extent: (height, width),
        mean_trend_slope: slope_mean,
        increasing_pixels: positive,
        decreasing_pixels: negative,
        anomalous_cells: anomaly_count,
        anomaly_threshold: anomalies.threshold,
    };

    let report_path = output_dir.join("analysis_report.json");
    std::fs::write(&report_path, serde_json::to_string_pretty(&report)?)?;
    info!("  Report saved to: {}", report_path.display());

    info!("");
    info!("=== Analysis Summary ===");
    info!("  Observations: {}", report.observations);
    info!(
        "  Spatial extent: {}x{} pixels",
        report.spatial_extent.0, report.spatial_extent.1
    );
    info!(
        "  Mean trend slope: {:.6} NDVI/month",
        report.mean_trend_slope
    );
    info!("  Anomalous cells: {}", report.anomalous_cells);
    info!("");
    info!("Time-series analysis completed successfully!");

    Ok(())
}

#[derive(Debug, serde::Serialize)]
struct AnalysisReport {
    observations: usize,
    spatial_extent: (usize, usize),
    mean_trend_slope: f64,
    increasing_pixels: usize,
    decreasing_pixels: usize,
    anomalous_cells: usize,
    anomaly_threshold: f64,
}

/// Create a synthetic single-band NDVI array (height, width, bands=1) for a given month
fn create_seasonal_ndvi_array(height: usize, width: usize, month: u32) -> Array3<f64> {
    let seasonal_factor = ((month as f64 * std::f64::consts::PI / 6.0).sin() * 0.3 + 0.5).max(0.2);

    Array3::from_shape_fn((height, width, 1), |(y, x, _)| {
        let dx = (x as f64) - (width as f64) / 2.0;
        let dy = (y as f64) - (height as f64) / 2.0;
        let dist = (dx * dx + dy * dy).sqrt() / (width.min(height) as f64 / 2.0);

        let base_ndvi = (1.0 - dist * 0.5).max(0.1);
        let noise = ((x + y) as f64 + f64::from(month)).sin() * 0.05;

        base_ndvi * seasonal_factor + noise
    })
}

fn array3_stats(arr: &Array3<f64>) -> (f64, f64, f64) {
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    let mut sum = 0.0;
    let mut count = 0usize;

    for &v in arr.iter() {
        if v.is_finite() {
            min = min.min(v);
            max = max.max(v);
            sum += v;
            count += 1;
        }
    }

    (min, max, sum / count.max(1) as f64)
}

/// Save a single band of a (height, width, bands) array as a GeoTIFF
fn save_array3_band(
    arr: &Array3<f64>,
    band: usize,
    path: &Path,
    gt: &GeoTransform,
) -> Result<(), Box<dyn std::error::Error>> {
    let shape = arr.shape();
    let (height, width) = (shape[0], shape[1]);

    let mut buffer = RasterBuffer::zeros(width as u64, height as u64, RasterDataType::Float32);

    for y in 0..height {
        for x in 0..width {
            buffer.set_pixel(x as u64, y as u64, arr[[y, x, band]])?;
        }
    }

    let config = WriterConfig::new(buffer.width(), buffer.height(), 1, buffer.data_type())
        .with_compression(Compression::Lzw)
        .with_tile_size(64, 64)
        .with_geo_transform(*gt)
        .with_epsg_code(4326);

    let mut writer = GeoTiffWriter::create(path, config, GeoTiffWriterOptions::default())?;
    writer.write(buffer.as_bytes())?;

    Ok(())
}
