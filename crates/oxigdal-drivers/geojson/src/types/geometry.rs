//! GeoJSON Geometry Types
//!
//! This module implements all geometry types defined in RFC 7946:
//! - Point
//! - LineString
//! - Polygon
//! - MultiPoint
//! - MultiLineString
//! - MultiPolygon
//! - GeometryCollection
//!
//! All types support serialization/deserialization and validation.

use serde::{Deserialize, Serialize};

use crate::error::{GeoJsonError, Result};
use crate::types::BBox;

/// A position in geographic space (longitude, latitude, \[elevation\])
///
/// According to RFC 7946, the first two elements are longitude and latitude
/// (in that order), and the third (optional) element is elevation.
pub type Position = Vec<f64>;

/// A coordinate (alias for Position)
pub type Coordinate = Position;

/// A sequence of coordinates
pub type CoordinateSequence = Vec<Position>;

/// GeoJSON geometry type discriminator
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GeometryType {
    /// Point geometry
    Point,
    /// LineString geometry
    LineString,
    /// Polygon geometry
    Polygon,
    /// MultiPoint geometry
    MultiPoint,
    /// MultiLineString geometry
    MultiLineString,
    /// MultiPolygon geometry
    MultiPolygon,
    /// GeometryCollection
    GeometryCollection,
}

impl GeometryType {
    /// Returns true if this is a multi-geometry type
    #[must_use]
    pub const fn is_multi(&self) -> bool {
        matches!(
            self,
            Self::MultiPoint
                | Self::MultiLineString
                | Self::MultiPolygon
                | Self::GeometryCollection
        )
    }

    /// Returns the dimensionality of coordinates (0=point, 1=line, 2=polygon)
    #[must_use]
    pub const fn coordinate_dimension(&self) -> u8 {
        match self {
            Self::Point | Self::MultiPoint => 0,
            Self::LineString | Self::MultiLineString => 1,
            Self::Polygon | Self::MultiPolygon => 2,
            Self::GeometryCollection => 0,
        }
    }

    /// Returns the string representation for GeoJSON
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
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

    /// Parses a geometry type from a string
    pub fn from_str(s: &str) -> Result<Self> {
        match s {
            "Point" => Ok(Self::Point),
            "LineString" => Ok(Self::LineString),
            "Polygon" => Ok(Self::Polygon),
            "MultiPoint" => Ok(Self::MultiPoint),
            "MultiLineString" => Ok(Self::MultiLineString),
            "MultiPolygon" => Ok(Self::MultiPolygon),
            "GeometryCollection" => Ok(Self::GeometryCollection),
            _ => Err(GeoJsonError::InvalidGeometryType {
                geometry_type: s.to_string(),
            }),
        }
    }
}

/// GeoJSON Geometry
///
/// This is the main enum for all geometry types defined in RFC 7946.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type")]
pub enum Geometry {
    /// Point geometry
    Point(Point),
    /// LineString geometry
    LineString(LineString),
    /// Polygon geometry
    Polygon(Polygon),
    /// MultiPoint geometry
    MultiPoint(MultiPoint),
    /// MultiLineString geometry
    MultiLineString(MultiLineString),
    /// MultiPolygon geometry
    MultiPolygon(MultiPolygon),
    /// GeometryCollection
    GeometryCollection(GeometryCollection),
}

impl Geometry {
    /// Returns the geometry type
    #[must_use]
    pub const fn geometry_type(&self) -> GeometryType {
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

    /// Returns the bounding box if set
    #[must_use]
    pub fn bbox(&self) -> Option<&BBox> {
        match self {
            Self::Point(p) => p.bbox.as_ref(),
            Self::LineString(ls) => ls.bbox.as_ref(),
            Self::Polygon(p) => p.bbox.as_ref(),
            Self::MultiPoint(mp) => mp.bbox.as_ref(),
            Self::MultiLineString(mls) => mls.bbox.as_ref(),
            Self::MultiPolygon(mp) => mp.bbox.as_ref(),
            Self::GeometryCollection(gc) => gc.bbox.as_ref(),
        }
    }

    /// Computes the bounding box for this geometry
    #[must_use]
    pub fn compute_bbox(&self) -> Option<BBox> {
        match self {
            Self::Point(p) => p.compute_bbox(),
            Self::LineString(ls) => ls.compute_bbox(),
            Self::Polygon(p) => p.compute_bbox(),
            Self::MultiPoint(mp) => mp.compute_bbox(),
            Self::MultiLineString(mls) => mls.compute_bbox(),
            Self::MultiPolygon(mp) => mp.compute_bbox(),
            Self::GeometryCollection(gc) => gc.compute_bbox(),
        }
    }

    /// Validates the geometry
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Point(p) => p.validate(),
            Self::LineString(ls) => ls.validate(),
            Self::Polygon(p) => p.validate(),
            Self::MultiPoint(mp) => mp.validate(),
            Self::MultiLineString(mls) => mls.validate(),
            Self::MultiPolygon(mp) => mp.validate(),
            Self::GeometryCollection(gc) => gc.validate(),
        }
    }

    /// Returns true if the geometry is empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Point(_) => false, // Points can't be empty
            Self::LineString(ls) => ls.coordinates.is_empty(),
            Self::Polygon(p) => p.coordinates.is_empty(),
            Self::MultiPoint(mp) => mp.coordinates.is_empty(),
            Self::MultiLineString(mls) => mls.coordinates.is_empty(),
            Self::MultiPolygon(mp) => mp.coordinates.is_empty(),
            Self::GeometryCollection(gc) => gc.geometries.is_empty(),
        }
    }

    /// Encode this geometry as ISO WKB (Well-Known Binary), little-endian.
    ///
    /// The encoding follows the OGC WKB specification:
    /// - byte order flag (0x01 = little-endian)
    /// - 4-byte geometry type
    /// - geometry coordinates
    ///
    /// Supported types (WKB type codes):
    /// - Point (1), LineString (2), Polygon (3)
    /// - MultiPoint (4), MultiLineString (5), MultiPolygon (6)
    /// - GeometryCollection (7)
    ///
    /// Returns `None` only when a position vector has fewer than 2 elements
    /// (malformed coordinate).
    #[must_use]
    pub fn to_wkb(&self) -> Option<Vec<u8>> {
        let mut buf = Vec::with_capacity(64);
        wkb_write_geometry(self, &mut buf)?;
        Some(buf)
    }
}

// ─── WKB encoding helpers ─────────────────────────────────────────────────────

/// Little-endian WKB byte-order marker.
const WKB_LE: u8 = 0x01;

/// WKB geometry type codes (2D, little-endian).
mod wkb_type {
    pub(super) const POINT: u32 = 1;
    pub(super) const LINESTRING: u32 = 2;
    pub(super) const POLYGON: u32 = 3;
    pub(super) const MULTIPOINT: u32 = 4;
    pub(super) const MULTILINESTRING: u32 = 5;
    pub(super) const MULTIPOLYGON: u32 = 6;
    pub(super) const GEOMETRYCOLLECTION: u32 = 7;
}

/// Write a single f64 coordinate into `buf` (little-endian).
#[inline]
fn wkb_push_f64(buf: &mut Vec<u8>, v: f64) {
    buf.extend_from_slice(&v.to_le_bytes());
}

/// Write a u32 into `buf` (little-endian).
#[inline]
fn wkb_push_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

/// Write the WKB header: byte-order flag (LE) + geometry type.
#[inline]
fn wkb_write_header(buf: &mut Vec<u8>, geom_type: u32) {
    buf.push(WKB_LE);
    wkb_push_u32(buf, geom_type);
}

/// Write a single position `[x, y, ...]` as two f64 values.
/// Returns `None` when the position has fewer than 2 elements.
fn wkb_write_position(buf: &mut Vec<u8>, pos: &[f64]) -> Option<()> {
    if pos.len() < 2 {
        return None;
    }
    wkb_push_f64(buf, pos[0]);
    wkb_push_f64(buf, pos[1]);
    Some(())
}

/// Write a ring (sequence of positions) as `count + coords`.
fn wkb_write_ring(buf: &mut Vec<u8>, ring: &[Vec<f64>]) -> Option<()> {
    wkb_push_u32(buf, ring.len() as u32);
    for pos in ring {
        wkb_write_position(buf, pos)?;
    }
    Some(())
}

/// Recursively encode a [`Geometry`] into WKB bytes.
fn wkb_write_geometry(geom: &Geometry, buf: &mut Vec<u8>) -> Option<()> {
    match geom {
        Geometry::Point(p) => {
            wkb_write_header(buf, wkb_type::POINT);
            wkb_write_position(buf, &p.coordinates)?;
        }
        Geometry::LineString(ls) => {
            wkb_write_header(buf, wkb_type::LINESTRING);
            wkb_push_u32(buf, ls.coordinates.len() as u32);
            for pos in &ls.coordinates {
                wkb_write_position(buf, pos)?;
            }
        }
        Geometry::Polygon(p) => {
            wkb_write_header(buf, wkb_type::POLYGON);
            wkb_push_u32(buf, p.coordinates.len() as u32);
            for ring in &p.coordinates {
                wkb_write_ring(buf, ring)?;
            }
        }
        Geometry::MultiPoint(mp) => {
            wkb_write_header(buf, wkb_type::MULTIPOINT);
            wkb_push_u32(buf, mp.coordinates.len() as u32);
            for pos in &mp.coordinates {
                // Each sub-geometry is its own Point WKB
                let sub = Geometry::Point(Point {
                    coordinates: pos.clone(),
                    bbox: None,
                });
                wkb_write_geometry(&sub, buf)?;
            }
        }
        Geometry::MultiLineString(mls) => {
            wkb_write_header(buf, wkb_type::MULTILINESTRING);
            wkb_push_u32(buf, mls.coordinates.len() as u32);
            for line in &mls.coordinates {
                let sub = Geometry::LineString(LineString {
                    coordinates: line.clone(),
                    bbox: None,
                });
                wkb_write_geometry(&sub, buf)?;
            }
        }
        Geometry::MultiPolygon(mp) => {
            wkb_write_header(buf, wkb_type::MULTIPOLYGON);
            wkb_push_u32(buf, mp.coordinates.len() as u32);
            for rings in &mp.coordinates {
                let sub = Geometry::Polygon(Polygon {
                    coordinates: rings.clone(),
                    bbox: None,
                });
                wkb_write_geometry(&sub, buf)?;
            }
        }
        Geometry::GeometryCollection(gc) => {
            wkb_write_header(buf, wkb_type::GEOMETRYCOLLECTION);
            wkb_push_u32(buf, gc.geometries.len() as u32);
            for sub in &gc.geometries {
                wkb_write_geometry(sub, buf)?;
            }
        }
    }
    Some(())
}

impl<'de> serde::Deserialize<'de> for Geometry {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error as _;

        if deserializer.is_human_readable() {
            // JSON path: use Value as intermediate to bypass serde Content issue
            // with arbitrary_precision feature of serde_json.
            // serde_json::Value::deserialize works correctly even with
            // arbitrary_precision, whereas serde's internal Content type does not.
            let value = serde_json::Value::deserialize(deserializer).map_err(D::Error::custom)?;
            let type_str = value
                .get("type")
                .and_then(|t| t.as_str())
                .ok_or_else(|| D::Error::custom("missing 'type' field in Geometry"))?;

            match type_str {
                "Point" => serde_json::from_value(value)
                    .map(Geometry::Point)
                    .map_err(D::Error::custom),
                "LineString" => serde_json::from_value(value)
                    .map(Geometry::LineString)
                    .map_err(D::Error::custom),
                "Polygon" => serde_json::from_value(value)
                    .map(Geometry::Polygon)
                    .map_err(D::Error::custom),
                "MultiPoint" => serde_json::from_value(value)
                    .map(Geometry::MultiPoint)
                    .map_err(D::Error::custom),
                "MultiLineString" => serde_json::from_value(value)
                    .map(Geometry::MultiLineString)
                    .map_err(D::Error::custom),
                "MultiPolygon" => serde_json::from_value(value)
                    .map(Geometry::MultiPolygon)
                    .map_err(D::Error::custom),
                "GeometryCollection" => serde_json::from_value(value)
                    .map(Geometry::GeometryCollection)
                    .map_err(D::Error::custom),
                other => Err(D::Error::custom(format!("unknown Geometry type: {other}"))),
            }
        } else {
            // Non-JSON path (e.g., MessagePack): use normal tagged deserialization
            #[derive(serde::Deserialize)]
            #[serde(tag = "type")]
            enum GeometryInner {
                Point(Point),
                LineString(LineString),
                Polygon(Polygon),
                MultiPoint(MultiPoint),
                MultiLineString(MultiLineString),
                MultiPolygon(MultiPolygon),
                GeometryCollection(GeometryCollection),
            }
            let inner = GeometryInner::deserialize(deserializer)?;
            std::result::Result::Ok(match inner {
                GeometryInner::Point(p) => Geometry::Point(p),
                GeometryInner::LineString(ls) => Geometry::LineString(ls),
                GeometryInner::Polygon(p) => Geometry::Polygon(p),
                GeometryInner::MultiPoint(mp) => Geometry::MultiPoint(mp),
                GeometryInner::MultiLineString(mls) => Geometry::MultiLineString(mls),
                GeometryInner::MultiPolygon(mp) => Geometry::MultiPolygon(mp),
                GeometryInner::GeometryCollection(gc) => Geometry::GeometryCollection(gc),
            })
        }
    }
}

/// Point geometry
///
/// A Point is a single position in geographic space.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Point {
    /// The coordinates of the point (longitude, latitude, \[elevation\])
    pub coordinates: Position,
    /// Optional bounding box
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbox: Option<BBox>,
}

impl Point {
    /// Creates a new Point
    pub fn new(coordinates: Position) -> Result<Self> {
        if coordinates.len() < 2 {
            return Err(GeoJsonError::invalid_coordinates(
                "Point must have at least 2 coordinates",
            ));
        }
        Ok(Self {
            coordinates,
            bbox: None,
        })
    }

    /// Creates a new 2D Point (longitude, latitude)
    pub fn new_2d(lon: f64, lat: f64) -> Result<Self> {
        Self::new(vec![lon, lat])
    }

    /// Creates a new 3D Point (longitude, latitude, elevation)
    pub fn new_3d(lon: f64, lat: f64, elevation: f64) -> Result<Self> {
        Self::new(vec![lon, lat, elevation])
    }

    /// Returns the longitude
    #[must_use]
    pub fn longitude(&self) -> Option<f64> {
        self.coordinates.first().copied()
    }

    /// Returns the latitude
    #[must_use]
    pub fn latitude(&self) -> Option<f64> {
        self.coordinates.get(1).copied()
    }

    /// Returns the elevation (if present)
    #[must_use]
    pub fn elevation(&self) -> Option<f64> {
        self.coordinates.get(2).copied()
    }

    /// Validates the point
    pub fn validate(&self) -> Result<()> {
        validate_position(&self.coordinates)?;
        if let Some(bbox) = &self.bbox {
            validate_bbox(bbox)?;
        }
        Ok(())
    }

    /// Computes the bounding box
    #[must_use]
    pub fn compute_bbox(&self) -> Option<BBox> {
        if self.coordinates.len() >= 2 {
            Some(vec![
                self.coordinates[0],
                self.coordinates[1],
                self.coordinates[0],
                self.coordinates[1],
            ])
        } else {
            None
        }
    }
}

/// LineString geometry
///
/// A LineString is a sequence of two or more positions forming a line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LineString {
    /// The coordinates of the line
    pub coordinates: CoordinateSequence,
    /// Optional bounding box
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbox: Option<BBox>,
}

impl LineString {
    /// Creates a new LineString
    pub fn new(coordinates: CoordinateSequence) -> Result<Self> {
        if coordinates.len() < 2 {
            return Err(GeoJsonError::invalid_coordinates(
                "LineString must have at least 2 positions",
            ));
        }
        Ok(Self {
            coordinates,
            bbox: None,
        })
    }

    /// Validates the LineString
    pub fn validate(&self) -> Result<()> {
        if self.coordinates.len() < 2 {
            return Err(GeoJsonError::invalid_coordinates(
                "LineString must have at least 2 positions",
            ));
        }
        for (i, pos) in self.coordinates.iter().enumerate() {
            validate_position(pos).map_err(|e| {
                GeoJsonError::validation_at(e.to_string(), format!("coordinates/{i}"))
            })?;
        }
        if let Some(bbox) = &self.bbox {
            validate_bbox(bbox)?;
        }
        Ok(())
    }

    /// Computes the bounding box
    #[must_use]
    pub fn compute_bbox(&self) -> Option<BBox> {
        compute_bbox_from_positions(&self.coordinates)
    }

    /// Returns the number of positions
    #[must_use]
    pub fn len(&self) -> usize {
        self.coordinates.len()
    }

    /// Returns true if the LineString is empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.coordinates.is_empty()
    }
}

/// Polygon geometry
///
/// A Polygon is defined by a list of linear rings. The first ring is the
/// exterior ring, and any subsequent rings are holes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Polygon {
    /// The coordinates of the polygon (exterior ring + holes)
    pub coordinates: Vec<CoordinateSequence>,
    /// Optional bounding box
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbox: Option<BBox>,
}

impl Polygon {
    /// Creates a new Polygon
    pub fn new(coordinates: Vec<CoordinateSequence>) -> Result<Self> {
        if coordinates.is_empty() {
            return Err(GeoJsonError::invalid_coordinates(
                "Polygon must have at least one ring",
            ));
        }
        Ok(Self {
            coordinates,
            bbox: None,
        })
    }

    /// Creates a new Polygon from an exterior ring
    pub fn from_exterior(exterior: CoordinateSequence) -> Result<Self> {
        Self::new(vec![exterior])
    }

    /// Creates a new Polygon with holes
    pub fn with_holes(
        exterior: CoordinateSequence,
        holes: Vec<CoordinateSequence>,
    ) -> Result<Self> {
        let mut rings = vec![exterior];
        rings.extend(holes);
        Self::new(rings)
    }

    /// Returns the exterior ring
    #[must_use]
    pub fn exterior(&self) -> Option<&CoordinateSequence> {
        self.coordinates.first()
    }

    /// Returns the holes (interior rings)
    #[must_use]
    pub fn holes(&self) -> &[CoordinateSequence] {
        if self.coordinates.len() > 1 {
            &self.coordinates[1..]
        } else {
            &[]
        }
    }

    /// Validates the Polygon
    pub fn validate(&self) -> Result<()> {
        if self.coordinates.is_empty() {
            return Err(GeoJsonError::invalid_coordinates(
                "Polygon must have at least one ring",
            ));
        }

        for (ring_idx, ring) in self.coordinates.iter().enumerate() {
            validate_linear_ring(ring).map_err(|e| {
                GeoJsonError::validation_at(e.to_string(), format!("coordinates/{ring_idx}"))
            })?;
        }

        if let Some(bbox) = &self.bbox {
            validate_bbox(bbox)?;
        }

        Ok(())
    }

    /// Computes the bounding box
    #[must_use]
    pub fn compute_bbox(&self) -> Option<BBox> {
        if self.coordinates.is_empty() {
            return None;
        }

        let all_positions: Vec<_> = self.coordinates.iter().flatten().cloned().collect();
        compute_bbox_from_positions(&all_positions)
    }

    /// Returns the number of rings
    #[must_use]
    pub fn ring_count(&self) -> usize {
        self.coordinates.len()
    }

    /// Returns true if the Polygon is empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.coordinates.is_empty()
    }
}

/// MultiPoint geometry
///
/// A MultiPoint is a collection of points.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MultiPoint {
    /// The coordinates of all points
    pub coordinates: CoordinateSequence,
    /// Optional bounding box
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbox: Option<BBox>,
}

impl MultiPoint {
    /// Creates a new MultiPoint
    pub fn new(coordinates: CoordinateSequence) -> Result<Self> {
        Ok(Self {
            coordinates,
            bbox: None,
        })
    }

    /// Validates the MultiPoint
    pub fn validate(&self) -> Result<()> {
        for (i, pos) in self.coordinates.iter().enumerate() {
            validate_position(pos).map_err(|e| {
                GeoJsonError::validation_at(e.to_string(), format!("coordinates/{i}"))
            })?;
        }
        if let Some(bbox) = &self.bbox {
            validate_bbox(bbox)?;
        }
        Ok(())
    }

    /// Computes the bounding box
    #[must_use]
    pub fn compute_bbox(&self) -> Option<BBox> {
        compute_bbox_from_positions(&self.coordinates)
    }

    /// Returns the number of points
    #[must_use]
    pub fn len(&self) -> usize {
        self.coordinates.len()
    }

    /// Returns true if the MultiPoint is empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.coordinates.is_empty()
    }
}

/// MultiLineString geometry
///
/// A MultiLineString is a collection of LineStrings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MultiLineString {
    /// The coordinates of all LineStrings
    pub coordinates: Vec<CoordinateSequence>,
    /// Optional bounding box
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbox: Option<BBox>,
}

impl MultiLineString {
    /// Creates a new MultiLineString
    pub fn new(coordinates: Vec<CoordinateSequence>) -> Result<Self> {
        Ok(Self {
            coordinates,
            bbox: None,
        })
    }

    /// Validates the MultiLineString
    pub fn validate(&self) -> Result<()> {
        for (line_idx, line) in self.coordinates.iter().enumerate() {
            if line.len() < 2 {
                return Err(GeoJsonError::validation_at(
                    "LineString must have at least 2 positions",
                    format!("coordinates/{line_idx}"),
                ));
            }
            for (pos_idx, pos) in line.iter().enumerate() {
                validate_position(pos).map_err(|e| {
                    GeoJsonError::validation_at(
                        e.to_string(),
                        format!("coordinates/{line_idx}/{pos_idx}"),
                    )
                })?;
            }
        }
        if let Some(bbox) = &self.bbox {
            validate_bbox(bbox)?;
        }
        Ok(())
    }

    /// Computes the bounding box
    #[must_use]
    pub fn compute_bbox(&self) -> Option<BBox> {
        if self.coordinates.is_empty() {
            return None;
        }

        let all_positions: Vec<_> = self.coordinates.iter().flatten().cloned().collect();
        compute_bbox_from_positions(&all_positions)
    }

    /// Returns the number of LineStrings
    #[must_use]
    pub fn len(&self) -> usize {
        self.coordinates.len()
    }

    /// Returns true if the MultiLineString is empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.coordinates.is_empty()
    }
}

/// MultiPolygon geometry
///
/// A MultiPolygon is a collection of Polygons.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MultiPolygon {
    /// The coordinates of all Polygons
    pub coordinates: Vec<Vec<CoordinateSequence>>,
    /// Optional bounding box
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbox: Option<BBox>,
}

impl MultiPolygon {
    /// Creates a new MultiPolygon
    pub fn new(coordinates: Vec<Vec<CoordinateSequence>>) -> Result<Self> {
        Ok(Self {
            coordinates,
            bbox: None,
        })
    }

    /// Validates the MultiPolygon
    pub fn validate(&self) -> Result<()> {
        for (poly_idx, polygon) in self.coordinates.iter().enumerate() {
            if polygon.is_empty() {
                return Err(GeoJsonError::validation_at(
                    "Polygon must have at least one ring",
                    format!("coordinates/{poly_idx}"),
                ));
            }
            for (ring_idx, ring) in polygon.iter().enumerate() {
                validate_linear_ring(ring).map_err(|e| {
                    GeoJsonError::validation_at(
                        e.to_string(),
                        format!("coordinates/{poly_idx}/{ring_idx}"),
                    )
                })?;
            }
        }
        if let Some(bbox) = &self.bbox {
            validate_bbox(bbox)?;
        }
        Ok(())
    }

    /// Computes the bounding box
    #[must_use]
    pub fn compute_bbox(&self) -> Option<BBox> {
        if self.coordinates.is_empty() {
            return None;
        }

        let all_positions: Vec<_> = self
            .coordinates
            .iter()
            .flatten()
            .flatten()
            .cloned()
            .collect();
        compute_bbox_from_positions(&all_positions)
    }

    /// Returns the number of Polygons
    #[must_use]
    pub fn len(&self) -> usize {
        self.coordinates.len()
    }

    /// Returns true if the MultiPolygon is empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.coordinates.is_empty()
    }
}

/// GeometryCollection
///
/// A GeometryCollection is a heterogeneous collection of geometries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeometryCollection {
    /// The geometries in the collection
    pub geometries: Vec<Geometry>,
    /// Optional bounding box
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbox: Option<BBox>,
}

impl GeometryCollection {
    /// Creates a new GeometryCollection
    pub fn new(geometries: Vec<Geometry>) -> Result<Self> {
        Ok(Self {
            geometries,
            bbox: None,
        })
    }

    /// Validates the GeometryCollection
    pub fn validate(&self) -> Result<()> {
        for (i, geom) in self.geometries.iter().enumerate() {
            geom.validate().map_err(|e| {
                GeoJsonError::validation_at(e.to_string(), format!("geometries/{i}"))
            })?;
        }
        if let Some(bbox) = &self.bbox {
            validate_bbox(bbox)?;
        }
        Ok(())
    }

    /// Computes the bounding box
    #[must_use]
    pub fn compute_bbox(&self) -> Option<BBox> {
        if self.geometries.is_empty() {
            return None;
        }

        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;

        for geom in &self.geometries {
            if let Some(bbox) = geom.compute_bbox() {
                if bbox.len() >= 4 {
                    min_x = min_x.min(bbox[0]);
                    min_y = min_y.min(bbox[1]);
                    max_x = max_x.max(bbox[2]);
                    max_y = max_y.max(bbox[3]);
                }
            }
        }

        if min_x.is_finite() && min_y.is_finite() && max_x.is_finite() && max_y.is_finite() {
            Some(vec![min_x, min_y, max_x, max_y])
        } else {
            None
        }
    }

    /// Returns the number of geometries
    #[must_use]
    pub fn len(&self) -> usize {
        self.geometries.len()
    }

    /// Returns true if the collection is empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.geometries.is_empty()
    }
}

/// Validates a position
fn validate_position(pos: &Position) -> Result<()> {
    if pos.len() < 2 {
        return Err(GeoJsonError::invalid_coordinates(
            "Position must have at least 2 coordinates",
        ));
    }

    // Check for valid numbers
    for (i, &coord) in pos.iter().enumerate() {
        if !coord.is_finite() {
            return Err(GeoJsonError::invalid_coordinates_at(
                format!("Coordinate at index {i} is not finite: {coord}"),
                i,
            ));
        }
    }

    // Validate longitude range [-180, 180]
    let lon = pos[0];
    if !(-180.0..=180.0).contains(&lon) {
        return Err(GeoJsonError::invalid_coordinates(format!(
            "Longitude out of range [-180, 180]: {lon}"
        )));
    }

    // Validate latitude range [-90, 90]
    let lat = pos[1];
    if !(-90.0..=90.0).contains(&lat) {
        return Err(GeoJsonError::invalid_coordinates(format!(
            "Latitude out of range [-90, 90]: {lat}"
        )));
    }

    Ok(())
}

/// Validates a linear ring
fn validate_linear_ring(ring: &CoordinateSequence) -> Result<()> {
    if ring.len() < 4 {
        return Err(GeoJsonError::invalid_coordinates(
            "Linear ring must have at least 4 positions",
        ));
    }

    // First and last position must be the same
    if let (Some(first), Some(last)) = (ring.first(), ring.last()) {
        if first != last {
            return Err(GeoJsonError::topology(
                "Linear ring must be closed (first and last positions must be equal)",
            ));
        }
    }

    // Validate all positions
    for (i, pos) in ring.iter().enumerate() {
        validate_position(pos)
            .map_err(|e| GeoJsonError::validation_at(e.to_string(), format!("position/{i}")))?;
    }

    Ok(())
}

/// Validates a bounding box
pub(crate) fn validate_bbox(bbox: &BBox) -> Result<()> {
    if bbox.len() != 4 && bbox.len() != 6 {
        return Err(GeoJsonError::InvalidBbox {
            message: format!("Bounding box must have 4 or 6 elements, got {}", bbox.len()),
        });
    }

    for &val in bbox {
        if !val.is_finite() {
            return Err(GeoJsonError::InvalidBbox {
                message: format!("Bounding box contains non-finite value: {val}"),
            });
        }
    }

    // Check min <= max for each dimension
    if bbox[0] > bbox[2] {
        return Err(GeoJsonError::InvalidBbox {
            message: format!("min_x ({}) > max_x ({})", bbox[0], bbox[2]),
        });
    }
    if bbox[1] > bbox[3] {
        return Err(GeoJsonError::InvalidBbox {
            message: format!("min_y ({}) > max_y ({})", bbox[1], bbox[3]),
        });
    }
    if bbox.len() == 6 && bbox[4] > bbox[5] {
        return Err(GeoJsonError::InvalidBbox {
            message: format!("min_z ({}) > max_z ({})", bbox[4], bbox[5]),
        });
    }

    Ok(())
}

/// Computes a bounding box from a sequence of positions
fn compute_bbox_from_positions(positions: &[Position]) -> Option<BBox> {
    if positions.is_empty() {
        return None;
    }

    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    for pos in positions {
        if pos.len() >= 2 {
            min_x = min_x.min(pos[0]);
            min_y = min_y.min(pos[1]);
            max_x = max_x.max(pos[0]);
            max_y = max_y.max(pos[1]);
        }
    }

    if min_x.is_finite() && min_y.is_finite() && max_x.is_finite() && max_y.is_finite() {
        Some(vec![min_x, min_y, max_x, max_y])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_geometry_type() {
        assert!(GeometryType::MultiPoint.is_multi());
        assert!(!GeometryType::Point.is_multi());
        assert_eq!(GeometryType::Point.as_str(), "Point");
        assert_eq!(
            GeometryType::from_str("Point").ok(),
            Some(GeometryType::Point)
        );
    }

    #[test]
    fn test_point() {
        let point = Point::new_2d(-122.4, 37.8).ok();
        assert!(point.is_some());
        let p = point.expect("valid point");
        assert_eq!(p.longitude(), Some(-122.4));
        assert_eq!(p.latitude(), Some(37.8));
        assert!(p.validate().is_ok());
    }

    #[test]
    fn test_point_invalid() {
        let result = Point::new(vec![0.0]);
        assert!(result.is_err());
    }

    #[test]
    fn test_linestring() {
        let coords = vec![vec![0.0, 0.0], vec![1.0, 1.0]];
        let linestring = LineString::new(coords).ok();
        assert!(linestring.is_some());
        let ls = linestring.expect("valid linestring");
        assert_eq!(ls.len(), 2);
        assert!(ls.validate().is_ok());
    }

    #[test]
    fn test_linestring_invalid() {
        let coords = vec![vec![0.0, 0.0]]; // Only one point
        let result = LineString::new(coords);
        assert!(result.is_err());
    }

    #[test]
    fn test_polygon() {
        let exterior = vec![
            vec![0.0, 0.0],
            vec![1.0, 0.0],
            vec![1.0, 1.0],
            vec![0.0, 1.0],
            vec![0.0, 0.0],
        ];
        let polygon = Polygon::from_exterior(exterior).ok();
        assert!(polygon.is_some());
        let p = polygon.expect("valid polygon");
        assert_eq!(p.ring_count(), 1);
        assert!(p.validate().is_ok());
    }

    #[test]
    fn test_polygon_with_hole() {
        let exterior = vec![
            vec![0.0, 0.0],
            vec![4.0, 0.0],
            vec![4.0, 4.0],
            vec![0.0, 4.0],
            vec![0.0, 0.0],
        ];
        let hole = vec![
            vec![1.0, 1.0],
            vec![3.0, 1.0],
            vec![3.0, 3.0],
            vec![1.0, 3.0],
            vec![1.0, 1.0],
        ];
        let polygon = Polygon::with_holes(exterior, vec![hole]).ok();
        assert!(polygon.is_some());
        let p = polygon.expect("valid polygon");
        assert_eq!(p.ring_count(), 2);
        assert_eq!(p.holes().len(), 1);
    }

    #[test]
    fn test_validate_position() {
        assert!(validate_position(&vec![0.0, 0.0]).is_ok());
        assert!(validate_position(&vec![0.0, 0.0, 100.0]).is_ok());
        assert!(validate_position(&vec![0.0]).is_err());
        assert!(validate_position(&vec![181.0, 0.0]).is_err());
        assert!(validate_position(&vec![0.0, 91.0]).is_err());
        assert!(validate_position(&vec![f64::NAN, 0.0]).is_err());
    }

    #[test]
    fn test_validate_linear_ring() {
        let valid = vec![
            vec![0.0, 0.0],
            vec![1.0, 0.0],
            vec![1.0, 1.0],
            vec![0.0, 0.0],
        ];
        assert!(validate_linear_ring(&valid).is_ok());

        let too_short = vec![vec![0.0, 0.0], vec![1.0, 0.0], vec![0.0, 0.0]];
        assert!(validate_linear_ring(&too_short).is_err());

        let not_closed = vec![
            vec![0.0, 0.0],
            vec![1.0, 0.0],
            vec![1.0, 1.0],
            vec![0.0, 1.0],
        ];
        assert!(validate_linear_ring(&not_closed).is_err());
    }

    #[test]
    fn test_compute_bbox() {
        let positions = vec![vec![0.0, 0.0], vec![1.0, 1.0], vec![2.0, 0.5]];
        let bbox = compute_bbox_from_positions(&positions);
        assert!(bbox.is_some());
        let b = bbox.expect("valid bbox");
        assert_eq!(b.len(), 4);
        assert_eq!(b[0], 0.0); // min_x
        assert_eq!(b[1], 0.0); // min_y
        assert_eq!(b[2], 2.0); // max_x
        assert_eq!(b[3], 1.0); // max_y
    }

    #[test]
    fn test_multipoint() {
        let coords = vec![vec![0.0, 0.0], vec![1.0, 1.0]];
        let mp = MultiPoint::new(coords).ok();
        assert!(mp.is_some());
        let multipoint = mp.expect("valid multipoint");
        assert_eq!(multipoint.len(), 2);
        assert!(multipoint.validate().is_ok());
    }

    #[test]
    fn test_geometry_collection() {
        let point = Geometry::Point(Point::new_2d(0.0, 0.0).expect("valid point"));
        let line = Geometry::LineString(
            LineString::new(vec![vec![0.0, 0.0], vec![1.0, 1.0]]).expect("valid linestring"),
        );
        let gc = GeometryCollection::new(vec![point, line]).ok();
        assert!(gc.is_some());
        let collection = gc.expect("valid collection");
        assert_eq!(collection.len(), 2);
        assert!(collection.validate().is_ok());
    }

    // ── to_wkb ────────────────────────────────────────────────────────────────

    #[test]
    fn test_wkb_point_size() {
        // WKB Point: 1 (byte order) + 4 (type) + 8 (x) + 8 (y) = 21 bytes
        let geom = Geometry::Point(Point::new_2d(139.7, 35.7).expect("point"));
        let wkb = geom.to_wkb().expect("wkb");
        assert_eq!(wkb.len(), 21, "WKB Point must be 21 bytes");
        assert_eq!(wkb[0], 0x01, "little-endian flag");
        assert_eq!(&wkb[1..5], &1u32.to_le_bytes(), "WKB type = 1 (Point)");
    }

    #[test]
    fn test_wkb_point_coordinates() {
        let x = 139.691711f64;
        let y = 35.689487f64;
        let geom = Geometry::Point(Point::new_2d(x, y).expect("point"));
        let wkb = geom.to_wkb().expect("wkb");
        let decoded_x = f64::from_le_bytes(wkb[5..13].try_into().expect("slice"));
        let decoded_y = f64::from_le_bytes(wkb[13..21].try_into().expect("slice"));
        assert!((decoded_x - x).abs() < 1e-10, "x mismatch: {decoded_x}");
        assert!((decoded_y - y).abs() < 1e-10, "y mismatch: {decoded_y}");
    }

    #[test]
    fn test_wkb_linestring() {
        let coords = vec![vec![0.0, 0.0], vec![1.0, 1.0], vec![2.0, 0.0]];
        let geom = Geometry::LineString(LineString::new(coords).expect("ls"));
        let wkb = geom.to_wkb().expect("wkb");
        // 1 + 4 + 4 (count) + 3 * 16 = 57 bytes
        assert_eq!(wkb.len(), 57);
        assert_eq!(&wkb[1..5], &2u32.to_le_bytes(), "WKB type = 2 (LineString)");
        let count = u32::from_le_bytes(wkb[5..9].try_into().expect("slice"));
        assert_eq!(count, 3);
    }

    #[test]
    fn test_wkb_polygon() {
        // Simple square ring
        let ring = vec![
            vec![0.0, 0.0],
            vec![1.0, 0.0],
            vec![1.0, 1.0],
            vec![0.0, 1.0],
            vec![0.0, 0.0],
        ];
        let geom = Geometry::Polygon(Polygon::from_exterior(ring).expect("polygon"));
        let wkb = geom.to_wkb().expect("wkb");
        // 1 + 4 + 4 (ring count) + 4 (point count per ring) + 5*16 = 93 bytes
        assert_eq!(wkb.len(), 93);
        assert_eq!(&wkb[1..5], &3u32.to_le_bytes(), "WKB type = 3 (Polygon)");
    }

    #[test]
    fn test_wkb_multipoint() {
        let coords = vec![vec![0.0, 0.0], vec![1.0, 1.0]];
        let geom = Geometry::MultiPoint(MultiPoint::new(coords).expect("mp"));
        let wkb = geom.to_wkb().expect("wkb");
        assert_eq!(&wkb[1..5], &4u32.to_le_bytes(), "WKB type = 4 (MultiPoint)");
        // 1 + 4 + 4 (count) + 2 * (1 + 4 + 16) = 51 bytes
        assert_eq!(wkb.len(), 51);
    }

    #[test]
    fn test_wkb_geometry_collection_empty() {
        let geom = Geometry::GeometryCollection(GeometryCollection::new(vec![]).expect("gc"));
        let wkb = geom.to_wkb().expect("wkb");
        // 1 + 4 + 4 (count=0) = 9 bytes
        assert_eq!(wkb.len(), 9);
        assert_eq!(
            &wkb[1..5],
            &7u32.to_le_bytes(),
            "WKB type = 7 (GeometryCollection)"
        );
    }

    #[test]
    fn test_wkb_point_invalid_coordinates_returns_none() {
        // Build a Geometry::Point with empty coordinates (invalid) directly
        let geom = Geometry::Point(Point {
            coordinates: vec![],
            bbox: None,
        });
        let result = geom.to_wkb();
        assert!(result.is_none(), "malformed point should return None");
    }
}
