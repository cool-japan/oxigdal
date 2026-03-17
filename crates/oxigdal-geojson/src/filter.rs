//! Feature filtering by property values and bounding box.

use crate::parser::FeatureCollection;
use crate::types::GeoJsonFeature;

// ─── FilterOp ───────────────────────────────────────────────────────────────

/// Comparison operators for property filters.
#[derive(Debug, Clone, PartialEq)]
pub enum FilterOp {
    /// Equal.
    Eq,
    /// Not equal.
    Ne,
    /// Less than.
    Lt,
    /// Less than or equal.
    Lte,
    /// Greater than.
    Gt,
    /// Greater than or equal.
    Gte,
    /// String contains sub-string.
    Contains,
    /// String starts with prefix.
    StartsWith,
}

// ─── PropertyFilter ──────────────────────────────────────────────────────────

/// A single property predicate.
#[derive(Debug, Clone)]
pub struct PropertyFilter {
    /// Property key to inspect.
    pub key: String,
    /// Comparison operator.
    pub operator: FilterOp,
    /// Value to compare against.
    pub value: serde_json::Value,
}

impl PropertyFilter {
    /// Evaluate this filter against a JSON property map.
    #[must_use]
    pub fn matches(&self, props: &serde_json::Value) -> bool {
        let actual = match props.get(&self.key) {
            Some(v) => v,
            None => return false,
        };

        match &self.operator {
            FilterOp::Eq => actual == &self.value,
            FilterOp::Ne => actual != &self.value,
            FilterOp::Lt => compare_f64(actual, &self.value, |a, b| a < b),
            FilterOp::Lte => compare_f64(actual, &self.value, |a, b| a <= b),
            FilterOp::Gt => compare_f64(actual, &self.value, |a, b| a > b),
            FilterOp::Gte => compare_f64(actual, &self.value, |a, b| a >= b),
            FilterOp::Contains => {
                let haystack = actual.as_str().unwrap_or("");
                let needle = self.value.as_str().unwrap_or("");
                haystack.contains(needle)
            }
            FilterOp::StartsWith => {
                let s = actual.as_str().unwrap_or("");
                let prefix = self.value.as_str().unwrap_or("");
                s.starts_with(prefix)
            }
        }
    }
}

fn compare_f64<F>(a: &serde_json::Value, b: &serde_json::Value, f: F) -> bool
where
    F: Fn(f64, f64) -> bool,
{
    match (a.as_f64(), b.as_f64()) {
        (Some(av), Some(bv)) => f(av, bv),
        _ => false,
    }
}

// ─── FeatureFilter ───────────────────────────────────────────────────────────

/// Composite feature filter combining property, bbox, and geometry-type tests.
#[derive(Debug, Clone, Default)]
pub struct FeatureFilter {
    /// Property predicates (ALL must match — AND semantics).
    pub property_filters: Vec<PropertyFilter>,
    /// Optional spatial bounding box filter `[minx, miny, maxx, maxy]`.
    pub bbox_filter: Option<[f64; 4]>,
    /// Optional allow-list of geometry type names (e.g. `["Point", "Polygon"]`).
    pub geometry_types: Option<Vec<String>>,
}

impl FeatureFilter {
    /// Create an empty (pass-all) filter.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Restrict to features whose geometry bbox intersects `bbox`.
    #[must_use]
    pub fn with_bbox(mut self, bbox: [f64; 4]) -> Self {
        self.bbox_filter = Some(bbox);
        self
    }

    /// Require `key == value` (equality).
    #[must_use]
    pub fn where_eq(mut self, key: impl Into<String>, value: impl Into<serde_json::Value>) -> Self {
        self.property_filters.push(PropertyFilter {
            key: key.into(),
            operator: FilterOp::Eq,
            value: value.into(),
        });
        self
    }

    /// Require `key > value` (numeric greater-than).
    #[must_use]
    pub fn where_gt(mut self, key: impl Into<String>, value: f64) -> Self {
        self.property_filters.push(PropertyFilter {
            key: key.into(),
            operator: FilterOp::Gt,
            value: serde_json::Value::from(value),
        });
        self
    }

    /// Require `key < value` (numeric less-than).
    #[must_use]
    pub fn where_lt(mut self, key: impl Into<String>, value: f64) -> Self {
        self.property_filters.push(PropertyFilter {
            key: key.into(),
            operator: FilterOp::Lt,
            value: serde_json::Value::from(value),
        });
        self
    }

    /// Restrict to features with one of the given geometry types.
    #[must_use]
    pub fn by_geometry_type(mut self, types: Vec<String>) -> Self {
        self.geometry_types = Some(types);
        self
    }

    /// Returns `true` when `feature` passes all active filters.
    #[must_use]
    pub fn matches(&self, feature: &GeoJsonFeature) -> bool {
        // --- property filters ---
        for pf in &self.property_filters {
            let pass = match &feature.properties {
                Some(props) => pf.matches(props),
                None => false,
            };
            if !pass {
                return false;
            }
        }

        // --- geometry type filter ---
        if let Some(allowed_types) = &self.geometry_types {
            let geom_type = feature
                .geometry
                .as_ref()
                .map(|g| g.geometry_type())
                .unwrap_or("null");
            if !allowed_types.iter().any(|t| t == geom_type) {
                return false;
            }
        }

        // --- bbox filter ---
        if let Some(filter_bb) = self.bbox_filter {
            match feature.bbox() {
                None => return false,
                Some(feat_bb) => {
                    // Intersects check: feature_bbox overlaps filter_bbox
                    if !bboxes_intersect(feat_bb, filter_bb) {
                        return false;
                    }
                }
            }
        }

        true
    }

    /// Apply this filter to a [`FeatureCollection`], returning a new collection
    /// containing only the matching features.
    #[must_use]
    pub fn apply(&self, fc: &FeatureCollection) -> FeatureCollection {
        let features: Vec<GeoJsonFeature> = fc
            .features
            .iter()
            .filter(|f| self.matches(f))
            .cloned()
            .collect();

        FeatureCollection {
            features,
            bbox: fc.bbox,
            crs: fc.crs.clone(),
            name: fc.name.clone(),
        }
    }
}

/// Returns `true` when two axis-aligned bounding boxes intersect.
fn bboxes_intersect(a: [f64; 4], b: [f64; 4]) -> bool {
    // a: [minx, miny, maxx, maxy]
    a[0] <= b[2] && a[2] >= b[0] && a[1] <= b[3] && a[3] >= b[1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::GeoJsonGeometry;

    fn point_feature(lon: f64, lat: f64, name: &str, value: f64) -> GeoJsonFeature {
        GeoJsonFeature {
            id: None,
            geometry: Some(GeoJsonGeometry::Point([lon, lat])),
            properties: Some(serde_json::json!({"name": name, "value": value})),
        }
    }

    #[test]
    fn test_eq_filter_matches() {
        let feat = point_feature(10.0, 20.0, "alpha", 42.0);
        let f = FeatureFilter::new().where_eq("name", "alpha");
        assert!(f.matches(&feat));
    }

    #[test]
    fn test_eq_filter_no_match() {
        let feat = point_feature(10.0, 20.0, "beta", 42.0);
        let f = FeatureFilter::new().where_eq("name", "alpha");
        assert!(!f.matches(&feat));
    }

    #[test]
    fn test_gt_filter() {
        let feat = point_feature(0.0, 0.0, "x", 100.0);
        assert!(FeatureFilter::new().where_gt("value", 50.0).matches(&feat));
        assert!(!FeatureFilter::new().where_gt("value", 200.0).matches(&feat));
    }

    #[test]
    fn test_bbox_filter_inside() {
        let feat = point_feature(5.0, 5.0, "x", 0.0);
        let f = FeatureFilter::new().with_bbox([0.0, 0.0, 10.0, 10.0]);
        assert!(f.matches(&feat));
    }

    #[test]
    fn test_bbox_filter_outside() {
        let feat = point_feature(50.0, 50.0, "x", 0.0);
        let f = FeatureFilter::new().with_bbox([0.0, 0.0, 10.0, 10.0]);
        assert!(!f.matches(&feat));
    }
}
