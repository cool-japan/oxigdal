//! Benchmarks for SIMD resampling operations
//!
//! This benchmark suite measures the performance of SIMD-optimized
//! image resampling algorithms (bilinear, bicubic, nearest neighbor).
#![allow(
    missing_docs,
    clippy::expect_used,
    clippy::panic,
    clippy::unit_arg,
    clippy::unnecessary_cast
)]

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxigdal_algorithms::simd::resampling::*;
use std::hint::black_box;

fn bench_bilinear_downsample(c: &mut Criterion) {
    let mut group = c.benchmark_group("bilinear_downsample");

    for (src_size, dst_size) in [(1000, 500), (2000, 1000), (4000, 2000)].iter() {
        let pixels = (src_size * src_size) as u64;
        group.throughput(Throughput::Elements(pixels));

        let src = vec![1.0_f32; src_size * src_size];
        let mut dst = vec![0.0_f32; dst_size * dst_size];

        group.bench_with_input(
            BenchmarkId::new(
                format!("{}x{}_to_{}x{}", src_size, src_size, dst_size, dst_size),
                pixels,
            ),
            &pixels,
            |bench, &_pixels| {
                bench.iter(|| {
                    bilinear_f32(
                        black_box(&src),
                        *src_size,
                        *src_size,
                        black_box(&mut dst),
                        *dst_size,
                        *dst_size,
                    )
                    .ok();
                });
            },
        );
    }

    group.finish();
}

fn bench_bilinear_upsample(c: &mut Criterion) {
    let mut group = c.benchmark_group("bilinear_upsample");

    for (src_size, dst_size) in [(500, 1000), (1000, 2000), (2000, 4000)].iter() {
        let pixels = (dst_size * dst_size) as u64;
        group.throughput(Throughput::Elements(pixels));

        let src = vec![1.0_f32; src_size * src_size];
        let mut dst = vec![0.0_f32; dst_size * dst_size];

        group.bench_with_input(
            BenchmarkId::new(
                format!("{}x{}_to_{}x{}", src_size, src_size, dst_size, dst_size),
                pixels,
            ),
            &pixels,
            |bench, &_pixels| {
                bench.iter(|| {
                    bilinear_f32(
                        black_box(&src),
                        *src_size,
                        *src_size,
                        black_box(&mut dst),
                        *dst_size,
                        *dst_size,
                    )
                    .ok();
                });
            },
        );
    }

    group.finish();
}

fn bench_bicubic_downsample(c: &mut Criterion) {
    let mut group = c.benchmark_group("bicubic_downsample");

    for (src_size, dst_size) in [(1000, 500), (2000, 1000)].iter() {
        let pixels = (src_size * src_size) as u64;
        group.throughput(Throughput::Elements(pixels));

        let src = vec![1.0_f32; src_size * src_size];
        let mut dst = vec![0.0_f32; dst_size * dst_size];

        group.bench_with_input(
            BenchmarkId::new(
                format!("{}x{}_to_{}x{}", src_size, src_size, dst_size, dst_size),
                pixels,
            ),
            &pixels,
            |bench, &_pixels| {
                bench.iter(|| {
                    bicubic_f32(
                        black_box(&src),
                        *src_size,
                        *src_size,
                        black_box(&mut dst),
                        *dst_size,
                        *dst_size,
                    )
                    .ok();
                });
            },
        );
    }

    group.finish();
}

fn bench_bicubic_upsample(c: &mut Criterion) {
    let mut group = c.benchmark_group("bicubic_upsample");

    for (src_size, dst_size) in [(500, 1000), (1000, 2000)].iter() {
        let pixels = (dst_size * dst_size) as u64;
        group.throughput(Throughput::Elements(pixels));

        let src = vec![1.0_f32; src_size * src_size];
        let mut dst = vec![0.0_f32; dst_size * dst_size];

        group.bench_with_input(
            BenchmarkId::new(
                format!("{}x{}_to_{}x{}", src_size, src_size, dst_size, dst_size),
                pixels,
            ),
            &pixels,
            |bench, &_pixels| {
                bench.iter(|| {
                    bicubic_f32(
                        black_box(&src),
                        *src_size,
                        *src_size,
                        black_box(&mut dst),
                        *dst_size,
                        *dst_size,
                    )
                    .ok();
                });
            },
        );
    }

    group.finish();
}

fn bench_nearest_downsample(c: &mut Criterion) {
    let mut group = c.benchmark_group("nearest_downsample");

    for (src_size, dst_size) in [(1000, 500), (2000, 1000), (4000, 2000)].iter() {
        let pixels = (src_size * src_size) as u64;
        group.throughput(Throughput::Elements(pixels));

        let src = vec![1.0_f32; src_size * src_size];
        let mut dst = vec![0.0_f32; dst_size * dst_size];

        group.bench_with_input(
            BenchmarkId::new(
                format!("{}x{}_to_{}x{}", src_size, src_size, dst_size, dst_size),
                pixels,
            ),
            &pixels,
            |bench, &_pixels| {
                bench.iter(|| {
                    nearest_f32(
                        black_box(&src),
                        *src_size,
                        *src_size,
                        black_box(&mut dst),
                        *dst_size,
                        *dst_size,
                    )
                    .ok();
                });
            },
        );
    }

    group.finish();
}

fn bench_downsample_average(c: &mut Criterion) {
    let mut group = c.benchmark_group("downsample_average");

    for (src_size, dst_size) in [(1000, 500), (2000, 1000), (4000, 2000)].iter() {
        let pixels = (src_size * src_size) as u64;
        group.throughput(Throughput::Elements(pixels));

        let src = vec![1.0_f32; src_size * src_size];
        let mut dst = vec![0.0_f32; dst_size * dst_size];

        group.bench_with_input(
            BenchmarkId::new(
                format!("{}x{}_to_{}x{}", src_size, src_size, dst_size, dst_size),
                pixels,
            ),
            &pixels,
            |bench, &_pixels| {
                bench.iter(|| {
                    downsample_average_f32(
                        black_box(&src),
                        *src_size,
                        *src_size,
                        black_box(&mut dst),
                        *dst_size,
                        *dst_size,
                    )
                    .ok();
                });
            },
        );
    }

    group.finish();
}

fn bench_resampling_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("resampling_comparison");

    let src_size = 1000;
    let dst_size = 500;
    let pixels = (src_size * src_size) as u64;

    group.throughput(Throughput::Elements(pixels));

    let src = vec![1.0_f32; src_size * src_size];
    let mut dst = vec![0.0_f32; dst_size * dst_size];

    group.bench_function("nearest", |bench| {
        bench.iter(|| {
            nearest_f32(
                black_box(&src),
                src_size,
                src_size,
                black_box(&mut dst),
                dst_size,
                dst_size,
            )
            .ok();
        });
    });

    group.bench_function("bilinear", |bench| {
        bench.iter(|| {
            bilinear_f32(
                black_box(&src),
                src_size,
                src_size,
                black_box(&mut dst),
                dst_size,
                dst_size,
            )
            .ok();
        });
    });

    group.bench_function("bicubic", |bench| {
        bench.iter(|| {
            bicubic_f32(
                black_box(&src),
                src_size,
                src_size,
                black_box(&mut dst),
                dst_size,
                dst_size,
            )
            .ok();
        });
    });

    group.bench_function("average", |bench| {
        bench.iter(|| {
            downsample_average_f32(
                black_box(&src),
                src_size,
                src_size,
                black_box(&mut dst),
                dst_size,
                dst_size,
            )
            .ok();
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_bilinear_downsample,
    bench_bilinear_upsample,
    bench_bicubic_downsample,
    bench_bicubic_upsample,
    bench_nearest_downsample,
    bench_downsample_average,
    bench_resampling_comparison
);
criterion_main!(benches);
