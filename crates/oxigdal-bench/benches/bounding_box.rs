//! Benchmarks for BoundingBox operations
#![allow(missing_docs, clippy::expect_used)]
//!
//! This benchmark suite measures the performance of:
//! - Intersection computation
//! - Union computation
//! - Containment tests
//! - Point containment tests
//! - Area and dimension calculations

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxigdal_core::types::BoundingBox;
use std::hint::black_box;

fn bench_intersection(c: &mut Criterion) {
    let mut group = c.benchmark_group("bbox/intersection");

    let scenarios = vec![
        (
            "overlapping",
            BoundingBox::new(0.0, 0.0, 100.0, 100.0).expect("valid"),
            BoundingBox::new(50.0, 50.0, 150.0, 150.0).expect("valid"),
        ),
        (
            "contained",
            BoundingBox::new(0.0, 0.0, 100.0, 100.0).expect("valid"),
            BoundingBox::new(25.0, 25.0, 75.0, 75.0).expect("valid"),
        ),
        (
            "disjoint",
            BoundingBox::new(0.0, 0.0, 100.0, 100.0).expect("valid"),
            BoundingBox::new(200.0, 200.0, 300.0, 300.0).expect("valid"),
        ),
        (
            "edge_touching",
            BoundingBox::new(0.0, 0.0, 100.0, 100.0).expect("valid"),
            BoundingBox::new(100.0, 0.0, 200.0, 100.0).expect("valid"),
        ),
    ];

    for (name, bbox1, bbox2) in scenarios {
        group.bench_with_input(
            BenchmarkId::from_parameter(name),
            &(bbox1, bbox2),
            |b, (b1, b2)| {
                b.iter(|| {
                    black_box(b1.intersection(black_box(b2)));
                });
            },
        );
    }

    group.finish();
}

fn bench_union(c: &mut Criterion) {
    let mut group = c.benchmark_group("bbox/union");

    let scenarios = vec![
        (
            "overlapping",
            BoundingBox::new(0.0, 0.0, 100.0, 100.0).expect("valid"),
            BoundingBox::new(50.0, 50.0, 150.0, 150.0).expect("valid"),
        ),
        (
            "disjoint",
            BoundingBox::new(0.0, 0.0, 100.0, 100.0).expect("valid"),
            BoundingBox::new(200.0, 200.0, 300.0, 300.0).expect("valid"),
        ),
        (
            "adjacent",
            BoundingBox::new(0.0, 0.0, 100.0, 100.0).expect("valid"),
            BoundingBox::new(100.0, 0.0, 200.0, 100.0).expect("valid"),
        ),
    ];

    for (name, bbox1, bbox2) in scenarios {
        group.bench_with_input(
            BenchmarkId::from_parameter(name),
            &(bbox1, bbox2),
            |b, (b1, b2)| {
                b.iter(|| {
                    black_box(b1.union(black_box(b2)));
                });
            },
        );
    }

    group.finish();
}

fn bench_contains(c: &mut Criterion) {
    let mut group = c.benchmark_group("bbox/contains");

    let outer = BoundingBox::new(0.0, 0.0, 100.0, 100.0).expect("valid");
    let inner = BoundingBox::new(25.0, 25.0, 75.0, 75.0).expect("valid");
    let overlapping = BoundingBox::new(50.0, 50.0, 150.0, 150.0).expect("valid");
    let disjoint = BoundingBox::new(200.0, 200.0, 300.0, 300.0).expect("valid");

    group.bench_function("contains_inner", |b| {
        b.iter(|| {
            black_box(outer.contains(black_box(&inner)));
        });
    });

    group.bench_function("contains_overlapping", |b| {
        b.iter(|| {
            black_box(outer.contains(black_box(&overlapping)));
        });
    });

    group.bench_function("contains_disjoint", |b| {
        b.iter(|| {
            black_box(outer.contains(black_box(&disjoint)));
        });
    });

    group.finish();
}

fn bench_contains_point(c: &mut Criterion) {
    let mut group = c.benchmark_group("bbox/contains_point");
    group.throughput(Throughput::Elements(10000));

    let bbox = BoundingBox::new(-180.0, -90.0, 180.0, 90.0).expect("valid");

    group.bench_function("world_grid", |b| {
        b.iter(|| {
            for lon in -180..180 {
                for lat in -90..90 {
                    black_box(bbox.contains_point(black_box(lon as f64), black_box(lat as f64)));
                }
            }
        });
    });

    group.finish();
}

fn bench_intersects(c: &mut Criterion) {
    let mut group = c.benchmark_group("bbox/intersects");

    let base = BoundingBox::new(0.0, 0.0, 100.0, 100.0).expect("valid");

    let test_cases = vec![
        (
            "overlapping",
            BoundingBox::new(50.0, 50.0, 150.0, 150.0).expect("valid"),
        ),
        (
            "contained",
            BoundingBox::new(25.0, 25.0, 75.0, 75.0).expect("valid"),
        ),
        (
            "disjoint",
            BoundingBox::new(200.0, 200.0, 300.0, 300.0).expect("valid"),
        ),
        (
            "edge_touching",
            BoundingBox::new(100.0, 0.0, 200.0, 100.0).expect("valid"),
        ),
        (
            "corner_touching",
            BoundingBox::new(100.0, 100.0, 200.0, 200.0).expect("valid"),
        ),
    ];

    for (name, test_bbox) in test_cases {
        group.bench_with_input(
            BenchmarkId::from_parameter(name),
            &test_bbox,
            |b, test_bbox| {
                b.iter(|| {
                    black_box(base.intersects(black_box(test_bbox)));
                });
            },
        );
    }

    group.finish();
}

fn bench_area_calculations(c: &mut Criterion) {
    let mut group = c.benchmark_group("bbox/area_calculations");

    let bboxes = vec![
        (
            "small",
            BoundingBox::new(0.0, 0.0, 1.0, 1.0).expect("valid"),
        ),
        (
            "medium",
            BoundingBox::new(0.0, 0.0, 100.0, 100.0).expect("valid"),
        ),
        (
            "large",
            BoundingBox::new(-180.0, -90.0, 180.0, 90.0).expect("valid"),
        ),
        ("web_mercator", BoundingBox::world_web_mercator()),
    ];

    for (name, bbox) in bboxes {
        group.bench_with_input(
            BenchmarkId::new("width_height_area", name),
            &bbox,
            |b, bbox| {
                b.iter(|| {
                    black_box(bbox.width());
                    black_box(bbox.height());
                    black_box(bbox.area());
                });
            },
        );

        group.bench_with_input(BenchmarkId::new("center", name), &bbox, |b, bbox| {
            b.iter(|| {
                black_box(bbox.center());
            });
        });
    }

    group.finish();
}

fn bench_expand(c: &mut Criterion) {
    let mut group = c.benchmark_group("bbox/expand");

    let bbox = BoundingBox::new(0.0, 0.0, 100.0, 100.0).expect("valid");

    group.bench_function("expand_uniform", |b| {
        b.iter(|| {
            black_box(bbox.expand(black_box(10.0)));
        });
    });

    group.bench_function("expand_to_include", |b| {
        b.iter(|| {
            black_box(bbox.expand_to_include(black_box(150.0), black_box(150.0)));
        });
    });

    group.finish();
}

fn bench_batch_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("bbox/batch_operations");

    // Generate random bboxes
    let mut bboxes = Vec::new();
    for i in 0..1000 {
        let x = (i % 100) as f64;
        let y = (i / 100) as f64;
        bboxes.push(BoundingBox::new(x, y, x + 10.0, y + 10.0).expect("valid"));
    }

    group.throughput(Throughput::Elements(1000));

    group.bench_function("union_many", |b| {
        b.iter(|| {
            let mut result = bboxes[0];
            for bbox in &bboxes[1..] {
                result = result.union(bbox);
            }
            black_box(result);
        });
    });

    group.bench_function("intersection_check_all", |b| {
        b.iter(|| {
            let target = BoundingBox::new(25.0, 25.0, 75.0, 75.0).expect("valid");
            let mut count = 0;
            for bbox in &bboxes {
                if target.intersects(bbox) {
                    count += 1;
                }
            }
            black_box(count);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_intersection,
    bench_union,
    bench_contains,
    bench_contains_point,
    bench_intersects,
    bench_area_calculations,
    bench_expand,
    bench_batch_operations
);
criterion_main!(benches);
