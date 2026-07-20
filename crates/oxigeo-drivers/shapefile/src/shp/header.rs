//! Shapefile header parsing
//!
//! This module handles parsing and writing the Shapefile main file header (100 bytes).
//! The header contains metadata about the file including file code, version, shape type,
//! and bounding box information.
//!
//! # Shapefile Header Structure (100 bytes)
//!
//! | Bytes | Type | Endian | Description |
//! |-------|------|--------|-------------|
//! | 0-3   | i32  | Big    | File Code (9994) |
//! | 4-23  | -    | -      | Unused (zeros) |
//! | 24-27 | i32  | Big    | File Length (in 16-bit words) |
//! | 28-31 | i32  | Little | Version (1000) |
//! | 32-35 | i32  | Little | Shape Type |
//! | 36-67 | f64  | Little | Bounding Box (Xmin, Ymin, Xmax, Ymax) |
//! | 68-99 | f64  | Little | Z/M ranges (Zmin, Zmax, Mmin, Mmax) |

use crate::error::{Result, ShapefileError};
use crate::shp::shapes::ShapeType;
use byteorder::{BigEndian, LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Read, Write};

/// Shapefile file code (magic number)
pub const FILE_CODE: i32 = 9994;

/// Shapefile version
pub const VERSION: i32 = 1000;

/// Shapefile header size in bytes
pub const HEADER_SIZE: usize = 100;

/// Shapefile header
#[derive(Debug, Clone, PartialEq)]
pub struct ShapefileHeader {
    /// File code (should be 9994)
    pub file_code: i32,
    /// File length in 16-bit words (including header)
    pub file_length: i32,
    /// Version (should be 1000)
    pub version: i32,
    /// Shape type
    pub shape_type: ShapeType,
    /// Bounding box
    pub bbox: BoundingBox,
}

/// Bounding box with optional Z and M ranges
#[derive(Debug, Clone, PartialEq)]
pub struct BoundingBox {
    /// Minimum X
    pub x_min: f64,
    /// Minimum Y
    pub y_min: f64,
    /// Maximum X
    pub x_max: f64,
    /// Maximum Y
    pub y_max: f64,
    /// Minimum Z (if present)
    pub z_min: Option<f64>,
    /// Maximum Z (if present)
    pub z_max: Option<f64>,
    /// Minimum M (if present)
    pub m_min: Option<f64>,
    /// Maximum M (if present)
    pub m_max: Option<f64>,
}

impl BoundingBox {
    /// Creates a new 2D bounding box
    pub fn new_2d(x_min: f64, y_min: f64, x_max: f64, y_max: f64) -> Result<Self> {
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
            z_min: None,
            z_max: None,
            m_min: None,
            m_max: None,
        })
    }

    /// Creates a new 3D bounding box with Z range
    pub fn new_3d(
        x_min: f64,
        y_min: f64,
        x_max: f64,
        y_max: f64,
        z_min: f64,
        z_max: f64,
    ) -> Result<Self> {
        let mut bbox = Self::new_2d(x_min, y_min, x_max, y_max)?;
        if z_min > z_max {
            return Err(ShapefileError::InvalidBbox {
                message: format!("z_min ({}) > z_max ({})", z_min, z_max),
            });
        }
        bbox.z_min = Some(z_min);
        bbox.z_max = Some(z_max);
        Ok(bbox)
    }

    /// Checks if Z values are valid (not NaN/Inf for optional Z values)
    #[allow(dead_code)]
    fn has_valid_z(&self) -> bool {
        match (self.z_min, self.z_max) {
            (Some(z_min), Some(z_max)) => z_min.is_finite() && z_max.is_finite(),
            (None, None) => true,
            _ => false,
        }
    }

    /// Checks if M values are valid (not NaN/Inf for optional M values)
    #[allow(dead_code)]
    fn has_valid_m(&self) -> bool {
        match (self.m_min, self.m_max) {
            (Some(m_min), Some(m_max)) => m_min.is_finite() && m_max.is_finite(),
            (None, None) => true,
            _ => false,
        }
    }
}

impl ShapefileHeader {
    /// Creates a new Shapefile header
    pub fn new(shape_type: ShapeType, bbox: BoundingBox) -> Self {
        Self {
            file_code: FILE_CODE,
            file_length: 50, // Will be updated when writing
            version: VERSION,
            shape_type,
            bbox,
        }
    }

    /// Reads a Shapefile header from a reader
    pub fn read<R: Read>(reader: &mut R) -> Result<Self> {
        // Read file code (big endian)
        let file_code = reader
            .read_i32::<BigEndian>()
            .map_err(|_| ShapefileError::unexpected_eof("reading file code"))?;

        if file_code != FILE_CODE {
            return Err(ShapefileError::InvalidFileCode { actual: file_code });
        }

        // Skip unused bytes (20 bytes)
        let mut unused = [0u8; 20];
        reader
            .read_exact(&mut unused)
            .map_err(|_| ShapefileError::unexpected_eof("reading unused header bytes"))?;

        // Read file length (big endian, in 16-bit words)
        let file_length = reader
            .read_i32::<BigEndian>()
            .map_err(|_| ShapefileError::unexpected_eof("reading file length"))?;

        // Read version (little endian)
        let version = reader
            .read_i32::<LittleEndian>()
            .map_err(|_| ShapefileError::unexpected_eof("reading version"))?;

        if version != VERSION {
            return Err(ShapefileError::InvalidVersion { version });
        }

        // Read shape type (little endian)
        let shape_type_code = reader
            .read_i32::<LittleEndian>()
            .map_err(|_| ShapefileError::unexpected_eof("reading shape type"))?;

        let shape_type = ShapeType::from_code(shape_type_code)?;

        // Read bounding box (little endian)
        let x_min = reader
            .read_f64::<LittleEndian>()
            .map_err(|_| ShapefileError::unexpected_eof("reading x_min"))?;
        let y_min = reader
            .read_f64::<LittleEndian>()
            .map_err(|_| ShapefileError::unexpected_eof("reading y_min"))?;
        let x_max = reader
            .read_f64::<LittleEndian>()
            .map_err(|_| ShapefileError::unexpected_eof("reading x_max"))?;
        let y_max = reader
            .read_f64::<LittleEndian>()
            .map_err(|_| ShapefileError::unexpected_eof("reading y_max"))?;

        // Read Z range (little endian)
        let z_min = reader
            .read_f64::<LittleEndian>()
            .map_err(|_| ShapefileError::unexpected_eof("reading z_min"))?;
        let z_max = reader
            .read_f64::<LittleEndian>()
            .map_err(|_| ShapefileError::unexpected_eof("reading z_max"))?;

        // Read M range (little endian)
        let m_min = reader
            .read_f64::<LittleEndian>()
            .map_err(|_| ShapefileError::unexpected_eof("reading m_min"))?;
        let m_max = reader
            .read_f64::<LittleEndian>()
            .map_err(|_| ShapefileError::unexpected_eof("reading m_max"))?;

        // Build bounding box
        let mut bbox = BoundingBox::new_2d(x_min, y_min, x_max, y_max)?;

        // Add Z range if valid (not NaN or very large negative number used as "no data")
        if z_min.is_finite() && z_max.is_finite() && z_min > -1e38 {
            bbox.z_min = Some(z_min);
            bbox.z_max = Some(z_max);
        }

        // Add M range if valid
        if m_min.is_finite() && m_max.is_finite() && m_min > -1e38 {
            bbox.m_min = Some(m_min);
            bbox.m_max = Some(m_max);
        }

        Ok(Self {
            file_code,
            file_length,
            version,
            shape_type,
            bbox,
        })
    }

    /// Writes a Shapefile header to a writer
    pub fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        // Write file code (big endian)
        writer
            .write_i32::<BigEndian>(self.file_code)
            .map_err(ShapefileError::Io)?;

        // Write unused bytes (20 bytes of zeros)
        writer.write_all(&[0u8; 20]).map_err(ShapefileError::Io)?;

        // Write file length (big endian, in 16-bit words)
        writer
            .write_i32::<BigEndian>(self.file_length)
            .map_err(ShapefileError::Io)?;

        // Write version (little endian)
        writer
            .write_i32::<LittleEndian>(self.version)
            .map_err(ShapefileError::Io)?;

        // Write shape type (little endian)
        writer
            .write_i32::<LittleEndian>(self.shape_type.to_code())
            .map_err(ShapefileError::Io)?;

        // Write bounding box (little endian)
        writer
            .write_f64::<LittleEndian>(self.bbox.x_min)
            .map_err(ShapefileError::Io)?;
        writer
            .write_f64::<LittleEndian>(self.bbox.y_min)
            .map_err(ShapefileError::Io)?;
        writer
            .write_f64::<LittleEndian>(self.bbox.x_max)
            .map_err(ShapefileError::Io)?;
        writer
            .write_f64::<LittleEndian>(self.bbox.y_max)
            .map_err(ShapefileError::Io)?;

        // Write Z range (little endian)
        let z_min = self.bbox.z_min.unwrap_or(0.0);
        let z_max = self.bbox.z_max.unwrap_or(0.0);
        writer
            .write_f64::<LittleEndian>(z_min)
            .map_err(ShapefileError::Io)?;
        writer
            .write_f64::<LittleEndian>(z_max)
            .map_err(ShapefileError::Io)?;

        // Write M range (little endian)
        let m_min = self.bbox.m_min.unwrap_or(0.0);
        let m_max = self.bbox.m_max.unwrap_or(0.0);
        writer
            .write_f64::<LittleEndian>(m_min)
            .map_err(ShapefileError::Io)?;
        writer
            .write_f64::<LittleEndian>(m_max)
            .map_err(ShapefileError::Io)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_bounding_box_2d() {
        let bbox = BoundingBox::new_2d(-180.0, -90.0, 180.0, 90.0);
        assert!(bbox.is_ok());
        let bbox = bbox.expect("valid 2d bounding box");
        assert_eq!(bbox.x_min, -180.0);
        assert_eq!(bbox.y_max, 90.0);
        assert!(bbox.z_min.is_none());
    }

    #[test]
    fn test_bounding_box_invalid() {
        let bbox = BoundingBox::new_2d(180.0, -90.0, -180.0, 90.0);
        assert!(bbox.is_err());
    }

    #[test]
    fn test_header_round_trip() {
        let bbox = BoundingBox::new_2d(-180.0, -90.0, 180.0, 90.0).expect("valid bounding box");
        let header = ShapefileHeader::new(ShapeType::Point, bbox);

        let mut buffer = Vec::new();
        header.write(&mut buffer).expect("write header to buffer");

        assert_eq!(buffer.len(), HEADER_SIZE);

        let mut cursor = Cursor::new(buffer);
        let read_header = ShapefileHeader::read(&mut cursor).expect("read header from cursor");

        assert_eq!(read_header.file_code, FILE_CODE);
        assert_eq!(read_header.version, VERSION);
        assert_eq!(read_header.shape_type, ShapeType::Point);
        assert_eq!(read_header.bbox.x_min, -180.0);
    }

    #[test]
    fn test_invalid_file_code() {
        let mut buffer = vec![0u8; HEADER_SIZE];
        // Write wrong file code
        let mut cursor = Cursor::new(&mut buffer);
        cursor
            .write_i32::<BigEndian>(1234)
            .expect("write invalid file code to cursor");

        let mut cursor = Cursor::new(&buffer[..]);
        let result = ShapefileHeader::read(&mut cursor);
        assert!(result.is_err());
    }
}
