//! Benchmarks for advanced SIMD operations
//!
//! This benchmark suite measures the performance of advanced SIMD modules:
//! - Projection (coordinate transformations)
//! - Filters (convolution operations)
//! - Colorspace (color space conversions)
//! - Histogram (histogram computation and analysis)
//! - Morphology (morphological operations)
//! - Threshold (thresholding operations)
#![allow(
    missing_docs,
    clippy::expect_used,
    clippy::panic,
    clippy::unit_arg,
    clippy::unnecessary_cast
)]

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxigdal_algorithms::simd::{colorspace, filters, histogram, morphology, projection, threshold};

// Benchmark coordinate projection
fn bench_projection(c: &mut Criterion) {
    let mut group = c.benchmark_group("projection");

    for size in [1_000, 10_000, 100_000] {
        let x = vec![0.0f64; size];
        let y = vec![0.0f64; size];
        let mut out_x = vec![0.0f64; size];
        let mut out_y = vec![0.0f64; size];

        group.throughput(Throughput::Elements(size as u64));

        // Affine transformation
        group.bench_with_input(BenchmarkId::new("affine_transform", size), &size, |b, _| {
            let matrix = projection::AffineMatrix2D::scale(2.0, 3.0);
            b.iter(|| {
                projection::affine_transform_2d(
                    black_box(&matrix),
                    black_box(&x),
                    black_box(&y),
                    black_box(&mut out_x),
                    black_box(&mut out_y),
                )
                .expect("affine transform benchmark failed");
            });
        });

        // Web Mercator projection
        let lon = vec![-122.0f64; size];
        let lat = vec![37.0f64; size];

        group.bench_with_input(
            BenchmarkId::new("latlon_to_web_mercator", size),
            &size,
            |b, _| {
                b.iter(|| {
                    projection::latlon_to_web_mercator(
                        black_box(&lon),
                        black_box(&lat),
                        black_box(&mut out_x),
                        black_box(&mut out_y),
                    )
                    .expect("Web Mercator projection benchmark failed");
                });
            },
        );

        // Degrees to radians
        let degrees = vec![45.0f64; size];
        let mut radians = vec![0.0f64; size];

        group.bench_with_input(
            BenchmarkId::new("degrees_to_radians", size),
            &size,
            |b, _| {
                b.iter(|| {
                    projection::degrees_to_radians(black_box(&degrees), black_box(&mut radians))
                        .expect("degrees to radians benchmark failed");
                });
            },
        );
    }

    group.finish();
}

// Benchmark convolution filters
fn bench_filters(c: &mut Criterion) {
    let mut group = c.benchmark_group("filters");

    for size in [100, 500, 1000] {
        let pixels = size * size;
        let input = vec![128u8; pixels];
        let mut output = vec![0u8; pixels];

        group.throughput(Throughput::Elements(pixels as u64));

        // Gaussian blur
        group.bench_with_input(
            BenchmarkId::new("gaussian_blur_3x3", size),
            &size,
            |b, _| {
                b.iter(|| {
                    filters::gaussian_blur_3x3(
                        black_box(&input),
                        black_box(&mut output),
                        size,
                        size,
                    )
                    .expect("Gaussian blur benchmark failed");
                });
            },
        );

        // Sobel X
        let mut output_i16 = vec![0i16; pixels];
        group.bench_with_input(BenchmarkId::new("sobel_x_3x3", size), &size, |b, _| {
            b.iter(|| {
                filters::sobel_x_3x3(black_box(&input), black_box(&mut output_i16), size, size)
                    .expect("Sobel X benchmark failed");
            });
        });

        // Box filter
        group.bench_with_input(BenchmarkId::new("box_filter_3x3", size), &size, |b, _| {
            b.iter(|| {
                filters::box_filter_3x3(black_box(&input), black_box(&mut output), size, size)
                    .expect("box filter benchmark failed");
            });
        });

        // Sharpen
        group.bench_with_input(BenchmarkId::new("sharpen_3x3", size), &size, |b, _| {
            b.iter(|| {
                filters::sharpen_3x3(black_box(&input), black_box(&mut output), size, size)
                    .expect("sharpen benchmark failed");
            });
        });
    }

    group.finish();
}

// Benchmark color space conversions
fn bench_colorspace(c: &mut Criterion) {
    let mut group = c.benchmark_group("colorspace");

    for size in [1_000, 10_000, 100_000] {
        let r = vec![255u8; size];
        let g = vec![128u8; size];
        let b_val = vec![64u8; size];
        let mut h = vec![0.0f32; size];
        let mut s = vec![0.0f32; size];
        let mut v = vec![0.0f32; size];

        group.throughput(Throughput::Elements(size as u64));

        // RGB to HSV
        group.bench_with_input(BenchmarkId::new("rgb_to_hsv", size), &size, |b, _| {
            b.iter(|| {
                colorspace::rgb_to_hsv(
                    black_box(&r),
                    black_box(&g),
                    black_box(&b_val),
                    black_box(&mut h),
                    black_box(&mut s),
                    black_box(&mut v),
                )
                .expect("RGB to HSV benchmark failed");
            });
        });

        // HSV to RGB
        let mut r_out = vec![0u8; size];
        let mut g_out = vec![0u8; size];
        let mut b_out = vec![0u8; size];

        group.bench_with_input(BenchmarkId::new("hsv_to_rgb", size), &size, |b, _| {
            b.iter(|| {
                colorspace::hsv_to_rgb(
                    black_box(&h),
                    black_box(&s),
                    black_box(&v),
                    black_box(&mut r_out),
                    black_box(&mut g_out),
                    black_box(&mut b_out),
                )
                .expect("HSV to RGB benchmark failed");
            });
        });

        // RGB to LAB
        let mut l = vec![0.0f32; size];
        let mut a = vec![0.0f32; size];
        let mut b_lab = vec![0.0f32; size];

        group.bench_with_input(BenchmarkId::new("rgb_to_lab", size), &size, |b, _| {
            b.iter(|| {
                colorspace::rgb_to_lab(
                    black_box(&r),
                    black_box(&g),
                    black_box(&b_val),
                    black_box(&mut l),
                    black_box(&mut a),
                    black_box(&mut b_lab),
                )
                .expect("RGB to LAB benchmark failed");
            });
        });

        // RGB to XYZ
        let mut x = vec![0.0f32; size];
        let mut y = vec![0.0f32; size];
        let mut z = vec![0.0f32; size];

        group.bench_with_input(BenchmarkId::new("rgb_to_xyz", size), &size, |b, _| {
            b.iter(|| {
                colorspace::rgb_to_xyz(
                    black_box(&r),
                    black_box(&g),
                    black_box(&b_val),
                    black_box(&mut x),
                    black_box(&mut y),
                    black_box(&mut z),
                )
                .expect("RGB to XYZ benchmark failed");
            });
        });
    }

    group.finish();
}

// Benchmark histogram operations
fn bench_histogram(c: &mut Criterion) {
    let mut group = c.benchmark_group("histogram");

    for size in [10_000, 100_000, 1_000_000] {
        let data = vec![128u8; size];

        group.throughput(Throughput::Elements(size as u64));

        // Histogram computation
        group.bench_with_input(BenchmarkId::new("histogram_u8", size), &size, |b, _| {
            b.iter(|| {
                histogram::histogram_u8(black_box(&data), 256).expect("histogram benchmark failed");
            });
        });

        // Histogram equalization
        let mut output = vec![0u8; size];
        group.bench_with_input(
            BenchmarkId::new("equalize_histogram", size),
            &size,
            |b, _| {
                b.iter(|| {
                    histogram::equalize_histogram(black_box(&data), black_box(&mut output))
                        .expect("equalize histogram benchmark failed");
                });
            },
        );

        // Otsu threshold
        group.bench_with_input(BenchmarkId::new("otsu_threshold", size), &size, |b, _| {
            b.iter(|| {
                threshold::otsu_threshold(black_box(&data))
                    .expect("Otsu threshold benchmark failed");
            });
        });

        // Histogram statistics (only for smaller sizes)
        if size <= 100_000 {
            let hist =
                histogram::histogram_u8(&data, 256).expect("histogram benchmark setup failed");
            group.bench_with_input(
                BenchmarkId::new("histogram_statistics", size),
                &size,
                |b, _| {
                    b.iter(|| {
                        histogram::histogram_statistics(black_box(&hist))
                            .expect("histogram statistics benchmark failed");
                    });
                },
            );
        }
    }

    group.finish();
}

// Benchmark morphological operations
fn bench_morphology(c: &mut Criterion) {
    let mut group = c.benchmark_group("morphology");

    for size in [100, 500, 1000] {
        let pixels = size * size;
        let input = vec![128u8; pixels];
        let mut output = vec![0u8; pixels];

        group.throughput(Throughput::Elements(pixels as u64));

        // Erosion
        group.bench_with_input(BenchmarkId::new("erode_3x3", size), &size, |b, _| {
            b.iter(|| {
                morphology::erode_3x3(black_box(&input), black_box(&mut output), size, size)
                    .expect("erode benchmark failed");
            });
        });

        // Dilation
        group.bench_with_input(BenchmarkId::new("dilate_3x3", size), &size, |b, _| {
            b.iter(|| {
                morphology::dilate_3x3(black_box(&input), black_box(&mut output), size, size)
                    .expect("dilate benchmark failed");
            });
        });

        // Opening
        group.bench_with_input(BenchmarkId::new("opening_3x3", size), &size, |b, _| {
            b.iter(|| {
                morphology::opening_3x3(black_box(&input), black_box(&mut output), size, size)
                    .expect("opening benchmark failed");
            });
        });

        // Closing
        group.bench_with_input(BenchmarkId::new("closing_3x3", size), &size, |b, _| {
            b.iter(|| {
                morphology::closing_3x3(black_box(&input), black_box(&mut output), size, size)
                    .expect("closing benchmark failed");
            });
        });

        // Morphological gradient
        group.bench_with_input(
            BenchmarkId::new("morphological_gradient_3x3", size),
            &size,
            |b, _| {
                b.iter(|| {
                    morphology::morphological_gradient_3x3(
                        black_box(&input),
                        black_box(&mut output),
                        size,
                        size,
                    )
                    .expect("morphological gradient benchmark failed");
                });
            },
        );

        // Top hat
        group.bench_with_input(BenchmarkId::new("top_hat_3x3", size), &size, |b, _| {
            b.iter(|| {
                morphology::top_hat_3x3(black_box(&input), black_box(&mut output), size, size)
                    .expect("top hat benchmark failed");
            });
        });
    }

    group.finish();
}

// Benchmark thresholding operations
fn bench_threshold(c: &mut Criterion) {
    let mut group = c.benchmark_group("threshold");

    for size in [10_000, 100_000, 1_000_000] {
        let data = vec![128u8; size];
        let mut output = vec![0u8; size];

        group.throughput(Throughput::Elements(size as u64));

        // Binary threshold
        group.bench_with_input(BenchmarkId::new("binary_threshold", size), &size, |b, _| {
            b.iter(|| {
                threshold::binary_threshold(black_box(&data), black_box(&mut output), 100, 255, 0)
                    .expect("binary threshold benchmark failed");
            });
        });

        // Threshold to zero
        group.bench_with_input(
            BenchmarkId::new("threshold_to_zero", size),
            &size,
            |b, _| {
                b.iter(|| {
                    threshold::threshold_to_zero(black_box(&data), black_box(&mut output), 100)
                        .expect("threshold to zero benchmark failed");
                });
            },
        );

        // Range threshold
        group.bench_with_input(BenchmarkId::new("threshold_range", size), &size, |b, _| {
            b.iter(|| {
                threshold::threshold_range(black_box(&data), black_box(&mut output), 50, 200)
                    .expect("threshold range benchmark failed");
            });
        });

        // Multi-threshold
        let thresholds = vec![64, 128, 192];
        let levels = vec![0, 85, 170, 255];
        group.bench_with_input(BenchmarkId::new("multi_threshold", size), &size, |b, _| {
            b.iter(|| {
                threshold::multi_threshold(
                    black_box(&data),
                    black_box(&mut output),
                    black_box(&thresholds),
                    black_box(&levels),
                )
                .expect("multi-threshold benchmark failed");
            });
        });

        // Otsu threshold
        group.bench_with_input(BenchmarkId::new("otsu_threshold", size), &size, |b, _| {
            b.iter(|| {
                threshold::otsu_threshold(black_box(&data))
                    .expect("Otsu threshold benchmark failed");
            });
        });
    }

    // Adaptive thresholding (only for image-sized data)
    let size = 500;
    let pixels = size * size;
    let data = vec![128u8; pixels];
    let mut output = vec![0u8; pixels];

    group.throughput(Throughput::Elements(pixels as u64));

    group.bench_with_input(
        BenchmarkId::new("adaptive_threshold_mean", size),
        &size,
        |b, _| {
            b.iter(|| {
                threshold::adaptive_threshold_mean(
                    black_box(&data),
                    black_box(&mut output),
                    size,
                    size,
                    11,
                    5,
                )
                .expect("adaptive threshold benchmark failed");
            });
        },
    );

    group.finish();
}

criterion_group!(
    benches,
    bench_projection,
    bench_filters,
    bench_colorspace,
    bench_histogram,
    bench_morphology,
    bench_threshold
);
criterion_main!(benches);
