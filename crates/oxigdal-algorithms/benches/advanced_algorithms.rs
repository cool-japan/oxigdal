//! Comprehensive benchmarks for advanced algorithms in oxigdal-algorithms
//!
//! This benchmark suite covers:
//! 1. Focal statistics benchmarks (scalar vs SIMD)
//! 2. Texture analysis benchmarks (GLCM and Haralick features)
//! 3. Hydrology benchmarks (flow direction, sink detection, watershed)
//! 4. Cost-distance benchmarks (Euclidean, Manhattan, Chebyshev)
//! 5. SIMD vs non-SIMD performance comparisons
//!
//! Run with: cargo bench --bench advanced_algorithms
#![allow(
    missing_docs,
    clippy::expect_used,
    clippy::panic,
    clippy::unit_arg,
    clippy::unnecessary_cast
)]

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxigdal_algorithms::raster::{
    FocalBoundaryMode as BoundaryMode,
    GlcmParams,
    TextureDirection,
    WindowShape,
    // Texture analysis
    compute_glcm,
    compute_haralick_features,
    // Focal operations
    focal_convolve,
    focal_mean,
    focal_mean_separable,
    focal_median,
    focal_range,
    focal_stddev,
};
use oxigdal_algorithms::simd::{cost_distance_simd, focal_simd, hydrology_simd, texture_simd};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;
use std::hint::black_box;

// ============================================================================
// Helper Functions
// ============================================================================

/// Creates synthetic DEM data with realistic elevation values
fn create_dem_data(width: usize, height: usize) -> Vec<f32> {
    (0..width * height)
        .map(|i| {
            let x = i % width;
            let y = i / width;
            // Create a mountain-like terrain with some variation
            let base = 1000.0_f32;
            let peak_x = width as f32 / 2.0;
            let peak_y = height as f32 / 2.0;
            let dist = ((x as f32 - peak_x).powi(2) + (y as f32 - peak_y).powi(2)).sqrt();
            let elevation = base + 500.0 * (-dist / 50.0).exp();
            // Add some noise
            elevation + ((x as f32 * 0.1).sin() + (y as f32 * 0.1).cos()) * 10.0
        })
        .collect()
}

/// Creates a RasterBuffer with synthetic DEM data
fn create_dem_raster(width: u64, height: u64) -> RasterBuffer {
    let mut raster = RasterBuffer::zeros(width, height, RasterDataType::Float64);
    let dem_data = create_dem_data(width as usize, height as usize);
    for (idx, &val) in dem_data.iter().enumerate() {
        let x = (idx % width as usize) as u64;
        let y = (idx / width as usize) as u64;
        let _ = raster.set_pixel(x, y, f64::from(val));
    }
    raster
}

/// Creates a source mask with a single source at the center
fn create_source_mask(width: usize, height: usize) -> Vec<u8> {
    let mut sources = vec![0_u8; width * height];
    let center = (height / 2) * width + (width / 2);
    sources[center] = 1;
    sources
}

/// Creates a source mask with multiple scattered sources
fn create_multi_source_mask(width: usize, height: usize, num_sources: usize) -> Vec<u8> {
    let mut sources = vec![0_u8; width * height];
    let step_x = width / (num_sources + 1);
    let step_y = height / (num_sources + 1);
    for i in 1..=num_sources {
        for j in 1..=num_sources {
            let idx = j * step_y * width + i * step_x;
            if idx < sources.len() {
                sources[idx] = 1;
            }
        }
    }
    sources
}

/// Creates quantized image data for GLCM
fn create_quantized_image(width: usize, height: usize, gray_levels: usize) -> Vec<u8> {
    (0..width * height)
        .map(|i| {
            let x = i % width;
            let y = i / width;
            // Create texture-like pattern
            let val = ((x as f32 / 10.0).sin().abs() + (y as f32 / 10.0).cos().abs())
                * gray_levels as f32
                / 2.0;
            val.min(gray_levels as f32 - 1.0) as u8
        })
        .collect()
}

// ============================================================================
// Focal Statistics Benchmarks
// ============================================================================

/// Benchmark focal mean operations - comparing different implementations
fn bench_focal_mean(c: &mut Criterion) {
    let mut group = c.benchmark_group("focal_mean");

    for size in [64, 128, 256, 512] {
        let raster = create_dem_raster(size, size);
        let pixels = (size * size) as u64;

        group.throughput(Throughput::Elements(pixels));

        // Benchmark generic focal mean with rectangular window
        let window = WindowShape::rectangular(3, 3).expect("valid window");
        group.bench_with_input(BenchmarkId::new("generic_3x3", size), &size, |b, _| {
            b.iter(|| {
                focal_mean(
                    black_box(&raster),
                    black_box(&window),
                    black_box(&BoundaryMode::Edge),
                )
            });
        });

        // Benchmark separable focal mean
        group.bench_with_input(BenchmarkId::new("separable_3x3", size), &size, |b, _| {
            b.iter(|| focal_mean_separable(black_box(&raster), black_box(3), black_box(3)));
        });

        // Benchmark SIMD separable focal mean
        let src_f32: Vec<f32> = (0..pixels)
            .map(|i| {
                let x = i % size;
                let y = i / size;
                (x as f32 * 0.01).sin() * 100.0 + (y as f32 * 0.01).cos() * 50.0 + 1000.0
            })
            .collect();
        let mut dst_f32 = vec![0.0_f32; pixels as usize];

        group.bench_with_input(
            BenchmarkId::new("simd_separable_3x3", size),
            &size,
            |b, _| {
                b.iter(|| {
                    focal_simd::focal_mean_separable_simd(
                        black_box(&src_f32),
                        black_box(&mut dst_f32),
                        size as usize,
                        size as usize,
                        3,
                        3,
                    )
                });
            },
        );

        // Larger window sizes
        if size >= 128 {
            let window_5x5 = WindowShape::rectangular(5, 5).expect("valid window");
            group.bench_with_input(BenchmarkId::new("generic_5x5", size), &size, |b, _| {
                b.iter(|| {
                    focal_mean(
                        black_box(&raster),
                        black_box(&window_5x5),
                        black_box(&BoundaryMode::Edge),
                    )
                });
            });

            group.bench_with_input(
                BenchmarkId::new("simd_separable_5x5", size),
                &size,
                |b, _| {
                    b.iter(|| {
                        focal_simd::focal_mean_separable_simd(
                            black_box(&src_f32),
                            black_box(&mut dst_f32),
                            size as usize,
                            size as usize,
                            5,
                            5,
                        )
                    });
                },
            );
        }
    }

    group.finish();
}

/// Benchmark focal variance and standard deviation
fn bench_focal_variance_stddev(c: &mut Criterion) {
    let mut group = c.benchmark_group("focal_variance_stddev");

    for size in [64, 128, 256, 512] {
        let raster = create_dem_raster(size, size);
        let pixels = (size * size) as u64;

        group.throughput(Throughput::Elements(pixels));

        // Generic focal stddev
        let window = WindowShape::rectangular(3, 3).expect("valid window");
        group.bench_with_input(
            BenchmarkId::new("generic_stddev_3x3", size),
            &size,
            |b, _| {
                b.iter(|| {
                    focal_stddev(
                        black_box(&raster),
                        black_box(&window),
                        black_box(&BoundaryMode::Edge),
                    )
                });
            },
        );

        // SIMD variance
        let src_f32: Vec<f32> = create_dem_data(size as usize, size as usize);
        let mut dst_f32 = vec![0.0_f32; pixels as usize];

        group.bench_with_input(
            BenchmarkId::new("simd_variance_3x3", size),
            &size,
            |b, _| {
                b.iter(|| {
                    focal_simd::focal_variance_simd(
                        black_box(&src_f32),
                        black_box(&mut dst_f32),
                        size as usize,
                        size as usize,
                        3,
                    )
                });
            },
        );

        // SIMD stddev
        group.bench_with_input(BenchmarkId::new("simd_stddev_3x3", size), &size, |b, _| {
            b.iter(|| {
                focal_simd::focal_stddev_simd(
                    black_box(&src_f32),
                    black_box(&mut dst_f32),
                    size as usize,
                    size as usize,
                    3,
                )
            });
        });
    }

    group.finish();
}

/// Benchmark focal min/max operations
fn bench_focal_min_max(c: &mut Criterion) {
    let mut group = c.benchmark_group("focal_min_max");

    for size in [64, 128, 256, 512] {
        let pixels = (size * size) as u64;
        let src_f32: Vec<f32> = create_dem_data(size as usize, size as usize);
        let mut min_out = vec![0.0_f32; pixels as usize];
        let mut max_out = vec![0.0_f32; pixels as usize];

        group.throughput(Throughput::Elements(pixels));

        // SIMD combined min/max
        group.bench_with_input(BenchmarkId::new("simd_min_max_3x3", size), &size, |b, _| {
            b.iter(|| {
                focal_simd::focal_min_max_simd(
                    black_box(&src_f32),
                    black_box(&mut min_out),
                    black_box(&mut max_out),
                    size as usize,
                    size as usize,
                    3,
                )
            });
        });

        // 5x5 window
        group.bench_with_input(BenchmarkId::new("simd_min_max_5x5", size), &size, |b, _| {
            b.iter(|| {
                focal_simd::focal_min_max_simd(
                    black_box(&src_f32),
                    black_box(&mut min_out),
                    black_box(&mut max_out),
                    size as usize,
                    size as usize,
                    5,
                )
            });
        });
    }

    group.finish();
}

/// Benchmark focal convolution operations
fn bench_focal_convolution(c: &mut Criterion) {
    let mut group = c.benchmark_group("focal_convolution");

    for size in [64, 128, 256, 512] {
        let raster = create_dem_raster(size, size);
        let pixels = (size * size) as u64;

        group.throughput(Throughput::Elements(pixels));

        // Gaussian kernel 3x3
        let gaussian_3x3 = vec![
            1.0 / 16.0,
            2.0 / 16.0,
            1.0 / 16.0,
            2.0 / 16.0,
            4.0 / 16.0,
            2.0 / 16.0,
            1.0 / 16.0,
            2.0 / 16.0,
            1.0 / 16.0,
        ];

        group.bench_with_input(
            BenchmarkId::new("generic_gaussian_3x3", size),
            &size,
            |b, _| {
                b.iter(|| {
                    focal_convolve(black_box(&raster), black_box(&gaussian_3x3), 3, 3, false)
                });
            },
        );

        // SIMD convolution
        let src_f32: Vec<f32> = create_dem_data(size as usize, size as usize);
        let mut dst_f32 = vec![0.0_f32; pixels as usize];
        let kernel_f32: Vec<f32> = gaussian_3x3.iter().map(|&x| x as f32).collect();

        group.bench_with_input(
            BenchmarkId::new("simd_convolve_3x3", size),
            &size,
            |b, _| {
                b.iter(|| {
                    focal_simd::focal_convolve_simd(
                        black_box(&src_f32),
                        black_box(&mut dst_f32),
                        size as usize,
                        size as usize,
                        black_box(&kernel_f32),
                        3,
                        3,
                        false,
                    )
                });
            },
        );

        // Sobel kernel for edge detection
        let sobel_x: Vec<f32> = vec![-1.0, 0.0, 1.0, -2.0, 0.0, 2.0, -1.0, 0.0, 1.0];

        group.bench_with_input(BenchmarkId::new("simd_sobel_3x3", size), &size, |b, _| {
            b.iter(|| {
                focal_simd::focal_convolve_simd(
                    black_box(&src_f32),
                    black_box(&mut dst_f32),
                    size as usize,
                    size as usize,
                    black_box(&sobel_x),
                    3,
                    3,
                    false,
                )
            });
        });
    }

    group.finish();
}

/// Benchmark focal median and range operations
fn bench_focal_median_range(c: &mut Criterion) {
    let mut group = c.benchmark_group("focal_median_range");

    for size in [64, 128, 256] {
        let raster = create_dem_raster(size, size);
        let pixels = (size * size) as u64;

        group.throughput(Throughput::Elements(pixels));

        let window = WindowShape::rectangular(3, 3).expect("valid window");

        // Focal median (computationally expensive due to sorting)
        group.bench_with_input(BenchmarkId::new("median_3x3", size), &size, |b, _| {
            b.iter(|| {
                focal_median(
                    black_box(&raster),
                    black_box(&window),
                    black_box(&BoundaryMode::Edge),
                )
            });
        });

        // Focal range
        group.bench_with_input(BenchmarkId::new("range_3x3", size), &size, |b, _| {
            b.iter(|| {
                focal_range(
                    black_box(&raster),
                    black_box(&window),
                    black_box(&BoundaryMode::Edge),
                )
            });
        });
    }

    group.finish();
}

// ============================================================================
// Texture Analysis Benchmarks
// ============================================================================

/// Benchmark GLCM construction
fn bench_glcm_construction(c: &mut Criterion) {
    let mut group = c.benchmark_group("glcm_construction");

    for gray_levels in [8, 16, 32, 64] {
        let size = 128;
        let pixels = size * size;

        group.throughput(Throughput::Elements(pixels as u64));

        // Generic GLCM
        let raster = create_dem_raster(size as u64, size as u64);
        let params = GlcmParams {
            gray_levels,
            normalize: true,
            symmetric: true,
            window_size: None,
        };

        group.bench_with_input(
            BenchmarkId::new("generic", gray_levels),
            &gray_levels,
            |b, _| {
                b.iter(|| {
                    compute_glcm(
                        black_box(&raster),
                        TextureDirection::Horizontal,
                        1,
                        black_box(&params),
                    )
                });
            },
        );

        // SIMD GLCM construction
        let quantized = create_quantized_image(size, size, gray_levels);
        let mut glcm = vec![0.0_f32; gray_levels * gray_levels];

        group.bench_with_input(
            BenchmarkId::new("simd_construct", gray_levels),
            &gray_levels,
            |b, _| {
                b.iter(|| {
                    texture_simd::glcm_construct_simd(
                        black_box(&quantized),
                        black_box(&mut glcm),
                        size,
                        size,
                        gray_levels,
                        1,
                        0,
                    )
                });
            },
        );
    }

    group.finish();
}

/// Benchmark GLCM normalization
fn bench_glcm_normalization(c: &mut Criterion) {
    let mut group = c.benchmark_group("glcm_normalization");

    for gray_levels in [8, 16, 32, 64, 128, 256] {
        let glcm_size = gray_levels * gray_levels;

        group.throughput(Throughput::Elements(glcm_size as u64));

        let mut glcm: Vec<f32> = (0..glcm_size).map(|i| (i % 100) as f32).collect();

        group.bench_with_input(
            BenchmarkId::new("simd_normalize", gray_levels),
            &gray_levels,
            |b, _| {
                b.iter(|| texture_simd::glcm_normalize_simd(black_box(&mut glcm), gray_levels));
            },
        );
    }

    group.finish();
}

/// Benchmark Haralick feature computation
fn bench_haralick_features(c: &mut Criterion) {
    let mut group = c.benchmark_group("haralick_features");

    for gray_levels in [8, 16, 32, 64] {
        let glcm_size = gray_levels * gray_levels;

        group.throughput(Throughput::Elements(glcm_size as u64));

        // Create a normalized GLCM
        let mut glcm: Vec<f32> = (0..glcm_size).map(|i| (i % 10 + 1) as f32).collect();
        let sum: f32 = glcm.iter().sum();
        for val in &mut glcm {
            *val /= sum;
        }

        // Individual features
        group.bench_with_input(
            BenchmarkId::new("contrast", gray_levels),
            &gray_levels,
            |b, _| {
                b.iter(|| texture_simd::texture_contrast_simd(black_box(&glcm), gray_levels));
            },
        );

        group.bench_with_input(
            BenchmarkId::new("energy", gray_levels),
            &gray_levels,
            |b, _| {
                b.iter(|| texture_simd::texture_energy_simd(black_box(&glcm), gray_levels));
            },
        );

        group.bench_with_input(
            BenchmarkId::new("entropy", gray_levels),
            &gray_levels,
            |b, _| {
                b.iter(|| texture_simd::texture_entropy_simd(black_box(&glcm), gray_levels));
            },
        );

        group.bench_with_input(
            BenchmarkId::new("homogeneity", gray_levels),
            &gray_levels,
            |b, _| {
                b.iter(|| texture_simd::texture_homogeneity_simd(black_box(&glcm), gray_levels));
            },
        );

        group.bench_with_input(
            BenchmarkId::new("correlation", gray_levels),
            &gray_levels,
            |b, _| {
                b.iter(|| texture_simd::texture_correlation_simd(black_box(&glcm), gray_levels));
            },
        );

        group.bench_with_input(
            BenchmarkId::new("dissimilarity", gray_levels),
            &gray_levels,
            |b, _| {
                b.iter(|| texture_simd::texture_dissimilarity_simd(black_box(&glcm), gray_levels));
            },
        );

        // All features combined
        group.bench_with_input(
            BenchmarkId::new("all_features", gray_levels),
            &gray_levels,
            |b, _| {
                b.iter(|| {
                    texture_simd::compute_haralick_features_simd(black_box(&glcm), gray_levels)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark texture feature computation for varying image sizes
fn bench_texture_image_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("texture_by_size");

    let gray_levels = 16;

    for size in [64, 128, 256, 512] {
        let pixels = size * size;

        group.throughput(Throughput::Elements(pixels as u64));

        // SIMD pipeline: construct + normalize + all features
        let quantized = create_quantized_image(size, size, gray_levels);
        let mut glcm = vec![0.0_f32; gray_levels * gray_levels];

        group.bench_with_input(BenchmarkId::new("full_pipeline", size), &size, |b, _| {
            b.iter(|| {
                texture_simd::glcm_construct_simd(
                    black_box(&quantized),
                    black_box(&mut glcm),
                    size,
                    size,
                    gray_levels,
                    1,
                    0,
                )
                .expect("construct");
                texture_simd::glcm_normalize_simd(black_box(&mut glcm), gray_levels)
                    .expect("normalize");
                texture_simd::compute_haralick_features_simd(black_box(&glcm), gray_levels)
            });
        });
    }

    group.finish();
}

// ============================================================================
// Hydrology Benchmarks
// ============================================================================

/// Benchmark D8 flow direction computation
fn bench_flow_direction(c: &mut Criterion) {
    let mut group = c.benchmark_group("flow_direction");

    for size in [64, 128, 256, 512] {
        let pixels = size * size;
        let dem: Vec<f32> = create_dem_data(size, size);
        let mut flow_dir = vec![0_u8; pixels];

        group.throughput(Throughput::Elements(pixels as u64));

        group.bench_with_input(BenchmarkId::new("d8_simd", size), &size, |b, _| {
            b.iter(|| {
                hydrology_simd::flow_direction_d8_simd(
                    black_box(&dem),
                    black_box(&mut flow_dir),
                    size,
                    size,
                )
            });
        });
    }

    group.finish();
}

/// Benchmark sink detection
fn bench_sink_detection(c: &mut Criterion) {
    let mut group = c.benchmark_group("sink_detection");

    for size in [64, 128, 256, 512] {
        let pixels = size * size;
        let dem: Vec<f32> = create_dem_data(size, size);
        let mut sinks = vec![0_u8; pixels];

        group.throughput(Throughput::Elements(pixels as u64));

        group.bench_with_input(BenchmarkId::new("simd", size), &size, |b, _| {
            b.iter(|| {
                hydrology_simd::detect_sinks_simd(
                    black_box(&dem),
                    black_box(&mut sinks),
                    size,
                    size,
                )
            });
        });
    }

    group.finish();
}

/// Benchmark slope computation for hydrology
fn bench_hydrology_slope(c: &mut Criterion) {
    let mut group = c.benchmark_group("hydrology_slope");

    for size in [64, 128, 256, 512] {
        let pixels = size * size;
        let dem: Vec<f32> = create_dem_data(size, size);
        let mut slope = vec![0.0_f32; pixels];

        group.throughput(Throughput::Elements(pixels as u64));

        group.bench_with_input(BenchmarkId::new("simd", size), &size, |b, _| {
            b.iter(|| {
                hydrology_simd::compute_slope_simd(
                    black_box(&dem),
                    black_box(&mut slope),
                    size,
                    size,
                )
            });
        });
    }

    group.finish();
}

/// Benchmark flat area detection
fn bench_flat_detection(c: &mut Criterion) {
    let mut group = c.benchmark_group("flat_detection");

    for size in [64, 128, 256, 512] {
        let pixels = size * size;
        let dem: Vec<f32> = create_dem_data(size, size);
        let mut flat = vec![0_u8; pixels];

        group.throughput(Throughput::Elements(pixels as u64));

        group.bench_with_input(BenchmarkId::new("simd", size), &size, |b, _| {
            b.iter(|| {
                hydrology_simd::detect_flats_simd(
                    black_box(&dem),
                    black_box(&mut flat),
                    size,
                    size,
                    0.1,
                )
            });
        });
    }

    group.finish();
}

/// Benchmark flow accumulation initialization
fn bench_flow_accumulation_init(c: &mut Criterion) {
    let mut group = c.benchmark_group("flow_accumulation_init");

    for size in [64, 128, 256, 512, 1024] {
        let pixels = size * size;
        let mut flow_acc = vec![0.0_f32; pixels];

        group.throughput(Throughput::Elements(pixels as u64));

        group.bench_with_input(BenchmarkId::new("simd", size), &size, |b, _| {
            b.iter(|| hydrology_simd::initialize_flow_accumulation_simd(black_box(&mut flow_acc)));
        });
    }

    group.finish();
}

/// Benchmark complete hydrology pipeline
fn bench_hydrology_pipeline(c: &mut Criterion) {
    let mut group = c.benchmark_group("hydrology_pipeline");

    for size in [64, 128, 256] {
        let pixels = size * size;
        let dem: Vec<f32> = create_dem_data(size, size);
        let mut flow_dir = vec![0_u8; pixels];
        let mut sinks = vec![0_u8; pixels];
        let mut slope = vec![0.0_f32; pixels];
        let mut flow_acc = vec![0.0_f32; pixels];

        group.throughput(Throughput::Elements(pixels as u64));

        group.bench_with_input(BenchmarkId::new("full_pipeline", size), &size, |b, _| {
            b.iter(|| {
                hydrology_simd::flow_direction_d8_simd(
                    black_box(&dem),
                    black_box(&mut flow_dir),
                    size,
                    size,
                )
                .expect("flow_dir");
                hydrology_simd::detect_sinks_simd(
                    black_box(&dem),
                    black_box(&mut sinks),
                    size,
                    size,
                )
                .expect("sinks");
                hydrology_simd::compute_slope_simd(
                    black_box(&dem),
                    black_box(&mut slope),
                    size,
                    size,
                )
                .expect("slope");
                hydrology_simd::initialize_flow_accumulation_simd(black_box(&mut flow_acc))
                    .expect("flow_acc");
            });
        });
    }

    group.finish();
}

// ============================================================================
// Cost-Distance Benchmarks
// ============================================================================

/// Benchmark Euclidean distance computation
fn bench_euclidean_distance(c: &mut Criterion) {
    let mut group = c.benchmark_group("euclidean_distance");

    for size in [64, 128, 256, 512] {
        let pixels = size * size;

        group.throughput(Throughput::Elements(pixels as u64));

        // Single source
        let sources = create_source_mask(size, size);
        let mut distance = vec![0.0_f32; pixels];

        group.bench_with_input(
            BenchmarkId::new("single_source_simd", size),
            &size,
            |b, _| {
                b.iter(|| {
                    cost_distance_simd::euclidean_distance_simd(
                        black_box(&sources),
                        black_box(&mut distance),
                        size,
                        size,
                        1.0,
                    )
                });
            },
        );

        // Multiple sources
        let multi_sources = create_multi_source_mask(size, size, 4);

        group.bench_with_input(
            BenchmarkId::new("multi_source_simd", size),
            &size,
            |b, _| {
                b.iter(|| {
                    cost_distance_simd::euclidean_distance_simd(
                        black_box(&multi_sources),
                        black_box(&mut distance),
                        size,
                        size,
                        1.0,
                    )
                });
            },
        );
    }

    group.finish();
}

/// Benchmark Manhattan distance computation
fn bench_manhattan_distance(c: &mut Criterion) {
    let mut group = c.benchmark_group("manhattan_distance");

    for size in [64, 128, 256, 512] {
        let pixels = size * size;
        let sources = create_source_mask(size, size);
        let mut distance = vec![0.0_f32; pixels];

        group.throughput(Throughput::Elements(pixels as u64));

        group.bench_with_input(BenchmarkId::new("simd", size), &size, |b, _| {
            b.iter(|| {
                cost_distance_simd::manhattan_distance_simd(
                    black_box(&sources),
                    black_box(&mut distance),
                    size,
                    size,
                    1.0,
                )
            });
        });
    }

    group.finish();
}

/// Benchmark Chebyshev distance computation
fn bench_chebyshev_distance(c: &mut Criterion) {
    let mut group = c.benchmark_group("chebyshev_distance");

    for size in [64, 128, 256, 512] {
        let pixels = size * size;
        let sources = create_source_mask(size, size);
        let mut distance = vec![0.0_f32; pixels];

        group.throughput(Throughput::Elements(pixels as u64));

        group.bench_with_input(BenchmarkId::new("simd", size), &size, |b, _| {
            b.iter(|| {
                cost_distance_simd::chebyshev_distance_simd(
                    black_box(&sources),
                    black_box(&mut distance),
                    size,
                    size,
                    1.0,
                )
            });
        });
    }

    group.finish();
}

/// Compare different distance metrics
fn bench_distance_metrics_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("distance_metrics_comparison");

    let size = 256;
    let pixels = size * size;
    let sources = create_source_mask(size, size);
    let mut distance = vec![0.0_f32; pixels];

    group.throughput(Throughput::Elements(pixels as u64));

    group.bench_function("euclidean", |b| {
        b.iter(|| {
            cost_distance_simd::euclidean_distance_simd(
                black_box(&sources),
                black_box(&mut distance),
                size,
                size,
                1.0,
            )
        });
    });

    group.bench_function("manhattan", |b| {
        b.iter(|| {
            cost_distance_simd::manhattan_distance_simd(
                black_box(&sources),
                black_box(&mut distance),
                size,
                size,
                1.0,
            )
        });
    });

    group.bench_function("chebyshev", |b| {
        b.iter(|| {
            cost_distance_simd::chebyshev_distance_simd(
                black_box(&sources),
                black_box(&mut distance),
                size,
                size,
                1.0,
            )
        });
    });

    group.finish();
}

/// Benchmark cost buffer initialization
fn bench_cost_buffer_init(c: &mut Criterion) {
    let mut group = c.benchmark_group("cost_buffer_init");

    for size in [64, 128, 256, 512, 1024, 2048] {
        let pixels = size * size;
        let mut buffer = vec![0.0_f32; pixels];

        group.throughput(Throughput::Elements(pixels as u64));

        group.bench_with_input(BenchmarkId::new("simd", size), &size, |b, _| {
            b.iter(|| {
                cost_distance_simd::initialize_cost_buffer_simd(
                    black_box(&mut buffer),
                    f32::INFINITY,
                )
            });
        });
    }

    group.finish();
}

/// Benchmark neighbor cost computation
fn bench_neighbor_costs(c: &mut Criterion) {
    let mut group = c.benchmark_group("neighbor_costs");

    for size in [64, 128, 256, 512] {
        let pixels = size * size;
        let cost_surface: Vec<f32> = (0..pixels).map(|i| 1.0 + (i % 10) as f32 * 0.1).collect();
        let mut neighbor_costs = [0.0_f32; 8];

        // We benchmark a single cell computation, but throughput is based on grid size
        // as this would typically be called for each cell
        group.throughput(Throughput::Elements(1));

        group.bench_with_input(BenchmarkId::new("simd", size), &size, |b, _| {
            b.iter(|| {
                cost_distance_simd::compute_neighbor_costs_simd(
                    black_box(&cost_surface),
                    size,
                    size,
                    size / 2,
                    size / 2,
                    1.0,
                    black_box(&mut neighbor_costs),
                )
            });
        });
    }

    group.finish();
}

// ============================================================================
// SIMD vs Non-SIMD Comparison Benchmarks
// ============================================================================

/// Comprehensive SIMD vs non-SIMD comparison for focal operations
fn bench_simd_vs_scalar_focal(c: &mut Criterion) {
    let mut group = c.benchmark_group("simd_vs_scalar_focal");

    let size = 256_u64;
    let pixels = (size * size) as u64;
    let raster = create_dem_raster(size, size);
    let src_f32: Vec<f32> = create_dem_data(size as usize, size as usize);
    let mut dst_f32 = vec![0.0_f32; pixels as usize];

    group.throughput(Throughput::Elements(pixels));

    // Focal mean comparison
    let window = WindowShape::rectangular(5, 5).expect("valid window");

    group.bench_function("focal_mean_generic_5x5", |b| {
        b.iter(|| {
            focal_mean(
                black_box(&raster),
                black_box(&window),
                black_box(&BoundaryMode::Edge),
            )
        });
    });

    group.bench_function("focal_mean_separable_5x5", |b| {
        b.iter(|| focal_mean_separable(black_box(&raster), black_box(5), black_box(5)));
    });

    group.bench_function("focal_mean_simd_5x5", |b| {
        b.iter(|| {
            focal_simd::focal_mean_separable_simd(
                black_box(&src_f32),
                black_box(&mut dst_f32),
                size as usize,
                size as usize,
                5,
                5,
            )
        });
    });

    group.finish();
}

/// Comprehensive SIMD vs non-SIMD comparison for texture analysis
fn bench_simd_vs_scalar_texture(c: &mut Criterion) {
    let mut group = c.benchmark_group("simd_vs_scalar_texture");

    let size = 128;
    let gray_levels = 32;
    let pixels = size * size;

    group.throughput(Throughput::Elements(pixels as u64));

    // Generic GLCM + features
    let raster = create_dem_raster(size as u64, size as u64);
    let params = GlcmParams {
        gray_levels,
        normalize: true,
        symmetric: true,
        window_size: None,
    };

    group.bench_function("glcm_generic", |b| {
        b.iter(|| {
            let glcm = compute_glcm(
                black_box(&raster),
                TextureDirection::Horizontal,
                1,
                black_box(&params),
            )
            .expect("glcm");
            compute_haralick_features(black_box(&glcm))
        });
    });

    // SIMD GLCM + features
    let quantized = create_quantized_image(size, size, gray_levels);
    let mut glcm = vec![0.0_f32; gray_levels * gray_levels];

    group.bench_function("glcm_simd", |b| {
        b.iter(|| {
            texture_simd::glcm_construct_simd(
                black_box(&quantized),
                black_box(&mut glcm),
                size,
                size,
                gray_levels,
                1,
                0,
            )
            .expect("construct");
            texture_simd::glcm_normalize_simd(black_box(&mut glcm), gray_levels).expect("norm");
            texture_simd::compute_haralick_features_simd(black_box(&glcm), gray_levels)
        });
    });

    group.finish();
}

/// Overall performance comparison across all algorithm categories
fn bench_overall_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("overall_comparison");

    let size = 256;
    let pixels = size * size;

    group.throughput(Throughput::Elements(pixels as u64));

    // Focal statistics
    let src_f32: Vec<f32> = create_dem_data(size, size);
    let mut dst_f32 = vec![0.0_f32; pixels];

    group.bench_function("focal_mean_5x5_simd", |b| {
        b.iter(|| {
            focal_simd::focal_mean_separable_simd(
                black_box(&src_f32),
                black_box(&mut dst_f32),
                size,
                size,
                5,
                5,
            )
        });
    });

    // Texture analysis
    let quantized = create_quantized_image(size, size, 32);
    let mut glcm = vec![0.0_f32; 32 * 32];

    group.bench_function("texture_pipeline_simd", |b| {
        b.iter(|| {
            texture_simd::glcm_construct_simd(
                black_box(&quantized),
                black_box(&mut glcm),
                size,
                size,
                32,
                1,
                0,
            )
            .expect("construct");
            texture_simd::glcm_normalize_simd(black_box(&mut glcm), 32).expect("norm");
            texture_simd::compute_haralick_features_simd(black_box(&glcm), 32)
        });
    });

    // Hydrology
    let dem: Vec<f32> = create_dem_data(size, size);
    let mut flow_dir = vec![0_u8; pixels];

    group.bench_function("hydrology_d8_simd", |b| {
        b.iter(|| {
            hydrology_simd::flow_direction_d8_simd(
                black_box(&dem),
                black_box(&mut flow_dir),
                size,
                size,
            )
        });
    });

    // Cost distance
    let sources = create_source_mask(size, size);
    let mut distance = vec![0.0_f32; pixels];

    group.bench_function("cost_distance_euclidean_simd", |b| {
        b.iter(|| {
            cost_distance_simd::euclidean_distance_simd(
                black_box(&sources),
                black_box(&mut distance),
                size,
                size,
                1.0,
            )
        });
    });

    group.finish();
}

// ============================================================================
// Criterion Configuration
// ============================================================================

criterion_group!(
    focal_benches,
    bench_focal_mean,
    bench_focal_variance_stddev,
    bench_focal_min_max,
    bench_focal_convolution,
    bench_focal_median_range,
);

criterion_group!(
    texture_benches,
    bench_glcm_construction,
    bench_glcm_normalization,
    bench_haralick_features,
    bench_texture_image_sizes,
);

criterion_group!(
    hydrology_benches,
    bench_flow_direction,
    bench_sink_detection,
    bench_hydrology_slope,
    bench_flat_detection,
    bench_flow_accumulation_init,
    bench_hydrology_pipeline,
);

criterion_group!(
    cost_distance_benches,
    bench_euclidean_distance,
    bench_manhattan_distance,
    bench_chebyshev_distance,
    bench_distance_metrics_comparison,
    bench_cost_buffer_init,
    bench_neighbor_costs,
);

criterion_group!(
    comparison_benches,
    bench_simd_vs_scalar_focal,
    bench_simd_vs_scalar_texture,
    bench_overall_comparison,
);

criterion_main!(
    focal_benches,
    texture_benches,
    hydrology_benches,
    cost_distance_benches,
    comparison_benches,
);
