//! Tutorial 06: Machine Learning Inference
//!
//! This tutorial demonstrates ML inference on geospatial data:
//! - Loading ONNX models
//! - Preprocessing raster data for ML
//! - Running inference (classification, segmentation, detection)
//! - Post-processing results
//! - Batch processing
//! - Performance optimization
//!
//! Run with:
//! ```bash
//! cargo run --example 06_ml_inference --features ml
//! ```

use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::{BoundingBox, GeoTransform, RasterDataType};
use oxigdal_geotiff::writer::{CompressionType, GeoTiffWriter, GeoTiffWriterOptions};
use oxigdal_ml::classification::{ClassificationModel, ClassificationResult};
use oxigdal_ml::detection::{Detection, DetectionModel, DetectionResult};
use oxigdal_ml::inference::{InferenceEngine, InferenceOptions};
use oxigdal_ml::models::onnx::OnnxModel;
use oxigdal_ml::postprocessing::{non_max_suppression, softmax};
use oxigdal_ml::preprocessing::{normalize, resize, to_chw_format};
use oxigdal_ml::segmentation::{SegmentationModel, SegmentationResult};
use std::env;
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Tutorial 06: Machine Learning Inference ===\n");

    let temp_dir = env::temp_dir();

    // Step 1: Preparing Input Data
    println!("Step 1: Preparing Input Data");
    println!("-----------------------------");

    // Create a sample RGB image (simulate satellite imagery)
    let width = 512;
    let height = 512;

    println!("Creating synthetic RGB satellite image...");
    let red_band = create_sample_band(width, height, 0)?;
    let green_band = create_sample_band(width, height, 1)?;
    let blue_band = create_sample_band(width, height, 2)?;

    println!("  Image size: {}x{}", width, height);
    println!("  Bands: 3 (RGB)");

    // Step 2: Image Preprocessing
    println!("\n\nStep 2: Image Preprocessing for ML");
    println!("-----------------------------------");

    println!("Preprocessing steps:");

    // Step 2a: Normalize to 0-1 range
    println!("\n1. Normalization:");
    let normalized_red = normalize(&red_band, 0.0, 255.0, 0.0, 1.0)?;
    let normalized_green = normalize(&green_band, 0.0, 255.0, 0.0, 1.0)?;
    let normalized_blue = normalize(&blue_band, 0.0, 255.0, 0.0, 1.0)?;

    println!("   Normalized from [0, 255] to [0, 1]");

    let norm_stats = normalized_red.compute_statistics()?;
    println!("   Red band range: [{:.4}, {:.4}]", norm_stats.min, norm_stats.max);

    // Step 2b: Resize to model input size
    println!("\n2. Resizing:");
    let model_input_size = 224; // Common size for many models
    let resized_red = resize(&normalized_red, model_input_size, model_input_size)?;
    let resized_green = resize(&normalized_green, model_input_size, model_input_size)?;
    let resized_blue = resize(&normalized_blue, model_input_size, model_input_size)?;

    println!("   Resized from {}x{} to {}x{}",
             width, height, model_input_size, model_input_size);

    // Step 2c: Convert to CHW format (Channel, Height, Width)
    println!("\n3. Format conversion:");
    let bands = vec![&resized_red, &resized_green, &resized_blue];
    let chw_tensor = to_chw_format(&bands)?;

    println!("   Converted to CHW format: [3, {}, {}]",
             model_input_size, model_input_size);
    println!("   Tensor shape: {:?}", chw_tensor.shape());

    // Step 3: Classification (Land Cover)
    println!("\n\nStep 3: Image Classification");
    println!("-----------------------------");

    println!("Model: Land cover classification");
    println!("  Input: RGB image (224x224)");
    println!("  Output: Class probabilities");
    println!("  Classes: Water, Forest, Urban, Agriculture, Barren");

    // Load classification model (simulated)
    println!("\nLoading ONNX model...");
    let classification_model_path = "models/land_cover_classifier.onnx";
    println!("  Model path: {}", classification_model_path);

    // In production:
    // let onnx_model = OnnxModel::from_file(classification_model_path)?;
    // let classifier = ClassificationModel::from_onnx(onnx_model)?;

    // Simulate inference
    println!("\nRunning inference...");
    let class_probs = simulate_classification_output();

    println!("  Inference complete!");
    println!("\nClassification results:");
    let class_names = vec!["Water", "Forest", "Urban", "Agriculture", "Barren"];

    for (i, (name, prob)) in class_names.iter().zip(class_probs.iter()).enumerate() {
        println!("  {}: {:.2}%", name, prob * 100.0);
    }

    let predicted_class = class_probs
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(idx, _)| idx)
        .ok_or("No class predicted")?;

    println!("\nPredicted class: {} ({:.2}% confidence)",
             class_names[predicted_class],
             class_probs[predicted_class] * 100.0);

    // Step 4: Semantic Segmentation
    println!("\n\nStep 4: Semantic Segmentation");
    println!("------------------------------");

    println!("Model: Semantic segmentation (U-Net)");
    println!("  Input: RGB image (512x512)");
    println!("  Output: Pixel-wise class labels");
    println!("  Classes: Background, Building, Road, Vegetation, Water");

    let segmentation_model_path = "models/unet_segmentation.onnx";
    println!("\nModel path: {}", segmentation_model_path);

    // In production:
    // let seg_model = OnnxModel::from_file(segmentation_model_path)?;
    // let segmenter = SegmentationModel::from_onnx(seg_model)?;

    println!("\nRunning segmentation inference...");

    // Simulate segmentation output
    let segmentation_map = simulate_segmentation_output(width, height)?;

    println!("  Segmentation complete!");

    // Compute class distribution
    let mut class_counts = vec![0u32; 5];
    for y in 0..segmentation_map.height() {
        for x in 0..segmentation_map.width() {
            let class_id = segmentation_map.get_pixel(x, y)? as usize;
            if class_id < class_counts.len() {
                class_counts[class_id] += 1;
            }
        }
    }

    let total_pixels = width * height;
    println!("\nSegmentation statistics:");
    let seg_classes = vec!["Background", "Building", "Road", "Vegetation", "Water"];

    for (name, count) in seg_classes.iter().zip(class_counts.iter()) {
        let percentage = (*count as f64 / total_pixels as f64) * 100.0;
        println!("  {}: {:.2}% ({} pixels)", name, percentage, count);
    }

    // Save segmentation result
    let bbox = BoundingBox::new(-10.0, 40.0, 10.0, 50.0)?;
    let gt = GeoTransform::from_bounds(&bbox, width, height)?;

    save_raster(&segmentation_map, &temp_dir.join("segmentation_result.tif"), &gt)?;
    println!("\nSaved segmentation map to: segmentation_result.tif");

    // Step 5: Object Detection
    println!("\n\nStep 5: Object Detection");
    println!("------------------------");

    println!("Model: Object detection (YOLO/Faster R-CNN)");
    println!("  Input: RGB image (512x512)");
    println!("  Output: Bounding boxes with class labels and confidence");
    println!("  Classes: Building, Vehicle, Tree");

    let detection_model_path = "models/object_detector.onnx";
    println!("\nModel path: {}", detection_model_path);

    // In production:
    // let det_model = OnnxModel::from_file(detection_model_path)?;
    // let detector = DetectionModel::from_onnx(det_model)?;

    println!("\nRunning object detection...");

    // Simulate detection output
    let raw_detections = simulate_detection_output();

    println!("  Detection complete!");
    println!("  Raw detections: {}", raw_detections.len());

    // Apply non-maximum suppression
    println!("\nApplying Non-Maximum Suppression (NMS)...");
    let nms_threshold = 0.5;
    let confidence_threshold = 0.3;

    let filtered_detections = non_max_suppression(
        &raw_detections,
        nms_threshold,
        confidence_threshold,
    )?;

    println!("  NMS threshold: {}", nms_threshold);
    println!("  Confidence threshold: {}", confidence_threshold);
    println!("  Filtered detections: {}", filtered_detections.len());

    println!("\nDetection results:");
    let det_classes = vec!["Building", "Vehicle", "Tree"];

    for (i, detection) in filtered_detections.iter().enumerate() {
        println!("  Detection {}:", i + 1);
        println!("    Class: {}", det_classes[detection.class_id as usize]);
        println!("    Confidence: {:.2}%", detection.confidence * 100.0);
        println!("    Bbox: [{:.0}, {:.0}, {:.0}, {:.0}]",
                 detection.bbox.0, detection.bbox.1,
                 detection.bbox.2, detection.bbox.3);
    }

    // Step 6: Batch Processing
    println!("\n\nStep 6: Batch Processing");
    println!("------------------------");

    println!("Processing multiple tiles efficiently...");

    let tile_size = 256;
    let num_tiles_x = width / tile_size;
    let num_tiles_y = height / tile_size;

    println!("  Image size: {}x{}", width, height);
    println!("  Tile size: {}x{}", tile_size, tile_size);
    println!("  Number of tiles: {}x{} = {}",
             num_tiles_x, num_tiles_y, num_tiles_x * num_tiles_y);

    println!("\nProcessing tiles in batch...");

    let mut batch_results = Vec::new();

    for ty in 0..num_tiles_y {
        for tx in 0..num_tiles_x {
            // Extract tile
            let x_offset = tx * tile_size;
            let y_offset = ty * tile_size;

            // Simulate processing
            let result = format!("Tile ({}, {}): Water=15%, Forest=60%, Urban=25%", tx, ty);
            batch_results.push(result);
        }
    }

    println!("  Processed {} tiles", batch_results.len());

    // Show first few results
    println!("\nSample results:");
    for (i, result) in batch_results.iter().take(4).enumerate() {
        println!("  {}", result);
    }

    // Step 7: Performance Optimization
    println!("\n\nStep 7: Performance Optimization");
    println!("---------------------------------");

    println!("Optimization techniques:");

    println!("\n1. Model Optimization:");
    println!("   - Quantization: INT8 models (4x faster, 4x smaller)");
    println!("   - Pruning: Remove unnecessary weights");
    println!("   - Knowledge distillation: Smaller student models");

    println!("\n2. Inference Optimization:");
    println!("   - Batch processing: Process multiple tiles together");
    println!("   - TensorRT: NVIDIA GPU optimization");
    println!("   - OpenVINO: Intel CPU/GPU optimization");
    println!("   - ONNX Runtime: Cross-platform optimization");

    println!("\n3. Data Pipeline Optimization:");
    println!("   - Parallel tile loading");
    println!("   - Async preprocessing");
    println!("   - Memory pooling");
    println!("   - Tile caching");

    // Create inference options
    let inference_options = InferenceOptions {
        batch_size: 8,
        use_gpu: true,
        num_threads: 4,
        execution_provider: "CUDA".to_string(),
        optimization_level: 3,
    };

    println!("\nInference configuration:");
    println!("  Batch size: {}", inference_options.batch_size);
    println!("  GPU acceleration: {}", inference_options.use_gpu);
    println!("  CPU threads: {}", inference_options.num_threads);
    println!("  Execution provider: {}", inference_options.execution_provider);
    println!("  Optimization level: {}", inference_options.optimization_level);

    // Step 8: Model Deployment Patterns
    println!("\n\nStep 8: Model Deployment Patterns");
    println!("----------------------------------");

    println!("\n1. Edge Deployment:");
    println!("   - Mobile devices (iOS/Android)");
    println!("   - IoT devices");
    println!("   - Drones and robots");
    println!("   - Considerations: Model size, latency, power");

    println!("\n2. Server Deployment:");
    println!("   - Batch processing");
    println!("   - Large-scale inference");
    println!("   - API services");
    println!("   - Considerations: Throughput, scalability");

    println!("\n3. Cloud Deployment:");
    println!("   - AWS SageMaker");
    println!("   - Google AI Platform");
    println!("   - Azure ML");
    println!("   - Considerations: Cost, autoscaling, multi-region");

    // Summary
    println!("\n\n=== Tutorial Complete! ===");
    println!("\nTopics Covered:");
    println!("  1. Image preprocessing (normalize, resize, format conversion)");
    println!("  2. Classification (land cover)");
    println!("  3. Semantic segmentation (U-Net)");
    println!("  4. Object detection (YOLO/Faster R-CNN)");
    println!("  5. Post-processing (NMS, softmax)");
    println!("  6. Batch processing strategies");
    println!("  7. Performance optimization");
    println!("  8. Deployment patterns");

    println!("\nKey Points:");
    println!("  - ONNX provides cross-platform model compatibility");
    println!("  - Preprocessing is critical for model accuracy");
    println!("  - Batch processing improves throughput");
    println!("  - Post-processing refines model outputs");
    println!("  - GPU acceleration essential for large-scale inference");

    println!("\nOutput Files:");
    println!("  - segmentation_result.tif");

    println!("\nNext Tutorial:");
    println!("  - Try tutorial 07 for web services (WMS/WMTS)");

    Ok(())
}

/// Create a sample band with synthetic data
fn create_sample_band(
    width: u32,
    height: u32,
    band: u32,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let mut buffer = RasterBuffer::zeros(width, height, RasterDataType::UInt8);

    for y in 0..height {
        for x in 0..width {
            // Create some spatial patterns
            let dx = (x as f64) / (width as f64);
            let dy = (y as f64) / (height as f64);

            let value = match band {
                0 => (dx * 200.0 + 55.0) as f64,           // Red
                1 => (dy * 150.0 + 100.0) as f64,          // Green
                2 => ((dx + dy) / 2.0 * 180.0 + 75.0) as f64, // Blue
                _ => 128.0,
            };

            buffer.set_pixel(x, y, value)?;
        }
    }

    Ok(buffer)
}

/// Simulate classification output
fn simulate_classification_output() -> Vec<f32> {
    vec![0.05, 0.35, 0.20, 0.30, 0.10] // Water, Forest, Urban, Agriculture, Barren
}

/// Simulate segmentation output
fn simulate_segmentation_output(
    width: u32,
    height: u32,
) -> Result<RasterBuffer, Box<dyn std::error::Error>> {
    let mut buffer = RasterBuffer::zeros(width, height, RasterDataType::UInt8);

    for y in 0..height {
        for x in 0..width {
            // Create regions
            let class_id = if y < height / 3 {
                4 // Water
            } else if y < 2 * height / 3 {
                if x < width / 2 { 3 } else { 2 } // Vegetation or Road
            } else {
                if (x / 32 + y / 32) % 2 == 0 { 1 } else { 0 } // Building or Background
            };

            buffer.set_pixel(x, y, class_id as f64)?;
        }
    }

    Ok(buffer)
}

/// Simulate detection output
fn simulate_detection_output() -> Vec<Detection> {
    vec![
        Detection {
            class_id: 0,
            confidence: 0.92,
            bbox: (50.0, 50.0, 150.0, 150.0),
        },
        Detection {
            class_id: 0,
            confidence: 0.85,
            bbox: (55.0, 55.0, 145.0, 145.0), // Overlapping detection
        },
        Detection {
            class_id: 1,
            confidence: 0.78,
            bbox: (200.0, 200.0, 250.0, 230.0),
        },
        Detection {
            class_id: 2,
            confidence: 0.65,
            bbox: (300.0, 100.0, 340.0, 180.0),
        },
        Detection {
            class_id: 2,
            confidence: 0.45,
            bbox: (400.0, 400.0, 430.0, 450.0),
        },
    ]
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
