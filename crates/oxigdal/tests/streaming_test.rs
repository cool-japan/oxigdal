//! Integration tests for FlatGeobuf and Shapefile feature streaming via
//! `StreamingExt::features()`.

use oxigdal::open::open;
use oxigdal::streaming::StreamingExt;

// ─── Shapefile streaming ─────────────────────────────────────────────────────

/// Write a minimal 3-point Shapefile, then verify that `features().count() == 3`.
#[cfg(feature = "shapefile")]
#[test]
fn test_stream_shapefile_features() {
    use oxigdal::shapefile::{
        ShapefileFeature, ShapefileSchemaBuilder, ShapefileWriter, shp::shapes::ShapeType,
    };
    use oxigdal_core::vector::{FieldValue, Geometry, Point};
    use std::collections::HashMap;

    let dir = std::env::temp_dir();
    let base = dir.join("stream_shp_test");

    // Build schema — a single "name" field
    let schema = ShapefileSchemaBuilder::new()
        .add_character_field("name", 50)
        .expect("schema")
        .build();

    let mut writer = ShapefileWriter::new(&base, ShapeType::Point, schema).expect("writer");

    // Three point features
    let features: Vec<ShapefileFeature> = vec![
        (1, 139.7, 35.7, "Tokyo"),
        (2, 2.35, 48.85, "Paris"),
        (3, -74.0, 40.7, "NewYork"),
    ]
    .into_iter()
    .map(|(n, x, y, name)| {
        let geom = Some(Geometry::Point(Point::new(x, y)));
        let mut attrs = HashMap::new();
        attrs.insert("name".to_string(), FieldValue::String(name.to_string()));
        ShapefileFeature::new(n, geom, attrs)
    })
    .collect();

    writer.write_features(&features).expect("write features");

    // Open the .shp file and stream
    let shp_path = base.with_extension("shp");
    let ds = open(&shp_path).expect("open shapefile");
    let count = ds.features().expect("features()").count();
    assert_eq!(count, 3, "should stream exactly 3 features from shapefile");
}

/// Verify that geometry bytes are non-empty for Point features from a Shapefile.
#[cfg(feature = "shapefile")]
#[test]
fn test_stream_shapefile_feature_has_wkb_geometry() {
    use oxigdal::shapefile::{
        ShapefileFeature, ShapefileSchemaBuilder, ShapefileWriter, shp::shapes::ShapeType,
    };
    use oxigdal_core::vector::{FieldValue, Geometry, Point};
    use std::collections::HashMap;

    let dir = std::env::temp_dir();
    let base = dir.join("stream_shp_wkb_test");

    let schema = ShapefileSchemaBuilder::new()
        .add_character_field("id", 10)
        .expect("schema")
        .build();

    let mut writer = ShapefileWriter::new(&base, ShapeType::Point, schema).expect("writer");

    let mut attrs = HashMap::new();
    attrs.insert("id".to_string(), FieldValue::String("A".to_string()));
    let feature = ShapefileFeature::new(1, Some(Geometry::Point(Point::new(10.0, 20.0))), attrs);

    writer.write_features(&[feature]).expect("write feature");

    let shp_path = base.with_extension("shp");
    let ds = open(&shp_path).expect("open");
    let mut stream = ds.features().expect("features()");
    let feat = stream.next().expect("first feature").expect("no error");
    assert!(
        feat.has_geometry(),
        "shapefile point feature must have WKB geometry"
    );
    // WKB Point LE: 1 + 4 + 8 + 8 = 21 bytes
    assert_eq!(feat.geometry_byte_len(), 21, "WKB Point should be 21 bytes");
}

/// Verify that Shapefile attribute properties are present in the streamed feature.
#[cfg(feature = "shapefile")]
#[test]
fn test_stream_shapefile_feature_properties() {
    use oxigdal::shapefile::{
        ShapefileFeature, ShapefileSchemaBuilder, ShapefileWriter, shp::shapes::ShapeType,
    };
    use oxigdal_core::vector::{FieldValue, Geometry, Point};
    use std::collections::HashMap;

    let dir = std::env::temp_dir();
    let base = dir.join("stream_shp_props_test");

    let schema = ShapefileSchemaBuilder::new()
        .add_character_field("city", 50)
        .expect("city field")
        .add_numeric_field("pop", 10, 0)
        .expect("pop field")
        .build();

    let mut writer = ShapefileWriter::new(&base, ShapeType::Point, schema).expect("writer");

    let mut attrs = HashMap::new();
    attrs.insert("city".to_string(), FieldValue::String("Berlin".to_string()));
    attrs.insert("pop".to_string(), FieldValue::Integer(3_600_000));
    let feature = ShapefileFeature::new(1, Some(Geometry::Point(Point::new(13.4, 52.5))), attrs);

    writer.write_features(&[feature]).expect("write");

    let shp_path = base.with_extension("shp");
    let ds = open(&shp_path).expect("open");
    let mut stream = ds.features().expect("features()");
    let feat = stream.next().expect("first").expect("no error");

    let city = feat.properties.get("city").expect("city property");
    assert_eq!(city.as_str(), Some("Berlin"));
}

// ─── FlatGeobuf streaming ────────────────────────────────────────────────────

/// Write a minimal 3-point FlatGeobuf file, then verify `features().count() == 3`.
#[cfg(feature = "flatgeobuf")]
#[test]
fn test_stream_flatgeobuf_features() {
    use oxigdal::flatgeobuf::{
        FlatGeobufWriterBuilder,
        header::{Column, ColumnType, GeometryType},
    };
    use oxigdal_core::vector::{Feature, FieldValue, Geometry, Point};
    use std::io::Cursor;

    // Write three Point features to an in-memory buffer, then flush to a
    // temp file so the `open()` path-based API can pick it up.
    let buf: Cursor<Vec<u8>> = Cursor::new(Vec::new());

    let writer_builder = FlatGeobufWriterBuilder::new(GeometryType::Point)
        .with_column(Column::new("name", ColumnType::String));

    let mut writer = writer_builder.build(buf).expect("build writer");

    let pts = [
        (139.7_f64, 35.7_f64, "Tokyo"),
        (2.35, 48.85, "Paris"),
        (-74.0, 40.7, "NewYork"),
    ];

    for (_x, _y, name) in &pts {
        let geom = Geometry::Point(Point::new(_x.to_owned(), _y.to_owned()));
        let mut feat = Feature::new(geom);
        feat.set_property("name", FieldValue::String(name.to_string()));
        writer.add_feature(&feat).expect("add feature");
    }

    let buf = writer.finish().expect("finish");
    let bytes = buf.into_inner();

    // Write to a temp file
    let dir = std::env::temp_dir();
    let path = dir.join("stream_fgb_test.fgb");
    std::fs::write(&path, &bytes).expect("write fgb file");

    let ds = open(&path).expect("open fgb");
    let count = ds.features().expect("features()").count();
    assert_eq!(count, 3, "should stream exactly 3 features from FlatGeobuf");
}

/// Verify WKB geometry is present for FlatGeobuf Point features.
#[cfg(feature = "flatgeobuf")]
#[test]
fn test_stream_flatgeobuf_feature_has_wkb_geometry() {
    use oxigdal::flatgeobuf::{FlatGeobufWriterBuilder, header::GeometryType};
    use oxigdal_core::vector::{Feature, Geometry, Point};
    use std::io::Cursor;

    let buf: Cursor<Vec<u8>> = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriterBuilder::new(GeometryType::Point)
        .build(buf)
        .expect("build");

    let geom = Geometry::Point(Point::new(10.0, 20.0));
    let feat = Feature::new(geom);
    writer.add_feature(&feat).expect("add");

    let bytes = writer.finish().expect("finish").into_inner();
    let dir = std::env::temp_dir();
    let path = dir.join("stream_fgb_geom_test.fgb");
    std::fs::write(&path, &bytes).expect("write");

    let ds = open(&path).expect("open");
    let mut stream = ds.features().expect("features()");
    let feat = stream.next().expect("first").expect("no error");
    assert!(
        feat.has_geometry(),
        "FlatGeobuf point feature must have WKB geometry"
    );
}
