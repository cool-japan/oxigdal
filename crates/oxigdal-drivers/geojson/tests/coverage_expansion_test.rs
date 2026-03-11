//! Expanded integration tests for GeoJSON driver - Core Driver Test Coverage
//!
//! This module adds 20+ additional tests covering:
//! - All geometry types
//! - Feature properties round-trip
//! - FeatureCollection with mixed geometry types
//! - Coordinate precision
//! - Nested properties (objects, arrays)
//! - Large coordinates / edge cases (antimeridian, poles)
//! - Error handling (invalid JSON, missing type field)

#![allow(clippy::panic)]

use oxigdal_geojson::reader::GeoJsonDocument;
use oxigdal_geojson::types::*;
use oxigdal_geojson::*;
use std::io::Cursor;

// ============================================================
// All geometry type tests
// ============================================================

/// Test 1: Point with exact coordinates preserved
#[test]
fn test_cov_point_coordinate_precision() {
    let lon = 139.6917;
    let lat = 35.6895;
    let point = Point::new_2d(lon, lat).expect("valid point for precision test");
    let geometry = Geometry::Point(point);

    let mut buffer = Vec::new();
    let mut writer = GeoJsonWriter::new(&mut buffer);
    writer
        .write_geometry(&geometry)
        .expect("write point precision");
    writer.flush().expect("flush point precision");

    let cursor = Cursor::new(buffer);
    let mut reader = GeoJsonReader::new(cursor);
    let doc = reader.read().expect("read point precision");

    if let GeoJsonDocument::Geometry(Geometry::Point(p)) = doc {
        let read_lon = p.longitude().expect("should have longitude");
        let read_lat = p.latitude().expect("should have latitude");
        assert!(
            (read_lon - lon).abs() < 1e-10,
            "Longitude should be preserved with high precision"
        );
        assert!(
            (read_lat - lat).abs() < 1e-10,
            "Latitude should be preserved with high precision"
        );
    } else {
        panic!("Expected Point geometry for precision test");
    }
}

/// Test 2: 3D Point with elevation
#[test]
fn test_cov_point_3d_with_elevation() {
    let point = Point::new_3d(100.0, 50.0, 1234.56).expect("valid 3D point");
    let geometry = Geometry::Point(point);

    let mut buffer = Vec::new();
    let mut writer = GeoJsonWriter::new(&mut buffer);
    writer.write_geometry(&geometry).expect("write 3D point");
    writer.flush().expect("flush 3D point");

    let cursor = Cursor::new(buffer);
    let mut reader = GeoJsonReader::new(cursor);
    let doc = reader.read().expect("read 3D point");

    if let GeoJsonDocument::Geometry(Geometry::Point(p)) = doc {
        assert_eq!(p.longitude(), Some(100.0), "3D point longitude");
        assert_eq!(p.latitude(), Some(50.0), "3D point latitude");
        let elev = p.elevation().expect("should have elevation");
        assert!(
            (elev - 1234.56).abs() < 1e-6,
            "3D point elevation should be 1234.56"
        );
    } else {
        panic!("Expected 3D Point geometry");
    }
}

/// Test 3: LineString with many coordinates
#[test]
fn test_cov_linestring_many_coords() {
    let mut coords = Vec::new();
    for i in 0..50 {
        let lon = (i as f64 * 0.1) - 2.5;
        let lat = (i as f64 * 0.05) - 1.25;
        coords.push(vec![lon, lat]);
    }

    let linestring = LineString::new(coords.clone()).expect("valid linestring many coords");
    let geometry = Geometry::LineString(linestring);

    let mut buffer = Vec::new();
    let mut writer = GeoJsonWriter::new(&mut buffer);
    writer
        .write_geometry(&geometry)
        .expect("write linestring many coords");
    writer.flush().expect("flush linestring many coords");

    let cursor = Cursor::new(buffer);
    let mut reader = GeoJsonReader::new(cursor);
    let doc = reader.read().expect("read linestring many coords");

    if let GeoJsonDocument::Geometry(Geometry::LineString(ls)) = doc {
        assert_eq!(ls.len(), 50, "LineString should have 50 coordinates");
    } else {
        panic!("Expected LineString geometry");
    }
}

/// Test 4: Polygon with multiple holes
#[test]
fn test_cov_polygon_multiple_holes() {
    let exterior = vec![
        vec![0.0, 0.0],
        vec![20.0, 0.0],
        vec![20.0, 20.0],
        vec![0.0, 20.0],
        vec![0.0, 0.0],
    ];
    let hole1 = vec![
        vec![2.0, 2.0],
        vec![5.0, 2.0],
        vec![5.0, 5.0],
        vec![2.0, 5.0],
        vec![2.0, 2.0],
    ];
    let hole2 = vec![
        vec![10.0, 10.0],
        vec![15.0, 10.0],
        vec![15.0, 15.0],
        vec![10.0, 15.0],
        vec![10.0, 10.0],
    ];

    let polygon =
        Polygon::with_holes(exterior, vec![hole1, hole2]).expect("valid polygon with 2 holes");
    let geometry = Geometry::Polygon(polygon);

    let mut buffer = Vec::new();
    let mut writer = GeoJsonWriter::new(&mut buffer);
    writer
        .write_geometry(&geometry)
        .expect("write polygon with holes");
    writer.flush().expect("flush polygon with holes");

    let cursor = Cursor::new(buffer);
    let mut reader = GeoJsonReader::new(cursor);
    let doc = reader.read().expect("read polygon with holes");

    if let GeoJsonDocument::Geometry(Geometry::Polygon(p)) = doc {
        assert_eq!(
            p.ring_count(),
            3,
            "Polygon should have 3 rings (exterior + 2 holes)"
        );
        assert_eq!(p.holes().len(), 2, "Polygon should have 2 holes");
    } else {
        panic!("Expected Polygon geometry with holes");
    }
}

/// Test 5: MultiPoint with varying coordinates
#[test]
fn test_cov_multipoint_varied() {
    let coords = vec![
        vec![-180.0, -90.0],
        vec![0.0, 0.0],
        vec![180.0, 90.0],
        vec![-122.4, 37.8],
        vec![139.7, 35.7],
    ];

    let multipoint = MultiPoint::new(coords).expect("valid multipoint varied");
    let geometry = Geometry::MultiPoint(multipoint);

    let mut buffer = Vec::new();
    let mut writer = GeoJsonWriter::new(&mut buffer);
    writer
        .write_geometry(&geometry)
        .expect("write multipoint varied");
    writer.flush().expect("flush multipoint varied");

    let cursor = Cursor::new(buffer);
    let mut reader = GeoJsonReader::new(cursor);
    let doc = reader.read().expect("read multipoint varied");

    if let GeoJsonDocument::Geometry(Geometry::MultiPoint(mp)) = doc {
        assert_eq!(mp.len(), 5, "MultiPoint should have 5 points");
    } else {
        panic!("Expected MultiPoint geometry");
    }
}

/// Test 6: MultiLineString round-trip
#[test]
fn test_cov_multilinestring_roundtrip() {
    let coords = vec![
        vec![vec![0.0, 0.0], vec![10.0, 10.0], vec![20.0, 0.0]],
        vec![vec![30.0, 30.0], vec![40.0, 40.0]],
        vec![
            vec![50.0, 50.0],
            vec![60.0, 60.0],
            vec![70.0, 50.0],
            vec![80.0, 60.0],
        ],
    ];

    let mls = MultiLineString::new(coords).expect("valid multilinestring");
    let geometry = Geometry::MultiLineString(mls);

    let mut buffer = Vec::new();
    let mut writer = GeoJsonWriter::new(&mut buffer);
    writer
        .write_geometry(&geometry)
        .expect("write multilinestring");
    writer.flush().expect("flush multilinestring");

    let cursor = Cursor::new(buffer);
    let mut reader = GeoJsonReader::new(cursor);
    let doc = reader.read().expect("read multilinestring");

    if let GeoJsonDocument::Geometry(Geometry::MultiLineString(read_mls)) = doc {
        assert_eq!(read_mls.len(), 3, "MultiLineString should have 3 lines");
    } else {
        panic!("Expected MultiLineString geometry");
    }
}

/// Test 7: MultiPolygon round-trip
#[test]
fn test_cov_multipolygon_roundtrip() {
    let poly1 = vec![vec![
        vec![0.0, 0.0],
        vec![5.0, 0.0],
        vec![5.0, 5.0],
        vec![0.0, 5.0],
        vec![0.0, 0.0],
    ]];
    let poly2 = vec![vec![
        vec![10.0, 10.0],
        vec![15.0, 10.0],
        vec![15.0, 15.0],
        vec![10.0, 15.0],
        vec![10.0, 10.0],
    ]];
    let poly3 = vec![vec![
        vec![20.0, 20.0],
        vec![25.0, 20.0],
        vec![25.0, 25.0],
        vec![20.0, 25.0],
        vec![20.0, 20.0],
    ]];

    let mp = MultiPolygon::new(vec![poly1, poly2, poly3]).expect("valid multipolygon");
    let geometry = Geometry::MultiPolygon(mp);

    let mut buffer = Vec::new();
    let mut writer = GeoJsonWriter::new(&mut buffer);
    writer
        .write_geometry(&geometry)
        .expect("write multipolygon");
    writer.flush().expect("flush multipolygon");

    let cursor = Cursor::new(buffer);
    let mut reader = GeoJsonReader::new(cursor);
    let doc = reader.read().expect("read multipolygon");

    if let GeoJsonDocument::Geometry(Geometry::MultiPolygon(read_mp)) = doc {
        assert_eq!(read_mp.len(), 3, "MultiPolygon should have 3 polygons");
    } else {
        panic!("Expected MultiPolygon geometry");
    }
}

/// Test 8: GeometryCollection with mixed types
#[test]
fn test_cov_geometry_collection_mixed() {
    let point = Geometry::Point(Point::new_2d(0.0, 0.0).expect("valid point for collection"));
    let line = Geometry::LineString(
        LineString::new(vec![vec![1.0, 1.0], vec![2.0, 2.0]]).expect("valid linestring"),
    );
    let polygon = Geometry::Polygon(
        Polygon::from_exterior(vec![
            vec![3.0, 3.0],
            vec![6.0, 3.0],
            vec![6.0, 6.0],
            vec![3.0, 6.0],
            vec![3.0, 3.0],
        ])
        .expect("valid polygon"),
    );
    let multipoint = Geometry::MultiPoint(
        MultiPoint::new(vec![vec![7.0, 7.0], vec![8.0, 8.0]]).expect("valid multipoint"),
    );

    let gc =
        GeometryCollection::new(vec![point, line, polygon, multipoint]).expect("valid collection");
    let geometry = Geometry::GeometryCollection(gc);

    let mut buffer = Vec::new();
    let mut writer = GeoJsonWriter::new(&mut buffer);
    writer
        .write_geometry(&geometry)
        .expect("write geometry collection");
    writer.flush().expect("flush geometry collection");

    let cursor = Cursor::new(buffer);
    let mut reader = GeoJsonReader::new(cursor);
    let doc = reader.read().expect("read geometry collection");

    if let GeoJsonDocument::Geometry(Geometry::GeometryCollection(read_gc)) = doc {
        assert_eq!(read_gc.len(), 4, "Collection should have 4 geometries");
    } else {
        panic!("Expected GeometryCollection");
    }
}

// ============================================================
// Feature properties round-trip tests
// ============================================================

/// Test 9: Feature with various property types
#[test]
fn test_cov_feature_property_types() {
    let point = Point::new_2d(0.0, 0.0).expect("valid point for property types");
    let geometry = Geometry::Point(point);

    let mut props = Properties::new();
    props.insert("string_val".to_string(), serde_json::json!("hello"));
    props.insert("int_val".to_string(), serde_json::json!(42));
    #[allow(clippy::approx_constant)]
    let float_literal = 3.14f64;
    props.insert("float_val".to_string(), serde_json::json!(float_literal));
    props.insert("bool_val".to_string(), serde_json::json!(true));
    props.insert("null_val".to_string(), serde_json::json!(null));

    let feature = Feature::new(Some(geometry), Some(props));

    let mut buffer = Vec::new();
    let mut writer = GeoJsonWriter::new(&mut buffer);
    writer
        .write_feature(&feature)
        .expect("write feature with property types");
    writer.flush().expect("flush feature with property types");

    let cursor = Cursor::new(buffer);
    let mut reader = GeoJsonReader::new(cursor);
    let doc = reader.read().expect("read feature with property types");

    if let GeoJsonDocument::Feature(f) = doc {
        assert_eq!(f.property_count(), 5, "Should have 5 properties");

        let str_val = f
            .get_property("string_val")
            .expect("should have string_val");
        assert_eq!(
            str_val.as_str(),
            Some("hello"),
            "string_val should be 'hello'"
        );

        let int_val = f.get_property("int_val").expect("should have int_val");
        assert_eq!(int_val.as_i64(), Some(42), "int_val should be 42");

        let float_val = f.get_property("float_val").expect("should have float_val");
        let fv = float_val.as_f64().expect("float_val should be f64");
        #[allow(clippy::approx_constant)]
        let expected_float = 3.14f64;
        assert!(
            (fv - expected_float).abs() < 1e-10,
            "float_val should be 3.14"
        );

        let bool_val = f.get_property("bool_val").expect("should have bool_val");
        assert_eq!(bool_val.as_bool(), Some(true), "bool_val should be true");

        let null_val = f.get_property("null_val").expect("should have null_val");
        assert!(null_val.is_null(), "null_val should be null");
    } else {
        panic!("Expected Feature with property types");
    }
}

/// Test 10: Nested properties (objects and arrays)
#[test]
fn test_cov_nested_properties() {
    let point = Point::new_2d(0.0, 0.0).expect("valid point for nested props");
    let geometry = Geometry::Point(point);

    let mut props = Properties::new();
    props.insert(
        "nested_obj".to_string(),
        serde_json::json!({"key1": "value1", "key2": 42}),
    );
    props.insert(
        "nested_arr".to_string(),
        serde_json::json!([1, 2, 3, "four", null]),
    );
    props.insert(
        "deep_nested".to_string(),
        serde_json::json!({"a": {"b": {"c": "deep"}}}),
    );

    let feature = Feature::new(Some(geometry), Some(props));

    let mut buffer = Vec::new();
    let mut writer = GeoJsonWriter::new(&mut buffer);
    writer
        .write_feature(&feature)
        .expect("write feature with nested props");
    writer.flush().expect("flush feature with nested props");

    let cursor = Cursor::new(buffer);
    let mut reader = GeoJsonReader::new(cursor);
    let doc = reader.read().expect("read feature with nested props");

    if let GeoJsonDocument::Feature(f) = doc {
        assert_eq!(f.property_count(), 3, "Should have 3 nested properties");

        let obj = f
            .get_property("nested_obj")
            .expect("should have nested_obj");
        assert!(obj.is_object(), "nested_obj should be an object");
        assert_eq!(
            obj.get("key1").and_then(|v| v.as_str()),
            Some("value1"),
            "nested_obj.key1 should be 'value1'"
        );

        let arr = f
            .get_property("nested_arr")
            .expect("should have nested_arr");
        assert!(arr.is_array(), "nested_arr should be an array");
        let arr_val = arr.as_array().expect("nested_arr should be array");
        assert_eq!(arr_val.len(), 5, "nested_arr should have 5 elements");

        let deep = f
            .get_property("deep_nested")
            .expect("should have deep_nested");
        let deep_val = deep
            .get("a")
            .and_then(|v| v.get("b"))
            .and_then(|v| v.get("c"))
            .and_then(|v| v.as_str());
        assert_eq!(deep_val, Some("deep"), "deep nested value should be 'deep'");
    } else {
        panic!("Expected Feature with nested props");
    }
}

/// Test 11: Feature with ID types (string and numeric)
#[test]
fn test_cov_feature_id_types() {
    let point = Point::new_2d(0.0, 0.0).expect("valid point for ID types");
    let geometry = Geometry::Point(point.clone());

    // String ID
    let f1 = Feature::with_id("string-id-123", Some(geometry.clone()), None);
    let mut buffer = Vec::new();
    let mut writer = GeoJsonWriter::new(&mut buffer);
    writer.write_feature(&f1).expect("write string ID feature");
    writer.flush().expect("flush string ID feature");

    let cursor = Cursor::new(buffer);
    let mut reader = GeoJsonReader::new(cursor);
    let doc = reader.read().expect("read string ID feature");

    if let GeoJsonDocument::Feature(f) = doc {
        assert!(f.id.is_some(), "Should have string ID");
    } else {
        panic!("Expected Feature with string ID");
    }

    // Numeric ID
    let f2 = Feature::with_id(42i64, Some(geometry), None);
    let mut buffer2 = Vec::new();
    let mut writer2 = GeoJsonWriter::new(&mut buffer2);
    writer2
        .write_feature(&f2)
        .expect("write numeric ID feature");
    writer2.flush().expect("flush numeric ID feature");

    let cursor2 = Cursor::new(buffer2);
    let mut reader2 = GeoJsonReader::new(cursor2);
    let doc2 = reader2.read().expect("read numeric ID feature");

    if let GeoJsonDocument::Feature(f) = doc2 {
        assert!(f.id.is_some(), "Should have numeric ID");
    } else {
        panic!("Expected Feature with numeric ID");
    }
}

// ============================================================
// FeatureCollection tests
// ============================================================

/// Test 12: FeatureCollection with mixed geometry types
#[test]
fn test_cov_feature_collection_mixed_geometries() {
    let mut fc = FeatureCollection::empty();

    // Add a Point feature
    let point = Point::new_2d(0.0, 0.0).expect("valid point for mixed FC");
    fc.add_feature(Feature::new(Some(Geometry::Point(point)), None));

    // Add a LineString feature
    let linestring = LineString::new(vec![vec![1.0, 1.0], vec![2.0, 2.0]])
        .expect("valid linestring for mixed FC");
    fc.add_feature(Feature::new(Some(Geometry::LineString(linestring)), None));

    // Add a Polygon feature
    let polygon = Polygon::from_exterior(vec![
        vec![3.0, 3.0],
        vec![6.0, 3.0],
        vec![6.0, 6.0],
        vec![3.0, 6.0],
        vec![3.0, 3.0],
    ])
    .expect("valid polygon for mixed FC");
    fc.add_feature(Feature::new(Some(Geometry::Polygon(polygon)), None));

    // Add a null geometry feature
    fc.add_feature(Feature::new(None, None));

    let mut buffer = Vec::new();
    let mut writer = GeoJsonWriter::new(&mut buffer);
    writer
        .write_feature_collection(&fc)
        .expect("write mixed FC");
    writer.flush().expect("flush mixed FC");

    let cursor = Cursor::new(buffer);
    let mut reader = GeoJsonReader::new(cursor);
    let doc = reader.read().expect("read mixed FC");

    if let GeoJsonDocument::FeatureCollection(read_fc) = doc {
        assert_eq!(read_fc.len(), 4, "Mixed FC should have 4 features");
    } else {
        panic!("Expected FeatureCollection");
    }
}

/// Test 13: FeatureCollection with computed bbox
#[test]
fn test_cov_feature_collection_computed_bbox() {
    let mut fc = FeatureCollection::empty();

    let p1 = Point::new_2d(-10.0, -20.0).expect("valid point p1");
    let p2 = Point::new_2d(30.0, 40.0).expect("valid point p2");

    fc.add_feature(Feature::new(Some(Geometry::Point(p1)), None));
    fc.add_feature(Feature::new(Some(Geometry::Point(p2)), None));

    fc.compute_bbox();

    assert!(fc.bbox.is_some(), "FC should have computed bbox");
    if let Some(bbox) = &fc.bbox {
        assert_eq!(bbox.len(), 4, "Bbox should have 4 elements");
        assert!((bbox[0] - (-10.0)).abs() < 1e-10, "min_x should be -10");
        assert!((bbox[1] - (-20.0)).abs() < 1e-10, "min_y should be -20");
        assert!((bbox[2] - 30.0).abs() < 1e-10, "max_x should be 30");
        assert!((bbox[3] - 40.0).abs() < 1e-10, "max_y should be 40");
    }
}

/// Test 14: Large FeatureCollection (300 features)
#[test]
fn test_cov_large_feature_collection() {
    let count = 300;
    let mut fc = FeatureCollection::with_capacity(count);

    for i in 0..count {
        let lon = ((i as f64 * 1.2) % 360.0) - 180.0;
        let lat = ((i as f64 * 0.6) % 180.0) - 90.0;
        let point = Point::new_2d(lon, lat).expect("valid point for large FC");
        let geometry = Geometry::Point(point);
        let mut props = Properties::new();
        props.insert("index".to_string(), serde_json::json!(i));
        props.insert(
            "label".to_string(),
            serde_json::json!(format!("Feature_{}", i)),
        );
        fc.add_feature(Feature::new(Some(geometry), Some(props)));
    }

    let mut buffer = Vec::new();
    let mut writer = GeoJsonWriter::new(&mut buffer);
    writer
        .write_feature_collection(&fc)
        .expect("write large FC");
    writer.flush().expect("flush large FC");

    let cursor = Cursor::new(buffer);
    let mut reader = GeoJsonReader::new(cursor);
    let doc = reader.read().expect("read large FC");

    if let GeoJsonDocument::FeatureCollection(read_fc) = doc {
        assert_eq!(read_fc.len(), count, "Large FC should have all features");
    } else {
        panic!("Expected large FeatureCollection");
    }
}

// ============================================================
// Edge cases - coordinates
// ============================================================

/// Test 15: Antimeridian crossing (180 / -180 boundary)
#[test]
fn test_cov_antimeridian_coordinates() {
    // Points at the antimeridian
    let p1 = Point::new_2d(180.0, 0.0).expect("valid point at 180");
    let p2 = Point::new_2d(-180.0, 0.0).expect("valid point at -180");

    for (label, point) in &[("180", p1), ("-180", p2)] {
        let geometry = Geometry::Point(point.clone());
        let mut buffer = Vec::new();
        let mut writer = GeoJsonWriter::new(&mut buffer);
        writer
            .write_geometry(&geometry)
            .unwrap_or_else(|e| panic!("write antimeridian point {}: {}", label, e));
        writer
            .flush()
            .unwrap_or_else(|e| panic!("flush antimeridian point {}: {}", label, e));

        let cursor = Cursor::new(buffer);
        let mut reader = GeoJsonReader::new(cursor);
        let doc = reader
            .read()
            .unwrap_or_else(|e| panic!("read antimeridian point {}: {}", label, e));
        assert!(
            doc.is_geometry(),
            "Antimeridian point {} should be valid geometry",
            label
        );
    }
}

/// Test 16: Pole coordinates
#[test]
fn test_cov_pole_coordinates() {
    // North Pole
    let north_pole = Point::new_2d(0.0, 90.0).expect("valid north pole");
    let geometry = Geometry::Point(north_pole);

    let mut buffer = Vec::new();
    let mut writer = GeoJsonWriter::new(&mut buffer);
    writer.write_geometry(&geometry).expect("write north pole");
    writer.flush().expect("flush north pole");

    let cursor = Cursor::new(buffer);
    let mut reader = GeoJsonReader::new(cursor);
    let doc = reader.read().expect("read north pole");

    if let GeoJsonDocument::Geometry(Geometry::Point(p)) = doc {
        assert_eq!(p.latitude(), Some(90.0), "North pole latitude should be 90");
    } else {
        panic!("Expected north pole Point");
    }

    // South Pole
    let south_pole = Point::new_2d(0.0, -90.0).expect("valid south pole");
    let geometry = Geometry::Point(south_pole);

    let mut buffer2 = Vec::new();
    let mut writer2 = GeoJsonWriter::new(&mut buffer2);
    writer2.write_geometry(&geometry).expect("write south pole");
    writer2.flush().expect("flush south pole");

    let cursor2 = Cursor::new(buffer2);
    let mut reader2 = GeoJsonReader::new(cursor2);
    let doc2 = reader2.read().expect("read south pole");

    if let GeoJsonDocument::Geometry(Geometry::Point(p)) = doc2 {
        assert_eq!(
            p.latitude(),
            Some(-90.0),
            "South pole latitude should be -90"
        );
    } else {
        panic!("Expected south pole Point");
    }
}

/// Test 17: Origin coordinates (0,0)
#[test]
fn test_cov_origin_coordinates() {
    let origin = Point::new_2d(0.0, 0.0).expect("valid origin");
    let geometry = Geometry::Point(origin);

    let mut buffer = Vec::new();
    let mut writer = GeoJsonWriter::new(&mut buffer);
    writer.write_geometry(&geometry).expect("write origin");
    writer.flush().expect("flush origin");

    let cursor = Cursor::new(buffer);
    let mut reader = GeoJsonReader::new(cursor);
    let doc = reader.read().expect("read origin");

    if let GeoJsonDocument::Geometry(Geometry::Point(p)) = doc {
        assert_eq!(p.longitude(), Some(0.0), "Origin longitude should be 0.0");
        assert_eq!(p.latitude(), Some(0.0), "Origin latitude should be 0.0");
    } else {
        panic!("Expected origin Point");
    }
}

// ============================================================
// Error handling tests
// ============================================================

/// Test 18: Invalid JSON input
#[test]
fn test_cov_invalid_json_input() {
    let invalid_json = b"this is not json at all!!!";
    let cursor = Cursor::new(invalid_json.as_slice());
    let mut reader = GeoJsonReader::new(cursor);
    let result = reader.read();
    assert!(result.is_err(), "Should fail on invalid JSON");
}

/// Test 19: JSON without type field
#[test]
fn test_cov_json_missing_type_field() {
    let json = r#"{"coordinates": [100.0, 0.0]}"#;
    let cursor = Cursor::new(json.as_bytes());
    let mut reader = GeoJsonReader::new(cursor);
    let result = reader.read();
    assert!(result.is_err(), "Should fail when type field is missing");
}

/// Test 20: Invalid geometry type string
#[test]
fn test_cov_invalid_geometry_type() {
    let json = r#"{"type": "InvalidType", "coordinates": [100.0, 0.0]}"#;
    let cursor = Cursor::new(json.as_bytes());
    let mut reader = GeoJsonReader::new(cursor);
    let result = reader.read();
    assert!(result.is_err(), "Should fail on invalid geometry type");
}

/// Test 21: Longitude out of range (strict validation)
#[test]
fn test_cov_longitude_out_of_range() {
    let json = r#"{"type": "Point", "coordinates": [200.0, 0.0]}"#;
    let cursor = Cursor::new(json.as_bytes());
    let config = oxigdal_geojson::validation::ValidationConfig::default();
    let mut reader = GeoJsonReader::with_validation_config(cursor, config);
    let result = reader.read();
    assert!(
        result.is_err(),
        "Should fail on longitude > 180 with strict validation"
    );
}

/// Test 22: Latitude out of range (strict validation)
#[test]
fn test_cov_latitude_out_of_range() {
    let json = r#"{"type": "Point", "coordinates": [0.0, 100.0]}"#;
    let cursor = Cursor::new(json.as_bytes());
    let config = oxigdal_geojson::validation::ValidationConfig::default();
    let mut reader = GeoJsonReader::with_validation_config(cursor, config);
    let result = reader.read();
    assert!(
        result.is_err(),
        "Should fail on latitude > 90 with strict validation"
    );
}

/// Test 23: Out-of-range coordinates accepted with validation disabled
#[test]
fn test_cov_validation_disabled_accepts_out_of_range() {
    let json = r#"{"type": "Point", "coordinates": [200.0, 100.0]}"#;
    let cursor = Cursor::new(json.as_bytes());
    let mut reader = GeoJsonReader::without_validation(cursor);
    let result = reader.read();
    assert!(
        result.is_ok(),
        "Should accept out-of-range coords with validation disabled"
    );
}

/// Test 24: Empty coordinates for Point
#[test]
fn test_cov_point_insufficient_coordinates() {
    let result = Point::new(vec![0.0]);
    assert!(result.is_err(), "Point with only 1 coordinate should fail");

    let result2 = Point::new(vec![]);
    assert!(result2.is_err(), "Point with 0 coordinates should fail");
}

/// Test 25: LineString with too few coordinates
#[test]
fn test_cov_linestring_too_few_coords() {
    let result = LineString::new(vec![vec![0.0, 0.0]]);
    assert!(result.is_err(), "LineString with 1 coordinate should fail");
}

// ============================================================
// Writer configuration tests
// ============================================================

/// Test 26: Pretty print vs compact output
#[test]
fn test_cov_pretty_vs_compact() {
    let fc = FeatureCollection::empty();

    // Pretty
    let mut pretty_buf = Vec::new();
    let mut writer_p = GeoJsonWriter::pretty(&mut pretty_buf);
    writer_p
        .write_feature_collection(&fc)
        .expect("write pretty FC");
    writer_p.flush().expect("flush pretty FC");
    let pretty_str = String::from_utf8(pretty_buf).expect("valid UTF-8 for pretty");
    assert!(
        pretty_str.contains('\n'),
        "Pretty output should contain newlines"
    );

    // Compact
    let mut compact_buf = Vec::new();
    let mut writer_c = GeoJsonWriter::compact(&mut compact_buf);
    writer_c
        .write_feature_collection(&fc)
        .expect("write compact FC");
    writer_c.flush().expect("flush compact FC");
    let compact_str = String::from_utf8(compact_buf).expect("valid UTF-8 for compact");
    assert!(
        !compact_str.trim().contains('\n'),
        "Compact output should not contain newlines"
    );

    // Compact should be shorter
    assert!(
        compact_str.len() <= pretty_str.len(),
        "Compact output should be shorter or equal to pretty output"
    );
}

/// Test 27: Feature with null geometry and properties
#[test]
fn test_cov_feature_null_everything() {
    let feature = Feature::new(None, None);

    let mut buffer = Vec::new();
    let mut writer = GeoJsonWriter::new(&mut buffer);
    writer.write_feature(&feature).expect("write null feature");
    writer.flush().expect("flush null feature");

    let cursor = Cursor::new(buffer);
    let mut reader = GeoJsonReader::new(cursor);
    let doc = reader.read().expect("read null feature");

    if let GeoJsonDocument::Feature(f) = doc {
        assert!(!f.has_geometry(), "Should not have geometry");
        assert_eq!(f.property_count(), 0, "Should have 0 properties");
    } else {
        panic!("Expected null Feature");
    }
}

/// Test 28: Empty GeometryCollection
#[test]
fn test_cov_empty_geometry_collection() {
    let gc = GeometryCollection::new(vec![]).expect("valid empty geometry collection");
    let geometry = Geometry::GeometryCollection(gc);

    let mut buffer = Vec::new();
    let mut writer = GeoJsonWriter::new(&mut buffer);
    writer
        .write_geometry(&geometry)
        .expect("write empty geometry collection");
    writer.flush().expect("flush empty geometry collection");

    let cursor = Cursor::new(buffer);
    let mut reader = GeoJsonReader::new(cursor);
    let doc = reader.read().expect("read empty geometry collection");

    if let GeoJsonDocument::Geometry(Geometry::GeometryCollection(read_gc)) = doc {
        assert!(
            read_gc.is_empty(),
            "Empty geometry collection should be empty after round-trip"
        );
    } else {
        panic!("Expected empty GeometryCollection");
    }
}
