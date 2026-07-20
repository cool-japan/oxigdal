//! Benchmarks for GeoJSON parsing and serialization
#![allow(missing_docs, clippy::expect_used)]
//!
//! This benchmark suite measures the performance of:
//! - GeoJSON parsing (Point, LineString, Polygon, MultiPolygon)
//! - GeoJSON serialization
//! - Feature extraction
//! - Property parsing
//! - Large feature collection handling
//!
//! Tests various dataset sizes:
//! - Small: 100 features (~10 KB)
//! - Medium: 10,000 features (~1 MB)
//! - Large: 100,000 features (~10 MB)

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;

/// Generate a simple Point GeoJSON feature
fn generate_point_feature(id: usize) -> String {
    format!(
        r#"{{
            "type": "Feature",
            "geometry": {{
                "type": "Point",
                "coordinates": [{}, {}]
            }},
            "properties": {{
                "id": {},
                "name": "Point {}",
                "value": {}
            }}
        }}"#,
        (id as f64) * 0.01,
        (id as f64) * 0.01,
        id,
        id,
        id as f64
    )
}

/// Generate a LineString GeoJSON feature
fn generate_linestring_feature(id: usize, points: usize) -> String {
    let coords: Vec<String> = (0..points)
        .map(|i| format!("[{}, {}]", (id + i) as f64 * 0.01, (id + i) as f64 * 0.01))
        .collect();

    format!(
        r#"{{
            "type": "Feature",
            "geometry": {{
                "type": "LineString",
                "coordinates": [{}]
            }},
            "properties": {{
                "id": {},
                "length": {}
            }}
        }}"#,
        coords.join(", "),
        id,
        points
    )
}

/// Generate a Polygon GeoJSON feature
fn generate_polygon_feature(id: usize, vertices: usize) -> String {
    let base_x = (id as f64) * 0.01;
    let base_y = (id as f64) * 0.01;
    let size = 0.001;

    let mut coords: Vec<String> = (0..vertices)
        .map(|i| {
            let angle = 2.0 * std::f64::consts::PI * (i as f64) / (vertices as f64);
            let x = base_x + size * angle.cos();
            let y = base_y + size * angle.sin();
            format!("[{}, {}]", x, y)
        })
        .collect();

    // Close the ring
    coords.push(coords[0].clone());

    format!(
        r#"{{
            "type": "Feature",
            "geometry": {{
                "type": "Polygon",
                "coordinates": [[{}]]
            }},
            "properties": {{
                "id": {},
                "vertices": {}
            }}
        }}"#,
        coords.join(", "),
        id,
        vertices
    )
}

/// Generate a FeatureCollection with specified number of features
fn generate_feature_collection(feature_count: usize, geometry_type: &str) -> String {
    let features: Vec<String> = (0..feature_count)
        .map(|i| match geometry_type {
            "Point" => generate_point_feature(i),
            "LineString" => generate_linestring_feature(i, 10),
            "Polygon" => generate_polygon_feature(i, 5),
            _ => generate_point_feature(i),
        })
        .collect();

    format!(
        r#"{{
            "type": "FeatureCollection",
            "features": [
                {}
            ]
        }}"#,
        features.join(",\n                ")
    )
}

fn bench_parse_point_features(c: &mut Criterion) {
    let mut group = c.benchmark_group("geojson/parse/point");

    let sizes = vec![(100, "small"), (1_000, "medium"), (10_000, "large")];

    for (count, label) in sizes {
        let geojson = generate_feature_collection(count, "Point");
        let _bytes = geojson.len();

        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(
            BenchmarkId::new(label, format!("{}_features", count)),
            &geojson,
            |b, json| {
                b.iter(|| {
                    black_box(serde_json::from_str::<serde_json::Value>(black_box(json)).ok());
                });
            },
        );
    }

    group.finish();
}

fn bench_parse_linestring_features(c: &mut Criterion) {
    let mut group = c.benchmark_group("geojson/parse/linestring");

    let sizes = vec![(100, "small"), (1_000, "medium"), (10_000, "large")];

    for (count, label) in sizes {
        let geojson = generate_feature_collection(count, "LineString");

        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(
            BenchmarkId::new(label, format!("{}_features", count)),
            &geojson,
            |b, json| {
                b.iter(|| {
                    black_box(serde_json::from_str::<serde_json::Value>(black_box(json)).ok());
                });
            },
        );
    }

    group.finish();
}

fn bench_parse_polygon_features(c: &mut Criterion) {
    let mut group = c.benchmark_group("geojson/parse/polygon");

    let sizes = vec![(100, "small"), (1_000, "medium"), (10_000, "large")];

    for (count, label) in sizes {
        let geojson = generate_feature_collection(count, "Polygon");

        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(
            BenchmarkId::new(label, format!("{}_features", count)),
            &geojson,
            |b, json| {
                b.iter(|| {
                    black_box(serde_json::from_str::<serde_json::Value>(black_box(json)).ok());
                });
            },
        );
    }

    group.finish();
}

fn bench_serialize_features(c: &mut Criterion) {
    let mut group = c.benchmark_group("geojson/serialize");

    let sizes = vec![(100, "small"), (1_000, "medium"), (10_000, "large")];

    for (count, label) in sizes {
        let geojson = generate_feature_collection(count, "Point");
        let parsed: serde_json::Value = serde_json::from_str(&geojson).expect("should parse");

        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(
            BenchmarkId::new(label, format!("{}_features", count)),
            &parsed,
            |b, value| {
                b.iter(|| {
                    black_box(serde_json::to_string(black_box(value)).ok());
                });
            },
        );
    }

    group.finish();
}

fn bench_serialize_pretty(c: &mut Criterion) {
    let mut group = c.benchmark_group("geojson/serialize_pretty");

    let sizes = vec![(100, "small"), (1_000, "medium")];

    for (count, label) in sizes {
        let geojson = generate_feature_collection(count, "Point");
        let parsed: serde_json::Value = serde_json::from_str(&geojson).expect("should parse");

        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(
            BenchmarkId::new(label, format!("{}_features", count)),
            &parsed,
            |b, value| {
                b.iter(|| {
                    black_box(serde_json::to_string_pretty(black_box(value)).ok());
                });
            },
        );
    }

    group.finish();
}

fn bench_parse_streaming(c: &mut Criterion) {
    let mut group = c.benchmark_group("geojson/parse/streaming");

    let count = 10_000;
    let geojson = generate_feature_collection(count, "Point");

    group.throughput(Throughput::Bytes(geojson.len() as u64));

    group.bench_function("stream_parse", |b| {
        b.iter(|| {
            let cursor = std::io::Cursor::new(black_box(geojson.as_bytes()));
            black_box(serde_json::from_reader::<_, serde_json::Value>(cursor).ok());
        });
    });

    group.finish();
}

fn bench_parse_properties(c: &mut Criterion) {
    let mut group = c.benchmark_group("geojson/parse/properties");

    // Generate features with different property counts
    let property_counts = vec![1, 5, 10, 20];

    for prop_count in property_counts {
        let mut feature = r#"{
                "type": "Feature",
                "geometry": {
                    "type": "Point",
                    "coordinates": [0, 0]
                },
                "properties": {"#
            .to_string();

        let props: Vec<String> = (0..prop_count)
            .map(|i| format!(r#""prop_{}": {}"#, i, i))
            .collect();

        feature.push_str(&props.join(", "));
        feature.push_str("}}");

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_props", prop_count)),
            &feature,
            |b, json| {
                b.iter(|| {
                    black_box(serde_json::from_str::<serde_json::Value>(black_box(json)).ok());
                });
            },
        );
    }

    group.finish();
}

fn bench_parse_complex_geometries(c: &mut Criterion) {
    let mut group = c.benchmark_group("geojson/parse/complex");

    // MultiPolygon with multiple rings
    let multi_polygon = r#"{
        "type": "Feature",
        "geometry": {
            "type": "MultiPolygon",
            "coordinates": [
                [[[0, 0], [1, 0], [1, 1], [0, 1], [0, 0]]],
                [[[2, 2], [3, 2], [3, 3], [2, 3], [2, 2]]]
            ]
        },
        "properties": {"id": 1}
    }"#;

    group.bench_function("multi_polygon", |b| {
        b.iter(|| {
            black_box(serde_json::from_str::<serde_json::Value>(black_box(multi_polygon)).ok());
        });
    });

    // GeometryCollection
    let geometry_collection = r#"{
        "type": "Feature",
        "geometry": {
            "type": "GeometryCollection",
            "geometries": [
                {"type": "Point", "coordinates": [0, 0]},
                {"type": "LineString", "coordinates": [[0, 0], [1, 1]]},
                {"type": "Polygon", "coordinates": [[[0, 0], [1, 0], [1, 1], [0, 1], [0, 0]]]}
            ]
        },
        "properties": {"id": 1}
    }"#;

    group.bench_function("geometry_collection", |b| {
        b.iter(|| {
            black_box(
                serde_json::from_str::<serde_json::Value>(black_box(geometry_collection)).ok(),
            );
        });
    });

    group.finish();
}

fn bench_parse_large_coordinates(c: &mut Criterion) {
    let mut group = c.benchmark_group("geojson/parse/large_coords");

    let coord_counts = vec![100, 1_000, 10_000];

    for count in coord_counts {
        let coords: Vec<String> = (0..count)
            .map(|i| format!("[{}, {}]", i as f64 * 0.01, i as f64 * 0.01))
            .collect();

        let linestring = format!(
            r#"{{
                "type": "Feature",
                "geometry": {{
                    "type": "LineString",
                    "coordinates": [{}]
                }},
                "properties": {{"id": 1}}
            }}"#,
            coords.join(", ")
        );

        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_coords", count)),
            &linestring,
            |b, json| {
                b.iter(|| {
                    black_box(serde_json::from_str::<serde_json::Value>(black_box(json)).ok());
                });
            },
        );
    }

    group.finish();
}

fn bench_parse_real_world_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("geojson/parse/real_world");

    // Simulate different file sizes
    let sizes = vec![
        (10, 1024, "1KB"),           // ~1 KB
        (100, 10 * 1024, "10KB"),    // ~10 KB
        (1000, 100 * 1024, "100KB"), // ~100 KB
    ];

    for (feature_count, _approx_size, label) in sizes {
        let geojson = generate_feature_collection(feature_count, "Polygon");
        let actual_size = geojson.len();

        group.throughput(Throughput::Bytes(actual_size as u64));
        group.bench_with_input(
            BenchmarkId::new(label, format!("{}_features", feature_count)),
            &geojson,
            |b, json| {
                b.iter(|| {
                    black_box(serde_json::from_str::<serde_json::Value>(black_box(json)).ok());
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_parse_point_features,
    bench_parse_linestring_features,
    bench_parse_polygon_features,
    bench_serialize_features,
    bench_serialize_pretty,
    bench_parse_streaming,
    bench_parse_properties,
    bench_parse_complex_geometries,
    bench_parse_large_coordinates,
    bench_parse_real_world_sizes
);
criterion_main!(benches);
