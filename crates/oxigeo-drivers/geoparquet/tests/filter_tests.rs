//! Integration tests for GeoParquet row-level spatial + attribute filtering.
//!
//! Test fixtures are built directly with Arrow/Parquet primitives so that we
//! can include attribute columns (the current high-level `GeoParquetWriter` API
//! only supports the geometry column).
#![allow(clippy::panic, clippy::expect_used)]

use arrow_array::{Array, BinaryArray, Float64Array, Int64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use oxigeo_core::types::BoundingBox;
use oxigeo_geoparquet::GeoParquetReader;
use oxigeo_geoparquet::filter::{AttributePredicates, ColumnCondition, CompareOp};
use oxigeo_geoparquet::geometry::{Geometry, Point, WkbWriter};
use oxigeo_geoparquet::metadata::{Crs, GeoParquetMetadata, GeometryColumnMetadata};
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

// ── Fixture builder ────────────────────────────────────────────────────────────

/// Encodes a 2-D WKB point (little-endian, type code 1).
fn point_wkb(x: f64, y: f64) -> Vec<u8> {
    let geom = Geometry::Point(Point::new_2d(x, y));
    let mut writer = WkbWriter::new(true);
    writer.write_geometry(&geom).expect("wkb encode")
}

/// Writes a GeoParquet file with 5 known-coordinate points plus "name" (Utf8)
/// and "score" (Float64) attribute columns.
///
/// Points:
///  row 0: (0.0, 0.0), name="alpha",  score=1.0
///  row 1: (5.0, 5.0), name="beta",   score=2.0
///  row 2: (15.0, 5.0), name="gamma",  score=3.0
///  row 3: (25.0, 5.0), name="delta",  score=4.0
///  row 4: (35.0, 5.0), name="epsilon",score=5.0
fn write_attributed_fixture(path: &Path) {
    let geom_field = Field::new("geometry", DataType::Binary, true);
    let name_field = Field::new("name", DataType::Utf8, true);
    let score_field = Field::new("score", DataType::Float64, true);
    let id_field = Field::new("id", DataType::Int64, true);

    let base_schema = Schema::new(vec![geom_field, name_field, score_field, id_field]);

    // Embed GeoParquet metadata so GeoParquetReader can open it.
    let col_meta = GeometryColumnMetadata::new_wkb().with_crs(Crs::wgs84());
    let mut geo_meta = GeoParquetMetadata::new("geometry");
    geo_meta.add_column("geometry", col_meta);
    let meta_json = geo_meta.to_json().expect("serialize geo meta");
    let mut schema_meta = base_schema.metadata().clone();
    schema_meta.insert("geo".to_string(), meta_json);
    let schema = Arc::new(base_schema.with_metadata(schema_meta));

    let wkbs: Vec<&[u8]> = Vec::new(); // placeholder — build owned first
    let raw: Vec<Vec<u8>> = vec![
        point_wkb(0.0, 0.0),
        point_wkb(5.0, 5.0),
        point_wkb(15.0, 5.0),
        point_wkb(25.0, 5.0),
        point_wkb(35.0, 5.0),
    ];
    let _ = wkbs; // silence unused
    let wkb_refs: Vec<Option<&[u8]>> = raw.iter().map(|v| Some(v.as_slice())).collect();
    let geom_array: Arc<dyn Array> = Arc::new(BinaryArray::from(wkb_refs));
    let name_array: Arc<dyn Array> = Arc::new(StringArray::from(vec![
        "alpha", "beta", "gamma", "delta", "epsilon",
    ]));
    let score_array: Arc<dyn Array> =
        Arc::new(Float64Array::from(vec![1.0f64, 2.0, 3.0, 4.0, 5.0]));
    let id_array: Arc<dyn Array> = Arc::new(Int64Array::from(vec![100i64, 200, 300, 400, 500]));

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![geom_array, name_array, score_array, id_array],
    )
    .expect("record batch");

    let file = File::create(path).expect("create file");
    let props = WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .build();
    let mut writer = ArrowWriter::try_new(file, schema, Some(props)).expect("arrow writer");
    writer.write(&batch).expect("write batch");
    writer.close().expect("close writer");
}

fn cleanup(path: &Path) {
    let _ = std::fs::remove_file(path);
}

// ── 1. test_spatial_filter_exact_row_level ────────────────────────────────────

#[test]
fn test_spatial_filter_exact_row_level() {
    let temp_dir = std::env::temp_dir();
    let path = temp_dir.join("geoparquet_spatial_row_filter.parquet");
    write_attributed_fixture(&path);

    let mut reader = GeoParquetReader::open(&path).expect("open");

    // bbox that contains only (0,0) and (5,5): [−1,−1,10,10]
    let bbox = BoundingBox::new(-1.0, -1.0, 10.0, 10.0).expect("bbox");
    let results = reader.read_filtered_exact(bbox).expect("filter");

    let total_rows: usize = results.iter().map(|b| b.num_rows()).sum();
    assert_eq!(
        total_rows, 2,
        "only (0,0) and (5,5) are inside [-1,-1,10,10]"
    );

    cleanup(&path);
}

// ── 2. test_attribute_filter_eq_string ───────────────────────────────────────

#[test]
fn test_attribute_filter_eq_string() {
    let temp_dir = std::env::temp_dir();
    let path = temp_dir.join("geoparquet_attr_string.parquet");
    write_attributed_fixture(&path);

    let mut reader = GeoParquetReader::open(&path).expect("open");

    let preds = AttributePredicates::all_of(vec![ColumnCondition::new(
        "name",
        CompareOp::Eq,
        serde_json::Value::String("gamma".into()),
    )]);

    let results = reader.read_with_filter(None, Some(preds)).expect("filter");

    let total_rows: usize = results.iter().map(|b| b.num_rows()).sum();
    assert_eq!(total_rows, 1, "only 'gamma' should match");

    // Verify the name value in the returned batch.
    let batch = &results[0];
    let name_col = batch
        .column_by_name("name")
        .expect("name col")
        .as_any()
        .downcast_ref::<StringArray>()
        .expect("string array");
    assert_eq!(name_col.value(0), "gamma");

    cleanup(&path);
}

// ── 3. test_attribute_filter_numeric_gt ──────────────────────────────────────

#[test]
fn test_attribute_filter_numeric_gt() {
    let temp_dir = std::env::temp_dir();
    let path = temp_dir.join("geoparquet_attr_numeric.parquet");
    write_attributed_fixture(&path);

    let mut reader = GeoParquetReader::open(&path).expect("open");

    // scores are 1..5; >3.0 means 4.0 and 5.0 (rows delta, epsilon)
    let preds = AttributePredicates::all_of(vec![ColumnCondition::new(
        "score",
        CompareOp::Gt,
        serde_json::json!(3.0f64),
    )]);

    let results = reader.read_with_filter(None, Some(preds)).expect("filter");

    let total_rows: usize = results.iter().map(|b| b.num_rows()).sum();
    assert_eq!(total_rows, 2, "score > 3.0 → delta(4.0) + epsilon(5.0)");

    cleanup(&path);
}

// ── 4. test_combined_spatial_attribute_filter ─────────────────────────────────

#[test]
fn test_combined_spatial_attribute_filter() {
    let temp_dir = std::env::temp_dir();
    let path = temp_dir.join("geoparquet_combined_filter.parquet");
    write_attributed_fixture(&path);

    let mut reader = GeoParquetReader::open(&path).expect("open");

    // bbox [−1,−1,30,10] contains rows 0(0,0), 1(5,5), 2(15,5), 3(25,5)
    // attribute filter: score > 2.0 → rows 2(3.0), 3(4.0), 4(5.0)
    // intersection → rows 2(15,5) and 3(25,5) satisfy both
    let bbox = BoundingBox::new(-1.0, -1.0, 30.0, 10.0).expect("bbox");
    let preds = AttributePredicates::all_of(vec![ColumnCondition::new(
        "score",
        CompareOp::Gt,
        serde_json::json!(2.0f64),
    )]);

    let results = reader
        .read_with_filter(Some(bbox), Some(preds))
        .expect("filter");

    let total_rows: usize = results.iter().map(|b| b.num_rows()).sum();
    assert_eq!(
        total_rows, 2,
        "rows at (15,5) and (25,5) satisfy both filters"
    );

    cleanup(&path);
}

// ── 5. test_empty_result_no_matches ──────────────────────────────────────────

#[test]
fn test_empty_result_no_matches() {
    let temp_dir = std::env::temp_dir();
    let path = temp_dir.join("geoparquet_no_match.parquet");
    write_attributed_fixture(&path);

    let mut reader = GeoParquetReader::open(&path).expect("open");

    // A bbox that doesn't touch any point (e.g. far away).
    let bbox = BoundingBox::new(1000.0, 1000.0, 2000.0, 2000.0).expect("bbox");
    let results = reader.read_filtered_exact(bbox).expect("filter");

    let total_rows: usize = results.iter().map(|b| b.num_rows()).sum();
    assert_eq!(total_rows, 0, "no geometry inside [1000,1000,2000,2000]");

    cleanup(&path);
}
