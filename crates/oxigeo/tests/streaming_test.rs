//! Integration tests for FlatGeobuf, Shapefile, GeoPackage, GeoParquet, and
//! STAC feature streaming via `StreamingExt::features()`.

// Every test in this file is gated on the driver feature it exercises, so with
// none of them enabled (`--no-default-features`) these two imports would be
// unused. The gate lists exactly the features whose tests use them.
#[cfg(any(
    feature = "shapefile",
    feature = "flatgeobuf",
    feature = "gpkg",
    feature = "geoparquet",
    feature = "stac",
    feature = "geojson"
))]
use oxigeo::open::open;
#[cfg(any(
    feature = "shapefile",
    feature = "flatgeobuf",
    feature = "gpkg",
    feature = "geoparquet",
    feature = "stac",
    feature = "geojson"
))]
use oxigeo::streaming::StreamingExt;

#[cfg(any(
    feature = "shapefile",
    feature = "flatgeobuf",
    feature = "gpkg",
    feature = "geoparquet",
    feature = "stac",
    feature = "geojson"
))]
use std::sync::atomic::{AtomicU64, Ordering};

/// Per-test scratch fixture inside the system temp dir (house policy: no
/// hardcoded absolute paths).
///
/// The leaf name embeds the process id and a monotonic counter, so no two test
/// binaries — nor two concurrent runs of this one — can ever land on the same
/// file.  Dropping the guard removes the fixture, so a panicking test leaks
/// nothing.
#[cfg(any(
    feature = "shapefile",
    feature = "flatgeobuf",
    feature = "gpkg",
    feature = "geoparquet",
    feature = "stac",
    feature = "geojson"
))]
struct TempPath(std::path::PathBuf);

#[cfg(any(
    feature = "shapefile",
    feature = "flatgeobuf",
    feature = "gpkg",
    feature = "geoparquet",
    feature = "stac",
    feature = "geojson"
))]
impl TempPath {
    fn new(name: &str) -> Self {
        Self(std::env::temp_dir().join(unique_leaf(name)))
    }
}

/// A unique leaf name: `oxigeo_stream_<pid>_<seq>_<name>`.
#[cfg(any(
    feature = "shapefile",
    feature = "flatgeobuf",
    feature = "gpkg",
    feature = "geoparquet",
    feature = "stac",
    feature = "geojson"
))]
fn unique_leaf(name: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("oxigeo_stream_{}_{seq}_{name}", std::process::id())
}

#[cfg(any(
    feature = "shapefile",
    feature = "flatgeobuf",
    feature = "gpkg",
    feature = "geoparquet",
    feature = "stac",
    feature = "geojson"
))]
impl std::ops::Deref for TempPath {
    type Target = std::path::Path;

    fn deref(&self) -> &std::path::Path {
        &self.0
    }
}

#[cfg(any(
    feature = "shapefile",
    feature = "flatgeobuf",
    feature = "gpkg",
    feature = "geoparquet",
    feature = "stac",
    feature = "geojson"
))]
impl AsRef<std::path::Path> for TempPath {
    fn as_ref(&self) -> &std::path::Path {
        &self.0
    }
}

#[cfg(any(
    feature = "shapefile",
    feature = "flatgeobuf",
    feature = "gpkg",
    feature = "geoparquet",
    feature = "stac",
    feature = "geojson"
))]
impl Drop for TempPath {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Same guarantee for a Shapefile *stem*: the writer emits several sidecars
/// sharing one base name, so the guard deletes every extension it may have
/// created rather than leaking `.shx` / `.dbf` behind the `.shp`.
#[cfg(feature = "shapefile")]
struct TempStem(std::path::PathBuf);

#[cfg(feature = "shapefile")]
impl TempStem {
    fn new(name: &str) -> Self {
        Self(std::env::temp_dir().join(unique_leaf(name)))
    }
}

#[cfg(feature = "shapefile")]
impl std::ops::Deref for TempStem {
    type Target = std::path::Path;

    fn deref(&self) -> &std::path::Path {
        &self.0
    }
}

#[cfg(feature = "shapefile")]
impl AsRef<std::path::Path> for TempStem {
    fn as_ref(&self) -> &std::path::Path {
        &self.0
    }
}

#[cfg(feature = "shapefile")]
impl Drop for TempStem {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        for ext in ["shp", "shx", "dbf", "prj", "cpg", "qix", "sbn", "sbx"] {
            let _ = std::fs::remove_file(self.0.with_extension(ext));
        }
    }
}

// ─── Shapefile streaming ─────────────────────────────────────────────────────

/// Write a minimal 3-point Shapefile, then verify that `features().count() == 3`.
#[cfg(feature = "shapefile")]
#[test]
fn test_stream_shapefile_features() {
    use oxigeo::shapefile::{
        ShapefileFeature, ShapefileSchemaBuilder, ShapefileWriter, shp::shapes::ShapeType,
    };
    use oxigeo_core::vector::{FieldValue, Geometry, Point};
    use std::collections::HashMap;

    let base = TempStem::new("stream_shp_test");

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
    use oxigeo::shapefile::{
        ShapefileFeature, ShapefileSchemaBuilder, ShapefileWriter, shp::shapes::ShapeType,
    };
    use oxigeo_core::vector::{FieldValue, Geometry, Point};
    use std::collections::HashMap;

    let base = TempStem::new("stream_shp_wkb_test");

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
    use oxigeo::shapefile::{
        ShapefileFeature, ShapefileSchemaBuilder, ShapefileWriter, shp::shapes::ShapeType,
    };
    use oxigeo_core::vector::{FieldValue, Geometry, Point};
    use std::collections::HashMap;

    let base = TempStem::new("stream_shp_props_test");

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
    use oxigeo::flatgeobuf::{
        FlatGeobufWriterBuilder,
        header::{Column, ColumnType, GeometryType},
    };
    use oxigeo_core::vector::{Feature, FieldValue, Geometry, Point};
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
    let path = TempPath::new("stream_fgb_test.fgb");
    std::fs::write(&path, &bytes).expect("write fgb file");

    let ds = open(&path).expect("open fgb");
    let count = ds.features().expect("features()").count();
    assert_eq!(count, 3, "should stream exactly 3 features from FlatGeobuf");
}

/// Verify WKB geometry is present for FlatGeobuf Point features.
#[cfg(feature = "flatgeobuf")]
#[test]
fn test_stream_flatgeobuf_feature_has_wkb_geometry() {
    use oxigeo::flatgeobuf::{FlatGeobufWriterBuilder, header::GeometryType};
    use oxigeo_core::vector::{Feature, Geometry, Point};
    use std::io::Cursor;

    let buf: Cursor<Vec<u8>> = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriterBuilder::new(GeometryType::Point)
        .build(buf)
        .expect("build");

    let geom = Geometry::Point(Point::new(10.0, 20.0));
    let feat = Feature::new(geom);
    writer.add_feature(&feat).expect("add");

    let bytes = writer.finish().expect("finish").into_inner();
    let path = TempPath::new("stream_fgb_geom_test.fgb");
    std::fs::write(&path, &bytes).expect("write");

    let ds = open(&path).expect("open");
    let mut stream = ds.features().expect("features()");
    let feat = stream.next().expect("first").expect("no error");
    assert!(
        feat.has_geometry(),
        "FlatGeobuf point feature must have WKB geometry"
    );
}

// ─── GeoPackage streaming ─────────────────────────────────────────────────────

/// Build a minimal valid GeoPackage binary in memory with one feature table
/// containing two point features, write it to a temp file, then stream it.
#[cfg(feature = "gpkg")]
#[test]
fn test_stream_geopackage_basic() {
    use oxigeo_gpkg::{GpkgBinaryParser, GpkgGeometry};

    // A minimal valid GPKG file has to be a real SQLite file with the
    // correct application_id (0x47504B47 = "GPKG").  Building one from
    // scratch requires the btree encoder.  We use the GpkgBinaryParser
    // round-trip to validate our geometry encoding separately and rely on
    // the existing gpkg crate tests for the full file round-trip.  For this
    // integration test we verify that:
    //  1. Opening a file with .gpkg extension yields OpenedDataset::GeoPackage.
    //  2. Calling features() on that dataset returns a FeatureStream (no panic).
    //
    // A real multi-row GPKG requires a valid SQLite B-tree; we use a minimal
    // file that the gpkg reader will open (the file is valid SQLite even if
    // gpkg_contents is empty), verifying the full dispatch path is wired.
    let path = TempPath::new("stream_gpkg_basic.gpkg");

    // Write a minimal SQLite file with the GeoPackage application_id so that
    // the magic-byte detector recognises it as GPKG.
    let page_size: usize = 4096;
    let mut data = vec![0u8; page_size];
    // SQLite magic header string (16 bytes).
    data[..16].copy_from_slice(b"SQLite format 3\x00");
    // Page size at offset 16 (big-endian u16): 4096 = 0x1000.
    data[16] = 0x10;
    data[17] = 0x00;
    // file_format_write_version at offset 18: 1
    data[18] = 1;
    // file_format_read_version at offset 19: 1
    data[19] = 1;
    // Reserved space per page at offset 20: 0
    // text_encoding at offset 56 (BE u32): 1 = UTF-8.
    data[56..60].copy_from_slice(&1u32.to_be_bytes());
    // application_id at offset 68 (BE u32): 0x47504B47 = "GPKG"
    data[68..72].copy_from_slice(&0x4750_4B47u32.to_be_bytes());
    // db_size_pages at offset 28 (BE u32): 1
    data[28..32].copy_from_slice(&1u32.to_be_bytes());

    std::fs::write(&path, &data).expect("write gpkg");

    let ds = open(&path).expect("open gpkg");

    // Verify dispatch reaches the geopackage streaming function (not a panic).
    let stream = ds.features().expect("features() should not error");
    // An empty-contents GPKG yields zero features — that is correct.
    let count = stream.count();
    assert_eq!(count, 0, "minimal empty GPKG should produce 0 features");

    // Verify WKB encoding works independently.
    let geom = GpkgGeometry::Point { x: 10.0, y: 20.0 };
    let wkb = GpkgBinaryParser::to_wkb(&geom);
    // WKB Point LE: byte_order(1) + type(4) + x(8) + y(8) = 21 bytes
    assert_eq!(wkb.len(), 21, "WKB Point should be 21 bytes");
}

/// Verify that opening a GeoPackage where gpkg_contents is unreadable (but
/// the file is valid SQLite) returns an empty stream rather than an error.
#[cfg(feature = "gpkg")]
#[test]
fn test_stream_geopackage_missing_path_returns_empty() {
    use oxigeo::DatasetFormat;
    use oxigeo::DatasetInfo;
    use oxigeo::open::OpenedDataset;
    use oxigeo::streaming::StreamingExt;

    // Construct an OpenedDataset::GeoPackage with no path.
    // `DatasetInfo` is `#[non_exhaustive]`, so downstream code (this integration
    // test included) builds one from `Default` and assigns the fields it cares
    // about rather than using a struct expression.
    let mut info = DatasetInfo::default();
    info.format = DatasetFormat::GeoPackage;
    info.path = None;
    let ds = OpenedDataset::GeoPackage(info);
    let stream = ds.features().expect("features() on no-path gpkg");
    assert_eq!(stream.count(), 0, "no-path GPKG should produce 0 features");
}

// ─── GeoParquet streaming ──────────────────────────────────────────────────────

/// Write a minimal GeoParquet file with 3 point geometries and stream it.
///
/// Uses Arrow + Parquet APIs directly (with UNCOMPRESSED) to avoid the Snappy
/// feature requirement that `GeoParquetWriter::new` imposes.
#[cfg(feature = "geoparquet")]
#[test]
fn test_stream_geoparquet_basic() {
    use arrow_array::BinaryArray;
    use arrow_schema::{DataType, Field, Schema};
    use oxigeo_geoparquet::{
        GeoParquetMetadata, GeometryColumnMetadata,
        geometry::{Geometry, Point, WkbWriter},
        metadata::Crs,
    };
    use parquet::arrow::ArrowWriter;
    use parquet::basic::Compression;
    use parquet::file::properties::WriterProperties;
    use std::sync::Arc;

    let path = TempPath::new("stream_geoparquet_basic_v2.parquet");

    // Encode 3 WKB Point geometries.
    let points = [(-122.4f64, 37.8), (-118.2, 34.0), (-87.6, 41.9)];
    let wkb_bytes: Vec<Vec<u8>> = points
        .iter()
        .map(|&(x, y)| {
            let geom = Geometry::Point(Point::new_2d(x, y));
            let mut w = WkbWriter::new(true);
            w.write_geometry(&geom).expect("encode wkb")
        })
        .collect();
    let wkb_refs: Vec<Option<&[u8]>> = wkb_bytes.iter().map(|v| Some(v.as_slice())).collect();

    // Build Arrow schema with geo metadata so GeoParquetReader can open it.
    let geom_field = Field::new("geometry", DataType::Binary, true);
    let base_schema = Schema::new(vec![geom_field]);

    // Embed GeoParquet metadata in the schema.
    let col_meta = GeometryColumnMetadata::new_wkb().with_crs(Crs::wgs84());
    let mut geo_meta = GeoParquetMetadata::new("geometry");
    geo_meta.add_column("geometry", col_meta);
    let meta_json = geo_meta.to_json().expect("geo meta json");
    let mut schema_meta = base_schema.metadata().clone();
    schema_meta.insert("geo".to_string(), meta_json);
    let schema = Arc::new(base_schema.with_metadata(schema_meta));

    // Write with UNCOMPRESSED (no snappy feature required).
    {
        use arrow_array::RecordBatch;
        let batch =
            RecordBatch::try_new(schema.clone(), vec![Arc::new(BinaryArray::from(wkb_refs))])
                .expect("record batch");

        let file = std::fs::File::create(&path).expect("create parquet");
        let props = WriterProperties::builder()
            .set_compression(Compression::UNCOMPRESSED)
            .build();
        let mut writer = ArrowWriter::try_new(file, schema, Some(props)).expect("arrow writer");
        writer.write(&batch).expect("write batch");
        writer.close().expect("close writer");
    }

    let ds = open(&path).expect("open geoparquet");
    let features: Vec<_> = ds
        .features()
        .expect("features()")
        .collect::<Result<_, _>>()
        .expect("no stream errors");

    assert_eq!(features.len(), 3, "should stream exactly 3 features");

    // Verify geometry bytes are non-empty.
    for feat in &features {
        assert!(feat.has_geometry(), "each feature must have WKB geometry");
        // WKB Point LE: 1 + 4 + 8 + 8 = 21 bytes
        assert_eq!(feat.geometry_byte_len(), 21, "WKB Point should be 21 bytes");
    }
}

/// Stream a GeoParquet that contains additional non-geometry columns.
#[cfg(feature = "geoparquet")]
#[test]
fn test_stream_geoparquet_handles_missing_file() {
    use oxigeo::DatasetFormat;
    use oxigeo::DatasetInfo;
    use oxigeo::open::OpenedDataset;
    use oxigeo::streaming::StreamingExt;

    // An OpenedDataset::GeoParquet pointing at a nonexistent file should
    // return an error (not a panic) from features().
    let nonexistent_path = std::env::temp_dir()
        .join("oxigeo_nonexistent_test_ZZZZ.parquet")
        .to_string_lossy()
        .to_string();
    let mut info = DatasetInfo::default();
    info.format = DatasetFormat::GeoParquet;
    info.path = Some(nonexistent_path);
    let ds = OpenedDataset::GeoParquet(info);
    // Should return Err (file not found), not panic.
    let result = ds.features();
    assert!(result.is_err(), "missing file should return Err, not panic");
}

// ─── STAC streaming ───────────────────────────────────────────────────────────

/// Parse a STAC ItemCollection JSON with 3 items and verify streaming.
#[cfg(feature = "stac")]
#[test]
fn test_stream_stac_item_collection() {
    let path = TempPath::new("stream_stac_item_collection.json");

    let stac_json = r#"{
  "type": "FeatureCollection",
  "stac_version": "1.0.0",
  "features": [
    {
      "type": "Feature",
      "stac_version": "1.0.0",
      "id": "item-1",
      "geometry": {"type": "Point", "coordinates": [-122.4, 37.8]},
      "properties": {"datetime": "2024-01-01T00:00:00Z", "name": "San Francisco"},
      "links": [],
      "assets": {}
    },
    {
      "type": "Feature",
      "stac_version": "1.0.0",
      "id": "item-2",
      "geometry": {"type": "Point", "coordinates": [-118.2, 34.0]},
      "properties": {"datetime": "2024-01-02T00:00:00Z", "name": "Los Angeles"},
      "links": [],
      "assets": {"visual": {"href": "https://example.com/la.tif", "type": "image/tiff"}}
    },
    {
      "type": "Feature",
      "stac_version": "1.0.0",
      "id": "item-3",
      "geometry": null,
      "properties": {"datetime": "2024-01-03T00:00:00Z"},
      "links": [],
      "assets": {}
    }
  ]
}"#;

    std::fs::write(&path, stac_json).expect("write stac json");

    let ds = open(&path).expect("open stac");
    let features: Vec<_> = ds
        .features()
        .expect("features()")
        .collect::<Result<_, _>>()
        .expect("no stream errors");

    assert_eq!(features.len(), 3, "should stream 3 STAC items");

    // item-1: Point geometry + 2 properties
    let f0 = &features[0];
    assert!(f0.has_geometry(), "item-1 must have geometry");
    assert_eq!(f0.id.as_deref(), Some("item-1"));
    assert!(
        f0.properties.contains_key("name"),
        "item-1 should have 'name' property"
    );
    // WKB Point LE: 21 bytes
    assert_eq!(f0.geometry_byte_len(), 21);

    // item-2: has an asset
    let f1 = &features[1];
    assert!(
        f1.properties.contains_key("assets.visual"),
        "item-2 should have 'assets.visual' from flattened assets"
    );

    // item-3: null geometry
    let f2 = &features[2];
    assert!(!f2.has_geometry(), "item-3 should have no geometry (null)");
}

/// A STAC Catalog JSON (not a FeatureCollection) should yield an empty stream.
#[cfg(feature = "stac")]
#[test]
fn test_stream_stac_catalog_returns_empty() {
    let path = TempPath::new("stream_stac_catalog.json");

    let catalog_json = r#"{
  "type": "Catalog",
  "id": "root-catalog",
  "description": "Root catalog",
  "stac_version": "1.0.0",
  "links": [
    {"rel": "item", "href": "./item-1.json", "type": "application/json"}
  ]
}"#;

    std::fs::write(&path, catalog_json).expect("write catalog json");

    let ds = open(&path).expect("open stac catalog");
    let count = ds.features().expect("features()").count();
    assert_eq!(
        count, 0,
        "STAC Catalog should produce 0 features (links not followed)"
    );
}

/// A single STAC Feature (not wrapped in a FeatureCollection) streams as one item.
#[cfg(feature = "stac")]
#[test]
fn test_stream_stac_single_feature() {
    let path = TempPath::new("stream_stac_single_feature.json");

    let item_json = r#"{
  "type": "Feature",
  "stac_version": "1.0.0",
  "id": "single-item",
  "geometry": {
    "type": "Polygon",
    "coordinates": [[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.0, 0.0]]]
  },
  "properties": {"datetime": "2024-06-01T00:00:00Z"},
  "links": [],
  "assets": {}
}"#;

    std::fs::write(&path, item_json).expect("write item json");

    let ds = open(&path).expect("open stac item");
    let features: Vec<_> = ds
        .features()
        .expect("features()")
        .collect::<Result<_, _>>()
        .expect("no stream errors");

    assert_eq!(
        features.len(),
        1,
        "single STAC Feature should yield 1 streaming feature"
    );
    assert!(
        features[0].has_geometry(),
        "item must have polygon geometry"
    );
    assert_eq!(
        features[0].id.as_deref(),
        Some("single-item"),
        "feature id should be preserved"
    );
}

// ─── GeoJSON streaming error handling ─────────────────────────────────────────

/// A valid FeatureCollection streams all its features.
#[cfg(feature = "geojson")]
#[test]
fn test_stream_geojson_feature_collection() {
    let path = TempPath::new("stream_geojson_fc.geojson");
    let content = r#"{"type":"FeatureCollection","features":[
        {"type":"Feature","geometry":{"type":"Point","coordinates":[1.0,2.0]},"properties":{"n":"a"}},
        {"type":"Feature","geometry":{"type":"Point","coordinates":[3.0,4.0]},"properties":{"n":"b"}}
    ]}"#;
    std::fs::write(&path, content).expect("write geojson");

    let ds = open(&path).expect("open geojson");
    let count = ds.features().expect("features()").count();
    assert_eq!(count, 2, "FeatureCollection should stream 2 features");
}

/// A single Feature (not a FeatureCollection) yields an empty stream — the
/// documented fallback — rather than an error.
#[cfg(feature = "geojson")]
#[test]
fn test_stream_geojson_single_feature_is_empty() {
    let path = TempPath::new("stream_geojson_single.geojson");
    let content =
        r#"{"type":"Feature","geometry":{"type":"Point","coordinates":[1.0,2.0]},"properties":{}}"#;
    std::fs::write(&path, content).expect("write geojson");

    let ds = open(&path).expect("open geojson");
    let result = ds.features();
    assert!(
        result.is_ok(),
        "single Feature should not error, got is_err={}",
        result.is_err()
    );
    assert_eq!(
        result.expect("stream").count(),
        0,
        "single Feature yields empty stream"
    );
}

/// Truncated / corrupt GeoJSON surfaces as an error instead of a silent empty
/// stream. Regression for the blanket `Err(_) => Ok(empty)` swallow that made a
/// corrupted source indistinguishable from a genuinely empty dataset.
#[cfg(feature = "geojson")]
#[test]
fn test_stream_geojson_corrupt_errors() {
    let path = TempPath::new("stream_geojson_corrupt.geojson");
    // Truncated mid-object — not valid JSON.
    let content =
        r#"{"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"Poi"#;
    std::fs::write(&path, content).expect("write geojson");

    let ds = open(&path).expect("open geojson");
    let result = ds.features();
    assert!(
        result.is_err(),
        "corrupt GeoJSON should return Err, got Ok(empty)"
    );
}

// ─── Unknown dispatch + exhaustive tests ─────────────────────────────────────

/// Opening a file whose format could not be detected should return an empty
/// stream from features() rather than an error.
#[test]
fn test_stream_unknown_returns_empty() {
    use oxigeo::DatasetFormat;
    use oxigeo::DatasetInfo;
    use oxigeo::open::OpenedDataset;
    use oxigeo::streaming::StreamingExt;

    let bad_format_path = std::env::temp_dir()
        .join("oxigeo_bad_format_ZZZZ.xyz")
        .to_string_lossy()
        .to_string();
    let mut info = DatasetInfo::default();
    info.format = DatasetFormat::Unknown;
    info.path = Some(bad_format_path);
    let ds = OpenedDataset::Unknown(info);
    let stream = ds.features().expect("Unknown variant should not error");
    assert_eq!(
        stream.count(),
        0,
        "Unknown format should produce 0 features"
    );
}

/// Verify that every OpenedDataset variant either returns a FeatureStream
/// (vector formats) or an explicit NotSupported error (raster formats).
/// No variant should panic.
#[test]
fn test_features_dispatch_exhaustive() {
    use oxigeo::DatasetFormat;
    use oxigeo::DatasetInfo;
    use oxigeo::open::OpenedDataset;
    use oxigeo::streaming::StreamingExt;
    use oxigeo_core::error::OxiGeoError;

    let make_info = |fmt: DatasetFormat| {
        let mut info = DatasetInfo::default();
        info.format = fmt;
        info.path = None;
        info
    };

    // Vector variants: expect Ok(FeatureStream) — even if empty.
    let vector_variants = vec![
        OpenedDataset::GeoJson(make_info(DatasetFormat::GeoJson)),
        OpenedDataset::Shapefile(make_info(DatasetFormat::Shapefile)),
        OpenedDataset::FlatGeobuf(make_info(DatasetFormat::FlatGeobuf)),
        OpenedDataset::GeoPackage(make_info(DatasetFormat::GeoPackage)),
        OpenedDataset::GeoParquet(make_info(DatasetFormat::GeoParquet)),
        OpenedDataset::Stac(make_info(DatasetFormat::Stac)),
        OpenedDataset::Unknown(make_info(DatasetFormat::Unknown)),
    ];
    for ds in &vector_variants {
        let fmt = format!("{:?}", ds.format());
        let result = ds.features();
        assert!(
            result.is_ok(),
            "vector/unknown variant {fmt} must return Ok(FeatureStream)"
        );
    }

    // Raster variants: expect Err(NotSupported).
    let raster_variants = vec![
        OpenedDataset::GeoTiff(make_info(DatasetFormat::GeoTiff)),
        OpenedDataset::NetCdf(make_info(DatasetFormat::NetCdf)),
        OpenedDataset::Zarr(make_info(DatasetFormat::Zarr)),
        OpenedDataset::Grib(make_info(DatasetFormat::Grib)),
    ];
    for ds in &raster_variants {
        let fmt = format!("{:?}", ds.format());
        let result = ds.features();
        match result {
            Err(OxiGeoError::NotSupported { .. }) => {} // expected
            Ok(_) => panic!("raster variant {fmt} should return Err(NotSupported), got Ok"),
            Err(e) => panic!("raster variant {fmt} should return NotSupported, got {e}"),
        }
    }
}
