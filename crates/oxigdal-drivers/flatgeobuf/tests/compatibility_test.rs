//! Real-world FlatGeobuf file compatibility tests
//!
//! Tests for reading various FlatGeobuf versions, geometry types, CRS configurations,
//! attribute types, spatial index verification, large files, and edge cases.
//!
//! All tests follow COOLJAPAN policies:
//! - No unwrap() - use ? or expect()
//! - Use std::env::temp_dir() for temp files
//! - Pure Rust implementation
#![allow(clippy::panic, clippy::while_let_on_iterator)]

use oxigdal_core::vector::{
    Coordinate, Feature, Geometry, GeometryCollection, LineString, MultiLineString, MultiPolygon,
    Point, Polygon, PropertyValue,
};
use oxigdal_flatgeobuf::{
    Column, ColumnType, CrsInfo, FlatGeobufError, FlatGeobufReader, FlatGeobufWriter,
    FlatGeobufWriterBuilder, GeometryType, Header, MAGIC_BYTES, VERSION, index::BoundingBox,
};
use std::fs::File;
use std::io::Cursor;
use std::path::PathBuf;

/// Helper to create a temp file path
fn temp_file_path(name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "oxigdal_fgb_test_{name}_{}.fgb",
        std::process::id()
    ));
    path
}

/// Helper to clean up temp files
fn cleanup_temp_file(path: &PathBuf) {
    let _ = std::fs::remove_file(path);
}

// =============================================================================
// SECTION 1: FlatGeobuf Version Tests
// =============================================================================

#[test]
fn test_magic_bytes_validation() {
    // Verify correct magic bytes constant
    assert_eq!(MAGIC_BYTES.len(), 8);
    assert_eq!(MAGIC_BYTES[0], b'f');
    assert_eq!(MAGIC_BYTES[1], b'g');
    assert_eq!(MAGIC_BYTES[2], b'b');
    assert_eq!(MAGIC_BYTES[3], 0x03); // Version marker
    assert_eq!(MAGIC_BYTES[4], b'f');
    assert_eq!(MAGIC_BYTES[5], b'g');
    assert_eq!(MAGIC_BYTES[6], b'b');
    assert_eq!(MAGIC_BYTES[7], 0x00);
}

#[test]
fn test_version_constant() {
    assert_eq!(VERSION, 3, "FlatGeobuf version should be 3");
}

#[test]
fn test_invalid_magic_bytes_detection() {
    let invalid_data = b"INVALID\x00some random data";
    let cursor = Cursor::new(invalid_data.to_vec());
    let result = FlatGeobufReader::new(cursor);

    assert!(result.is_err());
    if let Err(FlatGeobufError::InvalidMagic { expected, actual }) = result {
        assert_eq!(expected, MAGIC_BYTES);
        assert_eq!(&actual[..7], b"INVALID");
    } else {
        panic!("Expected InvalidMagic error");
    }
}

#[test]
fn test_truncated_magic_bytes() {
    let truncated_data = b"fgb";
    let cursor = Cursor::new(truncated_data.to_vec());
    let result = FlatGeobufReader::new(cursor);

    assert!(result.is_err(), "Should fail on truncated magic bytes");
}

#[test]
fn test_version_3_file_roundtrip() {
    // Create a version 3 file
    let header = Header::new(GeometryType::Point);
    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    let point = Point::new(1.0, 2.0);
    let feature = Feature::new(Geometry::Point(point));
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish");
    let data = cursor.into_inner();

    // Verify magic bytes in output
    assert_eq!(&data[0..8], MAGIC_BYTES);

    // Verify readable
    let cursor = Cursor::new(data);
    let reader = FlatGeobufReader::new(cursor);
    assert!(reader.is_ok());
}

// =============================================================================
// SECTION 2: Geometry Type Tests
// =============================================================================

#[test]
fn test_geometry_type_point() {
    let header = Header::new(GeometryType::Point);
    assert_eq!(header.geometry_type, GeometryType::Point);
    assert_eq!(header.geometry_type as u8, 1);
}

#[test]
fn test_geometry_type_linestring() {
    let header = Header::new(GeometryType::LineString);
    assert_eq!(header.geometry_type, GeometryType::LineString);
    assert_eq!(header.geometry_type as u8, 2);
}

#[test]
fn test_geometry_type_polygon() {
    let header = Header::new(GeometryType::Polygon);
    assert_eq!(header.geometry_type, GeometryType::Polygon);
    assert_eq!(header.geometry_type as u8, 3);
}

#[test]
fn test_geometry_type_multipoint() {
    let header = Header::new(GeometryType::MultiPoint);
    assert_eq!(header.geometry_type, GeometryType::MultiPoint);
    assert_eq!(header.geometry_type as u8, 4);
}

#[test]
fn test_geometry_type_multilinestring() {
    let header = Header::new(GeometryType::MultiLineString);
    assert_eq!(header.geometry_type, GeometryType::MultiLineString);
    assert_eq!(header.geometry_type as u8, 5);
}

#[test]
fn test_geometry_type_multipolygon() {
    let header = Header::new(GeometryType::MultiPolygon);
    assert_eq!(header.geometry_type, GeometryType::MultiPolygon);
    assert_eq!(header.geometry_type as u8, 6);
}

#[test]
fn test_geometry_type_geometry_collection() {
    let header = Header::new(GeometryType::GeometryCollection);
    assert_eq!(header.geometry_type, GeometryType::GeometryCollection);
    assert_eq!(header.geometry_type as u8, 7);
}

#[test]
fn test_geometry_type_unknown() {
    let header = Header::new(GeometryType::Unknown);
    assert_eq!(header.geometry_type, GeometryType::Unknown);
    assert_eq!(header.geometry_type as u8, 0);
}

#[test]
fn test_multilinestring_roundtrip() {
    let header = Header::new(GeometryType::MultiLineString);
    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    let ls1 = LineString::new(vec![
        Coordinate::new_2d(0.0, 0.0),
        Coordinate::new_2d(1.0, 1.0),
    ])
    .expect("Failed to create linestring 1");

    let ls2 = LineString::new(vec![
        Coordinate::new_2d(2.0, 2.0),
        Coordinate::new_2d(3.0, 3.0),
    ])
    .expect("Failed to create linestring 2");

    let mls = MultiLineString::new(vec![ls1, ls2]);
    let feature = Feature::new(Geometry::MultiLineString(mls));
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    let mut features_iter = reader.features().expect("Failed to get iterator");
    let result = features_iter.next().expect("No feature found");
    let feature = result.expect("Failed to read feature");

    if let Some(Geometry::MultiLineString(mls)) = &feature.geometry {
        assert_eq!(mls.line_strings.len(), 2);
        assert_eq!(mls.line_strings[0].len(), 2);
        assert_eq!(mls.line_strings[1].len(), 2);
    } else {
        panic!("Expected MultiLineString geometry");
    }
}

#[test]
fn test_multipolygon_roundtrip() {
    let header = Header::new(GeometryType::MultiPolygon);
    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    let ext1 = LineString::new(vec![
        Coordinate::new_2d(0.0, 0.0),
        Coordinate::new_2d(1.0, 0.0),
        Coordinate::new_2d(1.0, 1.0),
        Coordinate::new_2d(0.0, 1.0),
        Coordinate::new_2d(0.0, 0.0),
    ])
    .expect("Failed to create exterior 1");

    let ext2 = LineString::new(vec![
        Coordinate::new_2d(5.0, 5.0),
        Coordinate::new_2d(6.0, 5.0),
        Coordinate::new_2d(6.0, 6.0),
        Coordinate::new_2d(5.0, 6.0),
        Coordinate::new_2d(5.0, 5.0),
    ])
    .expect("Failed to create exterior 2");

    let poly1 = Polygon::new(ext1, vec![]).expect("Failed to create polygon 1");
    let poly2 = Polygon::new(ext2, vec![]).expect("Failed to create polygon 2");

    let mp = MultiPolygon::new(vec![poly1, poly2]);
    let feature = Feature::new(Geometry::MultiPolygon(mp));
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    let mut features_iter = reader.features().expect("Failed to get iterator");
    let result = features_iter.next().expect("No feature found");
    let feature = result.expect("Failed to read feature");

    if let Some(Geometry::MultiPolygon(mp)) = &feature.geometry {
        assert_eq!(mp.polygons.len(), 2);
    } else {
        panic!("Expected MultiPolygon geometry");
    }
}

#[test]
fn test_geometry_collection_roundtrip() {
    let header = Header::new(GeometryType::GeometryCollection);
    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    let point = Geometry::Point(Point::new(0.0, 0.0));
    let ls = Geometry::LineString(
        LineString::new(vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 1.0),
        ])
        .expect("Failed to create linestring"),
    );

    let gc = GeometryCollection::new(vec![point, ls]);
    let feature = Feature::new(Geometry::GeometryCollection(gc));
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    let mut features_iter = reader.features().expect("Failed to get iterator");
    let result = features_iter.next().expect("No feature found");
    let feature = result.expect("Failed to read feature");

    if let Some(Geometry::GeometryCollection(gc)) = &feature.geometry {
        assert_eq!(gc.geometries.len(), 2);
        assert!(matches!(&gc.geometries[0], Geometry::Point(_)));
        assert!(matches!(&gc.geometries[1], Geometry::LineString(_)));
    } else {
        panic!("Expected GeometryCollection");
    }
}

#[test]
fn test_2d_geometry() {
    let header = Header::new(GeometryType::Point);
    assert!(!header.has_z);
    assert!(!header.has_m);

    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    let point = Point::new(10.0, 20.0);
    let feature = Feature::new(Geometry::Point(point));
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    let mut features_iter = reader.features().expect("Failed to get iterator");
    let result = features_iter.next().expect("No feature found");
    let feature = result.expect("Failed to read feature");

    if let Some(Geometry::Point(p)) = &feature.geometry {
        assert_eq!(p.coord.x, 10.0);
        assert_eq!(p.coord.y, 20.0);
        assert!(p.coord.z.is_none());
        assert!(p.coord.m.is_none());
    } else {
        panic!("Expected Point geometry");
    }
}

#[test]
fn test_3d_geometry_z_only() {
    let header = Header::new(GeometryType::Point).with_z();
    assert!(header.has_z);
    assert!(!header.has_m);

    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    let point = Point::new_3d(10.0, 20.0, 30.0);
    let feature = Feature::new(Geometry::Point(point));
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    assert!(reader.header().has_z);
    assert!(!reader.header().has_m);

    let mut features_iter = reader.features().expect("Failed to get iterator");
    let result = features_iter.next().expect("No feature found");
    let feature = result.expect("Failed to read feature");

    if let Some(Geometry::Point(p)) = &feature.geometry {
        assert_eq!(p.coord.x, 10.0);
        assert_eq!(p.coord.y, 20.0);
        assert_eq!(p.coord.z, Some(30.0));
    } else {
        panic!("Expected Point geometry");
    }
}

#[test]
fn test_4d_geometry_zm() {
    let header = Header::new(GeometryType::Point).with_z().with_m();
    assert!(header.has_z);
    assert!(header.has_m);

    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    let coord = Coordinate::new_3dm(10.0, 20.0, 30.0, 40.0);
    let point = Point::from_coord(coord);
    let feature = Feature::new(Geometry::Point(point));
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    assert!(reader.header().has_z);
    assert!(reader.header().has_m);

    let mut features_iter = reader.features().expect("Failed to get iterator");
    let result = features_iter.next().expect("No feature found");
    let feature = result.expect("Failed to read feature");

    if let Some(Geometry::Point(p)) = &feature.geometry {
        assert_eq!(p.coord.x, 10.0);
        assert_eq!(p.coord.y, 20.0);
        assert_eq!(p.coord.z, Some(30.0));
        assert_eq!(p.coord.m, Some(40.0));
    } else {
        panic!("Expected Point geometry");
    }
}

// =============================================================================
// SECTION 3: CRS Configuration Tests
// =============================================================================

#[test]
fn test_crs_epsg_4326() {
    let crs = CrsInfo::from_epsg(4326);
    assert_eq!(crs.organization, Some("EPSG".to_string()));
    assert_eq!(crs.organization_code, Some(4326));
    assert_eq!(crs.name, Some("EPSG:4326".to_string()));
}

#[test]
fn test_crs_epsg_3857() {
    let crs = CrsInfo::from_epsg(3857);
    assert_eq!(crs.organization, Some("EPSG".to_string()));
    assert_eq!(crs.organization_code, Some(3857));
    assert_eq!(crs.name, Some("EPSG:3857".to_string()));
}

#[test]
fn test_crs_custom_wkt() {
    let wkt = r#"GEOGCS["WGS 84",DATUM["WGS_1984",SPHEROID["WGS 84",6378137,298.257223563]],PRIMEM["Greenwich",0],UNIT["degree",0.0174532925199433]]"#;
    let crs = CrsInfo::from_wkt(wkt);
    assert!(crs.wkt.is_some());
    assert_eq!(crs.wkt.as_deref(), Some(wkt));
}

#[test]
fn test_crs_empty() {
    let crs = CrsInfo::new();
    assert!(crs.organization.is_none());
    assert!(crs.organization_code.is_none());
    assert!(crs.wkt.is_none());
}

#[test]
fn test_crs_roundtrip_epsg_4326() {
    let header = Header::new(GeometryType::Point).with_crs(CrsInfo::from_epsg(4326));
    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    let point = Point::new(-122.4, 37.8);
    let feature = Feature::new(Geometry::Point(point));
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    let crs = reader.header().crs.as_ref().expect("Missing CRS");
    assert_eq!(crs.organization, Some("EPSG".to_string()));
    assert_eq!(crs.organization_code, Some(4326));
}

#[test]
fn test_crs_roundtrip_projected() {
    let header = Header::new(GeometryType::Point).with_crs(CrsInfo::from_epsg(32632)); // UTM zone 32N
    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    // UTM coordinates
    let point = Point::new(500000.0, 5000000.0);
    let feature = Feature::new(Geometry::Point(point));
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    let crs = reader.header().crs.as_ref().expect("Missing CRS");
    assert_eq!(crs.organization_code, Some(32632));
}

#[test]
fn test_no_crs_file() {
    let header = Header::new(GeometryType::Point);
    assert!(header.crs.is_none());

    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    let point = Point::new(0.0, 0.0);
    let feature = Feature::new(Geometry::Point(point));
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    assert!(reader.header().crs.is_none());
}

// =============================================================================
// SECTION 4: Attribute Type Tests
// =============================================================================

#[test]
fn test_column_type_byte() {
    let mut header = Header::new(GeometryType::Point);
    header.add_column(Column::new("byte_val", ColumnType::Byte));

    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    let mut feature = Feature::new(Geometry::Point(Point::new(0.0, 0.0)));
    feature.set_property("byte_val", PropertyValue::Integer(-128));
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    let mut features_iter = reader.features().expect("Failed to get iterator");
    let result = features_iter.next().expect("No feature found");
    let feature = result.expect("Failed to read feature");

    let val = feature
        .get_property("byte_val")
        .expect("Missing byte_val")
        .as_i64()
        .expect("Expected integer");
    assert_eq!(val, -128);
}

#[test]
fn test_column_type_ubyte() {
    let mut header = Header::new(GeometryType::Point);
    header.add_column(Column::new("ubyte_val", ColumnType::UByte));

    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    let mut feature = Feature::new(Geometry::Point(Point::new(0.0, 0.0)));
    feature.set_property("ubyte_val", PropertyValue::UInteger(255));
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    let mut features_iter = reader.features().expect("Failed to get iterator");
    let result = features_iter.next().expect("No feature found");
    let feature = result.expect("Failed to read feature");

    let val = feature
        .get_property("ubyte_val")
        .expect("Missing ubyte_val")
        .as_u64()
        .expect("Expected unsigned integer");
    assert_eq!(val, 255);
}

#[test]
fn test_column_type_bool() {
    let mut header = Header::new(GeometryType::Point);
    header.add_column(Column::new("bool_true", ColumnType::Bool));
    header.add_column(Column::new("bool_false", ColumnType::Bool));

    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    let mut feature = Feature::new(Geometry::Point(Point::new(0.0, 0.0)));
    feature.set_property("bool_true", PropertyValue::Bool(true));
    feature.set_property("bool_false", PropertyValue::Bool(false));
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    let mut features_iter = reader.features().expect("Failed to get iterator");
    let result = features_iter.next().expect("No feature found");
    let feature = result.expect("Failed to read feature");

    assert_eq!(
        feature.get_property("bool_true").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        feature.get_property("bool_false").and_then(|v| v.as_bool()),
        Some(false)
    );
}

#[test]
fn test_column_type_short() {
    let mut header = Header::new(GeometryType::Point);
    header.add_column(Column::new("short_val", ColumnType::Short));

    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    let mut feature = Feature::new(Geometry::Point(Point::new(0.0, 0.0)));
    feature.set_property("short_val", PropertyValue::Integer(-32768));
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    let mut features_iter = reader.features().expect("Failed to get iterator");
    let result = features_iter.next().expect("No feature found");
    let feature = result.expect("Failed to read feature");

    let val = feature
        .get_property("short_val")
        .expect("Missing short_val")
        .as_i64()
        .expect("Expected integer");
    assert_eq!(val, -32768);
}

#[test]
fn test_column_type_ushort() {
    let mut header = Header::new(GeometryType::Point);
    header.add_column(Column::new("ushort_val", ColumnType::UShort));

    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    let mut feature = Feature::new(Geometry::Point(Point::new(0.0, 0.0)));
    feature.set_property("ushort_val", PropertyValue::UInteger(65535));
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    let mut features_iter = reader.features().expect("Failed to get iterator");
    let result = features_iter.next().expect("No feature found");
    let feature = result.expect("Failed to read feature");

    let val = feature
        .get_property("ushort_val")
        .expect("Missing ushort_val")
        .as_u64()
        .expect("Expected unsigned integer");
    assert_eq!(val, 65535);
}

#[test]
fn test_column_type_int() {
    let mut header = Header::new(GeometryType::Point);
    header.add_column(Column::new("int_val", ColumnType::Int));

    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    let mut feature = Feature::new(Geometry::Point(Point::new(0.0, 0.0)));
    feature.set_property("int_val", PropertyValue::Integer(-2_147_483_648));
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    let mut features_iter = reader.features().expect("Failed to get iterator");
    let result = features_iter.next().expect("No feature found");
    let feature = result.expect("Failed to read feature");

    let val = feature
        .get_property("int_val")
        .expect("Missing int_val")
        .as_i64()
        .expect("Expected integer");
    assert_eq!(val, -2_147_483_648);
}

#[test]
fn test_column_type_uint() {
    let mut header = Header::new(GeometryType::Point);
    header.add_column(Column::new("uint_val", ColumnType::UInt));

    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    let mut feature = Feature::new(Geometry::Point(Point::new(0.0, 0.0)));
    feature.set_property("uint_val", PropertyValue::UInteger(4_294_967_295));
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    let mut features_iter = reader.features().expect("Failed to get iterator");
    let result = features_iter.next().expect("No feature found");
    let feature = result.expect("Failed to read feature");

    let val = feature
        .get_property("uint_val")
        .expect("Missing uint_val")
        .as_u64()
        .expect("Expected unsigned integer");
    assert_eq!(val, 4_294_967_295);
}

#[test]
fn test_column_type_long() {
    let mut header = Header::new(GeometryType::Point);
    header.add_column(Column::new("long_val", ColumnType::Long));

    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    let mut feature = Feature::new(Geometry::Point(Point::new(0.0, 0.0)));
    feature.set_property(
        "long_val",
        PropertyValue::Integer(-9_223_372_036_854_775_808),
    );
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    let mut features_iter = reader.features().expect("Failed to get iterator");
    let result = features_iter.next().expect("No feature found");
    let feature = result.expect("Failed to read feature");

    let val = feature
        .get_property("long_val")
        .expect("Missing long_val")
        .as_i64()
        .expect("Expected integer");
    assert_eq!(val, i64::MIN);
}

#[test]
fn test_column_type_ulong() {
    let mut header = Header::new(GeometryType::Point);
    header.add_column(Column::new("ulong_val", ColumnType::ULong));

    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    let mut feature = Feature::new(Geometry::Point(Point::new(0.0, 0.0)));
    feature.set_property("ulong_val", PropertyValue::UInteger(u64::MAX));
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    let mut features_iter = reader.features().expect("Failed to get iterator");
    let result = features_iter.next().expect("No feature found");
    let feature = result.expect("Failed to read feature");

    let val = feature
        .get_property("ulong_val")
        .expect("Missing ulong_val")
        .as_u64()
        .expect("Expected unsigned integer");
    assert_eq!(val, u64::MAX);
}

#[test]
fn test_column_type_float() {
    let mut header = Header::new(GeometryType::Point);
    header.add_column(Column::new("float_val", ColumnType::Float));

    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    let mut feature = Feature::new(Geometry::Point(Point::new(0.0, 0.0)));
    feature.set_property("float_val", PropertyValue::Float(std::f64::consts::PI));
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    let mut features_iter = reader.features().expect("Failed to get iterator");
    let result = features_iter.next().expect("No feature found");
    let feature = result.expect("Failed to read feature");

    let val = feature
        .get_property("float_val")
        .expect("Missing float_val")
        .as_f64()
        .expect("Expected float");
    // Float precision loss expected
    assert!((val - std::f64::consts::PI).abs() < 0.001);
}

#[test]
fn test_column_type_double() {
    let mut header = Header::new(GeometryType::Point);
    header.add_column(Column::new("double_val", ColumnType::Double));

    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    let mut feature = Feature::new(Geometry::Point(Point::new(0.0, 0.0)));
    feature.set_property("double_val", PropertyValue::Float(std::f64::consts::PI));
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    let mut features_iter = reader.features().expect("Failed to get iterator");
    let result = features_iter.next().expect("No feature found");
    let feature = result.expect("Failed to read feature");

    let val = feature
        .get_property("double_val")
        .expect("Missing double_val")
        .as_f64()
        .expect("Expected float");
    assert!((val - std::f64::consts::PI).abs() < 1e-15);
}

#[test]
fn test_column_type_string_unicode() {
    let mut header = Header::new(GeometryType::Point);
    header.add_column(Column::new("unicode_val", ColumnType::String));

    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    let mut feature = Feature::new(Geometry::Point(Point::new(0.0, 0.0)));
    feature.set_property(
        "unicode_val",
        PropertyValue::String("Hello, World! Emoji: 🌍🌏🌎".to_string()),
    );
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    let mut features_iter = reader.features().expect("Failed to get iterator");
    let result = features_iter.next().expect("No feature found");
    let feature = result.expect("Failed to read feature");

    if let Some(s) = feature
        .get_property("unicode_val")
        .and_then(|v| v.as_string())
    {
        assert_eq!(s, "Hello, World! Emoji: 🌍🌏🌎");
    } else {
        panic!("Expected string value");
    }
}

#[test]
fn test_column_type_json() {
    let mut header = Header::new(GeometryType::Point);
    header.add_column(Column::new("json_val", ColumnType::Json));

    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    let json_string = r#"{"key":"value","number":42,"array":[1,2,3]}"#;
    let mut feature = Feature::new(Geometry::Point(Point::new(0.0, 0.0)));
    feature.set_property("json_val", PropertyValue::String(json_string.to_string()));
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    let mut features_iter = reader.features().expect("Failed to get iterator");
    let result = features_iter.next().expect("No feature found");
    let feature = result.expect("Failed to read feature");

    if let Some(s) = feature.get_property("json_val").and_then(|v| v.as_string()) {
        assert_eq!(s, json_string);
    } else {
        panic!("Expected JSON string value");
    }
}

#[test]
fn test_column_type_datetime() {
    let mut header = Header::new(GeometryType::Point);
    header.add_column(Column::new("datetime_val", ColumnType::DateTime));

    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    let datetime_str = "2024-01-15T10:30:00Z";
    let mut feature = Feature::new(Geometry::Point(Point::new(0.0, 0.0)));
    feature.set_property(
        "datetime_val",
        PropertyValue::String(datetime_str.to_string()),
    );
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    let mut features_iter = reader.features().expect("Failed to get iterator");
    let result = features_iter.next().expect("No feature found");
    let feature = result.expect("Failed to read feature");

    if let Some(s) = feature
        .get_property("datetime_val")
        .and_then(|v| v.as_string())
    {
        assert_eq!(s, datetime_str);
    } else {
        panic!("Expected datetime string value");
    }
}

#[test]
fn test_null_property_values() {
    let mut header = Header::new(GeometryType::Point);
    header.add_column(Column::new("nullable", ColumnType::String));

    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    let mut feature = Feature::new(Geometry::Point(Point::new(0.0, 0.0)));
    feature.set_property("nullable", PropertyValue::Null);
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    let mut features_iter = reader.features().expect("Failed to get iterator");
    let result = features_iter.next().expect("No feature found");
    let feature = result.expect("Failed to read feature");

    let val = feature.get_property("nullable").expect("Missing property");
    assert!(val.is_null());
}

// =============================================================================
// SECTION 5: Spatial Index Tests
// =============================================================================

#[test]
fn test_spatial_index_creation() {
    let header = Header::new(GeometryType::Point).with_index(true);
    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    for i in 0..100 {
        let x = (i % 10) as f64;
        let y = (i / 10) as f64;
        let point = Point::new(x, y);
        let feature = Feature::new(Geometry::Point(point));
        writer.add_feature(&feature).expect("Failed to add feature");
    }

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    assert!(reader.header().has_index);
    assert!(reader.index().is_some());
}

#[test]
fn test_spatial_index_node_count() {
    let header = Header::new(GeometryType::Point).with_index(true);
    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    for i in 0..50 {
        let point = Point::new(i as f64, i as f64);
        let feature = Feature::new(Geometry::Point(point));
        writer.add_feature(&feature).expect("Failed to add feature");
    }

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    let index = reader.index().expect("Missing index");
    assert!(!index.nodes.is_empty());
}

#[test]
fn test_bounding_box_intersection() {
    let bbox1 = BoundingBox::new(0.0, 0.0, 10.0, 10.0);
    let bbox2 = BoundingBox::new(5.0, 5.0, 15.0, 15.0);
    let bbox3 = BoundingBox::new(20.0, 20.0, 30.0, 30.0);

    assert!(bbox1.intersects(&bbox2));
    assert!(bbox2.intersects(&bbox1));
    assert!(!bbox1.intersects(&bbox3));
    assert!(!bbox3.intersects(&bbox1));
}

#[test]
fn test_bounding_box_contains() {
    let outer = BoundingBox::new(0.0, 0.0, 100.0, 100.0);
    let inner = BoundingBox::new(25.0, 25.0, 75.0, 75.0);

    assert!(outer.contains(&inner));
    assert!(!inner.contains(&outer));
}

#[test]
fn test_bounding_box_expansion() {
    let mut bbox = BoundingBox::new(0.0, 0.0, 10.0, 10.0);
    bbox.expand(&BoundingBox::new(-5.0, -5.0, 15.0, 15.0));

    assert_eq!(bbox.min_x, -5.0);
    assert_eq!(bbox.min_y, -5.0);
    assert_eq!(bbox.max_x, 15.0);
    assert_eq!(bbox.max_y, 15.0);
}

#[test]
fn test_no_spatial_index() {
    let header = Header::new(GeometryType::Point).with_index(false);
    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    let point = Point::new(0.0, 0.0);
    let feature = Feature::new(Geometry::Point(point));
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    assert!(!reader.header().has_index);
    assert!(reader.index().is_none());
}

// =============================================================================
// SECTION 6: Large File Tests
// =============================================================================

#[test]
fn test_large_feature_count() {
    let header = Header::new(GeometryType::Point);
    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    const FEATURE_COUNT: usize = 1000;
    for i in 0..FEATURE_COUNT {
        let point = Point::new(i as f64, i as f64 * 2.0);
        let feature = Feature::new(Geometry::Point(point));
        writer.add_feature(&feature).expect("Failed to add feature");
    }

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    let mut count = 0;
    let mut features_iter = reader.features().expect("Failed to get iterator");
    while let Some(result) = features_iter.next() {
        result.expect("Failed to read feature");
        count += 1;
    }

    assert_eq!(count, FEATURE_COUNT);
}

#[test]
fn test_large_feature_count_with_index() {
    let header = Header::new(GeometryType::Point).with_index(true);
    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    const FEATURE_COUNT: usize = 500;
    for i in 0..FEATURE_COUNT {
        let x = (i % 50) as f64;
        let y = (i / 50) as f64;
        let point = Point::new(x, y);
        let feature = Feature::new(Geometry::Point(point));
        writer.add_feature(&feature).expect("Failed to add feature");
    }

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    assert!(reader.header().has_index);
    assert!(reader.index().is_some());

    let mut count = 0;
    let mut features_iter = reader.features().expect("Failed to get iterator");
    while let Some(result) = features_iter.next() {
        result.expect("Failed to read feature");
        count += 1;
    }

    assert_eq!(count, FEATURE_COUNT);
}

#[test]
fn test_large_polygon_many_vertices() {
    let header = Header::new(GeometryType::Polygon);
    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    // Create a polygon with 1000 vertices (circle approximation)
    const VERTEX_COUNT: usize = 1000;
    let mut coords = Vec::with_capacity(VERTEX_COUNT + 1);
    for i in 0..VERTEX_COUNT {
        let angle = 2.0 * std::f64::consts::PI * (i as f64) / (VERTEX_COUNT as f64);
        let x = 100.0 * angle.cos();
        let y = 100.0 * angle.sin();
        coords.push(Coordinate::new_2d(x, y));
    }
    // Close the ring
    coords.push(coords[0]);

    let exterior = LineString::new(coords).expect("Failed to create exterior");
    let polygon = Polygon::new(exterior, vec![]).expect("Failed to create polygon");
    let feature = Feature::new(Geometry::Polygon(polygon));
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    let mut features_iter = reader.features().expect("Failed to get iterator");
    let result = features_iter.next().expect("No feature found");
    let feature = result.expect("Failed to read feature");

    if let Some(Geometry::Polygon(poly)) = &feature.geometry {
        assert_eq!(poly.exterior.len(), VERTEX_COUNT + 1);
    } else {
        panic!("Expected Polygon geometry");
    }
}

#[test]
fn test_many_columns() {
    let mut header = Header::new(GeometryType::Point);

    const COLUMN_COUNT: usize = 50;
    for i in 0..COLUMN_COUNT {
        header.add_column(Column::new(format!("col_{i}"), ColumnType::Int));
    }

    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    let mut feature = Feature::new(Geometry::Point(Point::new(0.0, 0.0)));
    for i in 0..COLUMN_COUNT {
        feature.set_property(format!("col_{i}"), PropertyValue::Integer(i as i64));
    }
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    assert_eq!(reader.header().columns.len(), COLUMN_COUNT);

    let mut features_iter = reader.features().expect("Failed to get iterator");
    let result = features_iter.next().expect("No feature found");
    let feature = result.expect("Failed to read feature");

    for i in 0..COLUMN_COUNT {
        let val = feature
            .get_property(&format!("col_{i}"))
            .expect("Missing column")
            .as_i64()
            .expect("Expected integer");
        assert_eq!(val, i as i64);
    }
}

// =============================================================================
// SECTION 7: File I/O Tests
// =============================================================================

#[test]
fn test_file_write_and_read() {
    let path = temp_file_path("file_io");

    // Write to file
    {
        let file = File::create(&path).expect("Failed to create file");
        let header = Header::new(GeometryType::Point);
        let mut writer = FlatGeobufWriter::new(file, header).expect("Failed to create writer");

        for i in 0..10 {
            let point = Point::new(i as f64, i as f64);
            let feature = Feature::new(Geometry::Point(point));
            writer.add_feature(&feature).expect("Failed to add feature");
        }

        writer.finish().expect("Failed to finish");
    }

    // Read from file
    {
        let file = File::open(&path).expect("Failed to open file");
        let mut reader = FlatGeobufReader::new(file).expect("Failed to create reader");

        let mut count = 0;
        let mut features_iter = reader.features().expect("Failed to get iterator");
        while let Some(result) = features_iter.next() {
            result.expect("Failed to read feature");
            count += 1;
        }

        assert_eq!(count, 10);
    }

    cleanup_temp_file(&path);
}

#[test]
fn test_file_seek_operations() {
    let path = temp_file_path("file_seek");

    // Write features
    {
        let file = File::create(&path).expect("Failed to create file");
        let header = Header::new(GeometryType::Point);
        let mut writer = FlatGeobufWriter::new(file, header).expect("Failed to create writer");

        for i in 0..5 {
            let point = Point::new(i as f64, i as f64 * 10.0);
            let feature = Feature::new(Geometry::Point(point));
            writer.add_feature(&feature).expect("Failed to add feature");
        }

        writer.finish().expect("Failed to finish");
    }

    // Read and seek
    {
        let file = File::open(&path).expect("Failed to open file");
        let mut reader = FlatGeobufReader::new(file).expect("Failed to create reader");

        // Read first feature
        let feature1 = reader
            .read_feature()
            .expect("Failed to read")
            .expect("No feature");

        // Read second feature
        let feature2 = reader
            .read_feature()
            .expect("Failed to read")
            .expect("No feature");

        if let Some(Geometry::Point(p1)) = &feature1.geometry {
            if let Some(Geometry::Point(p2)) = &feature2.geometry {
                assert_eq!(p1.coord.x, 0.0);
                assert_eq!(p2.coord.x, 1.0);
            }
        }
    }

    cleanup_temp_file(&path);
}

// =============================================================================
// SECTION 8: Edge Case Tests
// =============================================================================

#[test]
fn test_empty_feature_collection() {
    let header = Header::new(GeometryType::Point);
    let cursor = Cursor::new(Vec::new());
    let writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    let mut features_iter = reader.features().expect("Failed to get iterator");
    assert!(features_iter.next().is_none());
}

#[test]
fn test_single_feature() {
    let header = Header::new(GeometryType::Point);
    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    let point = Point::new(42.0, 43.0);
    let feature = Feature::new(Geometry::Point(point));
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    let mut count = 0;
    let mut features_iter = reader.features().expect("Failed to get iterator");
    while let Some(result) = features_iter.next() {
        let f = result.expect("Failed to read feature");
        if let Some(Geometry::Point(p)) = &f.geometry {
            assert_eq!(p.coord.x, 42.0);
            assert_eq!(p.coord.y, 43.0);
        }
        count += 1;
    }

    assert_eq!(count, 1);
}

#[test]
fn test_extreme_coordinates() {
    let header = Header::new(GeometryType::Point);
    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    // Very large coordinates
    let point1 = Point::new(1e15, -1e15);
    let feature1 = Feature::new(Geometry::Point(point1));
    writer
        .add_feature(&feature1)
        .expect("Failed to add feature 1");

    // Very small coordinates
    let point2 = Point::new(1e-15, -1e-15);
    let feature2 = Feature::new(Geometry::Point(point2));
    writer
        .add_feature(&feature2)
        .expect("Failed to add feature 2");

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    let mut features_iter = reader.features().expect("Failed to get iterator");

    let f1 = features_iter
        .next()
        .expect("No feature 1")
        .expect("Failed to read");
    if let Some(Geometry::Point(p)) = &f1.geometry {
        assert_eq!(p.coord.x, 1e15);
        assert_eq!(p.coord.y, -1e15);
    }

    let f2 = features_iter
        .next()
        .expect("No feature 2")
        .expect("Failed to read");
    if let Some(Geometry::Point(p)) = &f2.geometry {
        assert!((p.coord.x - 1e-15).abs() < 1e-30);
        assert!((p.coord.y - (-1e-15)).abs() < 1e-30);
    }
}

#[test]
fn test_empty_string_property() {
    let mut header = Header::new(GeometryType::Point);
    header.add_column(Column::new("empty_str", ColumnType::String));

    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    let mut feature = Feature::new(Geometry::Point(Point::new(0.0, 0.0)));
    feature.set_property("empty_str", PropertyValue::String(String::new()));
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    let mut features_iter = reader.features().expect("Failed to get iterator");
    let result = features_iter.next().expect("No feature found");
    let feature = result.expect("Failed to read feature");

    if let Some(s) = feature
        .get_property("empty_str")
        .and_then(|v| v.as_string())
    {
        assert!(s.is_empty());
    } else {
        panic!("Expected empty string value");
    }
}

#[test]
fn test_long_string_property() {
    let mut header = Header::new(GeometryType::Point);
    header.add_column(Column::new("long_str", ColumnType::String));

    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    let long_string = "A".repeat(10_000);
    let mut feature = Feature::new(Geometry::Point(Point::new(0.0, 0.0)));
    feature.set_property("long_str", PropertyValue::String(long_string.clone()));
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    let mut features_iter = reader.features().expect("Failed to get iterator");
    let result = features_iter.next().expect("No feature found");
    let feature = result.expect("Failed to read feature");

    if let Some(s) = feature.get_property("long_str").and_then(|v| v.as_string()) {
        assert_eq!(s.len(), 10_000);
        assert_eq!(s, &long_string);
    } else {
        panic!("Expected long string value");
    }
}

#[test]
fn test_special_characters_in_column_name() {
    let mut header = Header::new(GeometryType::Point);
    header.add_column(Column::new("column with spaces", ColumnType::Int));
    header.add_column(Column::new("column_with_underscore", ColumnType::Int));
    header.add_column(Column::new("column-with-dash", ColumnType::Int));

    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    let mut feature = Feature::new(Geometry::Point(Point::new(0.0, 0.0)));
    feature.set_property("column with spaces", PropertyValue::Integer(1));
    feature.set_property("column_with_underscore", PropertyValue::Integer(2));
    feature.set_property("column-with-dash", PropertyValue::Integer(3));
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    assert_eq!(reader.header().columns[0].name, "column with spaces");
    assert_eq!(reader.header().columns[1].name, "column_with_underscore");
    assert_eq!(reader.header().columns[2].name, "column-with-dash");
}

#[test]
fn test_polygon_with_multiple_holes() {
    let header = Header::new(GeometryType::Polygon);
    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    let exterior = LineString::new(vec![
        Coordinate::new_2d(0.0, 0.0),
        Coordinate::new_2d(100.0, 0.0),
        Coordinate::new_2d(100.0, 100.0),
        Coordinate::new_2d(0.0, 100.0),
        Coordinate::new_2d(0.0, 0.0),
    ])
    .expect("Failed to create exterior");

    let hole1 = LineString::new(vec![
        Coordinate::new_2d(10.0, 10.0),
        Coordinate::new_2d(20.0, 10.0),
        Coordinate::new_2d(20.0, 20.0),
        Coordinate::new_2d(10.0, 20.0),
        Coordinate::new_2d(10.0, 10.0),
    ])
    .expect("Failed to create hole 1");

    let hole2 = LineString::new(vec![
        Coordinate::new_2d(30.0, 30.0),
        Coordinate::new_2d(40.0, 30.0),
        Coordinate::new_2d(40.0, 40.0),
        Coordinate::new_2d(30.0, 40.0),
        Coordinate::new_2d(30.0, 30.0),
    ])
    .expect("Failed to create hole 2");

    let hole3 = LineString::new(vec![
        Coordinate::new_2d(50.0, 50.0),
        Coordinate::new_2d(60.0, 50.0),
        Coordinate::new_2d(60.0, 60.0),
        Coordinate::new_2d(50.0, 60.0),
        Coordinate::new_2d(50.0, 50.0),
    ])
    .expect("Failed to create hole 3");

    let polygon =
        Polygon::new(exterior, vec![hole1, hole2, hole3]).expect("Failed to create polygon");
    let feature = Feature::new(Geometry::Polygon(polygon));
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let mut reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    let mut features_iter = reader.features().expect("Failed to get iterator");
    let result = features_iter.next().expect("No feature found");
    let feature = result.expect("Failed to read feature");

    if let Some(Geometry::Polygon(poly)) = &feature.geometry {
        assert_eq!(poly.interiors.len(), 3);
    } else {
        panic!("Expected Polygon geometry");
    }
}

#[test]
fn test_geometry_type_names() {
    assert_eq!(GeometryType::Point.to_name(), "Point");
    assert_eq!(GeometryType::LineString.to_name(), "LineString");
    assert_eq!(GeometryType::Polygon.to_name(), "Polygon");
    assert_eq!(GeometryType::MultiPoint.to_name(), "MultiPoint");
    assert_eq!(GeometryType::MultiLineString.to_name(), "MultiLineString");
    assert_eq!(GeometryType::MultiPolygon.to_name(), "MultiPolygon");
    assert_eq!(
        GeometryType::GeometryCollection.to_name(),
        "GeometryCollection"
    );
    assert_eq!(GeometryType::Unknown.to_name(), "Unknown");
}

#[test]
fn test_column_type_names() {
    assert_eq!(ColumnType::Byte.name(), "Byte");
    assert_eq!(ColumnType::UByte.name(), "UByte");
    assert_eq!(ColumnType::Bool.name(), "Bool");
    assert_eq!(ColumnType::Short.name(), "Short");
    assert_eq!(ColumnType::UShort.name(), "UShort");
    assert_eq!(ColumnType::Int.name(), "Int");
    assert_eq!(ColumnType::UInt.name(), "UInt");
    assert_eq!(ColumnType::Long.name(), "Long");
    assert_eq!(ColumnType::ULong.name(), "ULong");
    assert_eq!(ColumnType::Float.name(), "Float");
    assert_eq!(ColumnType::Double.name(), "Double");
    assert_eq!(ColumnType::String.name(), "String");
    assert_eq!(ColumnType::Json.name(), "Json");
    assert_eq!(ColumnType::DateTime.name(), "DateTime");
    assert_eq!(ColumnType::Binary.name(), "Binary");
}

#[test]
fn test_extent_calculation() {
    let header = Header::new(GeometryType::Point).with_index(true);
    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriter::new(cursor, header).expect("Failed to create writer");

    // Add points at various locations
    writer
        .add_feature(&Feature::new(Geometry::Point(Point::new(-10.0, -20.0))))
        .expect("Failed to add feature");
    writer
        .add_feature(&Feature::new(Geometry::Point(Point::new(30.0, 40.0))))
        .expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    if let Some(extent) = reader.header().extent {
        assert_eq!(extent[0], -10.0); // min_x
        assert_eq!(extent[1], -20.0); // min_y
        assert_eq!(extent[2], 30.0); // max_x
        assert_eq!(extent[3], 40.0); // max_y
    } else {
        panic!("Expected extent to be set");
    }
}

#[test]
fn test_writer_builder_full_options() {
    let cursor = Cursor::new(Vec::new());
    let mut writer = FlatGeobufWriterBuilder::new(GeometryType::Point)
        .with_z()
        .with_m()
        .with_index()
        .with_crs(CrsInfo::from_epsg(4326))
        .with_column(Column::new("name", ColumnType::String))
        .with_column(Column::new("value", ColumnType::Double))
        .build(cursor)
        .expect("Failed to build writer");

    let mut feature = Feature::new(Geometry::Point(Point::new_3d(1.0, 2.0, 3.0)));
    feature.set_property("name", PropertyValue::String("test".to_string()));
    feature.set_property("value", PropertyValue::Float(42.5));
    writer.add_feature(&feature).expect("Failed to add feature");

    let cursor = writer.finish().expect("Failed to finish");
    let cursor = Cursor::new(cursor.into_inner());
    let reader = FlatGeobufReader::new(cursor).expect("Failed to create reader");

    assert!(reader.header().has_z);
    assert!(reader.header().has_m);
    assert!(reader.header().has_index);
    assert!(reader.header().crs.is_some());
    assert_eq!(reader.header().columns.len(), 2);
}

#[test]
fn test_column_metadata() {
    let column = Column::new("test_col", ColumnType::String)
        .with_title("Test Column")
        .with_description("A test column description")
        .with_nullable(false)
        .with_unique(true)
        .with_primary_key(false);

    assert_eq!(column.name, "test_col");
    assert_eq!(column.title, Some("Test Column".to_string()));
    assert_eq!(
        column.description,
        Some("A test column description".to_string())
    );
    assert!(!column.nullable);
    assert!(column.unique);
    assert!(!column.primary_key);
}

#[test]
fn test_geometry_type_parsing() {
    // Valid geometry types
    assert_eq!(GeometryType::from_u8(0).ok(), Some(GeometryType::Unknown));
    assert_eq!(GeometryType::from_u8(1).ok(), Some(GeometryType::Point));
    assert_eq!(
        GeometryType::from_u8(2).ok(),
        Some(GeometryType::LineString)
    );
    assert_eq!(GeometryType::from_u8(3).ok(), Some(GeometryType::Polygon));
    assert_eq!(
        GeometryType::from_u8(4).ok(),
        Some(GeometryType::MultiPoint)
    );
    assert_eq!(
        GeometryType::from_u8(5).ok(),
        Some(GeometryType::MultiLineString)
    );
    assert_eq!(
        GeometryType::from_u8(6).ok(),
        Some(GeometryType::MultiPolygon)
    );
    assert_eq!(
        GeometryType::from_u8(7).ok(),
        Some(GeometryType::GeometryCollection)
    );

    // Invalid geometry type
    let result = GeometryType::from_u8(99);
    assert!(result.is_err());
}

#[test]
fn test_column_type_parsing() {
    // Valid column types
    assert_eq!(ColumnType::from_u8(0).ok(), Some(ColumnType::Byte));
    assert_eq!(ColumnType::from_u8(1).ok(), Some(ColumnType::UByte));
    assert_eq!(ColumnType::from_u8(2).ok(), Some(ColumnType::Bool));
    assert_eq!(ColumnType::from_u8(11).ok(), Some(ColumnType::String));
    assert_eq!(ColumnType::from_u8(14).ok(), Some(ColumnType::Binary));

    // Invalid column type
    let result = ColumnType::from_u8(99);
    assert!(result.is_err());
}
