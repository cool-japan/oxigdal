//! GeoJSON feature collection diff.
//!
//! Computes structural differences between two [`FeatureCollection`]s,
//! matching features by their `id` field when present and falling back to
//! index-based positional matching for id-less features.

use std::collections::HashMap;

use serde_json::Value;

use crate::parser::FeatureCollection;
use crate::types::{FeatureId, GeoJsonFeature, GeoJsonGeometry};

// ─── Public types ────────────────────────────────────────────────────────────

/// How a single property differs between old and new.
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyChange {
    /// Property key.
    pub key: String,
    /// Value in the old document, or `None` if the key did not exist.
    pub before: Option<Value>,
    /// Value in the new document, or `None` if the key was removed.
    pub after: Option<Value>,
}

/// Detailed description of a feature that changed in both collections.
#[derive(Debug, Clone)]
pub struct FeatureChangeDetail {
    /// Stringified feature ID for display (e.g. `"42"` or `"\"city-1\""`).
    pub id: Option<String>,
    /// `true` when the geometry differs beyond the coordinate epsilon.
    pub geometry_changed: bool,
    /// All property keys whose values changed, were added, or were removed.
    pub property_changes: Vec<PropertyChange>,
}

/// A single diff entry describing the fate of one logical feature.
#[derive(Debug, Clone)]
pub enum FeatureDiff {
    /// Feature is present in the new collection but absent in the old.
    Added(GeoJsonFeature),
    /// Feature is present in the old collection but absent in the new.
    Removed(GeoJsonFeature),
    /// Feature exists in both collections but its geometry and/or properties
    /// differ in at least one way.
    Changed {
        /// Original feature.
        before: GeoJsonFeature,
        /// Updated feature.
        after: GeoJsonFeature,
        /// Structured summary of what changed.
        detail: FeatureChangeDetail,
    },
}

impl FeatureDiff {
    /// Returns `true` if this entry represents an added feature.
    #[must_use]
    pub fn is_added(&self) -> bool {
        matches!(self, Self::Added(_))
    }

    /// Returns `true` if this entry represents a removed feature.
    #[must_use]
    pub fn is_removed(&self) -> bool {
        matches!(self, Self::Removed(_))
    }

    /// Returns `true` if this entry represents a changed feature.
    #[must_use]
    pub fn is_changed(&self) -> bool {
        matches!(self, Self::Changed { .. })
    }
}

/// Result of diffing two GeoJSON feature collections.
#[derive(Debug, Clone)]
pub struct GeoJsonDiff {
    /// Features present only in the new collection.
    pub added: Vec<GeoJsonFeature>,
    /// Features present only in the old collection.
    pub removed: Vec<GeoJsonFeature>,
    /// Features whose geometry or properties changed (only `Changed` variants).
    pub changed: Vec<FeatureDiff>,
    /// Number of features that are identical in both collections.
    pub unchanged_count: usize,
}

impl GeoJsonDiff {
    /// Total number of diff entries (added + removed + changed).
    #[must_use]
    pub fn total_changes(&self) -> usize {
        self.added.len() + self.removed.len() + self.changed.len()
    }

    /// `true` when there are no differences whatsoever.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.total_changes() == 0
    }

    /// Total features encountered across both collections
    /// (each matched pair counted once).
    #[must_use]
    pub fn total_features_seen(&self) -> usize {
        self.added.len() + self.removed.len() + self.changed.len() + self.unchanged_count
    }
}

// ─── Public functions ────────────────────────────────────────────────────────

/// Format a [`FeatureId`] as a human-readable string.
///
/// - String IDs are returned as-is.
/// - Numeric IDs whose integer and float representations agree are rendered
///   without a decimal point (`42`), otherwise with full float precision
///   (`3.14`).
#[must_use]
pub fn feature_id_to_string(id: &FeatureId) -> String {
    match id {
        FeatureId::String(s) => s.clone(),
        FeatureId::Number(n) => {
            // Render integers without the decimal point when safe.
            let truncated = n.trunc();
            if (*n - truncated).abs() < f64::EPSILON && truncated.abs() < 1e15_f64 {
                format!("{}", truncated as i64)
            } else {
                format!("{n}")
            }
        }
    }
}

/// Compare two GeoJSON feature collections and return a structured diff.
///
/// **Matching strategy**
///
/// 1. Features that carry a `Some(id)` are matched by their stringified ID
///    across both collections.  Features in old but not in new → Removed;
///    features in new but not in old → Added; features in both → compared
///    pair-wise (geometry + properties).
///
/// 2. ID-less features (both old and new) are matched by position (index 0
///    against index 0, etc.).  Surplus features in the longer list are Added
///    or Removed as appropriate.
///
/// Mixed collections (some features have IDs, some don't) are handled
/// correctly: the two groups are partitioned and processed independently.
#[must_use]
pub fn diff_feature_collections(old: &FeatureCollection, new: &FeatureCollection) -> GeoJsonDiff {
    let mut added: Vec<GeoJsonFeature> = Vec::new();
    let mut removed: Vec<GeoJsonFeature> = Vec::new();
    let mut changed: Vec<FeatureDiff> = Vec::new();
    let mut unchanged_count: usize = 0;

    // ── Partition into ID-keyed and positional buckets ────────────────────────

    let (old_keyed, old_positional): (Vec<GeoJsonFeature>, Vec<GeoJsonFeature>) =
        old.features.iter().cloned().partition(|f| f.id.is_some());

    let (new_keyed, new_positional): (Vec<GeoJsonFeature>, Vec<GeoJsonFeature>) =
        new.features.iter().cloned().partition(|f| f.id.is_some());

    // ── 1. ID-keyed matching ──────────────────────────────────────────────────

    // Build HashMaps keyed by stringified ID.  When duplicate IDs exist in the
    // same collection we keep the last occurrence (standard map semantics).
    let mut old_by_id: HashMap<String, GeoJsonFeature> = HashMap::new();
    for f in old_keyed {
        // Safety: we already checked f.id.is_some()
        if let Some(ref id) = f.id {
            old_by_id.insert(feature_id_to_string(id), f);
        }
    }

    let mut new_by_id: HashMap<String, GeoJsonFeature> = HashMap::new();
    for f in new_keyed {
        if let Some(ref id) = f.id {
            new_by_id.insert(feature_id_to_string(id), f);
        }
    }

    // Removed: in old but not in new
    for (key, feat) in &old_by_id {
        if !new_by_id.contains_key(key) {
            removed.push(feat.clone());
        }
    }

    // Added: in new but not in old
    for (key, feat) in &new_by_id {
        if !old_by_id.contains_key(key) {
            added.push(feat.clone());
        }
    }

    // Matched: compare pair-wise
    for (key, old_feat) in &old_by_id {
        if let Some(new_feat) = new_by_id.get(key) {
            compare_features(old_feat, new_feat, &mut changed, &mut unchanged_count);
        }
    }

    // ── 2. Positional (index-based) matching ──────────────────────────────────

    let min_len = old_positional.len().min(new_positional.len());

    for idx in 0..min_len {
        compare_features(
            &old_positional[idx],
            &new_positional[idx],
            &mut changed,
            &mut unchanged_count,
        );
    }

    // Extras in old → Removed
    for feat in old_positional.iter().skip(min_len) {
        removed.push(feat.clone());
    }

    // Extras in new → Added
    for feat in new_positional.iter().skip(min_len) {
        added.push(feat.clone());
    }

    GeoJsonDiff {
        added,
        removed,
        changed,
        unchanged_count,
    }
}

/// Compare two property [`serde_json::Map`]s and return the set of differences.
///
/// Each [`PropertyChange`] represents one key where the before/after values
/// differ (including additions where `before` is `None` and removals where
/// `after` is `None`).  Keys with identical values are omitted.
#[must_use]
pub fn diff_properties(
    old: &serde_json::Map<String, Value>,
    new: &serde_json::Map<String, Value>,
) -> Vec<PropertyChange> {
    let mut changes: Vec<PropertyChange> = Vec::new();

    // Keys present in old
    for (key, old_val) in old {
        match new.get(key) {
            Some(new_val) if old_val == new_val => {
                // Identical — skip
            }
            Some(new_val) => {
                // Changed
                changes.push(PropertyChange {
                    key: key.clone(),
                    before: Some(old_val.clone()),
                    after: Some(new_val.clone()),
                });
            }
            None => {
                // Removed
                changes.push(PropertyChange {
                    key: key.clone(),
                    before: Some(old_val.clone()),
                    after: None,
                });
            }
        }
    }

    // Keys present in new but not in old (additions)
    for (key, new_val) in new {
        if !old.contains_key(key) {
            changes.push(PropertyChange {
                key: key.clone(),
                before: None,
                after: Some(new_val.clone()),
            });
        }
    }

    changes
}

/// Check whether two [`GeoJsonGeometry`] values are equal within the given
/// coordinate epsilon.
///
/// The comparison is:
/// - Same variant first (different variants are always unequal).
/// - For `Null` variants: both must be `Null`.
/// - Coordinate arrays: element-wise `|a - b| <= eps`.
/// - Nested structures (Polygon rings, GeometryCollection members, etc.) are
///   compared recursively.
#[must_use]
pub fn geometries_equal_within_eps(a: &GeoJsonGeometry, b: &GeoJsonGeometry, eps: f64) -> bool {
    use GeoJsonGeometry as G;

    match (a, b) {
        (G::Null, G::Null) => true,

        (G::Point(pa), G::Point(pb)) => coords2_eq(pa, pb, eps),
        (G::PointZ(pa), G::PointZ(pb)) => coords3_eq(pa, pb, eps),

        (G::LineString(la), G::LineString(lb)) => rings2_eq(la, lb, eps),
        (G::LineStringZ(la), G::LineStringZ(lb)) => rings3_eq(la, lb, eps),

        (G::Polygon(ra), G::Polygon(rb)) => poly2_eq(ra, rb, eps),
        (G::PolygonZ(ra), G::PolygonZ(rb)) => poly3_eq(ra, rb, eps),

        (G::MultiPoint(ma), G::MultiPoint(mb)) => rings2_eq(ma, mb, eps),
        (G::MultiPointZ(ma), G::MultiPointZ(mb)) => rings3_eq(ma, mb, eps),

        (G::MultiLineString(la), G::MultiLineString(lb)) => {
            la.len() == lb.len()
                && la
                    .iter()
                    .zip(lb.iter())
                    .all(|(r1, r2)| rings2_eq(r1, r2, eps))
        }
        (G::MultiLineStringZ(la), G::MultiLineStringZ(lb)) => {
            la.len() == lb.len()
                && la
                    .iter()
                    .zip(lb.iter())
                    .all(|(r1, r2)| rings3_eq(r1, r2, eps))
        }

        (G::MultiPolygon(pa), G::MultiPolygon(pb)) => {
            pa.len() == pb.len()
                && pa
                    .iter()
                    .zip(pb.iter())
                    .all(|(p1, p2)| poly2_eq(p1, p2, eps))
        }
        (G::MultiPolygonZ(pa), G::MultiPolygonZ(pb)) => {
            pa.len() == pb.len()
                && pa
                    .iter()
                    .zip(pb.iter())
                    .all(|(p1, p2)| poly3_eq(p1, p2, eps))
        }

        (G::GeometryCollection(ga), G::GeometryCollection(gb)) => {
            ga.len() == gb.len()
                && ga
                    .iter()
                    .zip(gb.iter())
                    .all(|(g1, g2)| geometries_equal_within_eps(g1, g2, eps))
        }

        // Different variants → always unequal
        _ => false,
    }
}

// ─── Private helpers ─────────────────────────────────────────────────────────

/// Element-wise comparison of two 2-D coordinate arrays.
#[inline]
fn coords2_eq(a: &[f64; 2], b: &[f64; 2], eps: f64) -> bool {
    (a[0] - b[0]).abs() <= eps && (a[1] - b[1]).abs() <= eps
}

/// Element-wise comparison of two 3-D coordinate arrays.
#[inline]
fn coords3_eq(a: &[f64; 3], b: &[f64; 3], eps: f64) -> bool {
    (a[0] - b[0]).abs() <= eps && (a[1] - b[1]).abs() <= eps && (a[2] - b[2]).abs() <= eps
}

/// Pair-wise comparison of two slices of 2-D coordinates.
fn rings2_eq(a: &[[f64; 2]], b: &[[f64; 2]], eps: f64) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(p, q)| coords2_eq(p, q, eps))
}

/// Pair-wise comparison of two slices of 3-D coordinates.
fn rings3_eq(a: &[[f64; 3]], b: &[[f64; 3]], eps: f64) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(p, q)| coords3_eq(p, q, eps))
}

/// Ring-by-ring comparison for 2-D polygons.
fn poly2_eq(a: &[Vec<[f64; 2]>], b: &[Vec<[f64; 2]>], eps: f64) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b.iter())
            .all(|(r1, r2)| rings2_eq(r1, r2, eps))
}

/// Ring-by-ring comparison for 3-D polygons.
fn poly3_eq(a: &[Vec<[f64; 3]>], b: &[Vec<[f64; 3]>], eps: f64) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b.iter())
            .all(|(r1, r2)| rings3_eq(r1, r2, eps))
}

/// Compare optional geometries.  Two absent geometries are equal.
fn geoms_equal(a: &Option<GeoJsonGeometry>, b: &Option<GeoJsonGeometry>) -> bool {
    const EPS: f64 = 1e-9;
    match (a, b) {
        (None, None) => true,
        (Some(ga), Some(gb)) => geometries_equal_within_eps(ga, gb, EPS),
        _ => false,
    }
}

/// Extract a property Map from an `Option<serde_json::Value>`.
/// Returns an empty Map when the value is absent or not an Object.
fn value_to_map(v: &Option<Value>) -> serde_json::Map<String, Value> {
    match v {
        Some(Value::Object(m)) => m.clone(),
        _ => serde_json::Map::new(),
    }
}

/// Compare two features and push to `changed` or increment `unchanged_count`.
fn compare_features(
    old_feat: &GeoJsonFeature,
    new_feat: &GeoJsonFeature,
    changed: &mut Vec<FeatureDiff>,
    unchanged_count: &mut usize,
) {
    let geometry_changed = !geoms_equal(&old_feat.geometry, &new_feat.geometry);

    let old_props = value_to_map(&old_feat.properties);
    let new_props = value_to_map(&new_feat.properties);
    let property_changes = diff_properties(&old_props, &new_props);

    if !geometry_changed && property_changes.is_empty() {
        *unchanged_count += 1;
        return;
    }

    let id_str = old_feat
        .id
        .as_ref()
        .or(new_feat.id.as_ref())
        .map(feature_id_to_string);

    let detail = FeatureChangeDetail {
        id: id_str,
        geometry_changed,
        property_changes,
    };

    changed.push(FeatureDiff::Changed {
        before: old_feat.clone(),
        after: new_feat.clone(),
        detail,
    });
}

// ─── Unit tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::FeatureCollection;
    use crate::types::{FeatureId, GeoJsonFeature, GeoJsonGeometry};

    // Helper: build a minimal FeatureCollection from a Vec<GeoJsonFeature>.
    fn fc(features: Vec<GeoJsonFeature>) -> FeatureCollection {
        FeatureCollection {
            features,
            bbox: None,
            bbox_3d: None,
            crs: None,
            name: None,
        }
    }

    // Helper: point feature with no ID.
    fn pt(x: f64, y: f64) -> GeoJsonFeature {
        GeoJsonFeature {
            id: None,
            geometry: Some(GeoJsonGeometry::Point([x, y])),
            properties: None,
        }
    }

    // Helper: point feature with a string ID.
    fn pt_id(id: &str, x: f64, y: f64) -> GeoJsonFeature {
        GeoJsonFeature {
            id: Some(FeatureId::String(id.into())),
            geometry: Some(GeoJsonGeometry::Point([x, y])),
            properties: None,
        }
    }

    // Helper: point feature with a string ID and properties.
    fn pt_id_props(id: &str, x: f64, y: f64, props: Value) -> GeoJsonFeature {
        GeoJsonFeature {
            id: Some(FeatureId::String(id.into())),
            geometry: Some(GeoJsonGeometry::Point([x, y])),
            properties: Some(props),
        }
    }

    #[test]
    fn test_diff_empty_collections() {
        let diff = diff_feature_collections(&fc(vec![]), &fc(vec![]));
        assert_eq!(diff.added.len(), 0);
        assert_eq!(diff.removed.len(), 0);
        assert_eq!(diff.changed.len(), 0);
        assert_eq!(diff.unchanged_count, 0);
        assert!(diff.is_empty());
    }

    #[test]
    fn test_diff_identical_collections() {
        let features = vec![pt_id("a", 1.0, 2.0), pt_id("b", 3.0, 4.0)];
        let old = fc(features.clone());
        let new = fc(features);
        let diff = diff_feature_collections(&old, &new);
        assert_eq!(diff.unchanged_count, 2);
        assert!(diff.is_empty());
    }

    #[test]
    fn test_diff_added_feature_detected() {
        let old = fc(vec![pt_id("a", 1.0, 2.0)]);
        let new = fc(vec![pt_id("a", 1.0, 2.0), pt_id("b", 5.0, 6.0)]);
        let diff = diff_feature_collections(&old, &new);
        assert_eq!(diff.added.len(), 1);
        assert_eq!(diff.removed.len(), 0);
        assert_eq!(diff.changed.len(), 0);
    }

    #[test]
    fn test_diff_removed_feature_detected() {
        let old = fc(vec![pt_id("a", 1.0, 2.0), pt_id("b", 5.0, 6.0)]);
        let new = fc(vec![pt_id("a", 1.0, 2.0)]);
        let diff = diff_feature_collections(&old, &new);
        assert_eq!(diff.added.len(), 0);
        assert_eq!(diff.removed.len(), 1);
        assert_eq!(diff.changed.len(), 0);
    }

    #[test]
    fn test_diff_changed_geometry_detected() {
        let old = fc(vec![pt_id("a", 1.0, 2.0)]);
        let new = fc(vec![pt_id("a", 9.0, 8.0)]); // different coords
        let diff = diff_feature_collections(&old, &new);
        assert_eq!(diff.changed.len(), 1);
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
            serde_json::json!({"name": "old"}),
        )]);
        let new = fc(vec![pt_id_props(
            "a",
            1.0,
            2.0,
            serde_json::json!({"name": "new"}),
        )]);
        let diff = diff_feature_collections(&old, &new);
        assert_eq!(diff.changed.len(), 1);
        let has_prop_changes = match &diff.changed[0] {
            FeatureDiff::Changed { detail, .. } => !detail.property_changes.is_empty(),
            _ => false,
        };
        assert!(has_prop_changes, "property_changes must be non-empty");
    }

    #[test]
    fn test_diff_added_property_detected() {
        let old = fc(vec![pt_id_props(
            "a",
            1.0,
            2.0,
            serde_json::json!({"a": 1}),
        )]);
        let new = fc(vec![pt_id_props(
            "a",
            1.0,
            2.0,
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
                .expect("expected change for key 'b'"),
            _ => (None, None),
        };
        assert_eq!(before, None);
        assert_eq!(after, Some(serde_json::json!(2)));
    }

    #[test]
    fn test_diff_removed_property_detected() {
        let old = fc(vec![pt_id_props(
            "a",
            1.0,
            2.0,
            serde_json::json!({"a": 1, "b": 2}),
        )]);
        let new = fc(vec![pt_id_props(
            "a",
            1.0,
            2.0,
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
                .expect("expected change for key 'b'"),
            _ => (None, None),
        };
        assert_eq!(after, None);
        assert_eq!(before, Some(serde_json::json!(2)));
    }

    #[test]
    fn test_diff_positional_matching_without_ids() {
        // Two id-less features: first pair identical, second pair different.
        let old = fc(vec![pt(0.0, 0.0), pt(1.0, 1.0)]);
        let new = fc(vec![pt(0.0, 0.0), pt(9.0, 9.0)]);
        let diff = diff_feature_collections(&old, &new);
        assert_eq!(diff.unchanged_count, 1);
        assert_eq!(diff.changed.len(), 1);
        assert_eq!(diff.added.len(), 0);
        assert_eq!(diff.removed.len(), 0);
    }

    #[test]
    fn test_geometries_equal_within_eps_same_point() {
        let a = GeoJsonGeometry::Point([1.0, 2.0]);
        let b = GeoJsonGeometry::Point([1.0, 2.0]);
        assert!(geometries_equal_within_eps(&a, &b, 1e-9));
    }

    #[test]
    fn test_geometries_equal_within_eps_different_point() {
        let a = GeoJsonGeometry::Point([1.0, 2.0]);
        let b = GeoJsonGeometry::Point([1.0, 2.0 + 1e-8]); // delta > eps
        assert!(!geometries_equal_within_eps(&a, &b, 1e-9));
    }

    #[test]
    fn test_geometries_equal_within_eps_different_types() {
        let a = GeoJsonGeometry::Point([1.0, 2.0]);
        let b = GeoJsonGeometry::LineString(vec![[1.0, 2.0]]);
        assert!(!geometries_equal_within_eps(&a, &b, 1e-9));
    }

    #[test]
    fn test_diff_properties_no_changes() {
        let map: serde_json::Map<String, Value> =
            serde_json::from_str(r#"{"a": 1, "b": "hello"}"#).expect("valid json");
        let changes = diff_properties(&map, &map);
        assert!(changes.is_empty());
    }

    #[test]
    fn test_diff_total_changes_counts_all() {
        // 1 added (new id-keyed), 1 removed (old id-keyed), 1 changed (same id, different coords).
        let old = fc(vec![
            pt_id("remove-me", 0.0, 0.0),
            pt_id("change-me", 5.0, 5.0),
        ]);
        let new = fc(vec![
            pt_id("add-me", 99.0, 99.0),
            pt_id("change-me", 6.0, 6.0),
        ]);
        let diff = diff_feature_collections(&old, &new);
        assert_eq!(diff.added.len(), 1);
        assert_eq!(diff.removed.len(), 1);
        assert_eq!(diff.changed.len(), 1);
        assert_eq!(diff.total_changes(), 3);
    }
}
