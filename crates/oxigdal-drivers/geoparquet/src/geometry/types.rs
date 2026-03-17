//! Geometry types for GeoParquet
//!
//! This module defines in-memory representations of all OGC Simple Features
//! geometry types supported by GeoParquet.

#[cfg(not(feature = "std"))]
use alloc::vec;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use oxigdal_core::types::GeometryType as CoreGeometryType;

/// A 2D or 3D coordinate
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coordinate {
    /// X coordinate (longitude)
    pub x: f64,
    /// Y coordinate (latitude)
    pub y: f64,
    /// Z coordinate (elevation), optional
    pub z: Option<f64>,
    /// M coordinate (measure), optional
    pub m: Option<f64>,
}

impl Coordinate {
    /// Creates a new 2D coordinate
    pub const fn new_2d(x: f64, y: f64) -> Self {
        Self {
            x,
            y,
            z: None,
            m: None,
        }
    }

    /// Creates a new 3D coordinate
    pub const fn new_3d(x: f64, y: f64, z: f64) -> Self {
        Self {
            x,
            y,
            z: Some(z),
            m: None,
        }
    }

    /// Creates a new 2D coordinate with measure
    pub const fn new_2dm(x: f64, y: f64, m: f64) -> Self {
        Self {
            x,
            y,
            z: None,
            m: Some(m),
        }
    }

    /// Creates a new 3D coordinate with measure
    pub const fn new_3dm(x: f64, y: f64, z: f64, m: f64) -> Self {
        Self {
            x,
            y,
            z: Some(z),
            m: Some(m),
        }
    }

    /// Returns true if this coordinate has Z
    pub const fn has_z(&self) -> bool {
        self.z.is_some()
    }

    /// Returns true if this coordinate has M
    pub const fn has_m(&self) -> bool {
        self.m.is_some()
    }

    /// Returns the dimensionality (2, 3, or 4)
    pub const fn dimension(&self) -> u32 {
        2 + if self.has_z() { 1 } else { 0 } + if self.has_m() { 1 } else { 0 }
    }
}

/// Point geometry
#[derive(Debug, Clone, PartialEq)]
pub struct Point {
    /// The coordinate
    pub coord: Coordinate,
}

impl Point {
    /// Creates a new point
    pub const fn new(coord: Coordinate) -> Self {
        Self { coord }
    }

    /// Creates a new 2D point
    pub const fn new_2d(x: f64, y: f64) -> Self {
        Self {
            coord: Coordinate::new_2d(x, y),
        }
    }

    /// Creates a new 3D point
    pub const fn new_3d(x: f64, y: f64, z: f64) -> Self {
        Self {
            coord: Coordinate::new_3d(x, y, z),
        }
    }
}

/// LineString geometry
#[derive(Debug, Clone, PartialEq)]
pub struct LineString {
    /// The coordinates
    pub coords: Vec<Coordinate>,
}

impl LineString {
    /// Creates a new line string
    pub fn new(coords: Vec<Coordinate>) -> Self {
        Self { coords }
    }

    /// Returns true if this linestring is empty
    pub fn is_empty(&self) -> bool {
        self.coords.is_empty()
    }

    /// Returns the number of coordinates
    pub fn len(&self) -> usize {
        self.coords.len()
    }

    /// Returns true if this is a closed ring
    pub fn is_closed(&self) -> bool {
        !self.coords.is_empty() && self.coords.first() == self.coords.last()
    }
}

/// Polygon geometry
#[derive(Debug, Clone, PartialEq)]
pub struct Polygon {
    /// Exterior ring
    pub exterior: LineString,
    /// Interior rings (holes)
    pub interiors: Vec<LineString>,
}

impl Polygon {
    /// Creates a new polygon
    pub fn new(exterior: LineString, interiors: Vec<LineString>) -> Self {
        Self {
            exterior,
            interiors,
        }
    }

    /// Creates a new polygon with no holes
    pub fn new_simple(exterior: LineString) -> Self {
        Self {
            exterior,
            interiors: Vec::new(),
        }
    }

    /// Returns the number of interior rings
    pub fn num_interiors(&self) -> usize {
        self.interiors.len()
    }
}

/// MultiPoint geometry
#[derive(Debug, Clone, PartialEq)]
pub struct MultiPoint {
    /// The points
    pub points: Vec<Point>,
}

impl MultiPoint {
    /// Creates a new multi-point
    pub fn new(points: Vec<Point>) -> Self {
        Self { points }
    }

    /// Returns true if this multi-point is empty
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// Returns the number of points
    pub fn len(&self) -> usize {
        self.points.len()
    }
}

/// MultiLineString geometry
#[derive(Debug, Clone, PartialEq)]
pub struct MultiLineString {
    /// The line strings
    pub linestrings: Vec<LineString>,
}

impl MultiLineString {
    /// Creates a new multi-linestring
    pub fn new(linestrings: Vec<LineString>) -> Self {
        Self { linestrings }
    }

    /// Returns true if this multi-linestring is empty
    pub fn is_empty(&self) -> bool {
        self.linestrings.is_empty()
    }

    /// Returns the number of linestrings
    pub fn len(&self) -> usize {
        self.linestrings.len()
    }
}

/// MultiPolygon geometry
#[derive(Debug, Clone, PartialEq)]
pub struct MultiPolygon {
    /// The polygons
    pub polygons: Vec<Polygon>,
}

impl MultiPolygon {
    /// Creates a new multi-polygon
    pub fn new(polygons: Vec<Polygon>) -> Self {
        Self { polygons }
    }

    /// Returns true if this multi-polygon is empty
    pub fn is_empty(&self) -> bool {
        self.polygons.is_empty()
    }

    /// Returns the number of polygons
    pub fn len(&self) -> usize {
        self.polygons.len()
    }
}

/// GeometryCollection
#[derive(Debug, Clone, PartialEq)]
pub struct GeometryCollection {
    /// The geometries
    pub geometries: Vec<Geometry>,
}

impl GeometryCollection {
    /// Creates a new geometry collection
    pub fn new(geometries: Vec<Geometry>) -> Self {
        Self { geometries }
    }

    /// Returns true if this collection is empty
    pub fn is_empty(&self) -> bool {
        self.geometries.is_empty()
    }

    /// Returns the number of geometries
    pub fn len(&self) -> usize {
        self.geometries.len()
    }
}

/// Enum of all geometry types
#[derive(Debug, Clone, PartialEq)]
pub enum Geometry {
    /// Point
    Point(Point),
    /// LineString
    LineString(LineString),
    /// Polygon
    Polygon(Polygon),
    /// MultiPoint
    MultiPoint(MultiPoint),
    /// MultiLineString
    MultiLineString(MultiLineString),
    /// MultiPolygon
    MultiPolygon(MultiPolygon),
    /// GeometryCollection
    GeometryCollection(GeometryCollection),
}

impl Geometry {
    /// Returns the geometry type
    pub fn geometry_type(&self) -> GeometryType {
        match self {
            Self::Point(_) => GeometryType::Point,
            Self::LineString(_) => GeometryType::LineString,
            Self::Polygon(_) => GeometryType::Polygon,
            Self::MultiPoint(_) => GeometryType::MultiPoint,
            Self::MultiLineString(_) => GeometryType::MultiLineString,
            Self::MultiPolygon(_) => GeometryType::MultiPolygon,
            Self::GeometryCollection(_) => GeometryType::GeometryCollection,
        }
    }

    /// Returns the geometry type as a string
    pub fn type_name(&self) -> &'static str {
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

    /// Computes the bounding box of this geometry
    pub fn bbox(&self) -> Option<Vec<f64>> {
        match self {
            Self::Point(p) => Some(vec![p.coord.x, p.coord.y, p.coord.x, p.coord.y]),
            Self::LineString(ls) => Self::compute_coords_bbox(&ls.coords),
            Self::Polygon(poly) => Self::compute_coords_bbox(&poly.exterior.coords),
            Self::MultiPoint(mp) => {
                let coords: Vec<Coordinate> = mp.points.iter().map(|p| p.coord).collect();
                Self::compute_coords_bbox(&coords)
            }
            Self::MultiLineString(mls) => {
                let coords: Vec<Coordinate> = mls
                    .linestrings
                    .iter()
                    .flat_map(|ls| &ls.coords)
                    .copied()
                    .collect();
                Self::compute_coords_bbox(&coords)
            }
            Self::MultiPolygon(mpoly) => {
                let coords: Vec<Coordinate> = mpoly
                    .polygons
                    .iter()
                    .flat_map(|p| &p.exterior.coords)
                    .copied()
                    .collect();
                Self::compute_coords_bbox(&coords)
            }
            Self::GeometryCollection(gc) => {
                let mut min_x = f64::INFINITY;
                let mut min_y = f64::INFINITY;
                let mut max_x = f64::NEG_INFINITY;
                let mut max_y = f64::NEG_INFINITY;

                for geom in &gc.geometries {
                    if let Some(bbox) = geom.bbox() {
                        min_x = min_x.min(bbox[0]);
                        min_y = min_y.min(bbox[1]);
                        max_x = max_x.max(bbox[2]);
                        max_y = max_y.max(bbox[3]);
                    }
                }

                if min_x.is_finite() {
                    Some(vec![min_x, min_y, max_x, max_y])
                } else {
                    None
                }
            }
        }
    }

    /// Helper to compute bbox from coordinates
    fn compute_coords_bbox(coords: &[Coordinate]) -> Option<Vec<f64>> {
        if coords.is_empty() {
            return None;
        }

        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;

        for coord in coords {
            min_x = min_x.min(coord.x);
            min_y = min_y.min(coord.y);
            max_x = max_x.max(coord.x);
            max_y = max_y.max(coord.y);
        }

        Some(vec![min_x, min_y, max_x, max_y])
    }
}

/// Geometry type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum GeometryType {
    /// Point
    Point = 1,
    /// LineString
    LineString = 2,
    /// Polygon
    Polygon = 3,
    /// MultiPoint
    MultiPoint = 4,
    /// MultiLineString
    MultiLineString = 5,
    /// MultiPolygon
    MultiPolygon = 6,
    /// GeometryCollection
    GeometryCollection = 7,
}

impl GeometryType {
    /// Converts from WKB type code
    pub fn from_wkb_code(code: u32) -> Option<Self> {
        // Handle Z, M, ZM variants
        // WKB codes: base (1-7), Z (1001-1007), M (2001-2007), ZM (3001-3007)
        let base_code = code % 1000;
        match base_code {
            1 => Some(Self::Point),
            2 => Some(Self::LineString),
            3 => Some(Self::Polygon),
            4 => Some(Self::MultiPoint),
            5 => Some(Self::MultiLineString),
            6 => Some(Self::MultiPolygon),
            7 => Some(Self::GeometryCollection),
            _ => None,
        }
    }

    /// Converts to WKB type code
    pub fn to_wkb_code(self, has_z: bool, has_m: bool) -> u32 {
        let base = self as u32;
        if has_z && has_m {
            base + 3000
        } else if has_m {
            base + 2000
        } else if has_z {
            base + 1000
        } else {
            base
        }
    }

    /// Returns the type name
    pub fn name(self) -> &'static str {
        match self {
            Self::Point => "Point",
            Self::LineString => "LineString",
            Self::Polygon => "Polygon",
            Self::MultiPoint => "MultiPoint",
            Self::MultiLineString => "MultiLineString",
            Self::MultiPolygon => "MultiPolygon",
            Self::GeometryCollection => "GeometryCollection",
        }
    }

    /// Converts to oxigdal-core GeometryType
    pub fn to_core(self) -> CoreGeometryType {
        match self {
            Self::Point => CoreGeometryType::Point,
            Self::LineString => CoreGeometryType::LineString,
            Self::Polygon => CoreGeometryType::Polygon,
            Self::MultiPoint => CoreGeometryType::MultiPoint,
            Self::MultiLineString => CoreGeometryType::MultiLineString,
            Self::MultiPolygon => CoreGeometryType::MultiPolygon,
            Self::GeometryCollection => CoreGeometryType::GeometryCollection,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coordinate_creation() {
        let coord = Coordinate::new_2d(1.0, 2.0);
        assert_eq!(coord.x, 1.0);
        assert_eq!(coord.y, 2.0);
        assert!(!coord.has_z());
        assert!(!coord.has_m());
        assert_eq!(coord.dimension(), 2);

        let coord_3d = Coordinate::new_3d(1.0, 2.0, 3.0);
        assert!(coord_3d.has_z());
        assert_eq!(coord_3d.dimension(), 3);
    }

    #[test]
    fn test_point() {
        let point = Point::new_2d(1.0, 2.0);
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
        let ls = LineString::new(coords);
        assert_eq!(ls.len(), 3);
        assert!(!ls.is_empty());
        assert!(!ls.is_closed());
    }

    #[test]
    fn test_polygon() {
        let exterior = LineString::new(vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 0.0),
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(0.0, 1.0),
            Coordinate::new_2d(0.0, 0.0),
        ]);
        let poly = Polygon::new_simple(exterior);
        assert_eq!(poly.num_interiors(), 0);
    }

    #[test]
    fn test_geometry_type_conversion() {
        let point = Geometry::Point(Point::new_2d(0.0, 0.0));
        assert_eq!(point.geometry_type(), GeometryType::Point);
        assert_eq!(point.type_name(), "Point");
    }

    #[test]
    fn test_wkb_code_conversion() {
        assert_eq!(GeometryType::Point.to_wkb_code(false, false), 1);
        assert_eq!(GeometryType::Point.to_wkb_code(true, false), 1001);
        assert_eq!(GeometryType::Point.to_wkb_code(false, true), 2001);
        assert_eq!(GeometryType::Point.to_wkb_code(true, true), 3001);

        assert_eq!(GeometryType::from_wkb_code(1), Some(GeometryType::Point));
        assert_eq!(GeometryType::from_wkb_code(1001), Some(GeometryType::Point));
    }

    #[test]
    fn test_geometry_bbox() {
        let point = Geometry::Point(Point::new_2d(1.0, 2.0));
        let bbox = point.bbox();
        assert_eq!(bbox, Some(vec![1.0, 2.0, 1.0, 2.0]));

        let coords = vec![Coordinate::new_2d(0.0, 0.0), Coordinate::new_2d(5.0, 5.0)];
        let ls = Geometry::LineString(LineString::new(coords));
        let bbox = ls.bbox();
        assert_eq!(bbox, Some(vec![0.0, 0.0, 5.0, 5.0]));
    }
}
