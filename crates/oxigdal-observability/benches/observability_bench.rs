//! Benchmarks for observability overhead.
#![allow(missing_docs, clippy::expect_used, clippy::panic, clippy::unit_arg)]

use criterion::{Criterion, criterion_group, criterion_main};
use opentelemetry::global;
use oxigdal_observability::metrics::GeoMetrics;
use std::hint::black_box;

fn bench_metric_collection(c: &mut Criterion) {
    let meter = global::meter("benchmark");
    let metrics = GeoMetrics::new(meter).expect("Failed to create metrics");

    c.bench_function("raster_read_metric", |b| {
        b.iter(|| {
            metrics.raster.record_read(
                black_box(10.0),
                black_box(1024),
                black_box("GeoTIFF"),
                black_box(true),
            );
        });
    });
}

fn bench_cache_metrics(c: &mut Criterion) {
    let meter = global::meter("benchmark");
    let metrics = GeoMetrics::new(meter).expect("Failed to create metrics");

    c.bench_function("cache_hit_metric", |b| {
        b.iter(|| {
            metrics.cache.record_hit(black_box("tile"), black_box(1024));
        });
    });
}

fn bench_anomaly_detection(c: &mut Criterion) {
    use chrono::Utc;
    use oxigdal_observability::anomaly::{AnomalyDetector, DataPoint, statistical::ZScoreDetector};

    let mut detector = ZScoreDetector::new(3.0);
    let baseline_data: Vec<DataPoint> = (0..100)
        .map(|i| DataPoint::new(Utc::now(), 10.0 + (i as f64 % 5.0)))
        .collect();

    detector
        .update_baseline(&baseline_data)
        .expect("Failed to update baseline");

    c.bench_function("zscore_detection", |b| {
        b.iter(|| {
            let test_data = vec![DataPoint::new(Utc::now(), black_box(15.0))];
            let _ = detector.detect(&test_data);
        });
    });
}

criterion_group!(
    benches,
    bench_metric_collection,
    bench_cache_metrics,
    bench_anomaly_detection
);
criterion_main!(benches);
