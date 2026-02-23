//! GeoJSON Feature and FeatureCollection types
//!
//! This module implements Feature and FeatureCollection types according to RFC 7946.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{GeoJsonError, Result};
use crate::types::{BBox, Crs, ForeignMembers, Geometry};

/// Feature ID (can be string or number)
///
/// Custom `Deserialize` implementation is used instead of relying on the derived
/// `#[serde(untagged)]` deserializer to handle `serde_json/arbitrary_precision`
/// correctly when activated by workspace dependencies (e.g. `bigdecimal`). With
/// `arbitrary_precision`, numeric values are stored internally as `Content::Map`
/// rather than `Content::I64`, which breaks the generated untagged deserialization
/// code. Deserializing first into `serde_json::Value` sidesteps the issue because
/// `serde_json` handles its own `arbitrary_precision` feature correctly.
///
/// `#[serde(untagged)]` is kept for the `Serialize` derive so that string IDs are
/// written as `"feature-1"` rather than `{"String":"feature-1"}`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum FeatureId {
    /// String ID
    String(String),
    /// Numeric ID
    Number(i64),
}

impl<'de> Deserialize<'de> for FeatureId {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error as _;
        let value = serde_json::Value::deserialize(deserializer)?;
        match &value {
            serde_json::Value::String(s) => Ok(FeatureId::String(s.clone())),
            serde_json::Value::Number(n) => n
                .as_i64()
                .map(FeatureId::Number)
                .ok_or_else(|| D::Error::custom("FeatureId number must be representable as i64")),
            _ => Err(D::Error::custom("FeatureId must be a string or number")),
        }
    }
}

impl FeatureId {
    /// Creates a new string ID
    pub fn string<S: Into<String>>(s: S) -> Self {
        Self::String(s.into())
    }

    /// Creates a new numeric ID
    pub const fn number(n: i64) -> Self {
        Self::Number(n)
    }

    /// Returns the ID as a string
    #[must_use]
    pub fn as_string(&self) -> String {
        match self {
            Self::String(s) => s.clone(),
            Self::Number(n) => n.to_string(),
        }
    }
}

impl From<String> for FeatureId {
    fn from(s: String) -> Self {
        Self::String(s)
    }
}

impl From<&str> for FeatureId {
    fn from(s: &str) -> Self {
        Self::String(s.to_string())
    }
}

impl From<i64> for FeatureId {
    fn from(n: i64) -> Self {
        Self::Number(n)
    }
}

impl From<i32> for FeatureId {
    fn from(n: i32) -> Self {
        Self::Number(i64::from(n))
    }
}

impl From<u32> for FeatureId {
    fn from(n: u32) -> Self {
        Self::Number(i64::from(n))
    }
}

/// Feature properties (JSON object)
pub type Properties = serde_json::Map<String, Value>;

/// GeoJSON Feature
///
/// A Feature object represents a spatially bounded entity, associating
/// a Geometry with properties.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Feature {
    /// Type discriminator (always "Feature")
    #[serde(rename = "type")]
    pub feature_type: String,

    /// Optional feature ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<FeatureId>,

    /// Optional geometry (can be null)
    pub geometry: Option<Geometry>,

    /// Feature properties (can be null)
    pub properties: Option<Properties>,

    /// Optional bounding box
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbox: Option<BBox>,

    /// Optional CRS (deprecated in RFC 7946 but still supported)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crs: Option<Crs>,

    /// Foreign members (additional properties)
    #[serde(flatten)]
    pub foreign_members: Option<ForeignMembers>,
}

impl Feature {
    /// Creates a new Feature with geometry and properties
    pub fn new(geometry: Option<Geometry>, properties: Option<Properties>) -> Self {
        Self {
            feature_type: "Feature".to_string(),
            id: None,
            geometry,
            properties,
            bbox: None,
            crs: None,
            foreign_members: None,
        }
    }

    /// Creates a new Feature with ID, geometry, and properties
    pub fn with_id<I: Into<FeatureId>>(
        id: I,
        geometry: Option<Geometry>,
        properties: Option<Properties>,
    ) -> Self {
        Self {
            feature_type: "Feature".to_string(),
            id: Some(id.into()),
            geometry,
            properties,
            bbox: None,
            crs: None,
            foreign_members: None,
        }
    }

    /// Sets the feature ID
    pub fn set_id<I: Into<FeatureId>>(&mut self, id: I) {
        self.id = Some(id.into());
    }

    /// Sets the geometry
    pub fn set_geometry(&mut self, geometry: Geometry) {
        self.geometry = Some(geometry);
    }

    /// Sets the properties
    pub fn set_properties(&mut self, properties: Properties) {
        self.properties = Some(properties);
    }

    /// Adds a property
    pub fn add_property<K: Into<String>, V: Into<Value>>(&mut self, key: K, value: V) {
        let props = self.properties.get_or_insert_with(Properties::new);
        props.insert(key.into(), value.into());
    }

    /// Gets a property value
    #[must_use]
    pub fn get_property(&self, key: &str) -> Option<&Value> {
        self.properties.as_ref().and_then(|p| p.get(key))
    }

    /// Sets the bounding box
    pub fn set_bbox(&mut self, bbox: BBox) {
        self.bbox = Some(bbox);
    }

    /// Computes and sets the bounding box from geometry
    pub fn compute_bbox(&mut self) {
        if let Some(ref geometry) = self.geometry {
            self.bbox = geometry.compute_bbox();
        }
    }

    /// Sets the CRS
    pub fn set_crs(&mut self, crs: Crs) {
        self.crs = Some(crs);
    }

    /// Validates the feature
    pub fn validate(&self) -> Result<()> {
        if self.feature_type != "Feature" {
            return Err(GeoJsonError::InvalidFeature {
                message: format!(
                    "Invalid type: expected 'Feature', got '{}'",
                    self.feature_type
                ),
                feature_id: self.id.as_ref().map(|id| id.as_string()),
            });
        }

        if let Some(ref geometry) = self.geometry {
            geometry
                .validate()
                .map_err(|e| GeoJsonError::InvalidFeature {
                    message: format!("Invalid geometry: {e}"),
                    feature_id: self.id.as_ref().map(|id| id.as_string()),
                })?;
        }

        if let Some(ref bbox) = self.bbox {
            crate::types::geometry::validate_bbox(bbox)?;
        }

        Ok(())
    }

    /// Returns true if the feature has a geometry
    #[must_use]
    pub const fn has_geometry(&self) -> bool {
        self.geometry.is_some()
    }

    /// Returns true if the feature has properties
    #[must_use]
    pub const fn has_properties(&self) -> bool {
        self.properties.is_some()
    }

    /// Returns the number of properties
    #[must_use]
    pub fn property_count(&self) -> usize {
        self.properties.as_ref().map_or(0, |p| p.len())
    }
}

impl Default for Feature {
    fn default() -> Self {
        Self::new(None, None)
    }
}

/// GeoJSON FeatureCollection
///
/// A FeatureCollection is a collection of Feature objects.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureCollection {
    /// Type discriminator (always "FeatureCollection")
    #[serde(rename = "type")]
    pub collection_type: String,

    /// The features in the collection
    pub features: Vec<Feature>,

    /// Optional bounding box
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbox: Option<BBox>,

    /// Optional CRS (deprecated in RFC 7946 but still supported)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crs: Option<Crs>,

    /// Foreign members (additional properties)
    #[serde(flatten)]
    pub foreign_members: Option<ForeignMembers>,
}

impl FeatureCollection {
    /// Creates a new FeatureCollection
    pub fn new(features: Vec<Feature>) -> Self {
        Self {
            collection_type: "FeatureCollection".to_string(),
            features,
            bbox: None,
            crs: None,
            foreign_members: None,
        }
    }

    /// Creates an empty FeatureCollection
    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    /// Creates a FeatureCollection with capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            collection_type: "FeatureCollection".to_string(),
            features: Vec::with_capacity(capacity),
            bbox: None,
            crs: None,
            foreign_members: None,
        }
    }

    /// Adds a feature to the collection
    pub fn add_feature(&mut self, feature: Feature) {
        self.features.push(feature);
    }

    /// Adds multiple features to the collection
    pub fn add_features(&mut self, features: Vec<Feature>) {
        self.features.extend(features);
    }

    /// Sets the bounding box
    pub fn set_bbox(&mut self, bbox: BBox) {
        self.bbox = Some(bbox);
    }

    /// Computes and sets the bounding box from all features
    pub fn compute_bbox(&mut self) {
        if self.features.is_empty() {
            self.bbox = None;
            return;
        }

        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;

        for feature in &self.features {
            if let Some(ref geometry) = feature.geometry {
                if let Some(bbox) = geometry.compute_bbox() {
                    if bbox.len() >= 4 {
                        min_x = min_x.min(bbox[0]);
                        min_y = min_y.min(bbox[1]);
                        max_x = max_x.max(bbox[2]);
                        max_y = max_y.max(bbox[3]);
                    }
                }
            }
        }

        if min_x.is_finite() && min_y.is_finite() && max_x.is_finite() && max_y.is_finite() {
            self.bbox = Some(vec![min_x, min_y, max_x, max_y]);
        }
    }

    /// Sets the CRS for the entire collection
    pub fn set_crs(&mut self, crs: Crs) {
        self.crs = Some(crs);
    }

    /// Validates the feature collection
    pub fn validate(&self) -> Result<()> {
        if self.collection_type != "FeatureCollection" {
            return Err(GeoJsonError::InvalidFeatureCollection {
                message: format!(
                    "Invalid type: expected 'FeatureCollection', got '{}'",
                    self.collection_type
                ),
            });
        }

        for (i, feature) in self.features.iter().enumerate() {
            feature
                .validate()
                .map_err(|e| GeoJsonError::validation_at(e.to_string(), format!("features/{i}")))?;
        }

        if let Some(ref bbox) = self.bbox {
            crate::types::geometry::validate_bbox(bbox)?;
        }

        Ok(())
    }

    /// Returns the number of features
    #[must_use]
    pub fn len(&self) -> usize {
        self.features.len()
    }

    /// Returns true if the collection is empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.features.is_empty()
    }

    /// Returns an iterator over the features
    pub fn iter(&self) -> impl Iterator<Item = &Feature> {
        self.features.iter()
    }

    /// Returns a mutable iterator over the features
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Feature> {
        self.features.iter_mut()
    }

    /// Filters features by a predicate
    pub fn filter<F>(&self, predicate: F) -> Self
    where
        F: Fn(&Feature) -> bool,
    {
        Self::new(
            self.features
                .iter()
                .filter(|f| predicate(f))
                .cloned()
                .collect(),
        )
    }

    /// Returns features with a specific property value
    pub fn with_property(&self, key: &str, value: &Value) -> Self {
        self.filter(|f| f.properties.as_ref().and_then(|p| p.get(key)) == Some(value))
    }

    /// Removes all features
    pub fn clear(&mut self) {
        self.features.clear();
        self.bbox = None;
    }

    /// Retains only the features that satisfy the predicate
    pub fn retain<F>(&mut self, predicate: F)
    where
        F: FnMut(&Feature) -> bool,
    {
        self.features.retain(predicate);
    }
}

impl Default for FeatureCollection {
    fn default() -> Self {
        Self::empty()
    }
}

impl IntoIterator for FeatureCollection {
    type Item = Feature;
    type IntoIter = std::vec::IntoIter<Feature>;

    fn into_iter(self) -> Self::IntoIter {
        self.features.into_iter()
    }
}

impl<'a> IntoIterator for &'a FeatureCollection {
    type Item = &'a Feature;
    type IntoIter = std::slice::Iter<'a, Feature>;

    fn into_iter(self) -> Self::IntoIter {
        self.features.iter()
    }
}

impl<'a> IntoIterator for &'a mut FeatureCollection {
    type Item = &'a mut Feature;
    type IntoIter = std::slice::IterMut<'a, Feature>;

    fn into_iter(self) -> Self::IntoIter {
        self.features.iter_mut()
    }
}

impl FromIterator<Feature> for FeatureCollection {
    fn from_iter<T: IntoIterator<Item = Feature>>(iter: T) -> Self {
        Self::new(iter.into_iter().collect())
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use crate::types::geometry::Point;

    #[test]
    fn test_feature_id() {
        let string_id = FeatureId::string("feature-1");
        assert_eq!(string_id.as_string(), "feature-1");

        let num_id = FeatureId::number(42);
        assert_eq!(num_id.as_string(), "42");
    }

    #[test]
    fn test_feature_creation() {
        let point = Point::new_2d(-122.4, 37.8).expect("valid point");
        let geometry = Geometry::Point(point);

        let mut props = Properties::new();
        props.insert(
            "name".to_string(),
            Value::String("San Francisco".to_string()),
        );

        let feature = Feature::new(Some(geometry), Some(props));
        assert!(feature.has_geometry());
        assert!(feature.has_properties());
        assert_eq!(feature.property_count(), 1);
    }

    #[test]
    fn test_feature_with_id() {
        let point = Point::new_2d(0.0, 0.0).expect("valid point");
        let geometry = Geometry::Point(point);

        let feature = Feature::with_id("test-id", Some(geometry), None);
        assert!(feature.id.is_some());
        if let Some(FeatureId::String(id)) = &feature.id {
            assert_eq!(id, "test-id");
        } else {
            panic!("Expected string ID");
        }
    }

    #[test]
    fn test_feature_properties() {
        let mut feature = Feature::default();
        feature.add_property("name", "Test");
        feature.add_property("count", 42);

        assert_eq!(feature.property_count(), 2);
        assert!(feature.get_property("name").is_some());
    }

    #[test]
    fn test_feature_validation() {
        let point = Point::new_2d(-122.4, 37.8).expect("valid point");
        let geometry = Geometry::Point(point);
        let feature = Feature::new(Some(geometry), None);

        assert!(feature.validate().is_ok());
    }

    #[test]
    fn test_feature_collection_creation() {
        let fc = FeatureCollection::empty();
        assert!(fc.is_empty());
        assert_eq!(fc.len(), 0);
    }

    #[test]
    fn test_feature_collection_add() {
        let mut fc = FeatureCollection::empty();

        let point = Point::new_2d(0.0, 0.0).expect("valid point");
        let geometry = Geometry::Point(point);
        let feature = Feature::new(Some(geometry), None);

        fc.add_feature(feature);
        assert_eq!(fc.len(), 1);
        assert!(!fc.is_empty());
    }

    #[test]
    fn test_feature_collection_compute_bbox() {
        let mut fc = FeatureCollection::empty();

        let p1 = Point::new_2d(0.0, 0.0).expect("valid point");
        let p2 = Point::new_2d(10.0, 10.0).expect("valid point");

        fc.add_feature(Feature::new(Some(Geometry::Point(p1)), None));
        fc.add_feature(Feature::new(Some(Geometry::Point(p2)), None));

        fc.compute_bbox();
        assert!(fc.bbox.is_some());

        if let Some(bbox) = &fc.bbox {
            assert_eq!(bbox[0], 0.0);
            assert_eq!(bbox[1], 0.0);
            assert_eq!(bbox[2], 10.0);
            assert_eq!(bbox[3], 10.0);
        }
    }

    #[test]
    fn test_feature_collection_filter() {
        let mut fc = FeatureCollection::empty();

        for i in 0..5 {
            let point = Point::new_2d(f64::from(i), f64::from(i)).expect("valid point");
            let mut feature = Feature::new(Some(Geometry::Point(point)), None);
            feature.add_property("id", i);
            fc.add_feature(feature);
        }

        let filtered = fc.with_property("id", &Value::Number(serde_json::Number::from(2)));
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn test_feature_collection_iterator() {
        let mut fc = FeatureCollection::with_capacity(3);

        for i in 0..3 {
            let point = Point::new_2d(f64::from(i), f64::from(i)).expect("valid point");
            fc.add_feature(Feature::new(Some(Geometry::Point(point)), None));
        }

        let count = fc.iter().count();
        assert_eq!(count, 3);
    }

    #[test]
    fn test_feature_collection_validation() {
        let mut fc = FeatureCollection::empty();

        let point = Point::new_2d(0.0, 0.0).expect("valid point");
        let feature = Feature::new(Some(Geometry::Point(point)), None);
        fc.add_feature(feature);

        assert!(fc.validate().is_ok());
    }
}
