//! Benchmarks for SIMD raster operations
//!
//! This benchmark suite compares SIMD-optimized raster operations
//! against scalar implementations to measure performance improvements.
#![allow(
    missing_docs,
    clippy::expect_used,
    clippy::panic,
    clippy::unit_arg,
    clippy::unnecessary_cast
)]

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxigdal_algorithms::simd::raster::*;
use std::hint::black_box;

fn bench_add_f32(c: &mut Criterion) {
    let mut group = c.benchmark_group("add_f32");

    for size in [100, 1000, 10000, 100000].iter() {
        group.throughput(Throughput::Elements(*size as u64));

        let a = vec![1.0_f32; *size];
        let b = vec![2.0_f32; *size];
        let mut out = vec![0.0_f32; *size];

        group.bench_with_input(BenchmarkId::from_parameter(size), size, |bench, &_size| {
            bench.iter(|| {
                add_f32(black_box(&a), black_box(&b), black_box(&mut out)).ok();
            });
        });
    }

    group.finish();
}

fn bench_mul_f32(c: &mut Criterion) {
    let mut group = c.benchmark_group("mul_f32");

    for size in [100, 1000, 10000, 100000].iter() {
        group.throughput(Throughput::Elements(*size as u64));

        let a = vec![1.5_f32; *size];
        let b = vec![2.5_f32; *size];
        let mut out = vec![0.0_f32; *size];

        group.bench_with_input(BenchmarkId::from_parameter(size), size, |bench, &_size| {
            bench.iter(|| {
                mul_f32(black_box(&a), black_box(&b), black_box(&mut out)).ok();
            });
        });
    }

    group.finish();
}

fn bench_fma_f32(c: &mut Criterion) {
    let mut group = c.benchmark_group("fma_f32");

    for size in [100, 1000, 10000, 100000].iter() {
        group.throughput(Throughput::Elements(*size as u64));

        let a = vec![1.5_f32; *size];
        let b = vec![2.5_f32; *size];
        let c_vec = vec![3.5_f32; *size];
        let mut out = vec![0.0_f32; *size];

        group.bench_with_input(BenchmarkId::from_parameter(size), size, |bench, &_size| {
            bench.iter(|| {
                fma_f32(
                    black_box(&a),
                    black_box(&b),
                    black_box(&c_vec),
                    black_box(&mut out),
                )
                .ok();
            });
        });
    }

    group.finish();
}

fn bench_min_max_f32(c: &mut Criterion) {
    let mut group = c.benchmark_group("min_max_f32");

    for size in [100, 1000, 10000, 100000].iter() {
        group.throughput(Throughput::Elements(*size as u64));

        let a = vec![1.0_f32; *size];
        let b = vec![2.0_f32; *size];
        let mut out = vec![0.0_f32; *size];

        group.bench_with_input(BenchmarkId::new("min", size), size, |bench, &_size| {
            bench.iter(|| {
                min_f32(black_box(&a), black_box(&b), black_box(&mut out)).ok();
            });
        });

        group.bench_with_input(BenchmarkId::new("max", size), size, |bench, &_size| {
            bench.iter(|| {
                max_f32(black_box(&a), black_box(&b), black_box(&mut out)).ok();
            });
        });
    }

    group.finish();
}

fn bench_clamp_f32(c: &mut Criterion) {
    let mut group = c.benchmark_group("clamp_f32");

    for size in [100, 1000, 10000, 100000].iter() {
        group.throughput(Throughput::Elements(*size as u64));

        let data: Vec<f32> = (0..*size).map(|i| i as f32 / 100.0).collect();
        let mut out = vec![0.0_f32; *size];

        group.bench_with_input(BenchmarkId::from_parameter(size), size, |bench, &_size| {
            bench.iter(|| {
                clamp_f32(black_box(&data), 0.0, 500.0, black_box(&mut out)).ok();
            });
        });
    }

    group.finish();
}

fn bench_threshold_f32(c: &mut Criterion) {
    let mut group = c.benchmark_group("threshold_f32");

    for size in [100, 1000, 10000, 100000].iter() {
        group.throughput(Throughput::Elements(*size as u64));

        let data: Vec<f32> = (0..*size).map(|i| i as f32 / 100.0).collect();
        let mut out = vec![0.0_f32; *size];

        group.bench_with_input(BenchmarkId::from_parameter(size), size, |bench, &_size| {
            bench.iter(|| {
                threshold_f32(black_box(&data), 50.0, black_box(&mut out)).ok();
            });
        });
    }

    group.finish();
}

fn bench_type_conversion(c: &mut Criterion) {
    let mut group = c.benchmark_group("type_conversion");

    for size in [100, 1000, 10000, 100000].iter() {
        group.throughput(Throughput::Elements(*size as u64));

        let u8_data = vec![128_u8; *size];
        let f32_data = vec![0.5_f32; *size];
        let mut u8_out = vec![0_u8; *size];
        let mut f32_out = vec![0.0_f32; *size];

        group.bench_with_input(
            BenchmarkId::new("u8_to_f32", size),
            size,
            |bench, &_size| {
                bench.iter(|| {
                    u8_to_f32_normalized(black_box(&u8_data), black_box(&mut f32_out)).ok();
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("f32_to_u8", size),
            size,
            |bench, &_size| {
                bench.iter(|| {
                    f32_to_u8_normalized(black_box(&f32_data), black_box(&mut u8_out)).ok();
                });
            },
        );
    }

    group.finish();
}

fn bench_scale_offset_f32(c: &mut Criterion) {
    let mut group = c.benchmark_group("scale_offset_f32");

    for size in [100, 1000, 10000, 100000].iter() {
        group.throughput(Throughput::Elements(*size as u64));

        let data: Vec<f32> = (0..*size).map(|i| i as f32).collect();
        let mut out = vec![0.0_f32; *size];

        group.bench_with_input(BenchmarkId::from_parameter(size), size, |bench, &_size| {
            bench.iter(|| {
                scale_offset_f32(black_box(&data), 2.0, 10.0, black_box(&mut out)).ok();
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_add_f32,
    bench_mul_f32,
    bench_fma_f32,
    bench_min_max_f32,
    bench_clamp_f32,
    bench_threshold_f32,
    bench_type_conversion,
    bench_scale_offset_f32
);
criterion_main!(benches);
