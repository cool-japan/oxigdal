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
                    // Simulate reading a tiled GeoTIFF: parse tile headers + decode data
                    let tile_data: Vec<u16> = (0..(size * size))
                        .map(|i| ((i * 7 + 13) % 65535) as u16)
                        .collect();
                    // Simulate per-tile statistics that would happen during read
                    let sum: u64 = tile_data.iter().map(|&v| v as u64).sum();
                    black_box(sum)
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
                // Simulate GeoTIFF write: encode data to simulated byte stream
                let encoded: Vec<u8> = data.iter().flat_map(|&v| v.to_le_bytes()).collect();
                black_box(encoded.len())
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
                // Simulate bilinear resampling during reprojection
                let reprojected: Vec<f32> = data
                    .iter()
                    .enumerate()
                    .map(|(i, &v)| {
                        let x = (i % size) as f32;
                        let y = (i / size) as f32;
                        // Bilinear weight simulation
                        v * (0.5_f32 + 0.5_f32 * (x / size as f32).sin() * (y / size as f32).cos())
                    })
                    .collect();
                black_box(reprojected.len())
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
        group.bench_with_input(BenchmarkId::from_parameter(method), method, |b, method| {
            b.iter(|| {
                // Simulate different compression algorithms with varying complexity
                let compressed: Vec<u8> = match *method {
                    "lzw" => {
                        // LZW-like: pack 2 consecutive bytes
                        data.chunks(2)
                            .flat_map(|c| {
                                if c.len() == 2 && c[0] == c[1] {
                                    vec![0xFE, c[0]]
                                } else {
                                    c.to_vec()
                                }
                            })
                            .collect()
                    }
                    "deflate" | "zstd" => {
                        // Simulate deflate: XOR-based byte mixing
                        data.iter()
                            .enumerate()
                            .map(|(i, &b)| b ^ (i as u8))
                            .collect()
                    }
                    _ => data.clone(), // "none"
                };
                black_box(compressed.len())
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
