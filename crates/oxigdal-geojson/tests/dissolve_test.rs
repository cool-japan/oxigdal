//! Integration tests for feature dissolve / merge-by-property.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use oxigdal_geojson_stream::{
    DissolveOptions, DissolveStrategy, FeatureCollection, GeoJsonFeature, GeoJsonGeometry,
    PropertyAggregator, dissolve_feature_collection, dissolve_features,
};
use serde_json::{Value, json};

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Build a Point feature with a `"group"` key and any extra properties from `extra`.
fn make_point_feature(group: &str, extra: serde_json::Map<String, Value>) -> GeoJsonFeature {
    let mut props = extra;
    props.insert("group".to_string(), Value::String(group.to_string()));
    GeoJsonFeature {
        id: None,
        geometry: Some(GeoJsonGeometry::Point([0.0, 0.0])),
        properties: Some(Value::Object(props)),
    }
}

/// Build a Polygon feature (unit square) with a `"group"` key.
fn make_polygon_feature(group: &str) -> GeoJsonFeature {
    let ring = vec![
        [0.0_f64, 0.0],
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 1.0],
        [0.0, 0.0],
    ];
    GeoJsonFeature {
        id: None,
        geometry: Some(GeoJsonGeometry::Polygon(vec![ring])),
        properties: Some(json!({ "group": group })),
    }
}

/// Dissolve options that group by `"group"` with a given default aggregator.
fn opts_default(default_agg: PropertyAggregator) -> DissolveOptions {
    DissolveOptions {
        group_by: "group".to_string(),
        strategy: DissolveStrategy::Multipart,
        property_aggregators: std::collections::HashMap::new(),
        default_aggregator: default_agg,
    }
}

// ─── Geometry merging ─────────────────────────────────────────────────────────

#[test]
fn test_dissolve_two_points_same_property_returns_multipoint() {
    let features = vec![
        make_point_feature("a", serde_json::Map::new()),
        make_point_feature("a", serde_json::Map::new()),
    ];
    let opts = opts_default(PropertyAggregator::Drop);
    let (output, stats) = dissolve_features(&features, &opts).expect("dissolve ok");

    assert_eq!(stats.input_features, 2);
    assert_eq!(stats.output_features, 1);
    assert_eq!(stats.groups_formed, 1);
    assert_eq!(stats.features_with_null_group, 0);

    let geom = output[0].geometry.as_ref().expect("geometry present");
    assert!(
        matches!(geom, GeoJsonGeometry::MultiPoint(pts) if pts.len() == 2),
        "expected MultiPoint with 2 points, got {geom:?}"
    );
}

#[test]
fn test_dissolve_three_polygons_same_property_returns_multipolygon() {
    let features = vec![
        make_polygon_feature("b"),
        make_polygon_feature("b"),
        make_polygon_feature("b"),
    ];
    let opts = opts_default(PropertyAggregator::Drop);
    let (output, stats) = dissolve_features(&features, &opts).expect("dissolve ok");

    assert_eq!(stats.output_features, 1);
    assert_eq!(stats.groups_formed, 1);

    let geom = output[0].geometry.as_ref().expect("geometry present");
    assert!(
        matches!(geom, GeoJsonGeometry::MultiPolygon(polys) if polys.len() == 3),
        "expected MultiPolygon with 3 parts, got {geom:?}"
    );
}

// ─── Null group filtering ─────────────────────────────────────────────────────

#[test]
fn test_dissolve_features_with_null_group_dropped() {
    let null_feature = GeoJsonFeature {
        id: None,
        geometry: Some(GeoJsonGeometry::Point([1.0, 1.0])),
        properties: Some(json!({ "group": null })),
    };
    let normal = make_point_feature("x", serde_json::Map::new());
    let features = vec![null_feature, normal];
    let opts = opts_default(PropertyAggregator::Drop);
    let (output, _stats) = dissolve_features(&features, &opts).expect("dissolve ok");

    // Only the "x" group should appear.
    assert_eq!(output.len(), 1);
}

#[test]
fn test_dissolve_features_with_null_group_counted_in_stats() {
    let missing_group = GeoJsonFeature {
        id: None,
        geometry: Some(GeoJsonGeometry::Point([2.0, 2.0])),
        properties: Some(json!({})), // no "group" key at all
    };
    let null_group = GeoJsonFeature {
        id: None,
        geometry: Some(GeoJsonGeometry::Point([3.0, 3.0])),
        properties: Some(json!({ "group": null })),
    };
    let normal = make_point_feature("y", serde_json::Map::new());
    let features = vec![missing_group, null_group, normal];
    let opts = opts_default(PropertyAggregator::Drop);
    let (_output, stats) = dissolve_features(&features, &opts).expect("dissolve ok");

    assert_eq!(stats.features_with_null_group, 2);
    assert_eq!(stats.output_features, 1);
}

// ─── Multiple groups ──────────────────────────────────────────────────────────

#[test]
fn test_dissolve_three_groups_returns_three_features() {
    let features = vec![
        make_point_feature("alpha", serde_json::Map::new()),
        make_point_feature("beta", serde_json::Map::new()),
        make_point_feature("gamma", serde_json::Map::new()),
        make_point_feature("alpha", serde_json::Map::new()),
    ];
    let opts = opts_default(PropertyAggregator::Drop);
    let (output, stats) = dissolve_features(&features, &opts).expect("dissolve ok");

    assert_eq!(stats.groups_formed, 3);
    assert_eq!(output.len(), 3);
}

// ─── Property aggregation ─────────────────────────────────────────────────────

fn numeric_features(values: &[f64]) -> Vec<GeoJsonFeature> {
    values
        .iter()
        .map(|&v| {
            let mut m = serde_json::Map::new();
            m.insert("value".to_string(), json!(v));
            make_point_feature("g", m)
        })
        .collect()
}

#[test]
fn test_aggregate_property_sum() {
    let features = numeric_features(&[1.0, 2.0, 3.0]);
    let mut aggs = std::collections::HashMap::new();
    aggs.insert("value".to_string(), PropertyAggregator::Sum);
    let opts = DissolveOptions {
        group_by: "group".to_string(),
        strategy: DissolveStrategy::Multipart,
        property_aggregators: aggs,
        default_aggregator: PropertyAggregator::Drop,
    };
    let (output, _) = dissolve_features(&features, &opts).expect("dissolve ok");
    let props = output[0].properties.as_ref().expect("props");
    let v = props.get("value").expect("value key");
    assert_eq!(v.as_f64().expect("f64"), 6.0);
}

#[test]
fn test_aggregate_property_mean() {
    let features = numeric_features(&[10.0, 20.0, 30.0]);
    let mut aggs = std::collections::HashMap::new();
    aggs.insert("value".to_string(), PropertyAggregator::Mean);
    let opts = DissolveOptions {
        group_by: "group".to_string(),
        strategy: DissolveStrategy::Multipart,
        property_aggregators: aggs,
        default_aggregator: PropertyAggregator::Drop,
    };
    let (output, _) = dissolve_features(&features, &opts).expect("dissolve ok");
    let props = output[0].properties.as_ref().expect("props");
    let v = props.get("value").expect("value key");
    assert!((v.as_f64().expect("f64") - 20.0).abs() < 1e-10);
}

#[test]
fn test_aggregate_property_min_max() {
    let features = numeric_features(&[5.0, 1.0, 9.0, 3.0]);

    // Min
    let mut aggs_min = std::collections::HashMap::new();
    aggs_min.insert("value".to_string(), PropertyAggregator::Min);
    let opts_min = DissolveOptions {
        group_by: "group".to_string(),
        strategy: DissolveStrategy::Multipart,
        property_aggregators: aggs_min,
        default_aggregator: PropertyAggregator::Drop,
    };
    let (out_min, _) = dissolve_features(&features, &opts_min).expect("dissolve ok");
    let min_val = out_min[0]
        .properties
        .as_ref()
        .expect("props")
        .get("value")
        .expect("value")
        .as_f64()
        .expect("f64");
    assert_eq!(min_val, 1.0);

    // Max
    let mut aggs_max = std::collections::HashMap::new();
    aggs_max.insert("value".to_string(), PropertyAggregator::Max);
    let opts_max = DissolveOptions {
        group_by: "group".to_string(),
        strategy: DissolveStrategy::Multipart,
        property_aggregators: aggs_max,
        default_aggregator: PropertyAggregator::Drop,
    };
    let (out_max, _) = dissolve_features(&features, &opts_max).expect("dissolve ok");
    let max_val = out_max[0]
        .properties
        .as_ref()
        .expect("props")
        .get("value")
        .expect("value")
        .as_f64()
        .expect("f64");
    assert_eq!(max_val, 9.0);
}

#[test]
fn test_aggregate_property_concat_with_separator() {
    let features: Vec<GeoJsonFeature> = ["foo", "bar", "baz"]
        .iter()
        .map(|s| {
            let mut m = serde_json::Map::new();
            m.insert("label".to_string(), json!(s));
            make_point_feature("g", m)
        })
        .collect();

    let mut aggs = std::collections::HashMap::new();
    aggs.insert(
        "label".to_string(),
        PropertyAggregator::Concat {
            sep: "|".to_string(),
        },
    );
    let opts = DissolveOptions {
        group_by: "group".to_string(),
        strategy: DissolveStrategy::Multipart,
        property_aggregators: aggs,
        default_aggregator: PropertyAggregator::Drop,
    };
    let (output, _) = dissolve_features(&features, &opts).expect("dissolve ok");
    let props = output[0].properties.as_ref().expect("props");
    let v = props.get("label").expect("label key");
    assert_eq!(v.as_str().expect("str"), "foo|bar|baz");
}

#[test]
fn test_aggregate_property_keep_if_same_returns_value() {
    let features: Vec<GeoJsonFeature> = ["same", "same", "same"]
        .iter()
        .map(|s| {
            let mut m = serde_json::Map::new();
            m.insert("status".to_string(), json!(s));
            make_point_feature("g", m)
        })
        .collect();

    let mut aggs = std::collections::HashMap::new();
    aggs.insert("status".to_string(), PropertyAggregator::KeepIfSame);
    let opts = DissolveOptions {
        group_by: "group".to_string(),
        strategy: DissolveStrategy::Multipart,
        property_aggregators: aggs,
        default_aggregator: PropertyAggregator::Drop,
    };
    let (output, _) = dissolve_features(&features, &opts).expect("dissolve ok");
    let props = output[0].properties.as_ref().expect("props");
    let v = props.get("status").expect("status key");
    assert_eq!(v.as_str().expect("str"), "same");
}

#[test]
fn test_aggregate_property_keep_if_same_returns_null_when_different() {
    let features: Vec<GeoJsonFeature> = ["alpha", "beta", "alpha"]
        .iter()
        .map(|s| {
            let mut m = serde_json::Map::new();
            m.insert("tag".to_string(), json!(s));
            make_point_feature("g", m)
        })
        .collect();

    let mut aggs = std::collections::HashMap::new();
    aggs.insert("tag".to_string(), PropertyAggregator::KeepIfSame);
    let opts = DissolveOptions {
        group_by: "group".to_string(),
        strategy: DissolveStrategy::Multipart,
        property_aggregators: aggs,
        default_aggregator: PropertyAggregator::Drop,
    };
    let (output, _) = dissolve_features(&features, &opts).expect("dissolve ok");
    let props = output[0].properties.as_ref().expect("props");
    let v = props.get("tag").expect("tag key");
    assert!(v.is_null(), "expected Null for differing values, got {v:?}");
}

// ─── Default aggregator / Drop ────────────────────────────────────────────────

#[test]
fn test_dissolve_preserves_default_aggregator_drop_behavior() {
    // "extra" is not in property_aggregators; default = Drop → value is Null.
    let mut m = serde_json::Map::new();
    m.insert("extra".to_string(), json!("something"));
    let features = vec![make_point_feature("g", m)];
    let opts = opts_default(PropertyAggregator::Drop);
    let (output, _) = dissolve_features(&features, &opts).expect("dissolve ok");
    let props = output[0].properties.as_ref().expect("props");
    // "extra" is present but its aggregated value should be Null.
    let v = props.get("extra").expect("extra key present");
    assert!(v.is_null(), "expected Null (Drop), got {v:?}");
}

// ─── FeatureCollection wrapper ────────────────────────────────────────────────

#[test]
fn test_dissolve_feature_collection_returns_feature_collection() {
    let fc = FeatureCollection {
        features: vec![
            make_point_feature("c1", serde_json::Map::new()),
            make_point_feature("c1", serde_json::Map::new()),
            make_point_feature("c2", serde_json::Map::new()),
        ],
        bbox: None,
        bbox_3d: None,
        crs: None,
        name: Some("test".to_string()),
    };
    let opts = opts_default(PropertyAggregator::Drop);
    let (result, stats) = dissolve_feature_collection(&fc, &opts).expect("dissolve ok");

    assert_eq!(stats.groups_formed, 2);
    assert_eq!(result.features.len(), 2);
    // Name should be preserved.
    assert_eq!(result.name.as_deref(), Some("test"));
    // Bbox should be unset (recomputation is caller's responsibility).
    assert!(result.bbox.is_none());
}
