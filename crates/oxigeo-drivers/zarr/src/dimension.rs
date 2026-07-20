//! Dimension and shape utilities for Zarr arrays
//!
//! This module provides types and functions for working with array dimensions,
//! shapes, and coordinates in Zarr arrays.

use crate::error::{MetadataError, Result, ZarrError};
use serde::{Deserialize, Serialize};

/// Array shape - sequence of dimension sizes
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Shape(Vec<usize>);

impl Shape {
    /// Creates a new shape
    ///
    /// # Errors
    /// Returns error if any dimension is zero
    pub fn new(dims: Vec<usize>) -> Result<Self> {
        if dims.is_empty() {
            return Err(ZarrError::InvalidDimension {
                message: "Shape cannot be empty".to_string(),
            });
        }

        if dims.contains(&0) {
            return Err(ZarrError::InvalidDimension {
                message: "Shape dimensions cannot be zero".to_string(),
            });
        }

        Ok(Self(dims))
    }

    /// Creates a new shape without validation (for internal use)
    #[must_use]
    pub fn new_unchecked(dims: Vec<usize>) -> Self {
        Self(dims)
    }

    /// Returns the number of dimensions
    #[must_use]
    pub fn ndim(&self) -> usize {
        self.0.len()
    }

    /// Returns the dimensions as a slice
    #[must_use]
    pub fn as_slice(&self) -> &[usize] {
        &self.0
    }

    /// Returns the total number of elements
    #[must_use]
    pub fn size(&self) -> usize {
        self.0.iter().product()
    }

    /// Returns the size at a specific dimension
    #[must_use]
    pub fn dim(&self, index: usize) -> Option<usize> {
        self.0.get(index).copied()
    }

    /// Checks if this shape is compatible with another for broadcasting
    #[must_use]
    pub fn is_broadcastable_to(&self, other: &Self) -> bool {
        if self.ndim() > other.ndim() {
            return false;
        }

        self.0
            .iter()
            .rev()
            .zip(other.0.iter().rev())
            .all(|(a, b)| a == b || *a == 1)
    }

    /// Checks if a coordinate is within bounds
    #[must_use]
    pub fn contains_coord(&self, coord: &[usize]) -> bool {
        if coord.len() != self.ndim() {
            return false;
        }

        coord.iter().zip(self.0.iter()).all(|(c, s)| c < s)
    }

    /// Converts a multi-dimensional index to a flat index (C order)
    #[must_use]
    pub fn ravel_index(&self, indices: &[usize]) -> Option<usize> {
        if indices.len() != self.ndim() {
            return None;
        }

        if !self.contains_coord(indices) {
            return None;
        }

        let mut flat_index = 0;
        let mut multiplier = 1;

        for i in (0..self.ndim()).rev() {
            flat_index += indices[i] * multiplier;
            multiplier *= self.0[i];
        }

        Some(flat_index)
    }

    /// Converts a flat index to multi-dimensional indices (C order)
    #[must_use]
    pub fn unravel_index(&self, flat_index: usize) -> Option<Vec<usize>> {
        if flat_index >= self.size() {
            return None;
        }

        let mut indices = vec![0; self.ndim()];
        let mut remaining = flat_index;

        for i in (0..self.ndim()).rev() {
            indices[i] = remaining % self.0[i];
            remaining /= self.0[i];
        }

        Some(indices)
    }

    /// Returns a vector representation
    #[must_use]
    pub fn to_vec(&self) -> Vec<usize> {
        self.0.clone()
    }
}

impl From<Vec<usize>> for Shape {
    fn from(dims: Vec<usize>) -> Self {
        Self::new_unchecked(dims)
    }
}

impl AsRef<[usize]> for Shape {
    fn as_ref(&self) -> &[usize] {
        &self.0
    }
}

impl core::ops::Index<usize> for Shape {
    type Output = usize;

    fn index(&self, index: usize) -> &Self::Output {
        &self.0[index]
    }
}

/// Dimension separator character for Zarr paths
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DimensionSeparator {
    /// Dot separator (default for v2)
    #[default]
    Dot,
    /// Slash separator (default for v3)
    Slash,
}

impl DimensionSeparator {
    /// Returns the character representation
    #[must_use]
    pub const fn as_char(&self) -> char {
        match self {
            Self::Dot => '.',
            Self::Slash => '/',
        }
    }

    /// Creates from a character
    ///
    /// # Errors
    /// Returns error if character is not '.' or '/'
    pub fn from_char(c: char) -> Result<Self> {
        match c {
            '.' => Ok(Self::Dot),
            '/' => Ok(Self::Slash),
            _ => Err(ZarrError::Metadata(
                MetadataError::InvalidDimensionSeparator { separator: c },
            )),
        }
    }

    /// Returns the default separator for a Zarr version
    #[must_use]
    pub const fn default_for_version(version: u8) -> Self {
        match version {
            2 => Self::Dot,
            3 => Self::Slash,
            _ => Self::Slash,
        }
    }
}

impl core::fmt::Display for DimensionSeparator {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.as_char())
    }
}

/// Dimension metadata
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Dimension {
    /// Dimension name
    pub name: Option<String>,
    /// Dimension size
    pub size: usize,
    /// Dimension type (for Zarr v3)
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub dim_type: Option<String>,
    /// Unit (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

impl Dimension {
    /// Creates a new dimension
    #[must_use]
    pub fn new(size: usize) -> Self {
        Self {
            name: None,
            size,
            dim_type: None,
            unit: None,
        }
    }

    /// Creates a named dimension
    #[must_use]
    pub fn named(name: String, size: usize) -> Self {
        Self {
            name: Some(name),
            size,
            dim_type: None,
            unit: None,
        }
    }

    /// Sets the dimension type
    #[must_use]
    pub fn with_type(mut self, dim_type: String) -> Self {
        self.dim_type = Some(dim_type);
        self
    }

    /// Sets the unit
    #[must_use]
    pub fn with_unit(mut self, unit: String) -> Self {
        self.unit = Some(unit);
        self
    }
}

/// Slice specification for array indexing
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slice {
    /// Start index (inclusive)
    pub start: usize,
    /// End index (exclusive)
    pub end: usize,
    /// Step size
    pub step: usize,
}

impl Slice {
    /// Creates a new slice
    ///
    /// # Errors
    /// Returns error if step is zero or end < start
    pub fn new(start: usize, end: usize, step: usize) -> Result<Self> {
        if step == 0 {
            return Err(ZarrError::InvalidDimension {
                message: "Slice step cannot be zero".to_string(),
            });
        }

        if end < start {
            return Err(ZarrError::InvalidDimension {
                message: format!("Invalid slice: end ({end}) < start ({start})"),
            });
        }

        Ok(Self { start, end, step })
    }

    /// Creates a slice from a range
    #[must_use]
    pub fn from_range(range: core::ops::Range<usize>) -> Self {
        Self {
            start: range.start,
            end: range.end,
            step: 1,
        }
    }

    /// Returns the length of this slice
    #[must_use]
    pub fn len(&self) -> usize {
        if self.end <= self.start {
            0
        } else {
            (self.end - self.start).div_ceil(self.step)
        }
    }

    /// Checks if the slice is empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Checks if this slice is within bounds of a dimension
    #[must_use]
    pub fn is_valid_for_dimension(&self, dim_size: usize) -> bool {
        self.start < dim_size && self.end <= dim_size
    }

    /// Returns an iterator over the indices in this slice
    pub fn iter(&self) -> SliceIter {
        SliceIter {
            current: self.start,
            end: self.end,
            step: self.step,
        }
    }
}

/// Iterator over slice indices
pub struct SliceIter {
    current: usize,
    end: usize,
    step: usize,
}

impl Iterator for SliceIter {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current < self.end {
            let result = self.current;
            self.current += self.step;
            Some(result)
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        if self.current >= self.end {
            (0, Some(0))
        } else {
            let remaining = (self.end - self.current).div_ceil(self.step);
            (remaining, Some(remaining))
        }
    }
}

impl ExactSizeIterator for SliceIter {}

/// Multi-dimensional slice specification
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiDimSlice {
    slices: Vec<Slice>,
}

impl MultiDimSlice {
    /// Creates a new multi-dimensional slice
    ///
    /// # Errors
    /// Returns error if slices is empty
    pub fn new(slices: Vec<Slice>) -> Result<Self> {
        if slices.is_empty() {
            return Err(ZarrError::InvalidDimension {
                message: "MultiDimSlice cannot be empty".to_string(),
            });
        }

        Ok(Self { slices })
    }

    /// Returns the number of dimensions
    #[must_use]
    pub fn ndim(&self) -> usize {
        self.slices.len()
    }

    /// Returns the shape of the sliced array
    #[must_use]
    pub fn shape(&self) -> Shape {
        Shape::new_unchecked(self.slices.iter().map(Slice::len).collect())
    }

    /// Checks if this slice is valid for a given shape
    #[must_use]
    pub fn is_valid_for_shape(&self, shape: &Shape) -> bool {
        if self.ndim() != shape.ndim() {
            return false;
        }

        self.slices
            .iter()
            .zip(shape.as_slice())
            .all(|(slice, &dim_size)| slice.is_valid_for_dimension(dim_size))
    }

    /// Returns the slices
    #[must_use]
    pub fn slices(&self) -> &[Slice] {
        &self.slices
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shape_creation() {
        let shape = Shape::new(vec![10, 20, 30]).expect("valid shape");
        assert_eq!(shape.ndim(), 3);
        assert_eq!(shape.size(), 6000);
        assert_eq!(shape.dim(0), Some(10));
        assert_eq!(shape.dim(1), Some(20));
        assert_eq!(shape.dim(2), Some(30));
    }

    #[test]
    fn test_shape_invalid() {
        assert!(Shape::new(vec![]).is_err());
        assert!(Shape::new(vec![0, 10]).is_err());
    }

    #[test]
    fn test_shape_ravel_unravel() {
        let shape = Shape::new(vec![3, 4, 5]).expect("valid shape");

        // Test ravel
        assert_eq!(shape.ravel_index(&[0, 0, 0]), Some(0));
        assert_eq!(shape.ravel_index(&[0, 0, 1]), Some(1));
        assert_eq!(shape.ravel_index(&[0, 1, 0]), Some(5));
        assert_eq!(shape.ravel_index(&[1, 0, 0]), Some(20));
        assert_eq!(shape.ravel_index(&[2, 3, 4]), Some(59));

        // Test unravel
        assert_eq!(shape.unravel_index(0), Some(vec![0, 0, 0]));
        assert_eq!(shape.unravel_index(1), Some(vec![0, 0, 1]));
        assert_eq!(shape.unravel_index(5), Some(vec![0, 1, 0]));
        assert_eq!(shape.unravel_index(20), Some(vec![1, 0, 0]));
        assert_eq!(shape.unravel_index(59), Some(vec![2, 3, 4]));

        // Out of bounds
        assert_eq!(shape.ravel_index(&[3, 0, 0]), None);
        assert_eq!(shape.unravel_index(60), None);
    }

    #[test]
    fn test_shape_contains() {
        let shape = Shape::new(vec![10, 20]).expect("valid shape");
        assert!(shape.contains_coord(&[0, 0]));
        assert!(shape.contains_coord(&[9, 19]));
        assert!(!shape.contains_coord(&[10, 0]));
        assert!(!shape.contains_coord(&[0, 20]));
        assert!(!shape.contains_coord(&[0]));
    }

    #[test]
    fn test_dimension_separator() {
        assert_eq!(DimensionSeparator::Dot.as_char(), '.');
        assert_eq!(DimensionSeparator::Slash.as_char(), '/');

        assert_eq!(
            DimensionSeparator::from_char('.').expect("valid separator"),
            DimensionSeparator::Dot
        );
        assert_eq!(
            DimensionSeparator::from_char('/').expect("valid separator"),
            DimensionSeparator::Slash
        );
        assert!(DimensionSeparator::from_char('-').is_err());

        assert_eq!(
            DimensionSeparator::default_for_version(2),
            DimensionSeparator::Dot
        );
        assert_eq!(
            DimensionSeparator::default_for_version(3),
            DimensionSeparator::Slash
        );
    }

    #[test]
    fn test_slice() {
        let slice = Slice::new(0, 10, 2).expect("valid slice");
        assert_eq!(slice.len(), 5);
        assert!(!slice.is_empty());
        assert!(slice.is_valid_for_dimension(10));
        assert!(!slice.is_valid_for_dimension(5));

        let indices: Vec<_> = slice.iter().collect();
        assert_eq!(indices, vec![0, 2, 4, 6, 8]);
    }

    #[test]
    fn test_slice_from_range() {
        let slice = Slice::from_range(5..10);
        assert_eq!(slice.start, 5);
        assert_eq!(slice.end, 10);
        assert_eq!(slice.step, 1);
        assert_eq!(slice.len(), 5);
    }

    #[test]
    fn test_multi_dim_slice() {
        let slices = vec![
            Slice::new(0, 10, 1).expect("valid slice"),
            Slice::new(0, 20, 2).expect("valid slice"),
            Slice::new(5, 15, 1).expect("valid slice"),
        ];

        let multi_slice = MultiDimSlice::new(slices).expect("valid multi-slice");
        assert_eq!(multi_slice.ndim(), 3);

        let shape = multi_slice.shape();
        assert_eq!(shape.as_slice(), &[10, 10, 10]);

        let array_shape = Shape::new(vec![10, 20, 30]).expect("valid shape");
        assert!(multi_slice.is_valid_for_shape(&array_shape));
    }

    #[test]
    fn test_dimension() {
        let dim = Dimension::new(100);
        assert_eq!(dim.size, 100);
        assert!(dim.name.is_none());

        let named_dim = Dimension::named("time".to_string(), 365)
            .with_type("temporal".to_string())
            .with_unit("days".to_string());

        assert_eq!(named_dim.name, Some("time".to_string()));
        assert_eq!(named_dim.size, 365);
        assert_eq!(named_dim.dim_type, Some("temporal".to_string()));
        assert_eq!(named_dim.unit, Some("days".to_string()));
    }
}
