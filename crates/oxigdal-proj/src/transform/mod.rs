//! Coordinate transformation operations.
//!
//! This module provides coordinate transformation capabilities between different CRS
//! using the proj4rs library for pure Rust implementations, as well as native pure-Rust
//! implementations of many map projections.
//!
//! # Module Structure
//!
//! - `cylindrical`   — Cylindrical projections (Mercator, Transverse Mercator, Cassini, etc.)
//! - `pseudocylindrical` — Pseudo-cylindrical projections (Sinusoidal, Mollweide, Robinson, Eckert IV/VI)
//! - `conic`         — Conic projections (Lambert Conic, Equidistant Conic, Albers)
//! - `azimuthal`     — Azimuthal projections (Lambert Azimuthal Equal Area, Azimuthal Equidistant, Gnomonic)

pub mod azimuthal;
pub mod conic;
pub mod cylindrical;
pub mod pseudocylindrical;

use crate::crs::Crs;
use crate::error::{Error, Result};
use std::fmt;

// Re-export projection types for easy access
pub use azimuthal::{AzimuthalEquidistant, Gnomonic, LambertAzimuthalEqualArea};
pub use conic::{EquidistantConic, LambertConformalConic};
pub use cylindrical::{CassineSoldner, GaussKruger, TransverseMercator};
pub use pseudocylindrical::{EckertIV, EckertVI, Mollweide, Robinson, Sinusoidal};

/// A 2D coordinate (x, y) or (longitude, latitude).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coordinate {
    /// X coordinate (or longitude in geographic CRS)
    pub x: f64,
    /// Y coordinate (or latitude in geographic CRS)
    pub y: f64,
}

impl Coordinate {
    /// Creates a new coordinate.
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// Creates a coordinate from longitude and latitude (in degrees).
    pub fn from_lon_lat(lon: f64, lat: f64) -> Self {
        Self::new(lon, lat)
    }

    /// Returns the longitude (assumes geographic CRS).
    pub fn lon(&self) -> f64 {
        self.x
    }

    /// Returns the latitude (assumes geographic CRS).
    pub fn lat(&self) -> f64 {
        self.y
    }

    /// Validates that the coordinate is within valid bounds for a geographic CRS.
    pub fn validate_geographic(&self) -> Result<()> {
        if !(-180.0..=180.0).contains(&self.x) {
            return Err(Error::coordinate_out_of_bounds(self.x, self.y));
        }
        if !(-90.0..=90.0).contains(&self.y) {
            return Err(Error::coordinate_out_of_bounds(self.x, self.y));
        }
        Ok(())
    }

    /// Checks if the coordinate contains valid (finite) values.
    pub fn is_valid(&self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

impl fmt::Display for Coordinate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {})", self.x, self.y)
    }
}

/// A 3D coordinate (x, y, z).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coordinate3D {
    /// X coordinate
    pub x: f64,
    /// Y coordinate
    pub y: f64,
    /// Z coordinate (elevation/height)
    pub z: f64,
}

impl Coordinate3D {
    /// Creates a new 3D coordinate.
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    /// Converts to 2D coordinate (drops Z).
    pub fn to_2d(&self) -> Coordinate {
        Coordinate::new(self.x, self.y)
    }

    /// Checks if the coordinate contains valid (finite) values.
    pub fn is_valid(&self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}

impl From<Coordinate> for Coordinate3D {
    fn from(coord: Coordinate) -> Self {
        Self::new(coord.x, coord.y, 0.0)
    }
}

/// A bounding box defined by minimum and maximum coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundingBox {
    /// Minimum X coordinate
    pub min_x: f64,
    /// Minimum Y coordinate
    pub min_y: f64,
    /// Maximum X coordinate
    pub max_x: f64,
    /// Maximum Y coordinate
    pub max_y: f64,
}

impl BoundingBox {
    /// Creates a new bounding box.
    ///
    /// # Errors
    ///
    /// Returns an error if min > max for any dimension.
    pub fn new(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Result<Self> {
        if min_x > max_x {
            return Err(Error::invalid_bounding_box(format!(
                "min_x ({}) > max_x ({})",
                min_x, max_x
            )));
        }
        if min_y > max_y {
            return Err(Error::invalid_bounding_box(format!(
                "min_y ({}) > max_y ({})",
                min_y, max_y
            )));
        }

        Ok(Self {
            min_x,
            min_y,
            max_x,
            max_y,
        })
    }

    /// Creates a bounding box from two coordinates.
    pub fn from_coordinates(c1: Coordinate, c2: Coordinate) -> Result<Self> {
        let min_x = c1.x.min(c2.x);
        let min_y = c1.y.min(c2.y);
        let max_x = c1.x.max(c2.x);
        let max_y = c1.y.max(c2.y);
        Self::new(min_x, min_y, max_x, max_y)
    }

    /// Returns the width of the bounding box.
    pub fn width(&self) -> f64 {
        self.max_x - self.min_x
    }

    /// Returns the height of the bounding box.
    pub fn height(&self) -> f64 {
        self.max_y - self.min_y
    }

    /// Returns the center coordinate of the bounding box.
    pub fn center(&self) -> Coordinate {
        Coordinate::new(
            (self.min_x + self.max_x) / 2.0,
            (self.min_y + self.max_y) / 2.0,
        )
    }

    /// Returns the four corner coordinates.
    pub fn corners(&self) -> [Coordinate; 4] {
        [
            Coordinate::new(self.min_x, self.min_y),
            Coordinate::new(self.max_x, self.min_y),
            Coordinate::new(self.max_x, self.max_y),
            Coordinate::new(self.min_x, self.max_y),
        ]
    }

    /// Checks if a coordinate is within the bounding box.
    pub fn contains(&self, coord: &Coordinate) -> bool {
        coord.x >= self.min_x
            && coord.x <= self.max_x
            && coord.y >= self.min_y
            && coord.y <= self.max_y
    }

    /// Expands the bounding box to include a coordinate.
    pub fn expand_to_include(&mut self, coord: &Coordinate) {
        self.min_x = self.min_x.min(coord.x);
        self.min_y = self.min_y.min(coord.y);
        self.max_x = self.max_x.max(coord.x);
        self.max_y = self.max_y.max(coord.y);
    }
}

/// Coordinate transformer that handles transformations between CRS.
pub struct Transformer {
    source_crs: Crs,
    target_crs: Crs,
    proj: Option<proj4rs::Proj>,
}

impl Transformer {
    /// Creates a new transformer.
    ///
    /// # Arguments
    ///
    /// * `source_crs` - Source coordinate reference system
    /// * `target_crs` - Target coordinate reference system
    ///
    /// # Errors
    ///
    /// Returns an error if the transformation cannot be initialized.
    pub fn new(source_crs: Crs, target_crs: Crs) -> Result<Self> {
        // Check if CRS are the same (no transformation needed)
        let proj = if source_crs.is_equivalent(&target_crs) {
            None
        } else {
            // Initialize proj4rs transformation
            let source_proj_str = source_crs.to_proj_string()?;
            let target_proj_str = target_crs.to_proj_string()?;

            let _source_proj = proj4rs::Proj::from_proj_string(&source_proj_str)
                .map_err(|e| Error::projection_init_error(format!("Source CRS: {:?}", e)))?;

            let target_proj = proj4rs::Proj::from_proj_string(&target_proj_str)
                .map_err(|e| Error::projection_init_error(format!("Target CRS: {:?}", e)))?;

            // We'll store the target proj for now, and use proj4rs::transform later
            Some(target_proj)
        };

        Ok(Self {
            source_crs,
            target_crs,
            proj,
        })
    }

    /// Creates a transformer from EPSG codes.
    ///
    /// # Arguments
    ///
    /// * `source_epsg` - Source EPSG code
    /// * `target_epsg` - Target EPSG code
    ///
    /// # Errors
    ///
    /// Returns an error if the EPSG codes are invalid or transformation cannot be initialized.
    pub fn from_epsg(source_epsg: u32, target_epsg: u32) -> Result<Self> {
        let source_crs = Crs::from_epsg(source_epsg)?;
        let target_crs = Crs::from_epsg(target_epsg)?;
        Self::new(source_crs, target_crs)
    }

    /// Returns the source CRS.
    pub fn source_crs(&self) -> &Crs {
        &self.source_crs
    }

    /// Returns the target CRS.
    pub fn target_crs(&self) -> &Crs {
        &self.target_crs
    }

    /// Transforms a single coordinate.
    ///
    /// # Arguments
    ///
    /// * `coord` - Input coordinate in source CRS
    ///
    /// # Errors
    ///
    /// Returns an error if the transformation fails.
    pub fn transform(&self, coord: &Coordinate) -> Result<Coordinate> {
        // If no transformation needed, return as-is
        if self.proj.is_none() {
            return Ok(*coord);
        }

        // Validate input
        if !coord.is_valid() {
            return Err(Error::invalid_coordinate(
                "Coordinate contains non-finite values",
            ));
        }

        // Perform transformation using proj4rs
        self.transform_impl(coord)
    }

    /// Transforms a 3D coordinate.
    pub fn transform_3d(&self, coord: &Coordinate3D) -> Result<Coordinate3D> {
        if self.proj.is_none() {
            return Ok(*coord);
        }

        if !coord.is_valid() {
            return Err(Error::invalid_coordinate(
                "Coordinate contains non-finite values",
            ));
        }

        // Transform 2D part
        let coord_2d = coord.to_2d();
        let transformed_2d = self.transform_impl(&coord_2d)?;

        // Keep Z coordinate (proper 3D transformation would require more complex logic)
        Ok(Coordinate3D::new(
            transformed_2d.x,
            transformed_2d.y,
            coord.z,
        ))
    }

    /// Transforms multiple coordinates in batch.
    ///
    /// This is more efficient than transforming one-by-one for large datasets.
    ///
    /// # Arguments
    ///
    /// * `coords` - Input coordinates in source CRS
    ///
    /// # Errors
    ///
    /// Returns an error if any transformation fails.
    pub fn transform_batch(&self, coords: &[Coordinate]) -> Result<Vec<Coordinate>> {
        coords.iter().map(|c| self.transform(c)).collect()
    }

    /// Transforms a bounding box.
    ///
    /// This transforms all four corners and creates a new bounding box from the results.
    ///
    /// # Arguments
    ///
    /// * `bbox` - Input bounding box in source CRS
    ///
    /// # Errors
    ///
    /// Returns an error if the transformation fails.
    pub fn transform_bbox(&self, bbox: &BoundingBox) -> Result<BoundingBox> {
        if self.proj.is_none() {
            return Ok(*bbox);
        }

        // Transform all four corners
        let corners = bbox.corners();
        let transformed_corners = self.transform_batch(&corners)?;

        // Find new bounds
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;

        for corner in &transformed_corners {
            min_x = min_x.min(corner.x);
            min_y = min_y.min(corner.y);
            max_x = max_x.max(corner.x);
            max_y = max_y.max(corner.y);
        }

        BoundingBox::new(min_x, min_y, max_x, max_y)
    }

    /// Internal implementation of coordinate transformation using proj4rs.
    fn transform_impl(&self, coord: &Coordinate) -> Result<Coordinate> {
        let source_proj_str = self.source_crs.to_proj_string()?;
        let target_proj_str = self.target_crs.to_proj_string()?;

        let source_proj = proj4rs::Proj::from_proj_string(&source_proj_str)
            .map_err(|e| Error::from_proj4rs(format!("{:?}", e)))?;

        let target_proj = proj4rs::Proj::from_proj_string(&target_proj_str)
            .map_err(|e| Error::from_proj4rs(format!("{:?}", e)))?;

        // Convert to radians if source is geographic
        let mut x = coord.x;
        let mut y = coord.y;

        if self.source_crs.is_geographic() {
            x = x.to_radians();
            y = y.to_radians();
        }

        // Perform transformation using a mutable array (proj4rs requires slice)
        let mut points = [(x, y)];
        proj4rs::transform::transform(&source_proj, &target_proj, &mut points[..])
            .map_err(|e| Error::transformation_error(format!("{:?}", e)))?;

        let (mut result_x, mut result_y) = points[0];

        // Convert from radians if target is geographic
        if self.target_crs.is_geographic() {
            result_x = result_x.to_degrees();
            result_y = result_y.to_degrees();
        }

        let transformed = Coordinate::new(result_x, result_y);

        if !transformed.is_valid() {
            return Err(Error::transformation_error(
                "Transformation resulted in non-finite values",
            ));
        }

        Ok(transformed)
    }
}

/// Transforms a coordinate from one CRS to another (convenience function).
///
/// # Arguments
///
/// * `coord` - Input coordinate
/// * `source_crs` - Source CRS
/// * `target_crs` - Target CRS
///
/// # Errors
///
/// Returns an error if the transformation fails.
pub fn transform_coordinate(
    coord: &Coordinate,
    source_crs: &Crs,
    target_crs: &Crs,
) -> Result<Coordinate> {
    let transformer = Transformer::new(source_crs.clone(), target_crs.clone())?;
    transformer.transform(coord)
}

/// Transforms coordinates from one EPSG code to another (convenience function).
///
/// # Arguments
///
/// * `coord` - Input coordinate
/// * `source_epsg` - Source EPSG code
/// * `target_epsg` - Target EPSG code
///
/// # Errors
///
/// Returns an error if the transformation fails.
pub fn transform_epsg(
    coord: &Coordinate,
    source_epsg: u32,
    target_epsg: u32,
) -> Result<Coordinate> {
    let transformer = Transformer::from_epsg(source_epsg, target_epsg)?;
    transformer.transform(coord)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_coordinate_creation() {
        let coord = Coordinate::new(10.0, 20.0);
        assert_eq!(coord.x, 10.0);
        assert_eq!(coord.y, 20.0);
    }

    #[test]
    fn test_coordinate_from_lon_lat() {
        let coord = Coordinate::from_lon_lat(-122.4194, 37.7749);
        assert_eq!(coord.lon(), -122.4194);
        assert_eq!(coord.lat(), 37.7749);
    }

    #[test]
    fn test_coordinate_validation() {
        let valid = Coordinate::new(0.0, 0.0);
        assert!(valid.validate_geographic().is_ok());

        let invalid_lon = Coordinate::new(200.0, 0.0);
        assert!(invalid_lon.validate_geographic().is_err());

        let invalid_lat = Coordinate::new(0.0, 100.0);
        assert!(invalid_lat.validate_geographic().is_err());
    }

    #[test]
    fn test_coordinate_is_valid() {
        let valid = Coordinate::new(1.0, 2.0);
        assert!(valid.is_valid());

        let invalid = Coordinate::new(f64::NAN, 2.0);
        assert!(!invalid.is_valid());

        let infinite = Coordinate::new(f64::INFINITY, 2.0);
        assert!(!infinite.is_valid());
    }

    #[test]
    fn test_coordinate3d() {
        let coord = Coordinate3D::new(1.0, 2.0, 3.0);
        assert_eq!(coord.x, 1.0);
        assert_eq!(coord.y, 2.0);
        assert_eq!(coord.z, 3.0);

        let coord_2d = coord.to_2d();
        assert_eq!(coord_2d.x, 1.0);
        assert_eq!(coord_2d.y, 2.0);
    }

    #[test]
    fn test_bounding_box() {
        let bbox = BoundingBox::new(0.0, 0.0, 10.0, 20.0);
        assert!(bbox.is_ok());

        let bbox = bbox.expect("should be valid");
        assert_eq!(bbox.width(), 10.0);
        assert_eq!(bbox.height(), 20.0);

        let center = bbox.center();
        assert_eq!(center.x, 5.0);
        assert_eq!(center.y, 10.0);
    }

    #[test]
    fn test_bounding_box_invalid() {
        let result = BoundingBox::new(10.0, 0.0, 0.0, 20.0);
        assert!(result.is_err());

        let result = BoundingBox::new(0.0, 20.0, 10.0, 0.0);
        assert!(result.is_err());
    }

    #[test]
    fn test_bounding_box_contains() {
        let bbox = BoundingBox::new(0.0, 0.0, 10.0, 10.0).expect("valid bbox");

        assert!(bbox.contains(&Coordinate::new(5.0, 5.0)));
        assert!(bbox.contains(&Coordinate::new(0.0, 0.0)));
        assert!(bbox.contains(&Coordinate::new(10.0, 10.0)));
        assert!(!bbox.contains(&Coordinate::new(-1.0, 5.0)));
        assert!(!bbox.contains(&Coordinate::new(5.0, 11.0)));
    }

    #[test]
    fn test_bounding_box_expand() {
        let mut bbox = BoundingBox::new(0.0, 0.0, 10.0, 10.0).expect("valid bbox");

        bbox.expand_to_include(&Coordinate::new(15.0, 5.0));
        assert_eq!(bbox.max_x, 15.0);

        bbox.expand_to_include(&Coordinate::new(5.0, -5.0));
        assert_eq!(bbox.min_y, -5.0);
    }

    #[test]
    fn test_transformer_same_crs() {
        let wgs84 = Crs::wgs84();
        let transformer = Transformer::new(wgs84.clone(), wgs84.clone());
        assert!(transformer.is_ok());

        let transformer = transformer.expect("should create transformer");
        let coord = Coordinate::new(10.0, 20.0);
        let result = transformer.transform(&coord);
        assert!(result.is_ok());

        let result = result.expect("should transform");
        assert_eq!(result, coord);
    }

    #[test]
    fn test_transformer_wgs84_to_web_mercator() {
        let transformer = Transformer::from_epsg(4326, 3857);
        assert!(transformer.is_ok());

        let transformer = transformer.expect("should create transformer");

        // Transform London coordinates (0.0, 51.5)
        let london = Coordinate::from_lon_lat(0.0, 51.5);
        let result = transformer.transform(&london);
        assert!(result.is_ok());

        let result = result.expect("should transform");
        // Web Mercator should give us meters from equator
        // X should be close to 0 (prime meridian)
        assert_relative_eq!(result.x, 0.0, epsilon = 1.0);
        // Y should be positive (northern hemisphere)
        assert!(result.y > 6_000_000.0 && result.y < 7_000_000.0);
    }

    #[test]
    fn test_transform_batch() {
        let transformer = Transformer::from_epsg(4326, 4326).expect("same CRS");

        let coords = vec![
            Coordinate::new(0.0, 0.0),
            Coordinate::new(10.0, 10.0),
            Coordinate::new(20.0, 20.0),
        ];

        let result = transformer.transform_batch(&coords);
        assert!(result.is_ok());

        let result = result.expect("should transform");
        assert_eq!(result.len(), 3);
        assert_eq!(result[0], coords[0]);
        assert_eq!(result[1], coords[1]);
        assert_eq!(result[2], coords[2]);
    }

    #[test]
    fn test_transform_bbox() {
        let transformer = Transformer::from_epsg(4326, 4326).expect("same CRS");

        let bbox = BoundingBox::new(0.0, 0.0, 10.0, 10.0).expect("valid bbox");
        let result = transformer.transform_bbox(&bbox);
        assert!(result.is_ok());

        let result = result.expect("should transform");
        assert_eq!(result, bbox);
    }

    #[test]
    fn test_convenience_functions() {
        let wgs84 = Crs::wgs84();
        let coord = Coordinate::new(0.0, 0.0);

        let result = transform_coordinate(&coord, &wgs84, &wgs84);
        assert!(result.is_ok());
        assert_eq!(result.expect("should transform"), coord);

        let result = transform_epsg(&coord, 4326, 4326);
        assert!(result.is_ok());
        assert_eq!(result.expect("should transform"), coord);
    }

    #[test]
    fn test_transform_invalid_coordinate() {
        let transformer = Transformer::from_epsg(4326, 3857).expect("should create");

        let invalid = Coordinate::new(f64::NAN, 0.0);
        let result = transformer.transform(&invalid);
        assert!(result.is_err());
    }
}
