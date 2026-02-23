//! Benchmarks comparing pooled vs non-pooled spatial operations
//!
//! This benchmark suite demonstrates the allocation reduction benefits
//! of using object pooling for batch spatial operations.
#![allow(
    missing_docs,
    clippy::expect_used,
    clippy::panic,
    clippy::unit_arg,
    clippy::unnecessary_cast,
    clippy::needless_range_loop
)]

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxigdal_algorithms::vector::{
    BufferOptions, Coordinate, LineString, Point, Polygon, buffer_point, buffer_point_pooled,
    union_polygon, union_polygon_pooled,
};
use std::hint::black_box;

/// Creates a test point at the given coordinates
fn create_point(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

/// Creates a test square polygon
fn create_square(x: f64, y: f64, size: f64) -> Polygon {
    let coords = vec![
        Coordinate::new_2d(x, y),
        Coordinate::new_2d(x + size, y),
        Coordinate::new_2d(x + size, y + size),
        Coordinate::new_2d(x, y + size),
        Coordinate::new_2d(x, y),
    ];
    let exterior = LineString::new(coords).expect("Valid square");
    Polygon::new(exterior, vec![]).expect("Valid polygon")
}

/// Benchmark buffer operations (pooled vs non-pooled)
fn bench_buffer_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("buffer_operations");

    for size in [10, 100, 1000].iter() {
        // Non-pooled buffer
        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::new("non_pooled", size), size, |b, &size| {
            let points: Vec<Point> = (0..size)
                .map(|i| create_point(i as f64, i as f64))
                .collect();
            let options = BufferOptions::default();

            b.iter(|| {
                for point in &points {
                    let _ = black_box(buffer_point(point, 10.0, &options));
                }
            });
        });

        // Pooled buffer
        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::new("pooled", size), size, |b, &size| {
            let points: Vec<Point> = (0..size)
                .map(|i| create_point(i as f64, i as f64))
                .collect();
            let options = BufferOptions::default();

            b.iter(|| {
                for point in &points {
                    let _ = black_box(buffer_point_pooled(point, 10.0, &options));
                }
            });
        });
    }

    group.finish();
}

/// Benchmark union operations (pooled vs non-pooled)
fn bench_union_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("union_operations");

    for size in [10, 100, 1000].iter() {
        let polygons: Vec<Polygon> = (0..*size)
            .map(|i| create_square(i as f64 * 5.0, 0.0, 10.0))
            .collect();

        // Non-pooled union
        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::new("non_pooled", size), size, |b, _| {
            b.iter(|| {
                for i in 0..(polygons.len() - 1) {
                    let _ = black_box(union_polygon(&polygons[i], &polygons[i + 1]));
                }
            });
        });

        // Pooled union
        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::new("pooled", size), size, |b, _| {
            b.iter(|| {
                for i in 0..(polygons.len() - 1) {
                    let _ = black_box(union_polygon_pooled(&polygons[i], &polygons[i + 1]));
                }
            });
        });
    }

    group.finish();
}

/// Benchmark pool overhead for single operations
fn bench_pool_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("pool_overhead");

    let point = create_point(0.0, 0.0);
    let options = BufferOptions::default();

    // Single operation non-pooled
    group.bench_function("single_non_pooled", |b| {
        b.iter(|| black_box(buffer_point(&point, 10.0, &options)));
    });

    // Single operation pooled
    group.bench_function("single_pooled", |b| {
        b.iter(|| black_box(buffer_point_pooled(&point, 10.0, &options)));
    });

    group.finish();
}

/// Benchmark pool reuse efficiency
fn bench_pool_reuse(c: &mut Criterion) {
    use oxigdal_algorithms::vector::{clear_all_pools, get_pool_stats};

    let mut group = c.benchmark_group("pool_reuse");

    let points: Vec<Point> = (0..1000)
        .map(|i| create_point(i as f64, i as f64))
        .collect();
    let options = BufferOptions::default();

    // Measure pool fill rate
    group.bench_function("pool_accumulation", |b| {
        b.iter(|| {
            clear_all_pools();
            for point in &points {
                let _ = black_box(buffer_point_pooled(point, 10.0, &options));
            }
            let stats = get_pool_stats();
            black_box(stats);
        });
    });

    // Measure pool reuse
    group.bench_function("pool_reuse_rate", |b| {
        b.iter(|| {
            // Pre-fill pool
            clear_all_pools();
            for i in 0..16 {
                let _ = buffer_point_pooled(&points[i], 10.0, &options);
            }

            // Now use pooled objects repeatedly
            for point in &points[16..] {
                let _ = black_box(buffer_point_pooled(point, 10.0, &options));
            }

            let stats = get_pool_stats();
            black_box(stats);
        });
    });

    group.finish();
}

/// Benchmark memory allocation patterns
fn bench_allocation_patterns(c: &mut Criterion) {
    let mut group = c.benchmark_group("allocation_patterns");

    let batch_sizes = [100, 500, 1000, 2000];

    for &batch_size in &batch_sizes {
        let points: Vec<Point> = (0..batch_size)
            .map(|i| create_point(i as f64, i as f64))
            .collect();
        let options = BufferOptions::default();

        // Sequential non-pooled allocations
        group.throughput(Throughput::Elements(batch_size as u64));
        group.bench_with_input(
            BenchmarkId::new("sequential_non_pooled", batch_size),
            &batch_size,
            |b, _| {
                b.iter(|| {
                    let results: Vec<_> = points
                        .iter()
                        .map(|p| buffer_point(p, 10.0, &options))
                        .collect();
                    black_box(results);
                });
            },
        );

        // Sequential pooled allocations
        group.throughput(Throughput::Elements(batch_size as u64));
        group.bench_with_input(
            BenchmarkId::new("sequential_pooled", batch_size),
            &batch_size,
            |b, _| {
                b.iter(|| {
                    // Clear pool before each iteration for consistent measurement
                    oxigdal_algorithms::vector::clear_all_pools();
                    let results: Vec<_> = points
                        .iter()
                        .map(|p| buffer_point_pooled(p, 10.0, &options))
                        .collect();
                    black_box(results);
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_buffer_operations,
    bench_union_operations,
    bench_pool_overhead,
    bench_pool_reuse,
    bench_allocation_patterns,
);

criterion_main!(benches);
