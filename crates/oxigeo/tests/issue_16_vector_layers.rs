#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Regression tests for cool-japan/oxigeo#16 — "vector layer support is
//! incomplete".
//!
//! Two defects were reported:
//!
//! 1. `Dataset::open("x.gpkg")` reported `layer_count() == 0` for a GeoPackage
//!    with one layer, because [`Dataset::open`] never called into the GPKG
//!    driver — the `.gpkg` arm fell through to an empty `DatasetInfo`.
//! 2. There was no public API to open a layer and read its features at all.
//!
//! These tests pin both halves: the metadata probe, and the
//! `layers()` / `layer()` / `layer_by_name()` / `Layer::features()` API.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Per-test scratch fixture inside the system temp dir (house policy: no
/// hardcoded absolute paths).  The leaf name embeds the process id and a
/// monotonic counter so concurrent runs never collide; dropping the guard
/// removes the file and any sidecar it may have (`.dbf`, `.shx`, `.prj`).
struct TempFixture(PathBuf);

impl TempFixture {
    fn new(name: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        Self(std::env::temp_dir().join(format!(
            "oxigeo_issue16_{}_{seq}_{name}",
            std::process::id()
        )))
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn path_str(&self) -> &str {
        self.0.to_str().expect("fixture path is valid UTF-8")
    }
}

impl Drop for TempFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        for extension in ["shp", "dbf", "shx", "prj", "cpg"] {
            let _ = std::fs::remove_file(self.0.with_extension(extension));
        }
    }
}

// ─── GeoPackage ──────────────────────────────────────────────────────────────

/// A feature table for the fixture writer: name plus `(fid, x, y)` points.
#[cfg(feature = "gpkg")]
type PointTable<'a> = (&'a str, Vec<(i64, f64, f64)>);

/// Write a GeoPackage with the given point feature tables.
#[cfg(feature = "gpkg")]
fn write_gpkg(fixture: &TempFixture, tables: &[PointTable<'_>]) {
    use std::io::Write as _;

    let mut builder = oxigeo::gpkg::GeoPackageBuilder::new(4326);
    for (name, points) in tables {
        builder = builder.add_feature_table(*name, "POINT", points.clone());
    }
    let bytes = builder.build().expect("build GeoPackage");

    let mut file = std::fs::File::create(fixture.path()).expect("create .gpkg");
    file.write_all(&bytes).expect("write .gpkg");
}

/// The reporter's own program: open a `.gpkg`, print format / layers /
/// features.  Before the fix this reported `0` layers and `None` features.
#[cfg(feature = "gpkg")]
#[test]
fn test_issue_16_gpkg_layer_count_is_not_zero() {
    let fixture = TempFixture::new("count.gpkg");
    write_gpkg(
        &fixture,
        &[("cities", vec![(1, 10.0, 20.0), (2, 30.0, 40.0)])],
    );

    let dataset = oxigeo::Dataset::open(fixture.path_str()).expect("Dataset open");

    assert_eq!(dataset.format(), oxigeo::DatasetFormat::GeoPackage);
    assert_eq!(
        dataset.layer_count(),
        1,
        "a GeoPackage with one feature table must report one layer (#16)"
    );
    assert_eq!(
        dataset.feature_count(),
        Some(2),
        "feature_count must come from the real GPKG driver, not stay None (#16)"
    );
    assert_eq!(dataset.crs(), Some("EPSG:4326"));

    let bounds = dataset.bounds().expect("gpkg_contents carries an extent");
    assert!((bounds.min_x - 10.0).abs() < f64::EPSILON);
    assert!((bounds.max_y - 40.0).abs() < f64::EPSILON);
}

/// `layer_count()` must track the number of feature tables, and every one of
/// them must be reachable by index and by name.
#[cfg(feature = "gpkg")]
#[test]
fn test_issue_16_gpkg_layers_by_index_and_name() {
    let fixture = TempFixture::new("multi.gpkg");
    write_gpkg(
        &fixture,
        &[
            ("cities", vec![(1, 10.0, 20.0)]),
            ("stations", vec![(1, 1.0, 2.0), (2, 3.0, 4.0)]),
        ],
    );

    let dataset = oxigeo::Dataset::open(fixture.path_str()).expect("Dataset open");
    assert_eq!(dataset.layer_count(), 2);
    assert_eq!(
        dataset.layer_names().expect("layer_names"),
        vec!["cities".to_string(), "stations".to_string()]
    );

    let first = dataset.layer(0).expect("layer(0)");
    assert_eq!(first.name(), "cities");
    assert_eq!(first.index(), 0);
    assert_eq!(first.geometry_type(), Some("Point"));
    assert_eq!(first.feature_count(), Some(1));
    assert_eq!(first.crs(), Some("EPSG:4326"));

    let by_name = dataset.layer_by_name("stations").expect("layer_by_name");
    assert_eq!(by_name.name(), "stations");
    assert_eq!(by_name.feature_count(), Some(2));
    assert_eq!(
        by_name.field_names(),
        ["fid".to_string()],
        "the geometry column must not be listed as an attribute field"
    );

    // Lookups that cannot succeed report which layers do exist.
    let missing = dataset.layer_by_name("no_such_layer").unwrap_err();
    assert!(
        missing.to_string().contains("no_such_layer"),
        "unknown layer error should name the layer: {missing}"
    );
    let out_of_range = dataset.layer(7).unwrap_err();
    assert!(
        out_of_range.to_string().contains('7'),
        "out-of-range error should name the index: {out_of_range}"
    );
}

/// Reading features: geometry decodes from the GPKG geometry blob, attributes
/// come back keyed by column name, and the `INTEGER PRIMARY KEY` column is
/// resolved even when SQLite stores it as NULL in the record payload.
#[cfg(feature = "gpkg")]
#[test]
fn test_issue_16_gpkg_features_have_geometry_and_fields() {
    use oxigeo::{FeatureId, FieldValue, Geometry};

    let fixture = TempFixture::new("features.gpkg");
    write_gpkg(
        &fixture,
        &[("cities", vec![(1, 10.0, 20.0), (2, 30.0, 40.0)])],
    );

    let dataset = oxigeo::Dataset::open(fixture.path_str()).expect("Dataset open");
    let layer = dataset.layer(0).expect("layer(0)");
    let features: Vec<oxigeo::Feature> = layer.features().expect("features").collect();

    assert_eq!(features.len(), 2);

    match features[0].geometry.as_ref().expect("geometry") {
        Geometry::Point(point) => {
            assert!((point.coord.x - 10.0).abs() < f64::EPSILON);
            assert!((point.coord.y - 20.0).abs() < f64::EPSILON);
        }
        other => panic!("expected a Point, got {other:?}"),
    }

    assert_eq!(features[0].id, Some(FeatureId::Integer(1)));
    assert_eq!(
        features[0].properties.get("fid"),
        Some(&FieldValue::Integer(1)),
        "attribute columns must be readable by name"
    );
    assert!(
        !features[0].properties.contains_key("geom"),
        "the geometry column must not leak into the attribute map"
    );

    match features[1].geometry.as_ref().expect("geometry") {
        Geometry::Point(point) => assert!((point.coord.x - 30.0).abs() < f64::EPSILON),
        other => panic!("expected a Point, got {other:?}"),
    }
}

// ─── Shapefile ───────────────────────────────────────────────────────────────

/// Features of a Shapefile must be readable through the same layer API, with
/// their `.dbf` attributes attached.
#[cfg(feature = "shapefile")]
#[test]
fn test_issue_16_shapefile_features_with_attributes() {
    use oxigeo::core_types::vector::Point;
    use oxigeo::shapefile::shp::shapes::ShapeType;
    use oxigeo::shapefile::{ShapefileFeature, ShapefileSchemaBuilder, ShapefileWriter};
    use oxigeo::{FieldValue, Geometry};
    use std::collections::HashMap;

    let fixture = TempFixture::new("cities");

    let schema = ShapefileSchemaBuilder::new()
        .add_character_field("NAME", 32)
        .expect("NAME field")
        .add_numeric_field("POP", 10, 0)
        .expect("POP field")
        .build();

    let make = |record: i32, x: f64, y: f64, name: &str, population: i64| {
        let mut attributes = HashMap::new();
        attributes.insert("NAME".to_string(), FieldValue::String(name.to_string()));
        attributes.insert("POP".to_string(), FieldValue::Integer(population));
        ShapefileFeature::new(record, Some(Geometry::Point(Point::new(x, y))), attributes)
    };

    let mut writer = ShapefileWriter::new(fixture.path(), ShapeType::Point, schema)
        .expect("create ShapefileWriter");
    writer
        .write_features(&[
            make(1, 135.76, 35.02, "Kyoto", 1_463_723),
            make(2, 139.69, 35.69, "Tokyo", 13_960_236),
        ])
        .expect("write features");

    let shp_path = fixture.path().with_extension("shp");
    let dataset =
        oxigeo::Dataset::open(shp_path.to_str().expect("utf8")).expect("open shapefile dataset");

    assert_eq!(dataset.layer_count(), 1);

    let layers = dataset.layers().expect("layers");
    assert_eq!(layers.len(), 1, "a Shapefile holds exactly one layer");

    let layer = &layers[0];
    assert_eq!(layer.name(), fixture.path().file_stem().expect("stem"));
    assert_eq!(layer.geometry_type(), Some("Point"));
    assert_eq!(layer.feature_count(), Some(2));
    assert_eq!(
        layer.field_names(),
        ["NAME".to_string(), "POP".to_string()],
        "field names come from the .dbf header"
    );

    let features: Vec<oxigeo::Feature> = layer.features().expect("features").collect();
    assert_eq!(features.len(), 2);

    match features[0].geometry.as_ref().expect("geometry") {
        Geometry::Point(point) => {
            assert!((point.coord.x - 135.76).abs() < 1e-9);
            assert!((point.coord.y - 35.02).abs() < 1e-9);
        }
        other => panic!("expected a Point, got {other:?}"),
    }

    assert_eq!(
        features[0].properties.get("NAME"),
        Some(&FieldValue::String("Kyoto".to_string()))
    );
    match features[1].properties.get("POP") {
        Some(FieldValue::Integer(population)) => assert_eq!(*population, 13_960_236),
        Some(FieldValue::Float(population)) => {
            assert!((population - 13_960_236.0).abs() < 1.0);
        }
        other => panic!("expected a numeric POP attribute, got {other:?}"),
    }

    // The same layer is reachable by its name.
    let stem = fixture
        .path()
        .file_stem()
        .and_then(|s| s.to_str())
        .expect("stem");
    assert_eq!(
        dataset.layer_by_name(stem).expect("layer_by_name").name(),
        stem
    );
}

// ─── GeoJSON ─────────────────────────────────────────────────────────────────

/// GeoJSON is a default feature, so the layer API must work there too.
#[cfg(feature = "geojson")]
#[test]
fn test_issue_16_geojson_layer_features() {
    use oxigeo::{FieldValue, Geometry};
    use std::io::Write as _;

    let fixture = TempFixture::new("places.geojson");
    let mut file = std::fs::File::create(fixture.path()).expect("create .geojson");
    file.write_all(
        br#"{"type":"FeatureCollection","features":[
            {"type":"Feature","id":7,
             "geometry":{"type":"Point","coordinates":[135.76,35.02]},
             "properties":{"name":"Kyoto","pop":1463723}},
            {"type":"Feature",
             "geometry":{"type":"Point","coordinates":[139.69,35.69]},
             "properties":{"name":"Tokyo","pop":13960236}}
        ]}"#,
    )
    .expect("write .geojson");
    drop(file);

    let dataset = oxigeo::Dataset::open(fixture.path_str()).expect("Dataset open");
    let layer = dataset.layer(0).expect("layer(0)");

    assert_eq!(layer.feature_count(), Some(2));
    assert_eq!(layer.geometry_type(), Some("Point"));
    assert_eq!(layer.field_names(), ["name".to_string(), "pop".to_string()]);

    let features: Vec<oxigeo::Feature> = layer.features().expect("features").collect();
    assert_eq!(features.len(), 2);
    assert_eq!(
        features[0].properties.get("name"),
        Some(&FieldValue::String("Kyoto".to_string()))
    );
    match features[1].geometry.as_ref().expect("geometry") {
        Geometry::Point(point) => assert!((point.coord.y - 35.69).abs() < 1e-9),
        other => panic!("expected a Point, got {other:?}"),
    }
}

// ─── Honest errors for formats without a layer reader ────────────────────────

/// A raster dataset has no vector layers: `layers()` must say so with a typed
/// error instead of pretending the file has none.
#[cfg(feature = "geotiff")]
#[test]
fn test_issue_16_raster_layers_report_unsupported() {
    use std::io::Write as _;

    // Minimal little-endian TIFF: header + one IFD describing a 1×1 image.
    let mut tiff = Vec::new();
    tiff.extend_from_slice(&[0x49, 0x49, 42, 0]); // II, version 42
    tiff.extend_from_slice(&8u32.to_le_bytes()); // first IFD at offset 8
    let entries: [(u16, u16, u32, u32); 4] = [
        (256, 3, 1, 1), // ImageWidth  = 1
        (257, 3, 1, 1), // ImageLength = 1
        (258, 3, 1, 8), // BitsPerSample = 8
        (277, 3, 1, 1), // SamplesPerPixel = 1
    ];
    tiff.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for (tag, field_type, count, value) in entries {
        tiff.extend_from_slice(&tag.to_le_bytes());
        tiff.extend_from_slice(&field_type.to_le_bytes());
        tiff.extend_from_slice(&count.to_le_bytes());
        tiff.extend_from_slice(&value.to_le_bytes());
    }
    tiff.extend_from_slice(&0u32.to_le_bytes()); // no next IFD

    let fixture = TempFixture::new("raster.tif");
    let mut file = std::fs::File::create(fixture.path()).expect("create .tif");
    file.write_all(&tiff).expect("write .tif");
    drop(file);

    let dataset = oxigeo::Dataset::open(fixture.path_str()).expect("Dataset open");
    let error = dataset.layers().unwrap_err();
    assert!(
        matches!(error, oxigeo::OxiGeoError::NotSupported { .. }),
        "raster layers() must return NotSupported, got {error:?}"
    );
    assert!(
        error.to_string().contains("GTiff"),
        "the error should name the driver: {error}"
    );
}
