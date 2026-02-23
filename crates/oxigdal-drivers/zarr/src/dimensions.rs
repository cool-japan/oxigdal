//! Enhanced dimension handling for Zarr arrays
//!
//! This module provides advanced dimension features including:
//! - Named dimension support
//! - Dimension coordinate arrays
//! - Dimension attributes
//! - Dimension ordering and transposition
//! - Multi-dimensional indexing by name
//! - Advanced dimension slicing
//!
//! # Named Dimensions
//!
//! Named dimensions allow accessing array dimensions by meaningful names
//! rather than positional indices:
//!
//! ```ignore
//! let dims = NamedDimensions::from_names(vec!["time", "latitude", "longitude"])?;
//! let time_idx = dims.index_of("time")?; // Returns 0
//! ```
//!
//! # Coordinate Arrays
//!
//! Coordinate arrays hold the actual values along each dimension:
//!
//! ```ignore
//! let times = CoordinateArray::Float64(vec![0.0, 1.0, 2.0, 3.0]);
//! let coords = DimensionCoordinates::new("time", times)?;
//! let value = coords.value_at(2)?; // Returns 2.0
//! ```

use crate::dimension::{Shape, Slice};
use crate::error::{MetadataError, Result, ZarrError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Dimension name - a validated identifier for a dimension
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DimensionName(String);

impl DimensionName {
    /// Creates a new dimension name
    ///
    /// # Errors
    /// Returns error if name is empty or contains invalid characters
    pub fn new(name: impl Into<String>) -> Result<Self> {
        let name = name.into();

        if name.is_empty() {
            return Err(ZarrError::InvalidDimension {
                message: "Dimension name cannot be empty".to_string(),
            });
        }

        // Validate name follows identifier rules (alphanumeric + underscore, starts with letter/underscore)
        let mut chars = name.chars();
        let first = chars.next();

        match first {
            Some(c) if c.is_alphabetic() || c == '_' => {}
            _ => {
                return Err(ZarrError::InvalidDimension {
                    message: format!("Dimension name must start with letter or underscore: {name}"),
                });
            }
        }

        if !chars.all(|c| c.is_alphanumeric() || c == '_') {
            return Err(ZarrError::InvalidDimension {
                message: format!("Dimension name contains invalid characters: {name}"),
            });
        }

        Ok(Self(name))
    }

    /// Creates a dimension name without validation (internal use)
    #[must_use]
    pub fn new_unchecked(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// Returns the name as a string slice
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for DimensionName {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl AsRef<str> for DimensionName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Named dimensions collection - maps dimension names to indices
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedDimensions {
    /// Names in order of dimension index
    names: Vec<DimensionName>,
    /// Map from name to index for fast lookup
    #[serde(skip)]
    name_to_index: HashMap<String, usize>,
}

impl NamedDimensions {
    /// Creates named dimensions from a list of names
    ///
    /// # Errors
    /// Returns error if any name is invalid or duplicated
    pub fn from_names(names: Vec<impl Into<String>>) -> Result<Self> {
        let mut dim_names = Vec::with_capacity(names.len());
        let mut name_to_index = HashMap::with_capacity(names.len());

        for (idx, name) in names.into_iter().enumerate() {
            let dim_name = DimensionName::new(name)?;

            if name_to_index.contains_key(dim_name.as_str()) {
                return Err(ZarrError::InvalidDimension {
                    message: format!("Duplicate dimension name: {}", dim_name.as_str()),
                });
            }

            name_to_index.insert(dim_name.as_str().to_string(), idx);
            dim_names.push(dim_name);
        }

        Ok(Self {
            names: dim_names,
            name_to_index,
        })
    }

    /// Creates anonymous dimensions with default names (dim_0, dim_1, ...)
    #[must_use]
    pub fn anonymous(ndim: usize) -> Self {
        let names: Vec<DimensionName> = (0..ndim)
            .map(|i| DimensionName::new_unchecked(format!("dim_{i}")))
            .collect();

        let name_to_index = names
            .iter()
            .enumerate()
            .map(|(idx, name)| (name.as_str().to_string(), idx))
            .collect();

        Self { names, name_to_index }
    }

    /// Returns the number of dimensions
    #[must_use]
    pub fn ndim(&self) -> usize {
        self.names.len()
    }

    /// Returns the index of a dimension by name
    ///
    /// # Errors
    /// Returns error if dimension name is not found
    pub fn index_of(&self, name: &str) -> Result<usize> {
        self.name_to_index.get(name).copied().ok_or_else(|| {
            ZarrError::InvalidDimension {
                message: format!("Dimension not found: {name}"),
            }
        })
    }

    /// Returns the name at a given index
    #[must_use]
    pub fn name_at(&self, index: usize) -> Option<&DimensionName> {
        self.names.get(index)
    }

    /// Returns all dimension names
    #[must_use]
    pub fn names(&self) -> &[DimensionName] {
        &self.names
    }

    /// Checks if a dimension name exists
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.name_to_index.contains_key(name)
    }

    /// Creates a permuted version of the dimensions
    ///
    /// # Errors
    /// Returns error if permutation is invalid
    pub fn permute(&self, permutation: &[usize]) -> Result<Self> {
        if permutation.len() != self.ndim() {
            return Err(ZarrError::InvalidDimension {
                message: format!(
                    "Permutation length {} does not match dimension count {}",
                    permutation.len(),
                    self.ndim()
                ),
            });
        }

        // Validate permutation is a valid permutation
        let mut seen = vec![false; self.ndim()];
        for &idx in permutation {
            if idx >= self.ndim() {
                return Err(ZarrError::InvalidDimension {
                    message: format!("Invalid permutation index: {idx}"),
                });
            }
            if seen[idx] {
                return Err(ZarrError::InvalidDimension {
                    message: format!("Duplicate index in permutation: {idx}"),
                });
            }
            seen[idx] = true;
        }

        let new_names: Vec<String> = permutation
            .iter()
            .filter_map(|&idx| self.names.get(idx).map(|n| n.as_str().to_string()))
            .collect();

        Self::from_names(new_names)
    }

    /// Creates a transposed version (reverses dimension order)
    pub fn transpose(&self) -> Result<Self> {
        let permutation: Vec<usize> = (0..self.ndim()).rev().collect();
        self.permute(&permutation)
    }

    /// Renames a dimension
    ///
    /// # Errors
    /// Returns error if old name doesn't exist or new name is invalid
    pub fn rename(&mut self, old_name: &str, new_name: impl Into<String>) -> Result<()> {
        let idx = self.index_of(old_name)?;
        let new_dim_name = DimensionName::new(new_name)?;

        if self.contains(new_dim_name.as_str()) && new_dim_name.as_str() != old_name {
            return Err(ZarrError::InvalidDimension {
                message: format!("Dimension name already exists: {}", new_dim_name.as_str()),
            });
        }

        self.name_to_index.remove(old_name);
        self.name_to_index.insert(new_dim_name.as_str().to_string(), idx);
        self.names[idx] = new_dim_name;

        Ok(())
    }
}

/// Type of coordinate values
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CoordinateType {
    /// 32-bit signed integers
    Int32,
    /// 64-bit signed integers
    Int64,
    /// 32-bit floating point
    Float32,
    /// 64-bit floating point
    Float64,
    /// String labels
    String,
    /// Datetime values (stored as i64 nanoseconds since epoch)
    Datetime,
}

/// Coordinate array - holds the coordinate values for a dimension
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CoordinateArray {
    /// 32-bit integers
    Int32(Vec<i32>),
    /// 64-bit integers
    Int64(Vec<i64>),
    /// 32-bit floats
    Float32(Vec<f32>),
    /// 64-bit floats
    Float64(Vec<f64>),
    /// String labels
    String(Vec<String>),
    /// Datetime as nanoseconds since epoch
    Datetime(Vec<i64>),
}

impl CoordinateArray {
    /// Returns the length of the coordinate array
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Self::Int32(v) => v.len(),
            Self::Int64(v) => v.len(),
            Self::Float32(v) => v.len(),
            Self::Float64(v) => v.len(),
            Self::String(v) => v.len(),
            Self::Datetime(v) => v.len(),
        }
    }

    /// Checks if the array is empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the coordinate type
    #[must_use]
    pub fn coord_type(&self) -> CoordinateType {
        match self {
            Self::Int32(_) => CoordinateType::Int32,
            Self::Int64(_) => CoordinateType::Int64,
            Self::Float32(_) => CoordinateType::Float32,
            Self::Float64(_) => CoordinateType::Float64,
            Self::String(_) => CoordinateType::String,
            Self::Datetime(_) => CoordinateType::Datetime,
        }
    }

    /// Gets a value at the given index as f64
    ///
    /// # Errors
    /// Returns error if index is out of bounds or type doesn't support f64 conversion
    pub fn value_f64_at(&self, index: usize) -> Result<f64> {
        match self {
            Self::Int32(v) => v.get(index)
                .map(|&x| f64::from(x))
                .ok_or_else(|| ZarrError::OutOfBounds {
                    message: format!("Coordinate index out of bounds: {index}"),
                }),
            Self::Int64(v) => v.get(index)
                .map(|&x| x as f64)
                .ok_or_else(|| ZarrError::OutOfBounds {
                    message: format!("Coordinate index out of bounds: {index}"),
                }),
            Self::Float32(v) => v.get(index)
                .map(|&x| f64::from(x))
                .ok_or_else(|| ZarrError::OutOfBounds {
                    message: format!("Coordinate index out of bounds: {index}"),
                }),
            Self::Float64(v) => v.get(index)
                .copied()
                .ok_or_else(|| ZarrError::OutOfBounds {
                    message: format!("Coordinate index out of bounds: {index}"),
                }),
            Self::Datetime(v) => v.get(index)
                .map(|&x| x as f64)
                .ok_or_else(|| ZarrError::OutOfBounds {
                    message: format!("Coordinate index out of bounds: {index}"),
                }),
            Self::String(_) => Err(ZarrError::NotSupported {
                operation: "String coordinates cannot be converted to f64".to_string(),
            }),
        }
    }

    /// Gets a string value at the given index
    ///
    /// # Errors
    /// Returns error if index is out of bounds
    pub fn value_string_at(&self, index: usize) -> Result<String> {
        match self {
            Self::Int32(v) => v.get(index)
                .map(ToString::to_string)
                .ok_or_else(|| ZarrError::OutOfBounds {
                    message: format!("Coordinate index out of bounds: {index}"),
                }),
            Self::Int64(v) => v.get(index)
                .map(ToString::to_string)
                .ok_or_else(|| ZarrError::OutOfBounds {
                    message: format!("Coordinate index out of bounds: {index}"),
                }),
            Self::Float32(v) => v.get(index)
                .map(ToString::to_string)
                .ok_or_else(|| ZarrError::OutOfBounds {
                    message: format!("Coordinate index out of bounds: {index}"),
                }),
            Self::Float64(v) => v.get(index)
                .map(ToString::to_string)
                .ok_or_else(|| ZarrError::OutOfBounds {
                    message: format!("Coordinate index out of bounds: {index}"),
                }),
            Self::String(v) => v.get(index)
                .cloned()
                .ok_or_else(|| ZarrError::OutOfBounds {
                    message: format!("Coordinate index out of bounds: {index}"),
                }),
            Self::Datetime(v) => v.get(index)
                .map(ToString::to_string)
                .ok_or_else(|| ZarrError::OutOfBounds {
                    message: format!("Coordinate index out of bounds: {index}"),
                }),
        }
    }

    /// Finds the index of a value using linear search
    ///
    /// Returns the index of the first matching value, or None if not found
    #[must_use]
    pub fn find_index_f64(&self, value: f64) -> Option<usize> {
        match self {
            Self::Int32(v) => {
                let target = value as i32;
                v.iter().position(|&x| x == target)
            }
            Self::Int64(v) => {
                let target = value as i64;
                v.iter().position(|&x| x == target)
            }
            Self::Float32(v) => {
                let target = value as f32;
                v.iter().position(|&x| (x - target).abs() < f32::EPSILON)
            }
            Self::Float64(v) => {
                v.iter().position(|&x| (x - value).abs() < f64::EPSILON)
            }
            Self::Datetime(v) => {
                let target = value as i64;
                v.iter().position(|&x| x == target)
            }
            Self::String(_) => None,
        }
    }

    /// Finds the nearest index for a value (for sorted coordinates)
    ///
    /// Assumes coordinates are sorted in ascending order
    #[must_use]
    pub fn find_nearest_index_f64(&self, value: f64) -> Option<usize> {
        match self {
            Self::Float64(v) if !v.is_empty() => {
                // Binary search for nearest
                let mut best_idx = 0;
                let mut best_diff = (v[0] - value).abs();

                for (idx, &x) in v.iter().enumerate().skip(1) {
                    let diff = (x - value).abs();
                    if diff < best_diff {
                        best_diff = diff;
                        best_idx = idx;
                    }
                }

                Some(best_idx)
            }
            Self::Float32(v) if !v.is_empty() => {
                let value_f32 = value as f32;
                let mut best_idx = 0;
                let mut best_diff = (v[0] - value_f32).abs();

                for (idx, &x) in v.iter().enumerate().skip(1) {
                    let diff = (x - value_f32).abs();
                    if diff < best_diff {
                        best_diff = diff;
                        best_idx = idx;
                    }
                }

                Some(best_idx)
            }
            Self::Int64(v) if !v.is_empty() => {
                let target = value as i64;
                let mut best_idx = 0;
                let mut best_diff = (v[0] - target).unsigned_abs();

                for (idx, &x) in v.iter().enumerate().skip(1) {
                    let diff = (x - target).unsigned_abs();
                    if diff < best_diff {
                        best_diff = diff;
                        best_idx = idx;
                    }
                }

                Some(best_idx)
            }
            Self::Int32(v) if !v.is_empty() => {
                let target = value as i32;
                let mut best_idx = 0;
                let mut best_diff = (v[0] - target).unsigned_abs();

                for (idx, &x) in v.iter().enumerate().skip(1) {
                    let diff = (x - target).unsigned_abs();
                    if diff < best_diff {
                        best_diff = diff;
                        best_idx = idx;
                    }
                }

                Some(best_idx)
            }
            _ => None,
        }
    }

    /// Slices the coordinate array
    pub fn slice(&self, start: usize, end: usize, step: usize) -> Result<Self> {
        if step == 0 {
            return Err(ZarrError::InvalidDimension {
                message: "Step cannot be zero".to_string(),
            });
        }

        if start > end || end > self.len() {
            return Err(ZarrError::OutOfBounds {
                message: format!("Invalid slice: {}..{} for length {}", start, end, self.len()),
            });
        }

        Ok(match self {
            Self::Int32(v) => Self::Int32(v.iter().skip(start).take(end - start).step_by(step).copied().collect()),
            Self::Int64(v) => Self::Int64(v.iter().skip(start).take(end - start).step_by(step).copied().collect()),
            Self::Float32(v) => Self::Float32(v.iter().skip(start).take(end - start).step_by(step).copied().collect()),
            Self::Float64(v) => Self::Float64(v.iter().skip(start).take(end - start).step_by(step).copied().collect()),
            Self::String(v) => Self::String(v.iter().skip(start).take(end - start).step_by(step).cloned().collect()),
            Self::Datetime(v) => Self::Datetime(v.iter().skip(start).take(end - start).step_by(step).copied().collect()),
        })
    }
}

/// Dimension coordinates - coordinate values with metadata
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DimensionCoordinates {
    /// Dimension name
    pub name: DimensionName,
    /// Coordinate values
    pub values: CoordinateArray,
    /// Coordinate attributes (CF conventions, etc.)
    pub attributes: DimensionAttributes,
}

impl DimensionCoordinates {
    /// Creates new dimension coordinates
    ///
    /// # Errors
    /// Returns error if name is invalid or values are empty
    pub fn new(name: impl Into<String>, values: CoordinateArray) -> Result<Self> {
        if values.is_empty() {
            return Err(ZarrError::InvalidDimension {
                message: "Coordinate array cannot be empty".to_string(),
            });
        }

        Ok(Self {
            name: DimensionName::new(name)?,
            values,
            attributes: DimensionAttributes::new(),
        })
    }

    /// Creates dimension coordinates with attributes
    pub fn with_attributes(name: impl Into<String>, values: CoordinateArray, attributes: DimensionAttributes) -> Result<Self> {
        let mut coords = Self::new(name, values)?;
        coords.attributes = attributes;
        Ok(coords)
    }

    /// Returns the length of coordinates
    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Checks if empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Gets a value at the given index
    pub fn value_at(&self, index: usize) -> Result<f64> {
        self.values.value_f64_at(index)
    }

    /// Sets the units attribute (CF convention)
    pub fn set_units(&mut self, units: impl Into<String>) {
        self.attributes.set_units(units);
    }

    /// Sets the long name attribute (CF convention)
    pub fn set_long_name(&mut self, long_name: impl Into<String>) {
        self.attributes.set_long_name(long_name);
    }

    /// Sets the calendar attribute (for time dimensions, CF convention)
    pub fn set_calendar(&mut self, calendar: impl Into<String>) {
        self.attributes.set_calendar(calendar);
    }

    /// Slices the coordinates
    pub fn slice(&self, start: usize, end: usize, step: usize) -> Result<Self> {
        Ok(Self {
            name: self.name.clone(),
            values: self.values.slice(start, end, step)?,
            attributes: self.attributes.clone(),
        })
    }
}

/// Dimension attributes - metadata following CF conventions
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DimensionAttributes {
    /// Attribute map
    attrs: HashMap<String, serde_json::Value>,
}

impl DimensionAttributes {
    /// Creates new empty attributes
    #[must_use]
    pub fn new() -> Self {
        Self {
            attrs: HashMap::new(),
        }
    }

    /// Gets an attribute value
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&serde_json::Value> {
        self.attrs.get(key)
    }

    /// Sets an attribute value
    pub fn set(&mut self, key: impl Into<String>, value: serde_json::Value) {
        self.attrs.insert(key.into(), value);
    }

    /// Removes an attribute
    pub fn remove(&mut self, key: &str) -> Option<serde_json::Value> {
        self.attrs.remove(key)
    }

    /// Sets the units attribute (CF convention)
    pub fn set_units(&mut self, units: impl Into<String>) {
        self.set("units", serde_json::Value::String(units.into()));
    }

    /// Gets the units attribute
    #[must_use]
    pub fn units(&self) -> Option<&str> {
        self.attrs.get("units").and_then(|v| v.as_str())
    }

    /// Sets the long name attribute (CF convention)
    pub fn set_long_name(&mut self, long_name: impl Into<String>) {
        self.set("long_name", serde_json::Value::String(long_name.into()));
    }

    /// Gets the long name attribute
    #[must_use]
    pub fn long_name(&self) -> Option<&str> {
        self.attrs.get("long_name").and_then(|v| v.as_str())
    }

    /// Sets the standard name attribute (CF convention)
    pub fn set_standard_name(&mut self, standard_name: impl Into<String>) {
        self.set("standard_name", serde_json::Value::String(standard_name.into()));
    }

    /// Gets the standard name attribute
    #[must_use]
    pub fn standard_name(&self) -> Option<&str> {
        self.attrs.get("standard_name").and_then(|v| v.as_str())
    }

    /// Sets the calendar attribute (for time dimensions, CF convention)
    pub fn set_calendar(&mut self, calendar: impl Into<String>) {
        self.set("calendar", serde_json::Value::String(calendar.into()));
    }

    /// Gets the calendar attribute
    #[must_use]
    pub fn calendar(&self) -> Option<&str> {
        self.attrs.get("calendar").and_then(|v| v.as_str())
    }

    /// Sets the axis attribute (CF convention: X, Y, Z, T)
    pub fn set_axis(&mut self, axis: impl Into<String>) {
        self.set("axis", serde_json::Value::String(axis.into()));
    }

    /// Gets the axis attribute
    #[must_use]
    pub fn axis(&self) -> Option<&str> {
        self.attrs.get("axis").and_then(|v| v.as_str())
    }

    /// Sets positive direction (CF convention: up/down)
    pub fn set_positive(&mut self, positive: impl Into<String>) {
        self.set("positive", serde_json::Value::String(positive.into()));
    }

    /// Gets positive direction
    #[must_use]
    pub fn positive(&self) -> Option<&str> {
        self.attrs.get("positive").and_then(|v| v.as_str())
    }

    /// Sets valid range (CF convention)
    pub fn set_valid_range(&mut self, min: f64, max: f64) {
        self.set("valid_range", serde_json::json!([min, max]));
    }

    /// Gets valid range
    #[must_use]
    pub fn valid_range(&self) -> Option<(f64, f64)> {
        self.attrs.get("valid_range").and_then(|v| {
            v.as_array().and_then(|arr| {
                if arr.len() == 2 {
                    let min = arr[0].as_f64()?;
                    let max = arr[1].as_f64()?;
                    Some((min, max))
                } else {
                    None
                }
            })
        })
    }

    /// Checks if attributes is empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.attrs.is_empty()
    }

    /// Returns number of attributes
    #[must_use]
    pub fn len(&self) -> usize {
        self.attrs.len()
    }

    /// Returns an iterator over attribute keys
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.attrs.keys()
    }

    /// Returns an iterator over attribute key-value pairs
    pub fn iter(&self) -> impl Iterator<Item = (&String, &serde_json::Value)> {
        self.attrs.iter()
    }
}

/// Dimension ordering - C (row-major) or Fortran (column-major) order
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DimensionOrder {
    /// C order (row-major) - last dimension varies fastest
    #[default]
    C,
    /// Fortran order (column-major) - first dimension varies fastest
    Fortran,
}

impl DimensionOrder {
    /// Returns the character representation
    #[must_use]
    pub const fn as_char(&self) -> char {
        match self {
            Self::C => 'C',
            Self::Fortran => 'F',
        }
    }

    /// Creates from a character
    ///
    /// # Errors
    /// Returns error if character is not 'C' or 'F'
    pub fn from_char(c: char) -> Result<Self> {
        match c.to_ascii_uppercase() {
            'C' => Ok(Self::C),
            'F' => Ok(Self::Fortran),
            _ => Err(ZarrError::Metadata(MetadataError::InvalidArrayOrder { order: c })),
        }
    }

    /// Returns the iteration order for dimensions
    #[must_use]
    pub fn iteration_order(&self, ndim: usize) -> Vec<usize> {
        match self {
            Self::C => (0..ndim).collect(),
            Self::Fortran => (0..ndim).rev().collect(),
        }
    }
}

impl core::fmt::Display for DimensionOrder {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.as_char())
    }
}

/// Named index - allows indexing by dimension name
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamedIndex {
    /// Index by position
    Position(usize),
    /// Index by dimension name
    Name(String),
}

impl NamedIndex {
    /// Creates a positional index
    #[must_use]
    pub const fn position(idx: usize) -> Self {
        Self::Position(idx)
    }

    /// Creates a named index
    #[must_use]
    pub fn name(name: impl Into<String>) -> Self {
        Self::Name(name.into())
    }

    /// Resolves to a position using named dimensions
    pub fn resolve(&self, dims: &NamedDimensions) -> Result<usize> {
        match self {
            Self::Position(idx) => {
                if *idx >= dims.ndim() {
                    Err(ZarrError::OutOfBounds {
                        message: format!("Index {idx} out of bounds for {} dimensions", dims.ndim()),
                    })
                } else {
                    Ok(*idx)
                }
            }
            Self::Name(name) => dims.index_of(name),
        }
    }
}

/// Named slice - allows slicing by dimension name
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedSlice {
    /// Dimension identifier
    pub dimension: NamedIndex,
    /// Slice specification
    pub slice: Slice,
}

impl NamedSlice {
    /// Creates a new named slice
    ///
    /// # Errors
    /// Returns error if slice is invalid
    pub fn new(dimension: NamedIndex, start: usize, end: usize, step: usize) -> Result<Self> {
        Ok(Self {
            dimension,
            slice: Slice::new(start, end, step)?,
        })
    }

    /// Creates a named slice from a range
    #[must_use]
    pub fn from_range(dimension: NamedIndex, range: core::ops::Range<usize>) -> Self {
        Self {
            dimension,
            slice: Slice::from_range(range),
        }
    }

    /// Resolves the dimension index
    pub fn resolve_dimension(&self, dims: &NamedDimensions) -> Result<usize> {
        self.dimension.resolve(dims)
    }
}

/// Multi-dimensional named slice specification
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiDimNamedSlice {
    /// Slices for each dimension
    slices: Vec<NamedSlice>,
}

impl MultiDimNamedSlice {
    /// Creates a new multi-dimensional named slice
    ///
    /// # Errors
    /// Returns error if slices is empty
    pub fn new(slices: Vec<NamedSlice>) -> Result<Self> {
        if slices.is_empty() {
            return Err(ZarrError::InvalidDimension {
                message: "MultiDimNamedSlice cannot be empty".to_string(),
            });
        }

        Ok(Self { slices })
    }

    /// Returns the slices
    #[must_use]
    pub fn slices(&self) -> &[NamedSlice] {
        &self.slices
    }

    /// Resolves all slices to positional slices
    ///
    /// Returns a vector of (dimension_index, slice) pairs
    pub fn resolve(&self, dims: &NamedDimensions) -> Result<Vec<(usize, Slice)>> {
        self.slices
            .iter()
            .map(|ns| {
                let idx = ns.resolve_dimension(dims)?;
                Ok((idx, ns.slice.clone()))
            })
            .collect()
    }
}

/// Coordinate space - manages all dimension coordinates
#[derive(Debug, Clone, Default)]
pub struct CoordinateSpace {
    /// Dimension names
    dimensions: NamedDimensions,
    /// Coordinate arrays for each dimension
    coordinates: HashMap<String, DimensionCoordinates>,
    /// Dimension order
    order: DimensionOrder,
}

impl CoordinateSpace {
    /// Creates a new coordinate space
    pub fn new(dimensions: NamedDimensions) -> Self {
        Self {
            dimensions,
            coordinates: HashMap::new(),
            order: DimensionOrder::C,
        }
    }

    /// Creates with dimension order
    pub fn with_order(dimensions: NamedDimensions, order: DimensionOrder) -> Self {
        Self {
            dimensions,
            coordinates: HashMap::new(),
            order,
        }
    }

    /// Sets coordinates for a dimension
    ///
    /// # Errors
    /// Returns error if dimension doesn't exist
    pub fn set_coordinates(&mut self, name: &str, coords: DimensionCoordinates) -> Result<()> {
        if !self.dimensions.contains(name) {
            return Err(ZarrError::InvalidDimension {
                message: format!("Dimension not found: {name}"),
            });
        }

        self.coordinates.insert(name.to_string(), coords);
        Ok(())
    }

    /// Gets coordinates for a dimension
    #[must_use]
    pub fn get_coordinates(&self, name: &str) -> Option<&DimensionCoordinates> {
        self.coordinates.get(name)
    }

    /// Returns the named dimensions
    #[must_use]
    pub fn dimensions(&self) -> &NamedDimensions {
        &self.dimensions
    }

    /// Returns the dimension order
    #[must_use]
    pub fn order(&self) -> DimensionOrder {
        self.order
    }

    /// Checks if coordinates are defined for all dimensions
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.dimensions.names().iter().all(|name| {
            self.coordinates.contains_key(name.as_str())
        })
    }

    /// Returns the shape based on coordinate lengths
    ///
    /// # Errors
    /// Returns error if coordinates are missing
    pub fn shape(&self) -> Result<Shape> {
        let dims: Result<Vec<usize>> = self.dimensions.names().iter()
            .map(|name| {
                self.coordinates.get(name.as_str())
                    .map(|c| c.len())
                    .ok_or_else(|| ZarrError::InvalidDimension {
                        message: format!("Missing coordinates for dimension: {name}"),
                    })
            })
            .collect();

        Shape::new(dims?)
    }

    /// Validates that shape matches coordinate lengths
    ///
    /// # Errors
    /// Returns error if shapes don't match
    pub fn validate_shape(&self, shape: &Shape) -> Result<()> {
        if shape.ndim() != self.dimensions.ndim() {
            return Err(ZarrError::InvalidShape {
                expected: vec![self.dimensions.ndim()],
                actual: vec![shape.ndim()],
            });
        }

        for (idx, name) in self.dimensions.names().iter().enumerate() {
            if let Some(coords) = self.coordinates.get(name.as_str()) {
                if let Some(dim_size) = shape.dim(idx) {
                    if coords.len() != dim_size {
                        return Err(ZarrError::InvalidShape {
                            expected: vec![dim_size],
                            actual: vec![coords.len()],
                        });
                    }
                }
            }
        }

        Ok(())
    }
}

impl Default for NamedDimensions {
    fn default() -> Self {
        Self {
            names: Vec::new(),
            name_to_index: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dimension_name_validation() {
        assert!(DimensionName::new("time").is_ok());
        assert!(DimensionName::new("_private").is_ok());
        assert!(DimensionName::new("dim_0").is_ok());
        assert!(DimensionName::new("").is_err());
        assert!(DimensionName::new("0dim").is_err());
        assert!(DimensionName::new("dim-name").is_err());
    }

    #[test]
    fn test_named_dimensions() {
        let dims = NamedDimensions::from_names(vec!["time", "lat", "lon"])
            .expect("valid dimensions");

        assert_eq!(dims.ndim(), 3);
        assert_eq!(dims.index_of("time").expect("found"), 0);
        assert_eq!(dims.index_of("lat").expect("found"), 1);
        assert_eq!(dims.index_of("lon").expect("found"), 2);
        assert!(dims.index_of("unknown").is_err());

        assert!(dims.contains("time"));
        assert!(!dims.contains("unknown"));
    }

    #[test]
    fn test_named_dimensions_duplicate() {
        let result = NamedDimensions::from_names(vec!["time", "lat", "time"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_named_dimensions_permute() {
        let dims = NamedDimensions::from_names(vec!["time", "lat", "lon"])
            .expect("valid dimensions");

        let permuted = dims.permute(&[2, 0, 1]).expect("valid permutation");

        assert_eq!(permuted.index_of("lon").expect("found"), 0);
        assert_eq!(permuted.index_of("time").expect("found"), 1);
        assert_eq!(permuted.index_of("lat").expect("found"), 2);
    }

    #[test]
    fn test_named_dimensions_transpose() {
        let dims = NamedDimensions::from_names(vec!["time", "lat", "lon"])
            .expect("valid dimensions");

        let transposed = dims.transpose().expect("valid transpose");

        assert_eq!(transposed.index_of("lon").expect("found"), 0);
        assert_eq!(transposed.index_of("lat").expect("found"), 1);
        assert_eq!(transposed.index_of("time").expect("found"), 2);
    }

    #[test]
    fn test_coordinate_array_f64() {
        let coords = CoordinateArray::Float64(vec![0.0, 1.0, 2.0, 3.0]);

        assert_eq!(coords.len(), 4);
        assert!(!coords.is_empty());
        assert_eq!(coords.coord_type(), CoordinateType::Float64);

        assert_eq!(coords.value_f64_at(0).expect("valid"), 0.0);
        assert_eq!(coords.value_f64_at(2).expect("valid"), 2.0);
        assert!(coords.value_f64_at(10).is_err());
    }

    #[test]
    fn test_coordinate_array_find() {
        let coords = CoordinateArray::Float64(vec![0.0, 1.0, 2.0, 3.0]);

        assert_eq!(coords.find_index_f64(2.0), Some(2));
        assert_eq!(coords.find_index_f64(5.0), None);

        assert_eq!(coords.find_nearest_index_f64(1.3), Some(1));
        assert_eq!(coords.find_nearest_index_f64(1.7), Some(2));
    }

    #[test]
    fn test_coordinate_array_slice() {
        let coords = CoordinateArray::Float64(vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0]);

        let sliced = coords.slice(1, 5, 2).expect("valid slice");

        if let CoordinateArray::Float64(v) = sliced {
            assert_eq!(v, vec![1.0, 3.0]);
        } else {
            panic!("Expected Float64");
        }
    }

    #[test]
    fn test_dimension_coordinates() {
        let coords = DimensionCoordinates::new(
            "time",
            CoordinateArray::Float64(vec![0.0, 1.0, 2.0]),
        ).expect("valid coords");

        assert_eq!(coords.len(), 3);
        assert_eq!(coords.value_at(1).expect("valid"), 1.0);
    }

    #[test]
    fn test_dimension_attributes_cf_conventions() {
        let mut attrs = DimensionAttributes::new();

        attrs.set_units("days since 2000-01-01");
        attrs.set_long_name("Time");
        attrs.set_standard_name("time");
        attrs.set_calendar("gregorian");
        attrs.set_axis("T");
        attrs.set_valid_range(0.0, 365.0);

        assert_eq!(attrs.units(), Some("days since 2000-01-01"));
        assert_eq!(attrs.long_name(), Some("Time"));
        assert_eq!(attrs.standard_name(), Some("time"));
        assert_eq!(attrs.calendar(), Some("gregorian"));
        assert_eq!(attrs.axis(), Some("T"));
        assert_eq!(attrs.valid_range(), Some((0.0, 365.0)));
    }

    #[test]
    fn test_dimension_order() {
        assert_eq!(DimensionOrder::C.as_char(), 'C');
        assert_eq!(DimensionOrder::Fortran.as_char(), 'F');

        assert_eq!(DimensionOrder::from_char('C').expect("valid"), DimensionOrder::C);
        assert_eq!(DimensionOrder::from_char('f').expect("valid"), DimensionOrder::Fortran);
        assert!(DimensionOrder::from_char('X').is_err());

        assert_eq!(DimensionOrder::C.iteration_order(3), vec![0, 1, 2]);
        assert_eq!(DimensionOrder::Fortran.iteration_order(3), vec![2, 1, 0]);
    }

    #[test]
    fn test_named_index() {
        let dims = NamedDimensions::from_names(vec!["time", "lat", "lon"])
            .expect("valid dimensions");

        let pos_idx = NamedIndex::position(1);
        let name_idx = NamedIndex::name("lat");

        assert_eq!(pos_idx.resolve(&dims).expect("valid"), 1);
        assert_eq!(name_idx.resolve(&dims).expect("valid"), 1);

        let invalid_pos = NamedIndex::position(10);
        let invalid_name = NamedIndex::name("unknown");

        assert!(invalid_pos.resolve(&dims).is_err());
        assert!(invalid_name.resolve(&dims).is_err());
    }

    #[test]
    fn test_named_slice() {
        let dims = NamedDimensions::from_names(vec!["time", "lat", "lon"])
            .expect("valid dimensions");

        let slice = NamedSlice::new(NamedIndex::name("time"), 0, 10, 1)
            .expect("valid slice");

        assert_eq!(slice.resolve_dimension(&dims).expect("valid"), 0);
        assert_eq!(slice.slice.len(), 10);
    }

    #[test]
    fn test_coordinate_space() {
        let dims = NamedDimensions::from_names(vec!["time", "lat"])
            .expect("valid dimensions");

        let mut space = CoordinateSpace::new(dims);

        let time_coords = DimensionCoordinates::new(
            "time",
            CoordinateArray::Float64(vec![0.0, 1.0, 2.0]),
        ).expect("valid coords");

        let lat_coords = DimensionCoordinates::new(
            "lat",
            CoordinateArray::Float64(vec![-90.0, 0.0, 90.0]),
        ).expect("valid coords");

        space.set_coordinates("time", time_coords).expect("set ok");
        space.set_coordinates("lat", lat_coords).expect("set ok");

        assert!(space.is_complete());

        let shape = space.shape().expect("valid shape");
        assert_eq!(shape.as_slice(), &[3, 3]);
    }

    #[test]
    fn test_anonymous_dimensions() {
        let dims = NamedDimensions::anonymous(3);

        assert_eq!(dims.ndim(), 3);
        assert_eq!(dims.index_of("dim_0").expect("found"), 0);
        assert_eq!(dims.index_of("dim_1").expect("found"), 1);
        assert_eq!(dims.index_of("dim_2").expect("found"), 2);
    }

    #[test]
    fn test_rename_dimension() {
        let mut dims = NamedDimensions::from_names(vec!["dim_0", "dim_1"])
            .expect("valid dimensions");

        dims.rename("dim_0", "time").expect("rename ok");

        assert_eq!(dims.index_of("time").expect("found"), 0);
        assert!(dims.index_of("dim_0").is_err());
    }
}
