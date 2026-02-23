//! HDF5 attribute handling for metadata storage and retrieval.
//!
//! Attributes are small datasets attached to groups or datasets that store metadata.

use crate::datatype::Datatype;
use crate::error::{Hdf5Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// HDF5 attribute value
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AttributeValue {
    /// 8-bit signed integer
    Int8(i8),
    /// 8-bit unsigned integer
    UInt8(u8),
    /// 16-bit signed integer
    Int16(i16),
    /// 16-bit unsigned integer
    UInt16(u16),
    /// 32-bit signed integer
    Int32(i32),
    /// 32-bit unsigned integer
    UInt32(u32),
    /// 64-bit signed integer
    Int64(i64),
    /// 64-bit unsigned integer
    UInt64(u64),
    /// 32-bit floating-point
    Float32(f32),
    /// 64-bit floating-point
    Float64(f64),
    /// String value
    String(String),
    /// Array of 8-bit signed integers
    Int8Array(Vec<i8>),
    /// Array of 8-bit unsigned integers
    UInt8Array(Vec<u8>),
    /// Array of 16-bit signed integers
    Int16Array(Vec<i16>),
    /// Array of 16-bit unsigned integers
    UInt16Array(Vec<u16>),
    /// Array of 32-bit signed integers
    Int32Array(Vec<i32>),
    /// Array of 32-bit unsigned integers
    UInt32Array(Vec<u32>),
    /// Array of 64-bit signed integers
    Int64Array(Vec<i64>),
    /// Array of 64-bit unsigned integers
    UInt64Array(Vec<u64>),
    /// Array of 32-bit floating-point
    Float32Array(Vec<f32>),
    /// Array of 64-bit floating-point
    Float64Array(Vec<f64>),
    /// Array of strings
    StringArray(Vec<String>),
}

impl AttributeValue {
    /// Get the datatype of this attribute value
    pub fn datatype(&self) -> Datatype {
        match self {
            Self::Int8(_) | Self::Int8Array(_) => Datatype::Int8,
            Self::UInt8(_) | Self::UInt8Array(_) => Datatype::UInt8,
            Self::Int16(_) | Self::Int16Array(_) => Datatype::Int16,
            Self::UInt16(_) | Self::UInt16Array(_) => Datatype::UInt16,
            Self::Int32(_) | Self::Int32Array(_) => Datatype::Int32,
            Self::UInt32(_) | Self::UInt32Array(_) => Datatype::UInt32,
            Self::Int64(_) | Self::Int64Array(_) => Datatype::Int64,
            Self::UInt64(_) | Self::UInt64Array(_) => Datatype::UInt64,
            Self::Float32(_) | Self::Float32Array(_) => Datatype::Float32,
            Self::Float64(_) | Self::Float64Array(_) => Datatype::Float64,
            Self::String(s) => Datatype::FixedString {
                length: s.len(),
                padding: crate::datatype::StringPadding::NullTerminated,
            },
            Self::StringArray(arr) => Datatype::FixedString {
                length: arr.first().map(|s| s.len()).unwrap_or(0),
                padding: crate::datatype::StringPadding::NullTerminated,
            },
        }
    }

    /// Get the shape (dimensions) of this attribute value
    pub fn shape(&self) -> Vec<usize> {
        match self {
            Self::Int8(_)
            | Self::UInt8(_)
            | Self::Int16(_)
            | Self::UInt16(_)
            | Self::Int32(_)
            | Self::UInt32(_)
            | Self::Int64(_)
            | Self::UInt64(_)
            | Self::Float32(_)
            | Self::Float64(_)
            | Self::String(_) => vec![],
            Self::Int8Array(v) => vec![v.len()],
            Self::UInt8Array(v) => vec![v.len()],
            Self::Int16Array(v) => vec![v.len()],
            Self::UInt16Array(v) => vec![v.len()],
            Self::Int32Array(v) => vec![v.len()],
            Self::UInt32Array(v) => vec![v.len()],
            Self::Int64Array(v) => vec![v.len()],
            Self::UInt64Array(v) => vec![v.len()],
            Self::Float32Array(v) => vec![v.len()],
            Self::Float64Array(v) => vec![v.len()],
            Self::StringArray(v) => vec![v.len()],
        }
    }

    /// Convert to i32 if possible
    pub fn as_i32(&self) -> Result<i32> {
        match self {
            Self::Int8(v) => Ok(*v as i32),
            Self::UInt8(v) => Ok(*v as i32),
            Self::Int16(v) => Ok(*v as i32),
            Self::UInt16(v) => Ok(*v as i32),
            Self::Int32(v) => Ok(*v),
            Self::UInt32(v) => {
                i32::try_from(*v).map_err(|_| Hdf5Error::type_conversion("u32", "i32"))
            }
            Self::Int64(v) => {
                i32::try_from(*v).map_err(|_| Hdf5Error::type_conversion("i64", "i32"))
            }
            Self::UInt64(v) => {
                i32::try_from(*v).map_err(|_| Hdf5Error::type_conversion("u64", "i32"))
            }
            _ => Err(Hdf5Error::type_conversion(self.datatype().name(), "i32")),
        }
    }

    /// Convert to f64 if possible
    pub fn as_f64(&self) -> Result<f64> {
        match self {
            Self::Int8(v) => Ok(*v as f64),
            Self::UInt8(v) => Ok(*v as f64),
            Self::Int16(v) => Ok(*v as f64),
            Self::UInt16(v) => Ok(*v as f64),
            Self::Int32(v) => Ok(*v as f64),
            Self::UInt32(v) => Ok(*v as f64),
            Self::Int64(v) => Ok(*v as f64),
            Self::UInt64(v) => Ok(*v as f64),
            Self::Float32(v) => Ok(*v as f64),
            Self::Float64(v) => Ok(*v),
            _ => Err(Hdf5Error::type_conversion(self.datatype().name(), "f64")),
        }
    }

    /// Convert to string if possible
    pub fn as_string(&self) -> Result<String> {
        match self {
            Self::String(s) => Ok(s.clone()),
            Self::Int8(v) => Ok(v.to_string()),
            Self::UInt8(v) => Ok(v.to_string()),
            Self::Int16(v) => Ok(v.to_string()),
            Self::UInt16(v) => Ok(v.to_string()),
            Self::Int32(v) => Ok(v.to_string()),
            Self::UInt32(v) => Ok(v.to_string()),
            Self::Int64(v) => Ok(v.to_string()),
            Self::UInt64(v) => Ok(v.to_string()),
            Self::Float32(v) => Ok(v.to_string()),
            Self::Float64(v) => Ok(v.to_string()),
            _ => Err(Hdf5Error::type_conversion(self.datatype().name(), "string")),
        }
    }

    /// Convert to i32 array if possible
    pub fn as_i32_array(&self) -> Result<Vec<i32>> {
        match self {
            Self::Int32Array(v) => Ok(v.clone()),
            Self::Int8Array(v) => Ok(v.iter().map(|&x| x as i32).collect()),
            Self::UInt8Array(v) => Ok(v.iter().map(|&x| x as i32).collect()),
            Self::Int16Array(v) => Ok(v.iter().map(|&x| x as i32).collect()),
            Self::UInt16Array(v) => Ok(v.iter().map(|&x| x as i32).collect()),
            _ => Err(Hdf5Error::type_conversion(self.datatype().name(), "i32[]")),
        }
    }

    /// Convert to f64 array if possible
    pub fn as_f64_array(&self) -> Result<Vec<f64>> {
        match self {
            Self::Float64Array(v) => Ok(v.clone()),
            Self::Float32Array(v) => Ok(v.iter().map(|&x| x as f64).collect()),
            Self::Int8Array(v) => Ok(v.iter().map(|&x| x as f64).collect()),
            Self::UInt8Array(v) => Ok(v.iter().map(|&x| x as f64).collect()),
            Self::Int16Array(v) => Ok(v.iter().map(|&x| x as f64).collect()),
            Self::UInt16Array(v) => Ok(v.iter().map(|&x| x as f64).collect()),
            Self::Int32Array(v) => Ok(v.iter().map(|&x| x as f64).collect()),
            Self::UInt32Array(v) => Ok(v.iter().map(|&x| x as f64).collect()),
            _ => Err(Hdf5Error::type_conversion(self.datatype().name(), "f64[]")),
        }
    }

    /// Convert to string array if possible
    pub fn as_string_array(&self) -> Result<Vec<String>> {
        match self {
            Self::StringArray(v) => Ok(v.clone()),
            _ => Err(Hdf5Error::type_conversion(
                self.datatype().name(),
                "string[]",
            )),
        }
    }
}

/// HDF5 attribute
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Attribute {
    /// Attribute name
    name: String,
    /// Attribute value
    value: AttributeValue,
}

impl Attribute {
    /// Create a new attribute
    pub fn new(name: String, value: AttributeValue) -> Self {
        Self { name, value }
    }

    /// Create an i32 attribute
    pub fn i32(name: impl Into<String>, value: i32) -> Self {
        Self::new(name.into(), AttributeValue::Int32(value))
    }

    /// Create a f64 attribute
    pub fn f64(name: impl Into<String>, value: f64) -> Self {
        Self::new(name.into(), AttributeValue::Float64(value))
    }

    /// Create a string attribute
    pub fn string(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self::new(name.into(), AttributeValue::String(value.into()))
    }

    /// Create an i32 array attribute
    pub fn i32_array(name: impl Into<String>, value: Vec<i32>) -> Self {
        Self::new(name.into(), AttributeValue::Int32Array(value))
    }

    /// Create a f64 array attribute
    pub fn f64_array(name: impl Into<String>, value: Vec<f64>) -> Self {
        Self::new(name.into(), AttributeValue::Float64Array(value))
    }

    /// Create a string array attribute
    pub fn string_array(name: impl Into<String>, value: Vec<String>) -> Self {
        Self::new(name.into(), AttributeValue::StringArray(value))
    }

    /// Get the attribute name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the attribute value
    pub fn value(&self) -> &AttributeValue {
        &self.value
    }

    /// Get the datatype
    pub fn datatype(&self) -> Datatype {
        self.value.datatype()
    }

    /// Get the shape
    pub fn shape(&self) -> Vec<usize> {
        self.value.shape()
    }

    /// Get as i32 if possible
    pub fn as_i32(&self) -> Result<i32> {
        self.value.as_i32()
    }

    /// Get as f64 if possible
    pub fn as_f64(&self) -> Result<f64> {
        self.value.as_f64()
    }

    /// Get as string if possible
    pub fn as_string(&self) -> Result<String> {
        self.value.as_string()
    }

    /// Get as i32 array if possible
    pub fn as_i32_array(&self) -> Result<Vec<i32>> {
        self.value.as_i32_array()
    }

    /// Get as f64 array if possible
    pub fn as_f64_array(&self) -> Result<Vec<f64>> {
        self.value.as_f64_array()
    }

    /// Get as string array if possible
    pub fn as_string_array(&self) -> Result<Vec<String>> {
        self.value.as_string_array()
    }
}

/// Collection of attributes
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Attributes {
    /// Map of attribute name to attribute
    attributes: HashMap<String, Attribute>,
}

impl Attributes {
    /// Create a new attributes collection
    pub fn new() -> Self {
        Self {
            attributes: HashMap::new(),
        }
    }

    /// Add an attribute
    pub fn add(&mut self, attribute: Attribute) {
        self.attributes
            .insert(attribute.name().to_string(), attribute);
    }

    /// Get an attribute by name
    pub fn get(&self, name: &str) -> Result<&Attribute> {
        self.attributes
            .get(name)
            .ok_or_else(|| Hdf5Error::attribute_not_found(name))
    }

    /// Check if an attribute exists
    pub fn contains(&self, name: &str) -> bool {
        self.attributes.contains_key(name)
    }

    /// Get all attribute names
    pub fn names(&self) -> Vec<&str> {
        self.attributes.keys().map(|s| s.as_str()).collect()
    }

    /// Get the number of attributes
    pub fn len(&self) -> usize {
        self.attributes.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.attributes.is_empty()
    }

    /// Iterate over attributes
    pub fn iter(&self) -> impl Iterator<Item = &Attribute> {
        self.attributes.values()
    }

    /// Remove an attribute
    pub fn remove(&mut self, name: &str) -> Option<Attribute> {
        self.attributes.remove(name)
    }

    /// Clear all attributes
    pub fn clear(&mut self) {
        self.attributes.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attribute_value_i32() {
        let value = AttributeValue::Int32(42);
        assert_eq!(value.as_i32().ok(), Some(42));
        assert_eq!(value.as_f64().ok(), Some(42.0));
        assert_eq!(value.as_string().ok(), Some("42".to_string()));
        assert_eq!(value.shape(), Vec::<usize>::new());
    }

    #[test]
    fn test_attribute_value_f64() {
        let value = AttributeValue::Float64(3.125);
        assert_eq!(value.as_f64().ok(), Some(3.125));
        assert!(value.as_i32().is_err());
        assert_eq!(value.shape(), Vec::<usize>::new());
    }

    #[test]
    fn test_attribute_value_string() {
        let value = AttributeValue::String("hello".to_string());
        assert_eq!(value.as_string().ok(), Some("hello".to_string()));
        assert!(value.as_i32().is_err());
        assert_eq!(value.shape(), Vec::<usize>::new());
    }

    #[test]
    fn test_attribute_value_array() {
        let value = AttributeValue::Int32Array(vec![1, 2, 3, 4, 5]);
        assert_eq!(value.as_i32_array().ok(), Some(vec![1, 2, 3, 4, 5]));
        assert_eq!(
            value.as_f64_array().ok(),
            Some(vec![1.0, 2.0, 3.0, 4.0, 5.0])
        );
        assert_eq!(value.shape(), vec![5]);
    }

    #[test]
    fn test_attribute_creation() {
        let attr = Attribute::i32("version", 1);
        assert_eq!(attr.name(), "version");
        assert_eq!(attr.as_i32().ok(), Some(1));

        let attr = Attribute::f64("scale", 0.5);
        assert_eq!(attr.name(), "scale");
        assert_eq!(attr.as_f64().ok(), Some(0.5));

        let attr = Attribute::string("units", "meters");
        assert_eq!(attr.name(), "units");
        assert_eq!(attr.as_string().ok(), Some("meters".to_string()));
    }

    #[test]
    fn test_attributes_collection() {
        let mut attrs = Attributes::new();
        assert!(attrs.is_empty());

        attrs.add(Attribute::i32("version", 1));
        attrs.add(Attribute::f64("scale", 0.5));
        attrs.add(Attribute::string("units", "meters"));

        assert_eq!(attrs.len(), 3);
        assert!(attrs.contains("version"));
        assert!(attrs.contains("scale"));
        assert!(attrs.contains("units"));
        assert!(!attrs.contains("nonexistent"));

        let version = attrs.get("version").expect("version not found");
        assert_eq!(version.as_i32().ok(), Some(1));

        let scale = attrs.get("scale").expect("scale not found");
        assert_eq!(scale.as_f64().ok(), Some(0.5));

        let units = attrs.get("units").expect("units not found");
        assert_eq!(units.as_string().ok(), Some("meters".to_string()));

        assert!(attrs.get("nonexistent").is_err());

        attrs.remove("version");
        assert_eq!(attrs.len(), 2);
        assert!(!attrs.contains("version"));

        attrs.clear();
        assert!(attrs.is_empty());
    }

    #[test]
    fn test_attributes_names() {
        let mut attrs = Attributes::new();
        attrs.add(Attribute::i32("a", 1));
        attrs.add(Attribute::i32("b", 2));
        attrs.add(Attribute::i32("c", 3));

        let mut names = attrs.names();
        names.sort();
        assert_eq!(names, vec!["a", "b", "c"]);
    }
}
