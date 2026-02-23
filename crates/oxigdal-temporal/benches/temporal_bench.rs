//! Benchmarks for oxigdal-temporal
//!
//! Performance benchmarks for time series analysis, compositing, and change detection.
#![allow(missing_docs, clippy::expect_used)]

use chrono::{DateTime, NaiveDate};
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use oxigdal_temporal::{
    analysis::{
        anomaly::{AnomalyDetector, AnomalyMethod},
        forecast::{ForecastMethod, Forecaster},
        seasonality::{SeasonalityAnalyzer, SeasonalityMethod},
        trend::{TrendAnalyzer, TrendMethod},
    },
    change::detection::{ChangeDetectionConfig, ChangeDetectionMethod, ChangeDetector},
    compositing::{CompositingConfig, CompositingMethod, TemporalCompositor},
    gap_filling::{GapFillMethod, GapFiller},
    timeseries::{DataCube, TemporalMetadata, TimeSeriesRaster},
};
use scirs2_core::ndarray::Array3;
use std::hint::black_box;

fn create_test_timeseries(n_timesteps: usize, height: usize, width: usize) -> TimeSeriesRaster {
    let mut ts = TimeSeriesRaster::new();

    for i in 0..n_timesteps {
        let dt = DateTime::from_timestamp(1640995200 + i as i64 * 86400, 0).expect("valid");
        let date = NaiveDate::from_ymd_opt(2022, 1, 1 + i as u32).expect("valid");
        let metadata = TemporalMetadata::new(dt, date);

        let data = Array3::from_elem((height, width, 3), (i * 2) as f64);
        ts.add_raster(metadata, data).expect("should add");
    }

    ts
}

fn bench_trend_analysis(c: &mut Criterion) {
    let mut group = c.benchmark_group("trend_analysis");

    for size in [50, 100, 200].iter() {
        let ts = create_test_timeseries(20, *size, *size);

        group.bench_with_input(BenchmarkId::new("linear", size), &ts, |b, ts| {
            b.iter(|| {
                TrendAnalyzer::analyze(black_box(ts), TrendMethod::Linear).expect("should analyze")
            });
        });

        group.bench_with_input(BenchmarkId::new("sens_slope", size), &ts, |b, ts| {
            b.iter(|| {
                TrendAnalyzer::analyze(black_box(ts), TrendMethod::SensSlope)
                    .expect("should analyze")
            });
        });

        group.bench_with_input(BenchmarkId::new("mann_kendall", size), &ts, |b, ts| {
            b.iter(|| {
                TrendAnalyzer::analyze(black_box(ts), TrendMethod::MannKendall)
                    .expect("should analyze")
            });
        });
    }

    group.finish();
}

fn bench_compositing(c: &mut Criterion) {
    let mut group = c.benchmark_group("compositing");

    for size in [50, 100, 200].iter() {
        let ts = create_test_timeseries(20, *size, *size);

        group.bench_with_input(BenchmarkId::new("median", size), &ts, |b, ts| {
            let config = CompositingConfig {
                method: CompositingMethod::Median,
                ..Default::default()
            };
            b.iter(|| {
                TemporalCompositor::composite(black_box(ts), black_box(&config))
                    .expect("should composite")
            });
        });

        group.bench_with_input(BenchmarkId::new("mean", size), &ts, |b, ts| {
            let config = CompositingConfig {
                method: CompositingMethod::Mean,
                ..Default::default()
            };
            b.iter(|| {
                TemporalCompositor::composite(black_box(ts), black_box(&config))
                    .expect("should composite")
            });
        });
    }

    group.finish();
}

fn bench_change_detection(c: &mut Criterion) {
    let mut group = c.benchmark_group("change_detection");

    for size in [50, 100, 200].iter() {
        let ts = create_test_timeseries(20, *size, *size);

        group.bench_with_input(BenchmarkId::new("simple_difference", size), &ts, |b, ts| {
            let config = ChangeDetectionConfig {
                method: ChangeDetectionMethod::SimpleDifference,
                ..Default::default()
            };
            b.iter(|| {
                ChangeDetector::detect(black_box(ts), black_box(&config)).expect("should detect")
            });
        });

        group.bench_with_input(BenchmarkId::new("zscore", size), &ts, |b, ts| {
            let config = ChangeDetectionConfig {
                method: ChangeDetectionMethod::ZScore,
                ..Default::default()
            };
            b.iter(|| {
                ChangeDetector::detect(black_box(ts), black_box(&config)).expect("should detect")
            });
        });
    }

    group.finish();
}

fn bench_gap_filling(c: &mut Criterion) {
    let mut group = c.benchmark_group("gap_filling");

    for size in [50, 100].iter() {
        let ts = create_test_timeseries(20, *size, *size);

        group.bench_with_input(
            BenchmarkId::new("linear_interpolation", size),
            &ts,
            |b, ts| {
                b.iter(|| {
                    GapFiller::fill_gaps(black_box(ts), GapFillMethod::LinearInterpolation, None)
                        .expect("should fill")
                });
            },
        );

        group.bench_with_input(BenchmarkId::new("forward_fill", size), &ts, |b, ts| {
            b.iter(|| {
                GapFiller::fill_gaps(black_box(ts), GapFillMethod::ForwardFill, None)
                    .expect("should fill")
            });
        });
    }

    group.finish();
}

fn bench_seasonality(c: &mut Criterion) {
    let ts = create_test_timeseries(36, 100, 100);

    c.bench_function("seasonality_additive", |b| {
        b.iter(|| {
            SeasonalityAnalyzer::decompose(black_box(&ts), SeasonalityMethod::Additive, 12)
                .expect("should decompose")
        });
    });
}

fn bench_anomaly_detection(c: &mut Criterion) {
    let ts = create_test_timeseries(20, 100, 100);

    c.bench_function("anomaly_zscore", |b| {
        b.iter(|| {
            AnomalyDetector::detect(black_box(&ts), AnomalyMethod::ZScore, 2.0)
                .expect("should detect")
        });
    });

    c.bench_function("anomaly_iqr", |b| {
        b.iter(|| {
            AnomalyDetector::detect(black_box(&ts), AnomalyMethod::IQR, 1.5).expect("should detect")
        });
    });
}

fn bench_forecasting(c: &mut Criterion) {
    let ts = create_test_timeseries(20, 100, 100);

    c.bench_function("forecast_linear", |b| {
        b.iter(|| {
            Forecaster::forecast(black_box(&ts), ForecastMethod::LinearExtrapolation, 5, None)
                .expect("should forecast")
        });
    });
}

fn bench_datacube_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("datacube");

    let ts = create_test_timeseries(20, 100, 100);
    let datacube = DataCube::from_timeseries(&ts).expect("should convert");

    group.bench_function("from_timeseries", |b| {
        b.iter(|| DataCube::from_timeseries(black_box(&ts)).expect("should convert"));
    });

    group.bench_function("select_time_range", |b| {
        b.iter(|| {
            black_box(&datacube)
                .select_time_range(5, 15)
                .expect("should subset")
        });
    });

    group.bench_function("apply_temporal", |b| {
        b.iter(|| {
            black_box(&datacube)
                .apply_temporal(|values| values.iter().sum::<f64>() / values.len() as f64)
                .expect("should apply")
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_trend_analysis,
    bench_compositing,
    bench_change_detection,
    bench_gap_filling,
    bench_seasonality,
    bench_anomaly_detection,
    bench_forecasting,
    bench_datacube_operations,
);
criterion_main!(benches);
