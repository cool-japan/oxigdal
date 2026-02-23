//! Integration tests for ML pipeline features
//!
//! Tests for data loading, cloud removal, super-resolution, and temporal forecasting.

#![allow(unexpected_cfgs)]

use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::{GeoTransform, RasterDataType};

#[cfg(feature = "temporal")]
use ndarray::Array3;

#[cfg(feature = "cloud-removal")]
use oxigdal_ml::cloud::{CloudConfig, CloudRemover};

#[cfg(feature = "temporal")]
use oxigdal_ml::temporal::forecasting::{ForecastConfig, TemporalForecaster};

use oxigdal_ml::segmentation::PanopticSegmentation;

/// Test cloud detection configuration
#[test]
#[cfg(feature = "cloud-removal")]
fn test_cloud_config() {
    let config = CloudConfig::sentinel2();
    assert!(config.dilation_radius > 0);

    let landsat = CloudConfig::landsat();
    assert!(landsat.dilation_radius > 0);
}

/// Test cloud removal configuration
#[test]
#[cfg(feature = "cloud-removal")]
fn test_cloud_removal_config() {
    let config = CloudConfig::landsat();
    let _remover = CloudRemover::new(config);

    // Remover should be created successfully
    // Actual removal would require a model file
    let _ = &_remover; // verify remover was created
}

// Removed - covered by test_cloud_config

/// Test temporal forecaster creation and configuration
#[test]
#[cfg(feature = "temporal")]
fn test_temporal_forecaster_creation() {
    let config = ForecastConfig::new(1, 64, 2, 6);
    let result = TemporalForecaster::new(config);

    assert!(result.is_ok(), "Forecaster creation should succeed");
    let forecaster = result.expect("Should have forecaster");
    assert_eq!(forecaster.config().input_features, 1);
    assert_eq!(forecaster.config().hidden_dim, 64);
    assert_eq!(forecaster.config().forecast_horizon, 6);
}

/// Test temporal forecasting with mock data
#[test]
#[cfg(feature = "temporal")]
fn test_temporal_forecasting() {
    let config = ForecastConfig {
        input_features: 1,
        hidden_dim: 32,
        num_layers: 1,
        forecast_horizon: 3,
        dropout: 0.0,
        bidirectional: false,
        sequence_length: 10,
    };

    let forecaster = TemporalForecaster::new(config).expect("Failed to create forecaster");

    // Create mock input: batch_size=2, seq_len=10, features=1
    let input = Array3::<f32>::zeros((2, 10, 1));

    let result = forecaster.predict(&input);
    assert!(result.is_ok(), "Prediction should succeed");

    let forecast = result.expect("Should have forecast");
    assert_eq!(forecast.shape(), (2, 3, 1)); // batch_size=2, horizon=3, features=1
}

/// Test panoptic segmentation structure
#[test]
fn test_panoptic_segmentation_creation() {
    use oxigdal_ml::segmentation::PanopticSegment;

    // Create a simple panoptic mask
    let mask = RasterBuffer::zeros(64, 64, RasterDataType::UInt32);

    // Create segments
    let segments = vec![
        PanopticSegment {
            id: 1,
            class_id: 0,
            is_thing: false,
            pixel_count: 2000,
            score: 1.0,
        },
        PanopticSegment {
            id: 2,
            class_id: 1,
            is_thing: true,
            pixel_count: 500,
            score: 0.9,
        },
    ];

    let panoptic = PanopticSegmentation { mask, segments };

    assert_eq!(panoptic.segments.len(), 2);
    assert_eq!(panoptic.segments[0].class_id, 0);
    assert!(!panoptic.segments[0].is_thing);
    assert!(panoptic.segments[1].is_thing);
}

/// Test panoptic segmentation COCO export
#[test]
fn test_panoptic_coco_export() {
    use oxigdal_ml::segmentation::PanopticSegment;

    // Create simple panoptic segmentation
    let mask = RasterBuffer::zeros(64, 64, RasterDataType::UInt32);
    let segments = vec![PanopticSegment {
        id: 1,
        class_id: 0,
        is_thing: false,
        pixel_count: 2000,
        score: 1.0,
    }];

    let panoptic = PanopticSegmentation { mask, segments };

    let result = panoptic.to_coco_format();
    assert!(result.is_ok(), "COCO export should succeed");

    let coco_annotations = result.expect("Should have COCO format");
    assert!(!coco_annotations.is_empty(), "Should have annotations");
}

/// Test panoptic segmentation GeoJSON export
#[test]
fn test_panoptic_geojson_export() {
    use oxigdal_ml::segmentation::PanopticSegment;

    // Create simple panoptic segmentation with properly populated mask
    let mut mask = RasterBuffer::zeros(64, 64, RasterDataType::UInt32);
    // Fill a region with segment ID 1
    for y in 10..30 {
        for x in 10..30 {
            let _ = mask.set_pixel(x, y, 1.0);
        }
    }

    let segments = vec![PanopticSegment {
        id: 1,
        class_id: 0,
        is_thing: false,
        pixel_count: 400, // 20x20 region
        score: 1.0,
    }];

    let panoptic = PanopticSegmentation { mask, segments };

    let geo_transform = GeoTransform::new(0.0, 1.0, 0.0, 0.0, 0.0, -1.0);

    let result = panoptic.to_geojson(&geo_transform);
    assert!(result.is_ok(), "GeoJSON export should succeed");
}

/// Test simplified ML workflow
#[test]
#[cfg(feature = "temporal")]
fn test_simplified_ml_workflow() {
    // Test temporal forecasting workflow
    let forecast_config = ForecastConfig::new(1, 32, 1, 3);
    let forecaster = TemporalForecaster::new(forecast_config).expect("Failed to create forecaster");

    // Create time series
    let time_series = Array3::<f32>::zeros((1, 10, 1));
    let forecast = forecaster
        .predict(&time_series)
        .expect("Forecasting failed");

    assert_eq!(forecast.shape(), (1, 3, 1));
}

/// Test forecast configuration validation
#[test]
#[cfg(feature = "temporal")]
fn test_forecast_config_validation() {
    // Valid config
    let valid = ForecastConfig::new(1, 64, 2, 6);
    assert!(valid.validate().is_ok());

    // Invalid: zero input features
    let invalid = ForecastConfig::new(0, 64, 2, 6);
    assert!(invalid.validate().is_err());

    // Invalid: zero forecast horizon
    let invalid = ForecastConfig::new(1, 64, 2, 0);
    assert!(invalid.validate().is_err());
}

/// Test error handling for invalid inputs
#[test]
#[cfg(feature = "temporal")]
fn test_temporal_error_handling() {
    let config = ForecastConfig::new(2, 32, 1, 3);
    let forecaster = TemporalForecaster::new(config).expect("Failed to create forecaster");

    // Invalid input: wrong number of features
    let wrong_features = Array3::<f32>::zeros((1, 10, 1)); // Should be 2 features
    let result = forecaster.predict(&wrong_features);
    assert!(result.is_err(), "Should reject wrong feature count");
}

/// Test instance segmentation structure
#[test]
fn test_instance_segmentation() {
    use oxigdal_ml::segmentation::InstanceSegmentation;
    use std::collections::HashMap;

    let instances = RasterBuffer::zeros(50, 50, RasterDataType::UInt32);

    let mut instance_classes = HashMap::new();
    instance_classes.insert(1, 0); // Instance 1 is class 0
    instance_classes.insert(2, 1); // Instance 2 is class 1

    let mut instance_scores = HashMap::new();
    instance_scores.insert(1, 0.9);
    instance_scores.insert(2, 0.8);

    let seg = InstanceSegmentation {
        instances,
        instance_classes,
        instance_scores,
    };

    assert_eq!(seg.instance_classes.len(), 2);
    assert_eq!(seg.instance_scores.len(), 2);
    assert_eq!(
        *seg.instance_classes
            .get(&1)
            .expect("Should have instance 1"),
        0
    );
}

/// Test memory efficiency with large data
#[test]
#[cfg(feature = "temporal")]
fn test_memory_efficiency() {
    // Test that we can handle reasonably large data
    let config = ForecastConfig::new(10, 64, 2, 6);
    let forecaster = TemporalForecaster::new(config).expect("Failed to create forecaster");

    // Moderate size: 10 batch, 100 timesteps, 10 features
    let input = Array3::<f32>::zeros((10, 100, 10));

    let result = forecaster.predict(&input);
    assert!(result.is_ok(), "Should handle moderate-size data");
}
