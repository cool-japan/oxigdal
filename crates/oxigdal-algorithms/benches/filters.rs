//! Comprehensive benchmarks for spatial filters, focusing on median filter optimizations
//!
//! This benchmark suite demonstrates the performance improvements from:
//! - Quickselect algorithm (O(n) vs O(n log n))
//! - Histogram-based median for byte data
//! - Memory optimization with buffer reuse
//! - Cache-friendly access patterns
#![allow(
    missing_docs,
    clippy::expect_used,
    clippy::panic,
    clippy::unit_arg,
    clippy::unnecessary_cast
)]

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxigdal_algorithms::raster::{gaussian_blur, median_filter};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;
use std::hint::black_box;

/// Creates a test raster with byte-range values (0-255) for histogram optimization testing
fn create_byte_raster(width: u64, height: u64) -> RasterBuffer {
    let mut buffer = RasterBuffer::zeros(width, height, RasterDataType::UInt8);

    for y in 0..height {
        for x in 0..width {
            let val = ((x + y * 17) % 256) as f64;
            let _ = buffer.set_pixel(x, y, val);
        }
    }

    buffer
}

/// Creates a test raster with floating-point values for generic quickselect testing
fn create_float_raster(width: u64, height: u64) -> RasterBuffer {
    let mut buffer = RasterBuffer::zeros(width, height, RasterDataType::Float32);

    for y in 0..height {
        for x in 0..width {
            let val = (x as f64).sin() * 100.0 + (y as f64).cos() * 50.0 + 1000.0;
            let _ = buffer.set_pixel(x, y, val);
        }
    }

    buffer
}

/// Creates a noisy raster with salt-and-pepper noise for realistic median filter testing
fn create_noisy_raster(width: u64, height: u64) -> RasterBuffer {
    let mut buffer = RasterBuffer::zeros(width, height, RasterDataType::UInt8);

    for y in 0..height {
        for x in 0..width {
            let base_val = 128.0;
            // Add noise: 10% salt (255), 10% pepper (0), rest normal
            let val = match (x + y * 17) % 10 {
                0 => 0.0,   // pepper
                1 => 255.0, // salt
                _ => base_val + ((x + y) % 20) as f64 - 10.0,
            };
            let _ = buffer.set_pixel(x, y, val);
        }
    }

    buffer
}

/// Benchmark median filter with various kernel sizes on byte data
fn bench_median_filter_kernel_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("median_filter_kernel_sizes");

    let size = 512;
    let raster = create_byte_raster(size, size);

    for kernel_size in [3, 5, 7, 11].iter() {
        group.throughput(Throughput::Elements((size * size) as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(kernel_size),
            kernel_size,
            |b, &ks| {
                b.iter(|| {
                    median_filter(black_box(&raster), black_box(ks))
                        .expect("Median filter should succeed")
                });
            },
        );
    }

    group.finish();
}

/// Benchmark median filter on different image sizes (byte data with histogram optimization)
fn bench_median_filter_byte_data(c: &mut Criterion) {
    let mut group = c.benchmark_group("median_filter_byte_data");

    for size in [128, 256, 512, 1024].iter() {
        let raster = create_byte_raster(*size, *size);

        group.throughput(Throughput::Elements((size * size) as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                median_filter(black_box(&raster), black_box(3))
                    .expect("Median filter should succeed")
            });
        });
    }

    group.finish();
}

/// Benchmark median filter on floating-point data (using quickselect)
fn bench_median_filter_float_data(c: &mut Criterion) {
    let mut group = c.benchmark_group("median_filter_float_data");

    for size in [128, 256, 512, 1024].iter() {
        let raster = create_float_raster(*size, *size);

        group.throughput(Throughput::Elements((size * size) as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                median_filter(black_box(&raster), black_box(3))
                    .expect("Median filter should succeed")
            });
        });
    }

    group.finish();
}

/// Benchmark median filter on noisy data (realistic salt-and-pepper noise removal)
fn bench_median_filter_noise_removal(c: &mut Criterion) {
    let mut group = c.benchmark_group("median_filter_noise_removal");

    let size = 512;
    let noisy_raster = create_noisy_raster(size, size);

    for kernel_size in [3, 5, 7].iter() {
        group.throughput(Throughput::Elements((size * size) as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(kernel_size),
            kernel_size,
            |b, &ks| {
                b.iter(|| {
                    median_filter(black_box(&noisy_raster), black_box(ks))
                        .expect("Median filter should succeed")
                });
            },
        );
    }

    group.finish();
}

/// Benchmark comparison: median filter vs gaussian blur
fn bench_filter_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("filter_comparison");

    let size = 512;
    let raster = create_byte_raster(size, size);

    group.throughput(Throughput::Elements((size * size) as u64));

    group.bench_function("median_3x3", |b| {
        b.iter(|| {
            median_filter(black_box(&raster), black_box(3)).expect("Median filter should succeed")
        });
    });

    group.bench_function("gaussian_sigma1.5", |b| {
        b.iter(|| {
            gaussian_blur(black_box(&raster), black_box(1.5), black_box(Some(5)))
                .expect("Gaussian blur should succeed")
        });
    });

    group.bench_function("median_5x5", |b| {
        b.iter(|| {
            median_filter(black_box(&raster), black_box(5)).expect("Median filter should succeed")
        });
    });

    group.bench_function("gaussian_sigma2.0", |b| {
        b.iter(|| {
            gaussian_blur(black_box(&raster), black_box(2.0), black_box(Some(7)))
                .expect("Gaussian blur should succeed")
        });
    });

    group.finish();
}

/// Benchmark median filter with varying data patterns
fn bench_median_filter_patterns(c: &mut Criterion) {
    let mut group = c.benchmark_group("median_filter_data_patterns");

    let size = 512;

    // Uniform data
    let mut uniform = RasterBuffer::zeros(size, size, RasterDataType::UInt8);
    for y in 0..size {
        for x in 0..size {
            let _ = uniform.set_pixel(x, y, 128.0);
        }
    }

    // Gradient data
    let mut gradient = RasterBuffer::zeros(size, size, RasterDataType::UInt8);
    for y in 0..size {
        for x in 0..size {
            let val = (x as f64 / size as f64) * 255.0;
            let _ = gradient.set_pixel(x, y, val);
        }
    }

    // Random-ish data
    let random = create_byte_raster(size, size);

    group.throughput(Throughput::Elements((size * size) as u64));

    group.bench_function("uniform_data", |b| {
        b.iter(|| {
            median_filter(black_box(&uniform), black_box(3)).expect("Median filter should succeed")
        });
    });

    group.bench_function("gradient_data", |b| {
        b.iter(|| {
            median_filter(black_box(&gradient), black_box(3)).expect("Median filter should succeed")
        });
    });

    group.bench_function("random_data", |b| {
        b.iter(|| {
            median_filter(black_box(&random), black_box(3)).expect("Median filter should succeed")
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_median_filter_kernel_sizes,
    bench_median_filter_byte_data,
    bench_median_filter_float_data,
    bench_median_filter_noise_removal,
    bench_filter_comparison,
    bench_median_filter_patterns,
);
criterion_main!(benches);
