//! Geometry types for vector data
//!
//! This module provides geometry types compatible with Simple Features specification.

use crate::error::{OxiGeoError, Result};
use core::fmt;
use serde::{Deserialize, Serialize};

#[cfg(feature = "std")]
use std::vec::Vec;

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{string::ToString, vec::Vec};

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
    /// Returns true if any coordinate in this geometry has a Z value.
    #[must_use]
    pub fn has_z(&self) -> bool {
        match self {
            Self::Point(p) => p.coord.has_z(),
            Self::LineString(ls) => ls.coords.iter().any(|c| c.has_z()),
            Self::Polygon(poly) => {
                poly.exterior.coords.iter().any(|c| c.has_z())
                    || poly
                        .interiors
                        .iter()
                        .any(|r| r.coords.iter().any(|c| c.has_z()))
            }
            Self::MultiPoint(mp) => mp.points.iter().any(|p| p.coord.has_z()),
            Self::MultiLineString(mls) => mls
                .line_strings
                .iter()
                .any(|ls| ls.coords.iter().any(|c| c.has_z())),
            Self::MultiPolygon(mpoly) => mpoly.polygons.iter().any(|poly| {
                poly.exterior.coords.iter().any(|c| c.has_z())
                    || poly
                        .interiors
                        .iter()
                        .any(|r| r.coords.iter().any(|c| c.has_z()))
            }),
            Self::GeometryCollection(gc) => gc.geometries.iter().any(|g| g.has_z()),
        }
    }

    /// Returns true if any coordinate in this geometry has an M (measure) value.
    #[must_use]
    pub fn has_m(&self) -> bool {
        match self {
            Self::Point(p) => p.coord.has_m(),
            Self::LineString(ls) => ls.coords.iter().any(|c| c.has_m()),
            Self::Polygon(poly) => {
                poly.exterior.coords.iter().any(|c| c.has_m())
                    || poly
                        .interiors
                        .iter()
                        .any(|r| r.coords.iter().any(|c| c.has_m()))
            }
            Self::MultiPoint(mp) => mp.points.iter().any(|p| p.coord.has_m()),
            Self::MultiLineString(mls) => mls
                .line_strings
                .iter()
                .any(|ls| ls.coords.iter().any(|c| c.has_m())),
            Self::MultiPolygon(mpoly) => mpoly.polygons.iter().any(|poly| {
                poly.exterior.coords.iter().any(|c| c.has_m())
                    || poly
                        .interiors
                        .iter()
                        .any(|r| r.coords.iter().any(|c| c.has_m()))
            }),
            Self::GeometryCollection(gc) => gc.geometries.iter().any(|g| g.has_m()),
        }
    }

    /// Collects all Z values from the geometry's coordinates in order.
    ///
    /// Returns `None` if this geometry has no Z coordinates.
    /// For coordinates without Z, substitutes `0.0`.
    #[must_use]
    pub fn z_values(&self) -> Option<Vec<f64>> {
        if !self.has_z() {
            return None;
        }
        let mut out = Vec::new();
        Self::collect_z_from(self, &mut out);
        Some(out)
    }

    /// Collects all M values from the geometry's coordinates in order.
    ///
    /// Returns `None` if this geometry has no M coordinates.
    /// For coordinates without M, substitutes `0.0`.
    #[must_use]
    pub fn m_values(&self) -> Option<Vec<f64>> {
        if !self.has_m() {
            return None;
        }
        let mut out = Vec::new();
        Self::collect_m_from(self, &mut out);
        Some(out)
    }

    fn collect_z_from(geom: &Geometry, out: &mut Vec<f64>) {
        match geom {
            Self::Point(p) => out.push(p.coord.z.unwrap_or(0.0)),
            Self::LineString(ls) => {
                for c in &ls.coords {
                    out.push(c.z.unwrap_or(0.0));
                }
            }
            Self::Polygon(poly) => {
                for c in &poly.exterior.coords {
                    out.push(c.z.unwrap_or(0.0));
                }
                for ring in &poly.interiors {
                    for c in &ring.coords {
                        out.push(c.z.unwrap_or(0.0));
                    }
                }
            }
            Self::MultiPoint(mp) => {
                for p in &mp.points {
                    out.push(p.coord.z.unwrap_or(0.0));
                }
            }
            Self::MultiLineString(mls) => {
                for ls in &mls.line_strings {
                    for c in &ls.coords {
                        out.push(c.z.unwrap_or(0.0));
                    }
                }
            }
            Self::MultiPolygon(mpoly) => {
                for poly in &mpoly.polygons {
                    for c in &poly.exterior.coords {
                        out.push(c.z.unwrap_or(0.0));
                    }
                    for ring in &poly.interiors {
                        for c in &ring.coords {
                            out.push(c.z.unwrap_or(0.0));
                        }
                    }
                }
            }
            Self::GeometryCollection(gc) => {
                for g in &gc.geometries {
                    Self::collect_z_from(g, out);
                }
            }
        }
    }

    fn collect_m_from(geom: &Geometry, out: &mut Vec<f64>) {
        match geom {
            Self::Point(p) => out.push(p.coord.m.unwrap_or(0.0)),
            Self::LineString(ls) => {
                for c in &ls.coords {
                    out.push(c.m.unwrap_or(0.0));
                }
            }
            Self::Polygon(poly) => {
                for c in &poly.exterior.coords {
                    out.push(c.m.unwrap_or(0.0));
                }
                for ring in &poly.interiors {
                    for c in &ring.coords {
                        out.push(c.m.unwrap_or(0.0));
                    }
                }
            }
            Self::MultiPoint(mp) => {
                for p in &mp.points {
                    out.push(p.coord.m.unwrap_or(0.0));
                }
            }
            Self::MultiLineString(mls) => {
                for ls in &mls.line_strings {
                    for c in &ls.coords {
                        out.push(c.m.unwrap_or(0.0));
                    }
                }
            }
            Self::MultiPolygon(mpoly) => {
                for poly in &mpoly.polygons {
                    for c in &poly.exterior.coords {
                        out.push(c.m.unwrap_or(0.0));
                    }
                    for ring in &poly.interiors {
                        for c in &ring.coords {
                            out.push(c.m.unwrap_or(0.0));
                        }
                    }
                }
            }
            Self::GeometryCollection(gc) => {
                for g in &gc.geometries {
                    Self::collect_m_from(g, out);
                }
            }
        }
    }

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
            return Err(OxiGeoError::InvalidParameter {
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
            return Err(OxiGeoError::InvalidParameter {
                parameter: "exterior",
                message: "Polygon exterior ring must have at least 4 coordinates".to_string(),
            });
        }

        // Check if ring is closed
        let first = &exterior.coords[0];
        let last = &exterior.coords[exterior.coords.len() - 1];
        if (first.x - last.x).abs() > f64::EPSILON || (first.y - last.y).abs() > f64::EPSILON {
            return Err(OxiGeoError::InvalidParameter {
                parameter: "exterior",
                message: "Polygon exterior ring must be closed".to_string(),
            });
        }

        // Validate interior rings
        for interior in &interiors {
            if interior.coords.len() < 4 {
                return Err(OxiGeoError::InvalidParameter {
                    parameter: "interiors",
                    message: "Polygon interior ring must have at least 4 coordinates".to_string(),
                });
            }

            let first = &interior.coords[0];
            let last = &interior.coords[interior.coords.len() - 1];
            if (first.x - last.x).abs() > f64::EPSILON || (first.y - last.y).abs() > f64::EPSILON {
                return Err(OxiGeoError::InvalidParameter {
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

// ─── Display (WKT-style) ──────────────────────────────────────────────────────

impl fmt::Display for Coordinate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.x, self.y)?;
        if let Some(z) = self.z {
            write!(f, " {z}")?;
        }
        Ok(())
    }
}

impl fmt::Display for Point {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "POINT({})", self.coord)
    }
}

/// Writes a ring's coordinate list inside parentheses: `(x1 y1, x2 y2, ...)`.
fn fmt_ring(f: &mut fmt::Formatter<'_>, ring: &LineString) -> fmt::Result {
    write!(f, "(")?;
    for (i, c) in ring.coords.iter().enumerate() {
        if i > 0 {
            write!(f, ", ")?;
        }
        write!(f, "{c}")?;
    }
    write!(f, ")")
}

impl fmt::Display for LineString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LINESTRING")?;
        fmt_ring(f, self)
    }
}

impl fmt::Display for Polygon {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "POLYGON(")?;
        fmt_ring(f, &self.exterior)?;
        for interior in &self.interiors {
            write!(f, ", ")?;
            fmt_ring(f, interior)?;
        }
        write!(f, ")")
    }
}

impl fmt::Display for MultiPoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "MULTIPOINT(")?;
        for (i, p) in self.points.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "({} {})", p.coord.x, p.coord.y)?;
        }
        write!(f, ")")
    }
}

impl fmt::Display for MultiLineString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "MULTILINESTRING(")?;
        for (i, ls) in self.line_strings.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            fmt_ring(f, ls)?;
        }
        write!(f, ")")
    }
}

impl fmt::Display for MultiPolygon {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "MULTIPOLYGON(")?;
        for (i, poly) in self.polygons.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "(")?;
            fmt_ring(f, &poly.exterior)?;
            for interior in &poly.interiors {
                write!(f, ", ")?;
                fmt_ring(f, interior)?;
            }
            write!(f, ")")?;
        }
        write!(f, ")")
    }
}

impl fmt::Display for Geometry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Point(p) => write!(f, "{p}"),
            Self::LineString(ls) => write!(f, "{ls}"),
            Self::Polygon(poly) => write!(f, "{poly}"),
            Self::MultiPoint(mp) => write!(f, "{mp}"),
            Self::MultiLineString(mls) => write!(f, "{mls}"),
            Self::MultiPolygon(mpoly) => write!(f, "{mpoly}"),
            Self::GeometryCollection(gc) => {
                write!(f, "GEOMETRYCOLLECTION(")?;
                for (i, g) in gc.geometries.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{g}")?;
                }
                write!(f, ")")
            }
        }
    }
}

// ─── OGC WKB serialization ────────────────────────────────────────────────────

#[cfg(feature = "std")]
mod wkb {
    //! WKB (Well-Known Binary) serialization helpers.
    //!
    //! Uses little-endian byte order throughout (byte-order marker `0x01`).

    use std::io::Write;

    /// WKB geometry type codes (ISO 19125 / OGC 06-103r4 / SFSQL 1.2)
    pub(crate) const WKB_POINT: u32 = 1;
    pub(crate) const WKB_LINESTRING: u32 = 2;
    pub(crate) const WKB_POLYGON: u32 = 3;
    pub(crate) const WKB_MULTIPOINT: u32 = 4;
    pub(crate) const WKB_MULTILINESTRING: u32 = 5;
    pub(crate) const WKB_MULTIPOLYGON: u32 = 6;
    pub(crate) const WKB_GEOMETRYCOLLECTION: u32 = 7;

    /// ISO type code offset for geometries with Z coordinate.
    pub(crate) const WKB_Z_OFFSET: u32 = 1000;

    /// Writes the 5-byte WKB header: byte-order marker + type code.
    pub(crate) fn write_header<W: Write>(w: &mut W, wkb_type: u32) -> std::io::Result<()> {
        w.write_all(&[0x01])?; // little-endian
        w.write_all(&wkb_type.to_le_bytes())
    }

    /// Writes a single `f64` value as 8 little-endian bytes.
    pub(crate) fn write_f64<W: Write>(w: &mut W, v: f64) -> std::io::Result<()> {
        w.write_all(&v.to_le_bytes())
    }

    /// Writes a single `u32` value as 4 little-endian bytes.
    pub(crate) fn write_u32<W: Write>(w: &mut W, v: u32) -> std::io::Result<()> {
        w.write_all(&v.to_le_bytes())
    }
}

#[cfg(feature = "std")]
impl Geometry {
    /// Serializes this geometry to OGC WKB bytes (little-endian).
    ///
    /// The returned `Vec<u8>` contains a complete, self-describing WKB blob that
    /// can be consumed by any OGC-compliant geospatial library.
    ///
    /// # Panics
    ///
    /// This method cannot panic in practice: writing to a `Vec<u8>` is infallible.
    #[must_use]
    pub fn to_wkb(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        // Writing to Vec<u8> via std::io::Write is infallible.
        if self.write_wkb(&mut buf).is_err() {
            // SAFETY: Vec<u8> io::Write impl never returns Err.
            unreachable!("Vec<u8> writes are infallible");
        }
        buf
    }

    /// Writes this geometry as OGC WKB (little-endian) to the given writer.
    ///
    /// # Errors
    ///
    /// Returns an `std::io::Error` only if the underlying writer fails.
    /// Writing to an in-memory `Vec<u8>` will never produce an error.
    pub fn write_wkb<W: std::io::Write>(&self, w: &mut W) -> std::io::Result<()> {
        match self {
            Self::Point(p) => write_point_wkb(w, p),
            Self::LineString(ls) => write_linestring_wkb(w, ls),
            Self::Polygon(poly) => write_polygon_wkb(w, poly),
            Self::MultiPoint(mp) => write_multipoint_wkb(w, mp),
            Self::MultiLineString(mls) => write_multilinestring_wkb(w, mls),
            Self::MultiPolygon(mpoly) => write_multipolygon_wkb(w, mpoly),
            Self::GeometryCollection(gc) => write_geometrycollection_wkb(w, gc),
        }
    }
}

/// Writes a [`Point`] to a WKB writer.
///
/// Uses type code 1001 when a Z coordinate is present, otherwise 1.
#[cfg(feature = "std")]
fn write_point_wkb<W: std::io::Write>(w: &mut W, p: &Point) -> std::io::Result<()> {
    use wkb::{WKB_POINT, WKB_Z_OFFSET};
    let has_z = p.coord.z.is_some();
    let wkb_type = if has_z {
        WKB_POINT + WKB_Z_OFFSET
    } else {
        WKB_POINT
    };
    wkb::write_header(w, wkb_type)?;
    wkb::write_f64(w, p.coord.x)?;
    wkb::write_f64(w, p.coord.y)?;
    if let Some(z) = p.coord.z {
        wkb::write_f64(w, z)?;
    }
    Ok(())
}

/// Writes a [`LineString`] to a WKB writer (2D only, type code 2).
#[cfg(feature = "std")]
fn write_linestring_wkb<W: std::io::Write>(w: &mut W, ls: &LineString) -> std::io::Result<()> {
    wkb::write_header(w, wkb::WKB_LINESTRING)?;
    let count = ls.coords.len() as u32;
    wkb::write_u32(w, count)?;
    for coord in &ls.coords {
        wkb::write_f64(w, coord.x)?;
        wkb::write_f64(w, coord.y)?;
    }
    Ok(())
}

/// Writes a single linear ring (no WKB header) as count + coordinate pairs.
#[cfg(feature = "std")]
fn write_ring_wkb<W: std::io::Write>(w: &mut W, ring: &LineString) -> std::io::Result<()> {
    let count = ring.coords.len() as u32;
    wkb::write_u32(w, count)?;
    for coord in &ring.coords {
        wkb::write_f64(w, coord.x)?;
        wkb::write_f64(w, coord.y)?;
    }
    Ok(())
}

/// Writes a [`Polygon`] to a WKB writer (type code 3).
///
/// Ring count = 1 (exterior) + number of interior rings.
#[cfg(feature = "std")]
fn write_polygon_wkb<W: std::io::Write>(w: &mut W, poly: &Polygon) -> std::io::Result<()> {
    wkb::write_header(w, wkb::WKB_POLYGON)?;
    let ring_count = 1u32 + poly.interiors.len() as u32;
    wkb::write_u32(w, ring_count)?;
    write_ring_wkb(w, &poly.exterior)?;
    for interior in &poly.interiors {
        write_ring_wkb(w, interior)?;
    }
    Ok(())
}

/// Writes a [`MultiPoint`] to a WKB writer (type code 4).
///
/// Each member point is a complete WKB geometry (with its own header).
#[cfg(feature = "std")]
fn write_multipoint_wkb<W: std::io::Write>(w: &mut W, mp: &MultiPoint) -> std::io::Result<()> {
    wkb::write_header(w, wkb::WKB_MULTIPOINT)?;
    wkb::write_u32(w, mp.points.len() as u32)?;
    for point in &mp.points {
        write_point_wkb(w, point)?;
    }
    Ok(())
}

/// Writes a [`MultiLineString`] to a WKB writer (type code 5).
///
/// Each member line string is a complete WKB geometry (with its own header).
#[cfg(feature = "std")]
fn write_multilinestring_wkb<W: std::io::Write>(
    w: &mut W,
    mls: &MultiLineString,
) -> std::io::Result<()> {
    wkb::write_header(w, wkb::WKB_MULTILINESTRING)?;
    wkb::write_u32(w, mls.line_strings.len() as u32)?;
    for ls in &mls.line_strings {
        write_linestring_wkb(w, ls)?;
    }
    Ok(())
}

/// Writes a [`MultiPolygon`] to a WKB writer (type code 6).
///
/// Each member polygon is a complete WKB geometry (with its own header).
#[cfg(feature = "std")]
fn write_multipolygon_wkb<W: std::io::Write>(
    w: &mut W,
    mpoly: &MultiPolygon,
) -> std::io::Result<()> {
    wkb::write_header(w, wkb::WKB_MULTIPOLYGON)?;
    wkb::write_u32(w, mpoly.polygons.len() as u32)?;
    for poly in &mpoly.polygons {
        write_polygon_wkb(w, poly)?;
    }
    Ok(())
}

/// Writes a [`GeometryCollection`] to a WKB writer (type code 7).
///
/// Each member geometry is a complete WKB geometry (with its own header).
#[cfg(feature = "std")]
fn write_geometrycollection_wkb<W: std::io::Write>(
    w: &mut W,
    gc: &GeometryCollection,
) -> std::io::Result<()> {
    wkb::write_header(w, wkb::WKB_GEOMETRYCOLLECTION)?;
    wkb::write_u32(w, gc.geometries.len() as u32)?;
    for geom in &gc.geometries {
        geom.write_wkb(w)?;
    }
    Ok(())
}

// ─── end WKB ──────────────────────────────────────────────────────────────────

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

    // ─── WKB tests ────────────────────────────────────────────────────────────

    #[test]
    #[cfg(feature = "std")]
    fn test_wkb_point_2d() {
        // A 2D point: [01][01000000][8 bytes x][8 bytes y]
        let geom = Geometry::Point(Point::new(1.0_f64, 2.0_f64));
        let wkb = geom.to_wkb();

        assert_eq!(wkb[0], 0x01, "byte-order marker");
        assert_eq!(&wkb[1..5], &1u32.to_le_bytes(), "wkb_type must be 1");
        assert_eq!(wkb.len(), 21, "1 + 4 + 8 + 8 = 21 bytes for 2D point");

        let x = f64::from_le_bytes(wkb[5..13].try_into().expect("8 bytes"));
        let y = f64::from_le_bytes(wkb[13..21].try_into().expect("8 bytes"));
        assert_eq!(x, 1.0_f64);
        assert_eq!(y, 2.0_f64);
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_wkb_point_3d() {
        // A 3D point: [01][E9030000 = 1001 LE][8 bytes x][8 bytes y][8 bytes z]
        let geom = Geometry::Point(Point::new_3d(1.0_f64, 2.0_f64, 3.0_f64));
        let wkb = geom.to_wkb();

        assert_eq!(wkb[0], 0x01);
        assert_eq!(&wkb[1..5], &1001u32.to_le_bytes(), "wkb_type must be 1001");
        assert_eq!(wkb.len(), 29, "1 + 4 + 8 + 8 + 8 = 29 bytes for 3D point");

        let z = f64::from_le_bytes(wkb[21..29].try_into().expect("8 bytes"));
        assert_eq!(z, 3.0_f64);
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_wkb_write_equals_to_wkb() {
        let geom = Geometry::Point(Point::new(5.0_f64, 10.0_f64));
        let a = geom.to_wkb();
        let mut b = Vec::new();
        geom.write_wkb(&mut b).expect("Vec<u8> write cannot fail");
        assert_eq!(a, b);
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_wkb_linestring() {
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(2.0, 0.0),
        ];
        let ls = LineString::new(coords).expect("valid linestring");
        let geom = Geometry::LineString(ls);
        let wkb = geom.to_wkb();

        // Header: 1 byte marker + 4 byte type + 4 byte count + n * 16 bytes (2 f64)
        // = 1 + 4 + 4 + 3*16 = 57 bytes
        assert_eq!(wkb[0], 0x01);
        assert_eq!(&wkb[1..5], &2u32.to_le_bytes(), "wkb_type must be 2");

        let count = u32::from_le_bytes(wkb[5..9].try_into().expect("4 bytes"));
        assert_eq!(count, 3, "three coordinates");
        assert_eq!(wkb.len(), 1 + 4 + 4 + 3 * 16, "total length");
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_wkb_polygon_simple() {
        let exterior_coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 0.0),
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(0.0, 1.0),
            Coordinate::new_2d(0.0, 0.0),
        ];
        let exterior = LineString::new(exterior_coords).expect("valid exterior");
        let poly = Polygon::new(exterior, vec![]).expect("valid polygon");
        let geom = Geometry::Polygon(poly);
        let wkb = geom.to_wkb();

        assert_eq!(wkb[0], 0x01);
        assert_eq!(&wkb[1..5], &3u32.to_le_bytes(), "wkb_type must be 3");

        let ring_count = u32::from_le_bytes(wkb[5..9].try_into().expect("4 bytes"));
        assert_eq!(ring_count, 1, "one exterior ring");
        // ring: 4 byte point_count + 5 * 16 byte coordinates
        let point_count = u32::from_le_bytes(wkb[9..13].try_into().expect("4 bytes"));
        assert_eq!(point_count, 5);
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_wkb_geometry_collection() {
        let gc = GeometryCollection::new(vec![
            Geometry::Point(Point::new(0.0, 0.0)),
            Geometry::Point(Point::new(1.0, 1.0)),
        ]);
        let geom = Geometry::GeometryCollection(gc);
        let wkb = geom.to_wkb();

        assert_eq!(wkb[0], 0x01);
        assert_eq!(&wkb[1..5], &7u32.to_le_bytes(), "wkb_type must be 7");

        let member_count = u32::from_le_bytes(wkb[5..9].try_into().expect("4 bytes"));
        assert_eq!(member_count, 2, "two member geometries");
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_wkb_multipoint() {
        let mp = MultiPoint::new(vec![Point::new(0.0, 0.0), Point::new(3.0, 4.0)]);
        let geom = Geometry::MultiPoint(mp);
        let wkb = geom.to_wkb();

        assert_eq!(&wkb[1..5], &4u32.to_le_bytes(), "wkb_type must be 4");
        let count = u32::from_le_bytes(wkb[5..9].try_into().expect("4 bytes"));
        assert_eq!(count, 2);
    }

    #[test]
    fn test_display_geometry_point_wkt() {
        let p = Geometry::Point(Point::new(1.0, 2.0));
        assert_eq!(p.to_string(), "POINT(1 2)");
    }

    #[test]
    fn test_display_geometry_point_3d() {
        let p = Geometry::Point(Point::new_3d(1.0, 2.0, 3.0));
        assert_eq!(p.to_string(), "POINT(1 2 3)");
    }

    #[test]
    fn test_display_linestring() {
        let coords = vec![Coordinate::new_2d(0.0, 0.0), Coordinate::new_2d(1.0, 1.0)];
        let ls = Geometry::LineString(LineString::new(coords).expect("valid"));
        assert_eq!(ls.to_string(), "LINESTRING(0 0, 1 1)");
    }

    #[test]
    fn test_display_geometry_collection() {
        let gc = Geometry::GeometryCollection(GeometryCollection::new(vec![
            Geometry::Point(Point::new(0.0, 0.0)),
            Geometry::Point(Point::new(1.0, 1.0)),
        ]));
        assert_eq!(gc.to_string(), "GEOMETRYCOLLECTION(POINT(0 0), POINT(1 1))");
    }
}
