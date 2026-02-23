//! Database connector benchmarks.
#![allow(missing_docs, clippy::expect_used)]

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use geo_types::{Geometry, point, polygon};
use std::hint::black_box;

#[cfg(feature = "mongodb")]
use oxigdal_db_connectors::mongodb::geometry_to_geojson;
#[cfg(feature = "mysql")]
use oxigdal_db_connectors::mysql::geometry_to_wkt;
#[cfg(feature = "sqlite")]
use oxigdal_db_connectors::sqlite::geometry_to_wkb;

#[cfg(feature = "mysql")]
fn bench_wkt_conversion(c: &mut Criterion) {
    let mut group = c.benchmark_group("wkt_conversion");

    let point = Geometry::Point(point!(x: 1.0, y: 2.0));
    group.bench_function("point_to_wkt", |b| {
        b.iter(|| {
            let _ = geometry_to_wkt(black_box(&point));
        });
    });

    let poly = polygon![
        (x: 0.0, y: 0.0),
        (x: 10.0, y: 0.0),
        (x: 10.0, y: 10.0),
        (x: 0.0, y: 10.0),
        (x: 0.0, y: 0.0),
    ];
    let geom = Geometry::Polygon(poly);

    group.bench_function("polygon_to_wkt", |b| {
        b.iter(|| {
            let _ = geometry_to_wkt(black_box(&geom));
        });
    });

    group.finish();
}

#[cfg(feature = "sqlite")]
fn bench_wkb_conversion(c: &mut Criterion) {
    let mut group = c.benchmark_group("wkb_conversion");

    let point = Geometry::Point(point!(x: 1.0, y: 2.0));
    group.bench_function("point_to_wkb", |b| {
        b.iter(|| {
            let _ = geometry_to_wkb(black_box(&point));
        });
    });

    let poly = polygon![
        (x: 0.0, y: 0.0),
        (x: 10.0, y: 0.0),
        (x: 10.0, y: 10.0),
        (x: 0.0, y: 10.0),
        (x: 0.0, y: 0.0),
    ];
    let geom = Geometry::Polygon(poly);

    group.bench_function("polygon_to_wkb", |b| {
        b.iter(|| {
            let _ = geometry_to_wkb(black_box(&geom));
        });
    });

    group.finish();
}

#[cfg(feature = "mongodb")]
fn bench_geojson_conversion(c: &mut Criterion) {
    let mut group = c.benchmark_group("geojson_conversion");

    let point = Geometry::Point(point!(x: 1.0, y: 2.0));
    group.bench_function("point_to_geojson", |b| {
        b.iter(|| {
            let _ = geometry_to_geojson(black_box(&point));
        });
    });

    let poly = polygon![
        (x: 0.0, y: 0.0),
        (x: 10.0, y: 0.0),
        (x: 10.0, y: 10.0),
        (x: 0.0, y: 10.0),
        (x: 0.0, y: 0.0),
    ];
    let geom = Geometry::Polygon(poly);

    group.bench_function("polygon_to_geojson", |b| {
        b.iter(|| {
            let _ = geometry_to_geojson(black_box(&geom));
        });
    });

    group.finish();
}

#[cfg(all(feature = "mysql", feature = "sqlite", feature = "mongodb"))]
fn bench_format_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("format_comparison");

    let point = Geometry::Point(point!(x: 1.0, y: 2.0));

    group.bench_with_input(BenchmarkId::new("point", "wkt"), &point, |b, p| {
        b.iter(|| {
            let _ = geometry_to_wkt(black_box(p));
        });
    });

    group.bench_with_input(BenchmarkId::new("point", "wkb"), &point, |b, p| {
        b.iter(|| {
            let _ = geometry_to_wkb(black_box(p));
        });
    });

    group.bench_with_input(BenchmarkId::new("point", "geojson"), &point, |b, p| {
        b.iter(|| {
            let _ = geometry_to_geojson(black_box(p));
        });
    });

    group.finish();
}

#[cfg(all(feature = "mysql", feature = "sqlite", feature = "mongodb"))]
fn bench_complex_geometry(c: &mut Criterion) {
    let mut group = c.benchmark_group("complex_geometry");

    // Create a complex polygon with interior rings
    let exterior = vec![
        (0.0, 0.0),
        (100.0, 0.0),
        (100.0, 100.0),
        (0.0, 100.0),
        (0.0, 0.0),
    ];

    let interior1 = vec![
        (20.0, 20.0),
        (40.0, 20.0),
        (40.0, 40.0),
        (20.0, 40.0),
        (20.0, 20.0),
    ];

    let interior2 = vec![
        (60.0, 60.0),
        (80.0, 60.0),
        (80.0, 80.0),
        (60.0, 80.0),
        (60.0, 60.0),
    ];

    let poly = geo_types::Polygon::new(exterior.into(), vec![interior1.into(), interior2.into()]);
    let geom = Geometry::Polygon(poly);

    group.bench_function("complex_polygon_to_wkt", |b| {
        b.iter(|| {
            let _ = geometry_to_wkt(black_box(&geom));
        });
    });

    group.bench_function("complex_polygon_to_wkb", |b| {
        b.iter(|| {
            let _ = geometry_to_wkb(black_box(&geom));
        });
    });

    group.bench_function("complex_polygon_to_geojson", |b| {
        b.iter(|| {
            let _ = geometry_to_geojson(black_box(&geom));
        });
    });

    group.finish();
}

// Conditionally include benchmarks based on enabled features
#[cfg(all(feature = "mysql", feature = "sqlite", feature = "mongodb"))]
criterion_group!(
    benches,
    bench_wkt_conversion,
    bench_wkb_conversion,
    bench_geojson_conversion,
    bench_format_comparison,
    bench_complex_geometry
);

#[cfg(all(feature = "mysql", feature = "sqlite", not(feature = "mongodb")))]
criterion_group!(benches, bench_wkt_conversion, bench_wkb_conversion);

#[cfg(all(feature = "mysql", feature = "mongodb", not(feature = "sqlite")))]
criterion_group!(benches, bench_wkt_conversion, bench_geojson_conversion);

#[cfg(all(feature = "sqlite", feature = "mongodb", not(feature = "mysql")))]
criterion_group!(benches, bench_wkb_conversion, bench_geojson_conversion);

#[cfg(all(feature = "mysql", not(feature = "sqlite"), not(feature = "mongodb")))]
criterion_group!(benches, bench_wkt_conversion);

#[cfg(all(feature = "sqlite", not(feature = "mysql"), not(feature = "mongodb")))]
criterion_group!(benches, bench_wkb_conversion);

#[cfg(all(feature = "mongodb", not(feature = "mysql"), not(feature = "sqlite")))]
criterion_group!(benches, bench_geojson_conversion);

#[cfg(not(any(feature = "mysql", feature = "sqlite", feature = "mongodb")))]
criterion_group!(benches,);

criterion_main!(benches);
