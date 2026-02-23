//! Bounding box types for spatial extent representation
//!
//! This module provides various bounding box types for representing spatial extents
//! in both geographic (lat/lon) and projected coordinate systems.

use core::fmt;

use serde::{Deserialize, Serialize};

use crate::error::{OxiGdalError, Result};

/// A 2D bounding box in any coordinate system
///
/// The bounding box is defined by its minimum and maximum coordinates.
/// Coordinates can be in any unit (degrees, meters, etc.) depending on the CRS.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BoundingBox {
    /// Minimum X coordinate (west)
    pub min_x: f64,
    /// Minimum Y coordinate (south)
    pub min_y: f64,
    /// Maximum X coordinate (east)
    pub max_x: f64,
    /// Maximum Y coordinate (north)
    pub max_y: f64,
}

impl BoundingBox {
    /// Creates a new bounding box from min/max coordinates
    ///
    /// # Arguments
    /// * `min_x` - Minimum X coordinate
    /// * `min_y` - Minimum Y coordinate
    /// * `max_x` - Maximum X coordinate
    /// * `max_y` - Maximum Y coordinate
    ///
    /// # Errors
    /// Returns an error if min > max for either axis
    pub fn new(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Result<Self> {
        if min_x > max_x {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "min_x/max_x",
                message: format!("min_x ({min_x}) must be <= max_x ({max_x})"),
            });
        }
        if min_y > max_y {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "min_y/max_y",
                message: format!("min_y ({min_y}) must be <= max_y ({max_y})"),
            });
        }
        Ok(Self {
            min_x,
            min_y,
            max_x,
            max_y,
        })
    }

    /// Creates a new bounding box without validation
    ///
    /// # Safety
    /// This is safe but may create invalid bounding boxes where min > max.
    /// Use [`BoundingBox::new`] for validated construction.
    #[must_use]
    pub const fn new_unchecked(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        Self {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    }

    /// Creates a bounding box from (west, south, east, north) coordinates
    ///
    /// This is a common ordering used in many geospatial APIs.
    ///
    /// # Errors
    /// Returns an error if coordinates are invalid
    pub fn from_wsen(west: f64, south: f64, east: f64, north: f64) -> Result<Self> {
        Self::new(west, south, east, north)
    }

    /// Creates a point bounding box (zero area)
    #[must_use]
    pub const fn point(x: f64, y: f64) -> Self {
        Self {
            min_x: x,
            min_y: y,
            max_x: x,
            max_y: y,
        }
    }

    /// Creates the world bounds in WGS84 coordinates
    #[must_use]
    pub const fn world_wgs84() -> Self {
        Self {
            min_x: -180.0,
            min_y: -90.0,
            max_x: 180.0,
            max_y: 90.0,
        }
    }

    /// Creates the world bounds in Web Mercator coordinates (EPSG:3857)
    #[must_use]
    pub const fn world_web_mercator() -> Self {
        // Web Mercator bounds (approximately -180 to 180 degrees, ±85.06°)
        Self {
            min_x: -20_037_508.342_789_244,
            min_y: -20_037_508.342_789_244,
            max_x: 20_037_508.342_789_244,
            max_y: 20_037_508.342_789_244,
        }
    }

    /// Returns the minimum X coordinate (west)
    #[must_use]
    #[inline]
    pub const fn min_x(&self) -> f64 {
        self.min_x
    }

    /// Returns the minimum Y coordinate (south)
    #[must_use]
    #[inline]
    pub const fn min_y(&self) -> f64 {
        self.min_y
    }

    /// Returns the maximum X coordinate (east)
    #[must_use]
    #[inline]
    pub const fn max_x(&self) -> f64 {
        self.max_x
    }

    /// Returns the maximum Y coordinate (north)
    #[must_use]
    #[inline]
    pub const fn max_y(&self) -> f64 {
        self.max_y
    }

    /// Returns the width of the bounding box
    #[must_use]
    pub fn width(&self) -> f64 {
        self.max_x - self.min_x
    }

    /// Returns the height of the bounding box
    #[must_use]
    pub fn height(&self) -> f64 {
        self.max_y - self.min_y
    }

    /// Returns the area of the bounding box
    #[must_use]
    pub fn area(&self) -> f64 {
        self.width() * self.height()
    }

    /// Returns the center point of the bounding box
    #[must_use]
    pub fn center(&self) -> (f64, f64) {
        (
            f64::midpoint(self.min_x, self.max_x),
            f64::midpoint(self.min_y, self.max_y),
        )
    }

    /// Returns true if this bounding box contains the given point
    #[must_use]
    pub fn contains_point(&self, x: f64, y: f64) -> bool {
        x >= self.min_x && x <= self.max_x && y >= self.min_y && y <= self.max_y
    }

    /// Returns true if this bounding box fully contains another bounding box
    #[must_use]
    pub fn contains(&self, other: &Self) -> bool {
        self.min_x <= other.min_x
            && self.max_x >= other.max_x
            && self.min_y <= other.min_y
            && self.max_y >= other.max_y
    }

    /// Returns true if this bounding box is fully within another bounding box
    ///
    /// This is the inverse of `contains()`.
    #[must_use]
    pub fn is_within(&self, other: &Self) -> bool {
        other.contains(self)
    }

    /// Returns true if this bounding box intersects with another
    #[must_use]
    pub fn intersects(&self, other: &Self) -> bool {
        self.min_x <= other.max_x
            && self.max_x >= other.min_x
            && self.min_y <= other.max_y
            && self.max_y >= other.min_y
    }

    /// Returns the intersection of two bounding boxes, if any
    #[must_use]
    pub fn intersection(&self, other: &Self) -> Option<Self> {
        if !self.intersects(other) {
            return None;
        }

        Some(Self {
            min_x: self.min_x.max(other.min_x),
            min_y: self.min_y.max(other.min_y),
            max_x: self.max_x.min(other.max_x),
            max_y: self.max_y.min(other.max_y),
        })
    }

    /// Returns the union of two bounding boxes
    #[must_use]
    pub fn union(&self, other: &Self) -> Self {
        Self {
            min_x: self.min_x.min(other.min_x),
            min_y: self.min_y.min(other.min_y),
            max_x: self.max_x.max(other.max_x),
            max_y: self.max_y.max(other.max_y),
        }
    }

    /// Expands the bounding box by the given amount in all directions
    #[must_use]
    pub fn expand(&self, amount: f64) -> Self {
        Self {
            min_x: self.min_x - amount,
            min_y: self.min_y - amount,
            max_x: self.max_x + amount,
            max_y: self.max_y + amount,
        }
    }

    /// Expands the bounding box to include the given point
    #[must_use]
    pub fn expand_to_include(&self, x: f64, y: f64) -> Self {
        Self {
            min_x: self.min_x.min(x),
            min_y: self.min_y.min(y),
            max_x: self.max_x.max(x),
            max_y: self.max_y.max(y),
        }
    }

    /// Returns true if the bounding box has zero area
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.width() == 0.0 || self.height() == 0.0
    }

    /// Returns true if the bounding box is valid (finite values, min <= max)
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.min_x.is_finite()
            && self.min_y.is_finite()
            && self.max_x.is_finite()
            && self.max_y.is_finite()
            && self.min_x <= self.max_x
            && self.min_y <= self.max_y
    }

    /// Returns the bounding box as an array [`min_x`, `min_y`, `max_x`, `max_y`]
    #[must_use]
    pub const fn as_array(&self) -> [f64; 4] {
        [self.min_x, self.min_y, self.max_x, self.max_y]
    }

    /// Returns the bounding box as (west, south, east, north) tuple
    #[must_use]
    pub const fn as_wsen(&self) -> (f64, f64, f64, f64) {
        (self.min_x, self.min_y, self.max_x, self.max_y)
    }

    /// Creates a bounding box from an array [`min_x`, `min_y`, `max_x`, `max_y`]
    ///
    /// # Errors
    /// Returns an error if the array represents an invalid bounding box
    pub fn from_array(arr: [f64; 4]) -> Result<Self> {
        Self::new(arr[0], arr[1], arr[2], arr[3])
    }
}

impl Default for BoundingBox {
    fn default() -> Self {
        Self::world_wgs84()
    }
}

impl fmt::Display for BoundingBox {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "BoundingBox({}, {}, {}, {})",
            self.min_x, self.min_y, self.max_x, self.max_y
        )
    }
}

/// A 3D bounding box with optional Z coordinates
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BoundingBox3D {
    /// 2D bounding box (X, Y)
    pub xy: BoundingBox,
    /// Minimum Z coordinate
    pub min_z: f64,
    /// Maximum Z coordinate
    pub max_z: f64,
}

impl BoundingBox3D {
    /// Creates a new 3D bounding box
    ///
    /// # Errors
    /// Returns an error if any coordinates are invalid
    pub fn new(
        min_x: f64,
        min_y: f64,
        min_z: f64,
        max_x: f64,
        max_y: f64,
        max_z: f64,
    ) -> Result<Self> {
        let xy = BoundingBox::new(min_x, min_y, max_x, max_y)?;
        if min_z > max_z {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "min_z/max_z",
                message: format!("min_z ({min_z}) must be <= max_z ({max_z})"),
            });
        }
        Ok(Self { xy, min_z, max_z })
    }

    /// Returns the 2D bounding box (ignoring Z)
    #[must_use]
    pub const fn as_2d(&self) -> &BoundingBox {
        &self.xy
    }

    /// Returns the depth (Z range)
    #[must_use]
    pub fn depth(&self) -> f64 {
        self.max_z - self.min_z
    }

    /// Returns the volume of the bounding box
    #[must_use]
    pub fn volume(&self) -> f64 {
        self.xy.area() * self.depth()
    }
}

impl Default for BoundingBox3D {
    fn default() -> Self {
        Self {
            xy: BoundingBox::world_wgs84(),
            min_z: 0.0,
            max_z: 0.0,
        }
    }
}

/// Pixel extent (integer bounds)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PixelExtent {
    /// Column offset (x)
    pub col: u64,
    /// Row offset (y)
    pub row: u64,
    /// Width in pixels
    pub width: u64,
    /// Height in pixels
    pub height: u64,
}

impl PixelExtent {
    /// Creates a new pixel extent
    #[must_use]
    pub const fn new(col: u64, row: u64, width: u64, height: u64) -> Self {
        Self {
            col,
            row,
            width,
            height,
        }
    }

    /// Creates a pixel extent from origin (full raster)
    #[must_use]
    pub const fn from_dimensions(width: u64, height: u64) -> Self {
        Self {
            col: 0,
            row: 0,
            width,
            height,
        }
    }

    /// Returns the total number of pixels
    #[must_use]
    pub const fn pixel_count(&self) -> u64 {
        self.width * self.height
    }

    /// Returns the rightmost column (exclusive)
    #[must_use]
    pub const fn col_end(&self) -> u64 {
        self.col + self.width
    }

    /// Returns the bottommost row (exclusive)
    #[must_use]
    pub const fn row_end(&self) -> u64 {
        self.row + self.height
    }

    /// Returns true if this extent is empty
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// Returns true if this extent contains the given pixel
    #[must_use]
    pub const fn contains_pixel(&self, col: u64, row: u64) -> bool {
        col >= self.col && col < self.col_end() && row >= self.row && row < self.row_end()
    }

    /// Returns true if this extent intersects with another
    #[must_use]
    pub const fn intersects(&self, other: &Self) -> bool {
        self.col < other.col_end()
            && self.col_end() > other.col
            && self.row < other.row_end()
            && self.row_end() > other.row
    }

    /// Returns the intersection of two extents, if any
    #[must_use]
    pub fn intersection(&self, other: &Self) -> Option<Self> {
        if !self.intersects(other) {
            return None;
        }

        let col = self.col.max(other.col);
        let row = self.row.max(other.row);
        let col_end = self.col_end().min(other.col_end());
        let row_end = self.row_end().min(other.row_end());

        Some(Self {
            col,
            row,
            width: col_end - col,
            height: row_end - row,
        })
    }
}

impl Default for PixelExtent {
    fn default() -> Self {
        Self::new(0, 0, 0, 0)
    }
}

impl fmt::Display for PixelExtent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "PixelExtent(col={}, row={}, {}x{})",
            self.col, self.row, self.width, self.height
        )
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    #![allow(clippy::float_cmp)]

    use super::*;

    #[test]
    fn test_bbox_creation() {
        let bbox = BoundingBox::new(-180.0, -90.0, 180.0, 90.0);
        assert!(bbox.is_ok());

        let invalid = BoundingBox::new(180.0, -90.0, -180.0, 90.0);
        assert!(invalid.is_err());
    }

    #[test]
    fn test_bbox_dimensions() {
        let bbox = BoundingBox::new(0.0, 0.0, 100.0, 50.0).expect("valid bbox");
        assert_eq!(bbox.width(), 100.0);
        assert_eq!(bbox.height(), 50.0);
        assert_eq!(bbox.area(), 5000.0);
        assert_eq!(bbox.center(), (50.0, 25.0));
    }

    #[test]
    fn test_bbox_contains() {
        let outer = BoundingBox::new(0.0, 0.0, 100.0, 100.0).expect("valid bbox");
        let inner = BoundingBox::new(25.0, 25.0, 75.0, 75.0).expect("valid bbox");

        assert!(outer.contains(&inner));
        assert!(!inner.contains(&outer));
        assert!(outer.contains_point(50.0, 50.0));
        assert!(!outer.contains_point(150.0, 50.0));
    }

    #[test]
    fn test_bbox_intersection() {
        let a = BoundingBox::new(0.0, 0.0, 50.0, 50.0).expect("valid bbox");
        let b = BoundingBox::new(25.0, 25.0, 75.0, 75.0).expect("valid bbox");
        let c = BoundingBox::new(100.0, 100.0, 150.0, 150.0).expect("valid bbox");

        assert!(a.intersects(&b));
        assert!(!a.intersects(&c));

        let intersection = a.intersection(&b);
        assert!(intersection.is_some());
        let int_bbox = intersection.expect("intersection exists");
        assert_eq!(int_bbox.min_x, 25.0);
        assert_eq!(int_bbox.min_y, 25.0);
        assert_eq!(int_bbox.max_x, 50.0);
        assert_eq!(int_bbox.max_y, 50.0);
    }

    #[test]
    fn test_bbox_union() {
        let a = BoundingBox::new(0.0, 0.0, 50.0, 50.0).expect("valid bbox");
        let b = BoundingBox::new(25.0, 25.0, 75.0, 75.0).expect("valid bbox");

        let union = a.union(&b);
        assert_eq!(union.min_x, 0.0);
        assert_eq!(union.min_y, 0.0);
        assert_eq!(union.max_x, 75.0);
        assert_eq!(union.max_y, 75.0);
    }

    #[test]
    fn test_pixel_extent() {
        let extent = PixelExtent::new(10, 20, 100, 200);
        assert_eq!(extent.pixel_count(), 20_000);
        assert_eq!(extent.col_end(), 110);
        assert_eq!(extent.row_end(), 220);
        assert!(extent.contains_pixel(50, 100));
        assert!(!extent.contains_pixel(5, 100));
    }

    #[test]
    fn test_pixel_extent_intersection() {
        let a = PixelExtent::new(0, 0, 100, 100);
        let b = PixelExtent::new(50, 50, 100, 100);

        assert!(a.intersects(&b));
        let int_extent = a.intersection(&b).expect("intersection exists");
        assert_eq!(int_extent.col, 50);
        assert_eq!(int_extent.row, 50);
        assert_eq!(int_extent.width, 50);
        assert_eq!(int_extent.height, 50);
    }

    #[test]
    fn test_world_bounds() {
        let wgs84 = BoundingBox::world_wgs84();
        assert!(wgs84.is_valid());
        assert_eq!(wgs84.width(), 360.0);
        assert_eq!(wgs84.height(), 180.0);

        let web_mercator = BoundingBox::world_web_mercator();
        assert!(web_mercator.is_valid());
    }
}
