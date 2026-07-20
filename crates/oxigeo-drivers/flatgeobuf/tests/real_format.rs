//! Tests asserting that the on-disk bytes are the *real* FlatGeobuf format:
//! magic bytes, a size-prefixed FlatBuffers `Header` table, and size-prefixed
//! FlatBuffers `Feature` tables (interoperable with GDAL and other tooling),
//! rather than an ad-hoc custom binary layout.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use oxigeo_core::vector::{Coordinate, Feature, FieldValue, Geometry, LineString, Point, Polygon};
use oxigeo_flatgeobuf::fbs::{self, FbTable};
use oxigeo_flatgeobuf::feature_codec;
use oxigeo_flatgeobuf::geometry::GeometryCodec;
use oxigeo_flatgeobuf::{
    Column, ColumnType, FlatGeobufReader, FlatGeobufWriter, GeometryType, Header, MAGIC_BYTES,
};
use std::io::Cursor;

/// Splits a written FlatGeobuf byte stream (no spatial index) into its parts:
/// returns `(header_bytes, first_feature_bytes)`.
fn split_no_index(data: &[u8]) -> (&[u8], &[u8]) {
    assert_eq!(&data[0..8], MAGIC_BYTES, "magic bytes must be present");
    let header_size = u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as usize;
    let header_bytes = &data[12..12 + header_size];
    let feat_start = 12 + header_size;
    let feat_size = u32::from_le_bytes([
        data[feat_start],
        data[feat_start + 1],
        data[feat_start + 2],
        data[feat_start + 3],
    ]) as usize;
    let feature_bytes = &data[feat_start + 4..feat_start + 4 + feat_size];
    (header_bytes, feature_bytes)
}

#[test]
fn test_ondisk_header_is_real_flatbuffers() {
    let mut header = Header::new(GeometryType::Point);
    header.add_column(Column::new("name", ColumnType::String));
    header.add_column(Column::new("pop", ColumnType::Int));

    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("create writer");
    let mut f = Feature::new(Geometry::Point(Point::new(11.0, 22.0)));
    f.set_property("name", FieldValue::String("Tokyo".to_string()));
    f.set_property("pop", FieldValue::Integer(37_000_000));
    writer.add_feature(&f).expect("add feature");
    let cursor = writer.finish().expect("finish");
    let data = cursor.into_inner();

    let (header_bytes, _feature_bytes) = split_no_index(&data);

    // Decode the header as a genuine FlatBuffers `Header` table.
    let decoded = Header::from_bytes(header_bytes).expect("decode header FlatBuffer");
    assert_eq!(decoded.geometry_type, GeometryType::Point);
    assert_eq!(decoded.columns.len(), 2);
    assert_eq!(decoded.columns[0].name, "name");
    assert_eq!(decoded.columns[1].column_type, ColumnType::Int);
    assert_eq!(decoded.features_count, Some(1));

    // Independently walk the FlatBuffers wire format: the root uoffset must
    // point to a table whose vtable resolves the geometry_type slot to Point.
    let table = FbTable::root(header_bytes).expect("root table");
    let gt = table
        .get_u8(fbs::HEADER_VT_GEOMETRY_TYPE, 0)
        .expect("geometry_type slot");
    assert_eq!(gt, GeometryType::Point as u8);
    let features_count = table
        .get_u64(fbs::HEADER_VT_FEATURES_COUNT, 0)
        .expect("features_count slot");
    assert_eq!(features_count, 1);
}

#[test]
fn test_ondisk_feature_is_real_flatbuffers() {
    let header = Header::new(GeometryType::Point);
    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("create writer");
    writer
        .add_feature(&Feature::new(Geometry::Point(Point::new(3.5, -4.25))))
        .expect("add feature");
    let cursor = writer.finish().expect("finish");
    let data = cursor.into_inner();

    let (_header_bytes, feature_bytes) = split_no_index(&data);

    // Walk the feature FlatBuffer: Feature.geometry -> Geometry.xy = [3.5, -4.25].
    let feat = FbTable::root(feature_bytes).expect("feature root table");
    let geom = feat
        .get_table(fbs::FEATURE_VT_GEOMETRY)
        .expect("geometry slot")
        .expect("geometry present");
    let xy = geom
        .get_f64_vector(fbs::GEOM_VT_XY)
        .expect("xy slot")
        .expect("xy present");
    assert_eq!(xy, vec![3.5, -4.25]);

    // And decode via the shared codec for good measure.
    let codec = GeometryCodec::new(false, false);
    let decoded =
        feature_codec::decode_feature(&Header::new(GeometryType::Point), &codec, feature_bytes)
            .expect("decode feature");
    if let Some(Geometry::Point(p)) = decoded.geometry {
        assert_eq!(p.coord.x, 3.5);
        assert_eq!(p.coord.y, -4.25);
    } else {
        panic!("expected point geometry");
    }
}

#[test]
fn test_real_format_roundtrip_geometry_and_attributes() {
    let mut header = Header::new(GeometryType::Polygon);
    header.add_column(Column::new("name", ColumnType::String));
    header.add_column(Column::new("area", ColumnType::Double));
    header.add_column(Column::new("rank", ColumnType::Int));

    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("create writer");

    let exterior = LineString::new(vec![
        Coordinate::new_2d(0.0, 0.0),
        Coordinate::new_2d(4.0, 0.0),
        Coordinate::new_2d(4.0, 4.0),
        Coordinate::new_2d(0.0, 4.0),
        Coordinate::new_2d(0.0, 0.0),
    ])
    .expect("exterior");
    let hole = LineString::new(vec![
        Coordinate::new_2d(1.0, 1.0),
        Coordinate::new_2d(2.0, 1.0),
        Coordinate::new_2d(2.0, 2.0),
        Coordinate::new_2d(1.0, 2.0),
        Coordinate::new_2d(1.0, 1.0),
    ])
    .expect("hole");
    let poly = Polygon::new(exterior, vec![hole]).expect("polygon");

    let mut f = Feature::new(Geometry::Polygon(poly));
    f.set_property("name", FieldValue::String("plaza".to_string()));
    f.set_property("area", FieldValue::Float(12.5));
    f.set_property("rank", FieldValue::Integer(-7));
    writer.add_feature(&f).expect("add feature");

    let cursor = writer.finish().expect("finish");
    let data = cursor.into_inner();

    // Read back through the public reader (which parses real FlatBuffers).
    let cursor = Cursor::new(data);
    let mut reader = FlatGeobufReader::new(cursor).expect("open reader");
    assert_eq!(reader.header().geometry_type, GeometryType::Polygon);

    let mut it = reader.features().expect("iterator");
    let got = it.next().expect("one feature").expect("read feature");

    match &got.geometry {
        Some(Geometry::Polygon(p)) => {
            assert_eq!(p.exterior.coords.len(), 5);
            assert_eq!(p.interiors.len(), 1);
            assert_eq!(p.interiors[0].coords.len(), 5);
            assert_eq!(p.exterior.coords[1].x, 4.0);
            assert_eq!(p.interiors[0].coords[2].y, 2.0);
        }
        other => panic!("expected polygon, got {other:?}"),
    }

    assert_eq!(
        got.get_property("name").and_then(|v| v.as_string()),
        Some("plaza")
    );
    assert_eq!(
        got.get_property("area").and_then(|v| v.as_f64()),
        Some(12.5)
    );
    assert_eq!(got.get_property("rank").and_then(|v| v.as_i64()), Some(-7));

    assert!(it.next().is_none(), "exactly one feature expected");
}
