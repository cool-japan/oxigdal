//! Shapefile shape type definitions
//!
//! This module defines all shape types supported by the Shapefile format,
//! including 2D, Z (3D), and M (measured) variants.

use crate::error::{Result, ShapefileError};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Read, Write};

/// Shapefile shape types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShapeType {
    /// Null shape (empty)
    Null,
    /// Point (2D)
    Point,
    /// PolyLine (2D)
    PolyLine,
    /// Polygon (2D)
    Polygon,
    /// MultiPoint (2D)
    MultiPoint,
    /// Point with Z coordinate
    PointZ,
    /// PolyLine with Z coordinates
    PolyLineZ,
    /// Polygon with Z coordinates
    PolygonZ,
    /// MultiPoint with Z coordinates
    MultiPointZ,
    /// Point with M value (measure)
    PointM,
    /// PolyLine with M values
    PolyLineM,
    /// Polygon with M values
    PolygonM,
    /// MultiPoint with M values
    MultiPointM,
    /// MultiPatch (3D surface)
    MultiPatch,
}

impl ShapeType {
    /// Converts a shape type code to a `ShapeType`
    pub fn from_code(code: i32) -> Result<Self> {
        match code {
            0 => Ok(Self::Null),
            1 => Ok(Self::Point),
            3 => Ok(Self::PolyLine),
            5 => Ok(Self::Polygon),
            8 => Ok(Self::MultiPoint),
            11 => Ok(Self::PointZ),
            13 => Ok(Self::PolyLineZ),
            15 => Ok(Self::PolygonZ),
            18 => Ok(Self::MultiPointZ),
            21 => Ok(Self::PointM),
            23 => Ok(Self::PolyLineM),
            25 => Ok(Self::PolygonM),
            28 => Ok(Self::MultiPointM),
            31 => Ok(Self::MultiPatch),
            _ => Err(ShapefileError::InvalidShapeType { shape_type: code }),
        }
    }

    /// Converts a `ShapeType` to its code
    pub fn to_code(self) -> i32 {
        match self {
            Self::Null => 0,
            Self::Point => 1,
            Self::PolyLine => 3,
            Self::Polygon => 5,
            Self::MultiPoint => 8,
            Self::PointZ => 11,
            Self::PolyLineZ => 13,
            Self::PolygonZ => 15,
            Self::MultiPointZ => 18,
            Self::PointM => 21,
            Self::PolyLineM => 23,
            Self::PolygonM => 25,
            Self::MultiPointM => 28,
            Self::MultiPatch => 31,
        }
    }

    /// Returns true if this shape type has Z coordinates
    pub fn has_z(self) -> bool {
        matches!(
            self,
            Self::PointZ | Self::PolyLineZ | Self::PolygonZ | Self::MultiPointZ | Self::MultiPatch
        )
    }

    /// Returns true if this shape type has M values
    pub fn has_m(self) -> bool {
        matches!(
            self,
            Self::PointM
                | Self::PolyLineM
                | Self::PolygonM
                | Self::MultiPointM
                | Self::PointZ
                | Self::PolyLineZ
                | Self::PolygonZ
                | Self::MultiPointZ
                | Self::MultiPatch
        )
    }

    /// Returns the name of the shape type
    pub fn name(self) -> &'static str {
        match self {
            Self::Null => "Null",
            Self::Point => "Point",
            Self::PolyLine => "PolyLine",
            Self::Polygon => "Polygon",
            Self::MultiPoint => "MultiPoint",
            Self::PointZ => "PointZ",
            Self::PolyLineZ => "PolyLineZ",
            Self::PolygonZ => "PolygonZ",
            Self::MultiPointZ => "MultiPointZ",
            Self::PointM => "PointM",
            Self::PolyLineM => "PolyLineM",
            Self::PolygonM => "PolygonM",
            Self::MultiPointM => "MultiPointM",
            Self::MultiPatch => "MultiPatch",
        }
    }
}

/// A 2D point
#[derive(Debug, Clone, PartialEq)]
pub struct Point {
    /// X coordinate
    pub x: f64,
    /// Y coordinate
    pub y: f64,
}

impl Point {
    /// Creates a new point
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// Reads a point from a reader
    pub fn read<R: Read>(reader: &mut R) -> Result<Self> {
        let x = reader
            .read_f64::<LittleEndian>()
            .map_err(|_| ShapefileError::unexpected_eof("reading point x"))?;
        let y = reader
            .read_f64::<LittleEndian>()
            .map_err(|_| ShapefileError::unexpected_eof("reading point y"))?;

        if !x.is_finite() || !y.is_finite() {
            return Err(ShapefileError::invalid_coordinates(
                "point coordinates must be finite",
            ));
        }

        Ok(Self { x, y })
    }

    /// Writes a point to a writer
    pub fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        writer
            .write_f64::<LittleEndian>(self.x)
            .map_err(ShapefileError::Io)?;
        writer
            .write_f64::<LittleEndian>(self.y)
            .map_err(ShapefileError::Io)?;
        Ok(())
    }
}

/// A 3D point with Z coordinate
#[derive(Debug, Clone, PartialEq)]
pub struct PointZ {
    /// X coordinate
    pub x: f64,
    /// Y coordinate
    pub y: f64,
    /// Z coordinate
    pub z: f64,
    /// M value (optional measure)
    pub m: Option<f64>,
}

impl PointZ {
    /// Creates a new 3D point
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z, m: None }
    }

    /// Creates a new 3D point with M value
    pub fn new_with_m(x: f64, y: f64, z: f64, m: f64) -> Self {
        Self {
            x,
            y,
            z,
            m: Some(m),
        }
    }

    /// Reads a PointZ from a reader
    pub fn read<R: Read>(reader: &mut R) -> Result<Self> {
        let x = reader
            .read_f64::<LittleEndian>()
            .map_err(|_| ShapefileError::unexpected_eof("reading pointz x"))?;
        let y = reader
            .read_f64::<LittleEndian>()
            .map_err(|_| ShapefileError::unexpected_eof("reading pointz y"))?;
        let z = reader
            .read_f64::<LittleEndian>()
            .map_err(|_| ShapefileError::unexpected_eof("reading pointz z"))?;

        if !x.is_finite() || !y.is_finite() || !z.is_finite() {
            return Err(ShapefileError::invalid_coordinates(
                "pointz coordinates must be finite",
            ));
        }

        // M value is optional
        let m = match reader.read_f64::<LittleEndian>() {
            Ok(m_val) if m_val.is_finite() => Some(m_val),
            _ => None,
        };

        Ok(Self { x, y, z, m })
    }

    /// Writes a PointZ to a writer
    pub fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        writer
            .write_f64::<LittleEndian>(self.x)
            .map_err(ShapefileError::Io)?;
        writer
            .write_f64::<LittleEndian>(self.y)
            .map_err(ShapefileError::Io)?;
        writer
            .write_f64::<LittleEndian>(self.z)
            .map_err(ShapefileError::Io)?;
        writer
            .write_f64::<LittleEndian>(self.m.unwrap_or(0.0))
            .map_err(ShapefileError::Io)?;
        Ok(())
    }
}

/// A point with M value (measure)
#[derive(Debug, Clone, PartialEq)]
pub struct PointM {
    /// X coordinate
    pub x: f64,
    /// Y coordinate
    pub y: f64,
    /// M value (measure)
    pub m: f64,
}

impl PointM {
    /// Creates a new point with M value
    pub fn new(x: f64, y: f64, m: f64) -> Self {
        Self { x, y, m }
    }

    /// Reads a PointM from a reader
    pub fn read<R: Read>(reader: &mut R) -> Result<Self> {
        let x = reader
            .read_f64::<LittleEndian>()
            .map_err(|_| ShapefileError::unexpected_eof("reading pointm x"))?;
        let y = reader
            .read_f64::<LittleEndian>()
            .map_err(|_| ShapefileError::unexpected_eof("reading pointm y"))?;
        let m = reader
            .read_f64::<LittleEndian>()
            .map_err(|_| ShapefileError::unexpected_eof("reading pointm m"))?;

        if !x.is_finite() || !y.is_finite() {
            return Err(ShapefileError::invalid_coordinates(
                "pointm coordinates must be finite",
            ));
        }

        Ok(Self { x, y, m })
    }

    /// Writes a PointM to a writer
    pub fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        writer
            .write_f64::<LittleEndian>(self.x)
            .map_err(ShapefileError::Io)?;
        writer
            .write_f64::<LittleEndian>(self.y)
            .map_err(ShapefileError::Io)?;
        writer
            .write_f64::<LittleEndian>(self.m)
            .map_err(ShapefileError::Io)?;
        Ok(())
    }
}

/// Bounding box for shapes
#[derive(Debug, Clone, PartialEq)]
pub struct Box2D {
    /// Minimum X
    pub x_min: f64,
    /// Minimum Y
    pub y_min: f64,
    /// Maximum X
    pub x_max: f64,
    /// Maximum Y
    pub y_max: f64,
}

impl Box2D {
    /// Creates a new 2D bounding box
    pub fn new(x_min: f64, y_min: f64, x_max: f64, y_max: f64) -> Result<Self> {
        if x_min > x_max {
            return Err(ShapefileError::InvalidBbox {
                message: format!("x_min ({}) > x_max ({})", x_min, x_max),
            });
        }
        if y_min > y_max {
            return Err(ShapefileError::InvalidBbox {
                message: format!("y_min ({}) > y_max ({})", y_min, y_max),
            });
        }
        Ok(Self {
            x_min,
            y_min,
            x_max,
            y_max,
        })
    }

    /// Reads a 2D bounding box from a reader
    pub fn read<R: Read>(reader: &mut R) -> Result<Self> {
        let x_min = reader
            .read_f64::<LittleEndian>()
            .map_err(|_| ShapefileError::unexpected_eof("reading bbox x_min"))?;
        let y_min = reader
            .read_f64::<LittleEndian>()
            .map_err(|_| ShapefileError::unexpected_eof("reading bbox y_min"))?;
        let x_max = reader
            .read_f64::<LittleEndian>()
            .map_err(|_| ShapefileError::unexpected_eof("reading bbox x_max"))?;
        let y_max = reader
            .read_f64::<LittleEndian>()
            .map_err(|_| ShapefileError::unexpected_eof("reading bbox y_max"))?;

        Self::new(x_min, y_min, x_max, y_max)
    }

    /// Writes a 2D bounding box to a writer
    pub fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        writer
            .write_f64::<LittleEndian>(self.x_min)
            .map_err(ShapefileError::Io)?;
        writer
            .write_f64::<LittleEndian>(self.y_min)
            .map_err(ShapefileError::Io)?;
        writer
            .write_f64::<LittleEndian>(self.x_max)
            .map_err(ShapefileError::Io)?;
        writer
            .write_f64::<LittleEndian>(self.y_max)
            .map_err(ShapefileError::Io)?;
        Ok(())
    }

    /// Computes the bounding box from a list of points
    pub fn from_points(points: &[Point]) -> Result<Self> {
        if points.is_empty() {
            return Err(ShapefileError::invalid_geometry(
                "cannot compute bbox from empty points",
            ));
        }

        let mut x_min = points[0].x;
        let mut y_min = points[0].y;
        let mut x_max = points[0].x;
        let mut y_max = points[0].y;

        for point in &points[1..] {
            x_min = x_min.min(point.x);
            y_min = y_min.min(point.y);
            x_max = x_max.max(point.x);
            y_max = y_max.max(point.y);
        }

        Self::new(x_min, y_min, x_max, y_max)
    }
}

/// A multi-part shape (PolyLine or Polygon)
#[derive(Debug, Clone, PartialEq)]
pub struct MultiPartShape {
    /// Bounding box
    pub bbox: Box2D,
    /// Number of parts
    pub num_parts: i32,
    /// Number of points
    pub num_points: i32,
    /// Part start indices
    pub parts: Vec<i32>,
    /// Points
    pub points: Vec<Point>,
}

impl MultiPartShape {
    /// Creates a new multi-part shape
    pub fn new(parts: Vec<i32>, points: Vec<Point>) -> Result<Self> {
        if parts.is_empty() {
            return Err(ShapefileError::invalid_geometry("parts cannot be empty"));
        }
        if points.is_empty() {
            return Err(ShapefileError::invalid_geometry("points cannot be empty"));
        }

        let bbox = Box2D::from_points(&points)?;

        Ok(Self {
            bbox,
            num_parts: parts.len() as i32,
            num_points: points.len() as i32,
            parts,
            points,
        })
    }

    /// Reads a multi-part shape from a reader
    pub fn read<R: Read>(reader: &mut R) -> Result<Self> {
        let bbox = Box2D::read(reader)?;

        let num_parts = reader
            .read_i32::<LittleEndian>()
            .map_err(|_| ShapefileError::unexpected_eof("reading num_parts"))?;
        let num_points = reader
            .read_i32::<LittleEndian>()
            .map_err(|_| ShapefileError::unexpected_eof("reading num_points"))?;

        if !(0..=1_000_000).contains(&num_parts) {
            return Err(ShapefileError::limit_exceeded(
                "num_parts out of range",
                1_000_000,
                num_parts as usize,
            ));
        }

        if !(0..=100_000_000).contains(&num_points) {
            return Err(ShapefileError::limit_exceeded(
                "num_points out of range",
                100_000_000,
                num_points as usize,
            ));
        }

        let mut parts = Vec::with_capacity(num_parts as usize);
        for _ in 0..num_parts {
            let part = reader
                .read_i32::<LittleEndian>()
                .map_err(|_| ShapefileError::unexpected_eof("reading part index"))?;
            parts.push(part);
        }

        let mut points = Vec::with_capacity(num_points as usize);
        for _ in 0..num_points {
            let point = Point::read(reader)?;
            points.push(point);
        }

        Ok(Self {
            bbox,
            num_parts,
            num_points,
            parts,
            points,
        })
    }

    /// Writes a multi-part shape to a writer
    pub fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        self.bbox.write(writer)?;

        writer
            .write_i32::<LittleEndian>(self.num_parts)
            .map_err(ShapefileError::Io)?;
        writer
            .write_i32::<LittleEndian>(self.num_points)
            .map_err(ShapefileError::Io)?;

        for part in &self.parts {
            writer
                .write_i32::<LittleEndian>(*part)
                .map_err(ShapefileError::Io)?;
        }

        for point in &self.points {
            point.write(writer)?;
        }

        Ok(())
    }
}

/// A multi-part shape with Z coordinates (PolyLineZ, PolygonZ, or MultiPointZ)
///
/// Binary layout (after shape type):
/// - Box2D (32 bytes: x_min, y_min, x_max, y_max)
/// - num_parts (4 bytes)
/// - num_points (4 bytes)
/// - parts array (num_parts * 4 bytes)
/// - points array (num_points * 16 bytes: x, y pairs)
/// - z_range (16 bytes: z_min, z_max)
/// - z_values array (num_points * 8 bytes)
/// - m_range (16 bytes: m_min, m_max) \[optional\]
/// - m_values array (num_points * 8 bytes) \[optional\]
#[derive(Debug, Clone, PartialEq)]
pub struct MultiPartShapeZ {
    /// Base 2D shape data (bbox, parts, points)
    pub base: MultiPartShape,
    /// Z coordinate range (min, max)
    pub z_range: (f64, f64),
    /// Z coordinate values for each point
    pub z_values: Vec<f64>,
    /// M value range (min, max), optional
    pub m_range: Option<(f64, f64)>,
    /// M values for each point, optional
    pub m_values: Option<Vec<f64>>,
}

impl MultiPartShapeZ {
    /// Creates a new multi-part shape with Z coordinates
    pub fn new(
        parts: Vec<i32>,
        points: Vec<Point>,
        z_values: Vec<f64>,
        m_values: Option<Vec<f64>>,
    ) -> Result<Self> {
        if z_values.len() != points.len() {
            return Err(ShapefileError::invalid_geometry(format!(
                "z_values length ({}) must match points length ({})",
                z_values.len(),
                points.len()
            )));
        }
        if let Some(ref mv) = m_values {
            if mv.len() != points.len() {
                return Err(ShapefileError::invalid_geometry(format!(
                    "m_values length ({}) must match points length ({})",
                    mv.len(),
                    points.len()
                )));
            }
        }

        let base = MultiPartShape::new(parts, points)?;

        let z_min = z_values.iter().copied().fold(f64::INFINITY, f64::min);
        let z_max = z_values.iter().copied().fold(f64::NEG_INFINITY, f64::max);

        let m_range = m_values.as_ref().map(|mv| {
            let m_min = mv.iter().copied().fold(f64::INFINITY, f64::min);
            let m_max = mv.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            (m_min, m_max)
        });

        Ok(Self {
            base,
            z_range: (z_min, z_max),
            z_values,
            m_range,
            m_values,
        })
    }

    /// Reads a multi-part shape with Z from a reader
    pub fn read<R: Read>(reader: &mut R) -> Result<Self> {
        // Read the base 2D multi-part shape
        let base = MultiPartShape::read(reader)?;

        // Read Z range
        let z_min = reader
            .read_f64::<LittleEndian>()
            .map_err(|_| ShapefileError::unexpected_eof("reading z range min"))?;
        let z_max = reader
            .read_f64::<LittleEndian>()
            .map_err(|_| ShapefileError::unexpected_eof("reading z range max"))?;

        // Read Z values
        let num_points = base.num_points as usize;
        let mut z_values = Vec::with_capacity(num_points);
        for _ in 0..num_points {
            let z = reader
                .read_f64::<LittleEndian>()
                .map_err(|_| ShapefileError::unexpected_eof("reading z value"))?;
            z_values.push(z);
        }

        // Try to read optional M range and values
        let (m_range, m_values) = match reader.read_f64::<LittleEndian>() {
            Ok(m_min) => {
                let m_max = reader
                    .read_f64::<LittleEndian>()
                    .map_err(|_| ShapefileError::unexpected_eof("reading m range max"))?;

                let mut mv = Vec::with_capacity(num_points);
                for _ in 0..num_points {
                    let m = reader
                        .read_f64::<LittleEndian>()
                        .map_err(|_| ShapefileError::unexpected_eof("reading m value"))?;
                    mv.push(m);
                }

                // Check if M values are "no data" (less than -1e38)
                if m_min < -1e38 {
                    (None, None)
                } else {
                    (Some((m_min, m_max)), Some(mv))
                }
            }
            Err(_) => (None, None),
        };

        Ok(Self {
            base,
            z_range: (z_min, z_max),
            z_values,
            m_range,
            m_values,
        })
    }

    /// Writes a multi-part shape with Z to a writer
    pub fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        // Write base 2D shape
        self.base.write(writer)?;

        // Write Z range
        writer
            .write_f64::<LittleEndian>(self.z_range.0)
            .map_err(ShapefileError::Io)?;
        writer
            .write_f64::<LittleEndian>(self.z_range.1)
            .map_err(ShapefileError::Io)?;

        // Write Z values
        for z in &self.z_values {
            writer
                .write_f64::<LittleEndian>(*z)
                .map_err(ShapefileError::Io)?;
        }

        // Write M range and values (if present)
        if let (Some((m_min, m_max)), Some(m_values)) = (self.m_range, &self.m_values) {
            writer
                .write_f64::<LittleEndian>(m_min)
                .map_err(ShapefileError::Io)?;
            writer
                .write_f64::<LittleEndian>(m_max)
                .map_err(ShapefileError::Io)?;
            for m in m_values {
                writer
                    .write_f64::<LittleEndian>(*m)
                    .map_err(ShapefileError::Io)?;
            }
        }

        Ok(())
    }

    /// Returns the content length in 16-bit words (excluding shape type)
    pub fn content_length_words(&self) -> i32 {
        // Base content: bbox(32) + num_parts(4) + num_points(4) + parts + points
        let base_bytes = 32 + 4 + 4 + (self.base.num_parts * 4) + (self.base.num_points * 16);
        // Z data: z_range(16) + z_values
        let z_bytes = 16 + (self.base.num_points * 8);
        // M data (optional): m_range(16) + m_values
        let m_bytes = if self.m_values.is_some() {
            16 + (self.base.num_points * 8)
        } else {
            0
        };
        (base_bytes + z_bytes + m_bytes) / 2
    }
}

/// A multi-part shape with M (measure) values (PolyLineM, PolygonM, or MultiPointM)
///
/// Binary layout (after shape type):
/// - Box2D (32 bytes: x_min, y_min, x_max, y_max)
/// - num_parts (4 bytes)
/// - num_points (4 bytes)
/// - parts array (num_parts * 4 bytes)
/// - points array (num_points * 16 bytes: x, y pairs)
/// - m_range (16 bytes: m_min, m_max)
/// - m_values array (num_points * 8 bytes)
#[derive(Debug, Clone, PartialEq)]
pub struct MultiPartShapeM {
    /// Base 2D shape data (bbox, parts, points)
    pub base: MultiPartShape,
    /// M value range (min, max)
    pub m_range: (f64, f64),
    /// M values for each point
    pub m_values: Vec<f64>,
}

impl MultiPartShapeM {
    /// Creates a new multi-part shape with M values
    pub fn new(parts: Vec<i32>, points: Vec<Point>, m_values: Vec<f64>) -> Result<Self> {
        if m_values.len() != points.len() {
            return Err(ShapefileError::invalid_geometry(format!(
                "m_values length ({}) must match points length ({})",
                m_values.len(),
                points.len()
            )));
        }

        let base = MultiPartShape::new(parts, points)?;

        let m_min = m_values.iter().copied().fold(f64::INFINITY, f64::min);
        let m_max = m_values.iter().copied().fold(f64::NEG_INFINITY, f64::max);

        Ok(Self {
            base,
            m_range: (m_min, m_max),
            m_values,
        })
    }

    /// Reads a multi-part shape with M from a reader
    pub fn read<R: Read>(reader: &mut R) -> Result<Self> {
        // Read the base 2D multi-part shape
        let base = MultiPartShape::read(reader)?;

        // Read M range
        let m_min = reader
            .read_f64::<LittleEndian>()
            .map_err(|_| ShapefileError::unexpected_eof("reading m range min"))?;
        let m_max = reader
            .read_f64::<LittleEndian>()
            .map_err(|_| ShapefileError::unexpected_eof("reading m range max"))?;

        // Read M values
        let num_points = base.num_points as usize;
        let mut m_values = Vec::with_capacity(num_points);
        for _ in 0..num_points {
            let m = reader
                .read_f64::<LittleEndian>()
                .map_err(|_| ShapefileError::unexpected_eof("reading m value"))?;
            m_values.push(m);
        }

        Ok(Self {
            base,
            m_range: (m_min, m_max),
            m_values,
        })
    }

    /// Writes a multi-part shape with M to a writer
    pub fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        // Write base 2D shape
        self.base.write(writer)?;

        // Write M range
        writer
            .write_f64::<LittleEndian>(self.m_range.0)
            .map_err(ShapefileError::Io)?;
        writer
            .write_f64::<LittleEndian>(self.m_range.1)
            .map_err(ShapefileError::Io)?;

        // Write M values
        for m in &self.m_values {
            writer
                .write_f64::<LittleEndian>(*m)
                .map_err(ShapefileError::Io)?;
        }

        Ok(())
    }

    /// Returns the content length in 16-bit words (excluding shape type)
    pub fn content_length_words(&self) -> i32 {
        // Base content: bbox(32) + num_parts(4) + num_points(4) + parts + points
        let base_bytes = 32 + 4 + 4 + (self.base.num_parts * 4) + (self.base.num_points * 16);
        // M data: m_range(16) + m_values
        let m_bytes = 16 + (self.base.num_points * 8);
        (base_bytes + m_bytes) / 2
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_shape_type_conversion() {
        assert_eq!(
            ShapeType::from_code(1).expect("valid shape type code 1"),
            ShapeType::Point
        );
        assert_eq!(ShapeType::Point.to_code(), 1);
        assert_eq!(
            ShapeType::from_code(11).expect("valid shape type code 11"),
            ShapeType::PointZ
        );
        assert!(ShapeType::from_code(999).is_err());
    }

    #[test]
    fn test_shape_type_properties() {
        assert!(ShapeType::PointZ.has_z());
        assert!(!ShapeType::Point.has_z());
        assert!(ShapeType::PointZ.has_m());
        assert!(ShapeType::PointM.has_m());
        assert!(!ShapeType::Point.has_m());
    }

    #[test]
    fn test_point_round_trip() {
        let point = Point::new(10.5, 20.3);
        let mut buffer = Vec::new();
        point.write(&mut buffer).expect("write point");

        let mut cursor = Cursor::new(buffer);
        let read_point = Point::read(&mut cursor).expect("read point");

        assert_eq!(read_point, point);
    }

    #[test]
    fn test_pointz_round_trip() {
        let point = PointZ::new_with_m(10.5, 20.3, 30.7, 100.0);
        let mut buffer = Vec::new();
        point.write(&mut buffer).expect("write pointz");

        let mut cursor = Cursor::new(buffer);
        let read_point = PointZ::read(&mut cursor).expect("read pointz");

        assert_eq!(read_point, point);
    }

    #[test]
    fn test_box2d_from_points() {
        let points = vec![
            Point::new(0.0, 0.0),
            Point::new(10.0, 20.0),
            Point::new(-5.0, 15.0),
        ];

        let bbox = Box2D::from_points(&points).expect("compute bbox from points");
        assert_eq!(bbox.x_min, -5.0);
        assert_eq!(bbox.y_min, 0.0);
        assert_eq!(bbox.x_max, 10.0);
        assert_eq!(bbox.y_max, 20.0);
    }

    #[test]
    fn test_invalid_coordinates() {
        let mut buffer = Vec::new();
        buffer
            .write_f64::<LittleEndian>(f64::NAN)
            .expect("write NAN coordinate");
        buffer
            .write_f64::<LittleEndian>(10.0)
            .expect("write valid coordinate");

        let mut cursor = Cursor::new(buffer);
        let result = Point::read(&mut cursor);
        assert!(result.is_err());
    }
}
