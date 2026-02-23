//! Raster operations benchmarks using Criterion.
#![allow(missing_docs, clippy::expect_used, clippy::panic, clippy::unit_arg)]

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use std::time::Duration;

#[cfg(feature = "raster")]
fn bench_geotiff_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("geotiff_read");

    // Configure group settings
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(10));

    // Benchmark different tile sizes
    for tile_size in [256, 512, 1024].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(tile_size),
            tile_size,
            |b, &size| {
                b.iter(|| {
                    // Placeholder for actual GeoTIFF reading
                    // In real implementation, this would use oxigdal-geotiff
                    let _data = vec![0u8; size * size];
                    black_box(_data);
                });
            },
        );
    }

    group.finish();
}

#[cfg(feature = "raster")]
fn bench_geotiff_write(c: &mut Criterion) {
    let mut group = c.benchmark_group("geotiff_write");

    group.sample_size(10);
    group.measurement_time(Duration::from_secs(10));

    for size in [512, 1024, 2048].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let data = vec![0u16; size * size];
            b.iter(|| {
                // Placeholder for actual GeoTIFF writing
                black_box(&data);
            });
        });
    }

    group.finish();
}

#[cfg(feature = "raster")]
fn bench_raster_reprojection(c: &mut Criterion) {
    let mut group = c.benchmark_group("raster_reprojection");

    group.sample_size(5);
    group.measurement_time(Duration::from_secs(20));

    for size in [256, 512, 1024].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let data = vec![0.0f32; size * size];
            b.iter(|| {
                // Placeholder for actual reprojection
                black_box(&data);
            });
        });
    }

    group.finish();
}

#[cfg(feature = "raster")]
fn bench_compression_methods(c: &mut Criterion) {
    let mut group = c.benchmark_group("compression_methods");

    let data = vec![0u8; 1024 * 1024]; // 1MB of data

    for method in ["none", "lzw", "deflate", "zstd"].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(method), method, |b, _method| {
            b.iter(|| {
                // Placeholder for actual compression
                black_box(&data);
            });
        });
    }

    group.finish();
}

#[cfg(feature = "raster")]
criterion_group!(
    raster_benches,
    bench_geotiff_read,
    bench_geotiff_write,
    bench_raster_reprojection,
    bench_compression_methods
);

#[cfg(not(feature = "raster"))]
criterion_group!(raster_benches,);

criterion_main!(raster_benches);
