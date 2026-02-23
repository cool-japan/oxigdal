//! Benchmarks for geospatial algorithms
//!
//! This benchmark suite measures the performance of:
//! - Resampling algorithms (Nearest, Bilinear, Bicubic, Lanczos)
//! - Upsampling and downsampling operations
//! - Different data types and buffer sizes
//! - Edge cases (small, medium, large rasters)
//!
//! Tests various scenarios:
//! - Small rasters: 256x256 pixels
//! - Medium rasters: 1024x1024 pixels
//! - Large rasters: 4096x4096 pixels
#![allow(missing_docs, clippy::expect_used, clippy::panic, clippy::unit_arg)]

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;
use oxigdal_algorithms::resampling::{
    BicubicResampler, BilinearResampler, LanczosResampler, NearestResampler, ResamplingMethod,
    Resampler,
};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;

/// Generate a test raster with realistic data patterns
fn generate_test_raster(width: u64, height: u64, dtype: RasterDataType) -> RasterBuffer {
    let mut buffer = RasterBuffer::zeros(width, height, dtype);

    // Create a realistic gradient pattern with some noise
    for y in 0..height {
        for x in 0..width {
            let gradient_x = (x as f64) / (width as f64);
            let gradient_y = (y as f64) / (height as f64);

            // Combine gradients and add some pseudo-random variation
            let noise = ((x * 13 + y * 17) % 256) as f64 / 256.0;
            let value = ((gradient_x + gradient_y) / 2.0 + noise * 0.1) * 255.0;

            buffer.set_pixel(x, y, value).ok();
        }
    }

    buffer
}

fn bench_nearest_neighbor(c: &mut Criterion) {
    let mut group = c.benchmark_group("algorithms/resampling/nearest");

    let test_cases = vec![
        // (src_size, dst_size, label)
        ((256, 256), (128, 128), "downsample_2x"),
        ((256, 256), (512, 512), "upsample_2x"),
        ((1024, 1024), (512, 512), "downsample_large_2x"),
        ((1024, 1024), (2048, 2048), "upsample_large_2x"),
        ((256, 256), (64, 64), "downsample_4x"),
        ((256, 256), (1024, 1024), "upsample_4x"),
    ];

    for ((src_w, src_h), (dst_w, dst_h), label) in test_cases {
        let src = generate_test_raster(src_w, src_h, RasterDataType::Float32);
        let pixel_count = dst_w * dst_h;

        group.throughput(Throughput::Elements(pixel_count as u64));
        group.bench_with_input(
            BenchmarkId::new(label, format!("{}x{}_to_{}x{}", src_w, src_h, dst_w, dst_h)),
            &(src, dst_w, dst_h),
            |b, (src, dst_w, dst_h)| {
                let resampler = NearestResampler;
                b.iter(|| {
                    black_box(resampler.resample(black_box(src), *dst_w, *dst_h).ok());
                });
            },
        );
    }

    group.finish();
}

fn bench_bilinear(c: &mut Criterion) {
    let mut group = c.benchmark_group("algorithms/resampling/bilinear");

    let test_cases = vec![
        ((256, 256), (128, 128), "downsample_2x"),
        ((256, 256), (512, 512), "upsample_2x"),
        ((1024, 1024), (512, 512), "downsample_large_2x"),
        ((1024, 1024), (2048, 2048), "upsample_large_2x"),
    ];

    for ((src_w, src_h), (dst_w, dst_h), label) in test_cases {
        let src = generate_test_raster(src_w, src_h, RasterDataType::Float32);
        let pixel_count = dst_w * dst_h;

        group.throughput(Throughput::Elements(pixel_count as u64));
        group.bench_with_input(
            BenchmarkId::new(label, format!("{}x{}_to_{}x{}", src_w, src_h, dst_w, dst_h)),
            &(src, dst_w, dst_h),
            |b, (src, dst_w, dst_h)| {
                let resampler = BilinearResampler;
                b.iter(|| {
                    black_box(resampler.resample(black_box(src), *dst_w, *dst_h).ok());
                });
            },
        );
    }

    group.finish();
}

fn bench_bicubic(c: &mut Criterion) {
    let mut group = c.benchmark_group("algorithms/resampling/bicubic");

    let test_cases = vec![
        ((256, 256), (128, 128), "downsample_2x"),
        ((256, 256), (512, 512), "upsample_2x"),
        ((1024, 1024), (512, 512), "downsample_large_2x"),
    ];

    for ((src_w, src_h), (dst_w, dst_h), label) in test_cases {
        let src = generate_test_raster(src_w, src_h, RasterDataType::Float32);
        let pixel_count = dst_w * dst_h;

        group.throughput(Throughput::Elements(pixel_count as u64));
        group.bench_with_input(
            BenchmarkId::new(label, format!("{}x{}_to_{}x{}", src_w, src_h, dst_w, dst_h)),
            &(src, dst_w, dst_h),
            |b, (src, dst_w, dst_h)| {
                let resampler = BicubicResampler::new();
                b.iter(|| {
                    black_box(resampler.resample(black_box(src), *dst_w, *dst_h).ok());
                });
            },
        );
    }

    group.finish();
}

fn bench_lanczos(c: &mut Criterion) {
    let mut group = c.benchmark_group("algorithms/resampling/lanczos");

    let test_cases = vec![
        ((256, 256), (128, 128), "downsample_2x"),
        ((256, 256), (512, 512), "upsample_2x"),
        ((1024, 1024), (512, 512), "downsample_large_2x"),
    ];

    for ((src_w, src_h), (dst_w, dst_h), label) in test_cases {
        let src = generate_test_raster(src_w, src_h, RasterDataType::Float32);
        let pixel_count = dst_w * dst_h;

        group.throughput(Throughput::Elements(pixel_count as u64));
        group.bench_with_input(
            BenchmarkId::new(label, format!("{}x{}_to_{}x{}", src_w, src_h, dst_w, dst_h)),
            &(src, dst_w, dst_h),
            |b, (src, dst_w, dst_h)| {
                let resampler = LanczosResampler::new(3);
                b.iter(|| {
                    black_box(resampler.resample(black_box(src), *dst_w, *dst_h).ok());
                });
            },
        );
    }

    group.finish();
}

fn bench_resampling_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("algorithms/resampling/comparison");

    let src_w = 512;
    let src_h = 512;
    let dst_w = 256;
    let dst_h = 256;

    let src = generate_test_raster(src_w, src_h, RasterDataType::Float32);
    let pixel_count = dst_w * dst_h;
    group.throughput(Throughput::Elements(pixel_count as u64));

    let methods = vec![
        ("nearest", ResamplingMethod::Nearest),
        ("bilinear", ResamplingMethod::Bilinear),
        ("bicubic", ResamplingMethod::Bicubic),
        ("lanczos", ResamplingMethod::Lanczos),
    ];

    for (name, method) in methods {
        group.bench_with_input(
            BenchmarkId::new("downsample_2x", name),
            &(src.clone(), method),
            |b, (src, method)| {
                let resampler = Resampler::new(*method);
                b.iter(|| {
                    black_box(resampler.resample(black_box(src), dst_w, dst_h).ok());
                });
            },
        );
    }

    group.finish();
}

fn bench_datatype_performance(c: &mut Criterion) {
    let mut group = c.benchmark_group("algorithms/resampling/datatype");

    let src_w = 512;
    let src_h = 512;
    let dst_w = 256;
    let dst_h = 256;
    let pixel_count = dst_w * dst_h;

    let datatypes = vec![
        ("uint8", RasterDataType::UInt8),
        ("uint16", RasterDataType::UInt16),
        ("int32", RasterDataType::Int32),
        ("float32", RasterDataType::Float32),
        ("float64", RasterDataType::Float64),
    ];

    for (name, dtype) in datatypes {
        let src = generate_test_raster(src_w, src_h, dtype);
        group.throughput(Throughput::Elements(pixel_count as u64));

        group.bench_with_input(
            BenchmarkId::new("bilinear", name),
            &src,
            |b, src| {
                let resampler = BilinearResampler;
                b.iter(|| {
                    black_box(resampler.resample(black_box(src), dst_w, dst_h).ok());
                });
            },
        );
    }

    group.finish();
}

fn bench_extreme_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("algorithms/resampling/extreme");

    // Extreme downsampling
    let large_src = generate_test_raster(2048, 2048, RasterDataType::Float32);
    group.throughput(Throughput::Elements(64 * 64));

    group.bench_function("downsample_32x", |b| {
        let resampler = BilinearResampler;
        b.iter(|| {
            black_box(resampler.resample(black_box(&large_src), 64, 64).ok());
        });
    });

    // Extreme upsampling
    let small_src = generate_test_raster(64, 64, RasterDataType::Float32);
    group.throughput(Throughput::Elements(2048 * 2048));

    group.bench_function("upsample_32x", |b| {
        let resampler = BilinearResampler;
        b.iter(|| {
            black_box(resampler.resample(black_box(&small_src), 2048, 2048).ok());
        });
    });

    group.finish();
}

fn bench_aspect_ratio_change(c: &mut Criterion) {
    let mut group = c.benchmark_group("algorithms/resampling/aspect_ratio");

    let test_cases = vec![
        ((512, 512), (1024, 256), "wide"),
        ((512, 512), (256, 1024), "tall"),
        ((1024, 256), (512, 512), "wide_to_square"),
        ((256, 1024), (512, 512), "tall_to_square"),
    ];

    for ((src_w, src_h), (dst_w, dst_h), label) in test_cases {
        let src = generate_test_raster(src_w, src_h, RasterDataType::Float32);
        let pixel_count = dst_w * dst_h;

        group.throughput(Throughput::Elements(pixel_count as u64));
        group.bench_with_input(
            BenchmarkId::new(label, format!("{}x{}_to_{}x{}", src_w, src_h, dst_w, dst_h)),
            &(src, dst_w, dst_h),
            |b, (src, dst_w, dst_h)| {
                let resampler = BilinearResampler;
                b.iter(|| {
                    black_box(resampler.resample(black_box(src), *dst_w, *dst_h).ok());
                });
            },
        );
    }

    group.finish();
}

fn bench_sequential_resampling(c: &mut Criterion) {
    let mut group = c.benchmark_group("algorithms/resampling/sequential");

    // Simulate pyramid building: repeatedly downsample by 2x
    let src = generate_test_raster(1024, 1024, RasterDataType::Float32);

    group.bench_function("pyramid_4_levels", |b| {
        let resampler = BilinearResampler;
        b.iter(|| {
            let mut current = black_box(src.clone());

            // Level 1: 512x512
            current = resampler
                .resample(&current, 512, 512)
                .expect("should resample");

            // Level 2: 256x256
            current = resampler
                .resample(&current, 256, 256)
                .expect("should resample");

            // Level 3: 128x128
            current = resampler
                .resample(&current, 128, 128)
                .expect("should resample");

            // Level 4: 64x64
            current = resampler
                .resample(&current, 64, 64)
                .expect("should resample");

            black_box(current);
        });
    });

    group.finish();
}

fn bench_small_rasters(c: &mut Criterion) {
    let mut group = c.benchmark_group("algorithms/resampling/small");

    // Test very small rasters (common for tiles)
    let test_cases = vec![
        ((32, 32), (64, 64), "32_to_64"),
        ((64, 64), (32, 32), "64_to_32"),
        ((16, 16), (256, 256), "16_to_256"),
        ((256, 256), (16, 16), "256_to_16"),
    ];

    for ((src_w, src_h), (dst_w, dst_h), label) in test_cases {
        let src = generate_test_raster(src_w, src_h, RasterDataType::Float32);
        let pixel_count = dst_w * dst_h;

        group.throughput(Throughput::Elements(pixel_count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(label),
            &(src, dst_w, dst_h),
            |b, (src, dst_w, dst_h)| {
                let resampler = BilinearResampler;
                b.iter(|| {
                    black_box(resampler.resample(black_box(src), *dst_w, *dst_h).ok());
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_nearest_neighbor,
    bench_bilinear,
    bench_bicubic,
    bench_lanczos,
    bench_resampling_comparison,
    bench_datatype_performance,
    bench_extreme_scaling,
    bench_aspect_ratio_change,
    bench_sequential_resampling,
    bench_small_rasters
);
criterion_main!(benches);
