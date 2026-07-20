//! Integration tests for [`IncrementalFeatureReader`].

use oxigeo_geojson_stream::IncrementalFeatureReader;
use std::io::Cursor;

fn make_collection(n: usize) -> Vec<u8> {
    let features: Vec<String> = (0..n)
        .map(|i| {
            format!(
                r#"{{"type":"Feature","id":{i},"geometry":{{"type":"Point","coordinates":[{i}.0,0.0]}},"properties":{{"name":"feat{i}"}}}}"#
            )
        })
        .collect();
    format!(
        r#"{{"type":"FeatureCollection","features":[{}]}}"#,
        features.join(",")
    )
    .into_bytes()
}

#[test]
fn test_incremental_reads_100_features_one_at_a_time() {
    let data = make_collection(100);
    let reader = IncrementalFeatureReader::new(Cursor::new(data)).expect("new");
    let count = reader.fold(0usize, |acc, r| {
        r.expect("feature");
        acc + 1
    });
    assert_eq!(count, 100);
}

#[test]
fn test_incremental_preserves_id_and_properties() {
    let data = br#"{"type":"FeatureCollection","features":[
        {"type":"Feature","id":42,"geometry":{"type":"Point","coordinates":[1.0,2.0]},"properties":{"city":"Tokyo"}}
    ]}"#;
    let mut reader = IncrementalFeatureReader::new(Cursor::new(&data[..])).expect("new");
    let feat = reader.next().expect("first feature").expect("no error");
    assert!(feat.properties.is_some());
    let city: String = feat.get_property("city").expect("has city property");
    assert_eq!(city, "Tokyo");
}

#[test]
fn test_incremental_empty_features_array() {
    let data = br#"{"type":"FeatureCollection","features":[]}"#;
    let reader = IncrementalFeatureReader::new(Cursor::new(&data[..])).expect("new");
    assert_eq!(reader.count(), 0);
}

#[test]
fn test_incremental_errors_on_malformed_feature() {
    // Truncated JSON inside the features array.
    let data = br#"{"type":"FeatureCollection","features":[{"type":"Fe"#;
    let mut reader = IncrementalFeatureReader::new(Cursor::new(&data[..])).expect("new");
    let result = reader.next();
    assert!(result.is_some());
    assert!(result.expect("Some").is_err());
}

#[test]
fn test_incremental_skips_unknown_top_level_fields() {
    let data = br#"{"name":"test","bbox":[0,0,1,1],"type":"FeatureCollection","features":[
        {"type":"Feature","geometry":{"type":"Point","coordinates":[0.0,0.0]},"properties":null}
    ]}"#;
    let reader = IncrementalFeatureReader::new(Cursor::new(&data[..])).expect("new");
    assert_eq!(reader.count(), 1);
}

#[test]
fn test_incremental_multiple_features_ordering() {
    // Verify that features arrive in document order.
    let data = make_collection(10);
    let reader = IncrementalFeatureReader::new(Cursor::new(data)).expect("new");
    let features: Vec<_> = reader.map(|r| r.expect("valid")).collect();
    assert_eq!(features.len(), 10);
    // Each feature should have a sequential id.
    for (i, feat) in features.iter().enumerate() {
        if let Some(oxigeo_geojson_stream::FeatureId::Number(n)) = &feat.id {
            assert!(
                (n - i as f64).abs() < f64::EPSILON,
                "feature {i} had wrong id {n}"
            );
        }
    }
}

#[test]
fn test_incremental_null_properties() {
    let data = br#"{"type":"FeatureCollection","features":[
        {"type":"Feature","geometry":{"type":"Point","coordinates":[0.0,0.0]},"properties":null}
    ]}"#;
    let mut reader = IncrementalFeatureReader::new(Cursor::new(&data[..])).expect("new");
    let feat = reader.next().expect("first").expect("valid");
    // When "properties" is explicitly set to JSON null, the parser returns
    // Some(Value::Null) (the key exists with a null value).
    let props = &feat.properties;
    assert!(
        props.is_none() || props.as_ref().map(|v| v.is_null()).unwrap_or(false),
        "expected None or Some(null) for null properties, got {:?}",
        props
    );
}

#[test]
fn test_incremental_geometry_types() {
    let data = br#"{"type":"FeatureCollection","features":[
        {"type":"Feature","geometry":{"type":"LineString","coordinates":[[0.0,0.0],[1.0,1.0]]},"properties":null},
        {"type":"Feature","geometry":{"type":"Polygon","coordinates":[[[0.0,0.0],[1.0,0.0],[1.0,1.0],[0.0,1.0],[0.0,0.0]]]},"properties":null}
    ]}"#;
    let reader = IncrementalFeatureReader::new(Cursor::new(&data[..])).expect("new");
    let features: Vec<_> = reader.map(|r| r.expect("valid")).collect();
    assert_eq!(features.len(), 2);
    assert!(features[0].has_geometry());
    assert!(features[1].has_geometry());
}

#[test]
fn test_incremental_features_key_first() {
    // "features" appears before "type" — valid per RFC 7946 (keys may be in any order).
    let data = br#"{"features":[
        {"type":"Feature","geometry":{"type":"Point","coordinates":[5.0,5.0]},"properties":null}
    ],"type":"FeatureCollection"}"#;
    let reader = IncrementalFeatureReader::new(Cursor::new(&data[..])).expect("new");
    assert_eq!(reader.count(), 1);
}

#[test]
fn test_incremental_no_features_key_is_error() {
    let data = br#"{"type":"FeatureCollection"}"#;
    let result = IncrementalFeatureReader::new(Cursor::new(&data[..]));
    assert!(result.is_err());
}
