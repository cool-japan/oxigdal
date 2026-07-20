//! Integration tests for the GeoJSON feature collection diff module.

use oxigeo_geojson_stream::{
    FeatureCollection, FeatureDiff, FeatureId, GeoJsonFeature, GeoJsonGeometry,
    diff_feature_collections, diff_properties, geometries_equal_within_eps,
};

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn fc(features: Vec<GeoJsonFeature>) -> FeatureCollection {
    FeatureCollection {
        features,
        bbox: None,
        bbox_3d: None,
        crs: None,
        name: None,
    }
}

fn pt(x: f64, y: f64) -> GeoJsonFeature {
    GeoJsonFeature {
        id: None,
        geometry: Some(GeoJsonGeometry::Point([x, y])),
        properties: None,
    }
}

fn pt_id(id: &str, x: f64, y: f64) -> GeoJsonFeature {
    GeoJsonFeature {
        id: Some(FeatureId::String(id.into())),
        geometry: Some(GeoJsonGeometry::Point([x, y])),
        properties: None,
    }
}

fn pt_id_props(id: &str, x: f64, y: f64, props: serde_json::Value) -> GeoJsonFeature {
    GeoJsonFeature {
        id: Some(FeatureId::String(id.into())),
        geometry: Some(GeoJsonGeometry::Point([x, y])),
        properties: Some(props),
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn test_diff_empty_collections() {
    let diff = diff_feature_collections(&fc(vec![]), &fc(vec![]));
    assert_eq!(diff.added.len(), 0, "no added features in empty diff");
    assert_eq!(diff.removed.len(), 0, "no removed features in empty diff");
    assert_eq!(diff.changed.len(), 0, "no changed features in empty diff");
    assert_eq!(diff.unchanged_count, 0, "unchanged_count must be 0");
    assert_eq!(diff.total_changes(), 0);
    assert!(diff.is_empty());
}

#[test]
fn test_diff_identical_collections() {
    let features = vec![pt_id("x", 10.0, 20.0), pt_id("y", 30.0, 40.0)];
    let old = fc(features.clone());
    let new = fc(features);
    let diff = diff_feature_collections(&old, &new);
    assert_eq!(diff.unchanged_count, 2, "both features must be unchanged");
    assert!(diff.is_empty(), "identical collections produce empty diff");
}

#[test]
fn test_diff_added_feature_detected() {
    let old = fc(vec![pt_id("a", 1.0, 2.0)]);
    let new = fc(vec![pt_id("a", 1.0, 2.0), pt_id("b", 5.0, 6.0)]);
    let diff = diff_feature_collections(&old, &new);
    assert_eq!(diff.added.len(), 1, "one feature was added");
    assert_eq!(diff.removed.len(), 0);
    assert_eq!(diff.changed.len(), 0);
    assert_eq!(diff.unchanged_count, 1);
}

#[test]
fn test_diff_removed_feature_detected() {
    let old = fc(vec![pt_id("a", 1.0, 2.0), pt_id("b", 5.0, 6.0)]);
    let new = fc(vec![pt_id("a", 1.0, 2.0)]);
    let diff = diff_feature_collections(&old, &new);
    assert_eq!(diff.added.len(), 0);
    assert_eq!(diff.removed.len(), 1, "one feature was removed");
    assert_eq!(diff.changed.len(), 0);
    assert_eq!(diff.unchanged_count, 1);
}

#[test]
fn test_diff_changed_geometry_detected() {
    // Same ID but different coordinates → changed, geometry_changed == true.
    let old = fc(vec![pt_id("a", 1.0, 2.0)]);
    let new = fc(vec![pt_id("a", 9.0, 8.0)]);
    let diff = diff_feature_collections(&old, &new);
    assert_eq!(diff.changed.len(), 1, "one feature must be marked changed");
    let geometry_changed = match &diff.changed[0] {
        FeatureDiff::Changed { detail, .. } => detail.geometry_changed,
        _ => false,
    };
    assert!(geometry_changed, "geometry_changed must be true");
}

#[test]
fn test_diff_changed_property_detected() {
    let old = fc(vec![pt_id_props(
        "a",
        1.0,
        2.0,
        serde_json::json!({"name": "Tokyo"}),
    )]);
    let new = fc(vec![pt_id_props(
        "a",
        1.0,
        2.0,
        serde_json::json!({"name": "Osaka"}),
    )]);
    let diff = diff_feature_collections(&old, &new);
    assert_eq!(diff.changed.len(), 1);
    let (has_prop_changes, geom_unchanged) = match &diff.changed[0] {
        FeatureDiff::Changed { detail, .. } => (
            !detail.property_changes.is_empty(),
            !detail.geometry_changed,
        ),
        _ => (false, false),
    };
    assert!(has_prop_changes, "property_changes must be non-empty");
    assert!(geom_unchanged, "geometry must be unchanged");
}

#[test]
fn test_diff_added_property_detected() {
    // Old: {"a": 1}   New: {"a": 1, "b": 2}  → PropertyChange for "b" with before=None.
    let old = fc(vec![pt_id_props(
        "x",
        0.0,
        0.0,
        serde_json::json!({"a": 1}),
    )]);
    let new = fc(vec![pt_id_props(
        "x",
        0.0,
        0.0,
        serde_json::json!({"a": 1, "b": 2}),
    )]);
    let diff = diff_feature_collections(&old, &new);
    assert_eq!(diff.changed.len(), 1);
    let (before, after) = match &diff.changed[0] {
        FeatureDiff::Changed { detail, .. } => detail
            .property_changes
            .iter()
            .find(|c| c.key == "b")
            .map(|c| (c.before.clone(), c.after.clone()))
            .expect("expected PropertyChange for key 'b'"),
        _ => (None, None),
    };
    assert_eq!(before, None, "before must be None for added key");
    assert_eq!(
        after,
        Some(serde_json::json!(2)),
        "after must be the new value"
    );
}

#[test]
fn test_diff_removed_property_detected() {
    // Old: {"a": 1, "b": 2}   New: {"a": 1}  → PropertyChange for "b" with after=None.
    let old = fc(vec![pt_id_props(
        "x",
        0.0,
        0.0,
        serde_json::json!({"a": 1, "b": 2}),
    )]);
    let new = fc(vec![pt_id_props(
        "x",
        0.0,
        0.0,
        serde_json::json!({"a": 1}),
    )]);
    let diff = diff_feature_collections(&old, &new);
    assert_eq!(diff.changed.len(), 1);
    let (before, after) = match &diff.changed[0] {
        FeatureDiff::Changed { detail, .. } => detail
            .property_changes
            .iter()
            .find(|c| c.key == "b")
            .map(|c| (c.before.clone(), c.after.clone()))
            .expect("expected PropertyChange for key 'b'"),
        _ => (None, None),
    };
    assert_eq!(after, None, "after must be None for removed key");
    assert_eq!(
        before,
        Some(serde_json::json!(2)),
        "before must be the old value"
    );
}

#[test]
fn test_diff_positional_matching_without_ids() {
    // No IDs → index-based: index-0 same, index-1 different.
    let old = fc(vec![pt(0.0, 0.0), pt(1.0, 1.0)]);
    let new = fc(vec![pt(0.0, 0.0), pt(9.0, 9.0)]);
    let diff = diff_feature_collections(&old, &new);
    assert_eq!(diff.unchanged_count, 1, "first pair must be unchanged");
    assert_eq!(diff.changed.len(), 1, "second pair must be changed");
    assert_eq!(diff.added.len(), 0);
    assert_eq!(diff.removed.len(), 0);
}

#[test]
fn test_geometries_equal_within_eps_same_point() {
    let a = GeoJsonGeometry::Point([1.23456789, 9.87654321]);
    let b = GeoJsonGeometry::Point([1.23456789, 9.87654321]);
    assert!(
        geometries_equal_within_eps(&a, &b, 1e-9),
        "identical points must be equal within eps"
    );
}

#[test]
fn test_geometries_equal_within_eps_different_point() {
    let a = GeoJsonGeometry::Point([1.0, 2.0]);
    let b = GeoJsonGeometry::Point([1.0, 2.0 + 1e-8]); // 10 ns > 1e-9 eps
    assert!(
        !geometries_equal_within_eps(&a, &b, 1e-9),
        "delta > eps must not be equal"
    );
}

#[test]
fn test_geometries_equal_within_eps_different_types() {
    let a = GeoJsonGeometry::Point([1.0, 2.0]);
    let b = GeoJsonGeometry::LineString(vec![[1.0, 2.0]]);
    assert!(
        !geometries_equal_within_eps(&a, &b, 1e-9),
        "different geometry types must not be equal"
    );
}

#[test]
fn test_diff_properties_no_changes() {
    let map: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(r#"{"x": 42, "label": "foo", "flag": true}"#)
            .expect("valid JSON object");
    let changes = diff_properties(&map, &map);
    assert!(
        changes.is_empty(),
        "same maps must yield no PropertyChange entries"
    );
}

#[test]
fn test_diff_total_changes_counts_all() {
    // 1 added, 1 removed, 1 changed → total_changes() == 3.
    let old = fc(vec![
        pt_id("remove-me", 0.0, 0.0),
        pt_id("change-me", 5.0, 5.0),
    ]);
    let new = fc(vec![
        pt_id("add-me", 99.0, 99.0),
        pt_id("change-me", 6.0, 6.0),
    ]);
    let diff = diff_feature_collections(&old, &new);
    assert_eq!(diff.added.len(), 1, "one added");
    assert_eq!(diff.removed.len(), 1, "one removed");
    assert_eq!(diff.changed.len(), 1, "one changed");
    assert_eq!(diff.total_changes(), 3, "total_changes() must be 3");
    assert!(!diff.is_empty());
}
