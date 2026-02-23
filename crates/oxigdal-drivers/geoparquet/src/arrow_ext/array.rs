//! Arrow array builders and accessors for geometry data

use crate::error::{GeoParquetError, Result};
use crate::geometry::{Geometry, WkbWriter};
use arrow_array::{
    Array, BinaryArray,
    builder::{ArrayBuilder, BinaryBuilder},
};
use std::sync::Arc;

/// A wrapper around Arrow BinaryArray for geometry data
pub struct GeometryArray {
    array: BinaryArray,
}

impl GeometryArray {
    /// Creates a new geometry array from a BinaryArray
    pub fn new(array: BinaryArray) -> Self {
        Self { array }
    }

    /// Returns the underlying Arrow array
    pub fn array(&self) -> &BinaryArray {
        &self.array
    }

    /// Returns the number of geometries
    pub fn len(&self) -> usize {
        self.array.len()
    }

    /// Returns true if the array is empty
    pub fn is_empty(&self) -> bool {
        self.array.is_empty()
    }

    /// Gets a geometry at the specified index as WKB bytes
    pub fn value(&self, index: usize) -> Result<&[u8]> {
        if index >= self.len() {
            return Err(GeoParquetError::out_of_bounds(index, self.len()));
        }
        if self.array.is_null(index) {
            return Err(GeoParquetError::internal("Geometry at index is null"));
        }
        Ok(self.array.value(index))
    }

    /// Returns true if the geometry at the specified index is null
    pub fn is_null(&self, index: usize) -> bool {
        self.array.is_null(index)
    }

    /// Returns the number of null geometries
    pub fn null_count(&self) -> usize {
        self.array.null_count()
    }

    /// Converts this array into an `Arc<dyn Array>`
    pub fn into_arc(self) -> Arc<dyn Array> {
        Arc::new(self.array)
    }
}

/// Builder for geometry arrays
pub struct GeometryArrayBuilder {
    builder: BinaryBuilder,
    wkb_writer: WkbWriter,
}

impl GeometryArrayBuilder {
    /// Creates a new geometry array builder
    pub fn new() -> Self {
        Self {
            builder: BinaryBuilder::new(),
            wkb_writer: WkbWriter::new(true), // Use little endian by default
        }
    }

    /// Creates a new geometry array builder with capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            builder: BinaryBuilder::with_capacity(capacity, 0),
            wkb_writer: WkbWriter::new(true),
        }
    }

    /// Appends a geometry to the builder
    pub fn append_geometry(&mut self, geom: &Geometry) -> Result<()> {
        let wkb = self.wkb_writer.write_geometry(geom)?;
        self.builder.append_value(&wkb);
        Ok(())
    }

    /// Appends a null geometry
    pub fn append_null(&mut self) {
        self.builder.append_null();
    }

    /// Appends WKB bytes directly
    pub fn append_wkb(&mut self, wkb: &[u8]) {
        self.builder.append_value(wkb);
    }

    /// Appends an option geometry
    pub fn append_option(&mut self, geom: Option<&Geometry>) -> Result<()> {
        match geom {
            Some(g) => self.append_geometry(g),
            None => {
                self.append_null();
                Ok(())
            }
        }
    }

    /// Returns the current length of the builder
    pub fn len(&self) -> usize {
        self.builder.len()
    }

    /// Returns true if the builder is empty
    pub fn is_empty(&self) -> bool {
        self.builder.is_empty()
    }

    /// Builds the geometry array
    pub fn finish(mut self) -> GeometryArray {
        let array = self.builder.finish();
        GeometryArray::new(array)
    }

    /// Builds the geometry array and returns as `Arc<dyn Array>`
    pub fn finish_arc(self) -> Arc<dyn Array> {
        self.finish().into_arc()
    }
}

impl Default for GeometryArrayBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Creates a geometry array from a vector of geometries
pub fn from_geometries(geometries: &[Geometry]) -> Result<GeometryArray> {
    let mut builder = GeometryArrayBuilder::with_capacity(geometries.len());
    for geom in geometries {
        builder.append_geometry(geom)?;
    }
    Ok(builder.finish())
}

/// Creates a geometry array from a vector of optional geometries
pub fn from_optional_geometries(geometries: &[Option<Geometry>]) -> Result<GeometryArray> {
    let mut builder = GeometryArrayBuilder::with_capacity(geometries.len());
    for geom in geometries {
        builder.append_option(geom.as_ref())?;
    }
    Ok(builder.finish())
}

/// Creates a geometry array from WKB bytes
pub fn from_wkb_vec(wkb_data: Vec<Option<Vec<u8>>>) -> GeometryArray {
    let mut builder = GeometryArrayBuilder::with_capacity(wkb_data.len());
    for wkb in wkb_data {
        match wkb {
            Some(bytes) => builder.append_wkb(&bytes),
            None => builder.append_null(),
        }
    }
    builder.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Point;

    #[test]
    fn test_geometry_array_builder() -> Result<()> {
        let mut builder = GeometryArrayBuilder::new();

        let point1 = Geometry::Point(Point::new_2d(1.0, 2.0));
        let point2 = Geometry::Point(Point::new_2d(3.0, 4.0));

        builder.append_geometry(&point1)?;
        builder.append_geometry(&point2)?;
        builder.append_null();

        let array = builder.finish();
        assert_eq!(array.len(), 3);
        assert_eq!(array.null_count(), 1);
        assert!(!array.is_null(0));
        assert!(!array.is_null(1));
        assert!(array.is_null(2));

        Ok(())
    }

    #[test]
    fn test_geometry_array_access() -> Result<()> {
        let geometries = vec![
            Geometry::Point(Point::new_2d(1.0, 2.0)),
            Geometry::Point(Point::new_2d(3.0, 4.0)),
        ];

        let array = from_geometries(&geometries)?;
        assert_eq!(array.len(), 2);

        let wkb0 = array.value(0)?;
        assert!(!wkb0.is_empty());

        Ok(())
    }

    #[test]
    fn test_from_optional_geometries() -> Result<()> {
        let geometries = vec![
            Some(Geometry::Point(Point::new_2d(1.0, 2.0))),
            None,
            Some(Geometry::Point(Point::new_2d(3.0, 4.0))),
        ];

        let array = from_optional_geometries(&geometries)?;
        assert_eq!(array.len(), 3);
        assert_eq!(array.null_count(), 1);
        assert!(array.is_null(1));

        Ok(())
    }

    #[test]
    fn test_out_of_bounds_access() {
        let array = GeometryArrayBuilder::new().finish();
        let result = array.value(0);
        assert!(result.is_err());
    }
}
