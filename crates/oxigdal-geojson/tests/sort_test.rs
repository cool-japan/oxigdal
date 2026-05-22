//! Tests for the feature sorting module (sort.rs).
//!
//! All tests are pure — no I/O, no external files.

use oxigdal_geojson_stream::{
    FeatureCollection, FeatureId, FeatureSortKey, GeoJsonFeature, GeoJsonGeometry, SortOrder,
    feature_centroid, geohash_key, hilbert_key, sort_feature_collection, sort_features,
    sort_features_owned,
};

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Build a GeoJsonFeature with a Point geometry at `(lon, lat)`, an integer id,
/// and an optional properties JSON object.
fn make_feature(
    id: i64,
    props: &[(&str, serde_json::Value)],
    lon: f64,
    lat: f64,
) -> GeoJsonFeature {
    let properties = if props.is_empty() {
        None
    } else {
        let mut map = serde_json::Map::new();
        for (k, v) in props {
            map.insert((*k).to_string(), v.clone());
        }
        Some(serde_json::Value::Object(map))
    };
    GeoJsonFeature {
        id: Some(FeatureId::Number(id as f64)),
        geometry: Some(GeoJsonGeometry::Point([lon, lat])),
        properties,
    }
}

/// Build a GeoJsonFeature with no geometry (null geometry slot).
fn make_feature_no_geom(id: i64, props: &[(&str, serde_json::Value)]) -> GeoJsonFeature {
    let properties = if props.is_empty() {
        None
    } else {
        let mut map = serde_json::Map::new();
        for (k, v) in props {
            map.insert((*k).to_string(), v.clone());
        }
        Some(serde_json::Value::Object(map))
    };
    GeoJsonFeature {
        id: Some(FeatureId::Number(id as f64)),
        geometry: None,
        properties,
    }
}

// ─── Hilbert key tests ───────────────────────────────────────────────────────

/// The bottom-left corner of the world grid must map to Hilbert index 0.
#[test]
fn test_hilbert_key_origin_returns_zero() {
    let key = hilbert_key(-180.0, -90.0, 16);
    assert_eq!(key, 0, "bottom-left corner must be Hilbert index 0");
}

/// Two geographically distant points must produce different Hilbert keys.
#[test]
fn test_hilbert_key_distinct_for_distant_points() {
    // New York vs. Sydney — should differ at any reasonable precision.
    let ny = hilbert_key(-74.0, 40.7, 12);
    let syd = hilbert_key(151.2, -33.9, 12);
    assert_ne!(
        ny, syd,
        "New York and Sydney must have different Hilbert keys"
    );
}

/// Out-of-range precision values must be clamped to [1, 20] without panicking.
#[test]
fn test_hilbert_key_precision_clamping() {
    // precision = 0 clamped to 1 → 2x2 grid, bottom-left = 0
    let k0 = hilbert_key(-180.0, -90.0, 0);
    let k1 = hilbert_key(-180.0, -90.0, 1);
    assert_eq!(k0, k1, "precision 0 must clamp to 1");

    // precision = 30 clamped to 20 — must not panic
    let k30 = hilbert_key(0.0, 0.0, 30);
    let k20 = hilbert_key(0.0, 0.0, 20);
    assert_eq!(k30, k20, "precision 30 must clamp to 20");
}

// ─── Geohash key tests ────────────────────────────────────────────────────────

/// The bottom-left world corner must hash to the '0' character at precision 1.
#[test]
fn test_geohash_key_origin() {
    // Manually traced through the algorithm: 5 successive bisections starting
    // from [-180,180] and [-90,90] all yield 0-bits → BASE32[0] = '0'.
    let h = geohash_key(-180.0, -90.0, 1);
    assert_eq!(h, "0", "bottom-left corner geohash must be '0'");
}

/// San Francisco should start with "9q" (well-known geohash prefix for the
/// Bay Area; independent of the exact 5th character).
#[test]
fn test_geohash_key_known_value_sf() {
    let h = geohash_key(-122.4, 37.7, 5);
    assert_eq!(h.len(), 5, "geohash must have exactly 5 characters");
    assert!(
        h.starts_with("9q"),
        "San Francisco geohash must start with '9q', got '{h}'"
    );
}

/// The returned string length must equal the requested precision exactly.
#[test]
fn test_geohash_key_precision_truncates() {
    for prec in [1u8, 3, 6, 12] {
        let h = geohash_key(0.0, 0.0, prec);
        assert_eq!(
            h.len(),
            prec as usize,
            "geohash at precision {prec} must have {prec} characters"
        );
    }
    // Out-of-range precision is clamped: 0 → 1, 99 → 12.
    assert_eq!(geohash_key(0.0, 0.0, 0).len(), 1);
    assert_eq!(geohash_key(0.0, 0.0, 99).len(), 12);
}

// ─── Property sort tests ──────────────────────────────────────────────────────

/// Three features with string property "name" should sort ascending by value.
#[test]
fn test_sort_by_property_ascending_string() {
    let mut features = vec![
        make_feature(1, &[("name", serde_json::json!("b"))], 0.0, 0.0),
        make_feature(2, &[("name", serde_json::json!("a"))], 1.0, 1.0),
        make_feature(3, &[("name", serde_json::json!("c"))], 2.0, 2.0),
    ];
    sort_features(
        &mut features,
        &FeatureSortKey::Property("name".into()),
        SortOrder::Ascending,
    )
    .expect("sort must succeed");

    let names: Vec<&str> = features
        .iter()
        .map(|f| {
            f.properties
                .as_ref()
                .and_then(|p| p.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
        })
        .collect();
    assert_eq!(names, vec!["a", "b", "c"]);
}

/// Features with a numeric property should sort descending correctly.
#[test]
fn test_sort_by_property_descending_numeric() {
    let mut features = vec![
        make_feature(1, &[("score", serde_json::json!(1.0))], 0.0, 0.0),
        make_feature(2, &[("score", serde_json::json!(3.0))], 1.0, 1.0),
        make_feature(3, &[("score", serde_json::json!(2.0))], 2.0, 2.0),
    ];
    sort_features(
        &mut features,
        &FeatureSortKey::Property("score".into()),
        SortOrder::Descending,
    )
    .expect("sort must succeed");

    let scores: Vec<f64> = features
        .iter()
        .map(|f| {
            f.properties
                .as_ref()
                .and_then(|p| p.get("score"))
                .and_then(|v| v.as_f64())
                .unwrap_or(f64::NAN)
        })
        .collect();
    assert_eq!(scores, vec![3.0, 2.0, 1.0]);
}

/// A feature missing the sort property must appear after those that have it.
#[test]
fn test_sort_by_property_missing_field_sorts_to_end() {
    let mut features = vec![
        make_feature_no_geom(1, &[]), // no property at all
        make_feature(2, &[("rank", serde_json::json!(99))], 0.0, 0.0), // has "rank"
        make_feature(3, &[("other", serde_json::json!("x"))], 1.0, 0.0), // wrong key
        make_feature(4, &[("rank", serde_json::json!(1))], 2.0, 0.0), // has "rank"
    ];
    sort_features(
        &mut features,
        &FeatureSortKey::Property("rank".into()),
        SortOrder::Ascending,
    )
    .expect("sort must succeed");

    // First two must have the "rank" property; last two must not.
    let has_rank: Vec<bool> = features
        .iter()
        .map(|f| f.properties.as_ref().and_then(|p| p.get("rank")).is_some())
        .collect();
    assert_eq!(has_rank, vec![true, true, false, false]);
    // The first ranked feature should have rank=1 (ascending).
    let first_rank = features[0]
        .properties
        .as_ref()
        .and_then(|p| p.get("rank"))
        .and_then(|v| v.as_f64())
        .expect("first feature must have rank");
    assert_eq!(first_rank, 1.0);
}

// ─── Hilbert spatial sort tests ───────────────────────────────────────────────

/// Two points that are very close together must have identical or adjacent
/// Hilbert indices at high precision, indicating spatial locality.
#[test]
fn test_sort_by_hilbert_clusters_spatially_close_features() {
    let close_a = make_feature(1, &[], 0.0, 0.0);
    let close_b = make_feature(2, &[], 0.001, 0.001);
    let far_away = make_feature(3, &[], 120.0, 60.0);

    let ka = hilbert_key(0.0, 0.0, 20);
    let kb = hilbert_key(0.001, 0.001, 20);
    let kf = hilbert_key(120.0, 60.0, 20);

    // The two close points differ by at most a tiny delta compared to far.
    let close_delta = (ka as i128 - kb as i128).unsigned_abs();
    let far_delta_a = (ka as i128 - kf as i128).unsigned_abs();
    assert!(
        close_delta < far_delta_a,
        "close points must have nearer Hilbert indices than distant ones: \
         close_delta={close_delta}, far_delta={far_delta_a}"
    );

    // Sorting must not panic and must produce a deterministic result.
    let mut features = vec![far_away, close_b, close_a];
    sort_features(
        &mut features,
        &FeatureSortKey::Hilbert { precision: 16 },
        SortOrder::Ascending,
    )
    .expect("Hilbert sort must succeed");
    assert_eq!(features.len(), 3);
}

// ─── Geohash spatial sort tests ───────────────────────────────────────────────

/// After a geohash sort, features that share a geohash prefix must be
/// consecutive — i.e. spatially close features must end up grouped together.
#[test]
fn test_sort_by_geohash_lex_order_clusters_spatially() {
    // Three points near London, one point near Tokyo.
    let mut features = vec![
        make_feature(1, &[], -0.12, 51.51), // London
        make_feature(2, &[], 139.7, 35.7),  // Tokyo
        make_feature(3, &[], -0.10, 51.50), // near London
        make_feature(4, &[], -0.13, 51.52), // near London
    ];

    sort_features(
        &mut features,
        &FeatureSortKey::Geohash { precision: 6 },
        SortOrder::Ascending,
    )
    .expect("geohash sort must succeed");

    // Build the geohash strings for the sorted result.
    let hashes: Vec<String> = features
        .iter()
        .map(|f| {
            let (lon, lat) = feature_centroid(f).expect("all features have geometry");
            geohash_key(lon, lat, 6)
        })
        .collect();

    // Verify that the hashes are in non-decreasing lexicographic order.
    for pair in hashes.windows(2) {
        assert!(
            pair[0] <= pair[1],
            "geohash sort must yield non-decreasing order: '{}' > '{}'",
            pair[0],
            pair[1]
        );
    }

    // Tokyo's geohash prefix starts with a different character than London's.
    // Ensure Tokyo (xn*) is separated from London (gcpv*) in the sorted output.
    let tokyo_pos = features
        .iter()
        .position(|f| {
            f.id == Some(FeatureId::Number(2.0)) // id=2 = Tokyo
        })
        .expect("Tokyo must be in the sorted vec");
    // All three London features must appear before or after Tokyo, not mixed.
    let london_positions: Vec<usize> = features
        .iter()
        .enumerate()
        .filter(|(_, f)| f.id != Some(FeatureId::Number(2.0)))
        .map(|(i, _)| i)
        .collect();
    // Tokyo must be either before all London features or after all of them.
    let all_before = london_positions.iter().all(|&p| p > tokyo_pos);
    let all_after = london_positions.iter().all(|&p| p < tokyo_pos);
    assert!(
        all_before || all_after,
        "Tokyo must cluster separately from the three London points"
    );
}

// ─── FeatureCollection sort tests ─────────────────────────────────────────────

/// `sort_feature_collection` must sort features while preserving all metadata.
#[test]
fn test_sort_feature_collection_preserves_metadata() {
    let mut fc = FeatureCollection {
        features: vec![
            make_feature(3, &[("v", serde_json::json!(3))], 2.0, 0.0),
            make_feature(1, &[("v", serde_json::json!(1))], 0.0, 0.0),
            make_feature(2, &[("v", serde_json::json!(2))], 1.0, 0.0),
        ],
        bbox: Some([0.0, 0.0, 10.0, 10.0]),
        bbox_3d: None,
        crs: None,
        name: Some("test-collection".to_string()),
    };

    sort_feature_collection(
        &mut fc,
        &FeatureSortKey::Property("v".into()),
        SortOrder::Ascending,
    )
    .expect("sort must succeed");

    // Feature count must be preserved.
    assert_eq!(fc.features.len(), 3);
    // Metadata must be untouched.
    assert_eq!(fc.bbox, Some([0.0, 0.0, 10.0, 10.0]));
    assert_eq!(fc.name.as_deref(), Some("test-collection"));
    // Features must be in ascending order by "v".
    let vals: Vec<f64> = fc
        .features
        .iter()
        .map(|f| {
            f.properties
                .as_ref()
                .and_then(|p| p.get("v"))
                .and_then(|v| v.as_f64())
                .unwrap_or(f64::NAN)
        })
        .collect();
    assert_eq!(vals, vec![1.0, 2.0, 3.0]);
}

// ─── Centroid test ────────────────────────────────────────────────────────────

/// A polygon feature must yield a non-None centroid via `feature_centroid`.
#[test]
fn test_feature_centroid_polygon_returns_value() {
    let feature = GeoJsonFeature {
        id: None,
        geometry: Some(GeoJsonGeometry::Polygon(vec![vec![
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 10.0],
            [0.0, 10.0],
            [0.0, 0.0],
        ]])),
        properties: None,
    };

    let centroid = feature_centroid(&feature);
    assert!(centroid.is_some(), "polygon must have a centroid");
    let (cx, cy) = centroid.expect("just checked");
    // Centroid of a 10×10 square at origin is (5, 5).
    assert!((cx - 5.0).abs() < 1e-9, "centroid x must be 5.0, got {cx}");
    assert!((cy - 5.0).abs() < 1e-9, "centroid y must be 5.0, got {cy}");
}

// ─── sort_features_owned test ─────────────────────────────────────────────────

/// `sort_features_owned` must return a vec with the same count in sorted order.
#[test]
fn test_sort_features_owned_returns_sorted_vec() {
    let features = vec![
        make_feature(1, &[("z", serde_json::json!("gamma"))], 0.0, 0.0),
        make_feature(2, &[("z", serde_json::json!("alpha"))], 1.0, 1.0),
        make_feature(3, &[("z", serde_json::json!("beta"))], 2.0, 2.0),
        make_feature(4, &[("z", serde_json::json!("delta"))], 3.0, 3.0),
    ];

    let sorted = sort_features_owned(
        features,
        &FeatureSortKey::Property("z".into()),
        SortOrder::Ascending,
    )
    .expect("sort must succeed");

    assert_eq!(sorted.len(), 4, "must return all 4 features");

    let zvals: Vec<&str> = sorted
        .iter()
        .map(|f| {
            f.properties
                .as_ref()
                .and_then(|p| p.get("z"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
        })
        .collect();
    assert_eq!(zvals, vec!["alpha", "beta", "delta", "gamma"]);
}
