//! Integration tests for oxigdal-temporal
#![allow(clippy::expect_used)]

use chrono::{DateTime, NaiveDate};
use oxigdal_temporal::{
    aggregation::{AggregationConfig, AggregationStatistic, TemporalAggregator, TemporalWindow},
    change::{ChangeDetectionConfig, ChangeDetectionMethod, ChangeDetector},
    compositing::{CompositingConfig, CompositingMethod, TemporalCompositor},
    gap_filling::{GapFillMethod, GapFiller},
    phenology::{PhenologyConfig, PhenologyExtractor},
    stack::RasterStack,
    timeseries::{TemporalMetadata, TemporalResolution, TimeSeriesRaster},
    trend::{TrendAnalyzer, TrendMethod},
};
use scirs2_core::ndarray::Array3;

fn create_test_timeseries(n: usize) -> TimeSeriesRaster {
    let mut ts = TimeSeriesRaster::new();

    for i in 0..n {
        let dt = DateTime::from_timestamp(1640995200 + (i as i64) * 86400, 0).expect("valid");
        let date = NaiveDate::from_ymd_opt(2022, 1, 1 + i as u32).expect("valid");
        let metadata = TemporalMetadata::new(dt, date)
            .with_cloud_cover((i * 5) as f32)
            .with_quality_score(1.0 - (i as f32 * 0.05));

        let mut data = Array3::zeros((10, 10, 2));
        for j in 0..10 {
            for k in 0..10 {
                data[[j, k, 0]] = (i + j + k) as f64;
                data[[j, k, 1]] = (i + j + k) as f64 * 2.0;
            }
        }

        ts.add_raster(metadata, data).expect("should add");
    }

    ts
}

#[test]
fn test_timeseries_creation_and_queries() {
    let ts = create_test_timeseries(10);

    assert_eq!(ts.len(), 10);
    assert!(!ts.is_empty());
    assert_eq!(ts.expected_shape(), Some((10, 10, 2)));

    let (start, end) = ts.time_range().expect("should have range");
    assert!(start < end);

    let timestamps = ts.timestamps();
    assert_eq!(timestamps.len(), 10);
}

#[test]
fn test_timeseries_filtering() {
    let mut ts = create_test_timeseries(10);

    let removed = ts.filter_by_cloud_cover(15.0).expect("should filter");
    assert!(removed > 0);
    assert!(ts.len() < 10);
}

#[test]
fn test_pixel_timeseries_extraction() {
    let ts = create_test_timeseries(10);

    let pixel_ts = ts
        .extract_pixel_timeseries(5, 5, 0)
        .expect("should extract");

    assert_eq!(pixel_ts.len(), 10);

    // Values should be increasing
    for i in 1..pixel_ts.len() {
        assert!(pixel_ts[i] >= pixel_ts[i - 1]);
    }
}

#[test]
fn test_mean_compositing() {
    let ts = create_test_timeseries(10);

    let config = CompositingConfig {
        method: CompositingMethod::Mean,
        max_cloud_cover: None,
        min_quality: None,
        ..Default::default()
    };

    let composite = TemporalCompositor::composite(&ts, &config).expect("should composite");

    assert_eq!(composite.data.shape(), &[10, 10, 2]);
    assert!(composite.count[[0, 0, 0]] > 0);
}

#[test]
fn test_median_compositing() {
    let ts = create_test_timeseries(10);

    let config = CompositingConfig {
        method: CompositingMethod::Median,
        max_cloud_cover: Some(30.0),
        ..Default::default()
    };

    let composite = TemporalCompositor::composite(&ts, &config).expect("should composite");

    assert_eq!(composite.data.shape(), &[10, 10, 2]);
}

#[test]
fn test_maximum_value_composite() {
    let ts = create_test_timeseries(10);

    let config = CompositingConfig {
        method: CompositingMethod::Maximum,
        max_cloud_cover: None,
        min_quality: None,
        ..Default::default()
    };

    let composite = TemporalCompositor::composite(&ts, &config).expect("should composite");

    assert_eq!(composite.data.shape(), &[10, 10, 2]);
}

#[test]
fn test_temporal_interpolation() {
    let ts = TimeSeriesRaster::with_resolution(TemporalResolution::Daily);

    // Create sparse time series with gaps
    let ts = {
        let mut ts = ts;
        let dt1 = DateTime::from_timestamp(1640995200, 0).expect("valid");
        let date1 = NaiveDate::from_ymd_opt(2022, 1, 1).expect("valid");
        let metadata1 = TemporalMetadata::new(dt1, date1);
        ts.add_raster(metadata1, Array3::from_elem((5, 5, 1), 10.0))
            .expect("should add");

        // Gap of 10 days
        let dt2 = DateTime::from_timestamp(1641859200, 0).expect("valid");
        let date2 = NaiveDate::from_ymd_opt(2022, 1, 11).expect("valid");
        let metadata2 = TemporalMetadata::new(dt2, date2);
        ts.add_raster(metadata2, Array3::from_elem((5, 5, 1), 20.0))
            .expect("should add");
        ts
    };

    let filled_ts = GapFiller::fill_gaps(&ts, GapFillMethod::LinearInterpolation, None)
        .expect("should interpolate");

    // The filled time series should have at least as many entries
    assert!(filled_ts.len() >= ts.len());
}

#[test]
fn test_temporal_aggregation_monthly() {
    let ts = create_test_timeseries(30);

    let config = AggregationConfig {
        window: TemporalWindow::Monthly,
        statistics: vec![AggregationStatistic::Mean, AggregationStatistic::Max],
        ..Default::default()
    };

    let result = TemporalAggregator::aggregate(&ts, &config).expect("should aggregate");

    assert!(result.get("Mean").is_some());
    assert!(result.get("Max").is_some());

    let mean_ts = result.get("Mean").expect("should have mean");
    assert!(!mean_ts.is_empty());
}

#[test]
fn test_temporal_aggregation_rolling() {
    let ts = create_test_timeseries(20);

    let config = AggregationConfig {
        window: TemporalWindow::Rolling(7),
        statistics: vec![AggregationStatistic::Mean],
        min_observations: 5,
        ..Default::default()
    };

    let result = TemporalAggregator::aggregate(&ts, &config).expect("should aggregate");

    let mean_ts = result.get("Mean").expect("should have mean");
    assert!(!mean_ts.is_empty());
}

#[test]
fn test_change_detection_simple_difference() {
    let ts = create_test_timeseries(10);

    let config = ChangeDetectionConfig {
        method: ChangeDetectionMethod::SimpleDifference,
        ..Default::default()
    };

    let result = ChangeDetector::detect(&ts, &config).expect("should detect");

    assert_eq!(result.magnitude.shape(), &[10, 10, 2]);
    assert_eq!(result.direction.shape(), &[10, 10, 2]);

    // Should show positive change (increasing values)
    assert!(result.direction[[0, 0, 0]] > 0);
}

#[test]
fn test_change_detection_relative() {
    let ts = create_test_timeseries(10);

    let config = ChangeDetectionConfig {
        method: ChangeDetectionMethod::RelativeChange,
        ..Default::default()
    };

    let result = ChangeDetector::detect(&ts, &config).expect("should detect");
    assert_eq!(result.magnitude.shape(), &[10, 10, 2]);
}

#[test]
fn test_trend_analysis_linear() {
    let ts = create_test_timeseries(20);

    let result = TrendAnalyzer::analyze(&ts, TrendMethod::Linear).expect("should analyze");

    assert_eq!(result.slope.shape(), &[10, 10, 2]);
    assert_eq!(result.intercept.shape(), &[10, 10, 2]);

    // Slope should be positive (increasing trend)
    assert!(result.slope[[0, 0, 0]] > 0.0);
    assert_eq!(result.direction[[0, 0, 0]], 1);
}

#[test]
fn test_trend_analysis_sens_slope() {
    let ts = create_test_timeseries(15);

    let result = TrendAnalyzer::analyze(&ts, TrendMethod::SensSlope).expect("should analyze");

    assert_eq!(result.slope.shape(), &[10, 10, 2]);
    assert!(result.slope[[0, 0, 0]] > 0.0);
}

#[test]
fn test_raster_stack_operations() {
    let ts = create_test_timeseries(10);
    let stack = RasterStack::from_timeseries(&ts).expect("should create stack");

    assert_eq!(stack.shape(), (10, 10, 10, 2));

    let time_slice = stack.get_time_slice(5).expect("should get slice");
    assert_eq!(time_slice.shape(), &[10, 10, 2]);

    let pixel_ts = stack
        .get_pixel_timeseries(5, 5, 0)
        .expect("should get timeseries");
    assert_eq!(pixel_ts.len(), 10);
}

#[test]
fn test_stack_temporal_statistics() {
    let ts = create_test_timeseries(10);
    let stack = RasterStack::from_timeseries(&ts).expect("should create stack");

    let mean = stack.mean_temporal().expect("should compute mean");
    assert_eq!(mean.shape(), &[10, 10, 2]);

    let median = stack.median_temporal().expect("should compute median");
    assert_eq!(median.shape(), &[10, 10, 2]);

    let min = stack.min_temporal().expect("should compute min");
    let max = stack.max_temporal().expect("should compute max");

    // Max should be >= min for all pixels
    for i in 0..10 {
        for j in 0..10 {
            for k in 0..2 {
                assert!(max[[i, j, k]] >= min[[i, j, k]]);
            }
        }
    }
}

#[test]
fn test_stack_concatenation() {
    let ts1 = create_test_timeseries(5);
    let stack1 = RasterStack::from_timeseries(&ts1).expect("should create");

    let ts2 = create_test_timeseries(3);
    let stack2 = RasterStack::from_timeseries(&ts2).expect("should create");

    let concatenated =
        RasterStack::concatenate_time(vec![stack1, stack2]).expect("should concatenate");

    assert_eq!(concatenated.shape(), (8, 10, 10, 2));
}

#[test]
fn test_stack_subsetting() {
    let ts = create_test_timeseries(10);
    let stack = RasterStack::from_timeseries(&ts).expect("should create");

    let time_subset = stack.subset_time(2, 7).expect("should subset");
    assert_eq!(time_subset.shape(), (5, 10, 10, 2));

    let band_subset = stack.subset_bands(&[0]).expect("should subset");
    assert_eq!(band_subset.shape(), (10, 10, 10, 1));
}

#[test]
fn test_phenology_extraction() {
    let mut ts = TimeSeriesRaster::new();

    // Simulate vegetation phenology with sinusoidal pattern
    for i in 0..36 {
        // 36 observations over a year
        let dt = DateTime::from_timestamp(1640995200 + (i * 10 * 86400), 0).expect("valid");
        let days = i * 10;
        let date =
            NaiveDate::from_ymd_opt(2022, 1, 1).expect("valid") + chrono::Duration::days(days);
        let metadata = TemporalMetadata::new(dt, date);

        // Sinusoidal NDVI-like pattern
        let angle = (days as f64 / 365.0) * 2.0 * std::f64::consts::PI;
        let ndvi = 0.3 + 0.4 * angle.sin(); // NDVI values 0.3-0.7

        let data = Array3::from_elem((5, 5, 1), ndvi);
        ts.add_raster(metadata, data).expect("should add");
    }

    let config = PhenologyConfig::default();
    let metrics = PhenologyExtractor::extract(&ts, &config).expect("should extract");

    // Check that metrics were computed
    assert!(metrics.amplitude[[2, 2, 0]] > 0.0);
}

#[test]
fn test_integration_compositing_and_change_detection() {
    let ts = create_test_timeseries(20);

    // Create composite for first half
    let first_half = {
        let mut ts_first = TimeSeriesRaster::new();
        for idx in 0..10 {
            let entry = ts.get_by_index(idx).expect("should exist");
            if let Some(data) = &entry.data {
                ts_first
                    .add_raster(entry.metadata.clone(), data.clone())
                    .expect("should add");
            }
        }
        ts_first
    };

    // Create composite for second half
    let second_half = {
        let mut ts_second = TimeSeriesRaster::new();
        for idx in 10..20 {
            let entry = ts.get_by_index(idx).expect("should exist");
            if let Some(data) = &entry.data {
                ts_second
                    .add_raster(entry.metadata.clone(), data.clone())
                    .expect("should add");
            }
        }
        ts_second
    };

    let config = CompositingConfig {
        method: CompositingMethod::Mean,
        ..Default::default()
    };

    let composite1 = TemporalCompositor::composite(&first_half, &config).expect("should composite");
    let composite2 =
        TemporalCompositor::composite(&second_half, &config).expect("should composite");

    // Compare composites - should show increase
    for i in 0..10 {
        for j in 0..10 {
            for k in 0..2 {
                assert!(composite2.data[[i, j, k]] >= composite1.data[[i, j, k]]);
            }
        }
    }
}

#[test]
fn test_comprehensive_workflow() {
    // Create time series
    let mut ts = create_test_timeseries(30);

    // Filter by quality
    ts.filter_by_cloud_cover(25.0).expect("should filter");

    // Detect gaps
    let resolution = TemporalResolution::Daily;
    ts.set_resolution(resolution);
    let _gaps = ts.detect_gaps().expect("should detect gaps");
    // Gaps may or may not exist depending on filtering

    // Create stack
    let stack = RasterStack::from_timeseries(&ts).expect("should create stack");

    // Compute temporal statistics
    let mean = stack.mean_temporal().expect("should compute mean");
    let std = stack.std_temporal().expect("should compute std");

    // Detect changes
    let change_config = ChangeDetectionConfig::default();
    let changes = ChangeDetector::detect_stack(&stack, &change_config).expect("should detect");

    // Analyze trends
    let trend = TrendAnalyzer::analyze(&ts, TrendMethod::Linear).expect("should analyze");

    // All operations should complete successfully
    assert_eq!(mean.shape(), &[10, 10, 2]);
    assert_eq!(std.shape(), &[10, 10, 2]);
    assert_eq!(changes.magnitude.shape(), &[10, 10, 2]);
    assert_eq!(trend.slope.shape(), &[10, 10, 2]);
}
