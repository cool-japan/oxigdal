//! Well-Known Binary (WKB) encoding and decoding for PostGIS geometries
//!
//! This module provides WKB encoding and decoding for all OxiGDAL geometry types.
//! PostGIS uses Extended WKB (EWKB) which includes SRID information.

use crate::error::{Result, WkbError};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use oxigdal_core::vector::geometry::*;
use std::io::Cursor;

/// WKB byte order marker
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteOrder {
    /// Big-endian (network byte order)
    BigEndian,
    /// Little-endian (x86 byte order)
    LittleEndian,
}

impl ByteOrder {
    /// Returns the byte order marker value
    pub const fn to_byte(self) -> u8 {
        match self {
            Self::BigEndian => 0x00,
            Self::LittleEndian => 0x01,
        }
    }

    /// Creates a byte order from a marker byte
    pub fn from_byte(byte: u8) -> Result<Self> {
        match byte {
            0x00 => Ok(Self::BigEndian),
            0x01 => Ok(Self::LittleEndian),
            _ => Err(WkbError::InvalidByteOrder { byte }.into()),
        }
    }
}

/// WKB geometry type codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum WkbGeometryType {
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

impl WkbGeometryType {
    /// Creates a WKB geometry type from a type code
    pub fn from_code(code: u32) -> Result<Self> {
        // Mask off the SRID and Z/M flags
        let base_code = code & 0xFF;
        match base_code {
            1 => Ok(Self::Point),
            2 => Ok(Self::LineString),
            3 => Ok(Self::Polygon),
            4 => Ok(Self::MultiPoint),
            5 => Ok(Self::MultiLineString),
            6 => Ok(Self::MultiPolygon),
            7 => Ok(Self::GeometryCollection),
            _ => Err(WkbError::UnsupportedGeometryType { type_code: code }.into()),
        }
    }

    /// Returns the type code value
    pub const fn to_code(self) -> u32 {
        self as u32
    }
}

/// WKB flags
const SRID_FLAG: u32 = 0x2000_0000;
const Z_FLAG: u32 = 0x8000_0000;
const M_FLAG: u32 = 0x4000_0000;

/// WKB encoder for geometries
pub struct WkbEncoder {
    buffer: Vec<u8>,
    byte_order: ByteOrder,
    srid: Option<i32>,
}

impl WkbEncoder {
    /// Creates a new WKB encoder
    pub fn new() -> Self {
        Self {
            buffer: Vec::new(),
            byte_order: ByteOrder::LittleEndian,
            srid: None,
        }
    }

    /// Creates a new WKB encoder with SRID
    pub fn with_srid(srid: i32) -> Self {
        Self {
            buffer: Vec::new(),
            byte_order: ByteOrder::LittleEndian,
            srid: Some(srid),
        }
    }

    /// Sets the byte order
    pub fn set_byte_order(&mut self, byte_order: ByteOrder) {
        self.byte_order = byte_order;
    }

    /// Encodes a geometry to WKB format
    pub fn encode(&mut self, geometry: &Geometry) -> Result<Vec<u8>> {
        self.buffer.clear();
        self.encode_geometry(geometry)?;
        Ok(self.buffer.clone())
    }

    fn encode_geometry(&mut self, geometry: &Geometry) -> Result<()> {
        match geometry {
            Geometry::Point(p) => self.encode_point(p),
            Geometry::LineString(ls) => self.encode_linestring(ls),
            Geometry::Polygon(p) => self.encode_polygon(p),
            Geometry::MultiPoint(mp) => self.encode_multipoint(mp),
            Geometry::MultiLineString(mls) => self.encode_multilinestring(mls),
            Geometry::MultiPolygon(mp) => self.encode_multipolygon(mp),
            Geometry::GeometryCollection(gc) => self.encode_geometrycollection(gc),
        }
    }

    fn write_header(&mut self, geom_type: WkbGeometryType, has_z: bool, has_m: bool) -> Result<()> {
        // Write byte order
        self.buffer
            .write_u8(self.byte_order.to_byte())
            .map_err(|e| WkbError::EncodingFailed {
                message: e.to_string(),
            })?;

        // Calculate type code with flags
        let mut type_code = geom_type.to_code();
        if self.srid.is_some() {
            type_code |= SRID_FLAG;
        }
        if has_z {
            type_code |= Z_FLAG;
        }
        if has_m {
            type_code |= M_FLAG;
        }

        // Write geometry type
        self.buffer
            .write_u32::<LittleEndian>(type_code)
            .map_err(|e| WkbError::EncodingFailed {
                message: e.to_string(),
            })?;

        // Write SRID if present
        if let Some(srid) = self.srid {
            self.buffer
                .write_i32::<LittleEndian>(srid)
                .map_err(|e| WkbError::EncodingFailed {
                    message: e.to_string(),
                })?;
        }

        Ok(())
    }

    fn encode_point(&mut self, point: &Point) -> Result<()> {
        let has_z = point.coord.has_z();
        let has_m = point.coord.has_m();

        self.write_header(WkbGeometryType::Point, has_z, has_m)?;
        self.encode_coordinate(&point.coord)?;
        Ok(())
    }

    fn encode_coordinate(&mut self, coord: &Coordinate) -> Result<()> {
        self.buffer
            .write_f64::<LittleEndian>(coord.x)
            .map_err(|e| WkbError::EncodingFailed {
                message: e.to_string(),
            })?;
        self.buffer
            .write_f64::<LittleEndian>(coord.y)
            .map_err(|e| WkbError::EncodingFailed {
                message: e.to_string(),
            })?;

        if let Some(z) = coord.z {
            self.buffer
                .write_f64::<LittleEndian>(z)
                .map_err(|e| WkbError::EncodingFailed {
                    message: e.to_string(),
                })?;
        }

        if let Some(m) = coord.m {
            self.buffer
                .write_f64::<LittleEndian>(m)
                .map_err(|e| WkbError::EncodingFailed {
                    message: e.to_string(),
                })?;
        }

        Ok(())
    }

    fn encode_linestring(&mut self, linestring: &LineString) -> Result<()> {
        let has_z = linestring.coords.first().is_some_and(|c| c.has_z());
        let has_m = linestring.coords.first().is_some_and(|c| c.has_m());

        self.write_header(WkbGeometryType::LineString, has_z, has_m)?;

        self.buffer
            .write_u32::<LittleEndian>(linestring.coords.len() as u32)
            .map_err(|e| WkbError::EncodingFailed {
                message: e.to_string(),
            })?;

        for coord in &linestring.coords {
            self.encode_coordinate(coord)?;
        }

        Ok(())
    }

    fn encode_polygon(&mut self, polygon: &Polygon) -> Result<()> {
        let has_z = polygon.exterior.coords.first().is_some_and(|c| c.has_z());
        let has_m = polygon.exterior.coords.first().is_some_and(|c| c.has_m());

        self.write_header(WkbGeometryType::Polygon, has_z, has_m)?;

        // Write number of rings
        let num_rings = 1 + polygon.interiors.len();
        self.buffer
            .write_u32::<LittleEndian>(num_rings as u32)
            .map_err(|e| WkbError::EncodingFailed {
                message: e.to_string(),
            })?;

        // Write exterior ring
        self.buffer
            .write_u32::<LittleEndian>(polygon.exterior.coords.len() as u32)
            .map_err(|e| WkbError::EncodingFailed {
                message: e.to_string(),
            })?;
        for coord in &polygon.exterior.coords {
            self.encode_coordinate(coord)?;
        }

        // Write interior rings
        for interior in &polygon.interiors {
            self.buffer
                .write_u32::<LittleEndian>(interior.coords.len() as u32)
                .map_err(|e| WkbError::EncodingFailed {
                    message: e.to_string(),
                })?;
            for coord in &interior.coords {
                self.encode_coordinate(coord)?;
            }
        }

        Ok(())
    }

    fn encode_multipoint(&mut self, multipoint: &MultiPoint) -> Result<()> {
        let has_z = multipoint.points.first().is_some_and(|p| p.coord.has_z());
        let has_m = multipoint.points.first().is_some_and(|p| p.coord.has_m());

        self.write_header(WkbGeometryType::MultiPoint, has_z, has_m)?;

        self.buffer
            .write_u32::<LittleEndian>(multipoint.points.len() as u32)
            .map_err(|e| WkbError::EncodingFailed {
                message: e.to_string(),
            })?;

        for point in &multipoint.points {
            self.encode_point(point)?;
        }

        Ok(())
    }

    fn encode_multilinestring(&mut self, multilinestring: &MultiLineString) -> Result<()> {
        let has_z = multilinestring
            .line_strings
            .first()
            .and_then(|ls| ls.coords.first())
            .is_some_and(|c| c.has_z());
        let has_m = multilinestring
            .line_strings
            .first()
            .and_then(|ls| ls.coords.first())
            .is_some_and(|c| c.has_m());

        self.write_header(WkbGeometryType::MultiLineString, has_z, has_m)?;

        self.buffer
            .write_u32::<LittleEndian>(multilinestring.line_strings.len() as u32)
            .map_err(|e| WkbError::EncodingFailed {
                message: e.to_string(),
            })?;

        for linestring in &multilinestring.line_strings {
            self.encode_linestring(linestring)?;
        }

        Ok(())
    }

    fn encode_multipolygon(&mut self, multipolygon: &MultiPolygon) -> Result<()> {
        let has_z = multipolygon
            .polygons
            .first()
            .and_then(|p| p.exterior.coords.first())
            .is_some_and(|c| c.has_z());
        let has_m = multipolygon
            .polygons
            .first()
            .and_then(|p| p.exterior.coords.first())
            .is_some_and(|c| c.has_m());

        self.write_header(WkbGeometryType::MultiPolygon, has_z, has_m)?;

        self.buffer
            .write_u32::<LittleEndian>(multipolygon.polygons.len() as u32)
            .map_err(|e| WkbError::EncodingFailed {
                message: e.to_string(),
            })?;

        for polygon in &multipolygon.polygons {
            self.encode_polygon(polygon)?;
        }

        Ok(())
    }

    fn encode_geometrycollection(&mut self, collection: &GeometryCollection) -> Result<()> {
        let has_z = collection.geometries.first().is_some_and(|g| match g {
            Geometry::Point(p) => p.coord.has_z(),
            Geometry::LineString(ls) => ls.coords.first().is_some_and(|c| c.has_z()),
            Geometry::Polygon(p) => p.exterior.coords.first().is_some_and(|c| c.has_z()),
            _ => false,
        });
        let has_m = collection.geometries.first().is_some_and(|g| match g {
            Geometry::Point(p) => p.coord.has_m(),
            Geometry::LineString(ls) => ls.coords.first().is_some_and(|c| c.has_m()),
            Geometry::Polygon(p) => p.exterior.coords.first().is_some_and(|c| c.has_m()),
            _ => false,
        });

        self.write_header(WkbGeometryType::GeometryCollection, has_z, has_m)?;

        self.buffer
            .write_u32::<LittleEndian>(collection.geometries.len() as u32)
            .map_err(|e| WkbError::EncodingFailed {
                message: e.to_string(),
            })?;

        for geometry in &collection.geometries {
            self.encode_geometry(geometry)?;
        }

        Ok(())
    }
}

impl Default for WkbEncoder {
    fn default() -> Self {
        Self::new()
    }
}

/// WKB decoder for geometries
pub struct WkbDecoder {
    srid: Option<i32>,
}

impl WkbDecoder {
    /// Creates a new WKB decoder
    pub fn new() -> Self {
        Self { srid: None }
    }

    /// Decodes a geometry from WKB format
    pub fn decode(&mut self, wkb: &[u8]) -> Result<Geometry> {
        if wkb.is_empty() {
            return Err(WkbError::InvalidFormat {
                message: "Empty WKB buffer".to_string(),
            }
            .into());
        }

        let mut cursor = Cursor::new(wkb);
        self.decode_geometry(&mut cursor)
    }

    /// Returns the SRID from the last decoded geometry
    pub const fn srid(&self) -> Option<i32> {
        self.srid
    }

    fn decode_geometry(&mut self, cursor: &mut Cursor<&[u8]>) -> Result<Geometry> {
        // Read byte order
        let byte_order = cursor.read_u8().map_err(|e| WkbError::DecodingFailed {
            message: e.to_string(),
        })?;
        let _byte_order = ByteOrder::from_byte(byte_order)?;

        // Read geometry type
        let type_code =
            cursor
                .read_u32::<LittleEndian>()
                .map_err(|e| WkbError::DecodingFailed {
                    message: e.to_string(),
                })?;

        // Extract flags
        let has_srid = (type_code & SRID_FLAG) != 0;
        let has_z = (type_code & Z_FLAG) != 0;
        let has_m = (type_code & M_FLAG) != 0;

        // Read SRID if present
        if has_srid {
            self.srid =
                Some(
                    cursor
                        .read_i32::<LittleEndian>()
                        .map_err(|e| WkbError::DecodingFailed {
                            message: e.to_string(),
                        })?,
                );
        }

        let geom_type = WkbGeometryType::from_code(type_code)?;

        match geom_type {
            WkbGeometryType::Point => Ok(Geometry::Point(self.decode_point(cursor, has_z, has_m)?)),
            WkbGeometryType::LineString => Ok(Geometry::LineString(
                self.decode_linestring(cursor, has_z, has_m)?,
            )),
            WkbGeometryType::Polygon => Ok(Geometry::Polygon(
                self.decode_polygon(cursor, has_z, has_m)?,
            )),
            WkbGeometryType::MultiPoint => {
                Ok(Geometry::MultiPoint(self.decode_multipoint(cursor)?))
            }
            WkbGeometryType::MultiLineString => Ok(Geometry::MultiLineString(
                self.decode_multilinestring(cursor)?,
            )),
            WkbGeometryType::MultiPolygon => {
                Ok(Geometry::MultiPolygon(self.decode_multipolygon(cursor)?))
            }
            WkbGeometryType::GeometryCollection => Ok(Geometry::GeometryCollection(
                self.decode_geometrycollection(cursor)?,
            )),
        }
    }

    fn decode_coordinate(
        &self,
        cursor: &mut Cursor<&[u8]>,
        has_z: bool,
        has_m: bool,
    ) -> Result<Coordinate> {
        let x = cursor
            .read_f64::<LittleEndian>()
            .map_err(|e| WkbError::DecodingFailed {
                message: e.to_string(),
            })?;
        let y = cursor
            .read_f64::<LittleEndian>()
            .map_err(|e| WkbError::DecodingFailed {
                message: e.to_string(),
            })?;

        let z = if has_z {
            Some(
                cursor
                    .read_f64::<LittleEndian>()
                    .map_err(|e| WkbError::DecodingFailed {
                        message: e.to_string(),
                    })?,
            )
        } else {
            None
        };

        let m = if has_m {
            Some(
                cursor
                    .read_f64::<LittleEndian>()
                    .map_err(|e| WkbError::DecodingFailed {
                        message: e.to_string(),
                    })?,
            )
        } else {
            None
        };

        Ok(Coordinate { x, y, z, m })
    }

    fn decode_point(&self, cursor: &mut Cursor<&[u8]>, has_z: bool, has_m: bool) -> Result<Point> {
        let coord = self.decode_coordinate(cursor, has_z, has_m)?;
        Ok(Point { coord })
    }

    fn decode_linestring(
        &self,
        cursor: &mut Cursor<&[u8]>,
        has_z: bool,
        has_m: bool,
    ) -> Result<LineString> {
        let num_points =
            cursor
                .read_u32::<LittleEndian>()
                .map_err(|e| WkbError::DecodingFailed {
                    message: e.to_string(),
                })?;

        let mut coords = Vec::with_capacity(num_points as usize);
        for _ in 0..num_points {
            coords.push(self.decode_coordinate(cursor, has_z, has_m)?);
        }

        LineString::new(coords).map_err(|e| e.into())
    }

    fn decode_polygon(
        &self,
        cursor: &mut Cursor<&[u8]>,
        has_z: bool,
        has_m: bool,
    ) -> Result<Polygon> {
        let num_rings =
            cursor
                .read_u32::<LittleEndian>()
                .map_err(|e| WkbError::DecodingFailed {
                    message: e.to_string(),
                })?;

        if num_rings == 0 {
            return Err(WkbError::InvalidRing {
                message: "Polygon must have at least one ring".to_string(),
            }
            .into());
        }

        // Read exterior ring
        let exterior = self.decode_ring(cursor, has_z, has_m)?;

        // Read interior rings
        let mut interiors = Vec::with_capacity((num_rings - 1) as usize);
        for _ in 1..num_rings {
            interiors.push(self.decode_ring(cursor, has_z, has_m)?);
        }

        Polygon::new(exterior, interiors).map_err(|e| e.into())
    }

    fn decode_ring(
        &self,
        cursor: &mut Cursor<&[u8]>,
        has_z: bool,
        has_m: bool,
    ) -> Result<LineString> {
        let num_points =
            cursor
                .read_u32::<LittleEndian>()
                .map_err(|e| WkbError::DecodingFailed {
                    message: e.to_string(),
                })?;

        let mut coords = Vec::with_capacity(num_points as usize);
        for _ in 0..num_points {
            coords.push(self.decode_coordinate(cursor, has_z, has_m)?);
        }

        Ok(LineString { coords })
    }

    fn decode_multipoint(&mut self, cursor: &mut Cursor<&[u8]>) -> Result<MultiPoint> {
        let num_points =
            cursor
                .read_u32::<LittleEndian>()
                .map_err(|e| WkbError::DecodingFailed {
                    message: e.to_string(),
                })?;

        let mut points = Vec::with_capacity(num_points as usize);
        for _ in 0..num_points {
            if let Geometry::Point(p) = self.decode_geometry(cursor)? {
                points.push(p);
            } else {
                return Err(WkbError::InvalidFormat {
                    message: "Expected Point in MultiPoint".to_string(),
                }
                .into());
            }
        }

        Ok(MultiPoint { points })
    }

    fn decode_multilinestring(&mut self, cursor: &mut Cursor<&[u8]>) -> Result<MultiLineString> {
        let num_linestrings =
            cursor
                .read_u32::<LittleEndian>()
                .map_err(|e| WkbError::DecodingFailed {
                    message: e.to_string(),
                })?;

        let mut line_strings = Vec::with_capacity(num_linestrings as usize);
        for _ in 0..num_linestrings {
            if let Geometry::LineString(ls) = self.decode_geometry(cursor)? {
                line_strings.push(ls);
            } else {
                return Err(WkbError::InvalidFormat {
                    message: "Expected LineString in MultiLineString".to_string(),
                }
                .into());
            }
        }

        Ok(MultiLineString { line_strings })
    }

    fn decode_multipolygon(&mut self, cursor: &mut Cursor<&[u8]>) -> Result<MultiPolygon> {
        let num_polygons =
            cursor
                .read_u32::<LittleEndian>()
                .map_err(|e| WkbError::DecodingFailed {
                    message: e.to_string(),
                })?;

        let mut polygons = Vec::with_capacity(num_polygons as usize);
        for _ in 0..num_polygons {
            if let Geometry::Polygon(p) = self.decode_geometry(cursor)? {
                polygons.push(p);
            } else {
                return Err(WkbError::InvalidFormat {
                    message: "Expected Polygon in MultiPolygon".to_string(),
                }
                .into());
            }
        }

        Ok(MultiPolygon { polygons })
    }

    fn decode_geometrycollection(
        &mut self,
        cursor: &mut Cursor<&[u8]>,
    ) -> Result<GeometryCollection> {
        let num_geometries =
            cursor
                .read_u32::<LittleEndian>()
                .map_err(|e| WkbError::DecodingFailed {
                    message: e.to_string(),
                })?;

        let mut geometries = Vec::with_capacity(num_geometries as usize);
        for _ in 0..num_geometries {
            geometries.push(self.decode_geometry(cursor)?);
        }

        Ok(GeometryCollection { geometries })
    }
}

impl Default for WkbDecoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_byte_order() {
        assert_eq!(ByteOrder::LittleEndian.to_byte(), 0x01);
        assert_eq!(ByteOrder::BigEndian.to_byte(), 0x00);
        assert_eq!(
            ByteOrder::from_byte(0x01).ok(),
            Some(ByteOrder::LittleEndian)
        );
        assert_eq!(ByteOrder::from_byte(0x00).ok(), Some(ByteOrder::BigEndian));
        assert!(ByteOrder::from_byte(0x02).is_err());
    }

    #[test]
    fn test_point_encode_decode() {
        let point = Point::new(1.0, 2.0);
        let mut encoder = WkbEncoder::new();
        let wkb = encoder.encode(&Geometry::Point(point.clone())).ok();
        assert!(wkb.is_some());

        let wkb = wkb.expect("encoding failed");
        let mut decoder = WkbDecoder::new();
        let decoded = decoder.decode(&wkb).ok();
        assert!(decoded.is_some());

        if let Some(Geometry::Point(decoded_point)) = decoded {
            assert_eq!(decoded_point.coord.x, point.coord.x);
            assert_eq!(decoded_point.coord.y, point.coord.y);
        } else {
            panic!("Expected Point geometry");
        }
    }

    #[test]
    fn test_point_3d_encode_decode() {
        let point = Point::new_3d(1.0, 2.0, 3.0);
        let mut encoder = WkbEncoder::new();
        let wkb = encoder.encode(&Geometry::Point(point.clone())).ok();
        assert!(wkb.is_some());

        let wkb = wkb.expect("encoding failed");
        let mut decoder = WkbDecoder::new();
        let decoded = decoder.decode(&wkb).ok();
        assert!(decoded.is_some());

        if let Some(Geometry::Point(decoded_point)) = decoded {
            assert_eq!(decoded_point.coord.x, point.coord.x);
            assert_eq!(decoded_point.coord.y, point.coord.y);
            assert_eq!(decoded_point.coord.z, point.coord.z);
        } else {
            panic!("Expected Point geometry");
        }
    }

    #[test]
    fn test_linestring_encode_decode() {
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(2.0, 0.0),
        ];
        let linestring = LineString::new(coords).ok();
        assert!(linestring.is_some());
        let linestring = linestring.expect("linestring creation failed");

        let mut encoder = WkbEncoder::new();
        let wkb = encoder
            .encode(&Geometry::LineString(linestring.clone()))
            .ok();
        assert!(wkb.is_some());

        let wkb = wkb.expect("encoding failed");
        let mut decoder = WkbDecoder::new();
        let decoded = decoder.decode(&wkb).ok();
        assert!(decoded.is_some());

        if let Some(Geometry::LineString(decoded_ls)) = decoded {
            assert_eq!(decoded_ls.coords.len(), linestring.coords.len());
        } else {
            panic!("Expected LineString geometry");
        }
    }

    #[test]
    fn test_polygon_encode_decode() {
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

        let polygon = Polygon::new(exterior, vec![]);
        assert!(polygon.is_ok());
        let polygon = polygon.expect("polygon creation failed");

        let mut encoder = WkbEncoder::new();
        let wkb = encoder.encode(&Geometry::Polygon(polygon.clone())).ok();
        assert!(wkb.is_some());

        let wkb = wkb.expect("encoding failed");
        let mut decoder = WkbDecoder::new();
        let decoded = decoder.decode(&wkb).ok();
        assert!(decoded.is_some());

        if let Some(Geometry::Polygon(decoded_poly)) = decoded {
            assert_eq!(
                decoded_poly.exterior.coords.len(),
                polygon.exterior.coords.len()
            );
        } else {
            panic!("Expected Polygon geometry");
        }
    }

    #[test]
    fn test_wkb_with_srid() {
        let point = Point::new(1.0, 2.0);
        let mut encoder = WkbEncoder::with_srid(4326);
        let wkb = encoder.encode(&Geometry::Point(point)).ok();
        assert!(wkb.is_some());

        let wkb = wkb.expect("encoding failed");
        let mut decoder = WkbDecoder::new();
        let decoded = decoder.decode(&wkb).ok();
        assert!(decoded.is_some());
        assert_eq!(decoder.srid(), Some(4326));
    }
}
