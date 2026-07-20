//! NetCDF attribute types and utilities.
//!
//! Attributes provide metadata for NetCDF files, groups, and variables.
//! They can store text, numbers, or arrays of values.

use serde::{Deserialize, Serialize};

use crate::error::{NetCdfError, Result};

/// Attribute value types supported by NetCDF.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AttributeValue {
    /// Text attribute
    Text(String),
    /// 8-bit signed integer
    I8(Vec<i8>),
    /// 8-bit unsigned integer
    U8(Vec<u8>),
    /// 16-bit signed integer
    I16(Vec<i16>),
    /// 16-bit unsigned integer (NetCDF-4 only)
    U16(Vec<u16>),
    /// 32-bit signed integer
    I32(Vec<i32>),
    /// 32-bit unsigned integer (NetCDF-4 only)
    U32(Vec<u32>),
    /// 64-bit signed integer (NetCDF-4 only)
    I64(Vec<i64>),
    /// 64-bit unsigned integer (NetCDF-4 only)
    U64(Vec<u64>),
    /// 32-bit floating point
    F32(Vec<f32>),
    /// 64-bit floating point
    F64(Vec<f64>),
}

impl AttributeValue {
    /// Create a text attribute.
    #[must_use]
    pub fn text(s: impl Into<String>) -> Self {
        Self::Text(s.into())
    }

    /// Create a single i8 attribute.
    #[must_use]
    pub fn i8(value: i8) -> Self {
        Self::I8(vec![value])
    }

    /// Create an i8 array attribute.
    #[must_use]
    pub fn i8_array(values: Vec<i8>) -> Self {
        Self::I8(values)
    }

    /// Create a single u8 attribute.
    #[must_use]
    pub fn u8(value: u8) -> Self {
        Self::U8(vec![value])
    }

    /// Create a u8 array attribute.
    #[must_use]
    pub fn u8_array(values: Vec<u8>) -> Self {
        Self::U8(values)
    }

    /// Create a single i16 attribute.
    #[must_use]
    pub fn i16(value: i16) -> Self {
        Self::I16(vec![value])
    }

    /// Create an i16 array attribute.
    #[must_use]
    pub fn i16_array(values: Vec<i16>) -> Self {
        Self::I16(values)
    }

    /// Create a single u16 attribute (NetCDF-4 only).
    #[must_use]
    pub fn u16(value: u16) -> Self {
        Self::U16(vec![value])
    }

    /// Create a u16 array attribute (NetCDF-4 only).
    #[must_use]
    pub fn u16_array(values: Vec<u16>) -> Self {
        Self::U16(values)
    }

    /// Create a single i32 attribute.
    #[must_use]
    pub fn i32(value: i32) -> Self {
        Self::I32(vec![value])
    }

    /// Create an i32 array attribute.
    #[must_use]
    pub fn i32_array(values: Vec<i32>) -> Self {
        Self::I32(values)
    }

    /// Create a single u32 attribute (NetCDF-4 only).
    #[must_use]
    pub fn u32(value: u32) -> Self {
        Self::U32(vec![value])
    }

    /// Create a u32 array attribute (NetCDF-4 only).
    #[must_use]
    pub fn u32_array(values: Vec<u32>) -> Self {
        Self::U32(values)
    }

    /// Create a single i64 attribute (NetCDF-4 only).
    #[must_use]
    pub fn i64(value: i64) -> Self {
        Self::I64(vec![value])
    }

    /// Create an i64 array attribute (NetCDF-4 only).
    #[must_use]
    pub fn i64_array(values: Vec<i64>) -> Self {
        Self::I64(values)
    }

    /// Create a single u64 attribute (NetCDF-4 only).
    #[must_use]
    pub fn u64(value: u64) -> Self {
        Self::U64(vec![value])
    }

    /// Create a u64 array attribute (NetCDF-4 only).
    #[must_use]
    pub fn u64_array(values: Vec<u64>) -> Self {
        Self::U64(values)
    }

    /// Create a single f32 attribute.
    #[must_use]
    pub fn f32(value: f32) -> Self {
        Self::F32(vec![value])
    }

    /// Create an f32 array attribute.
    #[must_use]
    pub fn f32_array(values: Vec<f32>) -> Self {
        Self::F32(values)
    }

    /// Create a single f64 attribute.
    #[must_use]
    pub fn f64(value: f64) -> Self {
        Self::F64(vec![value])
    }

    /// Create an f64 array attribute.
    #[must_use]
    pub fn f64_array(values: Vec<f64>) -> Self {
        Self::F64(values)
    }

    /// Get as text if possible.
    ///
    /// # Errors
    ///
    /// Returns error if the attribute is not text.
    pub fn as_text(&self) -> Result<&str> {
        match self {
            Self::Text(s) => Ok(s),
            _ => Err(NetCdfError::AttributeError(
                "Attribute is not text".to_string(),
            )),
        }
    }

    /// Get as i32 if possible.
    ///
    /// # Errors
    ///
    /// Returns error if the attribute is not i32 or has multiple values.
    pub fn as_i32(&self) -> Result<i32> {
        match self {
            Self::I32(values) if values.len() == 1 => Ok(values[0]),
            Self::I32(_) => Err(NetCdfError::AttributeError(
                "Attribute has multiple values".to_string(),
            )),
            _ => Err(NetCdfError::AttributeError(
                "Attribute is not i32".to_string(),
            )),
        }
    }

    /// Get as f32 if possible.
    ///
    /// # Errors
    ///
    /// Returns error if the attribute is not f32 or has multiple values.
    pub fn as_f32(&self) -> Result<f32> {
        match self {
            Self::F32(values) if values.len() == 1 => Ok(values[0]),
            Self::F32(_) => Err(NetCdfError::AttributeError(
                "Attribute has multiple values".to_string(),
            )),
            _ => Err(NetCdfError::AttributeError(
                "Attribute is not f32".to_string(),
            )),
        }
    }

    /// Get as f64 if possible.
    ///
    /// # Errors
    ///
    /// Returns error if the attribute is not f64 or has multiple values.
    pub fn as_f64(&self) -> Result<f64> {
        match self {
            Self::F64(values) if values.len() == 1 => Ok(values[0]),
            Self::F64(_) => Err(NetCdfError::AttributeError(
                "Attribute has multiple values".to_string(),
            )),
            _ => Err(NetCdfError::AttributeError(
                "Attribute is not f64".to_string(),
            )),
        }
    }

    /// Get the type name.
    #[must_use]
    pub const fn type_name(&self) -> &'static str {
        match self {
            Self::Text(_) => "text",
            Self::I8(_) => "i8",
            Self::U8(_) => "u8",
            Self::I16(_) => "i16",
            Self::U16(_) => "u16",
            Self::I32(_) => "i32",
            Self::U32(_) => "u32",
            Self::I64(_) => "i64",
            Self::U64(_) => "u64",
            Self::F32(_) => "f32",
            Self::F64(_) => "f64",
        }
    }

    /// Get the number of values.
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Self::Text(s) => s.len(),
            Self::I8(v) => v.len(),
            Self::U8(v) => v.len(),
            Self::I16(v) => v.len(),
            Self::U16(v) => v.len(),
            Self::I32(v) => v.len(),
            Self::U32(v) => v.len(),
            Self::I64(v) => v.len(),
            Self::U64(v) => v.len(),
            Self::F32(v) => v.len(),
            Self::F64(v) => v.len(),
        }
    }

    /// Check if empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// An attribute with a name and value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Attribute {
    /// Name of the attribute
    name: String,
    /// Value of the attribute
    value: AttributeValue,
}

impl Attribute {
    /// Create a new attribute.
    ///
    /// # Arguments
    ///
    /// * `name` - Name of the attribute
    /// * `value` - Value of the attribute
    ///
    /// # Errors
    ///
    /// Returns error if the name is empty.
    pub fn new(name: impl Into<String>, value: AttributeValue) -> Result<Self> {
        let name = name.into();
        if name.is_empty() {
            return Err(NetCdfError::AttributeError(
                "Attribute name cannot be empty".to_string(),
            ));
        }
        Ok(Self { name, value })
    }

    /// Get the attribute name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the attribute value.
    #[must_use]
    pub const fn value(&self) -> &AttributeValue {
        &self.value
    }

    /// Get a mutable reference to the attribute value.
    pub fn value_mut(&mut self) -> &mut AttributeValue {
        &mut self.value
    }
}

/// Collection of attributes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Attributes {
    attributes: Vec<Attribute>,
}

impl Attributes {
    /// Create a new empty attribute collection.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            attributes: Vec::new(),
        }
    }

    /// Create from a vector of attributes.
    #[must_use]
    pub const fn from_vec(attributes: Vec<Attribute>) -> Self {
        Self { attributes }
    }

    /// Add an attribute.
    ///
    /// # Errors
    ///
    /// Returns error if an attribute with the same name already exists.
    pub fn add(&mut self, attribute: Attribute) -> Result<()> {
        if self.contains(attribute.name()) {
            return Err(NetCdfError::AttributeError(format!(
                "Attribute '{}' already exists",
                attribute.name()
            )));
        }
        self.attributes.push(attribute);
        Ok(())
    }

    /// Add or replace an attribute.
    pub fn set(&mut self, attribute: Attribute) {
        if let Some(existing) = self.get_mut(attribute.name()) {
            *existing = attribute;
        } else {
            self.attributes.push(attribute);
        }
    }

    /// Get an attribute by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Attribute> {
        self.attributes.iter().find(|a| a.name() == name)
    }

    /// Get a mutable reference to an attribute by name.
    pub fn get_mut(&mut self, name: &str) -> Option<&mut Attribute> {
        self.attributes.iter_mut().find(|a| a.name() == name)
    }

    /// Get an attribute value by name.
    #[must_use]
    pub fn get_value(&self, name: &str) -> Option<&AttributeValue> {
        self.get(name).map(|a| a.value())
    }

    /// Check if an attribute exists.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.attributes.iter().any(|a| a.name() == name)
    }

    /// Get the number of attributes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.attributes.len()
    }

    /// Check if empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.attributes.is_empty()
    }

    /// Get an iterator over attributes.
    pub fn iter(&self) -> impl Iterator<Item = &Attribute> {
        self.attributes.iter()
    }

    /// Get names of all attributes.
    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        self.attributes.iter().map(|a| a.name()).collect()
    }

    /// Remove an attribute by name.
    pub fn remove(&mut self, name: &str) -> Option<Attribute> {
        self.attributes
            .iter()
            .position(|a| a.name() == name)
            .map(|index| self.attributes.remove(index))
    }
}

impl IntoIterator for Attributes {
    type Item = Attribute;
    type IntoIter = std::vec::IntoIter<Attribute>;

    fn into_iter(self) -> Self::IntoIter {
        self.attributes.into_iter()
    }
}

impl<'a> IntoIterator for &'a Attributes {
    type Item = &'a Attribute;
    type IntoIter = std::slice::Iter<'a, Attribute>;

    fn into_iter(self) -> Self::IntoIter {
        self.attributes.iter()
    }
}

impl FromIterator<Attribute> for Attributes {
    fn from_iter<T: IntoIterator<Item = Attribute>>(iter: T) -> Self {
        Self {
            attributes: iter.into_iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    // Allow panic and expect in tests - per project policy
    #![allow(clippy::panic)]
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn test_text_attribute() {
        let attr = Attribute::new("title", AttributeValue::text("Test Data"))
            .expect("Failed to create text attribute");
        assert_eq!(attr.name(), "title");
        assert_eq!(
            attr.value().as_text().expect("Failed to get text value"),
            "Test Data"
        );
    }

    #[test]
    fn test_numeric_attribute() {
        let attr = Attribute::new("scale_factor", AttributeValue::f64(1.5))
            .expect("Failed to create numeric attribute");
        assert_eq!(attr.name(), "scale_factor");
        assert_eq!(attr.value().as_f64().expect("Failed to get f64 value"), 1.5);
    }

    #[test]
    fn test_array_attribute() {
        let values = vec![1.0, 2.0, 3.0];
        let attr = Attribute::new("coefficients", AttributeValue::f64_array(values.clone()))
            .expect("Failed to create array attribute");
        assert_eq!(attr.name(), "coefficients");
        match attr.value() {
            AttributeValue::F64(v) => assert_eq!(v, &values),
            other => {
                panic!("Expected F64 attribute value, got {:?}", other);
            }
        }
    }

    #[test]
    fn test_attribute_collection() {
        let mut attrs = Attributes::new();
        attrs
            .add(
                Attribute::new("title", AttributeValue::text("Test"))
                    .expect("Failed to create title attribute"),
            )
            .expect("Failed to add title attribute");
        attrs
            .add(
                Attribute::new("version", AttributeValue::i32(1))
                    .expect("Failed to create version attribute"),
            )
            .expect("Failed to add version attribute");

        assert_eq!(attrs.len(), 2);
        assert!(attrs.contains("title"));
        assert!(attrs.contains("version"));

        let title = attrs.get("title").expect("Failed to get title attribute");
        assert_eq!(
            title.value().as_text().expect("Failed to get text value"),
            "Test"
        );
    }

    #[test]
    fn test_empty_attribute_name() {
        let result = Attribute::new("", AttributeValue::text("test"));
        assert!(result.is_err());
    }

    #[test]
    fn test_duplicate_attribute() {
        let mut attrs = Attributes::new();
        attrs
            .add(
                Attribute::new("test", AttributeValue::i32(1))
                    .expect("Failed to create first test attribute"),
            )
            .expect("Failed to add first test attribute");
        let result = attrs.add(
            Attribute::new("test", AttributeValue::i32(2))
                .expect("Failed to create second test attribute"),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_attribute_set() {
        let mut attrs = Attributes::new();
        attrs.set(
            Attribute::new("test", AttributeValue::i32(1))
                .expect("Failed to create test attribute with value 1"),
        );
        assert_eq!(
            attrs
                .get("test")
                .expect("Failed to get test attribute")
                .value()
                .as_i32()
                .expect("Failed to get i32 value"),
            1
        );

        // Replace existing
        attrs.set(
            Attribute::new("test", AttributeValue::i32(2))
                .expect("Failed to create test attribute with value 2"),
        );
        assert_eq!(
            attrs
                .get("test")
                .expect("Failed to get test attribute after replacement")
                .value()
                .as_i32()
                .expect("Failed to get i32 value after replacement"),
            2
        );
        assert_eq!(attrs.len(), 1);
    }

    #[test]
    fn test_attribute_remove() {
        let mut attrs = Attributes::new();
        attrs.set(
            Attribute::new("test", AttributeValue::i32(1))
                .expect("Failed to create test attribute for removal test"),
        );
        assert!(attrs.contains("test"));

        let removed = attrs.remove("test");
        assert!(removed.is_some());
        assert!(!attrs.contains("test"));
    }
}
