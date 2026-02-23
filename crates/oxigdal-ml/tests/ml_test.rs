//! Integration tests for OxiGDAL ML

use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::{GeoTransform, RasterDataType};
use oxigdal_ml::classification::{classify_single_label, compute_confusion_metrics};
use oxigdal_ml::detection::{BoundingBox, Detection, NmsConfig, non_maximum_suppression};
use oxigdal_ml::postprocessing::{apply_threshold, mask_to_polygons};
use oxigdal_ml::preprocessing::{NormalizationParams, TileConfig, normalize, tile_raster};
use oxigdal_ml::segmentation::{find_connected_components, probability_to_mask};
use std::collections::HashMap;

#[test]
fn test_preprocessing_normalization() {
    let buffer = RasterBuffer::zeros(100, 100, RasterDataType::Float32);
    let params = NormalizationParams::imagenet();

    let result = normalize(&buffer, &params);
    assert!(result.is_ok());

    let normalized = result
        .ok()
        .unwrap_or_else(|| RasterBuffer::zeros(1, 1, RasterDataType::Float32));
    assert_eq!(normalized.width(), 100);
    assert_eq!(normalized.height(), 100);
}

#[test]
fn test_preprocessing_tiling() {
    let buffer = RasterBuffer::zeros(512, 512, RasterDataType::Float32);
    let config = TileConfig {
        tile_width: 256,
        tile_height: 256,
        overlap: 32,
        padding: oxigdal_ml::preprocessing::PaddingStrategy::Replicate,
    };

    let result = tile_raster(&buffer, &config);
    assert!(result.is_ok());

    let tiles = result.ok().unwrap_or_default();
    assert!(!tiles.is_empty());
    assert!(tiles.len() >= 4); // At least 2x2 grid
}

#[test]
fn test_segmentation_probability_to_mask() {
    let mut probs = RasterBuffer::zeros(100, 100, RasterDataType::Float32);

    // Set some probabilities
    for y in 40..60 {
        for x in 40..60 {
            let _ = probs.set_pixel(x, y, 0.8);
        }
    }

    let result = probability_to_mask(&probs, 2, 0.5);
    assert!(result.is_ok());

    let mask = result
        .ok()
        .unwrap_or_else(|| oxigdal_ml::segmentation::SegmentationMask {
            mask: RasterBuffer::zeros(1, 1, RasterDataType::UInt16),
            num_classes: 2,
            class_labels: None,
        });
    assert_eq!(mask.num_classes, 2);
}

#[test]
fn test_segmentation_connected_components() {
    let mut mask = RasterBuffer::zeros(100, 100, RasterDataType::Float32);

    // Create a few components
    for y in 10..20 {
        for x in 10..20 {
            let _ = mask.set_pixel(x, y, 1.0);
        }
    }

    for y in 80..90 {
        for x in 80..90 {
            let _ = mask.set_pixel(x, y, 1.0);
        }
    }

    let result = find_connected_components(&mask, 10);
    assert!(result.is_ok());

    let instances = result
        .ok()
        .unwrap_or_else(|| oxigdal_ml::segmentation::InstanceSegmentation {
            instances: RasterBuffer::zeros(1, 1, RasterDataType::UInt32),
            instance_classes: HashMap::new(),
            instance_scores: HashMap::new(),
        });
    assert!(!instances.instance_classes.is_empty());
}

#[test]
fn test_classification() {
    let mut probs = RasterBuffer::zeros(10, 1, RasterDataType::Float32);

    // Set some probabilities
    let _ = probs.set_pixel(0, 0, 0.1);
    let _ = probs.set_pixel(1, 0, 0.2);
    let _ = probs.set_pixel(2, 0, 0.7); // Highest probability

    let labels = vec![
        "class0".to_string(),
        "class1".to_string(),
        "class2".to_string(),
    ];

    let result = classify_single_label(&probs, Some(&labels), 0.5);
    assert!(result.is_ok());

    let classification =
        result
            .ok()
            .unwrap_or_else(|| oxigdal_ml::classification::ClassificationResult {
                class_id: 0,
                class_label: None,
                confidence: 0.0,
                probabilities: HashMap::new(),
            });
    assert!(classification.confidence >= 0.5);
}

#[test]
fn test_detection_nms() {
    let detections = vec![
        Detection {
            bbox: BoundingBox::new(10.0, 10.0, 50.0, 50.0),
            class_id: 0,
            class_label: Some("car".to_string()),
            confidence: 0.9,
            attributes: HashMap::new(),
        },
        Detection {
            bbox: BoundingBox::new(15.0, 15.0, 50.0, 50.0),
            class_id: 0,
            class_label: Some("car".to_string()),
            confidence: 0.8,
            attributes: HashMap::new(),
        },
        Detection {
            bbox: BoundingBox::new(100.0, 100.0, 50.0, 50.0),
            class_id: 1,
            class_label: Some("truck".to_string()),
            confidence: 0.85,
            attributes: HashMap::new(),
        },
    ];

    let config = NmsConfig {
        iou_threshold: 0.5,
        confidence_threshold: 0.7,
        max_detections: Some(10),
    };

    let result = non_maximum_suppression(&detections, &config);
    assert!(result.is_ok());

    let filtered = result.ok().unwrap_or_default();
    assert!(filtered.len() <= detections.len());
    assert!(filtered.len() >= 2); // Should keep at least 2 (different locations/classes)
}

#[test]
fn test_postprocessing_threshold() {
    let mut probs = RasterBuffer::zeros(100, 100, RasterDataType::Float32);

    // Set some probabilities
    for y in 40..60 {
        for x in 40..60 {
            let _ = probs.set_pixel(x, y, 0.8);
        }
    }

    let result = apply_threshold(&probs, 0.5);
    assert!(result.is_ok());

    let thresholded = result
        .ok()
        .unwrap_or_else(|| RasterBuffer::zeros(1, 1, RasterDataType::Float32));
    assert_eq!(thresholded.width(), 100);
    assert_eq!(thresholded.height(), 100);

    // Check that high probability pixels are set to 1
    let value = thresholded.get_pixel(50, 50);
    assert!(value.is_ok());
    let value = value.ok().unwrap_or(0.0);
    assert!((value - 1.0).abs() < f64::EPSILON);
}

#[test]
fn test_postprocessing_mask_to_polygons() {
    let mut mask = RasterBuffer::zeros(100, 100, RasterDataType::Float32);

    // Create a rectangular region
    for y in 20..40 {
        for x in 20..40 {
            let _ = mask.set_pixel(x, y, 1.0);
        }
    }

    let result = mask_to_polygons(&mask, 100.0);
    assert!(result.is_ok());

    let polygons = result.ok().unwrap_or_default();
    assert!(!polygons.is_empty());
}

#[test]
fn test_confusion_metrics() {
    let mut predictions = RasterBuffer::zeros(100, 100, RasterDataType::UInt8);
    let mut ground_truth = RasterBuffer::zeros(100, 100, RasterDataType::UInt8);

    // Set some predictions and ground truth
    for y in 0..50 {
        for x in 0..50 {
            let _ = predictions.set_pixel(x, y, 1.0);
            let _ = ground_truth.set_pixel(x, y, 1.0);
        }
    }

    let result = compute_confusion_metrics(&predictions, &ground_truth, 1);
    assert!(result.is_ok());

    let metrics = result.unwrap_or(oxigdal_ml::classification::ConfusionMetrics {
        true_positives: 0,
        false_positives: 0,
        true_negatives: 0,
        false_negatives: 0,
        precision: 0.0,
        recall: 0.0,
        f1_score: 0.0,
        accuracy: 0.0,
    });

    assert!(metrics.accuracy > 0.0);
    assert!(metrics.precision >= 0.0 && metrics.precision <= 1.0);
    assert!(metrics.recall >= 0.0 && metrics.recall <= 1.0);
}

#[test]
fn test_end_to_end_workflow() {
    // Simulate a complete ML workflow
    let input = RasterBuffer::zeros(256, 256, RasterDataType::Float32);

    // 1. Preprocessing
    let params = NormalizationParams::zero_mean_unit_variance();
    let normalized = normalize(&input, &params);
    assert!(normalized.is_ok());

    // 2. Simulate inference (would normally use a model)
    let predictions = normalized
        .ok()
        .unwrap_or_else(|| RasterBuffer::zeros(1, 1, RasterDataType::Float32));

    // 3. Postprocessing
    let mask = probability_to_mask(&predictions, 2, 0.5);
    assert!(mask.is_ok());

    // 4. Extract polygons
    let mask = mask
        .ok()
        .unwrap_or_else(|| oxigdal_ml::segmentation::SegmentationMask {
            mask: RasterBuffer::zeros(1, 1, RasterDataType::UInt16),
            num_classes: 2,
            class_labels: None,
        });
    let polygons = mask_to_polygons(&mask.mask, 10.0);
    assert!(polygons.is_ok());
}

#[test]
fn test_geotransform_integration() {
    // Test that GeoTransform works with detection georeferencing
    let gt = GeoTransform::new(0.0, 1.0, 0.0, 0.0, 0.0, -1.0);

    let (x, y) = gt.pixel_to_world(100.0, 100.0);
    // Check that world coordinates are computed correctly
    assert!((x - 100.0).abs() < f64::EPSILON);
    assert!((y - (-100.0)).abs() < f64::EPSILON);
}

#[test]
fn test_error_handling() {
    // Test that invalid parameters are properly rejected
    let buffer = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

    // Invalid threshold (> 1.0)
    let result = apply_threshold(&buffer, 1.5);
    assert!(result.is_err());

    // Invalid normalization (zero std)
    let params = NormalizationParams {
        mean: vec![0.0],
        std: vec![0.0],
    };
    let result = normalize(&buffer, &params);
    assert!(result.is_err());
}
