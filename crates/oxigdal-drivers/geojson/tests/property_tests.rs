//! Property-based tests using proptest
//!
//! These tests verify invariants and properties across randomly generated inputs.
#![allow(clippy::panic)]

use oxigdal_geojson::reader::GeoJsonDocument;
use oxigdal_geojson::types::*;
use oxigdal_geojson::*;
use proptest::prelude::*;
use std::io::Cursor;

// Strategy for generating valid coordinates
fn coord_strategy() -> impl Strategy<Value = f64> {
    -180.0..=180.0f64
}

fn lat_strategy() -> impl Strategy<Value = f64> {
    -90.0..=90.0f64
}

fn position_strategy() -> impl Strategy<Value = Position> {
    (coord_strategy(), lat_strategy()).prop_map(|(lon, lat)| vec![lon, lat])
}

fn positions_strategy(min_len: usize, max_len: usize) -> impl Strategy<Value = Vec<Position>> {
    prop::collection::vec(position_strategy(), min_len..=max_len)
}

proptest! {
    #[test]
    fn test_point_roundtrip(lon in coord_strategy(), lat in lat_strategy()) {
        let point = Point::new_2d(lon, lat).expect("valid point");
        let geometry = Geometry::Point(point);

        let mut buffer = Vec::new();
        let mut writer = GeoJsonWriter::new(&mut buffer);
        writer.write_geometry(&geometry).expect("write succeeded");
        writer.flush().expect("flush succeeded");

        let cursor = Cursor::new(buffer);
        let mut reader = GeoJsonReader::new(cursor);
        let doc = reader.read().expect("read succeeded");

        assert!(doc.is_geometry());
        assert!(matches!(&doc, GeoJsonDocument::Geometry(Geometry::Point(_))), "Expected Point geometry");
        if let GeoJsonDocument::Geometry(Geometry::Point(p)) = doc {
            let read_lon = p.longitude().expect("has longitude");
            let read_lat = p.latitude().expect("has latitude");
            assert!((read_lon - lon).abs() < 1e-10);
            assert!((read_lat - lat).abs() < 1e-10);
        }
    }

    #[test]
    fn test_linestring_roundtrip(coords in positions_strategy(2, 20)) {
        if coords.len() < 2 {
            return Ok(());
        }

        let linestring = LineString::new(coords.clone()).expect("valid linestring");
        let geometry = Geometry::LineString(linestring);

        let mut buffer = Vec::new();
        let mut writer = GeoJsonWriter::new(&mut buffer);
        writer.write_geometry(&geometry).expect("write succeeded");
        writer.flush().expect("flush succeeded");

        let cursor = Cursor::new(buffer);
        let mut reader = GeoJsonReader::new(cursor);
        let doc = reader.read().expect("read succeeded");

        assert!(doc.is_geometry());
        assert!(matches!(&doc, GeoJsonDocument::Geometry(Geometry::LineString(_))), "Expected LineString geometry");
        if let GeoJsonDocument::Geometry(Geometry::LineString(ls)) = doc {
            assert_eq!(ls.len(), coords.len());
        }
    }

    #[test]
    fn test_multipoint_roundtrip(coords in positions_strategy(0, 50)) {
        let multipoint = MultiPoint::new(coords.clone()).expect("valid multipoint");
        let geometry = Geometry::MultiPoint(multipoint);

        let mut buffer = Vec::new();
        let mut writer = GeoJsonWriter::new(&mut buffer);
        writer.write_geometry(&geometry).expect("write succeeded");
        writer.flush().expect("flush succeeded");

        let cursor = Cursor::new(buffer);
        let mut reader = GeoJsonReader::new(cursor);
        let doc = reader.read().expect("read succeeded");

        assert!(doc.is_geometry());
        assert!(matches!(&doc, GeoJsonDocument::Geometry(Geometry::MultiPoint(_))), "Expected MultiPoint geometry");
        if let GeoJsonDocument::Geometry(Geometry::MultiPoint(mp)) = doc {
            assert_eq!(mp.len(), coords.len());
        }
    }

    #[test]
    fn test_feature_collection_size(size in 0usize..100) {
        let mut fc = FeatureCollection::with_capacity(size);

        for i in 0..size {
            let point = Point::new_2d(f64::from(i as i32), 0.0).expect("valid point");
            let geometry = Geometry::Point(point);
            fc.add_feature(Feature::new(Some(geometry), None));
        }

        assert_eq!(fc.len(), size);

        let mut buffer = Vec::new();
        let mut writer = GeoJsonWriter::new(&mut buffer);
        writer.write_feature_collection(&fc).expect("write succeeded");
        writer.flush().expect("flush succeeded");

        let cursor = Cursor::new(buffer);
        let mut reader = GeoJsonReader::new(cursor);
        let doc = reader.read().expect("read succeeded");

        assert!(matches!(&doc, GeoJsonDocument::FeatureCollection(_)), "Expected FeatureCollection");
        if let GeoJsonDocument::FeatureCollection(read_fc) = doc {
            assert_eq!(read_fc.len(), size);
        }
    }

    #[test]
    fn test_bbox_computation(lon in coord_strategy(), lat in lat_strategy()) {
        let point = Point::new_2d(lon, lat).expect("valid point");
        let bbox = point.compute_bbox();

        assert!(bbox.is_some(), "Expected bbox");
        if let Some(b) = bbox {
            assert_eq!(b.len(), 4);
            assert_eq!(b[0], lon); // min_x
            assert_eq!(b[1], lat); // min_y
            assert_eq!(b[2], lon); // max_x
            assert_eq!(b[3], lat); // max_y
        }
    }

    #[test]
    fn test_position_validation(lon in -1000.0..1000.0f64, lat in -1000.0..1000.0f64) {
        let mut validator = Validator::new();
        let pos = vec![lon, lat];

        if (-180.0..=180.0).contains(&lon) && (-90.0..=90.0).contains(&lat) {
            // Should be valid
            assert!(validator.validate_position(&pos).is_ok());
        } else {
            // Should be invalid
            assert!(validator.validate_position(&pos).is_err());
        }
    }

    #[test]
    fn test_feature_property_roundtrip(
        name in "[a-z]{1,20}",
        value in 0i32..1000
    ) {
        let point = Point::new_2d(0.0, 0.0).expect("valid point");
        let geometry = Geometry::Point(point);

        let mut props = Properties::new();
        props.insert("name".to_string(), serde_json::json!(name));
        props.insert("value".to_string(), serde_json::json!(value));

        let feature = Feature::new(Some(geometry), Some(props));

        let mut buffer = Vec::new();
        let mut writer = GeoJsonWriter::new(&mut buffer);
        writer.write_feature(&feature).expect("write succeeded");
        writer.flush().expect("flush succeeded");

        let cursor = Cursor::new(buffer);
        let mut reader = GeoJsonReader::new(cursor);
        let doc = reader.read().expect("read succeeded");

        assert!(matches!(&doc, GeoJsonDocument::Feature(_)), "Expected Feature");
        if let GeoJsonDocument::Feature(f) = doc {
            assert_eq!(f.property_count(), 2);
            if let Some(n) = f.get_property("name") {
                let n_str: Option<&str> = n.as_str();
                assert_eq!(n_str, Some(name.as_str()));
            }
            if let Some(v) = f.get_property("value") {
                let v_i64: Option<i64> = v.as_i64();
                assert_eq!(v_i64, Some(i64::from(value)));
            }
        }
    }
}

// Additional property tests for edge cases
proptest! {
    #[test]
    fn test_empty_coordinates_rejected(len in 0usize..2) {
        let coords = vec![0.0; len];
        let result = Point::new(coords);
        if len < 2 {
            assert!(result.is_err());
        } else {
            assert!(result.is_ok());
        }
    }
}
