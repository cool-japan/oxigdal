//! Vector dataset component interface (wasm32-wasip2 compatible).
//!
//! Provides GeoJSON-like feature and collection types that are fully
//! transferable across the WASM component boundary.  Geometry is stored
//! as raw WKB bytes to avoid a dependency on a specific geometry library.

use std::collections::HashMap;

use crate::component::types::ComponentBbox;

/// A single property value that can cross the WASM component boundary.
///
/// The variants are kept intentionally small; rich types (e.g. timestamps,
/// GeoJSON geometries) should be serialised as strings or bytes.
#[derive(Debug, Clone)]
pub enum PropertyValue {
    /// SQL-NULL / JSON-null sentinel.
    Null,
    /// Boolean value.
    Bool(bool),
    /// 64-bit signed integer.
    Int(i64),
    /// 64-bit IEEE 754 float.
    Float(f64),
    /// UTF-8 string.
    String(String),
    /// Opaque byte blob.
    Bytes(Vec<u8>),
}

impl PropertyValue {
    /// Convert to `f64` if the variant is numeric.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Int(v) => Some(*v as f64),
            Self::Float(v) => Some(*v),
            _ => None,
        }
    }

    /// Borrow as a `&str` if this is a `String` variant.
    pub fn as_str(&self) -> Option<&str> {
        if let Self::String(s) = self {
            Some(s.as_str())
        } else {
            None
        }
    }

    /// Returns `true` for the `Null` variant.
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }
}

impl PartialEq for PropertyValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Null, Self::Null) => true,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Float(a), Self::Float(b)) => a == b,
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Bytes(a), Self::Bytes(b)) => a == b,
            _ => false,
        }
    }
}

/// A GeoJSON-like feature with WKB geometry, typed properties, and an
/// optional pre-computed bounding box.
#[derive(Debug, Clone)]
pub struct ComponentFeature {
    /// Optional stable identifier.
    pub id: Option<String>,
    /// Geometry encoded as ISO WKB (little-endian preferred).
    pub geometry_wkb: Option<Vec<u8>>,
    /// Attribute map.
    pub properties: HashMap<String, PropertyValue>,
    /// Pre-computed bounding box (may be absent if geometry is missing).
    pub bbox: Option<ComponentBbox>,
}

impl ComponentFeature {
    /// Create an empty feature with no geometry and no properties.
    pub fn new() -> Self {
        Self {
            id: None,
            geometry_wkb: None,
            properties: HashMap::new(),
            bbox: None,
        }
    }

    /// Builder: set the feature identifier.
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Builder: attach WKB-encoded geometry.
    pub fn with_geometry(mut self, wkb: Vec<u8>) -> Self {
        self.geometry_wkb = Some(wkb);
        self
    }

    /// Builder: attach a pre-computed bounding box.
    pub fn with_bbox(mut self, bbox: ComponentBbox) -> Self {
        self.bbox = Some(bbox);
        self
    }

    /// Insert or replace a property.
    pub fn set_property(&mut self, key: impl Into<String>, value: PropertyValue) {
        self.properties.insert(key.into(), value);
    }

    /// Look up a property by name.
    pub fn get_property(&self, key: &str) -> Option<&PropertyValue> {
        self.properties.get(key)
    }

    /// Returns `true` if WKB geometry is present.
    pub fn has_geometry(&self) -> bool {
        self.geometry_wkb.is_some()
    }

    /// Returns the number of properties.
    pub fn property_count(&self) -> usize {
        self.properties.len()
    }
}

impl Default for ComponentFeature {
    fn default() -> Self {
        Self::new()
    }
}

/// A homogeneous collection of [`ComponentFeature`]s.
#[derive(Debug, Clone)]
pub struct ComponentFeatureCollection {
    /// The features in this collection.
    pub features: Vec<ComponentFeature>,
    /// Optional WKT string describing the CRS.
    pub crs_wkt: Option<String>,
    /// Optional spatial extent of the entire collection.
    pub bbox: Option<ComponentBbox>,
}

impl ComponentFeatureCollection {
    /// Create an empty collection.
    pub fn new() -> Self {
        Self {
            features: Vec::new(),
            crs_wkt: None,
            bbox: None,
        }
    }

    /// Append a feature to the collection.
    pub fn add_feature(&mut self, feature: ComponentFeature) {
        self.features.push(feature);
    }

    /// Number of features in the collection.
    pub fn len(&self) -> usize {
        self.features.len()
    }

    /// Returns `true` if there are no features.
    pub fn is_empty(&self) -> bool {
        self.features.is_empty()
    }

    /// Retain only features whose bounding box intersects `bbox`.
    ///
    /// Features without a bounding box are always included (conservative).
    pub fn filter_by_bbox(&self, bbox: &ComponentBbox) -> Self {
        let filtered = self
            .features
            .iter()
            .filter(|f| {
                f.bbox.as_ref().map(|b| b.intersects(bbox)).unwrap_or(true) // no bbox → include conservatively
            })
            .cloned()
            .collect();

        Self {
            features: filtered,
            crs_wkt: self.crs_wkt.clone(),
            bbox: Some(bbox.clone()),
        }
    }

    /// Retain only features whose bounding box is *fully contained* by `bbox`.
    ///
    /// Features without a bounding box are always excluded.
    pub fn filter_by_bbox_strict(&self, bbox: &ComponentBbox) -> Self {
        let filtered = self
            .features
            .iter()
            .filter(|f| {
                f.bbox
                    .as_ref()
                    .map(|b| {
                        b.min_x >= bbox.min_x
                            && b.min_y >= bbox.min_y
                            && b.max_x <= bbox.max_x
                            && b.max_y <= bbox.max_y
                    })
                    .unwrap_or(false)
            })
            .cloned()
            .collect();

        Self {
            features: filtered,
            crs_wkt: self.crs_wkt.clone(),
            bbox: Some(bbox.clone()),
        }
    }

    /// Compute the union bounding box of all features that have one.
    pub fn compute_bbox(&self) -> Option<ComponentBbox> {
        let mut union: Option<ComponentBbox> = None;
        for f in &self.features {
            if let Some(b) = &f.bbox {
                union = Some(match union {
                    None => b.clone(),
                    Some(u) => u.union(b),
                });
            }
        }
        union
    }
}

impl Default for ComponentFeatureCollection {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feature_new_is_empty() {
        let f = ComponentFeature::new();
        assert!(f.id.is_none());
        assert!(!f.has_geometry());
        assert_eq!(f.property_count(), 0);
    }

    #[test]
    fn feature_with_id() {
        let f = ComponentFeature::new().with_id("feat-1");
        assert_eq!(f.id.as_deref(), Some("feat-1"));
    }

    #[test]
    fn feature_with_geometry() {
        let wkb = vec![1u8, 2, 3, 4];
        let f = ComponentFeature::new().with_geometry(wkb.clone());
        assert!(f.has_geometry());
        assert_eq!(f.geometry_wkb.as_deref(), Some(wkb.as_slice()));
    }

    #[test]
    fn feature_set_get_property() {
        let mut f = ComponentFeature::new();
        f.set_property("name", PropertyValue::String("hello".into()));
        assert_eq!(
            f.get_property("name").and_then(|v| v.as_str()),
            Some("hello")
        );
    }

    #[test]
    fn property_value_as_f64() {
        assert_eq!(PropertyValue::Int(42).as_f64(), Some(42.0));
        assert_eq!(PropertyValue::Float(1.234).as_f64(), Some(1.234));
        assert!(PropertyValue::Null.as_f64().is_none());
    }

    #[test]
    fn property_value_is_null() {
        assert!(PropertyValue::Null.is_null());
        assert!(!PropertyValue::Bool(true).is_null());
    }

    #[test]
    fn collection_add_and_len() {
        let mut col = ComponentFeatureCollection::new();
        assert!(col.is_empty());
        col.add_feature(ComponentFeature::new());
        assert_eq!(col.len(), 1);
    }

    #[test]
    fn collection_filter_by_bbox() {
        let mut col = ComponentFeatureCollection::new();
        // feature inside filter bbox
        col.add_feature(ComponentFeature::new().with_bbox(ComponentBbox::new(1.0, 1.0, 2.0, 2.0)));
        // feature outside filter bbox
        col.add_feature(
            ComponentFeature::new().with_bbox(ComponentBbox::new(100.0, 100.0, 200.0, 200.0)),
        );
        // feature without bbox (conservatively included)
        col.add_feature(ComponentFeature::new());

        let filter = ComponentBbox::new(0.0, 0.0, 5.0, 5.0);
        let result = col.filter_by_bbox(&filter);
        assert_eq!(result.len(), 2); // inside + no-bbox
    }

    #[test]
    fn collection_filter_no_match() {
        let mut col = ComponentFeatureCollection::new();
        col.add_feature(
            ComponentFeature::new().with_bbox(ComponentBbox::new(100.0, 100.0, 200.0, 200.0)),
        );
        let filter = ComponentBbox::new(0.0, 0.0, 5.0, 5.0);
        let result = col.filter_by_bbox_strict(&filter);
        assert!(result.is_empty());
    }
}
