//! Integration tests for the optional rayon-backed parallel GeoJSON parser.
//!
//! Every test here requires the `parallel` feature.  The most important
//! invariant is that the parallel path produces output identical to the
//! sequential parser when `preserve_order == true`.

#![cfg(feature = "parallel")]
#![allow(clippy::expect_used)]
#![allow(clippy::panic)]

use oxigdal_geojson_stream::parser::{FeatureCollection, GeoJsonDocument, GeoJsonParser};
use oxigdal_geojson_stream::types::{FeatureId, GeoJsonFeature, GeoJsonGeometry};
use oxigdal_geojson_stream::{
    ParallelParseOptions, parse_features_parallel, parse_features_parallel_default,
};

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Parse a `FeatureCollection` string through the *sequential* parser.
fn parse_sequential(s: &str) -> FeatureCollection {
    let doc = GeoJsonParser::new()
        .parse(s.as_bytes())
        .expect("sequential parse should succeed");
    match doc {
        GeoJsonDocument::FeatureCollection(fc) => fc,
        other => panic!("expected FeatureCollection, got {}", other.document_type()),
    }
}

/// Build a `FeatureCollection` JSON string with `n` point features whose `id`
/// equals their index and whose `properties.idx` equals their index.  The point
/// coordinate encodes the index so order is observable from geometry too.
fn make_collection(n: usize) -> String {
    let mut features = Vec::with_capacity(n);
    for i in 0..n {
        features.push(format!(
            r#"{{"type":"Feature","id":{i},"geometry":{{"type":"Point","coordinates":[{x},{y}]}},"properties":{{"idx":{i},"name":"f{i}"}}}}"#,
            i = i,
            x = i as f64,
            y = (i as f64) * 2.0
        ));
    }
    format!(
        r#"{{"type":"FeatureCollection","features":[{}]}}"#,
        features.join(",")
    )
}

/// Extract the numeric feature id (panics if absent or not numeric).
fn numeric_id(feature: &GeoJsonFeature) -> f64 {
    match &feature.id {
        Some(FeatureId::Number(n)) => *n,
        other => panic!("expected numeric id, got {other:?}"),
    }
}

// ─── 1. Empty collection ────────────────────────────────────────────────────

#[test]
fn test_parallel_parse_empty_collection() {
    let json = r#"{"type":"FeatureCollection","features":[]}"#;
    let fc = parse_features_parallel_default(json).expect("empty collection parses");
    assert!(fc.is_empty());
    assert_eq!(fc.len(), 0);
    // Must equal the sequential result exactly.
    assert_eq!(fc, parse_sequential(json));
}

// ─── 2. Single feature matches sequential ──────────────────────────────────

#[test]
fn test_parallel_parse_single_feature_matches_sequential() {
    let json = make_collection(1);
    let parallel = parse_features_parallel_default(&json).expect("parallel parse");
    let sequential = parse_sequential(&json);
    assert_eq!(parallel, sequential);
    assert_eq!(parallel.len(), 1);
}

// ─── 3. 100 features match sequential ───────────────────────────────────────

#[test]
fn test_parallel_parse_100_features_matches_sequential() {
    let json = make_collection(100);
    let parallel = parse_features_parallel_default(&json).expect("parallel parse");
    let sequential = parse_sequential(&json);
    assert_eq!(parallel.len(), 100);
    assert_eq!(parallel, sequential);
}

// ─── 4. Thread-count override ───────────────────────────────────────────────

#[test]
fn test_parallel_parse_large_collection_thread_count_override() {
    let json = make_collection(500);
    let options = ParallelParseOptions::new()
        .with_threads(Some(2))
        .with_chunk_size(64);
    let parallel = parse_features_parallel(&json, &options).expect("parallel parse");
    let sequential = parse_sequential(&json);
    assert_eq!(parallel.len(), 500);
    assert_eq!(parallel, sequential);

    // Running again with the local-pool override must not panic (no
    // build_global is used).
    let again = parse_features_parallel(&json, &options).expect("second parallel parse");
    assert_eq!(again, sequential);
}

// ─── 5. Order preserved when flag set ───────────────────────────────────────

#[test]
fn test_parallel_parse_preserves_order_when_flag_set() {
    let json = make_collection(300);
    let options = ParallelParseOptions::new()
        .with_preserve_order(true)
        .with_chunk_size(16);
    let fc = parse_features_parallel(&json, &options).expect("parallel parse");
    assert_eq!(fc.len(), 300);
    for (i, feature) in fc.features.iter().enumerate() {
        assert_eq!(numeric_id(feature), i as f64, "id mismatch at index {i}");
        let idx: i64 = feature
            .get_property("idx")
            .expect("idx property should exist");
        assert_eq!(idx, i as i64, "idx property mismatch at index {i}");
    }
}

// ─── 6. Unordered yields the same set of features ───────────────────────────

#[test]
fn test_parallel_parse_unordered_collects_same_features_as_set() {
    let json = make_collection(250);
    let options = ParallelParseOptions::new()
        .with_preserve_order(false)
        .with_chunk_size(8);
    let fc = parse_features_parallel(&json, &options).expect("parallel parse");
    assert_eq!(fc.len(), 250);

    // Compare as a sorted multiset of ids.
    let mut got_ids: Vec<i64> = fc.features.iter().map(|f| numeric_id(f) as i64).collect();
    got_ids.sort_unstable();
    let expected: Vec<i64> = (0..250).collect();
    assert_eq!(got_ids, expected);

    // The set of features must equal the sequential set (order-independent).
    let sequential = parse_sequential(&json);
    let mut got_sorted = fc.features.clone();
    let mut seq_sorted = sequential.features.clone();
    got_sorted.sort_by(|a, b| numeric_id(a).total_cmp(&numeric_id(b)));
    seq_sorted.sort_by(|a, b| numeric_id(a).total_cmp(&numeric_id(b)));
    assert_eq!(got_sorted, seq_sorted);
}

// ─── 7. Coordinate precision applies ────────────────────────────────────────

#[test]
fn test_parallel_parse_coordinate_precision_applies() {
    let json = r#"{"type":"FeatureCollection","features":[
        {"type":"Feature","geometry":{"type":"Point","coordinates":[1.23456,7.89012]},"properties":null}
    ]}"#;
    let options = ParallelParseOptions::new().with_coordinate_precision(Some(2));
    let fc = parse_features_parallel(json, &options).expect("parallel parse");
    assert_eq!(fc.len(), 1);
    let geom = fc.features[0]
        .geometry
        .as_ref()
        .expect("feature has geometry");
    match geom {
        GeoJsonGeometry::Point([x, y]) => {
            assert!((*x - 1.23).abs() < 1e-9, "x not rounded to 2 dp: {x}");
            assert!((*y - 7.89).abs() < 1e-9, "y not rounded to 2 dp: {y}");
        }
        other => panic!("expected Point, got {}", other.geometry_type()),
    }

    // Without precision, coordinates must be preserved exactly (== sequential).
    let exact = parse_features_parallel_default(json).expect("parallel parse");
    assert_eq!(exact, parse_sequential(json));
}

// ─── 8. Error propagation from a malformed feature ──────────────────────────

#[test]
fn test_parallel_parse_propagates_error_from_malformed_feature() {
    // The middle feature has a non-numeric coordinate, which is invalid.
    let json = r#"{"type":"FeatureCollection","features":[
        {"type":"Feature","geometry":{"type":"Point","coordinates":[0,0]},"properties":null},
        {"type":"Feature","geometry":{"type":"Point","coordinates":["bad",0]},"properties":null},
        {"type":"Feature","geometry":{"type":"Point","coordinates":[1,1]},"properties":null}
    ]}"#;
    let result = parse_features_parallel_default(json);
    assert!(result.is_err(), "malformed feature should produce an error");

    // The sequential parser must reject it too.
    let doc = GeoJsonParser::new().parse(json.as_bytes());
    assert!(doc.is_err(), "sequential parser should also reject it");
}

// ─── 9. Default chunk size ──────────────────────────────────────────────────

#[test]
fn test_parallel_parse_options_default_chunk_size_256() {
    let options = ParallelParseOptions::default();
    assert_eq!(options.chunk_size, 256);
    assert_eq!(options.threads, None);
    assert!(options.preserve_order);
    assert_eq!(options.coordinate_precision, None);
    // `new()` must equal `default()`.
    assert_eq!(ParallelParseOptions::new(), options);
}

// ─── 10. Builder methods ────────────────────────────────────────────────────

#[test]
fn test_parallel_parse_options_builder_methods() {
    let options = ParallelParseOptions::new()
        .with_chunk_size(32)
        .with_threads(Some(4))
        .with_preserve_order(false)
        .with_coordinate_precision(Some(3));
    assert_eq!(options.chunk_size, 32);
    assert_eq!(options.threads, Some(4));
    assert!(!options.preserve_order);
    assert_eq!(options.coordinate_precision, Some(3));
}

// ─── 11. chunk_size == 1 corner case ────────────────────────────────────────

#[test]
fn test_parallel_parse_chunk_size_one_corner_case() {
    let json = make_collection(50);
    let options = ParallelParseOptions::new().with_chunk_size(1);
    let fc = parse_features_parallel(&json, &options).expect("parallel parse");
    assert_eq!(fc.len(), 50);
    assert_eq!(fc, parse_sequential(&json));

    // chunk_size == 0 is normalised to 1 internally and must still work.
    let zero_opts = ParallelParseOptions::new().with_chunk_size(0);
    let fc0 = parse_features_parallel(&json, &zero_opts).expect("parallel parse with chunk 0");
    assert_eq!(fc0, parse_sequential(&json));
}

// ─── 12. chunk_size larger than the array ───────────────────────────────────

#[test]
fn test_parallel_parse_chunk_size_larger_than_array() {
    let json = make_collection(5);
    let options = ParallelParseOptions::new().with_chunk_size(1000);
    let fc = parse_features_parallel(&json, &options).expect("parallel parse");
    assert_eq!(fc.len(), 5);
    assert_eq!(fc, parse_sequential(&json));
}

// ─── 13. Feature id and properties preserved ────────────────────────────────

#[test]
fn test_parallel_parse_preserves_feature_id_and_properties() {
    let json = r#"{"type":"FeatureCollection","features":[
        {"type":"Feature","id":"alpha","geometry":{"type":"Point","coordinates":[3,4]},"properties":{"k":"v","n":42}},
        {"type":"Feature","id":7,"geometry":{"type":"LineString","coordinates":[[0,0],[1,1]]},"properties":{"flag":true}}
    ]}"#;
    let fc = parse_features_parallel_default(json).expect("parallel parse");
    let sequential = parse_sequential(json);
    assert_eq!(fc, sequential);

    // Spot-check the preserved id and properties.
    assert_eq!(fc.features[0].id, Some(FeatureId::String("alpha".into())));
    let k: String = fc.features[0].get_property("k").expect("k property");
    assert_eq!(k, "v");
    let n: i64 = fc.features[0].get_property("n").expect("n property");
    assert_eq!(n, 42);

    assert_eq!(fc.features[1].id, Some(FeatureId::Number(7.0)));
    let flag: bool = fc.features[1].get_property("flag").expect("flag property");
    assert!(flag);
}
