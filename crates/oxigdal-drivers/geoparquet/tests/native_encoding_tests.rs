//! Integration tests for GeoParquet 1.1 GeoArrow native encoding round-trips.
//!
//! Each test writes a small file via [`GeoParquetWriter`] using the native
//! encoding builder, reads it back via [`GeoParquetReader::read_geometries`],
//! and asserts a clean round-trip.  The back-compat regression test
//! `test_wkb_writer_default_unchanged` confirms the default writer still
//! emits a 1.0-shape WKB column (modulo the version-string bump to 1.1.0).
#![allow(clippy::expect_used)]
#![allow(clippy::panic)]

use arrow_array::{Array, BinaryArray, Float64Array, RecordBatch};
use arrow_schema::{DataType, Field, Schema};
use oxigdal_geoparquet::geometry::{
    Coordinate, Geometry, LineString, MultiPolygon, Point, Polygon, encode_native_array,
};
use oxigdal_geoparquet::metadata::{
    CoordDim, Crs, EncodingType, GeoParquetMetadata, GeometryColumnMetadata,
};
use oxigdal_geoparquet::{GeoParquetReader, GeoParquetWriter, GeoParquetWriterBuilder};
use parquet::arrow::ArrowWriter;
use parquet::file::properties::WriterProperties;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn temp_path(stem: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "oxigdal_native_{}_{}_{}.parquet",
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
    let geom_field = oxigdal_geoparquet::arrow_ext::create_geometry_field_for(
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
