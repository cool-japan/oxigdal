//! Geometry types for vector data
//!
//! This module provides geometry types compatible with Simple Features specification.

use crate::error::{OxiGdalError, Result};
use serde::{Deserialize, Serialize};

#[cfg(feature = "std")]
use std::vec::Vec;

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

/// Coordinate in 2D, 3D, or 4D space
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Coordinate {
    /// X coordinate (longitude)
    pub x: f64,
    /// Y coordinate (latitude)
    pub y: f64,
    /// Z coordinate (elevation) - optional
    pub z: Option<f64>,
    /// M coordinate (measure) - optional
    pub m: Option<f64>,
}

impl Coordinate {
    /// Creates a new 2D coordinate
    #[must_use]
    pub const fn new_2d(x: f64, y: f64) -> Self {
        Self {
            x,
            y,
            z: None,
            m: None,
        }
    }

    /// Creates a new 3D coordinate
    #[must_use]
    pub const fn new_3d(x: f64, y: f64, z: f64) -> Self {
        Self {
            x,
            y,
            z: Some(z),
            m: None,
        }
    }

    /// Creates a new coordinate with measure
    #[must_use]
    pub const fn new_2dm(x: f64, y: f64, m: f64) -> Self {
        Self {
            x,
            y,
            z: None,
            m: Some(m),
        }
    }

    /// Creates a new 3D coordinate with measure
    #[must_use]
    pub const fn new_3dm(x: f64, y: f64, z: f64, m: f64) -> Self {
        Self {
            x,
            y,
            z: Some(z),
            m: Some(m),
        }
    }

    /// Returns true if this coordinate has Z dimension
    #[must_use]
    pub const fn has_z(&self) -> bool {
        self.z.is_some()
    }

    /// Returns true if this coordinate has M dimension
    #[must_use]
    pub const fn has_m(&self) -> bool {
        self.m.is_some()
    }

    /// Returns the number of dimensions (2, 3, or 4)
    #[must_use]
    pub const fn dimensions(&self) -> u8 {
        let mut dims = 2;
        if self.z.is_some() {
            dims += 1;
        }
        if self.m.is_some() {
            dims += 1;
        }
        dims
    }

    /// Returns the X coordinate
    #[must_use]
    pub const fn x(&self) -> f64 {
        self.x
    }

    /// Returns the Y coordinate
    #[must_use]
    pub const fn y(&self) -> f64 {
        self.y
    }

    /// Returns the Z coordinate if present
    #[must_use]
    pub const fn z(&self) -> Option<f64> {
        self.z
    }

    /// Returns the M coordinate if present
    #[must_use]
    pub const fn m(&self) -> Option<f64> {
        self.m
    }
}

/// Geometry type enumeration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Geometry {
    /// Point geometry
    Point(Point),
    /// `LineString` geometry
    LineString(LineString),
    /// Polygon geometry
    Polygon(Polygon),
    /// `MultiPoint` geometry
    MultiPoint(MultiPoint),
    /// `MultiLineString` geometry
    MultiLineString(MultiLineString),
    /// `MultiPolygon` geometry
    MultiPolygon(MultiPolygon),
    /// `GeometryCollection`
    GeometryCollection(GeometryCollection),
}

impl Geometry {
    /// Returns the geometry type as a string
    #[must_use]
    pub const fn geometry_type(&self) -> &'static str {
        match self {
            Self::Point(_) => "Point",
            Self::LineString(_) => "LineString",
            Self::Polygon(_) => "Polygon",
            Self::MultiPoint(_) => "MultiPoint",
            Self::MultiLineString(_) => "MultiLineString",
            Self::MultiPolygon(_) => "MultiPolygon",
            Self::GeometryCollection(_) => "GeometryCollection",
        }
    }

    /// Returns true if the geometry is empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Point(p) => p.coord.x.is_nan() || p.coord.y.is_nan(),
            Self::LineString(ls) => ls.coords.is_empty(),
            Self::Polygon(p) => p.exterior.coords.is_empty(),
            Self::MultiPoint(mp) => mp.points.is_empty(),
            Self::MultiLineString(mls) => mls.line_strings.is_empty(),
            Self::MultiPolygon(mp) => mp.polygons.is_empty(),
            Self::GeometryCollection(gc) => gc.geometries.is_empty(),
        }
    }

    /// Computes the bounding box of the geometry
    #[must_use]
    pub fn bounds(&self) -> Option<(f64, f64, f64, f64)> {
        match self {
            Self::Point(p) => {
                if p.coord.x.is_nan() || p.coord.y.is_nan() {
                    None
                } else {
                    Some((p.coord.x, p.coord.y, p.coord.x, p.coord.y))
                }
            }
            Self::LineString(ls) => ls.bounds(),
            Self::Polygon(p) => p.bounds(),
            Self::MultiPoint(mp) => mp.bounds(),
            Self::MultiLineString(mls) => mls.bounds(),
            Self::MultiPolygon(mp) => mp.bounds(),
            Self::GeometryCollection(gc) => gc.bounds(),
        }
    }
}

/// Point geometry
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Point {
    /// Coordinate of the point
    pub coord: Coordinate,
}

impl Point {
    /// Creates a new 2D point
    #[must_use]
    pub const fn new(x: f64, y: f64) -> Self {
        Self {
            coord: Coordinate::new_2d(x, y),
        }
    }

    /// Creates a new 3D point
    #[must_use]
    pub const fn new_3d(x: f64, y: f64, z: f64) -> Self {
        Self {
            coord: Coordinate::new_3d(x, y, z),
        }
    }

    /// Creates a point from a coordinate
    #[must_use]
    pub const fn from_coord(coord: Coordinate) -> Self {
        Self { coord }
    }

    /// Returns the X coordinate
    #[must_use]
    pub const fn x(&self) -> f64 {
        self.coord.x
    }

    /// Returns the Y coordinate
    #[must_use]
    pub const fn y(&self) -> f64 {
        self.coord.y
    }

    /// Returns the Z coordinate if present
    #[must_use]
    pub const fn z(&self) -> Option<f64> {
        self.coord.z
    }
}

/// `LineString` geometry
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LineString {
    /// Coordinates of the line string
    pub coords: Vec<Coordinate>,
}

impl LineString {
    /// Creates a new line string
    pub fn new(coords: Vec<Coordinate>) -> Result<Self> {
        if coords.len() < 2 {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "coords",
                message: "LineString must have at least 2 coordinates".to_string(),
            });
        }
        Ok(Self { coords })
    }

    /// Creates a new empty line string (for building)
    #[must_use]
    pub const fn empty() -> Self {
        Self { coords: Vec::new() }
    }

    /// Adds a coordinate to the line string
    pub fn push(&mut self, coord: Coordinate) {
        self.coords.push(coord);
    }

    /// Returns the number of coordinates
    #[must_use]
    pub fn len(&self) -> usize {
        self.coords.len()
    }

    /// Returns true if the line string is empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.coords.is_empty()
    }

    /// Computes the bounding box
    #[must_use]
    pub fn bounds(&self) -> Option<(f64, f64, f64, f64)> {
        if self.coords.is_empty() {
            return None;
        }

        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;

        for coord in &self.coords {
            min_x = min_x.min(coord.x);
            min_y = min_y.min(coord.y);
            max_x = max_x.max(coord.x);
            max_y = max_y.max(coord.y);
        }

        Some((min_x, min_y, max_x, max_y))
    }

    /// Returns a reference to the coordinates as a slice
    #[must_use]
    pub fn coords(&self) -> &[Coordinate] {
        &self.coords
    }

    /// Returns an iterator over the coordinates
    pub fn points(&self) -> impl Iterator<Item = &Coordinate> {
        self.coords.iter()
    }
}

/// Polygon geometry
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Polygon {
    /// Exterior ring
    pub exterior: LineString,
    /// Interior rings (holes)
    pub interiors: Vec<LineString>,
}

impl Polygon {
    /// Creates a new polygon
    pub fn new(exterior: LineString, interiors: Vec<LineString>) -> Result<Self> {
        if exterior.coords.len() < 4 {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "exterior",
                message: "Polygon exterior ring must have at least 4 coordinates".to_string(),
            });
        }

        // Check if ring is closed
        let first = &exterior.coords[0];
        let last = &exterior.coords[exterior.coords.len() - 1];
        if (first.x - last.x).abs() > f64::EPSILON || (first.y - last.y).abs() > f64::EPSILON {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "exterior",
                message: "Polygon exterior ring must be closed".to_string(),
            });
        }

        // Validate interior rings
        for interior in &interiors {
            if interior.coords.len() < 4 {
                return Err(OxiGdalError::InvalidParameter {
                    parameter: "interiors",
                    message: "Polygon interior ring must have at least 4 coordinates".to_string(),
                });
            }

            let first = &interior.coords[0];
            let last = &interior.coords[interior.coords.len() - 1];
            if (first.x - last.x).abs() > f64::EPSILON || (first.y - last.y).abs() > f64::EPSILON {
                return Err(OxiGdalError::InvalidParameter {
                    parameter: "interiors",
                    message: "Polygon interior ring must be closed".to_string(),
                });
            }
        }

        Ok(Self {
            exterior,
            interiors,
        })
    }

    /// Creates a new empty polygon
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            exterior: LineString::empty(),
            interiors: Vec::new(),
        }
    }

    /// Computes the bounding box
    #[must_use]
    pub fn bounds(&self) -> Option<(f64, f64, f64, f64)> {
        self.exterior.bounds()
    }

    /// Returns a reference to the exterior ring
    #[must_use]
    pub fn exterior(&self) -> &LineString {
        &self.exterior
    }

    /// Returns a reference to the interior rings (holes)
    #[must_use]
    pub fn interiors(&self) -> &[LineString] {
        &self.interiors
    }
}

/// `MultiPoint` geometry
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MultiPoint {
    /// Points in the collection
    pub points: Vec<Point>,
}

impl MultiPoint {
    /// Creates a new multi-point
    #[must_use]
    pub const fn new(points: Vec<Point>) -> Self {
        Self { points }
    }

    /// Creates an empty multi-point
    #[must_use]
    pub const fn empty() -> Self {
        Self { points: Vec::new() }
    }

    /// Adds a point
    pub fn push(&mut self, point: Point) {
        self.points.push(point);
    }

    /// Computes the bounding box
    #[must_use]
    pub fn bounds(&self) -> Option<(f64, f64, f64, f64)> {
        if self.points.is_empty() {
            return None;
        }

        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;

        for point in &self.points {
            min_x = min_x.min(point.coord.x);
            min_y = min_y.min(point.coord.y);
            max_x = max_x.max(point.coord.x);
            max_y = max_y.max(point.coord.y);
        }

        Some((min_x, min_y, max_x, max_y))
    }
}

/// `MultiLineString` geometry
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MultiLineString {
    /// Line strings in the collection
    pub line_strings: Vec<LineString>,
}

impl MultiLineString {
    /// Creates a new multi-line-string
    #[must_use]
    pub const fn new(line_strings: Vec<LineString>) -> Self {
        Self { line_strings }
    }

    /// Creates an empty multi-line-string
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            line_strings: Vec::new(),
        }
    }

    /// Adds a line string
    pub fn push(&mut self, line_string: LineString) {
        self.line_strings.push(line_string);
    }

    /// Computes the bounding box
    #[must_use]
    pub fn bounds(&self) -> Option<(f64, f64, f64, f64)> {
        if self.line_strings.is_empty() {
            return None;
        }

        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;

        for ls in &self.line_strings {
            if let Some((x_min, y_min, x_max, y_max)) = ls.bounds() {
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

/// `MultiPolygon` geometry
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MultiPolygon {
    /// Polygons in the collection
    pub polygons: Vec<Polygon>,
}

impl MultiPolygon {
    /// Creates a new multi-polygon
    #[must_use]
    pub const fn new(polygons: Vec<Polygon>) -> Self {
        Self { polygons }
    }

    /// Creates an empty multi-polygon
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            polygons: Vec::new(),
        }
    }

    /// Adds a polygon
    pub fn push(&mut self, polygon: Polygon) {
        self.polygons.push(polygon);
    }

    /// Computes the bounding box
    #[must_use]
    pub fn bounds(&self) -> Option<(f64, f64, f64, f64)> {
        if self.polygons.is_empty() {
            return None;
        }

        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;

        for poly in &self.polygons {
            if let Some((x_min, y_min, x_max, y_max)) = poly.bounds() {
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

/// `GeometryCollection`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeometryCollection {
    /// Geometries in the collection
    pub geometries: Vec<Geometry>,
}

impl GeometryCollection {
    /// Creates a new geometry collection
    #[must_use]
    pub const fn new(geometries: Vec<Geometry>) -> Self {
        Self { geometries }
    }

    /// Creates an empty geometry collection
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            geometries: Vec::new(),
        }
    }

    /// Adds a geometry
    pub fn push(&mut self, geometry: Geometry) {
        self.geometries.push(geometry);
    }

    /// Computes the bounding box
    #[must_use]
    pub fn bounds(&self) -> Option<(f64, f64, f64, f64)> {
        if self.geometries.is_empty() {
            return None;
        }

        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;

        for geom in &self.geometries {
            if let Some((x_min, y_min, x_max, y_max)) = geom.bounds() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coordinate_2d() {
        let coord = Coordinate::new_2d(1.0, 2.0);
        assert_eq!(coord.x, 1.0);
        assert_eq!(coord.y, 2.0);
        assert!(!coord.has_z());
        assert!(!coord.has_m());
        assert_eq!(coord.dimensions(), 2);
    }

    #[test]
    fn test_coordinate_3d() {
        let coord = Coordinate::new_3d(1.0, 2.0, 3.0);
        assert_eq!(coord.x, 1.0);
        assert_eq!(coord.y, 2.0);
        assert_eq!(coord.z, Some(3.0));
        assert!(coord.has_z());
        assert!(!coord.has_m());
        assert_eq!(coord.dimensions(), 3);
    }

    #[test]
    fn test_point() {
        let point = Point::new(1.0, 2.0);
        assert_eq!(point.coord.x, 1.0);
        assert_eq!(point.coord.y, 2.0);
    }

    #[test]
    fn test_linestring() {
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(2.0, 0.0),
        ];
        let ls = LineString::new(coords).ok();
        assert!(ls.is_some());
        let ls = ls.expect("linestring creation failed");
        assert_eq!(ls.len(), 3);
        assert!(!ls.is_empty());

        let bounds = ls.bounds();
        assert!(bounds.is_some());
        let (min_x, min_y, max_x, max_y) = bounds.expect("bounds calculation failed");
        assert_eq!(min_x, 0.0);
        assert_eq!(min_y, 0.0);
        assert_eq!(max_x, 2.0);
        assert_eq!(max_y, 1.0);
    }

    #[test]
    fn test_linestring_invalid() {
        let coords = vec![Coordinate::new_2d(0.0, 0.0)];
        let result = LineString::new(coords);
        assert!(result.is_err());
    }

    #[test]
    fn test_polygon() {
        let exterior_coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 0.0),
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(0.0, 1.0),
            Coordinate::new_2d(0.0, 0.0),
        ];
        let exterior = LineString::new(exterior_coords).ok();
        assert!(exterior.is_some());
        let exterior = exterior.expect("linestring creation failed");

        let poly = Polygon::new(exterior, vec![]);
        assert!(poly.is_ok());
    }

    #[test]
    fn test_polygon_not_closed() {
        let exterior_coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 0.0),
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(0.0, 1.0),
        ];
        let exterior = LineString::new(exterior_coords).ok();
        assert!(exterior.is_some());
        let exterior = exterior.expect("linestring creation failed");

        let result = Polygon::new(exterior, vec![]);
        assert!(result.is_err());
    }
}
