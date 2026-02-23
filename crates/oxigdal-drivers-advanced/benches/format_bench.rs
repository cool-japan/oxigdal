//! Benchmarks for advanced format drivers.
#![allow(missing_docs)]

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use oxigdal_drivers_advanced::gml::*;
use oxigdal_drivers_advanced::gpkg::*;
use oxigdal_drivers_advanced::jp2::*;
use oxigdal_drivers_advanced::kml::*;
use std::hint::black_box;
use std::sync::atomic::{AtomicU32, Ordering};
use tempfile::NamedTempFile;

fn bench_jp2_image_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("jp2_image");

    for size in [256, 512, 1024].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            b.iter(|| {
                let components = vec![
                    ComponentInfo::new(0, 8, false),
                    ComponentInfo::new(1, 8, false),
                    ComponentInfo::new(2, 8, false),
                ];
                let img = Jp2Image::new(black_box(size), black_box(size), 3, 8, components);
                black_box(img);
            });
        });
    }

    group.finish();
}

fn bench_jp2_pixel_operations(c: &mut Criterion) {
    let components = vec![ComponentInfo::new(0, 8, false)];
    let mut img = Jp2Image::new(1024, 1024, 1, 8, components);

    c.bench_function("jp2_set_pixel", |b| {
        b.iter(|| {
            img.set_pixel(black_box(512), black_box(512), &[black_box(255)])
                .ok();
        });
    });

    c.bench_function("jp2_get_pixel", |b| {
        b.iter(|| {
            let pixel = img.get_pixel(black_box(512), black_box(512));
            black_box(pixel);
        });
    });
}

fn bench_jp2_metadata(c: &mut Criterion) {
    c.bench_function("jp2_metadata_creation", |b| {
        b.iter(|| {
            let mut metadata = Jp2Metadata::default();
            metadata.add_xml(black_box("<test>data</test>".to_string()));
            metadata.set_color_space(ColorSpace::Srgb);
            black_box(metadata);
        });
    });
}

fn bench_gpkg_creation(c: &mut Criterion) {
    c.bench_function("gpkg_create", |b| {
        b.iter(|| {
            let temp_file = NamedTempFile::new().ok();
            if let Some(f) = temp_file {
                let gpkg = GeoPackage::create(f.path()).ok();
                black_box(gpkg);
            }
        });
    });
}

fn bench_gpkg_feature_table(c: &mut Criterion) {
    let temp_file = NamedTempFile::new().ok();
    if let Some(f) = temp_file {
        if let Ok(mut gpkg) = GeoPackage::create(f.path()) {
            static COUNTER: AtomicU32 = AtomicU32::new(0);
            c.bench_function("gpkg_create_feature_table", |b| {
                b.iter(|| {
                    let table_name = format!(
                        "table_{}",
                        black_box(COUNTER.fetch_add(1, Ordering::Relaxed))
                    );
                    let table = gpkg
                        .create_feature_table(&table_name, GeometryType::Point, 4326)
                        .ok();
                    black_box(table);
                });
            });
        }
    }
}

fn bench_gpkg_extent(c: &mut Criterion) {
    c.bench_function("gpkg_extent_operations", |b| {
        b.iter(|| {
            let mut extent = Extent::new(
                black_box(0.0),
                black_box(0.0),
                black_box(100.0),
                black_box(100.0),
            );
            extent.expand(black_box(150.0), black_box(150.0));
            let contains = extent.contains(black_box(50.0), black_box(50.0));
            black_box((extent, contains));
        });
    });
}

fn bench_kml_document_creation(c: &mut Criterion) {
    c.bench_function("kml_document_creation", |b| {
        b.iter(|| {
            let mut doc = KmlDocument::new()
                .with_name(black_box("Test"))
                .with_description(black_box("Test document"));

            for i in 0..10 {
                let placemark = Placemark::new()
                    .with_name(format!("Point {}", i))
                    .with_geometry(KmlGeometry::Point(Coordinates::new(
                        black_box(-122.0 + i as f64),
                        black_box(37.0 + i as f64),
                    )));
                doc.add_placemark(placemark);
            }

            black_box(doc);
        });
    });
}

fn bench_kml_write(c: &mut Criterion) {
    let mut doc = KmlDocument::new().with_name("Test");
    for i in 0..100 {
        let placemark = Placemark::new()
            .with_name(format!("Point {}", i))
            .with_geometry(KmlGeometry::Point(Coordinates::new(-122.0, 37.0)));
        doc.add_placemark(placemark);
    }

    c.bench_function("kml_write", |b| {
        b.iter(|| {
            let mut buf = Vec::new();
            write_kml(&mut buf, black_box(&doc)).ok();
            black_box(buf);
        });
    });
}

fn bench_kml_coordinates(c: &mut Criterion) {
    c.bench_function("kml_coordinates_to_string", |b| {
        b.iter(|| {
            let coords =
                Coordinates::with_altitude(black_box(-122.08), black_box(37.42), black_box(100.0));
            let s = coords.to_kml_string();
            black_box(s);
        });
    });
}

fn bench_gml_feature_collection(c: &mut Criterion) {
    c.bench_function("gml_feature_collection", |b| {
        b.iter(|| {
            let mut collection = GmlFeatureCollection::new().with_crs(black_box("EPSG:4326"));

            for i in 0..50 {
                let mut feature = GmlFeature::new().with_id(format!("f{}", i)).with_geometry(
                    GmlGeometry::Point {
                        coordinates: vec![black_box(10.0 + i as f64), black_box(20.0 + i as f64)],
                    },
                );

                feature.add_property("name", format!("Feature {}", i));
                collection.add_feature(feature);
            }

            black_box(collection);
        });
    });
}

fn bench_gml_write(c: &mut Criterion) {
    let mut collection = GmlFeatureCollection::new();
    for i in 0..100 {
        let mut feature = GmlFeature::new().with_geometry(GmlGeometry::Point {
            coordinates: vec![10.0, 20.0],
        });
        feature.add_property("name", format!("Feature {}", i));
        collection.add_feature(feature);
    }

    c.bench_function("gml_write", |b| {
        b.iter(|| {
            let mut buf = Vec::new();
            write_gml(&mut buf, black_box(&collection)).ok();
            black_box(buf);
        });
    });
}

fn bench_gml_geometry(c: &mut Criterion) {
    c.bench_function("gml_point_creation", |b| {
        b.iter(|| {
            let point = GmlPoint::with_z(black_box(10.0), black_box(20.0), black_box(30.0));
            black_box(point);
        });
    });

    c.bench_function("gml_linestring_creation", |b| {
        b.iter(|| {
            let coords = (0..100).map(|i| vec![i as f64, i as f64 * 2.0]).collect();
            let line = GmlLineString::new(black_box(coords));
            black_box(line);
        });
    });
}

criterion_group!(
    benches,
    bench_jp2_image_creation,
    bench_jp2_pixel_operations,
    bench_jp2_metadata,
    bench_gpkg_creation,
    bench_gpkg_feature_table,
    bench_gpkg_extent,
    bench_kml_document_creation,
    bench_kml_write,
    bench_kml_coordinates,
    bench_gml_feature_collection,
    bench_gml_write,
    bench_gml_geometry,
);

criterion_main!(benches);
