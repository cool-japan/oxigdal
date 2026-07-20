//! Feature types for vector data
//!
//! A feature is a geometry with associated properties (attributes).

use crate::vector::geometry::Geometry;
use core::fmt;
use serde::{Deserialize, Serialize};
#[cfg(feature = "std")]
use serde_json::Value as JsonValue;

#[cfg(feature = "std")]
use std::collections::HashMap;
#[cfg(feature = "std")]
use std::string::String;

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{
    collections::BTreeMap as HashMap,
    string::{String, ToString},
    vec::Vec,
};

/// A feature with geometry and properties
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Feature {
    /// Optional feature ID
    pub id: Option<FeatureId>,
    /// Geometry (may be None for attribute-only features)
    pub geometry: Option<Geometry>,
    /// Feature properties as key-value pairs
    pub properties: HashMap<String, FieldValue>,
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
    pub fn set_property<S: Into<String>>(&mut self, key: S, value: FieldValue) {
        self.properties.insert(key.into(), value);
    }

    /// Gets a property value
    #[must_use]
    pub fn get_property(&self, key: &str) -> Option<&FieldValue> {
        self.properties.get(key)
    }

    /// Removes a property
    pub fn remove_property(&mut self, key: &str) -> Option<FieldValue> {
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

/// A typed field value for feature attributes.
///
/// This enum covers all primitive and composite types that can appear as
/// vector feature properties.  It replaces the former `PropertyValue` type
/// and extends it with [`FieldValue::Date`] and [`FieldValue::Blob`] variants.
///
/// # Variant ordering and serde
///
/// The enum is tagged with `#[serde(untagged)]`, so deserialization probes
/// variants in declaration order.  `Array` is declared before `Blob` so that
/// JSON arrays whose elements happen to fit in `0..=255` are still decoded as
/// `Array`, not accidentally as `Blob`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FieldValue {
    /// Null value
    Null,
    /// Boolean value
    Bool(bool),
    /// Signed integer value (i64)
    Integer(i64),
    /// Unsigned integer value (u64)
    UInteger(u64),
    /// Floating-point value (f64)
    Float(f64),
    /// UTF-8 string value
    String(String),
    /// Array of field values
    Array(Vec<FieldValue>),
    /// JSON object (nested key-value mapping)
    Object(HashMap<String, FieldValue>),
    /// Calendar date
    ///
    /// Only available when the `std` feature is enabled.
    #[cfg(feature = "std")]
    Date(time::Date),
    /// Raw binary blob
    Blob(Vec<u8>),
}

impl FieldValue {
    /// Returns `true` if the value is [`FieldValue::Null`].
    #[must_use]
    pub const fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Converts to a [`serde_json::Value`].
    ///
    /// This is an alias for [`to_json`](Self::to_json) and produces identical
    /// output. Prefer `to_json_value` in new code for clarity.
    #[cfg(feature = "std")]
    #[must_use]
    pub fn to_json_value(&self) -> JsonValue {
        match self {
            Self::Null => JsonValue::Null,
            Self::Bool(b) => JsonValue::Bool(*b),
            Self::Integer(i) => JsonValue::Number((*i).into()),
            Self::UInteger(u) => JsonValue::Number((*u).into()),
            Self::Float(f) => {
                JsonValue::Number(serde_json::Number::from_f64(*f).unwrap_or_else(|| 0.into()))
            }
            Self::String(s) => JsonValue::String(s.clone()),
            Self::Array(arr) => JsonValue::Array(arr.iter().map(Self::to_json_value).collect()),
            Self::Object(obj) => JsonValue::Object(
                obj.iter()
                    .map(|(k, v)| (k.clone(), v.to_json_value()))
                    .collect(),
            ),
            #[cfg(feature = "std")]
            Self::Date(d) => JsonValue::String(d.to_string()),
            Self::Blob(bytes) => JsonValue::Array(
                bytes
                    .iter()
                    .map(|b| JsonValue::Number(u64::from(*b).into()))
                    .collect(),
            ),
        }
    }

    /// Converts to a [`serde_json::Value`].
    ///
    /// Kept for backward compatibility; delegates to [`to_json_value`](Self::to_json_value).
    #[cfg(feature = "std")]
    #[must_use]
    pub fn to_json(&self) -> JsonValue {
        self.to_json_value()
    }

    /// Creates a [`FieldValue`] from a [`serde_json::Value`].
    ///
    /// JSON arrays are decoded as [`FieldValue::Array`]; raw blobs must be
    /// constructed directly.  JSON strings are decoded as
    /// [`FieldValue::String`] without attempting date parsing.
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

    /// Returns the string contents if this is a [`FieldValue::String`] variant.
    #[must_use]
    pub fn as_string(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s),
            _ => None,
        }
    }

    /// Returns the integer value if this is a [`FieldValue::Integer`] variant.
    #[must_use]
    pub const fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Integer(i) => Some(*i),
            _ => None,
        }
    }

    /// Returns the unsigned integer value if this is a [`FieldValue::UInteger`] variant.
    #[must_use]
    pub const fn as_u64(&self) -> Option<u64> {
        match self {
            Self::UInteger(u) => Some(*u),
            _ => None,
        }
    }

    /// Returns the float value if this is a numeric variant.
    ///
    /// Coerces `Integer` and `UInteger` to `f64` automatically.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Float(f) => Some(*f),
            Self::Integer(i) => Some(*i as f64),
            Self::UInteger(u) => Some(*u as f64),
            _ => None,
        }
    }

    /// Returns the boolean value if this is a [`FieldValue::Bool`] variant.
    #[must_use]
    pub const fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Returns the byte slice if this is a [`FieldValue::Blob`] variant.
    #[must_use]
    pub fn as_blob(&self) -> Option<&[u8]> {
        match self {
            Self::Blob(b) => Some(b),
            _ => None,
        }
    }

    /// Returns the [`time::Date`] if this is a [`FieldValue::Date`] variant.
    #[cfg(feature = "std")]
    #[must_use]
    pub const fn as_date(&self) -> Option<time::Date> {
        match self {
            Self::Date(d) => Some(*d),
            _ => None,
        }
    }
}

impl fmt::Display for FieldValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => f.write_str("null"),
            Self::Bool(b) => write!(f, "{b}"),
            Self::Integer(i) => write!(f, "{i}"),
            Self::UInteger(u) => write!(f, "{u}"),
            Self::Float(v) => write!(f, "{v}"),
            Self::String(s) => write!(f, "\"{s}\""),
            Self::Array(arr) => {
                write!(f, "[")?;
                for (i, v) in arr.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{v}")?;
                }
                write!(f, "]")
            }
            Self::Object(obj) => {
                write!(f, "{{")?;
                let mut first = true;
                for (k, v) in obj.iter() {
                    if !first {
                        write!(f, ", ")?;
                    }
                    first = false;
                    write!(f, "\"{k}\": {v}")?;
                }
                write!(f, "}}")
            }
            #[cfg(feature = "std")]
            Self::Date(d) => write!(f, "{d}"),
            Self::Blob(b) => write!(f, "Blob({} bytes)", b.len()),
        }
    }
}

#[cfg(feature = "std")]
impl From<serde_json::Value> for FieldValue {
    fn from(v: serde_json::Value) -> Self {
        Self::from_json(&v)
    }
}

impl From<bool> for FieldValue {
    fn from(b: bool) -> Self {
        Self::Bool(b)
    }
}

impl From<i64> for FieldValue {
    fn from(i: i64) -> Self {
        Self::Integer(i)
    }
}

impl From<i32> for FieldValue {
    fn from(i: i32) -> Self {
        Self::Integer(i64::from(i))
    }
}

impl From<u64> for FieldValue {
    fn from(u: u64) -> Self {
        Self::UInteger(u)
    }
}

impl From<u32> for FieldValue {
    fn from(u: u32) -> Self {
        Self::UInteger(u64::from(u))
    }
}

impl From<f64> for FieldValue {
    fn from(f: f64) -> Self {
        Self::Float(f)
    }
}

impl From<f32> for FieldValue {
    fn from(f: f32) -> Self {
        Self::Float(f64::from(f))
    }
}

impl From<String> for FieldValue {
    fn from(s: String) -> Self {
        Self::String(s)
    }
}

impl From<&str> for FieldValue {
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
    pub metadata: Option<HashMap<String, FieldValue>>,
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
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::vector::geometry::Point;

    #[test]
    fn test_feature_creation() {
        let point = Point::new(1.0, 2.0);
        let mut feature = Feature::new(Geometry::Point(point));

        feature.set_property("name", FieldValue::String("Test Point".to_string()));
        feature.set_property("value", FieldValue::Integer(42));

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
    fn test_field_value_conversions() {
        let pv_int = FieldValue::from(42_i64);
        assert_eq!(pv_int.as_i64(), Some(42));

        let pv_float = FieldValue::from(2.78_f64);
        assert_eq!(pv_float.as_f64(), Some(2.78));

        let pv_bool = FieldValue::from(true);
        assert_eq!(pv_bool.as_bool(), Some(true));

        let pv_str = FieldValue::from("hello");
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

    #[test]
    fn test_fieldvalue_variants_exhaustive() {
        let _ = FieldValue::Null;
        let _ = FieldValue::Bool(true);
        let _ = FieldValue::Integer(-1);
        let _ = FieldValue::UInteger(1u64);
        let _ = FieldValue::Float(1.5);
        #[cfg(feature = "std")]
        let _ = FieldValue::Date(
            time::Date::from_calendar_date(2024, time::Month::January, 1).expect("valid date"),
        );
        let _ = FieldValue::Blob(vec![0u8]);
        let _ = FieldValue::String("x".into());
        let _ = FieldValue::Array(vec![]);
        let _ = FieldValue::Object(Default::default());
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_fieldvalue_to_json_value_all_variants() {
        assert_eq!(FieldValue::Null.to_json_value(), serde_json::Value::Null);
        assert_eq!(
            FieldValue::Bool(true).to_json_value(),
            serde_json::Value::Bool(true)
        );
        let blob_json = FieldValue::Blob(vec![1, 2]).to_json_value();
        assert!(matches!(blob_json, serde_json::Value::Array(_)));
        if let serde_json::Value::Array(a) = blob_json {
            assert_eq!(a.len(), 2);
        }
        // Date variant
        let date =
            time::Date::from_calendar_date(2024, time::Month::March, 15).expect("valid date");
        let json = FieldValue::Date(date).to_json_value();
        assert!(matches!(json, serde_json::Value::String(_)));
        if let serde_json::Value::String(s) = json {
            assert!(s.contains("2024"), "date string should contain year");
        }
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_from_json_value() {
        let json = serde_json::json!({
            "name": "test",
            "count": 42,
            "flag": true,
            "score": 3.5,
            "items": [1, 2, 3]
        });
        let fv = FieldValue::from(json);
        assert!(matches!(fv, FieldValue::Object(_)));
        if let FieldValue::Object(map) = fv {
            assert!(map.contains_key("name"));
            assert!(map.contains_key("count"));
        }
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_blob_accessor() {
        let fv = FieldValue::Blob(vec![0xDE, 0xAD, 0xBE, 0xEF]);
        assert_eq!(fv.as_blob(), Some([0xDE, 0xAD, 0xBE, 0xEFu8].as_ref()));
    }

    #[test]
    fn test_display_fieldvalue_each_variant() {
        assert_eq!(FieldValue::Null.to_string(), "null");
        assert_eq!(FieldValue::Bool(true).to_string(), "true");
        assert_eq!(FieldValue::Bool(false).to_string(), "false");
        assert_eq!(FieldValue::Integer(-42).to_string(), "-42");
        assert_eq!(FieldValue::UInteger(99).to_string(), "99");
        assert_eq!(FieldValue::String("hello".into()).to_string(), "\"hello\"");
        assert_eq!(FieldValue::Blob(vec![1, 2, 3]).to_string(), "Blob(3 bytes)");
        assert_eq!(
            FieldValue::Array(vec![FieldValue::Integer(1)]).to_string(),
            "[1]"
        );
    }
}
