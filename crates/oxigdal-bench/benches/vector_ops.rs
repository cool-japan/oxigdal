//! Vector operations benchmarks using Criterion.
#![allow(missing_docs, clippy::expect_used, clippy::panic, clippy::unit_arg)]

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use std::time::Duration;

#[cfg(feature = "vector")]
fn bench_geojson_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("geojson_read");

    group.sample_size(10);
    group.measurement_time(Duration::from_secs(10));

    for feature_count in [100, 1000, 10000].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(feature_count),
            feature_count,
            |b, &count| {
                b.iter(|| {
                    // Simulate parsing GeoJSON features: generate and parse coordinate pairs
                    let features: Vec<(f64, f64)> = (0..count)
                        .map(|i| {
                            let lon = ((i as f64 * 1.23456789) % 360.0) - 180.0;
                            let lat = ((i as f64 * 0.98765432) % 180.0) - 90.0;
                            (lon, lat)
                        })
                        .collect();
                    // Simulate bounding-box computation during read
                    let min_lon = features
                        .iter()
                        .map(|(x, _)| *x)
                        .fold(f64::INFINITY, f64::min);
                    let max_lon = features
                        .iter()
                        .map(|(x, _)| *x)
                        .fold(f64::NEG_INFINITY, f64::max);
                    black_box((min_lon, max_lon))
                });
            },
        );
    }

    group.finish();
}

#[cfg(feature = "vector")]
fn bench_geojson_write(c: &mut Criterion) {
    let mut group = c.benchmark_group("geojson_write");

    group.sample_size(10);
    group.measurement_time(Duration::from_secs(10));

    for feature_count in [100, 1000, 10000].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(feature_count),
            feature_count,
            |b, &count| {
                let features = vec![0u8; count * 100];
                b.iter(|| {
                    // Simulate serializing GeoJSON: format coordinate pairs as JSON-like strings
                    let json_bytes: Vec<u8> = features
                        .chunks(100)
                        .flat_map(|chunk| {
                            let s: String = chunk
                                .iter()
                                .enumerate()
                                .map(|(i, &v)| format!("{{\"id\":{},\"v\":{}}}", i, v))
                                .collect::<Vec<_>>()
                                .join(",");
                            s.into_bytes()
                        })
                        .collect();
                    black_box(json_bytes.len())
                });
            },
        );
    }

    group.finish();
}

#[cfg(feature = "vector")]
fn bench_simplification(c: &mut Criterion) {
    let mut group = c.benchmark_group("geometry_simplification");

    group.sample_size(10);
    group.measurement_time(Duration::from_secs(10));

    for point_count in [100, 500, 1000].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(point_count),
            point_count,
            |b, &count| {
                let points: Vec<(f64, f64)> =
                    (0..count).map(|i| (i as f64, (i as f64).sin())).collect();

                b.iter(|| {
                    // Ramer-Douglas-Peucker-like simplification
                    let tolerance = 0.5_f64;
                    // Find the point farthest from the line between first and last point
                    let first = points[0];
                    let last = *points.last().unwrap_or(&first);
                    let max_dist = points[1..points.len().saturating_sub(1)]
                        .iter()
                        .map(|&(px, py)| {
                            // Distance from point to line segment
                            let dx = last.0 - first.0;
                            let dy = last.1 - first.1;
                            let len_sq = dx * dx + dy * dy;
                            if len_sq == 0.0 {
                                let ex = px - first.0;
                                let ey = py - first.1;
                                (ex * ex + ey * ey).sqrt()
                            } else {
                                let t = ((px - first.0) * dx + (py - first.1) * dy) / len_sq;
                                let t = t.clamp(0.0, 1.0);
                                let proj_x = first.0 + t * dx;
                                let proj_y = first.1 + t * dy;
                                ((px - proj_x).powi(2) + (py - proj_y).powi(2)).sqrt()
                            }
                        })
                        .fold(0.0_f64, f64::max);
                    // Simplified result: keep points where deviation exceeds tolerance
                    let simplified_count = if max_dist > tolerance { count } else { 2 };
                    black_box(simplified_count)
                });
            },
        );
    }

    group.finish();
}

#[cfg(feature = "vector")]
fn bench_buffer_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("geometry_buffer");

    group.sample_size(10);
    group.measurement_time(Duration::from_secs(10));

    for buffer_distance in [10.0, 50.0, 100.0].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(buffer_distance),
            buffer_distance,
            |b, &distance| {
                let points: Vec<(f64, f64)> =
                    vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];

                b.iter(|| {
                    // Simulate polygon buffer: compute offset polygon vertices
                    let buffered: Vec<(f64, f64)> = points
                        .iter()
                        .map(|&(x, y)| {
                            // Simple outward offset in cardinal direction
                            let nx = if x == 0.0 {
                                -1.0
                            } else if x > 0.0 {
                                1.0
                            } else {
                                -1.0
                            };
                            let ny = if y == 0.0 {
                                -1.0
                            } else if y > 0.0 {
                                1.0
                            } else {
                                -1.0
                            };
                            (x + nx * distance, y + ny * distance)
                        })
                        .collect();
                    // Compute area of buffered polygon (shoelace formula)
                    let n = buffered.len();
                    let area: f64 = (0..n)
                        .map(|i| {
                            let j = (i + 1) % n;
                            buffered[i].0 * buffered[j].1 - buffered[j].0 * buffered[i].1
                        })
                        .sum::<f64>()
                        .abs()
                        / 2.0;
                    black_box(area)
                });
            },
        );
    }

    group.finish();
}

#[cfg(feature = "vector")]
fn bench_spatial_indexing(c: &mut Criterion) {
    let mut group = c.benchmark_group("spatial_indexing");

    group.sample_size(10);
    group.measurement_time(Duration::from_secs(10));

    for feature_count in [100, 1000, 10000].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(feature_count),
            feature_count,
            |b, &count| {
                let features: Vec<(f64, f64)> = (0..count)
                    .map(|i| (i as f64 % 100.0, (i / 100) as f64))
                    .collect();

                b.iter(|| {
                    // Simulate R-tree-style spatial index construction: sort by Hilbert curve key
                    let mut indexed: Vec<(u64, (f64, f64))> = features
                        .iter()
                        .map(|&(x, y)| {
                            // Compute Hilbert-like space-filling index key
                            let ix = ((x + 100.0) * 100.0) as u64;
                            let iy = ((y + 100.0) * 100.0) as u64;
                            // Interleave bits (Morton code)
                            let key = interleave_bits(ix, iy);
                            (key, (x, y))
                        })
                        .collect();
                    indexed.sort_unstable_by_key(|&(k, _)| k);
                    black_box(indexed.len())
                });
            },
        );
    }

    group.finish();
}

#[cfg(feature = "vector")]
fn interleave_bits(x: u64, y: u64) -> u64 {
    let mut result = 0u64;
    for i in 0..32u64 {
        result |= ((x >> i) & 1) << (2 * i);
        result |= ((y >> i) & 1) << (2 * i + 1);
    }
    result
}

#[cfg(feature = "vector")]
criterion_group!(
    vector_benches,
    bench_geojson_read,
    bench_geojson_write,
    bench_simplification,
    bench_buffer_operations,
    bench_spatial_indexing
);

#[cfg(not(feature = "vector"))]
criterion_group!(vector_benches,);

criterion_main!(vector_benches);
