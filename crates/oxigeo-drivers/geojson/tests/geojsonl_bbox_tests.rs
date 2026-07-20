//! Integration tests for GeoJSONL (newline-delimited GeoJSON) support and
//! spatial bounding-box filtering.

#![allow(clippy::panic)]
#![allow(clippy::expect_used)]

use oxigeo_geojson::reader::GeoJsonDocument;
use oxigeo_geojson::types::*;
use oxigeo_geojson::{
    GeoJsonReader, GeoJsonWriter, feature_bbox_intersects, features_in_bbox, geometry_bbox, open,
    open_geojsonl, read_geojsonl, write_geojsonl, write_geojsonl_to_file,
};
use std::io::Cursor;

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Build a simple `Feature` containing a 2-D Point at `(lon, lat)`.
fn point_feature(lon: f64, lat: f64) -> Feature {
    let geom = Geometry::Point(Point::new_2d(lon, lat).expect("valid point"));
    Feature::new(Some(geom), None)
}

// ─── Test 1: roundtrip ───────────────────────────────────────────────────────

#[test]
fn test_geojsonl_roundtrip() {
    let features: Vec<Feature> = (0..5)
        .map(|i| point_feature(f64::from(i), f64::from(i)))
        .collect();

    // Write to in-memory buffer
    let mut buf = Vec::new();
    write_geojsonl(&mut buf, features.clone()).expect("write succeeded");

    // Read back
    let cursor = Cursor::new(buf);
    let result = read_geojsonl(cursor).expect("read succeeded");

    assert_eq!(result.len(), 5, "should have 5 features");

    // First feature geometry should match
    if let Some(Geometry::Point(p)) = &result[0].geometry {
        assert_eq!(p.longitude(), Some(0.0));
        assert_eq!(p.latitude(), Some(0.0));
    } else {
        panic!("first feature must be a Point");
    }
}

// ─── Test 2: blank lines skipped ─────────────────────────────────────────────

#[test]
fn test_geojsonl_blank_lines_skipped() {
    let f1 = serde_json::to_string(&point_feature(1.0, 1.0)).expect("serialise");
    let f2 = serde_json::to_string(&point_feature(2.0, 2.0)).expect("serialise");
    // Intersperse blank lines and a comment line
    let content = format!("{f1}\n\n// this is a comment\n{f2}\n\n");

    let cursor = Cursor::new(content.as_bytes());
    let result = read_geojsonl(cursor).expect("read succeeded");

    assert_eq!(result.len(), 2, "blank lines and comments must be skipped");
}

// ─── Test 3: invalid line returns error ──────────────────────────────────────

#[test]
fn test_geojsonl_invalid_line_returns_error() {
    let f1 = serde_json::to_string(&point_feature(1.0, 1.0)).expect("serialise");
    let content = format!("{f1}\nnot valid json\n");

    let cursor = Cursor::new(content.as_bytes());
    let result = read_geojsonl(cursor);

    assert!(result.is_err(), "invalid JSON line must produce an error");
    let err_str = result.err().map(|e| e.to_string()).unwrap_or_default();
    // The error message must mention the line number
    assert!(
        err_str.contains("line 2") || err_str.contains('2'),
        "error should reference line 2; got: {err_str}"
    );
}

// ─── Test 4: bbox filter returns matching features ────────────────────────────

#[test]
fn test_bbox_filter_returns_matching() {
    // 3×4 grid of points spanning lon [0,2], lat [0,3]
    let fc_json = {
        let mut fc = FeatureCollection::empty();
        for lon in 0..3i32 {
            for lat in 0..4i32 {
                fc.add_feature(point_feature(f64::from(lon), f64::from(lat)));
            }
        }
        serde_json::to_string(&fc).expect("serialise")
    };

    let cursor = Cursor::new(fc_json.as_bytes());
    let mut reader = GeoJsonReader::new(cursor);

    // Query: lon [0,1], lat [0,1]  →  covers (0,0),(1,0),(0,1),(1,1) = 4 points
    let result = features_in_bbox(&mut reader, 0.0, 0.0, 1.0, 1.0).expect("filter succeeded");

    assert_eq!(result.len(), 4, "should return 4 matching features");
}

// ─── Test 5: bbox filter returns empty result ─────────────────────────────────

#[test]
fn test_bbox_filter_empty_result() {
    let fc_json = {
        let mut fc = FeatureCollection::empty();
        for i in 0..5i32 {
            fc.add_feature(point_feature(f64::from(i), f64::from(i)));
        }
        serde_json::to_string(&fc).expect("serialise")
    };

    let cursor = Cursor::new(fc_json.as_bytes());
    let mut reader = GeoJsonReader::new(cursor);

    // Query bbox is completely outside all features
    let result = features_in_bbox(&mut reader, 50.0, 50.0, 60.0, 60.0).expect("filter succeeded");

    assert!(result.is_empty(), "no features should match outside bbox");
}

// ─── Test 6: auto-detection via `open` ───────────────────────────────────────

#[test]
fn test_geojsonl_auto_detected() {
    let features: Vec<Feature> = (0..3)
        .map(|i| point_feature(f64::from(i), f64::from(i)))
        .collect();

    let tmp = std::env::temp_dir().join("test_auto_detect.geojsonl");

    write_geojsonl_to_file(&tmp, features).expect("write succeeded");

    let doc = open(&tmp).expect("open succeeded");

    match doc {
        GeoJsonDocument::FeatureCollection(fc) => {
            assert_eq!(
                fc.len(),
                3,
                "auto-detected geojsonl should yield 3 features"
            );
        }
        other => panic!("expected FeatureCollection, got {other:?}"),
    }

    // Clean up
    let _ = std::fs::remove_file(&tmp);
}

// ─── Test 7: geometry_bbox for Point ─────────────────────────────────────────

#[test]
fn test_geometry_bbox_point() {
    let geom = Geometry::Point(Point::new_2d(10.0, 20.0).expect("valid point"));
    let bbox = geometry_bbox(&geom);
    assert!(bbox.is_some(), "Point must have a bbox");
    let (min_x, min_y, max_x, max_y) = bbox.expect("bbox");
    assert_eq!((min_x, min_y, max_x, max_y), (10.0, 20.0, 10.0, 20.0));
}

// ─── Test 8: geometry_bbox for Polygon ───────────────────────────────────────

#[test]
fn test_geometry_bbox_polygon() {
    let exterior = vec![
        vec![0.0, 0.0],
        vec![5.0, 0.0],
        vec![5.0, 3.0],
        vec![0.0, 3.0],
        vec![0.0, 0.0],
    ];
    let geom = Geometry::Polygon(Polygon::from_exterior(exterior).expect("valid polygon"));
    let bbox = geometry_bbox(&geom);
    assert!(bbox.is_some(), "Polygon must have a bbox");
    let (min_x, min_y, max_x, max_y) = bbox.expect("bbox");
    assert_eq!((min_x, min_y, max_x, max_y), (0.0, 0.0, 5.0, 3.0));
}

// ─── Test 9: bbox filter with MultiPoint geometry ────────────────────────────

#[test]
fn test_bbox_filter_multipoint() {
    // Build a FeatureCollection with MultiPoint features
    let coords_in = vec![vec![1.0, 1.0], vec![2.0, 2.0]]; // inside [0,3]x[0,3]
    let coords_out = vec![vec![10.0, 10.0], vec![11.0, 11.0]]; // outside

    let feature_in = Feature::new(
        Some(Geometry::MultiPoint(
            MultiPoint::new(coords_in).expect("valid multipoint"),
        )),
        None,
    );
    let feature_out = Feature::new(
        Some(Geometry::MultiPoint(
            MultiPoint::new(coords_out).expect("valid multipoint"),
        )),
        None,
    );

    let fc = FeatureCollection::new(vec![feature_in, feature_out]);
    let fc_json = serde_json::to_string(&fc).expect("serialise");

    let cursor = Cursor::new(fc_json.as_bytes());
    let mut reader = GeoJsonReader::new(cursor);

    let result = features_in_bbox(&mut reader, 0.0, 0.0, 3.0, 3.0).expect("filter succeeded");

    assert_eq!(
        result.len(),
        1,
        "only the in-bbox MultiPoint feature should be returned"
    );
    match &result[0].geometry {
        Some(Geometry::MultiPoint(_)) => {}
        other => panic!("expected MultiPoint, got {other:?}"),
    }
}

// ─── Test 10: feature_bbox_intersects helper ──────────────────────────────────

#[test]
fn test_feature_bbox_intersects_edge_cases() {
    // Touching edge: geom bbox [0,0,1,1], query [1,1,2,2] — touching corner
    let geom = Geometry::Point(Point::new_2d(1.0, 1.0).expect("point"));
    assert!(
        feature_bbox_intersects(&geom, 1.0, 1.0, 2.0, 2.0),
        "touching corner must intersect"
    );

    // Clearly outside
    let geom2 = Geometry::Point(Point::new_2d(5.0, 5.0).expect("point"));
    assert!(
        !feature_bbox_intersects(&geom2, 0.0, 0.0, 1.0, 1.0),
        "clearly outside must not intersect"
    );
}

// ─── Test 11: write_geojsonl_to_file + open_geojsonl roundtrip ────────────────

#[test]
fn test_geojsonl_file_roundtrip() {
    let features: Vec<Feature> = (0..4)
        .map(|i| point_feature(f64::from(i) * 10.0, f64::from(i) * 5.0))
        .collect();

    let tmp = std::env::temp_dir().join("test_file_roundtrip.ndjson");

    write_geojsonl_to_file(&tmp, features).expect("write succeeded");
    let result = open_geojsonl(&tmp).expect("read succeeded");

    assert_eq!(result.len(), 4);

    if let Some(Geometry::Point(p)) = &result[2].geometry {
        assert!((p.longitude().expect("lon") - 20.0).abs() < f64::EPSILON);
        assert!((p.latitude().expect("lat") - 10.0).abs() < f64::EPSILON);
    } else {
        panic!("feature 2 must be a Point");
    }

    let _ = std::fs::remove_file(&tmp);
}

// ─── Test 12: GeoJsonWriter streaming + geojsonl through same types ───────────

#[test]
fn test_geojsonl_write_read_with_writer_struct() {
    let features: Vec<Feature> = (0..3).map(|i| point_feature(f64::from(i), 0.0)).collect();

    // Use GeoJsonWriter for standard output to show both APIs co-exist
    let mut std_buf = Vec::new();
    {
        let mut w = GeoJsonWriter::new(&mut std_buf);
        w.write_features(features.clone()).expect("write_features");
    }

    // Use write_geojsonl for newline-delimited
    let mut nd_buf = Vec::new();
    write_geojsonl(&mut nd_buf, features.clone()).expect("write_geojsonl");

    let lines: Vec<&str> = std::str::from_utf8(&nd_buf)
        .expect("utf8")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();
    assert_eq!(lines.len(), 3, "should have 3 newline-delimited features");

    // Each line must be valid JSON with type=Feature
    for line in &lines {
        let val: serde_json::Value = serde_json::from_str(line).expect("valid json line");
        assert_eq!(val["type"], "Feature");
    }
}
