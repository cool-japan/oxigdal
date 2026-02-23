//! Integration tests for GeoJSON driver
#![allow(clippy::panic)]
//!
//! These tests verify end-to-end functionality including reading,
//! writing, and round-trip operations.

use oxigdal_geojson::reader::GeoJsonDocument;
use oxigdal_geojson::types::*;
use oxigdal_geojson::validation::ValidationConfig;
use oxigdal_geojson::writer::WriterConfig;
use oxigdal_geojson::*;
use std::io::Cursor;

#[test]
fn test_roundtrip_point() {
    let point = Point::new_2d(100.0, 0.5).expect("valid point");
    let geometry = Geometry::Point(point);

    // Write to buffer
    let mut buffer = Vec::new();
    let mut writer = GeoJsonWriter::new(&mut buffer);
    writer.write_geometry(&geometry).expect("write succeeded");
    writer.flush().expect("flush succeeded");

    // Read back
    let cursor = Cursor::new(buffer);
    let mut reader = GeoJsonReader::new(cursor);
    let doc = reader.read().expect("read succeeded");

    // Verify
    assert!(doc.is_geometry());
    if let GeoJsonDocument::Geometry(Geometry::Point(p)) = doc {
        assert_eq!(p.longitude(), Some(100.0));
        assert_eq!(p.latitude(), Some(0.5));
    } else {
        panic!("Expected Point geometry");
    }
}

#[test]
fn test_roundtrip_linestring() {
    let coords = vec![vec![100.0, 0.0], vec![101.0, 1.0], vec![102.0, 2.0]];
    let linestring = LineString::new(coords).expect("valid linestring");
    let geometry = Geometry::LineString(linestring);

    let mut buffer = Vec::new();
    let mut writer = GeoJsonWriter::new(&mut buffer);
    writer.write_geometry(&geometry).expect("write succeeded");
    writer.flush().expect("flush succeeded");

    let cursor = Cursor::new(buffer);
    let mut reader = GeoJsonReader::new(cursor);
    let doc = reader.read().expect("read succeeded");

    assert!(doc.is_geometry());
    if let GeoJsonDocument::Geometry(Geometry::LineString(ls)) = doc {
        assert_eq!(ls.len(), 3);
    } else {
        panic!("Expected LineString geometry");
    }
}

#[test]
fn test_roundtrip_polygon() {
    let exterior = vec![
        vec![100.0, 0.0],
        vec![101.0, 0.0],
        vec![101.0, 1.0],
        vec![100.0, 1.0],
        vec![100.0, 0.0],
    ];
    let polygon = Polygon::from_exterior(exterior).expect("valid polygon");
    let geometry = Geometry::Polygon(polygon);

    let mut buffer = Vec::new();
    let mut writer = GeoJsonWriter::new(&mut buffer);
    writer.write_geometry(&geometry).expect("write succeeded");
    writer.flush().expect("flush succeeded");

    let cursor = Cursor::new(buffer);
    let mut reader = GeoJsonReader::new(cursor);
    let doc = reader.read().expect("read succeeded");

    assert!(doc.is_geometry());
    if let GeoJsonDocument::Geometry(Geometry::Polygon(p)) = doc {
        assert_eq!(p.ring_count(), 1);
    } else {
        panic!("Expected Polygon geometry");
    }
}

#[test]
fn test_roundtrip_polygon_with_hole() {
    let exterior = vec![
        vec![100.0, 0.0],
        vec![105.0, 0.0],
        vec![105.0, 5.0],
        vec![100.0, 5.0],
        vec![100.0, 0.0],
    ];
    let hole = vec![
        vec![101.0, 1.0],
        vec![104.0, 1.0],
        vec![104.0, 4.0],
        vec![101.0, 4.0],
        vec![101.0, 1.0],
    ];
    let polygon = Polygon::with_holes(exterior, vec![hole]).expect("valid polygon");
    let geometry = Geometry::Polygon(polygon);

    let mut buffer = Vec::new();
    let mut writer = GeoJsonWriter::new(&mut buffer);
    writer.write_geometry(&geometry).expect("write succeeded");
    writer.flush().expect("flush succeeded");

    let cursor = Cursor::new(buffer);
    let mut reader = GeoJsonReader::new(cursor);
    let doc = reader.read().expect("read succeeded");

    assert!(doc.is_geometry());
    if let GeoJsonDocument::Geometry(Geometry::Polygon(p)) = doc {
        assert_eq!(p.ring_count(), 2);
        assert_eq!(p.holes().len(), 1);
    } else {
        panic!("Expected Polygon geometry");
    }
}

#[test]
fn test_roundtrip_multipoint() {
    let coords = vec![vec![100.0, 0.0], vec![101.0, 1.0], vec![102.0, 2.0]];
    let multipoint = MultiPoint::new(coords).expect("valid multipoint");
    let geometry = Geometry::MultiPoint(multipoint);

    let mut buffer = Vec::new();
    let mut writer = GeoJsonWriter::new(&mut buffer);
    writer.write_geometry(&geometry).expect("write succeeded");
    writer.flush().expect("flush succeeded");

    let cursor = Cursor::new(buffer);
    let mut reader = GeoJsonReader::new(cursor);
    let doc = reader.read().expect("read succeeded");

    assert!(doc.is_geometry());
    if let GeoJsonDocument::Geometry(Geometry::MultiPoint(mp)) = doc {
        assert_eq!(mp.len(), 3);
    } else {
        panic!("Expected MultiPoint geometry");
    }
}

#[test]
fn test_roundtrip_multilinestring() {
    let coords = vec![
        vec![vec![100.0, 0.0], vec![101.0, 1.0]],
        vec![vec![102.0, 2.0], vec![103.0, 3.0]],
    ];
    let multilinestring = MultiLineString::new(coords).expect("valid multilinestring");
    let geometry = Geometry::MultiLineString(multilinestring);

    let mut buffer = Vec::new();
    let mut writer = GeoJsonWriter::new(&mut buffer);
    writer.write_geometry(&geometry).expect("write succeeded");
    writer.flush().expect("flush succeeded");

    let cursor = Cursor::new(buffer);
    let mut reader = GeoJsonReader::new(cursor);
    let doc = reader.read().expect("read succeeded");

    assert!(doc.is_geometry());
    if let GeoJsonDocument::Geometry(Geometry::MultiLineString(mls)) = doc {
        assert_eq!(mls.len(), 2);
    } else {
        panic!("Expected MultiLineString geometry");
    }
}

#[test]
fn test_roundtrip_multipolygon() {
    let polygon1 = vec![vec![
        vec![100.0, 0.0],
        vec![101.0, 0.0],
        vec![101.0, 1.0],
        vec![100.0, 1.0],
        vec![100.0, 0.0],
    ]];
    let polygon2 = vec![vec![
        vec![102.0, 2.0],
        vec![103.0, 2.0],
        vec![103.0, 3.0],
        vec![102.0, 3.0],
        vec![102.0, 2.0],
    ]];
    let multipolygon = MultiPolygon::new(vec![polygon1, polygon2]).expect("valid multipolygon");
    let geometry = Geometry::MultiPolygon(multipolygon);

    let mut buffer = Vec::new();
    let mut writer = GeoJsonWriter::new(&mut buffer);
    writer.write_geometry(&geometry).expect("write succeeded");
    writer.flush().expect("flush succeeded");

    let cursor = Cursor::new(buffer);
    let mut reader = GeoJsonReader::new(cursor);
    let doc = reader.read().expect("read succeeded");

    assert!(doc.is_geometry());
    if let GeoJsonDocument::Geometry(Geometry::MultiPolygon(mp)) = doc {
        assert_eq!(mp.len(), 2);
    } else {
        panic!("Expected MultiPolygon geometry");
    }
}

#[test]
fn test_roundtrip_geometry_collection() {
    let point = Geometry::Point(Point::new_2d(100.0, 0.0).expect("valid point"));
    let line = Geometry::LineString(
        LineString::new(vec![vec![101.0, 0.0], vec![102.0, 1.0]]).expect("valid linestring"),
    );
    let collection = GeometryCollection::new(vec![point, line]).expect("valid geometry collection");
    let geometry = Geometry::GeometryCollection(collection);

    let mut buffer = Vec::new();
    let mut writer = GeoJsonWriter::new(&mut buffer);
    writer.write_geometry(&geometry).expect("write succeeded");
    writer.flush().expect("flush succeeded");

    let cursor = Cursor::new(buffer);
    let mut reader = GeoJsonReader::new(cursor);
    let doc = reader.read().expect("read succeeded");

    assert!(doc.is_geometry());
    if let GeoJsonDocument::Geometry(Geometry::GeometryCollection(gc)) = doc {
        assert_eq!(gc.len(), 2);
    } else {
        panic!("Expected GeometryCollection");
    }
}

#[test]
fn test_roundtrip_feature() {
    let point = Point::new_2d(100.0, 0.0).expect("valid point");
    let geometry = Geometry::Point(point);

    let mut props = Properties::new();
    props.insert("name".to_string(), serde_json::json!("Test Feature"));
    props.insert("count".to_string(), serde_json::json!(42));

    let feature = Feature::with_id("feature-1", Some(geometry), Some(props));

    let mut buffer = Vec::new();
    let mut writer = GeoJsonWriter::new(&mut buffer);
    writer.write_feature(&feature).expect("write succeeded");
    writer.flush().expect("flush succeeded");

    let cursor = Cursor::new(buffer);
    let mut reader = GeoJsonReader::new(cursor);
    let doc = reader.read().expect("read succeeded");

    assert!(doc.is_feature());
    if let GeoJsonDocument::Feature(f) = doc {
        assert!(f.id.is_some());
        assert!(f.has_geometry());
        assert!(f.has_properties());
        assert_eq!(f.property_count(), 2);
    } else {
        panic!("Expected Feature");
    }
}

#[test]
fn test_roundtrip_feature_collection() {
    let mut fc = FeatureCollection::empty();

    for i in 0..10 {
        let point = Point::new_2d(f64::from(i), f64::from(i)).expect("valid point");
        let geometry = Geometry::Point(point);

        let mut props = Properties::new();
        props.insert("id".to_string(), serde_json::json!(i));
        props.insert(
            "name".to_string(),
            serde_json::json!(format!("Feature {i}")),
        );

        let feature = Feature::new(Some(geometry), Some(props));
        fc.add_feature(feature);
    }

    let mut buffer = Vec::new();
    let mut writer = GeoJsonWriter::new(&mut buffer);
    writer
        .write_feature_collection(&fc)
        .expect("write succeeded");
    writer.flush().expect("flush succeeded");

    let cursor = Cursor::new(buffer);
    let mut reader = GeoJsonReader::new(cursor);
    let doc = reader.read().expect("read succeeded");

    assert!(doc.is_feature_collection());
    if let GeoJsonDocument::FeatureCollection(read_fc) = doc {
        assert_eq!(read_fc.len(), 10);
    } else {
        panic!("Expected FeatureCollection");
    }
}

#[test]
fn test_feature_with_null_geometry() {
    let feature = Feature::new(None, None);

    let mut buffer = Vec::new();
    let mut writer = GeoJsonWriter::new(&mut buffer);
    writer.write_feature(&feature).expect("write succeeded");
    writer.flush().expect("flush succeeded");

    let cursor = Cursor::new(buffer);
    let mut reader = GeoJsonReader::new(cursor);
    let doc = reader.read().expect("read succeeded");

    assert!(doc.is_feature());
    if let GeoJsonDocument::Feature(f) = doc {
        assert!(!f.has_geometry());
    } else {
        panic!("Expected Feature");
    }
}

#[test]
fn test_feature_with_bbox() {
    let point = Point::new_2d(100.0, 0.0).expect("valid point");
    let geometry = Geometry::Point(point);

    let mut feature = Feature::new(Some(geometry), None);
    feature.set_bbox(vec![100.0, 0.0, 100.0, 0.0]);

    let mut buffer = Vec::new();
    let config = WriterConfig::default().with_bbox(false);
    let mut writer = GeoJsonWriter::with_config(&mut buffer, config);
    writer.write_feature(&feature).expect("write succeeded");
    writer.flush().expect("flush succeeded");

    let json_str = String::from_utf8(buffer).expect("valid UTF-8");
    assert!(json_str.contains("bbox"));
}

#[test]
fn test_feature_collection_with_bbox() {
    let mut fc = FeatureCollection::empty();

    let p1 = Point::new_2d(0.0, 0.0).expect("valid point");
    let p2 = Point::new_2d(10.0, 10.0).expect("valid point");

    fc.add_feature(Feature::new(Some(Geometry::Point(p1)), None));
    fc.add_feature(Feature::new(Some(Geometry::Point(p2)), None));

    fc.compute_bbox();

    let mut buffer = Vec::new();
    let config = WriterConfig::default().with_bbox(false);
    let mut writer = GeoJsonWriter::with_config(&mut buffer, config);
    writer
        .write_feature_collection(&fc)
        .expect("write succeeded");
    writer.flush().expect("flush succeeded");

    let json_str = String::from_utf8(buffer).expect("valid UTF-8");
    assert!(json_str.contains("bbox"));
}

#[test]
fn test_pretty_print() {
    let fc = FeatureCollection::empty();

    let mut buffer = Vec::new();
    let mut writer = GeoJsonWriter::pretty(&mut buffer);
    writer
        .write_feature_collection(&fc)
        .expect("write succeeded");
    writer.flush().expect("flush succeeded");

    let json_str = String::from_utf8(buffer).expect("valid UTF-8");
    assert!(json_str.contains('\n'));
    assert!(json_str.contains("  "));
}

#[test]
fn test_compact_output() {
    let fc = FeatureCollection::empty();

    let mut buffer = Vec::new();
    let mut writer = GeoJsonWriter::compact(&mut buffer);
    writer
        .write_feature_collection(&fc)
        .expect("write succeeded");
    writer.flush().expect("flush succeeded");

    let json_str = String::from_utf8(buffer).expect("valid UTF-8");
    assert!(!json_str.trim().contains('\n'));
}

#[test]
fn test_large_feature_collection() {
    let mut fc = FeatureCollection::with_capacity(200);

    for i in 0..200 {
        let point = Point::new_2d(f64::from(i % 180), f64::from(i % 90)).expect("valid point");
        let geometry = Geometry::Point(point);
        let mut props = Properties::new();
        props.insert("index".to_string(), serde_json::json!(i));
        fc.add_feature(Feature::new(Some(geometry), Some(props)));
    }

    let mut buffer = Vec::new();
    let mut writer = GeoJsonWriter::new(&mut buffer);
    writer
        .write_feature_collection(&fc)
        .expect("write succeeded");
    writer.flush().expect("flush succeeded");

    let cursor = Cursor::new(buffer);
    let mut reader = GeoJsonReader::new(cursor);
    let doc = reader.read().expect("read succeeded");

    if let GeoJsonDocument::FeatureCollection(read_fc) = doc {
        assert_eq!(read_fc.len(), 200);
    } else {
        panic!("Expected FeatureCollection");
    }
}

#[test]
fn test_3d_coordinates() {
    let point = Point::new_3d(100.0, 0.0, 500.0).expect("valid 3d point");
    let geometry = Geometry::Point(point);

    let mut buffer = Vec::new();
    let mut writer = GeoJsonWriter::new(&mut buffer);
    writer.write_geometry(&geometry).expect("write succeeded");
    writer.flush().expect("flush succeeded");

    let cursor = Cursor::new(buffer);
    let mut reader = GeoJsonReader::new(cursor);
    let doc = reader.read().expect("read succeeded");

    if let GeoJsonDocument::Geometry(Geometry::Point(p)) = doc {
        assert_eq!(p.elevation(), Some(500.0));
    } else {
        panic!("Expected Point geometry");
    }
}

#[test]
fn test_crs_support() {
    let point = Point::new_2d(100.0, 0.0).expect("valid point");
    let geometry = Geometry::Point(point);

    let mut feature = Feature::new(Some(geometry), None);
    feature.set_crs(Crs::from_epsg(4326));

    let mut buffer = Vec::new();
    let config = WriterConfig::default().with_crs();
    let mut writer = GeoJsonWriter::with_config(&mut buffer, config);
    writer.write_feature(&feature).expect("write succeeded");
    writer.flush().expect("flush succeeded");

    let json_str = String::from_utf8(buffer).expect("valid UTF-8");
    assert!(json_str.contains("crs"));
}

#[test]
fn test_foreign_members() {
    let json = r#"{
        "type": "Feature",
        "geometry": {"type": "Point", "coordinates": [100.0, 0.0]},
        "properties": null,
        "customField": "customValue",
        "anotherField": 123
    }"#;

    let cursor = Cursor::new(json.as_bytes());
    let mut reader = GeoJsonReader::new(cursor);
    let doc = reader.read().expect("read succeeded");

    assert!(doc.is_feature());
}

#[test]
fn test_validation_strict() {
    let json = r#"{
        "type": "Point",
        "coordinates": [181.0, 0.0]
    }"#;

    let cursor = Cursor::new(json.as_bytes());
    let config = ValidationConfig::default();
    let mut reader = GeoJsonReader::with_validation_config(cursor, config);

    let result = reader.read();
    assert!(result.is_err());
}

#[test]
fn test_validation_disabled() {
    let json = r#"{
        "type": "Point",
        "coordinates": [181.0, 0.0]
    }"#;

    let cursor = Cursor::new(json.as_bytes());
    let mut reader = GeoJsonReader::without_validation(cursor);

    let result = reader.read();
    assert!(result.is_ok());
}

#[test]
fn test_empty_feature_collection() {
    let fc = FeatureCollection::empty();

    let mut buffer = Vec::new();
    let mut writer = GeoJsonWriter::new(&mut buffer);
    writer
        .write_feature_collection(&fc)
        .expect("write succeeded");
    writer.flush().expect("flush succeeded");

    let cursor = Cursor::new(buffer);
    let mut reader = GeoJsonReader::new(cursor);
    let doc = reader.read().expect("read succeeded");

    if let GeoJsonDocument::FeatureCollection(read_fc) = doc {
        assert!(read_fc.is_empty());
    } else {
        panic!("Expected FeatureCollection");
    }
}

#[test]
fn test_feature_filtering() {
    let mut fc = FeatureCollection::empty();

    for i in 0..10 {
        let point = Point::new_2d(f64::from(i), f64::from(i)).expect("valid point");
        let geometry = Geometry::Point(point);
        let mut props = Properties::new();
        props.insert(
            "type".to_string(),
            serde_json::json!(if i % 2 == 0 { "even" } else { "odd" }),
        );
        fc.add_feature(Feature::new(Some(geometry), Some(props)));
    }

    let even_features = fc.with_property("type", &serde_json::json!("even"));
    assert_eq!(even_features.len(), 5);

    let odd_features = fc.with_property("type", &serde_json::json!("odd"));
    assert_eq!(odd_features.len(), 5);
}
