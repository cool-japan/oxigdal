//! Integration tests for GeoParquet 1.1 row-group pruning + predicate pushdown.
//!
//! Each test builds a minimal Parquet file directly with the Arrow/Parquet APIs
//! so that we have full control over row-group boundaries and column layout.
#![allow(clippy::panic, clippy::expect_used)]

use arrow_array::{Array, BinaryArray, Float64Array, Int64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use oxigdal_geoparquet::GeoParquetReader;
use oxigdal_geoparquet::covering::BboxColumns;
use oxigdal_geoparquet::geometry::{Geometry, Point, WkbWriter};
use oxigdal_geoparquet::metadata::{Crs, GeoParquetMetadata, GeometryColumnMetadata};
use oxigdal_geoparquet::predicate::{AttributeFilter, ScalarValue};
use parquet::arrow::ArrowWriter;
use parquet::basic::{Compression, Repetition, Type as PhysicalType};
use parquet::file::properties::WriterProperties;
use parquet::schema::types::{SchemaDescriptor, Type};
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

// ── Helpers ────────────────────────────────────────────────────────────────────

fn point_wkb(x: f64, y: f64) -> Vec<u8> {
    let geom = Geometry::Point(Point::new_2d(x, y));
    let mut writer = WkbWriter::new(true);
    writer.write_geometry(&geom).expect("wkb encode")
}

fn cleanup(path: &Path) {
    let _ = std::fs::remove_file(path);
}

/// Embed the `geo` metadata key so `GeoParquetReader` can open the file.
fn geo_schema(base: Schema, geom_col: &str) -> Arc<Schema> {
    let col_meta = GeometryColumnMetadata::new_wkb().with_crs(Crs::wgs84());
    let mut geo_meta = GeoParquetMetadata::new(geom_col);
    geo_meta.add_column(geom_col, col_meta);
    let meta_json = geo_meta.to_json().expect("serialize geo meta");
    let mut schema_meta = base.metadata().clone();
    schema_meta.insert("geo".to_string(), meta_json);
    Arc::new(base.with_metadata(schema_meta))
}

// ── Test 1: row-group pruning — disjoint bbox ──────────────────────────────────

/// Write two row groups in different spatial zones; query bbox misses both.
/// Expect 0 rows returned.
#[test]
fn test_row_group_pruning_disjoint_bbox() {
    let temp_dir = std::env::temp_dir();
    let path = temp_dir.join("pushdown_rg_disjoint.parquet");

    // Row-group 0: points near (0,0) – zone A
    // Row-group 1: points near (100,100) – zone B
    // Query: (200,200,300,300) – misses both zones
    write_two_rg_with_covering_bbox(&path, 0.0, 0.0, 100.0, 100.0);

    let reader = GeoParquetReader::open(&path)
        .expect("open")
        .with_bbox_filter((200.0, 200.0, 300.0, 300.0));

    let results = reader.read_pushdown().expect("pushdown");
    let total: usize = results.iter().map(|b| b.num_rows()).sum();
    assert_eq!(total, 0, "no row groups or rows should match the query");

    cleanup(&path);
}

// ── Test 2: row-group pruning — partial overlap ────────────────────────────────

/// Write two row groups in different spatial zones; query overlaps only zone A.
/// Expect rows only from zone A.
#[test]
fn test_row_group_pruning_partial_overlap() {
    let temp_dir = std::env::temp_dir();
    let path = temp_dir.join("pushdown_rg_partial.parquet");

    // RG 0: zone A near (1,1)
    // RG 1: zone B near (100,100)
    // Query: (0,0,10,10) – overlaps zone A only
    write_two_rg_with_covering_bbox(&path, 1.0, 1.0, 100.0, 100.0);

    let reader = GeoParquetReader::open(&path)
        .expect("open")
        .with_bbox_filter((0.0, 0.0, 10.0, 10.0));

    let results = reader.read_pushdown().expect("pushdown");
    let total: usize = results.iter().map(|b| b.num_rows()).sum();
    assert!(total > 0, "should have rows from zone-A row group");
    // Zone B points (100,100) must not appear.
    for batch in &results {
        let xmin_col = batch
            .column_by_name("geometry_bbox_xmin")
            .expect("xmin col")
            .as_any()
            .downcast_ref::<Float64Array>()
            .expect("f64");
        for i in 0..xmin_col.len() {
            let v = xmin_col.value(i);
            assert!(
                v < 50.0,
                "zone-B point (xmin={v}) should not appear in results"
            );
        }
    }

    cleanup(&path);
}

// ── Test 3: predicate pushdown — Eq (Utf8) ────────────────────────────────────

#[test]
fn test_predicate_pushdown_eq() {
    let temp_dir = std::env::temp_dir();
    let path = temp_dir.join("pushdown_eq.parquet");
    write_attributed_parquet(&path);

    let filter = AttributeFilter::Eq {
        col: "name".to_string(),
        value: ScalarValue::Utf8("alpha".into()),
    };

    let reader = GeoParquetReader::open(&path)
        .expect("open")
        .with_attribute_filter(filter);

    let results = reader.read_pushdown().expect("pushdown");
    let total: usize = results.iter().map(|b| b.num_rows()).sum();
    assert!(total > 0, "should match 'alpha' rows");

    // All returned rows must have name == "alpha".
    for batch in &results {
        let names = batch
            .column_by_name("name")
            .expect("name col")
            .as_any()
            .downcast_ref::<StringArray>()
            .expect("string");
        for i in 0..names.len() {
            assert_eq!(names.value(i), "alpha", "name must be 'alpha'");
        }
    }

    cleanup(&path);
}

// ── Test 4: predicate pushdown — Range (Int64) ────────────────────────────────

#[test]
fn test_predicate_pushdown_range() {
    let temp_dir = std::env::temp_dir();
    let path = temp_dir.join("pushdown_range.parquet");
    write_attributed_parquet(&path);

    let filter = AttributeFilter::Range {
        col: "population".to_string(),
        lo: ScalarValue::Int64(0),
        hi: ScalarValue::Int64(500_000),
    };

    let reader = GeoParquetReader::open(&path)
        .expect("open")
        .with_attribute_filter(filter);

    let results = reader.read_pushdown().expect("pushdown");

    for batch in &results {
        let pop = batch
            .column_by_name("population")
            .expect("population col")
            .as_any()
            .downcast_ref::<Int64Array>()
            .expect("i64");
        for i in 0..pop.len() {
            let v = pop.value(i);
            assert!(
                (0..=500_000_i64).contains(&v),
                "population {v} out of range [0,500000]"
            );
        }
    }

    cleanup(&path);
}

// ── Test 5: BboxColumns::detect — struct shape ────────────────────────────────

#[test]
fn test_covering_bbox_struct_shape() {
    let struct_fields = vec![
        Arc::new(
            Type::primitive_type_builder("xmin", PhysicalType::DOUBLE)
                .with_repetition(Repetition::OPTIONAL)
                .build()
                .expect("prim"),
        ),
        Arc::new(
            Type::primitive_type_builder("ymin", PhysicalType::DOUBLE)
                .with_repetition(Repetition::OPTIONAL)
                .build()
                .expect("prim"),
        ),
        Arc::new(
            Type::primitive_type_builder("xmax", PhysicalType::DOUBLE)
                .with_repetition(Repetition::OPTIONAL)
                .build()
                .expect("prim"),
        ),
        Arc::new(
            Type::primitive_type_builder("ymax", PhysicalType::DOUBLE)
                .with_repetition(Repetition::OPTIONAL)
                .build()
                .expect("prim"),
        ),
    ];
    let bbox_struct = Type::group_type_builder("geometry_bbox")
        .with_repetition(Repetition::OPTIONAL)
        .with_fields(struct_fields)
        .build()
        .expect("struct");

    let schema_type = Type::group_type_builder("schema")
        .with_fields(vec![
            Arc::new(
                Type::primitive_type_builder("geometry", PhysicalType::BYTE_ARRAY)
                    .with_repetition(Repetition::OPTIONAL)
                    .build()
                    .expect("prim"),
            ),
            Arc::new(bbox_struct),
        ])
        .build()
        .expect("schema");

    let schema_descr = SchemaDescriptor::new(Arc::new(schema_type));
    let bbox = BboxColumns::detect(&schema_descr, "geometry");
    assert!(bbox.is_some(), "struct bbox should be detected");
    let bbox = bbox.expect("present");
    assert!(bbox.is_available());
}

// ── Test 6: BboxColumns::detect — flat columns shape ─────────────────────────

#[test]
fn test_covering_bbox_flat_columns_shape() {
    let schema_type = Type::group_type_builder("schema")
        .with_fields(vec![
            Arc::new(
                Type::primitive_type_builder("geometry", PhysicalType::BYTE_ARRAY)
                    .with_repetition(Repetition::OPTIONAL)
                    .build()
                    .expect("prim"),
            ),
            Arc::new(
                Type::primitive_type_builder("geometry_bbox_xmin", PhysicalType::DOUBLE)
                    .with_repetition(Repetition::OPTIONAL)
                    .build()
                    .expect("prim"),
            ),
            Arc::new(
                Type::primitive_type_builder("geometry_bbox_ymin", PhysicalType::DOUBLE)
                    .with_repetition(Repetition::OPTIONAL)
                    .build()
                    .expect("prim"),
            ),
            Arc::new(
                Type::primitive_type_builder("geometry_bbox_xmax", PhysicalType::DOUBLE)
                    .with_repetition(Repetition::OPTIONAL)
                    .build()
                    .expect("prim"),
            ),
            Arc::new(
                Type::primitive_type_builder("geometry_bbox_ymax", PhysicalType::DOUBLE)
                    .with_repetition(Repetition::OPTIONAL)
                    .build()
                    .expect("prim"),
            ),
        ])
        .build()
        .expect("schema");

    let schema_descr = SchemaDescriptor::new(Arc::new(schema_type));
    let bbox = BboxColumns::detect(&schema_descr, "geometry");
    assert!(bbox.is_some(), "flat bbox columns should be detected");
    let bbox = bbox.expect("present");
    assert!(bbox.is_available());
    assert_eq!(bbox.xmin_col, 1);
    assert_eq!(bbox.ymin_col, 2);
    assert_eq!(bbox.xmax_col, 3);
    assert_eq!(bbox.ymax_col, 4);
}

// ── Test 7: no covering bbox → fallback to WKB-based filtering ───────────────

#[test]
fn test_no_covering_bbox_falls_back_to_wkb_bbox() {
    let temp_dir = std::env::temp_dir();
    let path = temp_dir.join("pushdown_no_bbox_cols.parquet");

    // Write a plain GeoParquet file (no bbox columns).
    write_plain_geo_parquet(&path);

    // BboxColumns::detect should return None for a geometry-only schema.
    {
        use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
        let file = File::open(&path).expect("open");
        let builder = ParquetRecordBatchReaderBuilder::try_new(file).expect("builder");
        let schema = builder.parquet_schema().clone();
        let bbox = BboxColumns::detect(&schema, "geometry");
        assert!(bbox.is_none(), "plain schema should have no bbox columns");
    }

    // Reader still works — WKB-based fallback.
    let reader = GeoParquetReader::open(&path)
        .expect("open")
        // points at (0,0),(5,5),(100,100) — bbox (0,0,10,10) hits first two
        .with_bbox_filter((0.0, 0.0, 10.0, 10.0));

    let results = reader.read_pushdown().expect("pushdown");
    let total: usize = results.iter().map(|b| b.num_rows()).sum();
    assert_eq!(
        total, 2,
        "WKB fallback should return rows at (0,0) and (5,5)"
    );

    cleanup(&path);
}

// ── Test 8: combined bbox + attribute filter ──────────────────────────────────

#[test]
fn test_predicate_combined_with_bbox() {
    let temp_dir = std::env::temp_dir();
    let path = temp_dir.join("pushdown_combined.parquet");
    write_attributed_parquet(&path);

    // bbox (0,0,20,20) should hit rows with x in [1,5,15] (name=alpha,beta,gamma)
    // attribute: name == "alpha"
    // combined: only "alpha" row at (1,1)
    let filter = AttributeFilter::Eq {
        col: "name".to_string(),
        value: ScalarValue::Utf8("alpha".into()),
    };

    let reader = GeoParquetReader::open(&path)
        .expect("open")
        .with_bbox_filter((0.0, 0.0, 20.0, 20.0))
        .with_attribute_filter(filter);

    let results = reader.read_pushdown().expect("pushdown");
    let total: usize = results.iter().map(|b| b.num_rows()).sum();
    assert!(total > 0, "should return at least one row");

    for batch in &results {
        let names = batch
            .column_by_name("name")
            .expect("name col")
            .as_any()
            .downcast_ref::<StringArray>()
            .expect("string");
        for i in 0..names.len() {
            assert_eq!(names.value(i), "alpha");
        }
    }

    cleanup(&path);
}

// ── Fixture writers ────────────────────────────────────────────────────────────

/// Writes a file with two row groups, each having flat covering.bbox columns.
///
/// RG 0 has points near `(ax, ay)`.
/// RG 1 has points near `(bx, by)`.
fn write_two_rg_with_covering_bbox(path: &Path, ax: f64, ay: f64, bx: f64, by: f64) {
    let geom_col = "geometry";
    let schema = build_flat_bbox_schema(geom_col);

    // RG 0
    let rg0 = make_bbox_batch(schema.clone(), geom_col, ax, ay, 4);
    // RG 1
    let rg1 = make_bbox_batch(schema.clone(), geom_col, bx, by, 4);

    let file = File::create(path).expect("create");
    let props = WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .set_max_row_group_row_count(Some(4))
        .build();
    let mut writer = ArrowWriter::try_new(file, schema, Some(props)).expect("writer");
    writer.write(&rg0).expect("write rg0");
    writer.flush().expect("flush rg0");
    writer.write(&rg1).expect("write rg1");
    writer.close().expect("close");
}

/// Build Arrow schema with flat covering.bbox columns + geometry + GeoParquet metadata.
fn build_flat_bbox_schema(geom_col: &str) -> Arc<Schema> {
    let xmin_name = format!("{geom_col}_bbox_xmin");
    let ymin_name = format!("{geom_col}_bbox_ymin");
    let xmax_name = format!("{geom_col}_bbox_xmax");
    let ymax_name = format!("{geom_col}_bbox_ymax");

    let base = Schema::new(vec![
        Field::new(geom_col, DataType::Binary, true),
        Field::new(&xmin_name, DataType::Float64, false),
        Field::new(&ymin_name, DataType::Float64, false),
        Field::new(&xmax_name, DataType::Float64, false),
        Field::new(&ymax_name, DataType::Float64, false),
    ]);
    geo_schema(base, geom_col)
}

/// Make a RecordBatch with `n_rows` rows all at `(cx, cy)`.
fn make_bbox_batch(
    schema: Arc<Schema>,
    geom_col: &str,
    cx: f64,
    cy: f64,
    n_rows: usize,
) -> RecordBatch {
    let raw: Vec<Vec<u8>> = (0..n_rows).map(|_| point_wkb(cx, cy)).collect();
    let wkb_refs: Vec<Option<&[u8]>> = raw.iter().map(|v| Some(v.as_slice())).collect();
    let geom_array: Arc<dyn Array> = Arc::new(BinaryArray::from(wkb_refs));

    // Point bbox = (x,y,x,y)
    let xmins: Vec<f64> = vec![cx; n_rows];
    let ymins: Vec<f64> = vec![cy; n_rows];
    let xmaxs: Vec<f64> = vec![cx; n_rows];
    let ymaxs: Vec<f64> = vec![cy; n_rows];

    let geom_col_idx = schema.index_of(geom_col).expect("geom col");
    let xmin_name = schema.field(geom_col_idx + 1).name().clone();
    let ymin_name = schema.field(geom_col_idx + 2).name().clone();
    let xmax_name = schema.field(geom_col_idx + 3).name().clone();
    let ymax_name = schema.field(geom_col_idx + 4).name().clone();
    let _ = (xmin_name, ymin_name, xmax_name, ymax_name); // names used via schema order

    RecordBatch::try_new(
        schema,
        vec![
            geom_array,
            Arc::new(Float64Array::from(xmins)),
            Arc::new(Float64Array::from(ymins)),
            Arc::new(Float64Array::from(xmaxs)),
            Arc::new(Float64Array::from(ymaxs)),
        ],
    )
    .expect("batch")
}

/// Write a parquet with geometry + name (Utf8) + population (Int64) columns.
/// Points: alpha(1,1,500000), beta(5,5,1500000), gamma(15,15,200000), delta(100,100,3000000)
fn write_attributed_parquet(path: &Path) {
    let base = Schema::new(vec![
        Field::new("geometry", DataType::Binary, true),
        Field::new("name", DataType::Utf8, true),
        Field::new("population", DataType::Int64, true),
    ]);
    let schema = geo_schema(base, "geometry");

    let points = [(1.0f64, 1.0), (5.0, 5.0), (15.0, 15.0), (100.0, 100.0)];
    let raw: Vec<Vec<u8>> = points.iter().map(|(x, y)| point_wkb(*x, *y)).collect();
    let wkb_refs: Vec<Option<&[u8]>> = raw.iter().map(|v| Some(v.as_slice())).collect();

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(BinaryArray::from(wkb_refs)),
            Arc::new(StringArray::from(vec!["alpha", "beta", "gamma", "delta"])),
            Arc::new(Int64Array::from(vec![
                500_000i64, 1_500_000, 200_000, 3_000_000,
            ])),
        ],
    )
    .expect("batch");

    let file = File::create(path).expect("create");
    let props = WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .build();
    let mut writer = ArrowWriter::try_new(file, schema, Some(props)).expect("writer");
    writer.write(&batch).expect("write");
    writer.close().expect("close");
}

/// Write a plain GeoParquet file with only a geometry column (no bbox columns).
fn write_plain_geo_parquet(path: &Path) {
    let base = Schema::new(vec![Field::new("geometry", DataType::Binary, true)]);
    let schema = geo_schema(base, "geometry");

    let points = [(0.0f64, 0.0), (5.0, 5.0), (100.0, 100.0)];
    let raw: Vec<Vec<u8>> = points.iter().map(|(x, y)| point_wkb(*x, *y)).collect();
    let wkb_refs: Vec<Option<&[u8]>> = raw.iter().map(|v| Some(v.as_slice())).collect();

    let batch = RecordBatch::try_new(schema.clone(), vec![Arc::new(BinaryArray::from(wkb_refs))])
        .expect("batch");

    let file = File::create(path).expect("create");
    let props = WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .build();
    let mut writer = ArrowWriter::try_new(file, schema, Some(props)).expect("writer");
    writer.write(&batch).expect("write");
    writer.close().expect("close");
}

/// Write a plain GeoParquet file (geometry column only, no covering.bbox) whose
/// rows are the supplied geometries encoded as little-endian WKB.
fn write_plain_geo_parquet_with(path: &Path, geoms: &[Geometry]) {
    let base = Schema::new(vec![Field::new("geometry", DataType::Binary, true)]);
    let schema = geo_schema(base, "geometry");

    let raw: Vec<Vec<u8>> = geoms
        .iter()
        .map(|g| WkbWriter::new(true).write_geometry(g).expect("wkb encode"))
        .collect();
    let wkb_refs: Vec<Option<&[u8]>> = raw.iter().map(|v| Some(v.as_slice())).collect();

    let batch = RecordBatch::try_new(schema.clone(), vec![Arc::new(BinaryArray::from(wkb_refs))])
        .expect("batch");

    let file = File::create(path).expect("create");
    let props = WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .build();
    let mut writer = ArrowWriter::try_new(file, schema, Some(props)).expect("writer");
    writer.write(&batch).expect("write");
    writer.close().expect("close");
}

// ── Test 9: WKB fallback must not silently drop multi-geometry rows ─────────────

/// Regression for the `wkb_bbox()` silent-failure defect: before the fix,
/// `wkb_bbox()` returned `None` for `MultiPolygon`/`MultiLineString`/
/// `GeometryCollection`/big-endian WKB, and the pushdown WKB fallback mask
/// (initialised `false`, only set `true` inside the `Some(..)` branch) treated
/// `None` as "row does not match". A bbox query against a GeoParquet 1.0 file
/// (no covering.bbox columns) therefore silently omitted every matching
/// MultiPolygon / MultiLineString feature. This test proves those rows are now
/// returned.
#[test]
fn test_wkb_fallback_matches_multi_geometries() {
    use oxigdal_geoparquet::geometry::{
        Coordinate, LineString, MultiLineString, MultiPolygon, Polygon,
    };

    let temp_dir = std::env::temp_dir();
    let path = temp_dir.join("pushdown_multi_geom_no_bbox.parquet");

    // Row 0: MultiPolygon whose parts lie inside the query box (0,0,10,10).
    let mpoly_in = Geometry::MultiPolygon(MultiPolygon::new(vec![
        Polygon::new_simple(LineString::new(vec![
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(2.0, 1.0),
            Coordinate::new_2d(2.0, 2.0),
            Coordinate::new_2d(1.0, 1.0),
        ])),
        Polygon::new_simple(LineString::new(vec![
            Coordinate::new_2d(3.0, 3.0),
            Coordinate::new_2d(4.0, 3.0),
            Coordinate::new_2d(4.0, 4.0),
            Coordinate::new_2d(3.0, 3.0),
        ])),
    ]));
    // Row 1: MultiLineString crossing into the query box.
    let mls_in = Geometry::MultiLineString(MultiLineString::new(vec![LineString::new(vec![
        Coordinate::new_2d(-5.0, 5.0),
        Coordinate::new_2d(5.0, 5.0),
    ])]));
    // Row 2: MultiPolygon far outside the query box.
    let mpoly_out = Geometry::MultiPolygon(MultiPolygon::new(vec![Polygon::new_simple(
        LineString::new(vec![
            Coordinate::new_2d(100.0, 100.0),
            Coordinate::new_2d(101.0, 100.0),
            Coordinate::new_2d(101.0, 101.0),
            Coordinate::new_2d(100.0, 100.0),
        ]),
    )]));

    write_plain_geo_parquet_with(&path, &[mpoly_in, mls_in, mpoly_out]);

    // Sanity: this file genuinely has no covering.bbox columns, so the read
    // takes the WKB-decode fallback path.
    {
        use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
        let file = File::open(&path).expect("open");
        let builder = ParquetRecordBatchReaderBuilder::try_new(file).expect("builder");
        let schema = builder.parquet_schema().clone();
        assert!(
            BboxColumns::detect(&schema, "geometry").is_none(),
            "fixture must have no covering.bbox columns"
        );
    }

    let reader = GeoParquetReader::open(&path)
        .expect("open")
        .with_bbox_filter((0.0, 0.0, 10.0, 10.0));
    let results = reader.read_pushdown().expect("pushdown");
    let total: usize = results.iter().map(|b| b.num_rows()).sum();
    assert_eq!(
        total, 2,
        "WKB fallback must return the intersecting MultiPolygon and MultiLineString rows"
    );

    cleanup(&path);
}
