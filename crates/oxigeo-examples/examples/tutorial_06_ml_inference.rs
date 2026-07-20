//! Tutorial 06: Machine Learning Inference
//!
//! This tutorial demonstrates ML inference on geospatial data using the
//! `oxigeo-ml` crate's raster-native pre/post-processing utilities:
//! - Preprocessing raster data for ML (normalize, resize)
//! - Running "inference" (here: simulated probability maps)
//! - Classification, segmentation, and object detection post-processing
//! - Batch tiling
//!
//! Run with:
//! ```bash
//! cargo run --example tutorial_06_ml_inference
//! ```

use oxigeo_core::buffer::RasterBuffer;
use oxigeo_core::types::{BoundingBox, GeoTransform, RasterDataType};
use oxigeo_geotiff::tiff::Compression;
use oxigeo_geotiff::writer::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};
use oxigeo_ml::classification::classify_single_label;
use oxigeo_ml::detection::{
    BoundingBox as MlBoundingBox, Detection, NmsConfig, non_maximum_suppression,
};
use oxigeo_ml::preprocessing::{NormalizationParams, normalize, resize_nearest};
use oxigeo_ml::segmentation::probability_to_mask;
use std::env;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Tutorial 06: Machine Learning Inference ===\n");

    let temp_dir = env::temp_dir();

    // Step 1: Preparing Input Data
    println!("Step 1: Preparing Input Data");
    println!("-----------------------------");

    let width = 256u64;
    let height = 256u64;

    println!("Creating synthetic RGB satellite image...");
    let red_band = create_sample_band(width, height, 0);

    println!("  Image size: {}x{}", width, height);

    // Step 2: Image Preprocessing
    println!("\n\nStep 2: Image Preprocessing for ML");
    println!("-----------------------------------");

    println!("1. Normalization:");
    let normalized_red = normalize(&red_band, &NormalizationParams::from_range(0.0, 255.0), 0)?;
    let norm_stats = normalized_red.compute_statistics()?;
    println!(
        "   Red band range: [{:.4}, {:.4}]",
        norm_stats.min, norm_stats.max
    );

    println!("\n2. Resizing:");
    let model_input_size = 128;
    let resized_red = resize_nearest(&normalized_red, model_input_size, model_input_size)?;
    println!(
        "   Resized from {}x{} to {}x{}",
        width, height, model_input_size, model_input_size
    );

    // Step 3: Classification (Land Cover)
    println!("\n\nStep 3: Image Classification");
    println!("-----------------------------");

    println!("Model: Land cover classification (simulated probability map)");
    let class_probs = resized_red.clone();
    let labels: Vec<String> = ["Water", "Forest", "Urban", "Agriculture", "Barren"]
        .iter()
        .map(|s| s.to_string())
        .collect();

    let result = classify_single_label(&class_probs, Some(&labels), 0.0)?;
    println!(
        "  Predicted class id: {} ({:?}), confidence: {:.2}%",
        result.class_id,
        result.class_label,
        result.confidence * 100.0
    );

    // Step 4: Semantic Segmentation
    println!("\n\nStep 4: Semantic Segmentation");
    println!("------------------------------");

    println!("Model: Semantic segmentation (simulated foreground/background)");
    let segmentation_probs = create_segmentation_probs(width, height);
    let seg_mask = probability_to_mask(&segmentation_probs, 2, 0.5)?;

    let mut foreground = 0u64;
    let mut background = 0u64;
    for y in 0..seg_mask.mask.height() {
        for x in 0..seg_mask.mask.width() {
            if seg_mask.mask.get_pixel(x, y)? > 0.5 {
                foreground += 1;
            } else {
                background += 1;
            }
        }
    }

    let total_pixels = width * height;
    println!("\nSegmentation statistics:");
    println!(
        "  Foreground: {:.2}% ({} pixels)",
        foreground as f64 / total_pixels as f64 * 100.0,
        foreground
    );
    println!(
        "  Background: {:.2}% ({} pixels)",
        background as f64 / total_pixels as f64 * 100.0,
        background
    );

    // Save segmentation result
    let bbox = BoundingBox::new(-10.0, 40.0, 10.0, 50.0)?;
    let gt = GeoTransform::from_bounds(&bbox, width, height)?;
    save_raster(
        &seg_mask.mask,
        &temp_dir.join("segmentation_result.tif"),
        &gt,
    )?;
    println!("\nSaved segmentation map to: segmentation_result.tif");

    // Step 5: Object Detection
    println!("\n\nStep 5: Object Detection");
    println!("------------------------");

    println!("Model: Object detection (simulated raw detections)");

    let raw_detections = simulate_detection_output();
    println!("  Raw detections: {}", raw_detections.len());

    // Apply non-maximum suppression
    println!("\nApplying Non-Maximum Suppression (NMS)...");
    let nms_config = NmsConfig {
        iou_threshold: 0.5,
        confidence_threshold: 0.3,
        ..Default::default()
    };

    let filtered_detections = non_maximum_suppression(&raw_detections, &nms_config)?;

    println!("  IoU threshold: {}", nms_config.iou_threshold);
    println!(
        "  Confidence threshold: {}",
        nms_config.confidence_threshold
    );
    println!("  Filtered detections: {}", filtered_detections.len());

    println!("\nDetection results:");
    let det_classes = ["Building", "Vehicle", "Tree"];

    for (i, detection) in filtered_detections.iter().enumerate() {
        println!("  Detection {}:", i + 1);
        println!(
            "    Class: {}",
            det_classes
                .get(detection.class_id)
                .copied()
                .unwrap_or("Unknown")
        );
        println!("    Confidence: {:.2}%", detection.confidence * 100.0);
        println!(
            "    Bbox: [{:.0}, {:.0}, {:.0}, {:.0}]",
            detection.bbox.x, detection.bbox.y, detection.bbox.width, detection.bbox.height
        );
    }

    // Step 6: Batch Processing
    println!("\n\nStep 6: Batch Processing");
    println!("------------------------");

    println!("Processing multiple tiles efficiently...");

    let tile_size = 128;
    let num_tiles_x = width / tile_size;
    let num_tiles_y = height / tile_size;

    println!("  Image size: {}x{}", width, height);
    println!("  Tile size: {}x{}", tile_size, tile_size);
    println!(
        "  Number of tiles: {}x{} = {}",
        num_tiles_x,
        num_tiles_y,
        num_tiles_x * num_tiles_y
    );

    println!("\nProcessing tiles in batch...");

    let mut batch_results = Vec::new();

    for ty in 0..num_tiles_y {
        for tx in 0..num_tiles_x {
            let x_offset = tx * tile_size;
            let y_offset = ty * tile_size;
            let tile = red_band.window(x_offset, y_offset, tile_size, tile_size)?;
            let tile_stats = tile.compute_statistics()?;
            batch_results.push(format!(
                "Tile ({}, {}): mean={:.2}",
                tx, ty, tile_stats.mean
            ));
        }
    }

    println!("  Processed {} tiles", batch_results.len());
    println!("\nSample results:");
    for result in batch_results.iter().take(4) {
        println!("  {}", result);
    }

    // Summary
    println!("\n\n=== Tutorial Complete! ===");
    println!("\nTopics Covered:");
    println!("  1. Image preprocessing (normalize, resize)");
    println!("  2. Classification (land cover)");
    println!("  3. Semantic segmentation");
    println!("  4. Object detection with Non-Maximum Suppression");
    println!("  5. Batch tile processing");

    println!("\nKey Points:");
    println!("  - `oxigeo-ml` provides raster-native pre/post-processing helpers");
    println!("  - Batch processing improves throughput");
    println!("  - Post-processing (NMS, thresholding) refines model outputs");

    println!("\nOutput Files:");
    println!("  - segmentation_result.tif");

    Ok(())
}

/// Create a sample band with synthetic data
fn create_sample_band(width: u64, height: u64, band: u32) -> RasterBuffer {
    let mut buffer = RasterBuffer::zeros(width, height, RasterDataType::UInt8);

    for y in 0..height {
        for x in 0..width {
            let dx = (x as f64) / (width as f64);
            let dy = (y as f64) / (height as f64);

            let value = match band {
                0 => dx * 200.0 + 55.0,
                1 => dy * 150.0 + 100.0,
                _ => ((dx + dy) / 2.0 * 180.0) + 75.0,
            };

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
            let prob = (1.0 - dist * 1.5).clamp(0.0, 1.0);
            let _ = buffer.set_pixel(x, y, prob);
        }
    }

    buffer
}

/// Simulate raw detection output
fn simulate_detection_output() -> Vec<Detection> {
    vec![
        Detection {
            bbox: MlBoundingBox::new(50.0, 50.0, 100.0, 100.0),
            class_id: 0,
            class_label: None,
            confidence: 0.92,
            attributes: Default::default(),
        },
        Detection {
            bbox: MlBoundingBox::new(55.0, 55.0, 90.0, 90.0),
            class_id: 0,
            class_label: None,
            confidence: 0.85,
            attributes: Default::default(),
        },
        Detection {
            bbox: MlBoundingBox::new(200.0, 200.0, 50.0, 30.0),
            class_id: 1,
            class_label: None,
            confidence: 0.78,
            attributes: Default::default(),
        },
        Detection {
            bbox: MlBoundingBox::new(300.0, 100.0, 40.0, 80.0),
            class_id: 2,
            class_label: None,
            confidence: 0.65,
            attributes: Default::default(),
        },
        Detection {
            bbox: MlBoundingBox::new(400.0, 400.0, 30.0, 50.0),
            class_id: 2,
            class_label: None,
            confidence: 0.45,
            attributes: Default::default(),
        },
    ]
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
