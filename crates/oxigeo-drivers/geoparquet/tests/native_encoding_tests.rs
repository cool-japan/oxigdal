//! Integration tests for GeoParquet 1.1 GeoArrow native encoding round-trips.
//!
//! Each test writes a small file via [`GeoParquetWriter`] using the native
//! encoding builder, reads it back via [`GeoParquetReader::read_geometries`],
//! and asserts a clean round-trip.  The back-compat regression test
//! `test_wkb_writer_default_unchanged` confirms the default writer still
//! emits a 1.0-shape WKB column (modulo the version-string bump to 1.1.0).
#![allow(clippy::expect_used)]
#![allow(clippy::panic)]

use arrow_array::builder::{FixedSizeListBuilder, Float64Builder};
use arrow_array::{Array, ArrayRef, BinaryArray, Float64Array, Int32Array, ListArray, RecordBatch};
use arrow_buffer::NullBuffer;
use arrow_schema::{DataType, Field, Schema, SchemaRef};
use oxigeo_geoparquet::geometry::{
    Coordinate, Geometry, LineString, MultiPolygon, Point, Polygon, WkbWriter, encode_native_array,
};
use oxigeo_geoparquet::metadata::{
    CoordDim, Crs, EncodingType, GeoParquetMetadata, GeometryColumnMetadata,
};
use oxigeo_geoparquet::{GeoParquetReader, GeoParquetWriter, GeoParquetWriterBuilder};
use parquet::arrow::ArrowWriter;
use parquet::file::properties::WriterProperties;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn temp_path(stem: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "oxigeo_native_{}_{}_{}.parquet",
        stem,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    p
}

fn cleanup(p: &Path) {
    let _ = std::fs::remove_file(p);
}

/// Attaches the GeoParquet `geo` metadata blob for a single geometry column to
/// `fields`, producing a schema `GeoParquetReader::open` will accept.
fn schema_with_geo_metadata(fields: Vec<Field>, column: &str, encoding: EncodingType) -> SchemaRef {
    let column_meta = if encoding == EncodingType::Wkb {
        GeometryColumnMetadata::new_wkb()
    } else {
        GeometryColumnMetadata::new_native(encoding)
    }
    .with_crs(Crs::wgs84());
    let mut geo_meta = GeoParquetMetadata::new(column);
    geo_meta.add_column(column, column_meta);

    let mut schema_meta = std::collections::HashMap::new();
    schema_meta.insert(
        "geo".to_string(),
        geo_meta.to_json().expect("serialize geo metadata"),
    );
    Arc::new(Schema::new_with_metadata(fields, schema_meta))
}

/// Writes a single `RecordBatch` to `path` with the plain Arrow writer.
///
/// Used by the null-preservation tests: the crate's own `GeoParquetWriter`
/// takes `&Geometry` and therefore cannot express a null geometry row, so those
/// fixtures have to be hand-built.
fn write_batch(path: &Path, schema: SchemaRef, batch: &RecordBatch) {
    let _ = std::fs::remove_file(path);
    let file = File::create(path).expect("create fixture");
    let mut writer = ArrowWriter::try_new(file, schema, Some(WriterProperties::builder().build()))
        .expect("arrow writer");
    writer.write(batch).expect("write batch");
    writer.close().expect("close writer");
}

/// Encodes `geom` as little-endian ISO WKB.
fn wkb(geom: &Geometry) -> Vec<u8> {
    WkbWriter::new(true).write_geometry(geom).expect("to wkb")
}

// ── Round-trip tests ────────────────────────────────────────────────────────────

#[test]
fn test_native_point_roundtrip_2d() {
    let path = temp_path("point_2d");
    let metadata = GeometryColumnMetadata::new_native(EncodingType::Point);
    let mut writer = GeoParquetWriterBuilder::new("geometry", metadata)
        .encoding(EncodingType::Point)
        .coord_dim(CoordDim::Xy)
        .build(&path)
        .expect("build writer");
    let geoms = vec![
        Geometry::Point(Point::new_2d(1.0, 2.0)),
        Geometry::Point(Point::new_2d(3.0, 4.0)),
        Geometry::Point(Point::new_2d(-5.5, 6.25)),
    ];
    for g in &geoms {
        writer.add_geometry(g).expect("add");
    }
    writer.finish().expect("finish");

    let reader = GeoParquetReader::open(&path).expect("open");
    let back = reader.read_geometries(0).expect("read geoms");
    assert_eq!(back, geoms);
    cleanup(&path);
}

#[test]
fn test_native_point_roundtrip_xyz() {
    let path = temp_path("point_xyz");
    let metadata = GeometryColumnMetadata::new_native(EncodingType::Point).with_crs(Crs::wgs84());
    let mut writer = GeoParquetWriterBuilder::new("geometry", metadata)
        .encoding(EncodingType::Point)
        .coord_dim(CoordDim::Xyz)
        .build(&path)
        .expect("build writer");
    let geoms = vec![
        Geometry::Point(Point::new_3d(1.0, 2.0, 10.0)),
        Geometry::Point(Point::new_3d(3.0, 4.0, 20.0)),
    ];
    for g in &geoms {
        writer.add_geometry(g).expect("add");
    }
    writer.finish().expect("finish");

    let reader = GeoParquetReader::open(&path).expect("open");
    let back = reader.read_geometries(0).expect("read");
    assert_eq!(back, geoms);
    cleanup(&path);
}

#[test]
fn test_native_linestring_roundtrip() {
    let path = temp_path("linestring");
    let metadata = GeometryColumnMetadata::new_native(EncodingType::LineString);
    let mut writer = GeoParquetWriterBuilder::new("geometry", metadata)
        .encoding(EncodingType::LineString)
        .coord_dim(CoordDim::Xy)
        .build(&path)
        .expect("build writer");
    let geoms = vec![
        Geometry::LineString(LineString::new(vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(2.0, 0.5),
        ])),
        Geometry::LineString(LineString::new(vec![
            Coordinate::new_2d(10.0, 10.0),
            Coordinate::new_2d(11.0, 11.0),
        ])),
    ];
    for g in &geoms {
        writer.add_geometry(g).expect("add");
    }
    writer.finish().expect("finish");

    let reader = GeoParquetReader::open(&path).expect("open");
    let back = reader.read_geometries(0).expect("read");
    assert_eq!(back, geoms);
    cleanup(&path);
}

#[test]
fn test_native_polygon_with_holes_roundtrip() {
    let path = temp_path("polygon_holes");
    let metadata = GeometryColumnMetadata::new_native(EncodingType::Polygon);
    let mut writer = GeoParquetWriterBuilder::new("geometry", metadata)
        .encoding(EncodingType::Polygon)
        .coord_dim(CoordDim::Xy)
        .build(&path)
        .expect("build writer");
    let exterior = LineString::new(vec![
        Coordinate::new_2d(0.0, 0.0),
        Coordinate::new_2d(10.0, 0.0),
        Coordinate::new_2d(10.0, 10.0),
        Coordinate::new_2d(0.0, 10.0),
        Coordinate::new_2d(0.0, 0.0),
    ]);
    let hole = LineString::new(vec![
        Coordinate::new_2d(2.0, 2.0),
        Coordinate::new_2d(4.0, 2.0),
        Coordinate::new_2d(4.0, 4.0),
        Coordinate::new_2d(2.0, 4.0),
        Coordinate::new_2d(2.0, 2.0),
    ]);
    let geoms = vec![Geometry::Polygon(Polygon::new(exterior, vec![hole]))];
    for g in &geoms {
        writer.add_geometry(g).expect("add");
    }
    writer.finish().expect("finish");

    let reader = GeoParquetReader::open(&path).expect("open");
    let back = reader.read_geometries(0).expect("read");
    assert_eq!(back, geoms);
    cleanup(&path);
}

#[test]
fn test_native_multipolygon_roundtrip() {
    let path = temp_path("multipolygon");
    let metadata = GeometryColumnMetadata::new_native(EncodingType::MultiPolygon);
    let mut writer = GeoParquetWriterBuilder::new("geometry", metadata)
        .encoding(EncodingType::MultiPolygon)
        .coord_dim(CoordDim::Xy)
        .build(&path)
        .expect("build writer");
    let p1 = Polygon::new_simple(LineString::new(vec![
        Coordinate::new_2d(0.0, 0.0),
        Coordinate::new_2d(1.0, 0.0),
        Coordinate::new_2d(1.0, 1.0),
        Coordinate::new_2d(0.0, 0.0),
    ]));
    let p2 = Polygon::new_simple(LineString::new(vec![
        Coordinate::new_2d(10.0, 10.0),
        Coordinate::new_2d(11.0, 10.0),
        Coordinate::new_2d(11.0, 11.0),
        Coordinate::new_2d(10.0, 10.0),
    ]));
    let geoms = vec![Geometry::MultiPolygon(MultiPolygon::new(vec![p1, p2]))];
    for g in &geoms {
        writer.add_geometry(g).expect("add");
    }
    writer.finish().expect("finish");

    let reader = GeoParquetReader::open(&path).expect("open");
    let back = reader.read_geometries(0).expect("read");
    assert_eq!(back, geoms);
    cleanup(&path);
}

#[test]
fn test_native_mixed_types_rejected_by_validate() {
    // Attempt to write a LineString into a Point-typed native column.
    let path = temp_path("mixed_rejected");
    let metadata = GeometryColumnMetadata::new_native(EncodingType::Point);
    let mut writer = GeoParquetWriterBuilder::new("geometry", metadata)
        .encoding(EncodingType::Point)
        .coord_dim(CoordDim::Xy)
        .build(&path)
        .expect("build writer");

    // This first geometry is fine.
    writer
        .add_geometry(&Geometry::Point(Point::new_2d(0.0, 0.0)))
        .expect("add point");
    // The second is wrong-type.
    writer
        .add_geometry(&Geometry::LineString(LineString::new(vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 1.0),
        ])))
        .expect("staging is allowed; reject happens at flush");

    let result = writer.finish();
    assert!(
        result.is_err(),
        "writing mixed types into a Point native column must be rejected at flush"
    );
    cleanup(&path);
}

// ── Back-compat regression: WKB writer default unchanged ────────────────────────

/// The default writer (no `encoding(...)` / `coord_dim(...)` calls) must
/// continue to emit a `Binary` geometry column carrying WKB blobs.  Only
/// the `geo` JSON `version` field changes (from `"1.0.0"` to `"1.1.0"`).
#[test]
fn test_wkb_writer_default_unchanged() {
    let path = temp_path("wkb_default");
    let metadata = GeometryColumnMetadata::new_wkb().with_crs(Crs::wgs84());
    let mut writer = GeoParquetWriter::new(&path, "geometry", metadata).expect("writer");
    writer
        .add_geometry(&Geometry::Point(Point::new_2d(1.0, 2.0)))
        .expect("add");
    writer
        .add_geometry(&Geometry::Point(Point::new_2d(3.0, 4.0)))
        .expect("add");
    writer.finish().expect("finish");

    // Re-open and verify the geometry column is still BinaryArray.
    let file = File::open(&path).expect("open");
    let builder = parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder::try_new(file)
        .expect("builder");
    let mut reader = builder.build().expect("build");
    let batch: RecordBatch = reader
        .next()
        .expect("at least one batch")
        .expect("ok batch");
    let geom_col = batch.column_by_name("geometry").expect("geom col");
    assert!(
        geom_col.as_any().is::<BinaryArray>(),
        "default writer must produce BinaryArray for WKB"
    );

    // GeoParquet metadata version must be bumped, but reader must still load.
    let reader = GeoParquetReader::open(&path).expect("read with new reader");
    assert_eq!(reader.metadata().version, "1.1.0");
    let geoms = reader.read_geometries(0).expect("read geoms");
    assert_eq!(geoms.len(), 2);
    cleanup(&path);
}

// ── Native + covering.bbox pushdown ─────────────────────────────────────────────

/// Write a file that has both a native Point geometry column AND covering
/// bbox columns (geometry_bbox_xmin/ymin/xmax/ymax).  Verify that
/// `read_pushdown` correctly intersects rows with the query bbox.
#[test]
fn test_native_with_covering_bbox_pushdown() {
    let path = temp_path("native_pushdown");

    // Build the schema by hand: native Point geometry + four bbox flat columns.
    let geom_field = oxigeo_geoparquet::arrow_ext::create_geometry_field_for(
        "geometry",
        EncodingType::Point,
        CoordDim::Xy,
        true,
        None,
    );
    let bbox_field = |name: &str| Field::new(name, DataType::Float64, false);
    let schema = Arc::new(Schema::new(vec![
        geom_field,
        bbox_field("geometry_bbox_xmin"),
        bbox_field("geometry_bbox_ymin"),
        bbox_field("geometry_bbox_xmax"),
        bbox_field("geometry_bbox_ymax"),
    ]));

    // Embed the geo metadata so GeoParquetReader::open accepts it.
    let column_meta =
        GeometryColumnMetadata::new_native(EncodingType::Point).with_crs(Crs::wgs84());
    let mut geo_meta = GeoParquetMetadata::new("geometry");
    geo_meta.add_column("geometry", column_meta);
    let geo_json = geo_meta.to_json().expect("to_json");
    let mut schema_meta = schema.metadata().clone();
    schema_meta.insert("geo".to_string(), geo_json);
    let schema = Arc::new(Schema::new_with_metadata(
        schema.fields().to_vec(),
        schema_meta,
    ));

    // Three points; bbox columns mirror each point exactly (degenerate boxes).
    let geoms = vec![
        Geometry::Point(Point::new_2d(1.0, 1.0)),
        Geometry::Point(Point::new_2d(50.0, 50.0)),
        Geometry::Point(Point::new_2d(2.0, 2.0)),
    ];
    let geom_arr =
        encode_native_array(&geoms, EncodingType::Point, CoordDim::Xy).expect("encode native");
    let xs: Vec<f64> = vec![1.0, 50.0, 2.0];
    let ys: Vec<f64> = vec![1.0, 50.0, 2.0];
    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            geom_arr,
            Arc::new(Float64Array::from(xs.clone())),
            Arc::new(Float64Array::from(ys.clone())),
            Arc::new(Float64Array::from(xs)),
            Arc::new(Float64Array::from(ys)),
        ],
    )
    .expect("batch");

    let _ = std::fs::remove_file(&path);
    let file = File::create(&path).expect("create");
    let mut writer = ArrowWriter::try_new(
        file,
        schema.clone(),
        Some(WriterProperties::builder().build()),
    )
    .expect("writer");
    writer.write(&batch).expect("write");
    writer.close().expect("close");

    // Query bbox (-1, -1, 5, 5) — overlaps points 0 and 2, misses point 1.
    let reader = GeoParquetReader::open(&path)
        .expect("open")
        .with_bbox_filter((-1.0, -1.0, 5.0, 5.0));
    let results = reader.read_pushdown().expect("pushdown");
    let total: usize = results.iter().map(|b| b.num_rows()).sum();
    assert_eq!(total, 2, "should retain points 0 and 2 only");

    // Confirm the geometry column is still typed as a FixedSizeList<f64,2>.
    for batch in &results {
        let geom_col = batch.column_by_name("geometry").expect("geom");
        // The column's DataType should still be FixedSizeList (native), not Binary.
        match geom_col.data_type() {
            DataType::FixedSizeList(_, 2) => {}
            other => panic!("expected FixedSizeList<f64,2> for native geometry, got {other:?}"),
        }
        // Row 1's xmin is 50, so the surviving bbox xmins must be < 5.
        let xmin = batch
            .column_by_name("geometry_bbox_xmin")
            .expect("xmin")
            .as_any()
            .downcast_ref::<Float64Array>()
            .expect("f64");
        for i in 0..xmin.len() {
            assert!(xmin.value(i) < 5.0, "row {i} should have xmin < 5");
        }
    }
    cleanup(&path);
}

// ── Batch reader: encoding dispatch (native decode through `read_all`) ──────────

/// Regression: `GeoParquetBatchReader::extract_geometries` used to downcast the
/// geometry column to `BinaryArray` unconditionally, so a GeoArrow-native file
/// read through `read_all()` / `next_batch()` failed with a type mismatch
/// instead of decoding.  The batch reader now carries the geometry column's
/// declared encoding and dispatches exactly like
/// `GeoParquetReader::read_geometries`.
#[test]
fn test_batch_reader_decodes_native_point_column() {
    let path = temp_path("batch_native_point");
    let metadata = GeometryColumnMetadata::new_native(EncodingType::Point);
    let mut writer = GeoParquetWriterBuilder::new("geometry", metadata)
        .encoding(EncodingType::Point)
        .coord_dim(CoordDim::Xy)
        .build(&path)
        .expect("build writer");
    let geoms = vec![
        Geometry::Point(Point::new_2d(1.0, 2.0)),
        Geometry::Point(Point::new_2d(3.0, 4.0)),
        Geometry::Point(Point::new_2d(-5.5, 6.25)),
    ];
    for g in &geoms {
        writer.add_geometry(g).expect("add");
    }
    writer.finish().expect("finish");

    let reader = GeoParquetReader::open(&path).expect("open");
    let mut batches = reader.read_all().expect("read_all");
    assert_eq!(batches.geometry_encoding(), EncodingType::Point);

    let mut collected: Vec<Geometry> = Vec::new();
    let mut saw_batch = false;
    while let Some(batch) = batches.next_batch().expect("next_batch") {
        saw_batch = true;
        // Before the fix this call returned a `type_mismatch` error.
        collected.extend(
            batches
                .extract_geometries(&batch)
                .expect("native geometries must decode through the batch reader"),
        );
    }
    assert!(saw_batch, "expected at least one batch");
    assert_eq!(collected, geoms);
    cleanup(&path);
}

/// The same dispatch fix must hold for a nested native encoding (LineString),
/// not just the flat `FixedSizeList` Point case.
#[test]
fn test_batch_reader_decodes_native_linestring_column() {
    let path = temp_path("batch_native_line");
    let metadata = GeometryColumnMetadata::new_native(EncodingType::LineString);
    let mut writer = GeoParquetWriterBuilder::new("geometry", metadata)
        .encoding(EncodingType::LineString)
        .coord_dim(CoordDim::Xy)
        .build(&path)
        .expect("build writer");
    let geoms = vec![
        Geometry::LineString(LineString::new(vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 1.0),
        ])),
        Geometry::LineString(LineString::new(vec![
            Coordinate::new_2d(5.0, 5.0),
            Coordinate::new_2d(6.0, 7.0),
            Coordinate::new_2d(8.0, 9.0),
        ])),
    ];
    for g in &geoms {
        writer.add_geometry(g).expect("add");
    }
    writer.finish().expect("finish");

    let reader = GeoParquetReader::open(&path).expect("open");
    let mut batches = reader.read_all().expect("read_all");
    let mut collected: Vec<Geometry> = Vec::new();
    while let Some(batch) = batches.next_batch().expect("next_batch") {
        collected.extend(batches.extract_geometries(&batch).expect("extract"));
    }
    assert_eq!(collected, geoms);
    cleanup(&path);
}

// ── Null geometries keep their row index ───────────────────────────────────────

/// Builds a three-row WKB fixture whose middle geometry is NULL, plus an `id`
/// property column so the index desync is directly observable.
fn write_wkb_null_fixture(path: &Path) -> Vec<Option<Geometry>> {
    let expected = vec![
        Some(Geometry::Point(Point::new_2d(1.0, 1.0))),
        None,
        Some(Geometry::Point(Point::new_2d(3.0, 3.0))),
    ];
    let blob0 = wkb(expected[0].as_ref().expect("row 0"));
    let blob2 = wkb(expected[2].as_ref().expect("row 2"));

    let schema = schema_with_geo_metadata(
        vec![
            Field::new("geometry", DataType::Binary, true),
            Field::new("id", DataType::Int32, false),
        ],
        "geometry",
        EncodingType::Wkb,
    );

    let geom_arr = BinaryArray::from(vec![Some(blob0.as_slice()), None, Some(blob2.as_slice())]);
    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(geom_arr) as ArrayRef,
            Arc::new(Int32Array::from(vec![10, 20, 30])) as ArrayRef,
        ],
    )
    .expect("batch");

    write_batch(path, schema, &batch);
    expected
}

/// Regression (WKB arm): `read_geometries` drops null rows, silently shifting
/// every later geometry one slot away from its property row.
/// `read_geometries_optional` must keep the null at index 1.
#[test]
fn test_read_geometries_optional_preserves_wkb_null_index() {
    let path = temp_path("wkb_null_index");
    let expected = write_wkb_null_fixture(&path);

    let reader = GeoParquetReader::open(&path).expect("open");

    // Legacy behaviour is deliberately unchanged: nulls are dropped.
    let compacted = reader.read_geometries(0).expect("read_geometries");
    assert_eq!(compacted.len(), 2, "legacy path still compacts nulls");

    // The new optional variant keeps one entry per row.
    let optional = reader
        .read_geometries_optional(0)
        .expect("read_geometries_optional");
    assert_eq!(optional.len(), 3, "one entry per row, nulls included");
    assert!(optional[1].is_none(), "the null must stay at index 1");
    assert_eq!(optional, expected);

    // And the geometries now line up with the property column again.
    let batch = reader.read_row_group(0).expect("row group");
    let ids = batch
        .column_by_name("id")
        .expect("id column")
        .as_any()
        .downcast_ref::<Int32Array>()
        .expect("Int32Array");
    assert_eq!(optional.len(), ids.len());
    assert_eq!(ids.value(2), 30);
    assert_eq!(
        optional[2],
        Some(Geometry::Point(Point::new_2d(3.0, 3.0))),
        "row 2's geometry must pair with row 2's id"
    );

    cleanup(&path);
}

/// The batch-reader twin of the previous test: `extract_geometries_optional`
/// must also keep the null at its index (WKB arm).
#[test]
fn test_extract_geometries_optional_preserves_wkb_null_index() {
    let path = temp_path("wkb_null_index_batch");
    let expected = write_wkb_null_fixture(&path);

    let reader = GeoParquetReader::open(&path).expect("open");
    let mut batches = reader.read_all().expect("read_all");

    let mut optional: Vec<Option<Geometry>> = Vec::new();
    let mut compacted: Vec<Geometry> = Vec::new();
    while let Some(batch) = batches.next_batch().expect("next_batch") {
        assert_eq!(batch.num_rows(), 3);
        optional.extend(
            batches
                .extract_geometries_optional(&batch)
                .expect("extract optional"),
        );
        compacted.extend(batches.extract_geometries(&batch).expect("extract"));
    }

    assert_eq!(compacted.len(), 2, "legacy path still compacts nulls");
    assert_eq!(optional.len(), 3);
    assert!(optional[1].is_none());
    assert_eq!(optional, expected);
    cleanup(&path);
}

/// Builds a three-row **native** Point fixture whose middle geometry is NULL.
///
/// `encode_native_array` takes `&[Geometry]` and so cannot express a null, so
/// the `FixedSizeList<f64, 2>` array is built by hand.  The null slot is still
/// padded with two coordinate values, as the fixed-size layout requires.
fn write_native_null_fixture(path: &Path) -> Vec<Option<Geometry>> {
    let expected = vec![
        Some(Geometry::Point(Point::new_2d(1.0, 1.0))),
        None,
        Some(Geometry::Point(Point::new_2d(3.0, 3.0))),
    ];

    let mut builder = FixedSizeListBuilder::new(Float64Builder::new(), 2)
        .with_field(Arc::new(Field::new("xy", DataType::Float64, false)));
    for entry in &expected {
        match entry {
            Some(Geometry::Point(p)) => {
                builder.values().append_value(p.coord.x);
                builder.values().append_value(p.coord.y);
                builder.append(true);
            }
            _ => {
                // Fixed-size lists still occupy `arity` child slots when null.
                builder.values().append_value(0.0);
                builder.values().append_value(0.0);
                builder.append(false);
            }
        }
    }
    let geom_arr = builder.finish();

    let geom_field = oxigeo_geoparquet::arrow_ext::create_geometry_field_for(
        "geometry",
        EncodingType::Point,
        CoordDim::Xy,
        true,
        None,
    );
    let schema = schema_with_geo_metadata(
        vec![geom_field, Field::new("id", DataType::Int32, false)],
        "geometry",
        EncodingType::Point,
    );

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(geom_arr) as ArrayRef,
            Arc::new(Int32Array::from(vec![10, 20, 30])) as ArrayRef,
        ],
    )
    .expect("batch");

    write_batch(path, schema, &batch);
    expected
}

/// Regression (native arm): `decode_native_array` flattens nulls away, so
/// `read_geometries` on a native column loses the row alignment just as the WKB
/// path did.  `read_geometries_optional` routes through
/// `decode_native_array_optional` and must keep the null at index 1.
#[test]
fn test_read_geometries_optional_preserves_native_null_index() {
    let path = temp_path("native_null_index");
    let expected = write_native_null_fixture(&path);

    let reader = GeoParquetReader::open(&path).expect("open");

    let compacted = reader.read_geometries(0).expect("read_geometries");
    assert_eq!(compacted.len(), 2, "legacy path still compacts nulls");

    let optional = reader
        .read_geometries_optional(0)
        .expect("read_geometries_optional");
    assert_eq!(optional.len(), 3);
    assert!(
        optional[1].is_none(),
        "the null native geometry must stay at index 1, not be flattened away"
    );
    assert_eq!(optional, expected);
    cleanup(&path);
}

/// Batch-reader twin for the native arm — exercises both the encoding dispatch
/// (issue 12) and the null preservation (issue 11) in one pass.
#[test]
fn test_extract_geometries_optional_preserves_native_null_index() {
    let path = temp_path("native_null_index_batch");
    let expected = write_native_null_fixture(&path);

    let reader = GeoParquetReader::open(&path).expect("open");
    let mut batches = reader.read_all().expect("read_all");
    assert_eq!(batches.geometry_encoding(), EncodingType::Point);

    let mut optional: Vec<Option<Geometry>> = Vec::new();
    while let Some(batch) = batches.next_batch().expect("next_batch") {
        optional.extend(
            batches
                .extract_geometries_optional(&batch)
                .expect("native optional extraction must decode, not type-mismatch"),
        );
    }

    assert_eq!(optional.len(), 3);
    assert!(optional[1].is_none());
    assert_eq!(optional, expected);
    cleanup(&path);
}

/// Null preservation must also hold for the deepest native layout,
/// `List<List<List<FixedSizeList<f64, N>>>>` (MultiPolygon) — that decoder walks
/// three offset levels, so a missing null guard there would silently produce a
/// bogus polygon instead of a `None`.
///
/// `encode_native_array` cannot emit a null, so a valid three-row array is
/// encoded first and row 1's validity bit is then cleared in place.
#[test]
fn test_read_geometries_optional_preserves_native_multipolygon_null_index() {
    let path = temp_path("native_mpoly_null");

    let square = |x: f64| {
        Geometry::MultiPolygon(MultiPolygon::new(vec![Polygon::new_simple(
            LineString::new(vec![
                Coordinate::new_2d(x, x),
                Coordinate::new_2d(x + 1.0, x),
                Coordinate::new_2d(x + 1.0, x + 1.0),
                Coordinate::new_2d(x, x),
            ]),
        )]))
    };
    let dense = vec![square(0.0), square(10.0), square(20.0)];
    let encoded = encode_native_array(&dense, EncodingType::MultiPolygon, CoordDim::Xy)
        .expect("encode multipolygons");

    // Clear row 1's validity bit, leaving the offsets/values untouched.
    let list = encoded
        .as_any()
        .downcast_ref::<ListArray>()
        .expect("multipolygon column is a ListArray")
        .clone();
    let validity: Vec<bool> = (0..list.len()).map(|i| i != 1).collect();
    let (field, offsets, values, _) = list.into_parts();
    let geom_arr = ListArray::new(field, offsets, values, Some(NullBuffer::from(validity)));

    let geom_field = oxigeo_geoparquet::arrow_ext::create_geometry_field_for(
        "geometry",
        EncodingType::MultiPolygon,
        CoordDim::Xy,
        true,
        None,
    );
    let schema = schema_with_geo_metadata(
        vec![geom_field, Field::new("id", DataType::Int32, false)],
        "geometry",
        EncodingType::MultiPolygon,
    );
    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(geom_arr) as ArrayRef,
            Arc::new(Int32Array::from(vec![10, 20, 30])) as ArrayRef,
        ],
    )
    .expect("batch");
    write_batch(&path, schema, &batch);

    let expected = vec![Some(dense[0].clone()), None, Some(dense[2].clone())];

    let reader = GeoParquetReader::open(&path).expect("open");
    assert_eq!(
        reader.read_geometries(0).expect("read_geometries").len(),
        2,
        "legacy path still compacts nulls"
    );

    let optional = reader
        .read_geometries_optional(0)
        .expect("read_geometries_optional");
    assert_eq!(optional.len(), 3);
    assert!(
        optional[1].is_none(),
        "the null multipolygon must stay at index 1"
    );
    assert_eq!(optional, expected);

    // And through the batch reader, which also has to dispatch on the encoding.
    let mut batches = reader.read_all().expect("read_all");
    let mut via_batches: Vec<Option<Geometry>> = Vec::new();
    while let Some(b) = batches.next_batch().expect("next_batch") {
        via_batches.extend(
            batches
                .extract_geometries_optional(&b)
                .expect("extract optional"),
        );
    }
    assert_eq!(via_batches, expected);

    cleanup(&path);
}
