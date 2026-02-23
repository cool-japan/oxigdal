//! Comprehensive benchmarks for SIMD-optimized algorithms
//!
//! This benchmark suite measures the performance of SIMD-accelerated implementations
//! of focal, texture, terrain, hydrology, and cost-distance algorithms.
#![allow(
    missing_docs,
    clippy::expect_used,
    clippy::panic,
    clippy::unit_arg,
    clippy::unnecessary_cast
)]

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxigdal_algorithms::simd::{
    cost_distance_simd, focal_simd, hydrology_simd, terrain_simd, texture_simd,
};
use std::hint::black_box;

/// Benchmark focal statistics operations
fn bench_focal_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("focal_simd");

    for size in [100, 500, 1000] {
        let pixels = size * size;
        let src = vec![100.0_f32; pixels];
        let mut dst = vec![0.0_f32; pixels];

        group.throughput(Throughput::Elements(pixels as u64));

        // Focal mean (separable)
        group.bench_with_input(
            BenchmarkId::new("focal_mean_separable", size),
            &size,
            |b, &s| {
                b.iter(|| {
                    focal_simd::focal_mean_separable_simd(
                        black_box(&src),
                        black_box(&mut dst),
                        s,
                        s,
                        3,
                        3,
                    )
                    .expect("focal mean benchmark failed");
                });
            },
        );

        // Focal variance
        group.bench_with_input(BenchmarkId::new("focal_variance", size), &size, |b, &s| {
            b.iter(|| {
                focal_simd::focal_variance_simd(black_box(&src), black_box(&mut dst), s, s, 3)
                    .expect("focal variance benchmark failed");
            });
        });

        // Focal standard deviation
        group.bench_with_input(BenchmarkId::new("focal_stddev", size), &size, |b, &s| {
            b.iter(|| {
                focal_simd::focal_stddev_simd(black_box(&src), black_box(&mut dst), s, s, 3)
                    .expect("focal stddev benchmark failed");
            });
        });

        // Focal min/max
        let mut min_out = vec![0.0_f32; pixels];
        let mut max_out = vec![0.0_f32; pixels];

        group.bench_with_input(BenchmarkId::new("focal_min_max", size), &size, |b, &s| {
            b.iter(|| {
                focal_simd::focal_min_max_simd(
                    black_box(&src),
                    black_box(&mut min_out),
                    black_box(&mut max_out),
                    s,
                    s,
                    3,
                )
                .expect("focal min max benchmark failed");
            });
        });

        // Focal convolution
        let kernel = vec![1.0_f32 / 9.0; 9]; // 3x3 uniform kernel

        group.bench_with_input(BenchmarkId::new("focal_convolve", size), &size, |b, &s| {
            b.iter(|| {
                focal_simd::focal_convolve_simd(
                    black_box(&src),
                    black_box(&mut dst),
                    s,
                    s,
                    black_box(&kernel),
                    3,
                    3,
                    true,
                )
                .expect("focal convolve benchmark failed");
            });
        });
    }

    group.finish();
}

/// Benchmark texture analysis operations
fn bench_texture_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("texture_simd");

    for gray_levels in [8, 16, 32] {
        let size = 100;
        let pixels = size * size;
        let quantized = vec![0_u8; pixels];
        let mut glcm = vec![0.0_f32; gray_levels * gray_levels];

        group.throughput(Throughput::Elements(pixels as u64));

        // GLCM construction
        group.bench_with_input(
            BenchmarkId::new("glcm_construct", gray_levels),
            &gray_levels,
            |b, &gl| {
                b.iter(|| {
                    texture_simd::glcm_construct_simd(
                        black_box(&quantized),
                        black_box(&mut glcm),
                        size,
                        size,
                        gl,
                        1,
                        0,
                    )
                    .expect("GLCM construct benchmark failed");
                });
            },
        );

        // GLCM normalization
        let mut glcm_to_normalize = vec![1.0_f32; gray_levels * gray_levels];

        group.bench_with_input(
            BenchmarkId::new("glcm_normalize", gray_levels),
            &gray_levels,
            |b, &gl| {
                b.iter(|| {
                    texture_simd::glcm_normalize_simd(black_box(&mut glcm_to_normalize), gl)
                        .expect("GLCM normalize benchmark failed");
                });
            },
        );

        // Individual Haralick features
        let normalized_glcm =
            vec![1.0_f32 / (gray_levels * gray_levels) as f32; gray_levels * gray_levels];

        group.bench_with_input(
            BenchmarkId::new("texture_contrast", gray_levels),
            &gray_levels,
            |b, &gl| {
                b.iter(|| {
                    texture_simd::texture_contrast_simd(black_box(&normalized_glcm), gl)
                        .expect("texture contrast benchmark failed");
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("texture_energy", gray_levels),
            &gray_levels,
            |b, &gl| {
                b.iter(|| {
                    texture_simd::texture_energy_simd(black_box(&normalized_glcm), gl)
                        .expect("texture energy benchmark failed");
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("texture_entropy", gray_levels),
            &gray_levels,
            |b, &gl| {
                b.iter(|| {
                    texture_simd::texture_entropy_simd(black_box(&normalized_glcm), gl)
                        .expect("texture entropy benchmark failed");
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("texture_homogeneity", gray_levels),
            &gray_levels,
            |b, &gl| {
                b.iter(|| {
                    texture_simd::texture_homogeneity_simd(black_box(&normalized_glcm), gl)
                        .expect("texture homogeneity benchmark failed");
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("texture_correlation", gray_levels),
            &gray_levels,
            |b, &gl| {
                b.iter(|| {
                    texture_simd::texture_correlation_simd(black_box(&normalized_glcm), gl)
                        .expect("texture correlation benchmark failed");
                });
            },
        );

        // All Haralick features
        group.bench_with_input(
            BenchmarkId::new("haralick_features_complete", gray_levels),
            &gray_levels,
            |b, &gl| {
                b.iter(|| {
                    texture_simd::compute_haralick_features_simd(black_box(&normalized_glcm), gl)
                        .expect("Haralick features benchmark failed");
                });
            },
        );
    }

    group.finish();
}

/// Benchmark terrain analysis operations
fn bench_terrain_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("terrain_simd");

    for size in [100, 500, 1000] {
        let pixels = size * size;
        let dem = vec![100.0_f32; pixels];
        let mut output = vec![0.0_f32; pixels];

        group.throughput(Throughput::Elements(pixels as u64));

        // Slope
        group.bench_with_input(BenchmarkId::new("terrain_slope", size), &size, |b, &s| {
            b.iter(|| {
                terrain_simd::terrain_slope_simd(
                    black_box(&dem),
                    black_box(&mut output),
                    s,
                    s,
                    30.0,
                )
                .expect("terrain slope benchmark failed");
            });
        });

        // Aspect
        group.bench_with_input(BenchmarkId::new("terrain_aspect", size), &size, |b, &s| {
            b.iter(|| {
                terrain_simd::terrain_aspect_simd(
                    black_box(&dem),
                    black_box(&mut output),
                    s,
                    s,
                    30.0,
                )
                .expect("terrain aspect benchmark failed");
            });
        });

        // TPI (Topographic Position Index)
        group.bench_with_input(BenchmarkId::new("terrain_tpi", size), &size, |b, &s| {
            b.iter(|| {
                terrain_simd::terrain_tpi_simd(black_box(&dem), black_box(&mut output), s, s, 3)
                    .expect("terrain TPI benchmark failed");
            });
        });

        // TRI (Terrain Ruggedness Index)
        group.bench_with_input(BenchmarkId::new("terrain_tri", size), &size, |b, &s| {
            b.iter(|| {
                terrain_simd::terrain_tri_simd(black_box(&dem), black_box(&mut output), s, s)
                    .expect("terrain TRI benchmark failed");
            });
        });

        // Roughness
        group.bench_with_input(
            BenchmarkId::new("terrain_roughness", size),
            &size,
            |b, &s| {
                b.iter(|| {
                    terrain_simd::terrain_roughness_simd(
                        black_box(&dem),
                        black_box(&mut output),
                        s,
                        s,
                        3,
                    )
                    .expect("terrain roughness benchmark failed");
                });
            },
        );
    }

    group.finish();
}

/// Benchmark hydrology operations
fn bench_hydrology_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("hydrology_simd");

    for size in [100, 500, 1000] {
        let pixels = size * size;
        let dem = vec![100.0_f32; pixels];

        group.throughput(Throughput::Elements(pixels as u64));

        // Flow direction (D8)
        let mut flow_dir = vec![0_u8; pixels];

        group.bench_with_input(
            BenchmarkId::new("flow_direction_d8", size),
            &size,
            |b, &s| {
                b.iter(|| {
                    hydrology_simd::flow_direction_d8_simd(
                        black_box(&dem),
                        black_box(&mut flow_dir),
                        s,
                        s,
                    )
                    .expect("flow direction D8 benchmark failed");
                });
            },
        );

        // Sink detection
        let mut sinks = vec![0_u8; pixels];

        group.bench_with_input(BenchmarkId::new("detect_sinks", size), &size, |b, &s| {
            b.iter(|| {
                hydrology_simd::detect_sinks_simd(black_box(&dem), black_box(&mut sinks), s, s)
                    .expect("detect sinks benchmark failed");
            });
        });

        // Slope computation
        let mut slope = vec![0.0_f32; pixels];

        group.bench_with_input(BenchmarkId::new("compute_slope", size), &size, |b, &s| {
            b.iter(|| {
                hydrology_simd::compute_slope_simd(black_box(&dem), black_box(&mut slope), s, s)
                    .expect("compute slope benchmark failed");
            });
        });

        // Flow accumulation initialization
        let mut flow_acc = vec![0.0_f32; pixels];

        group.bench_with_input(
            BenchmarkId::new("init_flow_accumulation", size),
            &size,
            |b, _| {
                b.iter(|| {
                    hydrology_simd::initialize_flow_accumulation_simd(black_box(&mut flow_acc))
                        .expect("init flow accumulation benchmark failed");
                });
            },
        );

        // Flat detection
        let mut flat = vec![0_u8; pixels];

        group.bench_with_input(BenchmarkId::new("detect_flats", size), &size, |b, &s| {
            b.iter(|| {
                hydrology_simd::detect_flats_simd(black_box(&dem), black_box(&mut flat), s, s, 0.1)
                    .expect("detect flats benchmark failed");
            });
        });
    }

    group.finish();
}

/// Benchmark cost-distance operations
fn bench_cost_distance_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("cost_distance_simd");

    for size in [100, 500, 1000] {
        let pixels = size * size;
        let mut sources = vec![0_u8; pixels];
        sources[pixels / 2] = 1; // Single source in center

        let mut distance = vec![0.0_f32; pixels];

        group.throughput(Throughput::Elements(pixels as u64));

        // Euclidean distance
        group.bench_with_input(
            BenchmarkId::new("euclidean_distance", size),
            &size,
            |b, &s| {
                b.iter(|| {
                    cost_distance_simd::euclidean_distance_simd(
                        black_box(&sources),
                        black_box(&mut distance),
                        s,
                        s,
                        1.0,
                    )
                    .expect("Euclidean distance benchmark failed");
                });
            },
        );

        // Manhattan distance
        group.bench_with_input(
            BenchmarkId::new("manhattan_distance", size),
            &size,
            |b, &s| {
                b.iter(|| {
                    cost_distance_simd::manhattan_distance_simd(
                        black_box(&sources),
                        black_box(&mut distance),
                        s,
                        s,
                        1.0,
                    )
                    .expect("Manhattan distance benchmark failed");
                });
            },
        );

        // Chebyshev distance
        group.bench_with_input(
            BenchmarkId::new("chebyshev_distance", size),
            &size,
            |b, &s| {
                b.iter(|| {
                    cost_distance_simd::chebyshev_distance_simd(
                        black_box(&sources),
                        black_box(&mut distance),
                        s,
                        s,
                        1.0,
                    )
                    .expect("Chebyshev distance benchmark failed");
                });
            },
        );

        // Cost buffer initialization
        let mut buffer = vec![0.0_f32; pixels];

        group.bench_with_input(BenchmarkId::new("init_cost_buffer", size), &size, |b, _| {
            b.iter(|| {
                cost_distance_simd::initialize_cost_buffer_simd(
                    black_box(&mut buffer),
                    f32::INFINITY,
                )
                .expect("init cost buffer benchmark failed");
            });
        });

        // Neighbor costs computation (single cell)
        let cost_surface = vec![1.0_f32; pixels];
        let mut neighbor_costs = [0.0_f32; 8];

        group.bench_with_input(
            BenchmarkId::new("compute_neighbor_costs", size),
            &size,
            |b, &s| {
                b.iter(|| {
                    cost_distance_simd::compute_neighbor_costs_simd(
                        black_box(&cost_surface),
                        s,
                        s,
                        s / 2,
                        s / 2,
                        1.0,
                        black_box(&mut neighbor_costs),
                    )
                    .expect("compute neighbor costs benchmark failed");
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_focal_operations,
    bench_texture_operations,
    bench_terrain_operations,
    bench_hydrology_operations,
    bench_cost_distance_operations
);

criterion_main!(benches);
