//! Time series benchmarks
#![allow(missing_docs, clippy::expect_used, clippy::panic, clippy::unit_arg)]

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use oxigdal_analytics::timeseries::{AnomalyDetector, AnomalyMethod, TrendDetector, TrendMethod};
use scirs2_core::ndarray::Array1;
use std::hint::black_box;

fn generate_time_series(n: usize, trend: f64) -> Array1<f64> {
    use scirs2_core::random::thread_rng;
    let mut rng = thread_rng();
    let mut data = Vec::with_capacity(n);

    for i in 0..n {
        let noise = rng.gen_range(-1.0..1.0);
        data.push((i as f64) * trend + noise);
    }

    Array1::from_vec(data)
}

fn bench_mann_kendall(c: &mut Criterion) {
    let mut group = c.benchmark_group("mann_kendall");

    for n in [100, 500, 1000].iter() {
        let data = generate_time_series(*n, 0.1);

        group.bench_with_input(BenchmarkId::new("detect", n), &data, |b, data| {
            let detector = TrendDetector::new(TrendMethod::MannKendall, 0.05);
            b.iter(|| {
                detector
                    .detect(&black_box(data.view()))
                    .expect("trend detection should succeed in benchmark");
            });
        });
    }

    group.finish();
}

fn bench_anomaly_detection(c: &mut Criterion) {
    let mut group = c.benchmark_group("anomaly_detection");

    for n in [100, 500, 1000].iter() {
        let data = generate_time_series(*n, 0.0);

        group.bench_with_input(BenchmarkId::new("zscore", n), &data, |b, data| {
            let detector = AnomalyDetector::new(AnomalyMethod::ZScore, 3.0);
            b.iter(|| {
                detector
                    .detect(&black_box(data.view()))
                    .expect("z-score anomaly detection should succeed in benchmark");
            });
        });

        group.bench_with_input(BenchmarkId::new("iqr", n), &data, |b, data| {
            let detector = AnomalyDetector::new(AnomalyMethod::IQR, 1.5);
            b.iter(|| {
                detector
                    .detect(&black_box(data.view()))
                    .expect("IQR anomaly detection should succeed in benchmark");
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_mann_kendall, bench_anomaly_detection);
criterion_main!(benches);
