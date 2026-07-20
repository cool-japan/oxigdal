//! ML Inference Pipeline
//!
//! This example demonstrates a tiled machine-learning inference pipeline built
//! entirely on `oxigdal-ml`'s raster-native pre/post-processing utilities:
//! - Tiling a large raster for batch inference (`tile_raster`)
//! - Preprocessing (normalization) per tile
//! - Running "inference" (simulated probability maps) per tile
//! - Segmentation post-processing + GeoJSON export
//! - Object detection with Non-Maximum Suppression + georeferenced GeoJSON export
//! - Performance profiling and a JSON report
//!
//! Run with:
//! ```bash
//! cargo run --example ml_inference --release
//! ```

use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::{BoundingBox, GeoTransform, RasterDataType};
use oxigdal_ml::detection::{
    BoundingBox as MlBoundingBox, Detection, NmsConfig, georeference_detections,
    non_maximum_suppression,
};
use oxigdal_ml::postprocessing::{export_detections_geojson, export_segmentation_geojson};
use oxigdal_ml::preprocessing::{NormalizationParams, Tile, TileConfig, normalize, tile_raster};
use oxigdal_ml::segmentation::probability_to_mask;
use std::path::PathBuf;
use std::time::Instant;
use tracing::info;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("ml_inference=info")
        .init();

    info!("Starting ML Inference Pipeline");

    let output_dir = std::env::temp_dir().join("ml_inference_output");
    std::fs::create_dir_all(&output_dir)?;

    // Step 1: Prepare a synthetic "satellite image"
    info!("Step 1: Preparing input image");

    let width = 1024u64;
    let height = 1024u64;
    let bbox = BoundingBox::new(-10.0, 40.0, -9.0, 41.0)?;
    let geo_transform = GeoTransform::from_bounds(&bbox, width, height)?;

    let image = create_synthetic_image(width, height);
    info!("  Image dimensions: {}x{}", width, height);

    // Step 2: Tile the image for batch inference
    info!("Step 2: Tiling the image");

    let tile_config = TileConfig {
        tile_width: 256,
        tile_height: 256,
        overlap: 32,
        ..Default::default()
    };

    let tiles: Vec<Tile> = tile_raster(&image, &tile_config)?;
    info!(
        "  Image split into {} tiles ({}x{}, overlap {})",
        tiles.len(),
        tile_config.tile_width,
        tile_config.tile_height,
        tile_config.overlap
    );

    // Step 3: Run "inference" tile-by-tile
    info!("Step 3: Running tiled inference");

    let start_time = Instant::now();
    let norm_params = NormalizationParams::from_range(0.0, 255.0);

    let mut tile_means = Vec::with_capacity(tiles.len());
    for (idx, tile) in tiles.iter().enumerate() {
        let normalized = normalize(&tile.buffer, &norm_params, 0)?;
        let stats = normalized.compute_statistics()?;
        tile_means.push(stats.mean);

        if idx % 8 == 0 {
            info!("  Processed tile {}/{}", idx + 1, tiles.len());
        }
    }

    let inference_time = start_time.elapsed();
    let avg_tile_ms = inference_time.as_secs_f64() * 1000.0 / tiles.len().max(1) as f64;

    info!(
        "  Inference completed in {:.3}s",
        inference_time.as_secs_f64()
    );
    info!("  Average time per tile: {:.3}ms", avg_tile_ms);
    info!(
        "  Throughput: {:.2} tiles/sec",
        tiles.len() as f64 / inference_time.as_secs_f64().max(1e-9)
    );

    // Step 4: Segmentation post-processing
    info!("Step 4: Semantic segmentation");

    let segmentation_probs = create_segmentation_probs(width, height);
    let mask = probability_to_mask(&segmentation_probs, 2, 0.5)?;

    let seg_path = output_dir.join("segmentation.geojson");
    export_segmentation_geojson(&mask, &seg_path, 4.0)?;
    info!(
        "  Segmentation polygons exported to: {}",
        seg_path.display()
    );

    // Step 5: Object detection with NMS
    info!("Step 5: Object detection");

    let raw_detections = simulate_detections(width, height);
    info!("  Raw detections: {}", raw_detections.len());

    let nms_config = NmsConfig {
        iou_threshold: 0.4,
        confidence_threshold: 0.3,
        ..Default::default()
    };
    let filtered = non_maximum_suppression(&raw_detections, &nms_config)?;
    info!("  Detections after NMS: {}", filtered.len());

    let geo_detections = georeference_detections(&filtered, &geo_transform)?;
    let det_path = output_dir.join("detections.geojson");
    export_detections_geojson(&geo_detections, &det_path)?;
    info!(
        "  Georeferenced detections exported to: {}",
        det_path.display()
    );

    // Step 6: Performance report
    info!("Step 6: Generating performance report");

    let report = PerformanceReport {
        input_image_size: (width, height),
        tile_size: (tile_config.tile_width, tile_config.tile_height),
        num_tiles: tiles.len(),
        total_inference_time_secs: inference_time.as_secs_f64(),
        avg_tile_time_ms: avg_tile_ms,
        throughput_tiles_per_sec: tiles.len() as f64 / inference_time.as_secs_f64().max(1e-9),
        segmentation_output: seg_path.clone(),
        detections_output: det_path.clone(),
        num_detections_after_nms: filtered.len(),
        mean_of_tile_means: tile_means.iter().sum::<f64>() / tile_means.len().max(1) as f64,
    };

    let report_path = output_dir.join("performance_report.json");
    std::fs::write(&report_path, serde_json::to_string_pretty(&report)?)?;
    info!("  Report saved to: {}", report_path.display());

    // Summary
    info!("");
    info!("=== Inference Summary ===");
    info!("  Input: {}x{} pixels", width, height);
    info!("  Tiles processed: {}", tiles.len());
    info!("  Total time: {:.3}s", inference_time.as_secs_f64());
    info!(
        "  Throughput: {:.2} tiles/sec",
        tiles.len() as f64 / inference_time.as_secs_f64().max(1e-9)
    );
    info!("  Output directory: {}", output_dir.display());
    info!("");
    info!("ML inference pipeline completed successfully!");

    Ok(())
}

#[derive(Debug, serde::Serialize)]
struct PerformanceReport {
    input_image_size: (u64, u64),
    tile_size: (usize, usize),
    num_tiles: usize,
    total_inference_time_secs: f64,
    avg_tile_time_ms: f64,
    throughput_tiles_per_sec: f64,
    segmentation_output: PathBuf,
    detections_output: PathBuf,
    num_detections_after_nms: usize,
    mean_of_tile_means: f64,
}

/// Create a synthetic single-band "satellite image"
fn create_synthetic_image(width: u64, height: u64) -> RasterBuffer {
    let mut buffer = RasterBuffer::zeros(width, height, RasterDataType::Float32);

    for y in 0..height {
        for x in 0..width {
            let nx = x as f64 / width as f64;
            let ny = y as f64 / height as f64;
            let value = ((nx * 20.0).sin() + (ny * 20.0).cos()).abs() * 127.0 + 64.0;
            let _ = buffer.set_pixel(x, y, value);
        }
    }

    buffer
}

/// Create a synthetic segmentation probability map (0..1)
fn create_segmentation_probs(width: u64, height: u64) -> RasterBuffer {
    let mut buffer = RasterBuffer::zeros(width, height, RasterDataType::Float32);

    for y in 0..height {
        for x in 0..width {
            let dx = (x as f64) / (width as f64) - 0.5;
            let dy = (y as f64) / (height as f64) - 0.5;
            let dist = (dx * dx + dy * dy).sqrt();
            let prob = (1.0 - dist * 1.4).clamp(0.0, 1.0);
            let _ = buffer.set_pixel(x, y, prob);
        }
    }

    buffer
}

/// Simulate raw object-detection output (some overlapping boxes for NMS to prune)
fn simulate_detections(width: u64, height: u64) -> Vec<Detection> {
    let mut detections = Vec::new();

    for i in 0..12u32 {
        let x = (i as f32 * 83.0) % width as f32;
        let y = (i as f32 * 131.0) % height as f32;

        detections.push(Detection {
            bbox: MlBoundingBox::new(x, y, 40.0, 40.0),
            class_id: (i % 3) as usize,
            class_label: None,
            confidence: 0.4 + (i as f32 % 5.0) * 0.1,
            attributes: Default::default(),
        });

        // Add an overlapping duplicate for some detections to exercise NMS
        if i % 2 == 0 {
            detections.push(Detection {
                bbox: MlBoundingBox::new(x + 5.0, y + 5.0, 40.0, 40.0),
                class_id: (i % 3) as usize,
                class_label: None,
                confidence: 0.35 + (i as f32 % 5.0) * 0.1,
                attributes: Default::default(),
            });
        }
    }

    detections
}
