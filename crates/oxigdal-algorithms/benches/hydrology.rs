//! Comprehensive benchmarks for hydrological analysis algorithms
//!
//! This benchmark suite tests the performance of flow direction, flow accumulation,
//! watershed delineation, sink filling, and stream network extraction algorithms.
//!
//! ## Performance Characteristics
//!
//! - **Flow Direction (D8)**: O(n) - single pass over DEM
//! - **Flow Direction (D-infinity)**: O(n) - single pass with trigonometric calculations
//! - **Flow Accumulation**: O(n log n) - depends on topological ordering
//! - **Sink Filling**: O(n log n) - priority queue-based algorithm
//! - **Watershed Delineation**: O(n) - region growing from pour points
//! - **Stream Network**: O(n) - threshold-based extraction
#![allow(
    missing_docs,
    clippy::expect_used,
    clippy::panic,
    clippy::unit_arg,
    clippy::unnecessary_cast,
    dead_code
)]

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxigdal_algorithms::raster::hydrology::{
    compute_d8_flow_direction, compute_dinf_flow_direction, compute_flow_accumulation,
    compute_stream_order, compute_weighted_flow_accumulation, delineate_watersheds,
    extract_stream_network, fill_sinks, identify_sinks,
};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;
use std::hint::black_box;

/// Creates a realistic DEM with varied terrain
///
/// Generates a synthetic DEM that mimics real topography with:
/// - Multiple peaks and valleys
/// - Gradual slopes
/// - Some flat areas
/// - Natural-looking elevation patterns
fn create_realistic_dem(width: usize, height: usize, terrain_type: TerrainType) -> RasterBuffer {
    let mut dem = RasterBuffer::zeros(width as u64, height as u64, RasterDataType::Float32);

    match terrain_type {
        TerrainType::Mountainous => {
            // Create mountainous terrain with multiple peaks
            for y in 0..height {
                for x in 0..width {
                    let x_norm = x as f64 / width as f64;
                    let y_norm = y as f64 / height as f64;

                    // Multiple sine waves for mountain ridges
                    let elevation = 1000.0
                        + 500.0 * (x_norm * std::f64::consts::PI * 2.0).sin()
                        + 300.0 * (y_norm * std::f64::consts::PI * 3.0).sin()
                        + 200.0 * ((x_norm + y_norm) * std::f64::consts::PI * 4.0).sin();

                    let _ = dem.set_pixel(x as u64, y as u64, elevation);
                }
            }
        }
        TerrainType::Rolling => {
            // Create rolling hills terrain
            for y in 0..height {
                for x in 0..width {
                    let x_norm = x as f64 / width as f64;
                    let y_norm = y as f64 / height as f64;

                    let elevation = 500.0
                        + 100.0 * (x_norm * std::f64::consts::PI).sin()
                        + 80.0 * (y_norm * std::f64::consts::PI * 1.5).cos()
                        + 50.0 * ((x_norm * y_norm) * std::f64::consts::PI * 5.0).sin();

                    let _ = dem.set_pixel(x as u64, y as u64, elevation);
                }
            }
        }
        TerrainType::WithSinks => {
            // Create terrain with deliberate sinks/depressions
            for y in 0..height {
                for x in 0..width {
                    let x_norm = x as f64 / width as f64;
                    let y_norm = y as f64 / height as f64;

                    let mut elevation = 1000.0 + 200.0 * (x_norm * std::f64::consts::PI).sin();

                    // Add some sinks
                    let sink_dist1 = ((x_norm - 0.3).powi(2) + (y_norm - 0.3).powi(2)).sqrt();
                    let sink_dist2 = ((x_norm - 0.7).powi(2) + (y_norm - 0.7).powi(2)).sqrt();

                    if sink_dist1 < 0.1 {
                        elevation -= 150.0 * (1.0 - sink_dist1 / 0.1);
                    }
                    if sink_dist2 < 0.1 {
                        elevation -= 150.0 * (1.0 - sink_dist2 / 0.1);
                    }

                    let _ = dem.set_pixel(x as u64, y as u64, elevation);
                }
            }
        }
    }

    dem
}

#[derive(Debug, Clone, Copy)]
enum TerrainType {
    Mountainous,
    Rolling,
    WithSinks,
}

/// Benchmark D8 flow direction computation
///
/// D8 is the standard 8-direction flow routing method.
/// Time complexity: O(n) where n is the number of cells
fn bench_d8_flow_direction(c: &mut Criterion) {
    let mut group = c.benchmark_group("flow_direction_d8");

    for &size in &[100, 500, 1000, 2000] {
        let dem = create_realistic_dem(size, size, TerrainType::Mountainous);
        let cell_size = 30.0; // 30 meter resolution

        group.throughput(Throughput::Elements((size * size) as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| {
                compute_d8_flow_direction(black_box(&dem), black_box(cell_size))
                    .expect("Flow direction failed")
            });
        });
    }

    group.finish();
}

/// Benchmark D-infinity flow direction computation
///
/// D-infinity provides continuous flow direction angles.
/// Time complexity: O(n) but with higher constant due to trigonometry
fn bench_dinf_flow_direction(c: &mut Criterion) {
    let mut group = c.benchmark_group("flow_direction_dinf");

    for &size in &[100, 500, 1000, 2000] {
        let dem = create_realistic_dem(size, size, TerrainType::Mountainous);
        let cell_size = 30.0;

        group.throughput(Throughput::Elements((size * size) as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| {
                compute_dinf_flow_direction(black_box(&dem), black_box(cell_size))
                    .expect("D-infinity flow direction failed")
            });
        });
    }

    group.finish();
}

/// Benchmark flow accumulation computation
///
/// Calculates contributing area for each cell.
/// Time complexity: O(n log n) due to topological sorting
fn bench_flow_accumulation(c: &mut Criterion) {
    let mut group = c.benchmark_group("flow_accumulation");

    for &size in &[100, 500, 1000] {
        let dem = create_realistic_dem(size, size, TerrainType::Mountainous);
        let cell_size = 30.0;

        group.throughput(Throughput::Elements((size * size) as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| {
                compute_flow_accumulation(black_box(&dem), black_box(cell_size))
                    .expect("Flow accumulation failed")
            });
        });
    }

    group.finish();
}

/// Benchmark weighted flow accumulation
///
/// Flow accumulation with per-cell weights (e.g., precipitation).
/// Time complexity: O(n log n)
fn bench_weighted_flow_accumulation(c: &mut Criterion) {
    let mut group = c.benchmark_group("weighted_flow_accumulation");

    for &size in &[100, 500, 1000] {
        let dem = create_realistic_dem(size, size, TerrainType::Mountainous);
        let cell_size = 30.0;

        // Create weight raster (e.g., precipitation data)
        let mut weights = RasterBuffer::zeros(size as u64, size as u64, RasterDataType::Float32);
        for y in 0..size {
            for x in 0..size {
                let weight = 100.0 + 50.0 * ((x + y) as f64 / size as f64);
                let _ = weights.set_pixel(x as u64, y as u64, weight);
            }
        }

        group.throughput(Throughput::Elements((size * size) as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| {
                compute_weighted_flow_accumulation(
                    black_box(&dem),
                    black_box(&weights),
                    black_box(cell_size),
                )
                .expect("Weighted flow accumulation failed")
            });
        });
    }

    group.finish();
}

/// Benchmark sink identification
///
/// Identifies local minima in the DEM.
/// Time complexity: O(n)
fn bench_identify_sinks(c: &mut Criterion) {
    let mut group = c.benchmark_group("identify_sinks");

    for &size in &[100, 500, 1000, 2000] {
        let dem = create_realistic_dem(size, size, TerrainType::WithSinks);

        group.throughput(Throughput::Elements((size * size) as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| identify_sinks(black_box(&dem)).expect("Sink identification failed"));
        });
    }

    group.finish();
}

/// Benchmark sink filling
///
/// Fills depressions to create hydrologically correct DEM.
/// Time complexity: O(n log n) using priority queue
fn bench_fill_sinks(c: &mut Criterion) {
    let mut group = c.benchmark_group("fill_sinks");

    for &size in &[100, 500, 1000] {
        let dem = create_realistic_dem(size, size, TerrainType::WithSinks);
        let epsilon = 0.1;

        group.throughput(Throughput::Elements((size * size) as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| {
                fill_sinks(black_box(&dem), black_box(epsilon)).expect("Sink filling failed")
            });
        });
    }

    group.finish();
}

/// Benchmark watershed delineation
///
/// Delineates drainage basins from flow direction.
/// Time complexity: O(n) for region growing
fn bench_delineate_watersheds(c: &mut Criterion) {
    let mut group = c.benchmark_group("delineate_watersheds");

    for &size in &[100, 500, 1000] {
        let dem = create_realistic_dem(size, size, TerrainType::Mountainous);
        let cell_size = 30.0;

        let flow_acc =
            compute_flow_accumulation(&dem, cell_size).expect("Flow accumulation failed");

        // Create pour points raster (high accumulation cells)
        let mut pour_points = RasterBuffer::zeros(size as u64, size as u64, RasterDataType::UInt8);
        let mut point_count = 0;
        for y in 0..size {
            for x in 0..size {
                if let Ok(acc) = flow_acc.get_pixel(x as u64, y as u64) {
                    if acc > (size * size / 100) as f64 {
                        // Top 1% accumulation
                        let _ = pour_points.set_pixel(x as u64, y as u64, 1.0);
                        point_count += 1;
                        if point_count >= 5 {
                            break;
                        }
                    }
                }
            }
            if point_count >= 5 {
                break;
            }
        }

        group.throughput(Throughput::Elements((size * size) as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| {
                delineate_watersheds(
                    black_box(&dem),
                    black_box(&pour_points),
                    black_box(cell_size),
                )
                .expect("Watershed delineation failed")
            });
        });
    }

    group.finish();
}

/// Benchmark stream network extraction
///
/// Extracts stream network based on accumulation threshold.
/// Time complexity: O(n)
fn bench_extract_stream_network(c: &mut Criterion) {
    let mut group = c.benchmark_group("extract_stream_network");

    for &size in &[100, 500, 1000] {
        let dem = create_realistic_dem(size, size, TerrainType::Mountainous);
        let cell_size = 30.0;

        let flow_acc =
            compute_flow_accumulation(&dem, cell_size).expect("Flow accumulation failed");

        let threshold = (size * size / 100) as f64; // 1% of total area

        group.throughput(Throughput::Elements((size * size) as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| {
                extract_stream_network(black_box(&flow_acc), black_box(threshold))
                    .expect("Stream extraction failed")
            });
        });
    }

    group.finish();
}

/// Benchmark stream order computation (Strahler)
///
/// Computes hierarchical stream ordering.
/// Time complexity: O(n)
fn bench_stream_order(c: &mut Criterion) {
    let mut group = c.benchmark_group("stream_order");

    for &size in &[100, 500, 1000] {
        let dem = create_realistic_dem(size, size, TerrainType::Mountainous);
        let cell_size = 30.0;

        let flow_acc =
            compute_flow_accumulation(&dem, cell_size).expect("Flow accumulation failed");

        let threshold = (size * size / 100) as f64;

        group.throughput(Throughput::Elements((size * size) as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| {
                compute_stream_order(
                    black_box(&dem),
                    black_box(&flow_acc),
                    black_box(threshold),
                    black_box(cell_size),
                )
                .expect("Stream order computation failed")
            });
        });
    }

    group.finish();
}

/// Benchmark complete hydrological workflow
///
/// Tests the typical workflow: sink filling -> flow direction -> flow accumulation
fn bench_complete_workflow(c: &mut Criterion) {
    let mut group = c.benchmark_group("hydrology_workflow");

    for &size in &[100, 500, 1000] {
        let dem = create_realistic_dem(size, size, TerrainType::WithSinks);
        let cell_size = 30.0;
        let epsilon = 0.1;

        group.throughput(Throughput::Elements((size * size) as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| {
                // Complete workflow
                let filled =
                    fill_sinks(black_box(&dem), black_box(epsilon)).expect("Sink filling failed");
                let _flow_acc = compute_flow_accumulation(&filled, black_box(cell_size))
                    .expect("Flow accumulation failed");
            });
        });
    }

    group.finish();
}

/// Benchmark D8 vs D-infinity comparison
///
/// Direct comparison of the two flow direction methods
fn bench_flow_method_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("flow_method_comparison");
    let size = 1000;
    let dem = create_realistic_dem(size, size, TerrainType::Mountainous);
    let cell_size = 30.0;

    group.throughput(Throughput::Elements((size * size) as u64));

    group.bench_function("d8", |b| {
        b.iter(|| {
            compute_d8_flow_direction(black_box(&dem), black_box(cell_size)).expect("D8 failed")
        });
    });

    group.bench_function("dinf", |b| {
        b.iter(|| {
            compute_dinf_flow_direction(black_box(&dem), black_box(cell_size))
                .expect("D-infinity failed")
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_d8_flow_direction,
    bench_dinf_flow_direction,
    bench_flow_accumulation,
    bench_weighted_flow_accumulation,
    bench_identify_sinks,
    bench_fill_sinks,
    bench_delineate_watersheds,
    bench_extract_stream_network,
    bench_stream_order,
    bench_complete_workflow,
    bench_flow_method_comparison,
);

criterion_main!(benches);
