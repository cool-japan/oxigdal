//! Integration tests for the metadata-only pushdown planner ([`plan_pushdown`])
//! and the `Cmp` / multi-filter predicate paths, using a fixture that mirrors
//! the real VIDA GeoParquet layout:
//!
//! * SNAPPY compression,
//! * a covering bbox stored in a struct column literally named `bbox`
//!   (children `xmin`, `xmax`, `ymin`, `ymax` — VIDA field order),
//! * a `covering.bbox` object in the `geo` metadata pointing at that struct,
//! * top-level attribute columns `area_in_meters` (Float64) and `confidence`
//!   (Float64) plus a `name` (Utf8) column,
//! * multiple row groups in disjoint spatial zones.
#![allow(clippy::panic, clippy::expect_used, clippy::unwrap_used)]

use arrow_array::{
    Array, ArrayRef, BinaryArray, Float64Array, RecordBatch, StringArray, StructArray,
};
use arrow_schema::{DataType, Field, Fields, Schema};
use oxigdal_geoparquet::GeoParquetReader;
use oxigdal_geoparquet::covering::BboxColumns;
use oxigdal_geoparquet::geometry::{Geometry, Point, WkbWriter};
use oxigdal_geoparquet::metadata::{Covering, Crs, GeoParquetMetadata, GeometryColumnMetadata};
use oxigdal_geoparquet::plan::plan_pushdown;
use oxigdal_geoparquet::predicate::{AttributeFilter, CmpOp, ScalarValue};
use parquet::arrow::ArrowWriter;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::basic::Compression;
use parquet::file::metadata::ParquetMetaData;
use parquet::file::properties::WriterProperties;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

// ── Fixture construction ────────────────────────────────────────────────────────

const GEOM_COL: &str = "geometry";

/// Ordered struct children matching the VIDA layout: xmin, xmax, ymin, ymax.
fn bbox_fields() -> Fields {
    Fields::from(vec![
        Field::new("xmin", DataType::Float64, false),
        Field::new("xmax", DataType::Float64, false),
        Field::new("ymin", DataType::Float64, false),
        Field::new("ymax", DataType::Float64, false),
    ])
}

/// Builds the GeoParquet `geo` metadata mirroring VIDA.  When `with_covering`
/// is true the `covering.bbox` object points at the `bbox` struct; otherwise it
/// is omitted (exercising the plain-`bbox` struct heuristic fallback).
fn vida_geo_meta(with_covering: bool) -> GeoParquetMetadata {
    let mut col_meta = GeometryColumnMetadata::new_wkb().with_crs(Crs::wgs84());
    if with_covering {
        col_meta = col_meta.with_covering(Covering::bbox_struct("bbox"));
    }
    let mut geo = GeoParquetMetadata::new(GEOM_COL);
    geo.add_column(GEOM_COL, col_meta);
    geo
}

/// Arrow schema with geometry + bbox struct + area/confidence/name + geo JSON.
fn vida_schema(with_covering: bool) -> Arc<Schema> {
    let base = Schema::new(vec![
        Field::new(GEOM_COL, DataType::Binary, true),
        Field::new("bbox", DataType::Struct(bbox_fields()), false),
        Field::new("area_in_meters", DataType::Float64, false),
        Field::new("confidence", DataType::Float64, false),
        Field::new("name", DataType::Utf8, true),
    ]);
    let meta_json = vida_geo_meta(with_covering)
        .to_json()
        .expect("serialize geo meta");
    let mut schema_meta = base.metadata().clone();
    schema_meta.insert("geo".to_string(), meta_json);
    Arc::new(base.with_metadata(schema_meta))
}

fn point_wkb(x: f64, y: f64) -> Vec<u8> {
    let geom = Geometry::Point(Point::new_2d(x, y));
    let mut writer = WkbWriter::new(true);
    writer.write_geometry(&geom).expect("wkb encode")
}

/// One spatial "zone" → one row group of `n` points near `(cx, cy)`, with
/// `area_in_meters` starting at `area_base`.
fn make_zone_batch(
    schema: Arc<Schema>,
    cx: f64,
    cy: f64,
    n: usize,
    area_base: f64,
    name: &str,
) -> RecordBatch {
    let raw: Vec<Vec<u8>> = (0..n).map(|_| point_wkb(cx, cy)).collect();
    let wkb_refs: Vec<Option<&[u8]>> = raw.iter().map(|v| Some(v.as_slice())).collect();
    let geom_array: ArrayRef = Arc::new(BinaryArray::from(wkb_refs));

    // Point bbox = (x, y, x, y); struct order xmin, xmax, ymin, ymax.
    let xmin: ArrayRef = Arc::new(Float64Array::from(vec![cx; n]));
    let xmax: ArrayRef = Arc::new(Float64Array::from(vec![cx; n]));
    let ymin: ArrayRef = Arc::new(Float64Array::from(vec![cy; n]));
    let ymax: ArrayRef = Arc::new(Float64Array::from(vec![cy; n]));
    let bbox = StructArray::new(bbox_fields(), vec![xmin, xmax, ymin, ymax], None);

    let areas: Vec<f64> = (0..n).map(|i| area_base + i as f64).collect();
    let conf: Vec<f64> = (0..n).map(|_| 0.9).collect();
    let names: Vec<&str> = (0..n).map(|_| name).collect();

    RecordBatch::try_new(
        schema,
        vec![
            geom_array,
            Arc::new(bbox),
            Arc::new(Float64Array::from(areas)),
            Arc::new(Float64Array::from(conf)),
            Arc::new(StringArray::from(names)),
        ],
    )
    .expect("batch")
}

/// Writes a 3-row-group VIDA-mirror file:
/// * RG0 near (0,0),     area 1000..1003, name "zone_a"
/// * RG1 near (100,100), area 5000..5003, name "zone_b"
/// * RG2 near (200,200), area 9000..9003, name "zone_c"
fn write_vida_fixture(path: &Path, with_covering: bool) {
    let schema = vida_schema(with_covering);
    let rg0 = make_zone_batch(schema.clone(), 0.0, 0.0, 4, 1000.0, "zone_a");
    let rg1 = make_zone_batch(schema.clone(), 100.0, 100.0, 4, 5000.0, "zone_b");
    let rg2 = make_zone_batch(schema.clone(), 200.0, 200.0, 4, 9000.0, "zone_c");

    let file = File::create(path).expect("create");
    let props = WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .build();
    let mut writer = ArrowWriter::try_new(file, schema, Some(props)).expect("writer");
    for batch in [rg0, rg1, rg2] {
        writer.write(&batch).expect("write");
        writer.flush().expect("flush"); // one row group per batch
    }
    writer.close().expect("close");
}

fn temp_path(name: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "oxigdal_gpq_plan_{}_{}_{}.parquet",
        name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    p
}

fn cleanup(path: &Path) {
    let _ = std::fs::remove_file(path);
}

fn read_meta(path: &Path) -> Arc<ParquetMetaData> {
    let file = File::open(path).expect("open");
    let builder = ParquetRecordBatchReaderBuilder::try_new(file).expect("builder");
    builder.metadata().clone()
}

// ── Tests: covering detection ────────────────────────────────────────────────────

/// The covering path in the `geo` metadata resolves the struct-nested bbox.
#[test]
fn test_covering_path_detection() {
    let path = temp_path("cov_detect");
    write_vida_fixture(&path, true);
    let meta = read_meta(&path);
    let schema_descr = meta.file_metadata().schema_descr();
    let geo = vida_geo_meta(true);

    let bc = BboxColumns::detect_with_covering(schema_descr, GEOM_COL, &geo)
        .expect("covering detected via geo metadata");
    // geometry=0, bbox.xmin=1, bbox.xmax=2, bbox.ymin=3, bbox.ymax=4
    assert_eq!(bc.xmin_col, 1);
    assert_eq!(bc.xmax_col, 2);
    assert_eq!(bc.ymin_col, 3);
    assert_eq!(bc.ymax_col, 4);
    cleanup(&path);
}

/// With no `covering` object in the metadata, the plain-`bbox` struct-root
/// heuristic still detects the columns.
#[test]
fn test_plain_bbox_struct_fallback() {
    let path = temp_path("plain_bbox");
    write_vida_fixture(&path, false);
    let meta = read_meta(&path);
    let schema_descr = meta.file_metadata().schema_descr();
    let geo = vida_geo_meta(false); // no covering object

    // detect_with_covering must fall back to the struct heuristic.
    let bc = BboxColumns::detect_with_covering(schema_descr, GEOM_COL, &geo)
        .expect("plain 'bbox' struct heuristic should detect covering");
    assert_eq!(bc.xmin_col, 1);
    assert_eq!(bc.ymax_col, 4);

    // Plain BboxColumns::detect (no geo) also works via the struct heuristic.
    assert!(BboxColumns::detect(schema_descr, GEOM_COL).is_some());
    cleanup(&path);
}

// ── Tests: spatial pruning + ranges ──────────────────────────────────────────────

/// A query bbox over zone B must prune RG0 and RG2, keeping only RG1.
#[test]
fn test_plan_spatial_pruning_survivors() {
    let path = temp_path("prune");
    write_vida_fixture(&path, true);
    let meta = read_meta(&path);
    let geo = vida_geo_meta(true);

    let plan = plan_pushdown(
        &meta,
        &geo,
        GEOM_COL,
        Some((90.0, 90.0, 110.0, 110.0)),
        &[],
        &[GEOM_COL.to_string()],
    )
    .expect("plan");

    assert_eq!(plan.total_row_groups, 3);
    assert_eq!(plan.row_groups, vec![1], "only zone-B row group survives");
    assert!(plan.bbox_cols.is_some());
    cleanup(&path);
}

/// The plan's byte ranges must equal `ColumnChunkMetaData::byte_range()` (which
/// includes the dictionary page) for every surviving (row group, leaf).
#[test]
fn test_plan_ranges_match_byte_range_including_dictionary() {
    let path = temp_path("ranges");
    write_vida_fixture(&path, true);
    let meta = read_meta(&path);
    let geo = vida_geo_meta(true);

    // Query hits zone B only; ask for geometry + area columns.
    let plan = plan_pushdown(
        &meta,
        &geo,
        GEOM_COL,
        Some((90.0, 90.0, 110.0, 110.0)),
        &[],
        &[GEOM_COL.to_string(), "area_in_meters".to_string()],
    )
    .expect("plan");

    assert!(!plan.ranges.is_empty(), "should fetch some column chunks");
    let mut summed = 0u64;
    for r in &plan.ranges {
        let (start, length) = meta
            .row_group(r.row_group)
            .column(r.leaf_column)
            .byte_range();
        assert_eq!(r.start, start, "range start must match byte_range start");
        assert_eq!(
            r.length, length,
            "range length must match byte_range length"
        );
        summed += length;
    }
    assert_eq!(
        plan.estimated_bytes, summed,
        "estimated_bytes = Σ range length"
    );

    // Leaf-set = union(bbox leaves ∪ output leaves).  Only RG1 survives.
    let leaves: std::collections::BTreeSet<usize> =
        plan.ranges.iter().map(|r| r.leaf_column).collect();
    // geometry=0, bbox xmin/xmax/ymin/ymax=1..4, area_in_meters=5
    for expected in [0usize, 1, 2, 3, 4, 5] {
        assert!(
            leaves.contains(&expected),
            "leaf {expected} expected in fetch set"
        );
    }
    // confidence(6) and name(7) were not requested → excluded.
    assert!(!leaves.contains(&6));
    assert!(!leaves.contains(&7));
    cleanup(&path);
}

// ── Tests: attribute-stats pruning ───────────────────────────────────────────────

/// `area_in_meters > 6000` prunes RG0 (max 1003) and RG1 (max 5003) by column
/// statistics, leaving only RG2 (max 9003).
#[test]
fn test_plan_attribute_stats_pruning() {
    let path = temp_path("attr_prune");
    write_vida_fixture(&path, true);
    let meta = read_meta(&path);
    let geo = vida_geo_meta(true);

    let filter = AttributeFilter::Cmp {
        col: "area_in_meters".to_string(),
        op: CmpOp::Gt,
        value: ScalarValue::Float64(6000.0),
    };
    let plan = plan_pushdown(
        &meta,
        &geo,
        GEOM_COL,
        None,
        std::slice::from_ref(&filter),
        &[],
    )
    .expect("plan");
    assert_eq!(
        plan.row_groups,
        vec![2],
        "only zone-C row group can hold area > 6000"
    );
    cleanup(&path);
}

/// Combined spatial + attribute pruning: a bbox over zone C and `area > 8000`
/// both select RG2.
#[test]
fn test_plan_combined_spatial_and_attribute_pruning() {
    let path = temp_path("combined_prune");
    write_vida_fixture(&path, true);
    let meta = read_meta(&path);
    let geo = vida_geo_meta(true);

    let filter = AttributeFilter::Cmp {
        col: "area_in_meters".to_string(),
        op: CmpOp::Gt,
        value: ScalarValue::Float64(8000.0),
    };
    let plan = plan_pushdown(
        &meta,
        &geo,
        GEOM_COL,
        Some((190.0, 190.0, 210.0, 210.0)),
        std::slice::from_ref(&filter),
        &[],
    )
    .expect("plan");
    assert_eq!(plan.row_groups, vec![2]);
    cleanup(&path);
}

// ── Tests: end-to-end read via struct covering (exercises struct predicate) ──────

/// Reading through `read_pushdown` with a struct-nested covering bbox must
/// return only rows inside the query box — validating the struct-aware
/// `CoveringBboxPredicate` path.
#[test]
fn test_read_pushdown_struct_covering_bbox() {
    let path = temp_path("read_struct_cov");
    write_vida_fixture(&path, true);

    let reader = GeoParquetReader::open(&path)
        .expect("open")
        .with_bbox_filter((90.0, 90.0, 110.0, 110.0));
    let results = reader.read_pushdown().expect("pushdown");
    let total: usize = results.iter().map(|b| b.num_rows()).sum();
    assert_eq!(total, 4, "only the 4 zone-B points intersect the query box");

    // Every surviving row's bbox.xmin must be near 100 (zone B), not 0 or 200.
    for batch in &results {
        let bbox = batch
            .column_by_name("bbox")
            .expect("bbox col")
            .as_any()
            .downcast_ref::<StructArray>()
            .expect("struct");
        let xmin = bbox
            .column_by_name("xmin")
            .expect("xmin")
            .as_any()
            .downcast_ref::<Float64Array>()
            .expect("f64");
        for i in 0..xmin.len() {
            assert!((xmin.value(i) - 100.0).abs() < 1e-9);
        }
    }
    cleanup(&path);
}

// ── Tests: Cmp predicate + multi-filter conjunction ──────────────────────────────

/// A `Cmp` attribute filter (`area_in_meters > 5001`) is enforced at row level.
#[test]
fn test_read_pushdown_cmp_predicate() {
    let path = temp_path("cmp_pred");
    write_vida_fixture(&path, true);

    let filter = AttributeFilter::Cmp {
        col: "area_in_meters".to_string(),
        op: CmpOp::Gt,
        value: ScalarValue::Float64(5001.0),
    };
    let reader = GeoParquetReader::open(&path)
        .expect("open")
        .with_attribute_filter(filter);
    let results = reader.read_pushdown().expect("pushdown");

    let mut count = 0usize;
    for batch in &results {
        let area = batch
            .column_by_name("area_in_meters")
            .expect("area")
            .as_any()
            .downcast_ref::<Float64Array>()
            .expect("f64");
        for i in 0..area.len() {
            assert!(area.value(i) > 5001.0, "row must satisfy area > 5001");
            count += 1;
        }
    }
    // zone B: 5002,5003 (2 rows) + zone C: 9000..9003 (4 rows) = 6 rows.
    assert_eq!(count, 6);
    cleanup(&path);
}

/// Multiple attribute filters combine conjunctively: `area > 5001` AND
/// `confidence <= 0.9` → same 6 rows (all have confidence 0.9).
#[test]
fn test_read_pushdown_multi_filter_conjunction() {
    let path = temp_path("multi_filter");
    write_vida_fixture(&path, true);

    let filters = vec![
        AttributeFilter::Cmp {
            col: "area_in_meters".to_string(),
            op: CmpOp::Gt,
            value: ScalarValue::Float64(5001.0),
        },
        AttributeFilter::Cmp {
            col: "confidence".to_string(),
            op: CmpOp::Le,
            value: ScalarValue::Float64(0.9),
        },
    ];
    let reader = GeoParquetReader::open(&path)
        .expect("open")
        .with_attribute_filters(filters);
    let results = reader.read_pushdown().expect("pushdown");
    let total: usize = results.iter().map(|b| b.num_rows()).sum();
    assert_eq!(total, 6, "conjunction of area>5001 and confidence<=0.9");

    // Tighten: confidence < 0.5 excludes everything.
    let reader2 = GeoParquetReader::open(&path)
        .expect("open")
        .with_attribute_filters(vec![AttributeFilter::Cmp {
            col: "confidence".to_string(),
            op: CmpOp::Lt,
            value: ScalarValue::Float64(0.5),
        }]);
    let results2 = reader2.read_pushdown().expect("pushdown");
    let total2: usize = results2.iter().map(|b| b.num_rows()).sum();
    assert_eq!(total2, 0, "no rows have confidence < 0.5");
    cleanup(&path);
}

/// Combined bbox (struct covering) + `Cmp` attribute filter.
#[test]
fn test_read_pushdown_bbox_plus_cmp() {
    let path = temp_path("bbox_plus_cmp");
    write_vida_fixture(&path, true);

    // Zone C box + area > 9001 → zone-C points 9002, 9003 (2 rows).
    let filter = AttributeFilter::Cmp {
        col: "area_in_meters".to_string(),
        op: CmpOp::Gt,
        value: ScalarValue::Float64(9001.0),
    };
    let reader = GeoParquetReader::open(&path)
        .expect("open")
        .with_bbox_filter((190.0, 190.0, 210.0, 210.0))
        .with_attribute_filter(filter);
    let results = reader.read_pushdown().expect("pushdown");
    let total: usize = results.iter().map(|b| b.num_rows()).sum();
    assert_eq!(total, 2);
    cleanup(&path);
}

// ── Tests: integer literal ↔ Float64 column coercion (`area_in_meters > 1000`) ────

/// Regression for the E2E defect: an `Int64` literal compared against the
/// `Float64` `area_in_meters` column previously failed at execution with
/// "Type mismatch: expected Int64, found Float64".  Every operator must now
/// coerce the literal and return the correct rows through `read_pushdown`.
#[test]
fn test_read_pushdown_int_literal_vs_float_column_all_ops() {
    let path = temp_path("int_lit_float_ops");
    write_vida_fixture(&path, true);

    // Fixture areas: RG0 {1000..1003}, RG1 {5000..5003}, RG2 {9000..9003}.
    let count = |op: CmpOp, v: i64| -> usize {
        let reader = GeoParquetReader::open(&path)
            .expect("open")
            .with_attribute_filter(AttributeFilter::Cmp {
                col: "area_in_meters".to_string(),
                op,
                value: ScalarValue::Int64(v),
            });
        reader
            .read_pushdown()
            .expect("pushdown")
            .iter()
            .map(|b| b.num_rows())
            .sum()
    };

    assert_eq!(count(CmpOp::Gt, 1000), 11, "area > 1000 excludes only 1000");
    assert_eq!(count(CmpOp::Ge, 1000), 12, "area >= 1000 keeps all rows");
    assert_eq!(count(CmpOp::Lt, 5000), 4, "area < 5000 keeps only RG0");
    assert_eq!(
        count(CmpOp::Le, 5000),
        5,
        "area <= 5000 keeps RG0 + one RG1 row"
    );
    assert_eq!(
        count(CmpOp::NotEq, 9000),
        11,
        "area <> 9000 excludes only 9000"
    );

    cleanup(&path);
}

/// Plan-level attribute stats pruning must coerce an `Int64` literal against the
/// `Float64` column identically to a `Float64` literal — no mis-prune.
#[test]
fn test_plan_attribute_stats_pruning_int_literal() {
    let path = temp_path("attr_prune_int_lit");
    write_vida_fixture(&path, true);
    let meta = read_meta(&path);
    let geo = vida_geo_meta(true);

    let filter = AttributeFilter::Cmp {
        col: "area_in_meters".to_string(),
        op: CmpOp::Gt,
        value: ScalarValue::Int64(6000), // integer literal, Float64 column
    };
    let plan = plan_pushdown(
        &meta,
        &geo,
        GEOM_COL,
        None,
        std::slice::from_ref(&filter),
        &[],
    )
    .expect("plan");
    assert_eq!(
        plan.row_groups,
        vec![2],
        "only zone-C row group can hold area > 6000 (int literal must coerce)"
    );

    // A tighter integer threshold above every max prunes all row groups.
    let filter_all = AttributeFilter::Cmp {
        col: "area_in_meters".to_string(),
        op: CmpOp::Gt,
        value: ScalarValue::Int64(9003),
    };
    let plan_all = plan_pushdown(
        &meta,
        &geo,
        GEOM_COL,
        None,
        std::slice::from_ref(&filter_all),
        &[],
    )
    .expect("plan");
    assert!(
        plan_all.row_groups.is_empty(),
        "area > 9003 prunes every row group"
    );
    cleanup(&path);
}

/// A mixed-type IN list (`Int64` + `Float64`) against the `Float64` column
/// resolves through `read_pushdown` to exactly the matching rows.
#[test]
fn test_read_pushdown_int_in_list_mixed() {
    let path = temp_path("in_list_mixed");
    write_vida_fixture(&path, true);

    let filter = AttributeFilter::In {
        col: "area_in_meters".to_string(),
        values: vec![
            ScalarValue::Int64(1001),
            ScalarValue::Float64(5002.0),
            ScalarValue::Int64(9003),
        ],
    };
    let reader = GeoParquetReader::open(&path)
        .expect("open")
        .with_attribute_filter(filter);
    let results = reader.read_pushdown().expect("pushdown");
    let total: usize = results.iter().map(|b| b.num_rows()).sum();
    assert_eq!(total, 3, "IN (1001, 5002.0, 9003) matches one row per zone");
    cleanup(&path);
}
