//! Tests for coordinate precision control in the GeoJSON writer.
//!
//! Verifies that `WriterConfig::with_precision(n)` correctly rounds every
//! coordinate value in the output to `n` decimal places.
#![allow(clippy::expect_used, clippy::panic)]

use oxigdal_geojson::types::{
    Feature, FeatureCollection, Geometry, LineString, MultiPoint, Point, Polygon,
};
use oxigdal_geojson::writer::{GeoJsonWriter, WriterConfig};

// ─── helpers ─────────────────────────────────────────────────────────────────

fn write_geometry_with_precision(geom: &Geometry, precision: usize) -> String {
    let mut buf = Vec::new();
    let config = WriterConfig::compact().with_precision(precision);
    let mut writer = GeoJsonWriter::with_config(&mut buf, config);
    writer.write_geometry(geom).expect("write geometry");
    writer.flush().expect("flush");
    String::from_utf8(buf).expect("valid UTF-8")
}

fn write_geometry_no_precision(geom: &Geometry) -> String {
    let mut buf = Vec::new();
    let mut writer = GeoJsonWriter::compact(&mut buf);
    writer.write_geometry(geom).expect("write geometry");
    writer.flush().expect("flush");
    String::from_utf8(buf).expect("valid UTF-8")
}

// ─── tests ───────────────────────────────────────────────────────────────────

/// precision=2 rounds 1.23456789 → 1.23 and 2.98765432 → 2.99
#[test]
fn test_coordinate_precision_2dp() {
    let point = Point::new_2d(1.23456789, 2.98765432).expect("valid point");
    let geom = Geometry::Point(point);
    let json = write_geometry_with_precision(&geom, 2);

    // Must contain the rounded values
    assert!(json.contains("1.23"), "expected 1.23 in '{json}'");
    assert!(json.contains("2.99"), "expected 2.99 in '{json}'");
    // Must NOT contain the original high-precision digits
    assert!(
        !json.contains("1.234"),
        "original digits still present in '{json}'"
    );
    assert!(
        !json.contains("2.987"),
        "original digits still present in '{json}'"
    );
}

/// precision=0 rounds all coordinates to integers
#[test]
fn test_coordinate_precision_0dp() {
    // Use 0.7 and 1.3 — they round to 1 and 1 respectively
    let point = Point::new_2d(0.7_f64, 1.3_f64).expect("valid point");
    let geom = Geometry::Point(point);
    let json = write_geometry_with_precision(&geom, 0);

    // Should contain integer representations
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    let coords = parsed["coordinates"].as_array().expect("coordinates array");
    // Each coordinate should be a whole number after rounding
    for c in coords {
        let v = c.as_f64().expect("numeric coord");
        assert_eq!(
            v,
            v.round(),
            "coordinate {v} is not an integer after precision=0 rounding"
        );
    }
}

/// No precision setting → full f64 precision is preserved (more than 4 dp)
#[test]
fn test_coordinate_precision_none() {
    // A coordinate whose exact f64 representation extends past 4 decimal places
    let lon = 1.234_567_89_f64;
    let lat = 2.987_654_32_f64;
    let point = Point::new_2d(lon, lat).expect("valid point");
    let geom = Geometry::Point(point);
    let json = write_geometry_no_precision(&geom);

    // The JSON output must contain more than 4 significant decimal-place digits
    // for at least one coordinate.
    let has_high_precision = json.contains("1.23456") || json.contains("2.98765");
    assert!(
        has_high_precision,
        "expected full precision in '{json}' when no precision is set"
    );
}

/// Polygon with precision=4 — all coordinates in every ring are rounded
#[test]
fn test_precision_roundtrip_polygon() {
    let exterior = vec![
        vec![0.123_456_789_f64, 0.0_f64],
        vec![1.0_f64, 0.123_456_789_f64],
        vec![1.123_456_789_f64, 1.123_456_789_f64],
        vec![0.0_f64, 1.0_f64],
        vec![0.123_456_789_f64, 0.0_f64],
    ];
    let poly = Polygon::from_exterior(exterior).expect("valid polygon");
    let geom = Geometry::Polygon(poly);
    let json = write_geometry_with_precision(&geom, 4);

    // Parse back and check every coordinate value has at most 4 decimal places
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    let rings = parsed["coordinates"].as_array().expect("rings");
    for ring in rings {
        for pos in ring.as_array().expect("positions") {
            for c in pos.as_array().expect("coordinates") {
                let v = c.as_f64().expect("numeric coord");
                // Round-trip: value rounded to 4dp should equal itself
                let factor = 10_f64.powi(4);
                let rounded = (v * factor).round() / factor;
                assert!(
                    (v - rounded).abs() < 1e-10,
                    "coordinate {v} has more than 4 decimal places after precision=4"
                );
            }
        }
    }
}

/// LineString with 5 points, precision=3
#[test]
fn test_precision_linestring() {
    let coords: Vec<Vec<f64>> = vec![
        vec![0.123_456_f64, 0.987_654_f64],
        vec![10.555_555_f64, 20.444_444_f64],
        vec![-1.234_567_f64, -2.345_678_f64],
        vec![5.111_111_f64, 5.999_999_f64],
        vec![0.000_001_f64, 0.000_009_f64],
    ];
    let ls = LineString::new(coords).expect("valid linestring");
    let geom = Geometry::LineString(ls);
    let json = write_geometry_with_precision(&geom, 3);

    let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    let positions = parsed["coordinates"].as_array().expect("positions");
    assert_eq!(positions.len(), 5, "expected 5 positions");

    // Every coordinate must be a multiple of 0.001
    for pos in positions {
        for c in pos.as_array().expect("coord pair") {
            let v = c.as_f64().expect("numeric");
            let factor = 1_000_f64;
            let rounded = (v * factor).round() / factor;
            assert!(
                (v - rounded).abs() < 1e-10,
                "coordinate {v} has more than 3 decimal places after precision=3"
            );
        }
    }
}

/// precision applied to a Feature (geometry inside Feature is rounded)
#[test]
fn test_precision_feature_geometry() {
    let point = Point::new_2d(45.123_456_789_f64, 90.0_f64).expect("valid point");
    let geom = Geometry::Point(point);
    let feature = Feature::new(Some(geom), None);

    let mut buf = Vec::new();
    let config = WriterConfig::compact().with_precision(2);
    let mut writer = GeoJsonWriter::with_config(&mut buf, config);
    writer.write_feature(&feature).expect("write feature");
    writer.flush().expect("flush");
    let json = String::from_utf8(buf).expect("utf8");

    assert!(
        json.contains("45.12"),
        "expected rounded coord 45.12 in '{json}'"
    );
    assert!(
        !json.contains("45.123"),
        "unrounded digits still present in '{json}'"
    );
}

/// precision applied to a FeatureCollection
#[test]
fn test_precision_feature_collection() {
    let p1 = Point::new_2d(1.111_111_f64, 2.222_222_f64).expect("p1");
    let p2 = Point::new_2d(3.333_333_f64, 4.444_444_f64).expect("p2");
    let fc = FeatureCollection::new(vec![
        Feature::new(Some(Geometry::Point(p1)), None),
        Feature::new(Some(Geometry::Point(p2)), None),
    ]);

    let mut buf = Vec::new();
    let config = WriterConfig::compact().with_precision(2);
    let mut writer = GeoJsonWriter::with_config(&mut buf, config);
    writer.write_feature_collection(&fc).expect("write fc");
    writer.flush().expect("flush");
    let json = String::from_utf8(buf).expect("utf8");

    let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    let features = parsed["features"].as_array().expect("features array");
    for feat in features {
        let coords = feat["geometry"]["coordinates"].as_array().expect("coords");
        for c in coords {
            let v = c.as_f64().expect("numeric");
            let factor = 100_f64;
            let rounded = (v * factor).round() / factor;
            assert!(
                (v - rounded).abs() < 1e-10,
                "coord {v} has more than 2 dp in feature collection output"
            );
        }
    }
}

/// MultiPoint with precision=1 — all points rounded
#[test]
fn test_precision_multipoint() {
    let coords = vec![vec![1.55_f64, 2.44_f64], vec![3.95_f64, 4.05_f64]];
    let mp = MultiPoint::new(coords).expect("valid multipoint");
    let geom = Geometry::MultiPoint(mp);
    let json = write_geometry_with_precision(&geom, 1);

    let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    let positions = parsed["coordinates"].as_array().expect("positions");
    for pos in positions {
        for c in pos.as_array().expect("coord pair") {
            let v = c.as_f64().expect("numeric");
            let factor = 10_f64;
            let rounded = (v * factor).round() / factor;
            assert!(
                (v - rounded).abs() < 1e-10,
                "multipoint coord {v} has more than 1 dp after precision=1"
            );
        }
    }
}
