//! Geometry encoding and decoding for `FlatGeobuf` format
//!
//! Geometries in `FlatGeobuf` are stored as sequences of coordinates with
//! a type indicator. This module handles conversion between `OxiGDAL` geometry
//! types and `FlatGeobuf` binary encoding.

use crate::error::{FlatGeobufError, Result};
use crate::header::GeometryType;
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use oxigdal_core::vector::{
    Coordinate, Geometry, GeometryCollection, LineString, MultiLineString, MultiPoint,
    MultiPolygon, Point, Polygon,
};
use std::io::{Read, Write};

/// Geometry encoder/decoder
pub struct GeometryCodec {
    has_z: bool,
    has_m: bool,
}

impl GeometryCodec {
    /// Creates a new geometry codec
    #[must_use]
    pub const fn new(has_z: bool, has_m: bool) -> Self {
        Self { has_z, has_m }
    }

    /// Encodes a geometry to bytes
    pub fn encode<W: Write>(&self, writer: &mut W, geometry: &Geometry) -> Result<()> {
        match geometry {
            Geometry::Point(p) => self.encode_point(writer, p),
            Geometry::LineString(ls) => self.encode_linestring(writer, ls),
            Geometry::Polygon(p) => self.encode_polygon(writer, p),
            Geometry::MultiPoint(mp) => self.encode_multipoint(writer, mp),
            Geometry::MultiLineString(mls) => self.encode_multilinestring(writer, mls),
            Geometry::MultiPolygon(mp) => self.encode_multipolygon(writer, mp),
            Geometry::GeometryCollection(gc) => self.encode_geometry_collection(writer, gc),
        }
    }

    /// Decodes a geometry from bytes
    pub fn decode<R: Read>(&self, reader: &mut R, geom_type: GeometryType) -> Result<Geometry> {
        match geom_type {
            GeometryType::Point => Ok(Geometry::Point(self.decode_point(reader)?)),
            GeometryType::LineString => Ok(Geometry::LineString(self.decode_linestring(reader)?)),
            GeometryType::Polygon => Ok(Geometry::Polygon(self.decode_polygon(reader)?)),
            GeometryType::MultiPoint => Ok(Geometry::MultiPoint(self.decode_multipoint(reader)?)),
            GeometryType::MultiLineString => Ok(Geometry::MultiLineString(
                self.decode_multilinestring(reader)?,
            )),
            GeometryType::MultiPolygon => {
                Ok(Geometry::MultiPolygon(self.decode_multipolygon(reader)?))
            }
            GeometryType::GeometryCollection => Ok(Geometry::GeometryCollection(
                self.decode_geometry_collection(reader)?,
            )),
            _ => Err(FlatGeobufError::UnsupportedGeometryType(geom_type as u8)),
        }
    }

    /// Encodes a coordinate
    fn encode_coordinate<W: Write>(&self, writer: &mut W, coord: &Coordinate) -> Result<()> {
        writer.write_f64::<LittleEndian>(coord.x)?;
        writer.write_f64::<LittleEndian>(coord.y)?;

        if self.has_z {
            writer.write_f64::<LittleEndian>(coord.z.unwrap_or(0.0))?;
        }

        if self.has_m {
            writer.write_f64::<LittleEndian>(coord.m.unwrap_or(0.0))?;
        }

        Ok(())
    }

    /// Decodes a coordinate
    fn decode_coordinate<R: Read>(&self, reader: &mut R) -> Result<Coordinate> {
        let x = reader.read_f64::<LittleEndian>()?;
        let y = reader.read_f64::<LittleEndian>()?;

        let z = if self.has_z {
            Some(reader.read_f64::<LittleEndian>()?)
        } else {
            None
        };

        let m = if self.has_m {
            Some(reader.read_f64::<LittleEndian>()?)
        } else {
            None
        };

        Ok(Coordinate { x, y, z, m })
    }

    /// Encodes a point
    fn encode_point<W: Write>(&self, writer: &mut W, point: &Point) -> Result<()> {
        self.encode_coordinate(writer, &point.coord)
    }

    /// Decodes a point
    fn decode_point<R: Read>(&self, reader: &mut R) -> Result<Point> {
        let coord = self.decode_coordinate(reader)?;
        Ok(Point::from_coord(coord))
    }

    /// Encodes a linestring
    fn encode_linestring<W: Write>(&self, writer: &mut W, linestring: &LineString) -> Result<()> {
        writer.write_u32::<LittleEndian>(linestring.coords.len() as u32)?;
        for coord in &linestring.coords {
            self.encode_coordinate(writer, coord)?;
        }
        Ok(())
    }

    /// Decodes a linestring
    fn decode_linestring<R: Read>(&self, reader: &mut R) -> Result<LineString> {
        let count = reader.read_u32::<LittleEndian>()?;
        let mut coords = Vec::with_capacity(count as usize);

        for _ in 0..count {
            coords.push(self.decode_coordinate(reader)?);
        }

        if coords.len() < 2 {
            return Err(FlatGeobufError::InvalidGeometry(
                "LineString must have at least 2 coordinates".to_string(),
            ));
        }

        LineString::new(coords).map_err(FlatGeobufError::OxiGdal)
    }

    /// Encodes a polygon
    fn encode_polygon<W: Write>(&self, writer: &mut W, polygon: &Polygon) -> Result<()> {
        // Number of rings
        writer.write_u32::<LittleEndian>((1 + polygon.interiors.len()) as u32)?;

        // Exterior ring
        self.encode_linestring(writer, &polygon.exterior)?;

        // Interior rings
        for interior in &polygon.interiors {
            self.encode_linestring(writer, interior)?;
        }

        Ok(())
    }

    /// Decodes a polygon
    fn decode_polygon<R: Read>(&self, reader: &mut R) -> Result<Polygon> {
        let ring_count = reader.read_u32::<LittleEndian>()?;

        if ring_count == 0 {
            return Err(FlatGeobufError::InvalidGeometry(
                "Polygon must have at least 1 ring".to_string(),
            ));
        }

        let exterior = self.decode_linestring(reader)?;

        let mut interiors = Vec::with_capacity(ring_count.saturating_sub(1) as usize);
        for _ in 1..ring_count {
            interiors.push(self.decode_linestring(reader)?);
        }

        Polygon::new(exterior, interiors).map_err(FlatGeobufError::OxiGdal)
    }

    /// Encodes a multipoint
    fn encode_multipoint<W: Write>(&self, writer: &mut W, multipoint: &MultiPoint) -> Result<()> {
        writer.write_u32::<LittleEndian>(multipoint.points.len() as u32)?;
        for point in &multipoint.points {
            self.encode_coordinate(writer, &point.coord)?;
        }
        Ok(())
    }

    /// Decodes a multipoint
    fn decode_multipoint<R: Read>(&self, reader: &mut R) -> Result<MultiPoint> {
        let count = reader.read_u32::<LittleEndian>()?;
        let mut points = Vec::with_capacity(count as usize);

        for _ in 0..count {
            let coord = self.decode_coordinate(reader)?;
            points.push(Point::from_coord(coord));
        }

        Ok(MultiPoint::new(points))
    }

    /// Encodes a multilinestring
    fn encode_multilinestring<W: Write>(
        &self,
        writer: &mut W,
        multilinestring: &MultiLineString,
    ) -> Result<()> {
        writer.write_u32::<LittleEndian>(multilinestring.line_strings.len() as u32)?;
        for linestring in &multilinestring.line_strings {
            self.encode_linestring(writer, linestring)?;
        }
        Ok(())
    }

    /// Decodes a multilinestring
    fn decode_multilinestring<R: Read>(&self, reader: &mut R) -> Result<MultiLineString> {
        let count = reader.read_u32::<LittleEndian>()?;
        let mut line_strings = Vec::with_capacity(count as usize);

        for _ in 0..count {
            line_strings.push(self.decode_linestring(reader)?);
        }

        Ok(MultiLineString::new(line_strings))
    }

    /// Encodes a multipolygon
    fn encode_multipolygon<W: Write>(
        &self,
        writer: &mut W,
        multipolygon: &MultiPolygon,
    ) -> Result<()> {
        writer.write_u32::<LittleEndian>(multipolygon.polygons.len() as u32)?;
        for polygon in &multipolygon.polygons {
            self.encode_polygon(writer, polygon)?;
        }
        Ok(())
    }

    /// Decodes a multipolygon
    fn decode_multipolygon<R: Read>(&self, reader: &mut R) -> Result<MultiPolygon> {
        let count = reader.read_u32::<LittleEndian>()?;
        let mut polygons = Vec::with_capacity(count as usize);

        for _ in 0..count {
            polygons.push(self.decode_polygon(reader)?);
        }

        Ok(MultiPolygon::new(polygons))
    }

    /// Encodes a geometry collection
    fn encode_geometry_collection<W: Write>(
        &self,
        writer: &mut W,
        collection: &GeometryCollection,
    ) -> Result<()> {
        writer.write_u32::<LittleEndian>(collection.geometries.len() as u32)?;
        for geometry in &collection.geometries {
            // Write geometry type
            let geom_type = match geometry {
                Geometry::Point(_) => GeometryType::Point,
                Geometry::LineString(_) => GeometryType::LineString,
                Geometry::Polygon(_) => GeometryType::Polygon,
                Geometry::MultiPoint(_) => GeometryType::MultiPoint,
                Geometry::MultiLineString(_) => GeometryType::MultiLineString,
                Geometry::MultiPolygon(_) => GeometryType::MultiPolygon,
                Geometry::GeometryCollection(_) => GeometryType::GeometryCollection,
            };
            writer.write_u8(geom_type as u8)?;

            // Write geometry
            self.encode(writer, geometry)?;
        }
        Ok(())
    }

    /// Decodes a geometry collection
    fn decode_geometry_collection<R: Read>(&self, reader: &mut R) -> Result<GeometryCollection> {
        let count = reader.read_u32::<LittleEndian>()?;
        let mut geometries = Vec::with_capacity(count as usize);

        for _ in 0..count {
            let geom_type = GeometryType::from_u8(reader.read_u8()?)?;
            geometries.push(self.decode(reader, geom_type)?);
        }

        Ok(GeometryCollection::new(geometries))
    }

    /// Calculates the byte size needed to encode a geometry
    #[must_use]
    pub fn encoded_size(&self, geometry: &Geometry) -> usize {
        match geometry {
            Geometry::Point(_) => self.coord_size(),
            Geometry::LineString(ls) => 4 + ls.coords.len() * self.coord_size(),
            Geometry::Polygon(p) => {
                let mut size = 4; // ring count
                size += 4 + p.exterior.coords.len() * self.coord_size();
                for interior in &p.interiors {
                    size += 4 + interior.coords.len() * self.coord_size();
                }
                size
            }
            Geometry::MultiPoint(mp) => 4 + mp.points.len() * self.coord_size(),
            Geometry::MultiLineString(mls) => {
                let mut size = 4; // linestring count
                for ls in &mls.line_strings {
                    size += 4 + ls.coords.len() * self.coord_size();
                }
                size
            }
            Geometry::MultiPolygon(mp) => {
                let mut size = 4; // polygon count
                for p in &mp.polygons {
                    size += 4; // ring count
                    size += 4 + p.exterior.coords.len() * self.coord_size();
                    for interior in &p.interiors {
                        size += 4 + interior.coords.len() * self.coord_size();
                    }
                }
                size
            }
            Geometry::GeometryCollection(gc) => {
                let mut size = 4; // geometry count
                for geom in &gc.geometries {
                    size += 1; // geometry type
                    size += self.encoded_size(geom);
                }
                size
            }
        }
    }

    /// Returns the size of a single coordinate in bytes
    #[must_use]
    const fn coord_size(&self) -> usize {
        let mut size = 16; // x, y (2 * 8 bytes)
        if self.has_z {
            size += 8;
        }
        if self.has_m {
            size += 8;
        }
        size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_point() {
        let codec = GeometryCodec::new(false, false);
        let point = Point::new(10.0, 20.0);

        let mut buffer = Vec::new();
        codec.encode_point(&mut buffer, &point).ok();

        let mut cursor = std::io::Cursor::new(buffer);
        let decoded = codec.decode_point(&mut cursor).ok();

        assert!(decoded.is_some());
        let decoded = decoded.expect("decode failed");
        assert_eq!(decoded.coord.x, 10.0);
        assert_eq!(decoded.coord.y, 20.0);
    }

    #[test]
    fn test_encode_decode_point_3d() {
        let codec = GeometryCodec::new(true, false);
        let point = Point::new_3d(10.0, 20.0, 30.0);

        let mut buffer = Vec::new();
        codec.encode_point(&mut buffer, &point).ok();

        let mut cursor = std::io::Cursor::new(buffer);
        let decoded = codec.decode_point(&mut cursor).ok();

        assert!(decoded.is_some());
        let decoded = decoded.expect("decode failed");
        assert_eq!(decoded.coord.x, 10.0);
        assert_eq!(decoded.coord.y, 20.0);
        assert_eq!(decoded.coord.z, Some(30.0));
    }

    #[test]
    fn test_encode_decode_linestring() {
        let codec = GeometryCodec::new(false, false);
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(2.0, 0.0),
        ];
        let linestring = LineString::new(coords).ok();
        assert!(linestring.is_some());
        let linestring = linestring.expect("linestring creation failed");

        let mut buffer = Vec::new();
        codec.encode_linestring(&mut buffer, &linestring).ok();

        let mut cursor = std::io::Cursor::new(buffer);
        let decoded = codec.decode_linestring(&mut cursor).ok();

        assert!(decoded.is_some());
        let decoded = decoded.expect("decode failed");
        assert_eq!(decoded.len(), 3);
    }

    #[test]
    fn test_coord_size() {
        let codec_2d = GeometryCodec::new(false, false);
        assert_eq!(codec_2d.coord_size(), 16);

        let codec_3d = GeometryCodec::new(true, false);
        assert_eq!(codec_3d.coord_size(), 24);

        let codec_2dm = GeometryCodec::new(false, true);
        assert_eq!(codec_2dm.coord_size(), 24);

        let codec_4d = GeometryCodec::new(true, true);
        assert_eq!(codec_4d.coord_size(), 32);
    }
}
