//! Integration tests for GeoPackage vector feature table types.

use oxigdal_gpkg::{
    FeatureRow, FeatureTable, FieldDefinition, FieldType, FieldValue, GpkgBinaryParser, GpkgError,
    GpkgGeometry, SrsInfo, TileMatrix,
};
use std::collections::HashMap;

// ─────────────────────────────────────────────────────────────────────────────
// FieldType tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn field_type_from_sql_integer() {
    assert_eq!(FieldType::from_sql_type("INTEGER"), FieldType::Integer);
}

#[test]
fn field_type_from_sql_integer_lowercase() {
    assert_eq!(FieldType::from_sql_type("integer"), FieldType::Integer);
}

#[test]
fn field_type_from_sql_int() {
    assert_eq!(FieldType::from_sql_type("INT"), FieldType::Integer);
}

#[test]
fn field_type_from_sql_real() {
    assert_eq!(FieldType::from_sql_type("REAL"), FieldType::Real);
}

#[test]
fn field_type_from_sql_real_lowercase() {
    assert_eq!(FieldType::from_sql_type("real"), FieldType::Real);
}

#[test]
fn field_type_from_sql_float() {
    assert_eq!(FieldType::from_sql_type("FLOAT"), FieldType::Real);
}

#[test]
fn field_type_from_sql_text() {
    assert_eq!(FieldType::from_sql_type("TEXT"), FieldType::Text);
}

#[test]
fn field_type_from_sql_text_lowercase() {
    assert_eq!(FieldType::from_sql_type("text"), FieldType::Text);
}

#[test]
fn field_type_from_sql_blob() {
    assert_eq!(FieldType::from_sql_type("BLOB"), FieldType::Blob);
}

#[test]
fn field_type_from_sql_blob_lowercase() {
    assert_eq!(FieldType::from_sql_type("blob"), FieldType::Blob);
}

#[test]
fn field_type_from_sql_boolean() {
    assert_eq!(FieldType::from_sql_type("BOOLEAN"), FieldType::Boolean);
}

#[test]
fn field_type_from_sql_bool() {
    assert_eq!(FieldType::from_sql_type("BOOL"), FieldType::Boolean);
}

#[test]
fn field_type_from_sql_date() {
    assert_eq!(FieldType::from_sql_type("DATE"), FieldType::Date);
}

#[test]
fn field_type_from_sql_datetime() {
    assert_eq!(FieldType::from_sql_type("DATETIME"), FieldType::DateTime);
}

#[test]
fn field_type_from_sql_timestamp() {
    assert_eq!(FieldType::from_sql_type("TIMESTAMP"), FieldType::DateTime);
}

#[test]
fn field_type_from_sql_unknown_falls_back_to_text() {
    assert_eq!(FieldType::from_sql_type("FOOBAR"), FieldType::Text);
}

#[test]
fn field_type_as_str_roundtrip() {
    let types = [
        FieldType::Integer,
        FieldType::Real,
        FieldType::Text,
        FieldType::Blob,
        FieldType::Boolean,
        FieldType::Date,
        FieldType::DateTime,
        FieldType::Null,
    ];
    for ft in &types {
        let s = ft.as_str();
        assert!(!s.is_empty(), "as_str should not be empty for {ft:?}");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// FieldValue tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn field_value_as_integer_correct() {
    assert_eq!(FieldValue::Integer(42).as_integer(), Some(42));
}

#[test]
fn field_value_as_integer_wrong_variant() {
    assert_eq!(FieldValue::Text("hi".into()).as_integer(), None);
}

#[test]
fn field_value_as_real_correct() {
    let v = FieldValue::Real(3.125);
    let got = v
        .as_real()
        .expect("as_real should return Some for Real variant");
    assert!((got - 3.125).abs() < 1e-10);
}

#[test]
fn field_value_as_real_wrong_variant() {
    assert_eq!(FieldValue::Integer(1).as_real(), None);
}

#[test]
fn field_value_as_text_correct() {
    assert_eq!(FieldValue::Text("hello".into()).as_text(), Some("hello"));
}

#[test]
fn field_value_as_text_wrong_variant() {
    assert_eq!(FieldValue::Integer(1).as_text(), None);
}

#[test]
fn field_value_as_bool_correct() {
    assert_eq!(FieldValue::Boolean(true).as_bool(), Some(true));
    assert_eq!(FieldValue::Boolean(false).as_bool(), Some(false));
}

#[test]
fn field_value_is_null_true() {
    assert!(FieldValue::Null.is_null());
}

#[test]
fn field_value_is_null_false() {
    assert!(!FieldValue::Integer(0).is_null());
}

#[test]
fn field_value_field_type_integer() {
    assert_eq!(FieldValue::Integer(0).field_type(), FieldType::Integer);
}

#[test]
fn field_value_field_type_real() {
    assert_eq!(FieldValue::Real(0.0).field_type(), FieldType::Real);
}

#[test]
fn field_value_field_type_text() {
    assert_eq!(FieldValue::Text("".into()).field_type(), FieldType::Text);
}

#[test]
fn field_value_field_type_blob() {
    assert_eq!(FieldValue::Blob(vec![]).field_type(), FieldType::Blob);
}

#[test]
fn field_value_field_type_boolean() {
    assert_eq!(FieldValue::Boolean(false).field_type(), FieldType::Boolean);
}

#[test]
fn field_value_field_type_null() {
    assert_eq!(FieldValue::Null.field_type(), FieldType::Null);
}

// ─────────────────────────────────────────────────────────────────────────────
// WKB Point parsing
// ─────────────────────────────────────────────────────────────────────────────

/// Hand-crafted LE WKB: byte_order=1, type=1 (Point), x=2.0, y=4.0
fn wkb_point_2_4() -> Vec<u8> {
    // 0x4000000000000000 in little-endian = 2.0
    // 0x4010000000000000 in little-endian = 4.0
    let mut bytes = vec![
        0x01u8, // byte order: LE
        0x01, 0x00, 0x00, 0x00, // wkb_type: 1 (Point)
    ];
    bytes.extend_from_slice(&2.0f64.to_le_bytes()); // x = 2.0
    bytes.extend_from_slice(&4.0f64.to_le_bytes()); // y = 4.0
    bytes
}

#[test]
fn parse_wkb_point_le() {
    let wkb = wkb_point_2_4();
    let geom = GpkgBinaryParser::parse_wkb(&wkb).expect("parse should succeed");
    match geom {
        GpkgGeometry::Point { x, y } => {
            assert!((x - 2.0).abs() < 1e-15, "x should be 2.0, got {x}");
            assert!((y - 4.0).abs() < 1e-15, "y should be 4.0, got {y}");
        }
        other => unreachable!("Expected Point, got {:?}", other),
    }
}

#[test]
fn parse_wkb_point_exact_bytes() {
    // The bytes from the task description: Point(2.0, 4.0)
    // 01 01000000 0000000000000040 0000000000001040
    let bytes = vec![
        0x01u8, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x40, // 2.0 LE
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x40, // 4.0 LE
    ];
    let geom = GpkgBinaryParser::parse_wkb(&bytes).expect("parse should succeed");
    match geom {
        GpkgGeometry::Point { x, y } => {
            assert!((x - 2.0).abs() < 1e-15);
            assert!((y - 4.0).abs() < 1e-15);
        }
        other => unreachable!("Expected Point, got {:?}", other),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// WKB LineString parsing
// ─────────────────────────────────────────────────────────────────────────────

fn wkb_linestring_2pts() -> Vec<u8> {
    let mut bytes = vec![
        0x01u8, // LE
        0x02, 0x00, 0x00, 0x00, // type: 2 (LineString)
        0x02, 0x00, 0x00, 0x00, // num_points: 2
    ];
    bytes.extend_from_slice(&0.0f64.to_le_bytes()); // x0
    bytes.extend_from_slice(&0.0f64.to_le_bytes()); // y0
    bytes.extend_from_slice(&1.0f64.to_le_bytes()); // x1
    bytes.extend_from_slice(&1.0f64.to_le_bytes()); // y1
    bytes
}

#[test]
fn parse_wkb_linestring_two_points() {
    let wkb = wkb_linestring_2pts();
    let geom = GpkgBinaryParser::parse_wkb(&wkb).expect("parse linestring");
    match geom {
        GpkgGeometry::LineString { coords } => {
            assert_eq!(coords.len(), 2);
            assert!((coords[0].0).abs() < 1e-15);
            assert!((coords[1].0 - 1.0).abs() < 1e-15);
        }
        other => unreachable!("Expected LineString, got {:?}", other),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// WKB Polygon parsing
// ─────────────────────────────────────────────────────────────────────────────

fn wkb_polygon_square() -> Vec<u8> {
    // A 1×1 square: (0,0),(1,0),(1,1),(0,1),(0,0)
    let ring: &[(f64, f64)] = &[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (0.0, 0.0)];
    let mut bytes = vec![
        0x01u8, // LE
        0x03, 0x00, 0x00, 0x00, // type: 3 (Polygon)
        0x01, 0x00, 0x00, 0x00, // num_rings: 1
        0x05, 0x00, 0x00, 0x00, // num_points in ring: 5
    ];
    for (x, y) in ring {
        bytes.extend_from_slice(&x.to_le_bytes());
        bytes.extend_from_slice(&y.to_le_bytes());
    }
    bytes
}

#[test]
fn parse_wkb_polygon_exterior_ring() {
    let wkb = wkb_polygon_square();
    let geom = GpkgBinaryParser::parse_wkb(&wkb).expect("parse polygon");
    match geom {
        GpkgGeometry::Polygon { rings } => {
            assert_eq!(rings.len(), 1);
            assert_eq!(rings[0].len(), 5);
            // first == last (closed ring)
            assert!((rings[0][0].0 - rings[0][4].0).abs() < 1e-15);
            assert!((rings[0][0].1 - rings[0][4].1).abs() < 1e-15);
        }
        other => unreachable!("Expected Polygon, got {:?}", other),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// WKB round-trip tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn wkb_roundtrip_point() {
    let original = GpkgGeometry::Point {
        x: 12.345,
        y: -67.89,
    };
    let encoded = GpkgBinaryParser::to_wkb(&original);
    let decoded = GpkgBinaryParser::parse_wkb(&encoded).expect("decode point");
    assert_eq!(original, decoded);
}

#[test]
fn wkb_roundtrip_linestring() {
    let original = GpkgGeometry::LineString {
        coords: vec![(0.0, 0.0), (1.0, 2.0), (3.0, 4.0)],
    };
    let encoded = GpkgBinaryParser::to_wkb(&original);
    let decoded = GpkgBinaryParser::parse_wkb(&encoded).expect("decode linestring");
    assert_eq!(original, decoded);
}

#[test]
fn wkb_roundtrip_polygon() {
    let original = GpkgGeometry::Polygon {
        rings: vec![vec![
            (0.0, 0.0),
            (1.0, 0.0),
            (1.0, 1.0),
            (0.0, 1.0),
            (0.0, 0.0),
        ]],
    };
    let encoded = GpkgBinaryParser::to_wkb(&original);
    let decoded = GpkgBinaryParser::parse_wkb(&encoded).expect("decode polygon");
    assert_eq!(original, decoded);
}

// ─────────────────────────────────────────────────────────────────────────────
// GPB format tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn to_gpb_starts_with_magic_bytes() {
    let geom = GpkgGeometry::Point { x: 0.0, y: 0.0 };
    let gpb = GpkgBinaryParser::to_gpb(&geom, 4326);
    assert_eq!(gpb[0], 0x47, "first byte should be 'G' (0x47)");
    assert_eq!(gpb[1], 0x50, "second byte should be 'P' (0x50)");
}

#[test]
fn to_gpb_parse_roundtrip_point() {
    let original = GpkgGeometry::Point {
        x: 13.4050,
        y: 52.5200,
    };
    let gpb = GpkgBinaryParser::to_gpb(&original, 4326);
    let decoded = GpkgBinaryParser::parse(&gpb).expect("parse GPB");
    assert_eq!(original, decoded);
}

#[test]
fn to_gpb_parse_roundtrip_linestring() {
    let original = GpkgGeometry::LineString {
        coords: vec![(10.0, 50.0), (11.0, 51.0)],
    };
    let gpb = GpkgBinaryParser::to_gpb(&original, 4326);
    let decoded = GpkgBinaryParser::parse(&gpb).expect("parse GPB linestring");
    assert_eq!(original, decoded);
}

#[test]
fn parse_gpb_valid_header() {
    let geom = GpkgGeometry::Polygon {
        rings: vec![vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 0.0)]],
    };
    let gpb = GpkgBinaryParser::to_gpb(&geom, 4326);
    let decoded = GpkgBinaryParser::parse(&gpb).expect("parse GPB polygon");
    assert_eq!(geom, decoded);
}

#[test]
fn parse_gpb_empty_geometry_flag() {
    let geom = GpkgGeometry::Empty;
    let gpb = GpkgBinaryParser::to_gpb(&geom, 4326);
    let decoded = GpkgBinaryParser::parse(&gpb).expect("parse GPB empty");
    assert_eq!(decoded, GpkgGeometry::Empty);
}

#[test]
fn parse_gpb_invalid_magic_returns_error() {
    let bad = vec![0x00u8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    let result = GpkgBinaryParser::parse(&bad);
    assert!(result.is_err());
    match result {
        Err(GpkgError::InvalidGeometryMagic) => {}
        Err(e) => unreachable!("Expected InvalidGeometryMagic, got {:?}", e),
        Ok(_) => unreachable!("Expected error but got Ok"),
    }
}

#[test]
fn parse_gpb_too_short_returns_error() {
    let short = vec![0x47u8, 0x50]; // only 2 bytes
    let result = GpkgBinaryParser::parse(&short);
    assert!(result.is_err());
}

// ─────────────────────────────────────────────────────────────────────────────
// GpkgGeometry::geometry_type
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn geometry_type_point() {
    assert_eq!(
        GpkgGeometry::Point { x: 0.0, y: 0.0 }.geometry_type(),
        "Point"
    );
}

#[test]
fn geometry_type_linestring() {
    assert_eq!(
        GpkgGeometry::LineString { coords: vec![] }.geometry_type(),
        "LineString"
    );
}

#[test]
fn geometry_type_polygon() {
    assert_eq!(
        GpkgGeometry::Polygon { rings: vec![] }.geometry_type(),
        "Polygon"
    );
}

#[test]
fn geometry_type_multipoint() {
    assert_eq!(
        GpkgGeometry::MultiPoint { points: vec![] }.geometry_type(),
        "MultiPoint"
    );
}

#[test]
fn geometry_type_multilinestring() {
    assert_eq!(
        GpkgGeometry::MultiLineString { lines: vec![] }.geometry_type(),
        "MultiLineString"
    );
}

#[test]
fn geometry_type_multipolygon() {
    assert_eq!(
        GpkgGeometry::MultiPolygon { polygons: vec![] }.geometry_type(),
        "MultiPolygon"
    );
}

#[test]
fn geometry_type_geometrycollection() {
    assert_eq!(
        GpkgGeometry::GeometryCollection(vec![]).geometry_type(),
        "GeometryCollection"
    );
}

#[test]
fn geometry_type_empty() {
    assert_eq!(GpkgGeometry::Empty.geometry_type(), "Empty");
}

// ─────────────────────────────────────────────────────────────────────────────
// GpkgGeometry::bbox
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn bbox_point() {
    let bbox = GpkgGeometry::Point { x: 5.0, y: 10.0 }.bbox();
    assert_eq!(bbox, Some((5.0, 10.0, 5.0, 10.0)));
}

#[test]
fn bbox_linestring() {
    let geom = GpkgGeometry::LineString {
        coords: vec![(1.0, 2.0), (3.0, 4.0), (0.0, 5.0)],
    };
    let bbox = geom.bbox();
    assert_eq!(bbox, Some((0.0, 2.0, 3.0, 5.0)));
}

#[test]
fn bbox_polygon() {
    let geom = GpkgGeometry::Polygon {
        rings: vec![vec![(0.0, 0.0), (2.0, 0.0), (2.0, 3.0), (0.0, 0.0)]],
    };
    let bbox = geom.bbox();
    assert_eq!(bbox, Some((0.0, 0.0, 2.0, 3.0)));
}

#[test]
fn bbox_empty_is_none() {
    assert_eq!(GpkgGeometry::Empty.bbox(), None);
}

#[test]
fn bbox_empty_linestring_is_none() {
    let geom = GpkgGeometry::LineString { coords: vec![] };
    assert_eq!(geom.bbox(), None);
}

// ─────────────────────────────────────────────────────────────────────────────
// FeatureRow tests
// ─────────────────────────────────────────────────────────────────────────────

fn make_feature_row(fid: i64) -> FeatureRow {
    let mut fields = HashMap::new();
    fields.insert("name".into(), FieldValue::Text("Test".into()));
    fields.insert("count".into(), FieldValue::Integer(99));
    FeatureRow {
        fid,
        geometry: Some(GpkgGeometry::Point { x: 1.0, y: 2.0 }),
        fields,
    }
}

#[test]
fn feature_row_get_field_found() {
    let row = make_feature_row(1);
    assert!(row.get_field("name").is_some());
}

#[test]
fn feature_row_get_field_not_found() {
    let row = make_feature_row(1);
    assert!(row.get_field("nonexistent").is_none());
}

#[test]
fn feature_row_get_integer() {
    let row = make_feature_row(1);
    assert_eq!(row.get_integer("count"), Some(99));
}

#[test]
fn feature_row_get_text() {
    let row = make_feature_row(1);
    assert_eq!(row.get_text("name"), Some("Test"));
}

#[test]
fn feature_row_get_real_absent() {
    let row = make_feature_row(1);
    assert_eq!(row.get_real("missing"), None);
}

// ─────────────────────────────────────────────────────────────────────────────
// FeatureTable tests
// ─────────────────────────────────────────────────────────────────────────────

fn make_table_with_features() -> FeatureTable {
    let mut table = FeatureTable::new("parcels", "geom");
    table.add_feature(FeatureRow {
        fid: 1,
        geometry: Some(GpkgGeometry::Point { x: 0.0, y: 0.0 }),
        fields: {
            let mut m = HashMap::new();
            m.insert("zone".into(), FieldValue::Text("A".into()));
            m
        },
    });
    table.add_feature(FeatureRow {
        fid: 2,
        geometry: Some(GpkgGeometry::Point { x: 10.0, y: 10.0 }),
        fields: {
            let mut m = HashMap::new();
            m.insert("zone".into(), FieldValue::Text("B".into()));
            m
        },
    });
    table.add_feature(FeatureRow {
        fid: 3,
        geometry: Some(GpkgGeometry::Point { x: 20.0, y: 20.0 }),
        fields: {
            let mut m = HashMap::new();
            m.insert("zone".into(), FieldValue::Text("A".into()));
            m
        },
    });
    table
}

#[test]
fn feature_table_feature_count() {
    let table = make_table_with_features();
    assert_eq!(table.feature_count(), 3);
}

#[test]
fn feature_table_feature_count_empty() {
    let table = FeatureTable::new("empty", "geom");
    assert_eq!(table.feature_count(), 0);
}

#[test]
fn feature_table_add_feature_increments_count() {
    let mut table = FeatureTable::new("t", "geom");
    assert_eq!(table.feature_count(), 0);
    table.add_feature(FeatureRow {
        fid: 1,
        geometry: None,
        fields: HashMap::new(),
    });
    assert_eq!(table.feature_count(), 1);
}

#[test]
fn feature_table_get_feature_found() {
    let table = make_table_with_features();
    assert!(table.get_feature(2).is_some());
}

#[test]
fn feature_table_get_feature_not_found() {
    let table = make_table_with_features();
    assert!(table.get_feature(99).is_none());
}

#[test]
fn feature_table_bbox_union() {
    let table = make_table_with_features();
    let bbox = table.bbox().expect("should have bbox");
    assert!((bbox.0 - 0.0).abs() < 1e-10, "min_x should be 0.0");
    assert!((bbox.1 - 0.0).abs() < 1e-10, "min_y should be 0.0");
    assert!((bbox.2 - 20.0).abs() < 1e-10, "max_x should be 20.0");
    assert!((bbox.3 - 20.0).abs() < 1e-10, "max_y should be 20.0");
}

#[test]
fn feature_table_bbox_empty_table_is_none() {
    let table = FeatureTable::new("e", "geom");
    assert_eq!(table.bbox(), None);
}

#[test]
fn feature_table_features_in_bbox_includes_within() {
    let table = make_table_with_features();
    // Query bbox covers fid=1 (0,0) and fid=2 (10,10)
    let results = table.features_in_bbox(0.0, 0.0, 15.0, 15.0);
    let fids: Vec<i64> = results.iter().map(|r| r.fid).collect();
    assert!(fids.contains(&1), "fid=1 should be included");
    assert!(fids.contains(&2), "fid=2 should be included");
    assert!(!fids.contains(&3), "fid=3 (20,20) should be excluded");
}

#[test]
fn feature_table_features_in_bbox_excludes_outside() {
    let table = make_table_with_features();
    // Bbox only covers top-right corner
    let results = table.features_in_bbox(15.0, 15.0, 25.0, 25.0);
    let fids: Vec<i64> = results.iter().map(|r| r.fid).collect();
    assert!(
        !fids.contains(&1),
        "fid=1 should NOT be in top-right corner"
    );
    assert!(
        !fids.contains(&2),
        "fid=2 should NOT be in top-right corner"
    );
    assert!(fids.contains(&3), "fid=3 should be included");
}

#[test]
fn feature_table_features_in_bbox_empty_table() {
    let table = FeatureTable::new("empty", "geom");
    let results = table.features_in_bbox(-180.0, -90.0, 180.0, 90.0);
    assert!(results.is_empty());
}

#[test]
fn feature_table_distinct_values() {
    let table = make_table_with_features();
    let values = table.distinct_values("zone");
    assert_eq!(values.len(), 2, "should have 2 distinct zone values");
}

#[test]
fn feature_table_distinct_values_missing_field() {
    let table = make_table_with_features();
    let values = table.distinct_values("nonexistent");
    assert!(values.is_empty());
}

#[test]
fn feature_table_to_geojson_contains_feature_collection() {
    let table = make_table_with_features();
    let json = table.to_geojson();
    assert!(
        json.contains("FeatureCollection"),
        "GeoJSON should contain 'FeatureCollection'"
    );
}

#[test]
fn feature_table_to_geojson_contains_feature_type() {
    let table = make_table_with_features();
    let json = table.to_geojson();
    assert!(
        json.contains("\"Feature\""),
        "GeoJSON should contain Feature objects"
    );
}

#[test]
fn feature_table_to_geojson_empty_table_is_valid() {
    let table = FeatureTable::new("empty", "geom");
    let json = table.to_geojson();
    assert!(
        json.contains("FeatureCollection"),
        "should be FeatureCollection"
    );
    assert!(
        json.contains("\"features\":[]"),
        "features array should be empty"
    );
}

#[test]
fn feature_table_to_geojson_contains_point_type() {
    let table = make_table_with_features();
    let json = table.to_geojson();
    assert!(json.contains("\"Point\""), "should have Point geometry");
}

#[test]
fn feature_table_schema_field_definition() {
    let mut table = FeatureTable::new("t", "geom");
    table.schema.push(FieldDefinition {
        name: "id".into(),
        field_type: FieldType::Integer,
        not_null: true,
        primary_key: true,
        default_value: None,
    });
    assert_eq!(table.schema.len(), 1);
    assert_eq!(table.schema[0].name, "id");
}

// ─────────────────────────────────────────────────────────────────────────────
// SrsInfo tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn srs_info_wgs84_srs_id() {
    let srs = SrsInfo::wgs84();
    assert_eq!(srs.srs_id, 4326);
}

#[test]
fn srs_info_wgs84_organization() {
    let srs = SrsInfo::wgs84();
    assert_eq!(srs.organization, "EPSG");
}

#[test]
fn srs_info_wgs84_epsg_code() {
    let srs = SrsInfo::wgs84();
    assert_eq!(srs.epsg_code(), Some(4326));
}

#[test]
fn srs_info_web_mercator_srs_id() {
    let srs = SrsInfo::web_mercator();
    assert_eq!(srs.srs_id, 3857);
}

#[test]
fn srs_info_wgs84_is_geographic() {
    let srs = SrsInfo::wgs84();
    assert!(srs.is_geographic(), "WGS84 should be geographic");
}

#[test]
fn srs_info_web_mercator_is_not_geographic() {
    let srs = SrsInfo::web_mercator();
    assert!(
        !srs.is_geographic(),
        "Web Mercator should NOT be geographic"
    );
}

#[test]
fn srs_info_epsg_code_non_epsg_org() {
    let srs = SrsInfo {
        srs_name: "Custom".into(),
        srs_id: 9999,
        organization: "CUSTOM".into(),
        org_coord_sys_id: 9999,
        definition: "".into(),
        description: None,
    };
    assert_eq!(srs.epsg_code(), None);
}

// ─────────────────────────────────────────────────────────────────────────────
// TileMatrix tests
// ─────────────────────────────────────────────────────────────────────────────

fn make_tile_matrix(mw: u32, mh: u32) -> TileMatrix {
    TileMatrix {
        table_name: "imagery".into(),
        zoom_level: 1,
        matrix_width: mw,
        matrix_height: mh,
        tile_width: 256,
        tile_height: 256,
        pixel_x_size: 0.01,
        pixel_y_size: 0.01,
    }
}

#[test]
fn tile_matrix_tile_count() {
    let tm = make_tile_matrix(4, 2);
    assert_eq!(tm.tile_count(), 8);
}

#[test]
fn tile_matrix_tile_count_single() {
    let tm = make_tile_matrix(1, 1);
    assert_eq!(tm.tile_count(), 1);
}

#[test]
fn tile_matrix_tile_count_zero_dimension() {
    let tm = make_tile_matrix(0, 4);
    assert_eq!(tm.tile_count(), 0);
}

#[test]
fn tile_matrix_pixel_resolution() {
    let tm = TileMatrix {
        table_name: "t".into(),
        zoom_level: 2,
        matrix_width: 8,
        matrix_height: 4,
        tile_width: 256,
        tile_height: 256,
        pixel_x_size: 0.1,
        pixel_y_size: 0.2,
    };
    assert!((tm.pixel_resolution() - 0.15).abs() < 1e-15);
}

#[test]
fn tile_matrix_pixel_resolution_equal_sides() {
    let tm = make_tile_matrix(8, 4);
    assert!((tm.pixel_resolution() - 0.01).abs() < 1e-15);
}

// ─────────────────────────────────────────────────────────────────────────────
// GpkgError display tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn gpkg_error_wkb_parse_error_display() {
    let err = GpkgError::WkbParseError("unexpected end of data".into());
    let msg = format!("{err}");
    assert!(
        msg.contains("unexpected end of data"),
        "display message should contain the error description"
    );
}

#[test]
fn gpkg_error_unknown_wkb_type_display() {
    let err = GpkgError::UnknownWkbType(42);
    let msg = format!("{err}");
    assert!(msg.contains("42"));
}

#[test]
fn gpkg_error_insufficient_data_display() {
    let err = GpkgError::InsufficientData {
        needed: 16,
        available: 4,
    };
    let msg = format!("{err}");
    assert!(msg.contains("16"));
    assert!(msg.contains("4"));
}

#[test]
fn gpkg_error_invalid_geometry_magic_display() {
    let err = GpkgError::InvalidGeometryMagic;
    let msg = format!("{err}");
    assert!(!msg.is_empty());
}

// ─────────────────────────────────────────────────────────────────────────────
// Multi* WKB round-trip tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn wkb_roundtrip_multipoint() {
    let original = GpkgGeometry::MultiPoint {
        points: vec![(1.0, 2.0), (3.0, 4.0)],
    };
    let encoded = GpkgBinaryParser::to_wkb(&original);
    let decoded = GpkgBinaryParser::parse_wkb(&encoded).expect("decode multipoint");
    assert_eq!(original, decoded);
}

#[test]
fn wkb_roundtrip_multilinestring() {
    let original = GpkgGeometry::MultiLineString {
        lines: vec![vec![(0.0, 0.0), (1.0, 1.0)], vec![(2.0, 2.0), (3.0, 3.0)]],
    };
    let encoded = GpkgBinaryParser::to_wkb(&original);
    let decoded = GpkgBinaryParser::parse_wkb(&encoded).expect("decode multilinestring");
    assert_eq!(original, decoded);
}

#[test]
fn wkb_roundtrip_multipolygon() {
    let original = GpkgGeometry::MultiPolygon {
        polygons: vec![
            vec![vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 0.0)]],
            vec![vec![(5.0, 5.0), (6.0, 5.0), (6.0, 6.0), (5.0, 5.0)]],
        ],
    };
    let encoded = GpkgBinaryParser::to_wkb(&original);
    let decoded = GpkgBinaryParser::parse_wkb(&encoded).expect("decode multipolygon");
    assert_eq!(original, decoded);
}

#[test]
fn wkb_roundtrip_geometry_collection() {
    let original = GpkgGeometry::GeometryCollection(vec![
        GpkgGeometry::Point { x: 1.0, y: 2.0 },
        GpkgGeometry::LineString {
            coords: vec![(0.0, 0.0), (1.0, 1.0)],
        },
    ]);
    let encoded = GpkgBinaryParser::to_wkb(&original);
    let decoded = GpkgBinaryParser::parse_wkb(&encoded).expect("decode geometry collection");
    assert_eq!(original, decoded);
}

// ─────────────────────────────────────────────────────────────────────────────
// GpkgGeometry::point_count
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn point_count_point() {
    assert_eq!(GpkgGeometry::Point { x: 0.0, y: 0.0 }.point_count(), 1);
}

#[test]
fn point_count_linestring() {
    let g = GpkgGeometry::LineString {
        coords: vec![(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)],
    };
    assert_eq!(g.point_count(), 3);
}

#[test]
fn point_count_empty() {
    assert_eq!(GpkgGeometry::Empty.point_count(), 0);
}

// ─────────────────────────────────────────────────────────────────────────────
// null geometry in FeatureTable GeoJSON
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn feature_table_to_geojson_null_geometry() {
    let mut table = FeatureTable::new("t", "geom");
    table.add_feature(FeatureRow {
        fid: 1,
        geometry: None,
        fields: HashMap::new(),
    });
    let json = table.to_geojson();
    assert!(
        json.contains("\"geometry\":null"),
        "null geometry should appear as null in JSON"
    );
}
