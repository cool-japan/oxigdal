//! Benchmarks for resampling algorithms
#![allow(
    missing_docs,
    clippy::expect_used,
    clippy::panic,
    clippy::unit_arg,
    clippy::unnecessary_cast
)]

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use oxigdal_algorithms::resampling::{
    BicubicResampler, BilinearResampler, LanczosResampler, NearestResampler, Resampler,
    ResamplingMethod,
};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;
use std::hint::black_box;

fn create_test_dem(size: u64) -> RasterBuffer {
    let mut buffer = RasterBuffer::zeros(size, size, RasterDataType::Float32);

    // Fill with synthetic elevation data (cone shape)
    let center = size as f64 / 2.0;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f64 - center;
            let dy = y as f64 - center;
            let dist = (dx * dx + dy * dy).sqrt();
            let elevation = ((size as f64 / 2.0) - dist).max(0.0);
            buffer.set_pixel(x, y, elevation).ok();
        }
    }

    buffer
}

fn bench_resampling_methods(c: &mut Criterion) {
    let mut group = c.benchmark_group("resampling_methods");

    let sizes = [128, 256, 512];

    for size in &sizes {
        let src = create_test_dem(*size);
        let dst_size = size / 2; // Downsample 2x

        group.bench_with_input(BenchmarkId::new("nearest", size), &src, |b, src| {
            let resampler = NearestResampler::new();
            b.iter(|| {
                black_box(resampler.resample(src, dst_size, dst_size).ok());
            });
        });

        group.bench_with_input(BenchmarkId::new("bilinear", size), &src, |b, src| {
            let resampler = BilinearResampler::new();
            b.iter(|| {
                black_box(resampler.resample(src, dst_size, dst_size).ok());
            });
        });

        group.bench_with_input(BenchmarkId::new("bicubic", size), &src, |b, src| {
            let resampler = BicubicResampler::new();
            b.iter(|| {
                black_box(resampler.resample(src, dst_size, dst_size).ok());
            });
        });

        group.bench_with_input(BenchmarkId::new("lanczos3", size), &src, |b, src| {
            let resampler = LanczosResampler::new(3);
            b.iter(|| {
                black_box(resampler.resample(src, dst_size, dst_size).ok());
            });
        });
    }

    group.finish();
}

fn bench_resampling_scales(c: &mut Criterion) {
    let mut group = c.benchmark_group("resampling_scales");

    let src = create_test_dem(256);

    let scales = [
        (128, "downsample_2x"),
        (512, "upsample_2x"),
        (1024, "upsample_4x"),
    ];

    for (dst_size, name) in &scales {
        group.bench_with_input(
            BenchmarkId::new("bilinear", name),
            dst_size,
            |b, &dst_size| {
                let resampler = Resampler::new(ResamplingMethod::Bilinear);
                b.iter(|| {
                    black_box(resampler.resample(&src, dst_size, dst_size).ok());
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_resampling_methods, bench_resampling_scales);
criterion_main!(benches);
