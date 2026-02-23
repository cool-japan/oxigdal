//! Benchmarks for SIMD statistics operations
//!
//! This benchmark suite measures the performance of SIMD-optimized
//! statistical operations on raster data.
#![allow(
    missing_docs,
    clippy::expect_used,
    clippy::panic,
    clippy::unit_arg,
    clippy::unnecessary_cast
)]

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxigdal_algorithms::simd::statistics::*;
use std::hint::black_box;

fn bench_sum_f32(c: &mut Criterion) {
    let mut group = c.benchmark_group("sum_f32");

    for size in [100, 1000, 10000, 100000, 1000000].iter() {
        group.throughput(Throughput::Elements(*size as u64));

        let data = vec![1.0_f32; *size];

        group.bench_with_input(BenchmarkId::from_parameter(size), size, |bench, &_size| {
            bench.iter(|| {
                let _sum = sum_f32(black_box(&data));
            });
        });
    }

    group.finish();
}

fn bench_mean_f32(c: &mut Criterion) {
    let mut group = c.benchmark_group("mean_f32");

    for size in [100, 1000, 10000, 100000, 1000000].iter() {
        group.throughput(Throughput::Elements(*size as u64));

        let data: Vec<f32> = (0..*size).map(|i| i as f32).collect();

        group.bench_with_input(BenchmarkId::from_parameter(size), size, |bench, &_size| {
            bench.iter(|| {
                let _mean = mean_f32(black_box(&data)).ok();
            });
        });
    }

    group.finish();
}

fn bench_minmax_f32(c: &mut Criterion) {
    let mut group = c.benchmark_group("minmax_f32");

    for size in [100, 1000, 10000, 100000, 1000000].iter() {
        group.throughput(Throughput::Elements(*size as u64));

        let data: Vec<f32> = (0..*size).map(|i| (i % 1000) as f32).collect();

        group.bench_with_input(BenchmarkId::from_parameter(size), size, |bench, &_size| {
            bench.iter(|| {
                let _minmax = minmax_f32(black_box(&data)).ok();
            });
        });
    }

    group.finish();
}

fn bench_variance_f32(c: &mut Criterion) {
    let mut group = c.benchmark_group("variance_f32");

    for size in [100, 1000, 10000, 100000].iter() {
        group.throughput(Throughput::Elements(*size as u64));

        let data: Vec<f32> = (0..*size).map(|i| i as f32).collect();

        group.bench_with_input(BenchmarkId::from_parameter(size), size, |bench, &_size| {
            bench.iter(|| {
                let _var = variance_f32(black_box(&data)).ok();
            });
        });
    }

    group.finish();
}

fn bench_std_dev_f32(c: &mut Criterion) {
    let mut group = c.benchmark_group("std_dev_f32");

    for size in [100, 1000, 10000, 100000].iter() {
        group.throughput(Throughput::Elements(*size as u64));

        let data: Vec<f32> = (0..*size).map(|i| i as f32).collect();

        group.bench_with_input(BenchmarkId::from_parameter(size), size, |bench, &_size| {
            bench.iter(|| {
                let _std = std_dev_f32(black_box(&data)).ok();
            });
        });
    }

    group.finish();
}

fn bench_histogram_f32(c: &mut Criterion) {
    let mut group = c.benchmark_group("histogram_f32");

    for size in [1000, 10000, 100000].iter() {
        group.throughput(Throughput::Elements(*size as u64));

        let data: Vec<f32> = (0..*size).map(|i| (i % 1000) as f32).collect();

        for bins in [10, 100, 256].iter() {
            group.bench_with_input(
                BenchmarkId::new(format!("bins_{}", bins), size),
                size,
                |bench, &_size| {
                    bench.iter(|| {
                        let _hist = histogram_f32(black_box(&data), *bins, 0.0, 1000.0).ok();
                    });
                },
            );
        }
    }

    group.finish();
}

fn bench_histogram_auto_f32(c: &mut Criterion) {
    let mut group = c.benchmark_group("histogram_auto_f32");

    for size in [1000, 10000, 100000].iter() {
        group.throughput(Throughput::Elements(*size as u64));

        let data: Vec<f32> = (0..*size).map(|i| (i % 1000) as f32).collect();

        group.bench_with_input(BenchmarkId::from_parameter(size), size, |bench, &_size| {
            bench.iter(|| {
                let _hist = histogram_auto_f32(black_box(&data), 256).ok();
            });
        });
    }

    group.finish();
}

fn bench_argmin_argmax_f32(c: &mut Criterion) {
    let mut group = c.benchmark_group("argmin_argmax_f32");

    for size in [100, 1000, 10000, 100000].iter() {
        group.throughput(Throughput::Elements(*size as u64));

        let data: Vec<f32> = (0..*size).map(|i| (i % 1000) as f32).collect();

        group.bench_with_input(BenchmarkId::new("argmin", size), size, |bench, &_size| {
            bench.iter(|| {
                let _idx = argmin_f32(black_box(&data)).ok();
            });
        });

        group.bench_with_input(BenchmarkId::new("argmax", size), size, |bench, &_size| {
            bench.iter(|| {
                let _idx = argmax_f32(black_box(&data)).ok();
            });
        });
    }

    group.finish();
}

fn bench_sum_f64(c: &mut Criterion) {
    let mut group = c.benchmark_group("sum_f64");

    for size in [100, 1000, 10000, 100000].iter() {
        group.throughput(Throughput::Elements(*size as u64));

        let data = vec![1.0_f64; *size];

        group.bench_with_input(BenchmarkId::from_parameter(size), size, |bench, &_size| {
            bench.iter(|| {
                let _sum = sum_f64(black_box(&data));
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_sum_f32,
    bench_mean_f32,
    bench_minmax_f32,
    bench_variance_f32,
    bench_std_dev_f32,
    bench_histogram_f32,
    bench_histogram_auto_f32,
    bench_argmin_argmax_f32,
    bench_sum_f64
);
criterion_main!(benches);
