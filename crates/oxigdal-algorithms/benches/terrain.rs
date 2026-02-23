//! Comprehensive benchmarks for terrain analysis algorithms
//!
//! This benchmark suite tests various terrain metrics including TPI, TRI, slope,
//! aspect, hillshade, curvature, roughness, and VRM across different data sizes,
//! neighborhood sizes, and with SIMD vs non-SIMD comparisons.
//!
//! ## Performance Characteristics
//!
//! - **Slope/Aspect**: O(n) - 3x3 window operation
//! - **Hillshade**: O(n) - requires slope and aspect
//! - **TPI**: O(n * k²) - k = neighborhood radius
//! - **TRI**: O(n) - 3x3 window
//! - **Roughness**: O(n * k²) - k = neighborhood radius
//! - **Curvature**: O(n) - second derivative calculations
//! - **VRM**: O(n * k²) - k = neighborhood radius, computationally intensive
//!
//! ## SIMD Acceleration
//!
//! Expected speedup: 3-5x for most operations on supported platforms
#![allow(
    missing_docs,
    clippy::expect_used,
    clippy::panic,
    clippy::unit_arg,
    clippy::unnecessary_cast
)]

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxigdal_algorithms::raster::{
    CurvatureType, HillshadeParams, aspect, compute_aspect_degrees, compute_curvature,
    compute_roughness, compute_slope_degrees, compute_tpi, compute_tri, compute_vrm, hillshade,
    slope,
};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;
use std::hint::black_box;

#[cfg(feature = "simd")]
use oxigdal_algorithms::simd::terrain_simd::{terrain_slope_simd, terrain_tri_simd};

/// Terrain pattern for test data generation
#[derive(Debug, Clone, Copy)]
enum TerrainPattern {
    /// Smooth linear slope
    Linear,
    /// Multiple peaks and valleys
    Mountainous,
    /// Relatively flat with small variations
    Flat,
    /// Rolling hills
    Rolling,
}

/// Creates a realistic DEM based on terrain pattern
fn create_terrain_dem(width: usize, height: usize, pattern: TerrainPattern) -> RasterBuffer {
    let mut dem = RasterBuffer::zeros(width as u64, height as u64, RasterDataType::Float32);

    match pattern {
        TerrainPattern::Linear => {
            for y in 0..height {
                for x in 0..width {
                    let elevation = 1000.0 + (x as f64 * 0.5) + (y as f64 * 0.3);
                    let _ = dem.set_pixel(x as u64, y as u64, elevation);
                }
            }
        }
        TerrainPattern::Mountainous => {
            for y in 0..height {
                for x in 0..width {
                    let x_norm = x as f64 / width as f64;
                    let y_norm = y as f64 / height as f64;

                    let elevation = 500.0
                        + 800.0 * (x_norm * std::f64::consts::PI * 2.0).sin()
                        + 600.0 * (y_norm * std::f64::consts::PI * 2.0).cos()
                        + 300.0 * ((x_norm + y_norm) * std::f64::consts::PI * 5.0).sin()
                        + 150.0 * ((x_norm - y_norm) * std::f64::consts::PI * 7.0).cos();

                    let _ = dem.set_pixel(x as u64, y as u64, elevation);
                }
            }
        }
        TerrainPattern::Flat => {
            for y in 0..height {
                for x in 0..width {
                    // Small random-like variations
                    let noise = 5.0 * ((x * 17 + y * 23) as f64 % 10.0 - 5.0) / 5.0;
                    let elevation = 1000.0 + noise;
                    let _ = dem.set_pixel(x as u64, y as u64, elevation);
                }
            }
        }
        TerrainPattern::Rolling => {
            for y in 0..height {
                for x in 0..width {
                    let x_norm = x as f64 / width as f64;
                    let y_norm = y as f64 / height as f64;

                    let elevation = 800.0
                        + 150.0 * (x_norm * std::f64::consts::PI).sin()
                        + 120.0 * (y_norm * std::f64::consts::PI * 1.3).cos()
                        + 80.0 * ((x_norm + y_norm) * std::f64::consts::PI * 3.0).sin();

                    let _ = dem.set_pixel(x as u64, y as u64, elevation);
                }
            }
        }
    }

    dem
}

/// Creates DEM as Vec<f32> for SIMD benchmarks
fn create_terrain_vec(width: usize, height: usize, pattern: TerrainPattern) -> Vec<f32> {
    let dem = create_terrain_dem(width, height, pattern);
    let mut data = vec![0.0f32; width * height];

    for y in 0..height {
        for x in 0..width {
            if let Ok(val) = dem.get_pixel(x as u64, y as u64) {
                data[y * width + x] = val as f32;
            }
        }
    }

    data
}

/// Benchmark slope computation with varying data sizes
///
/// Time complexity: O(n)
/// Expected SIMD speedup: 3-4x
fn bench_slope(c: &mut Criterion) {
    let mut group = c.benchmark_group("slope");

    for &size in &[100, 500, 1000, 2000, 5000] {
        let dem = create_terrain_dem(size, size, TerrainPattern::Mountainous);
        let cell_size = 30.0; // 30 meter resolution

        group.throughput(Throughput::Elements((size * size) as u64));
        group.bench_with_input(BenchmarkId::new("standard", size), &size, |b, _| {
            b.iter(|| slope(black_box(&dem), black_box(cell_size), 1.0).ok());
        });
    }

    group.finish();
}

/// Benchmark aspect computation
///
/// Time complexity: O(n)
/// Expected SIMD speedup: 3-4x
fn bench_aspect(c: &mut Criterion) {
    let mut group = c.benchmark_group("aspect");

    for &size in &[100, 500, 1000, 2000, 5000] {
        let dem = create_terrain_dem(size, size, TerrainPattern::Mountainous);
        let cell_size = 30.0;

        group.throughput(Throughput::Elements((size * size) as u64));
        group.bench_with_input(BenchmarkId::new("standard", size), &size, |b, _| {
            b.iter(|| aspect(black_box(&dem), black_box(cell_size), 1.0).ok());
        });
    }

    group.finish();
}

/// Benchmark hillshade with different sun positions
///
/// Time complexity: O(n)
fn bench_hillshade(c: &mut Criterion) {
    let mut group = c.benchmark_group("hillshade");

    for &size in &[100, 500, 1000, 2000] {
        let dem = create_terrain_dem(size, size, TerrainPattern::Mountainous);

        group.throughput(Throughput::Elements((size * size) as u64));
        group.bench_with_input(BenchmarkId::new("size", size), &size, |b, _| {
            b.iter(|| {
                let params = HillshadeParams::new(315.0, 45.0);
                hillshade(black_box(&dem), black_box(params)).ok()
            });
        });
    }

    // Benchmark different sun angles
    let size = 1000;
    let dem = create_terrain_dem(size, size, TerrainPattern::Mountainous);

    for &azimuth in &[0.0, 90.0, 180.0, 270.0, 315.0] {
        group.bench_with_input(
            BenchmarkId::new("azimuth", azimuth as usize),
            &azimuth,
            |b, &az| {
                b.iter(|| {
                    let params = HillshadeParams::new(az, 45.0);
                    hillshade(black_box(&dem), black_box(params)).ok()
                });
            },
        );
    }

    group.finish();
}

/// Benchmark TPI with varying neighborhood sizes
///
/// Time complexity: O(n * k²) where k is neighborhood radius
fn bench_tpi(c: &mut Criterion) {
    let mut group = c.benchmark_group("tpi");

    // Vary data size
    for &size in &[100, 500, 1000] {
        let dem = create_terrain_dem(size, size, TerrainPattern::Mountainous);
        let cell_size = 30.0;

        group.throughput(Throughput::Elements((size * size) as u64));
        group.bench_with_input(BenchmarkId::new("size_3x3", size), &size, |b, _| {
            b.iter(|| {
                compute_tpi(black_box(&dem), black_box(3), black_box(cell_size))
                    .expect("TPI failed")
            });
        });
    }

    // Vary neighborhood size on fixed data
    let size = 1000;
    let dem = create_terrain_dem(size, size, TerrainPattern::Mountainous);
    let cell_size = 30.0;

    for &neighborhood in &[3, 5, 7, 9, 11] {
        group.bench_with_input(
            BenchmarkId::new("neighborhood", neighborhood),
            &neighborhood,
            |b, &n| {
                b.iter(|| {
                    compute_tpi(black_box(&dem), black_box(n), black_box(cell_size))
                        .expect("TPI failed")
                });
            },
        );
    }

    group.finish();
}

/// Benchmark TRI (Terrain Ruggedness Index)
///
/// Time complexity: O(n)
/// Expected SIMD speedup: 3-4x
fn bench_tri(c: &mut Criterion) {
    let mut group = c.benchmark_group("tri");

    for &size in &[100, 500, 1000, 2000, 5000] {
        let dem = create_terrain_dem(size, size, TerrainPattern::Mountainous);
        let cell_size = 30.0;

        group.throughput(Throughput::Elements((size * size) as u64));
        group.bench_with_input(BenchmarkId::new("standard", size), &size, |b, _| {
            b.iter(|| compute_tri(black_box(&dem), black_box(cell_size)).expect("TRI failed"));
        });
    }

    group.finish();
}

/// Benchmark surface roughness with varying neighborhood sizes
///
/// Time complexity: O(n * k²)
fn bench_roughness(c: &mut Criterion) {
    let mut group = c.benchmark_group("roughness");

    // Vary data size
    for &size in &[100, 500, 1000] {
        let dem = create_terrain_dem(size, size, TerrainPattern::Mountainous);

        group.throughput(Throughput::Elements((size * size) as u64));
        group.bench_with_input(BenchmarkId::new("size_3x3", size), &size, |b, _| {
            b.iter(|| compute_roughness(black_box(&dem), black_box(3)).expect("Roughness failed"));
        });
    }

    // Vary neighborhood size
    let size = 1000;
    let dem = create_terrain_dem(size, size, TerrainPattern::Mountainous);

    for &neighborhood in &[3, 5, 7, 9] {
        group.bench_with_input(
            BenchmarkId::new("neighborhood", neighborhood),
            &neighborhood,
            |b, &n| {
                b.iter(|| {
                    compute_roughness(black_box(&dem), black_box(n)).expect("Roughness failed")
                });
            },
        );
    }

    group.finish();
}

/// Benchmark curvature computation for all types
///
/// Time complexity: O(n)
fn bench_curvature(c: &mut Criterion) {
    let mut group = c.benchmark_group("curvature");

    let size = 1000;
    let dem = create_terrain_dem(size, size, TerrainPattern::Mountainous);
    let cell_size = 30.0;

    group.throughput(Throughput::Elements((size * size) as u64));

    // Benchmark each curvature type
    let curvature_types = [
        ("profile", CurvatureType::Profile),
        ("planform", CurvatureType::Planform),
        ("total", CurvatureType::Total),
        ("mean", CurvatureType::Mean),
        ("gaussian", CurvatureType::Gaussian),
    ];

    for (name, curv_type) in curvature_types.iter() {
        group.bench_function(*name, |b| {
            b.iter(|| {
                compute_curvature(black_box(&dem), black_box(cell_size), black_box(*curv_type))
                    .expect("Curvature failed")
            });
        });
    }

    // Also benchmark different data sizes for profile curvature
    for &test_size in &[100, 500, 1000, 2000] {
        let test_dem = create_terrain_dem(test_size, test_size, TerrainPattern::Mountainous);

        group.bench_with_input(
            BenchmarkId::new("profile_size", test_size),
            &test_size,
            |b, _| {
                b.iter(|| {
                    compute_curvature(
                        black_box(&test_dem),
                        black_box(cell_size),
                        black_box(CurvatureType::Profile),
                    )
                    .expect("Curvature failed")
                });
            },
        );
    }

    group.finish();
}

/// Benchmark VRM (Vector Ruggedness Measure)
///
/// Time complexity: O(n * k²)
/// Most computationally intensive terrain metric
fn bench_vrm(c: &mut Criterion) {
    let mut group = c.benchmark_group("vrm");
    group.sample_size(10); // VRM is slow, reduce sample size

    // Vary data size (smaller sizes due to computational cost)
    for &size in &[100, 250, 500] {
        let dem = create_terrain_dem(size, size, TerrainPattern::Mountainous);
        let cell_size = 30.0;

        group.throughput(Throughput::Elements((size * size) as u64));
        group.bench_with_input(BenchmarkId::new("size_3x3", size), &size, |b, _| {
            b.iter(|| {
                compute_vrm(black_box(&dem), black_box(3), black_box(cell_size))
                    .expect("VRM failed")
            });
        });
    }

    // Vary neighborhood size on smaller dataset
    let size = 250;
    let dem = create_terrain_dem(size, size, TerrainPattern::Mountainous);
    let cell_size = 30.0;

    for &neighborhood in &[3, 5, 7] {
        group.bench_with_input(
            BenchmarkId::new("neighborhood", neighborhood),
            &neighborhood,
            |b, &n| {
                b.iter(|| {
                    compute_vrm(black_box(&dem), black_box(n), black_box(cell_size))
                        .expect("VRM failed")
                });
            },
        );
    }

    group.finish();
}

/// Benchmark terrain patterns (real-world scenarios)
fn bench_terrain_patterns(c: &mut Criterion) {
    let mut group = c.benchmark_group("terrain_patterns");

    let size = 1000;
    let cell_size = 30.0;

    let patterns = [
        ("linear", TerrainPattern::Linear),
        ("mountainous", TerrainPattern::Mountainous),
        ("flat", TerrainPattern::Flat),
        ("rolling", TerrainPattern::Rolling),
    ];

    for (name, pattern) in patterns.iter() {
        let dem = create_terrain_dem(size, size, *pattern);

        group.throughput(Throughput::Elements((size * size) as u64));
        group.bench_function(*name, |b| {
            b.iter(|| {
                // Complete terrain analysis workflow
                let _ = compute_slope_degrees(black_box(&dem), black_box(cell_size));
                let _ = compute_aspect_degrees(black_box(&dem), black_box(cell_size));
                let _ = compute_tri(black_box(&dem), black_box(cell_size));
            });
        });
    }

    group.finish();
}

/// SIMD vs non-SIMD comparison for slope
#[cfg(feature = "simd")]
fn bench_simd_slope(c: &mut Criterion) {
    let mut group = c.benchmark_group("simd_slope");

    let size = 2000;
    let dem_vec = create_terrain_vec(size, size, TerrainPattern::Mountainous);
    let dem_buffer = create_terrain_dem(size, size, TerrainPattern::Mountainous);
    let cell_size = 30.0;

    group.throughput(Throughput::Elements((size * size) as u64));

    group.bench_function("simd", |b| {
        let mut output = vec![0.0f32; size * size];
        b.iter(|| {
            terrain_slope_simd(
                black_box(&dem_vec),
                black_box(&mut output),
                black_box(size),
                black_box(size),
                black_box(cell_size as f32),
            )
            .expect("SIMD slope failed");
        });
    });

    group.bench_function("scalar", |b| {
        b.iter(|| slope(black_box(&dem_buffer), black_box(cell_size), 1.0).ok());
    });

    group.finish();
}

/// SIMD vs non-SIMD comparison for TRI
#[cfg(feature = "simd")]
fn bench_simd_tri(c: &mut Criterion) {
    let mut group = c.benchmark_group("simd_tri");

    let size = 2000;
    let dem_vec = create_terrain_vec(size, size, TerrainPattern::Mountainous);
    let dem_buffer = create_terrain_dem(size, size, TerrainPattern::Mountainous);
    let cell_size = 30.0;

    group.throughput(Throughput::Elements((size * size) as u64));

    group.bench_function("simd", |b| {
        let mut output = vec![0.0f32; size * size];
        b.iter(|| {
            terrain_tri_simd(
                black_box(&dem_vec),
                black_box(&mut output),
                black_box(size),
                black_box(size),
            )
            .ok()
        });
    });

    group.bench_function("scalar", |b| {
        b.iter(|| compute_tri(black_box(&dem_buffer), black_box(cell_size)).expect("TRI failed"));
    });

    group.finish();
}

/// SIMD vs non-SIMD comparison for hillshade
///
/// Expected speedup: 3-4x with SIMD acceleration
/// SIMD implementation uses batched neighborhood extraction and processes 4 pixels at a time
#[cfg(feature = "simd")]
fn bench_simd_hillshade(c: &mut Criterion) {
    let mut group = c.benchmark_group("simd_hillshade");

    // Test with different sizes to show scalability
    for &size in &[500, 1000, 2000] {
        let dem_buffer = create_terrain_dem(size, size, TerrainPattern::Mountainous);

        group.throughput(Throughput::Elements((size * size) as u64));

        // Test with SIMD enabled (default)
        group.bench_with_input(BenchmarkId::new("simd", size), &size, |b, _| {
            b.iter(|| {
                let params = HillshadeParams::new(315.0, 45.0)
                    .with_z_factor(1.0)
                    .with_pixel_size(30.0);
                hillshade(black_box(&dem_buffer), black_box(params)).ok()
            });
        });

        // For comparison: benchmark shows scalar fallback performance
        // (Note: The implementation auto-selects SIMD when available,
        // this benchmark measures the overall performance improvement)
    }

    // Benchmark different terrain patterns
    let size = 1000;
    for pattern in &[
        TerrainPattern::Flat,
        TerrainPattern::Linear,
        TerrainPattern::Rolling,
        TerrainPattern::Mountainous,
    ] {
        let dem = create_terrain_dem(size, size, *pattern);
        let pattern_name = format!("{pattern:?}");

        group.bench_with_input(
            BenchmarkId::new("terrain", pattern_name.as_str()),
            pattern,
            |b, _| {
                b.iter(|| {
                    let params = HillshadeParams::new(315.0, 45.0);
                    hillshade(black_box(&dem), black_box(params)).ok()
                });
            },
        );
    }

    // Benchmark different sun positions (azimuth)
    let dem = create_terrain_dem(1000, 1000, TerrainPattern::Mountainous);
    for &azimuth in &[0.0, 90.0, 180.0, 270.0, 315.0] {
        group.bench_with_input(
            BenchmarkId::new("azimuth", azimuth as usize),
            &azimuth,
            |b, &az| {
                b.iter(|| {
                    let params = HillshadeParams::new(az, 45.0);
                    hillshade(black_box(&dem), black_box(params)).ok()
                });
            },
        );
    }

    group.finish();
}

#[cfg(feature = "simd")]
criterion_group!(
    benches,
    bench_slope,
    bench_aspect,
    bench_hillshade,
    bench_tpi,
    bench_tri,
    bench_roughness,
    bench_curvature,
    bench_vrm,
    bench_terrain_patterns,
    bench_simd_slope,
    bench_simd_tri,
    bench_simd_hillshade,
);

#[cfg(not(feature = "simd"))]
criterion_group!(
    benches,
    bench_slope,
    bench_aspect,
    bench_hillshade,
    bench_tpi,
    bench_tri,
    bench_roughness,
    bench_curvature,
    bench_vrm,
    bench_terrain_patterns,
);

criterion_main!(benches);
