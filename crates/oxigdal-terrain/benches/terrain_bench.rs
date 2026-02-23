//! Benchmarks for oxigdal-terrain.
#![allow(missing_docs)]

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use oxigdal_terrain::derivatives::*;
use scirs2_core::prelude::*;
use std::hint::black_box;

fn create_test_dem(size: usize) -> Array2<f64> {
    let mut dem = Array2::zeros((size, size));
    for y in 0..size {
        for x in 0..size {
            dem[[y, x]] = 100.0 + ((x as f64).sin() + (y as f64).cos()) * 20.0;
        }
    }
    dem
}

fn bench_slope(c: &mut Criterion) {
    let mut group = c.benchmark_group("slope");

    for size in [100, 500, 1000].iter() {
        let dem = create_test_dem(*size);

        group.bench_with_input(BenchmarkId::new("horn", size), size, |b, _| {
            b.iter(|| {
                slope_horn(
                    black_box(&dem),
                    black_box(10.0),
                    black_box(SlopeUnits::Degrees),
                    black_box(None),
                )
            });
        });

        group.bench_with_input(
            BenchmarkId::new("zevenbergen_thorne", size),
            size,
            |b, _| {
                b.iter(|| {
                    slope_zevenbergen_thorne(
                        black_box(&dem),
                        black_box(10.0),
                        black_box(SlopeUnits::Degrees),
                        black_box(None),
                    )
                });
            },
        );
    }

    group.finish();
}

fn bench_aspect(c: &mut Criterion) {
    let mut group = c.benchmark_group("aspect");

    for size in [100, 500].iter() {
        let dem = create_test_dem(*size);

        group.bench_with_input(BenchmarkId::new("horn", size), size, |b, _| {
            b.iter(|| {
                aspect_horn(
                    black_box(&dem),
                    black_box(10.0),
                    black_box(FlatHandling::NoDirection),
                    black_box(None),
                )
            });
        });
    }

    group.finish();
}

fn bench_curvature(c: &mut Criterion) {
    let mut group = c.benchmark_group("curvature");

    for size in [100, 500].iter() {
        let dem = create_test_dem(*size);

        group.bench_with_input(BenchmarkId::new("profile", size), size, |b, _| {
            b.iter(|| profile_curvature(black_box(&dem), black_box(10.0), black_box(None)));
        });

        group.bench_with_input(BenchmarkId::new("plan", size), size, |b, _| {
            b.iter(|| plan_curvature(black_box(&dem), black_box(10.0), black_box(None)));
        });
    }

    group.finish();
}

fn bench_hillshade(c: &mut Criterion) {
    let mut group = c.benchmark_group("hillshade");

    for size in [100, 500].iter() {
        let dem = create_test_dem(*size);

        group.bench_with_input(BenchmarkId::new("traditional", size), size, |b, _| {
            b.iter(|| {
                hillshade_traditional(
                    black_box(&dem),
                    black_box(10.0),
                    black_box(315.0),
                    black_box(45.0),
                    black_box(1.0),
                    black_box(None),
                )
            });
        });
    }

    group.finish();
}

fn bench_tpi(c: &mut Criterion) {
    let mut group = c.benchmark_group("tpi");

    for size in [100, 500].iter() {
        let dem = create_test_dem(*size);

        group.bench_with_input(BenchmarkId::new("radius_1", size), size, |b, _| {
            b.iter(|| tpi(black_box(&dem), black_box(1), black_box(None)));
        });

        group.bench_with_input(BenchmarkId::new("radius_3", size), size, |b, _| {
            b.iter(|| tpi(black_box(&dem), black_box(3), black_box(None)));
        });
    }

    group.finish();
}

fn bench_tri(c: &mut Criterion) {
    let mut group = c.benchmark_group("tri");

    for size in [100, 500].iter() {
        let dem = create_test_dem(*size);

        group.bench_with_input(BenchmarkId::new("standard", size), size, |b, _| {
            b.iter(|| tri(black_box(&dem), black_box(None)));
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_slope,
    bench_aspect,
    bench_curvature,
    bench_hillshade,
    bench_tpi,
    bench_tri
);
criterion_main!(benches);
