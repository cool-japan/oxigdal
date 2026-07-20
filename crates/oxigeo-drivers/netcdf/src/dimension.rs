//! NetCDF dimension types and utilities.
//!
//! Dimensions define the shape of variables in NetCDF files. This module provides
//! types for managing dimensions, including unlimited dimensions which can grow
//! over time.

use serde::{Deserialize, Serialize};

use crate::error::{NetCdfError, Result};

/// Represents a dimension in a NetCDF file.
///
/// Dimensions have a name and size. The size can be fixed or unlimited.
/// Unlimited dimensions are typically used for the time dimension.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dimension {
    /// Name of the dimension
    name: String,
    /// Size of the dimension
    size: DimensionSize,
}

impl Dimension {
    /// Create a new fixed-size dimension.
    ///
    /// # Arguments
    ///
    /// * `name` - Name of the dimension
    /// * `size` - Size of the dimension
    ///
    /// # Errors
    ///
    /// Returns error if the name is empty.
    pub fn new(name: impl Into<String>, size: usize) -> Result<Self> {
        let name = name.into();
        if name.is_empty() {
            return Err(NetCdfError::DimensionError(
                "Dimension name cannot be empty".to_string(),
            ));
        }
        Ok(Self {
            name,
            size: DimensionSize::Fixed(size),
        })
    }

    /// Create a new unlimited dimension.
    ///
    /// Unlimited dimensions can grow over time. Typically used for the time dimension.
    ///
    /// # Arguments
    ///
    /// * `name` - Name of the dimension
    /// * `current_size` - Current size of the dimension
    ///
    /// # Errors
    ///
    /// Returns error if the name is empty.
    pub fn new_unlimited(name: impl Into<String>, current_size: usize) -> Result<Self> {
        let name = name.into();
        if name.is_empty() {
            return Err(NetCdfError::DimensionError(
                "Dimension name cannot be empty".to_string(),
            ));
        }
        Ok(Self {
            name,
            size: DimensionSize::Unlimited(current_size),
        })
    }

    /// Get the dimension name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the current size of the dimension.
    #[must_use]
    pub fn len(&self) -> usize {
        match self.size {
            DimensionSize::Fixed(size) | DimensionSize::Unlimited(size) => size,
        }
    }

    /// Check if the dimension is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Check if the dimension is unlimited.
    #[must_use]
    pub fn is_unlimited(&self) -> bool {
        matches!(self.size, DimensionSize::Unlimited(_))
    }

    /// Get the dimension size type.
    #[must_use]
    pub const fn size(&self) -> &DimensionSize {
        &self.size
    }

    /// Set the dimension size (for unlimited dimensions).
    ///
    /// # Errors
    ///
    /// Returns error if trying to change the size of a fixed dimension.
    pub fn set_len(&mut self, new_size: usize) -> Result<()> {
        match &mut self.size {
            DimensionSize::Unlimited(size) => {
                *size = new_size;
                Ok(())
            }
            DimensionSize::Fixed(_) => Err(NetCdfError::UnlimitedDimensionError(format!(
                "Cannot change size of fixed dimension '{}'",
                self.name
            ))),
        }
    }
}

/// Dimension size can be fixed or unlimited.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DimensionSize {
    /// Fixed size dimension
    Fixed(usize),
    /// Unlimited dimension with current size
    Unlimited(usize),
}

impl DimensionSize {
    /// Get the current size.
    #[must_use]
    pub const fn len(&self) -> usize {
        match self {
            Self::Fixed(size) | Self::Unlimited(size) => *size,
        }
    }

    /// Check if the size is zero.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Check if unlimited.
    #[must_use]
    pub const fn is_unlimited(&self) -> bool {
        matches!(self, Self::Unlimited(_))
    }
}

/// Collection of dimensions for a NetCDF file or variable.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Dimensions {
    dimensions: Vec<Dimension>,
}

impl Dimensions {
    /// Create a new empty dimension collection.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            dimensions: Vec::new(),
        }
    }

    /// Create from a vector of dimensions.
    #[must_use]
    pub const fn from_vec(dimensions: Vec<Dimension>) -> Self {
        Self { dimensions }
    }

    /// Add a dimension.
    ///
    /// # Errors
    ///
    /// Returns error if a dimension with the same name already exists.
    pub fn add(&mut self, dimension: Dimension) -> Result<()> {
        if self.contains(dimension.name()) {
            return Err(NetCdfError::DimensionError(format!(
                "Dimension '{}' already exists",
                dimension.name()
            )));
        }
        self.dimensions.push(dimension);
        Ok(())
    }

    /// Get a dimension by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Dimension> {
        self.dimensions.iter().find(|d| d.name() == name)
    }

    /// Get a mutable reference to a dimension by name.
    pub fn get_mut(&mut self, name: &str) -> Option<&mut Dimension> {
        self.dimensions.iter_mut().find(|d| d.name() == name)
    }

    /// Get a dimension by index.
    #[must_use]
    pub fn get_by_index(&self, index: usize) -> Option<&Dimension> {
        self.dimensions.get(index)
    }

    /// Check if a dimension exists.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.dimensions.iter().any(|d| d.name() == name)
    }

    /// Get the number of dimensions.
    #[must_use]
    pub fn len(&self) -> usize {
        self.dimensions.len()
    }

    /// Check if empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.dimensions.is_empty()
    }

    /// Get an iterator over dimensions.
    pub fn iter(&self) -> impl Iterator<Item = &Dimension> {
        self.dimensions.iter()
    }

    /// Get the unlimited dimension if present.
    #[must_use]
    pub fn unlimited(&self) -> Option<&Dimension> {
        self.dimensions.iter().find(|d| d.is_unlimited())
    }

    /// Get names of all dimensions.
    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        self.dimensions.iter().map(|d| d.name()).collect()
    }

    /// Get total size (product of all dimension sizes).
    ///
    /// Returns `None` if any dimension is empty or if the product would overflow.
    #[must_use]
    pub fn total_size(&self) -> Option<usize> {
        if self.dimensions.is_empty() {
            return Some(0);
        }

        self.dimensions
            .iter()
            .map(|d| d.len())
            .try_fold(1usize, |acc, size| acc.checked_mul(size))
    }

    /// Get the shape as a vector of sizes.
    #[must_use]
    pub fn shape(&self) -> Vec<usize> {
        self.dimensions.iter().map(|d| d.len()).collect()
    }
}

impl IntoIterator for Dimensions {
    type Item = Dimension;
    type IntoIter = std::vec::IntoIter<Dimension>;

    fn into_iter(self) -> Self::IntoIter {
        self.dimensions.into_iter()
    }
}

impl<'a> IntoIterator for &'a Dimensions {
    type Item = &'a Dimension;
    type IntoIter = std::slice::Iter<'a, Dimension>;

    fn into_iter(self) -> Self::IntoIter {
        self.dimensions.iter()
    }
}

impl FromIterator<Dimension> for Dimensions {
    fn from_iter<T: IntoIterator<Item = Dimension>>(iter: T) -> Self {
        Self {
            dimensions: iter.into_iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fixed_dimension() {
        let dim = Dimension::new("x", 100).expect("Failed to create fixed dimension");
        assert_eq!(dim.name(), "x");
        assert_eq!(dim.len(), 100);
        assert!(!dim.is_unlimited());
        assert!(!dim.is_empty());
    }

    #[test]
    fn test_unlimited_dimension() {
        let mut dim =
            Dimension::new_unlimited("time", 10).expect("Failed to create unlimited dimension");
        assert_eq!(dim.name(), "time");
        assert_eq!(dim.len(), 10);
        assert!(dim.is_unlimited());

        dim.set_len(20).expect("Failed to set dimension length");
        assert_eq!(dim.len(), 20);
    }

    #[test]
    fn test_fixed_dimension_cannot_change_size() {
        let mut dim = Dimension::new("x", 100).expect("Failed to create fixed dimension");
        let result = dim.set_len(200);
        assert!(result.is_err());
    }

    #[test]
    fn test_dimension_collection() {
        let mut dims = Dimensions::new();
        dims.add(Dimension::new("x", 100).expect("Failed to create dimension x"))
            .expect("Failed to add dimension x to collection");
        dims.add(Dimension::new("y", 200).expect("Failed to create dimension y"))
            .expect("Failed to add dimension y to collection");
        dims.add(
            Dimension::new_unlimited("time", 10)
                .expect("Failed to create unlimited dimension time"),
        )
        .expect("Failed to add unlimited dimension time to collection");

        assert_eq!(dims.len(), 3);
        assert!(dims.contains("x"));
        assert!(dims.contains("y"));
        assert!(dims.contains("time"));

        assert_eq!(dims.get("x").expect("Failed to get dimension x").len(), 100);
        assert_eq!(dims.get("y").expect("Failed to get dimension y").len(), 200);

        let unlimited = dims.unlimited().expect("Failed to get unlimited dimension");
        assert_eq!(unlimited.name(), "time");
        assert_eq!(unlimited.len(), 10);
    }

    #[test]
    fn test_dimension_total_size() {
        let mut dims = Dimensions::new();
        dims.add(Dimension::new("x", 10).expect("Failed to create dimension x"))
            .expect("Failed to add dimension x");
        dims.add(Dimension::new("y", 20).expect("Failed to create dimension y"))
            .expect("Failed to add dimension y");
        dims.add(Dimension::new("z", 30).expect("Failed to create dimension z"))
            .expect("Failed to add dimension z");

        assert_eq!(dims.total_size(), Some(10 * 20 * 30));
    }

    #[test]
    fn test_dimension_shape() {
        let mut dims = Dimensions::new();
        dims.add(Dimension::new("x", 10).expect("Failed to create dimension x"))
            .expect("Failed to add dimension x");
        dims.add(Dimension::new("y", 20).expect("Failed to create dimension y"))
            .expect("Failed to add dimension y");

        assert_eq!(dims.shape(), vec![10, 20]);
    }

    #[test]
    fn test_empty_dimension_name() {
        let result = Dimension::new("", 100);
        assert!(result.is_err());
    }

    #[test]
    fn test_duplicate_dimension() {
        let mut dims = Dimensions::new();
        dims.add(Dimension::new("x", 100).expect("Failed to create dimension x"))
            .expect("Failed to add dimension x");
        let result =
            dims.add(Dimension::new("x", 200).expect("Failed to create duplicate dimension x"));
        assert!(result.is_err());
    }
}
