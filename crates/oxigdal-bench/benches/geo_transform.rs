//! Benchmarks for GeoTransform operations
#![allow(missing_docs, clippy::expect_used)]
//!
//! This benchmark suite measures the performance of:
//! - Pixel to world coordinate transformation
//! - World to pixel coordinate transformation
//! - Transform inversion
//! - Transform composition
//! - Bounds computation

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxigdal_core::types::{BoundingBox, GeoTransform};
use std::hint::black_box;

fn bench_pixel_to_world(c: &mut Criterion) {
    let mut group = c.benchmark_group("geotransform/pixel_to_world");

    let transforms = vec![
        ("north_up", GeoTransform::north_up(-180.0, 90.0, 0.1, -0.1)),
        ("rotated_45deg", {
            let angle = std::f64::consts::PI / 4.0;
            GeoTransform::new(
                0.0,
                angle.cos(),
                -angle.sin(),
                0.0,
                angle.sin(),
                angle.cos(),
            )
        }),
        (
            "high_resolution",
            GeoTransform::north_up(0.0, 0.0, 0.000001, -0.000001),
        ),
    ];

    for (name, transform) in transforms {
        group.bench_with_input(BenchmarkId::from_parameter(name), &transform, |b, gt| {
            b.iter(|| {
                for x in 0..100 {
                    for y in 0..100 {
                        black_box(gt.pixel_to_world(black_box(x as f64), black_box(y as f64)));
                    }
                }
            });
        });
    }

    group.finish();
}

fn bench_world_to_pixel(c: &mut Criterion) {
    let mut group = c.benchmark_group("geotransform/world_to_pixel");

    let transforms = vec![
        ("north_up", GeoTransform::north_up(-180.0, 90.0, 0.1, -0.1)),
        ("rotated_45deg", {
            let angle = std::f64::consts::PI / 4.0;
            GeoTransform::new(
                0.0,
                angle.cos(),
                -angle.sin(),
                0.0,
                angle.sin(),
                angle.cos(),
            )
        }),
    ];

    for (name, transform) in transforms {
        group.bench_with_input(BenchmarkId::from_parameter(name), &transform, |b, gt| {
            b.iter(|| {
                for x in -180..180 {
                    for y in -90..90 {
                        let result = gt.world_to_pixel(black_box(x as f64), black_box(y as f64));
                        black_box(result.ok());
                    }
                }
            });
        });
    }

    group.finish();
}

fn bench_inverse(c: &mut Criterion) {
    let mut group = c.benchmark_group("geotransform/inverse");

    let transforms = vec![
        ("north_up", GeoTransform::north_up(-180.0, 90.0, 0.1, -0.1)),
        ("rotated_30deg", {
            let angle = std::f64::consts::PI / 6.0;
            GeoTransform::new(
                0.0,
                angle.cos(),
                -angle.sin(),
                0.0,
                angle.sin(),
                angle.cos(),
            )
        }),
        (
            "complex",
            GeoTransform::new(100.0, 0.5, 0.1, 200.0, 0.2, -0.5),
        ),
    ];

    for (name, transform) in transforms {
        group.bench_with_input(BenchmarkId::from_parameter(name), &transform, |b, gt| {
            b.iter(|| {
                black_box(gt.inverse().ok());
            });
        });
    }

    group.finish();
}

fn bench_compose(c: &mut Criterion) {
    let mut group = c.benchmark_group("geotransform/compose");

    let gt1 = GeoTransform::north_up(-180.0, 90.0, 0.1, -0.1);
    let gt2 = GeoTransform::north_up(0.0, 0.0, 2.0, -2.0);

    group.bench_function("compose_north_up", |b| {
        b.iter(|| {
            black_box(gt1.compose(black_box(&gt2)));
        });
    });

    let angle1 = std::f64::consts::PI / 4.0;
    let gt3 = GeoTransform::new(
        0.0,
        angle1.cos(),
        -angle1.sin(),
        0.0,
        angle1.sin(),
        angle1.cos(),
    );
    let angle2 = std::f64::consts::PI / 6.0;
    let gt4 = GeoTransform::new(
        0.0,
        angle2.cos(),
        -angle2.sin(),
        0.0,
        angle2.sin(),
        angle2.cos(),
    );

    group.bench_function("compose_rotated", |b| {
        b.iter(|| {
            black_box(gt3.compose(black_box(&gt4)));
        });
    });

    group.finish();
}

fn bench_compute_bounds(c: &mut Criterion) {
    let mut group = c.benchmark_group("geotransform/compute_bounds");

    let sizes = vec![(256, 256), (1024, 1024), (4096, 4096), (10000, 10000)];

    for (width, height) in sizes {
        let gt = GeoTransform::north_up(-180.0, 90.0, 360.0 / width as f64, -180.0 / height as f64);

        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}x{}", width, height)),
            &(gt, width, height),
            |b, (gt, w, h)| {
                b.iter(|| {
                    black_box(gt.compute_bounds(black_box(*w), black_box(*h)));
                });
            },
        );
    }

    group.finish();
}

fn bench_from_bounds(c: &mut Criterion) {
    let mut group = c.benchmark_group("geotransform/from_bounds");

    let bbox = BoundingBox::new(-180.0, -90.0, 180.0, 90.0).expect("valid bbox");

    let sizes = vec![(256, 256), (1024, 1024), (4096, 4096), (10000, 10000)];

    for (width, height) in sizes {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}x{}", width, height)),
            &(bbox, width, height),
            |b, (bbox, w, h)| {
                b.iter(|| {
                    black_box(
                        GeoTransform::from_bounds(black_box(bbox), black_box(*w), black_box(*h))
                            .ok(),
                    );
                });
            },
        );
    }

    group.finish();
}

fn bench_roundtrip_accuracy(c: &mut Criterion) {
    let mut group = c.benchmark_group("geotransform/roundtrip");

    let gt = GeoTransform::north_up(-180.0, 90.0, 0.1, -0.1);

    group.bench_function("pixel_world_pixel", |b| {
        b.iter(|| {
            for px in 0..100 {
                for py in 0..100 {
                    let (wx, wy) = gt.pixel_to_world(px as f64, py as f64);
                    let result = gt.world_to_pixel(wx, wy);
                    black_box(result.ok());
                }
            }
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_pixel_to_world,
    bench_world_to_pixel,
    bench_inverse,
    bench_compose,
    bench_compute_bounds,
    bench_from_bounds,
    bench_roundtrip_accuracy
);
criterion_main!(benches);
