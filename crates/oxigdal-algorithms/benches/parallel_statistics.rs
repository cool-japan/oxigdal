//! Benchmarks for parallel statistics operations
//!
//! This benchmark suite compares sequential vs. parallel implementations
//! of raster statistics operations, demonstrating speedups on large datasets.
#![allow(
    missing_docs,
    clippy::expect_used,
    clippy::panic,
    clippy::unit_arg,
    clippy::unnecessary_cast
)]

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxigdal_algorithms::raster::{
    Zone, compute_histogram, compute_percentiles, compute_statistics, compute_zonal_statistics,
};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;
use std::hint::black_box;

/// Creates a test raster with specified dimensions
fn create_test_raster(width: u64, height: u64) -> RasterBuffer {
    let mut raster = RasterBuffer::zeros(width, height, RasterDataType::Float32);

    // Fill with realistic elevation-like data
    for y in 0..height {
        for x in 0..width {
            let val = ((x as f64 * 0.1).sin() + (y as f64 * 0.1).cos()) * 100.0 + 500.0;
            raster.set_pixel(x, y, val).ok();
        }
    }

    raster
}

fn bench_statistics_small(c: &mut Criterion) {
    let mut group = c.benchmark_group("statistics_small");
    let size = (100, 100); // 10K pixels

    group.throughput(Throughput::Elements((size.0 * size.1) as u64));

    let raster = create_test_raster(size.0, size.1);

    group.bench_function("100x100", |bench| {
        bench.iter(|| {
            let _stats = compute_statistics(black_box(&raster)).ok();
        });
    });

    group.finish();
}

fn bench_statistics_medium(c: &mut Criterion) {
    let mut group = c.benchmark_group("statistics_medium");
    let size = (1000, 1000); // 1M pixels

    group.throughput(Throughput::Elements((size.0 * size.1) as u64));

    let raster = create_test_raster(size.0, size.1);

    group.bench_function("1000x1000", |bench| {
        bench.iter(|| {
            let _stats = compute_statistics(black_box(&raster)).ok();
        });
    });

    group.finish();
}

fn bench_statistics_large(c: &mut Criterion) {
    let mut group = c.benchmark_group("statistics_large");
    let size = (4000, 4000); // 16M pixels

    group.throughput(Throughput::Elements((size.0 * size.1) as u64));
    group.sample_size(10); // Reduce sample size for large datasets

    let raster = create_test_raster(size.0, size.1);

    group.bench_function("4000x4000", |bench| {
        bench.iter(|| {
            let _stats = compute_statistics(black_box(&raster)).ok();
        });
    });

    group.finish();
}

fn bench_percentiles_small(c: &mut Criterion) {
    let mut group = c.benchmark_group("percentiles_small");
    let size = (100, 100);

    group.throughput(Throughput::Elements((size.0 * size.1) as u64));

    let raster = create_test_raster(size.0, size.1);

    group.bench_function("100x100", |bench| {
        bench.iter(|| {
            let _perc = compute_percentiles(black_box(&raster)).ok();
        });
    });

    group.finish();
}

fn bench_percentiles_medium(c: &mut Criterion) {
    let mut group = c.benchmark_group("percentiles_medium");
    let size = (1000, 1000);

    group.throughput(Throughput::Elements((size.0 * size.1) as u64));

    let raster = create_test_raster(size.0, size.1);

    group.bench_function("1000x1000", |bench| {
        bench.iter(|| {
            let _perc = compute_percentiles(black_box(&raster)).ok();
        });
    });

    group.finish();
}

fn bench_percentiles_large(c: &mut Criterion) {
    let mut group = c.benchmark_group("percentiles_large");
    let size = (4000, 4000);

    group.throughput(Throughput::Elements((size.0 * size.1) as u64));
    group.sample_size(10);

    let raster = create_test_raster(size.0, size.1);

    group.bench_function("4000x4000", |bench| {
        bench.iter(|| {
            let _perc = compute_percentiles(black_box(&raster)).ok();
        });
    });

    group.finish();
}

fn bench_histogram_small(c: &mut Criterion) {
    let mut group = c.benchmark_group("histogram_small");
    let size = (100, 100);

    group.throughput(Throughput::Elements((size.0 * size.1) as u64));

    let raster = create_test_raster(size.0, size.1);

    for bins in [10, 50, 100, 256].iter() {
        group.bench_with_input(BenchmarkId::new("100x100", bins), bins, |bench, &bins| {
            bench.iter(|| {
                let _hist = compute_histogram(black_box(&raster), bins, None, None).ok();
            });
        });
    }

    group.finish();
}

fn bench_histogram_medium(c: &mut Criterion) {
    let mut group = c.benchmark_group("histogram_medium");
    let size = (1000, 1000);

    group.throughput(Throughput::Elements((size.0 * size.1) as u64));

    let raster = create_test_raster(size.0, size.1);

    for bins in [10, 50, 100, 256].iter() {
        group.bench_with_input(BenchmarkId::new("1000x1000", bins), bins, |bench, &bins| {
            bench.iter(|| {
                let _hist = compute_histogram(black_box(&raster), bins, None, None).ok();
            });
        });
    }

    group.finish();
}

fn bench_histogram_large(c: &mut Criterion) {
    let mut group = c.benchmark_group("histogram_large");
    let size = (4000, 4000);

    group.throughput(Throughput::Elements((size.0 * size.1) as u64));
    group.sample_size(10);

    let raster = create_test_raster(size.0, size.1);

    for bins in [10, 50, 100, 256].iter() {
        group.bench_with_input(BenchmarkId::new("4000x4000", bins), bins, |bench, &bins| {
            bench.iter(|| {
                let _hist = compute_histogram(black_box(&raster), bins, None, None).ok();
            });
        });
    }

    group.finish();
}

fn bench_zonal_statistics(c: &mut Criterion) {
    let mut group = c.benchmark_group("zonal_statistics");
    let size = (1000, 1000);

    let raster = create_test_raster(size.0, size.1);

    // Create test zones (4 quadrants)
    let zones = vec![
        Zone {
            id: 1,
            pixels: (0..500)
                .flat_map(|y| (0..500).map(move |x| (x, y)))
                .collect(),
        },
        Zone {
            id: 2,
            pixels: (0..500)
                .flat_map(|y| (500..1000).map(move |x| (x, y)))
                .collect(),
        },
        Zone {
            id: 3,
            pixels: (500..1000)
                .flat_map(|y| (0..500).map(move |x| (x, y)))
                .collect(),
        },
        Zone {
            id: 4,
            pixels: (500..1000)
                .flat_map(|y| (500..1000).map(move |x| (x, y)))
                .collect(),
        },
    ];

    group.throughput(Throughput::Elements((size.0 * size.1) as u64));

    group.bench_function("4_zones_1000x1000", |bench| {
        bench.iter(|| {
            let _zonal = compute_zonal_statistics(black_box(&raster), black_box(&zones)).ok();
        });
    });

    group.finish();
}

fn bench_memory_usage(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_usage");

    // Test memory consumption for different sizes
    for &size in &[100, 500, 1000, 2000] {
        let raster = create_test_raster(size, size);
        let pixels = (size * size) as u64;

        group.throughput(Throughput::Elements(pixels));

        group.bench_with_input(
            BenchmarkId::new("statistics", pixels),
            &raster,
            |bench, raster| {
                bench.iter(|| {
                    let _stats = compute_statistics(black_box(raster)).ok();
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    statistics,
    bench_statistics_small,
    bench_statistics_medium,
    bench_statistics_large
);

criterion_group!(
    percentiles,
    bench_percentiles_small,
    bench_percentiles_medium,
    bench_percentiles_large
);

criterion_group!(
    histogram,
    bench_histogram_small,
    bench_histogram_medium,
    bench_histogram_large
);

criterion_group!(zonal, bench_zonal_statistics);

criterion_group!(memory, bench_memory_usage);

criterion_main!(statistics, percentiles, histogram, zonal, memory);
