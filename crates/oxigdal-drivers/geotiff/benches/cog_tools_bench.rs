//! Benchmarks for COG tools and optimization
//!
//! This benchmarks various COG creation, optimization, and analysis operations.
#![allow(
    missing_docs,
    clippy::expect_used,
    clippy::panic,
    clippy::unit_arg,
    clippy::unnecessary_cast,
    clippy::needless_range_loop
)]

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;

use oxigdal_core::types::RasterDataType;
use oxigdal_geotiff::cog::{
    CompressionPreferences, OptimizationGoal, OverviewPreferences, analyze_for_cog,
    analyze_for_compression, optimize_overviews,
};
use oxigdal_geotiff::tiff::{Compression, PhotometricInterpretation};

/// Benchmarks compression analysis
fn bench_compression_analysis(c: &mut Criterion) {
    let mut group = c.benchmark_group("compression_analysis");

    for size in [256, 512, 1024, 2048].iter() {
        let width = *size;
        let height = *size;
        let data = vec![128u8; width * height];

        group.throughput(Throughput::Bytes((width * height) as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}x{}", width, height)),
            &(width, height, data),
            |b, (w, h, d)| {
                b.iter(|| {
                    analyze_for_compression(
                        black_box(d),
                        black_box(RasterDataType::UInt8),
                        black_box(*w),
                        black_box(*h),
                        black_box(1),
                        black_box(PhotometricInterpretation::BlackIsZero),
                        black_box(&CompressionPreferences::default()),
                    )
                });
            },
        );
    }

    group.finish();
}

/// Benchmarks overview optimization
fn bench_overview_optimization(c: &mut Criterion) {
    let mut group = c.benchmark_group("overview_optimization");

    for size in [1024, 2048, 4096, 8192].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}x{}", size, size)),
            size,
            |b, &size| {
                b.iter(|| {
                    optimize_overviews(
                        black_box(size as u64),
                        black_box(size as u64),
                        black_box(RasterDataType::UInt8),
                        black_box(PhotometricInterpretation::BlackIsZero),
                        black_box(Compression::Deflate),
                        black_box(&OverviewPreferences::default()),
                    )
                });
            },
        );
    }

    group.finish();
}

/// Benchmarks full COG analysis
fn bench_cog_analysis(c: &mut Criterion) {
    let mut group = c.benchmark_group("cog_analysis");

    for size in [512, 1024, 2048].iter() {
        let width = *size;
        let height = *size;
        let data = vec![128u8; width * height];

        group.throughput(Throughput::Bytes((width * height) as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}x{}", width, height)),
            &(width, height, data),
            |b, (w, h, d)| {
                b.iter(|| {
                    analyze_for_cog(
                        black_box(d),
                        black_box(*w as u64),
                        black_box(*h as u64),
                        black_box(RasterDataType::UInt8),
                        black_box(1),
                        black_box(PhotometricInterpretation::BlackIsZero),
                        black_box(OptimizationGoal::Balanced),
                        black_box(None),
                    )
                });
            },
        );
    }

    group.finish();
}

/// Benchmarks entropy calculation
fn bench_entropy_calculation(c: &mut Criterion) {
    let mut group = c.benchmark_group("entropy_calculation");

    for size in [1024, 4096, 16384, 65536].iter() {
        let data = vec![128u8; *size];

        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &data, |b, d| {
            b.iter(|| {
                // Inline entropy calculation for benchmarking
                let mut counts = [0u32; 256];
                for &byte in black_box(d) {
                    counts[byte as usize] += 1;
                }

                let total = d.len() as f64;
                let mut entropy = 0.0;

                for &count in &counts {
                    if count > 0 {
                        let p = count as f64 / total;
                        entropy -= p * p.log2();
                    }
                }

                black_box(entropy)
            });
        });
    }

    group.finish();
}

/// Benchmarks sparsity analysis
fn bench_sparsity_analysis(c: &mut Criterion) {
    let mut group = c.benchmark_group("sparsity_analysis");

    for sparsity_percent in [0, 25, 50, 75, 90].iter() {
        let size = 10000;
        let mut data = vec![128u8; size];

        // Create sparse data
        let zero_count = (size * sparsity_percent / 100) as usize;
        for i in 0..zero_count {
            data[i] = 0;
        }

        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}%_sparse", sparsity_percent)),
            &data,
            |b, d| {
                b.iter(|| {
                    let zero_count = d.iter().filter(|&&b| b == 0).count();
                    let sparsity_ratio = zero_count as f64 / d.len() as f64;
                    black_box(sparsity_ratio)
                });
            },
        );
    }

    group.finish();
}

/// Benchmarks smoothness analysis
fn bench_smoothness_analysis(c: &mut Criterion) {
    let mut group = c.benchmark_group("smoothness_analysis");

    for size in [64, 128, 256, 512].iter() {
        let width = *size;
        let height = *size;

        // Create smooth gradient data
        let mut data = Vec::with_capacity(width * height);
        for y in 0..height {
            for x in 0..width {
                let value = ((x + y) * 255 / (width + height)) as u8;
                data.push(value);
            }
        }

        group.throughput(Throughput::Bytes((width * height) as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}x{}", width, height)),
            &(width, height, data),
            |b, (w, h, d)| {
                b.iter(|| {
                    let mut total_diff = 0u64;
                    let mut sample_count = 0u64;

                    for y in (0..h - 1).step_by(8) {
                        let row_start = y * w;
                        let next_row_start = (y + 1) * w;

                        for x in (0..w - 1).step_by(8) {
                            let idx = row_start + x;
                            let next_idx = row_start + x + 1;
                            let below_idx = next_row_start + x;

                            let h_diff = (d[idx] as i16 - d[next_idx] as i16).unsigned_abs() as u64;
                            let v_diff =
                                (d[idx] as i16 - d[below_idx] as i16).unsigned_abs() as u64;

                            total_diff += h_diff + v_diff;
                            sample_count += 2;
                        }
                    }

                    let avg_diff = total_diff as f64 / sample_count as f64;
                    let smoothness = 1.0 - (avg_diff / 255.0).min(1.0);
                    black_box(smoothness)
                });
            },
        );
    }

    group.finish();
}

/// Benchmarks optimization goal comparison
fn bench_optimization_goals(c: &mut Criterion) {
    let mut group = c.benchmark_group("optimization_goals");

    let width = 1024;
    let height = 1024;
    let data = vec![128u8; width * height];

    let goals = [
        ("minimize_size", OptimizationGoal::MinimizeSize),
        ("minimize_latency", OptimizationGoal::MinimizeLatency),
        ("balanced", OptimizationGoal::Balanced),
        ("cloud_cost", OptimizationGoal::CloudCost),
        ("web_serving", OptimizationGoal::WebServing),
    ];

    for (name, goal) in goals.iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(name),
            &(width, height, &data, goal),
            |b, (w, h, d, g)| {
                b.iter(|| {
                    analyze_for_cog(
                        black_box(*d),
                        black_box(*w as u64),
                        black_box(*h as u64),
                        black_box(RasterDataType::UInt8),
                        black_box(1),
                        black_box(PhotometricInterpretation::BlackIsZero),
                        black_box(**g),
                        black_box(None),
                    )
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_compression_analysis,
    bench_overview_optimization,
    bench_cog_analysis,
    bench_entropy_calculation,
    bench_sparsity_analysis,
    bench_smoothness_analysis,
    bench_optimization_goals,
);
criterion_main!(benches);
