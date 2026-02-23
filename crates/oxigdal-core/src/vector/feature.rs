//! Feature types for vector data
//!
//! A feature is a geometry with associated properties (attributes).

use crate::vector::geometry::Geometry;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

#[cfg(feature = "std")]
use std::collections::HashMap;
#[cfg(feature = "std")]
use std::string::String;

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{
    collections::BTreeMap as HashMap,
    string::{String, ToString},
};

/// A feature with geometry and properties
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Feature {
    /// Optional feature ID
    pub id: Option<FeatureId>,
    /// Geometry (may be None for attribute-only features)
    pub geometry: Option<Geometry>,
    /// Feature properties as key-value pairs
    pub properties: HashMap<String, PropertyValue>,
}

impl Feature {
    /// Creates a new feature with geometry and no properties
    #[must_use]
    pub fn new(geometry: Geometry) -> Self {
        Self {
            id: None,
            geometry: Some(geometry),
            properties: HashMap::new(),
        }
    }

    /// Creates a new feature with geometry and ID
    #[must_use]
    pub fn with_id(id: FeatureId, geometry: Geometry) -> Self {
        Self {
            id: Some(id),
            geometry: Some(geometry),
            properties: HashMap::new(),
        }
    }

    /// Creates a new feature without geometry (attribute-only)
    #[must_use]
    pub fn new_attribute_only() -> Self {
        Self {
            id: None,
            geometry: None,
            properties: HashMap::new(),
        }
    }

    /// Sets a property value
    pub fn set_property<S: Into<String>>(&mut self, key: S, value: PropertyValue) {
        self.properties.insert(key.into(), value);
    }

    /// Gets a property value
    #[must_use]
    pub fn get_property(&self, key: &str) -> Option<&PropertyValue> {
        self.properties.get(key)
    }

    /// Removes a property
    pub fn remove_property(&mut self, key: &str) -> Option<PropertyValue> {
        self.properties.remove(key)
    }

    /// Returns true if the feature has a geometry
    #[must_use]
    pub const fn has_geometry(&self) -> bool {
        self.geometry.is_some()
    }

    /// Returns the bounding box of the geometry
    #[must_use]
    pub fn bounds(&self) -> Option<(f64, f64, f64, f64)> {
        self.geometry
            .as_ref()
            .and_then(super::geometry::Geometry::bounds)
    }
}

/// Feature ID - can be either integer or string
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FeatureId {
    /// Integer ID
    Integer(i64),
    /// String ID
    String(String),
}

impl From<i64> for FeatureId {
    fn from(id: i64) -> Self {
        Self::Integer(id)
    }
}

impl From<String> for FeatureId {
    fn from(id: String) -> Self {
        Self::String(id)
    }
}

impl From<&str> for FeatureId {
    fn from(id: &str) -> Self {
        Self::String(id.to_string())
    }
}

/// Property value type
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PropertyValue {
    /// Null value
    Null,
    /// Boolean value
    Bool(bool),
    /// Integer value (i64)
    Integer(i64),
    /// Unsigned integer value
    UInteger(u64),
    /// Float value (f64)
    Float(f64),
    /// String value
    String(String),
    /// Array of values
    Array(Vec<PropertyValue>),
    /// JSON object
    Object(HashMap<String, PropertyValue>),
}

impl PropertyValue {
    /// Returns true if the value is null
    #[must_use]
    pub const fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Converts to JSON Value
    #[cfg(feature = "std")]
    #[must_use]
    pub fn to_json(&self) -> JsonValue {
        match self {
            Self::Null => JsonValue::Null,
            Self::Bool(b) => JsonValue::Bool(*b),
            Self::Integer(i) => JsonValue::Number((*i).into()),
            Self::UInteger(u) => JsonValue::Number((*u).into()),
            Self::Float(f) => {
                JsonValue::Number(serde_json::Number::from_f64(*f).unwrap_or_else(|| 0.into()))
            }
            Self::String(s) => JsonValue::String(s.clone()),
            Self::Array(arr) => JsonValue::Array(arr.iter().map(PropertyValue::to_json).collect()),
            Self::Object(obj) => {
                JsonValue::Object(obj.iter().map(|(k, v)| (k.clone(), v.to_json())).collect())
            }
        }
    }

    /// Creates from JSON Value
    #[cfg(feature = "std")]
    #[must_use]
    pub fn from_json(value: &JsonValue) -> Self {
        match value {
            JsonValue::Null => Self::Null,
            JsonValue::Bool(b) => Self::Bool(*b),
            JsonValue::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Self::Integer(i)
                } else if let Some(u) = n.as_u64() {
                    Self::UInteger(u)
                } else if let Some(f) = n.as_f64() {
                    Self::Float(f)
                } else {
                    Self::Null
                }
            }
            JsonValue::String(s) => Self::String(s.clone()),
            JsonValue::Array(arr) => Self::Array(arr.iter().map(Self::from_json).collect()),
            JsonValue::Object(obj) => Self::Object(
                obj.iter()
                    .map(|(k, v)| (k.clone(), Self::from_json(v)))
                    .collect(),
            ),
        }
    }

    /// Tries to convert to string
    #[must_use]
    pub fn as_string(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s),
            _ => None,
        }
    }

    /// Tries to convert to i64
    #[must_use]
    pub const fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Integer(i) => Some(*i),
            _ => None,
        }
    }

    /// Tries to convert to u64
    #[must_use]
    pub const fn as_u64(&self) -> Option<u64> {
        match self {
            Self::UInteger(u) => Some(*u),
            _ => None,
        }
    }

    /// Tries to convert to f64
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Float(f) => Some(*f),
            Self::Integer(i) => Some(*i as f64),
            Self::UInteger(u) => Some(*u as f64),
            _ => None,
        }
    }

    /// Tries to convert to bool
    #[must_use]
    pub const fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

impl From<bool> for PropertyValue {
    fn from(b: bool) -> Self {
        Self::Bool(b)
    }
}

impl From<i64> for PropertyValue {
    fn from(i: i64) -> Self {
        Self::Integer(i)
    }
}

impl From<i32> for PropertyValue {
    fn from(i: i32) -> Self {
        Self::Integer(i64::from(i))
    }
}

impl From<u64> for PropertyValue {
    fn from(u: u64) -> Self {
        Self::UInteger(u)
    }
}

impl From<u32> for PropertyValue {
    fn from(u: u32) -> Self {
        Self::UInteger(u64::from(u))
    }
}

impl From<f64> for PropertyValue {
    fn from(f: f64) -> Self {
        Self::Float(f)
    }
}

impl From<f32> for PropertyValue {
    fn from(f: f32) -> Self {
        Self::Float(f64::from(f))
    }
}

impl From<String> for PropertyValue {
    fn from(s: String) -> Self {
        Self::String(s)
    }
}

impl From<&str> for PropertyValue {
    fn from(s: &str) -> Self {
        Self::String(s.to_string())
    }
}

/// A collection of features
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureCollection {
    /// Features in the collection
    pub features: Vec<Feature>,
    /// Optional metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, PropertyValue>>,
}

impl FeatureCollection {
    /// Creates a new feature collection
    #[must_use]
    pub const fn new(features: Vec<Feature>) -> Self {
        Self {
            features,
            metadata: None,
        }
    }

    /// Creates an empty feature collection
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            features: Vec::new(),
            metadata: None,
        }
    }

    /// Adds a feature to the collection
    pub fn push(&mut self, feature: Feature) {
        self.features.push(feature);
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

    /// Computes the bounding box of all features
    #[must_use]
    pub fn bounds(&self) -> Option<(f64, f64, f64, f64)> {
        if self.features.is_empty() {
            return None;
        }

        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;

        for feature in &self.features {
            if let Some((x_min, y_min, x_max, y_max)) = feature.bounds() {
                min_x = min_x.min(x_min);
                min_y = min_y.min(y_min);
                max_x = max_x.max(x_max);
                max_y = max_y.max(y_max);
            }
        }

        if min_x.is_infinite() {
            None
        } else {
            Some((min_x, min_y, max_x, max_y))
        }
    }
}

impl Default for FeatureCollection {
    fn default() -> Self {
        Self::empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vector::geometry::Point;

    #[test]
    fn test_feature_creation() {
        let point = Point::new(1.0, 2.0);
        let mut feature = Feature::new(Geometry::Point(point));

        feature.set_property("name", PropertyValue::String("Test Point".to_string()));
        feature.set_property("value", PropertyValue::Integer(42));

        assert!(feature.has_geometry());
        assert_eq!(feature.properties.len(), 2);

        let name = feature.get_property("name");
        assert!(name.is_some());
        assert_eq!(name.and_then(|v| v.as_string()), Some("Test Point"));

        let value = feature.get_property("value");
        assert!(value.is_some());
        assert_eq!(value.and_then(|v| v.as_i64()), Some(42));
    }

    #[test]
    fn test_feature_id() {
        let point = Point::new(1.0, 2.0);
        let feature = Feature::with_id(FeatureId::Integer(123), Geometry::Point(point));

        assert_eq!(feature.id, Some(FeatureId::Integer(123)));
    }

    #[test]
    fn test_property_value_conversions() {
        let pv_int = PropertyValue::from(42_i64);
        assert_eq!(pv_int.as_i64(), Some(42));

        let pv_float = PropertyValue::from(2.78_f64);
        assert_eq!(pv_float.as_f64(), Some(2.78));

        let pv_bool = PropertyValue::from(true);
        assert_eq!(pv_bool.as_bool(), Some(true));

        let pv_str = PropertyValue::from("hello");
        assert_eq!(pv_str.as_string(), Some("hello"));
    }

    #[test]
    fn test_feature_collection() {
        let mut collection = FeatureCollection::empty();
        assert!(collection.is_empty());

        let point1 = Point::new(1.0, 2.0);
        let feature1 = Feature::new(Geometry::Point(point1));
        collection.push(feature1);

        let point2 = Point::new(3.0, 4.0);
        let feature2 = Feature::new(Geometry::Point(point2));
        collection.push(feature2);

        assert_eq!(collection.len(), 2);
        assert!(!collection.is_empty());

        let bounds = collection.bounds();
        assert!(bounds.is_some());
        let (min_x, min_y, max_x, max_y) = bounds.expect("bounds calculation failed");
        assert_eq!(min_x, 1.0);
        assert_eq!(min_y, 2.0);
        assert_eq!(max_x, 3.0);
        assert_eq!(max_y, 4.0);
    }
}
