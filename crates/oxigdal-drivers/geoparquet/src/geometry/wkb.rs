//! WKB (Well-Known Binary) encoding and decoding
//!
//! This module implements the OGC Simple Features WKB specification
//! for encoding and decoding geometries.

use crate::error::{GeoParquetError, Result};
use crate::geometry::{
    Coordinate, Geometry, GeometryCollection, GeometryType, LineString, MultiLineString,
    MultiPoint, MultiPolygon, Point, Polygon,
};
use byteorder::{BigEndian, LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::Cursor;

/// WKB byte order marker
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WkbByteOrder {
    /// Big endian (XDR)
    BigEndian = 0,
    /// Little endian (NDR)
    LittleEndian = 1,
}

impl WkbByteOrder {
    /// Creates from byte value
    pub fn from_byte(byte: u8) -> Result<Self> {
        match byte {
            0 => Ok(Self::BigEndian),
            1 => Ok(Self::LittleEndian),
            other => Err(GeoParquetError::invalid_wkb(format!(
                "Invalid byte order marker: {other}"
            ))),
        }
    }

    /// Returns true if this is little endian
    pub const fn is_little_endian(self) -> bool {
        matches!(self, Self::LittleEndian)
    }
}

/// WKB writer for encoding geometries
pub struct WkbWriter {
    buffer: Vec<u8>,
    little_endian: bool,
}

impl WkbWriter {
    /// Creates a new WKB writer with the specified byte order
    pub fn new(little_endian: bool) -> Self {
        Self {
            buffer: Vec::new(),
            little_endian,
        }
    }

    /// Writes a geometry to WKB format
    pub fn write_geometry(&mut self, geom: &Geometry) -> Result<Vec<u8>> {
        self.buffer.clear();
        self.write_geometry_impl(geom)?;
        Ok(self.buffer.clone())
    }

    /// Internal implementation of geometry writing
    fn write_geometry_impl(&mut self, geom: &Geometry) -> Result<()> {
        match geom {
            Geometry::Point(p) => self.write_point(p),
            Geometry::LineString(ls) => self.write_linestring(ls),
            Geometry::Polygon(poly) => self.write_polygon(poly),
            Geometry::MultiPoint(mp) => self.write_multipoint(mp),
            Geometry::MultiLineString(mls) => self.write_multilinestring(mls),
            Geometry::MultiPolygon(mpoly) => self.write_multipolygon(mpoly),
            Geometry::GeometryCollection(gc) => self.write_geometrycollection(gc),
        }
    }

    /// Writes a point
    fn write_point(&mut self, point: &Point) -> Result<()> {
        self.write_byte_order()?;
        self.write_geometry_type(GeometryType::Point, &point.coord)?;
        self.write_coordinate(&point.coord)?;
        Ok(())
    }

    /// Writes a linestring
    fn write_linestring(&mut self, ls: &LineString) -> Result<()> {
        self.write_byte_order()?;
        let coord_type = ls.coords.first().ok_or_else(|| {
            GeoParquetError::invalid_geometry("LineString must have at least one coordinate")
        })?;
        self.write_geometry_type(GeometryType::LineString, coord_type)?;
        self.write_u32(ls.coords.len() as u32)?;
        for coord in &ls.coords {
            self.write_coordinate(coord)?;
        }
        Ok(())
    }

    /// Writes a polygon
    fn write_polygon(&mut self, poly: &Polygon) -> Result<()> {
        self.write_byte_order()?;
        let coord_type = poly.exterior.coords.first().ok_or_else(|| {
            GeoParquetError::invalid_geometry("Polygon exterior must have at least one coordinate")
        })?;
        self.write_geometry_type(GeometryType::Polygon, coord_type)?;
        self.write_u32((1 + poly.interiors.len()) as u32)?;

        // Write exterior ring
        self.write_u32(poly.exterior.coords.len() as u32)?;
        for coord in &poly.exterior.coords {
            self.write_coordinate(coord)?;
        }

        // Write interior rings
        for interior in &poly.interiors {
            self.write_u32(interior.coords.len() as u32)?;
            for coord in &interior.coords {
                self.write_coordinate(coord)?;
            }
        }

        Ok(())
    }

    /// Writes a multipoint
    fn write_multipoint(&mut self, mp: &MultiPoint) -> Result<()> {
        self.write_byte_order()?;
        let coord_type = mp
            .points
            .first()
            .map(|p| &p.coord)
            .ok_or_else(|| GeoParquetError::invalid_geometry("MultiPoint cannot be empty"))?;
        self.write_geometry_type(GeometryType::MultiPoint, coord_type)?;
        self.write_u32(mp.points.len() as u32)?;

        for point in &mp.points {
            self.write_point(point)?;
        }

        Ok(())
    }

    /// Writes a multilinestring
    fn write_multilinestring(&mut self, mls: &MultiLineString) -> Result<()> {
        self.write_byte_order()?;
        let coord_type = mls
            .linestrings
            .first()
            .and_then(|ls| ls.coords.first())
            .ok_or_else(|| GeoParquetError::invalid_geometry("MultiLineString cannot be empty"))?;
        self.write_geometry_type(GeometryType::MultiLineString, coord_type)?;
        self.write_u32(mls.linestrings.len() as u32)?;

        for ls in &mls.linestrings {
            self.write_linestring(ls)?;
        }

        Ok(())
    }

    /// Writes a multipolygon
    fn write_multipolygon(&mut self, mpoly: &MultiPolygon) -> Result<()> {
        self.write_byte_order()?;
        let coord_type = mpoly
            .polygons
            .first()
            .and_then(|p| p.exterior.coords.first())
            .ok_or_else(|| GeoParquetError::invalid_geometry("MultiPolygon cannot be empty"))?;
        self.write_geometry_type(GeometryType::MultiPolygon, coord_type)?;
        self.write_u32(mpoly.polygons.len() as u32)?;

        for poly in &mpoly.polygons {
            self.write_polygon(poly)?;
        }

        Ok(())
    }

    /// Writes a geometry collection
    fn write_geometrycollection(&mut self, gc: &GeometryCollection) -> Result<()> {
        self.write_byte_order()?;
        // For GeometryCollection, use 2D as default
        let dummy_coord = Coordinate::new_2d(0.0, 0.0);
        self.write_geometry_type(GeometryType::GeometryCollection, &dummy_coord)?;
        self.write_u32(gc.geometries.len() as u32)?;

        for geom in &gc.geometries {
            self.write_geometry_impl(geom)?;
        }

        Ok(())
    }

    /// Writes byte order marker
    fn write_byte_order(&mut self) -> Result<()> {
        if self.little_endian {
            self.buffer.write_u8(WkbByteOrder::LittleEndian as u8)?;
        } else {
            self.buffer.write_u8(WkbByteOrder::BigEndian as u8)?;
        }
        Ok(())
    }

    /// Writes geometry type with Z/M flags
    fn write_geometry_type(&mut self, geom_type: GeometryType, coord: &Coordinate) -> Result<()> {
        let code = geom_type.to_wkb_code(coord.has_z(), coord.has_m());
        self.write_u32(code)
    }

    /// Writes a coordinate
    fn write_coordinate(&mut self, coord: &Coordinate) -> Result<()> {
        self.write_f64(coord.x)?;
        self.write_f64(coord.y)?;
        if let Some(z) = coord.z {
            self.write_f64(z)?;
        }
        if let Some(m) = coord.m {
            self.write_f64(m)?;
        }
        Ok(())
    }

    /// Writes a u32
    fn write_u32(&mut self, value: u32) -> Result<()> {
        if self.little_endian {
            self.buffer.write_u32::<LittleEndian>(value)?;
        } else {
            self.buffer.write_u32::<BigEndian>(value)?;
        }
        Ok(())
    }

    /// Writes a f64
    fn write_f64(&mut self, value: f64) -> Result<()> {
        if self.little_endian {
            self.buffer.write_f64::<LittleEndian>(value)?;
        } else {
            self.buffer.write_f64::<BigEndian>(value)?;
        }
        Ok(())
    }
}

/// Maximum nesting depth for GeometryCollection.
///
/// This guard prevents stack exhaustion from maliciously crafted deeply
/// nested WKB inputs. At depth 64 the reader returns
/// `GeoParquetError::InvalidWkb` rather than recursing further.
pub const MAX_COLLECTION_DEPTH: usize = 64;

/// WKB reader for decoding geometries
pub struct WkbReader<'a> {
    cursor: Cursor<&'a [u8]>,
}

impl<'a> WkbReader<'a> {
    /// Creates a new WKB reader
    pub fn new(bytes: &'a [u8]) -> Self {
        Self {
            cursor: Cursor::new(bytes),
        }
    }

    /// Reads a geometry from WKB format
    pub fn read_geometry(&mut self) -> Result<Geometry> {
        self.read_geometry_depth(0)
    }

    /// Internal implementation of geometry reading with depth guard.
    fn read_geometry_depth(&mut self, depth: usize) -> Result<Geometry> {
        let byte_order = self.read_byte_order()?;
        let (geom_type, has_z, has_m) = self.read_geometry_type(byte_order)?;

        match geom_type {
            GeometryType::Point => {
                let point = self.read_point(byte_order, has_z, has_m)?;
                Ok(Geometry::Point(point))
            }
            GeometryType::LineString => {
                let ls = self.read_linestring(byte_order, has_z, has_m)?;
                Ok(Geometry::LineString(ls))
            }
            GeometryType::Polygon => {
                let poly = self.read_polygon(byte_order, has_z, has_m)?;
                Ok(Geometry::Polygon(poly))
            }
            GeometryType::MultiPoint => {
                let mp = self.read_multipoint(byte_order)?;
                Ok(Geometry::MultiPoint(mp))
            }
            GeometryType::MultiLineString => {
                let mls = self.read_multilinestring(byte_order)?;
                Ok(Geometry::MultiLineString(mls))
            }
            GeometryType::MultiPolygon => {
                let mpoly = self.read_multipolygon(byte_order)?;
                Ok(Geometry::MultiPolygon(mpoly))
            }
            GeometryType::GeometryCollection => {
                let gc = self.read_geometrycollection(byte_order, depth)?;
                Ok(Geometry::GeometryCollection(gc))
            }
        }
    }

    /// Reads a point (without header)
    fn read_point(&mut self, byte_order: WkbByteOrder, has_z: bool, has_m: bool) -> Result<Point> {
        let coord = self.read_coordinate(byte_order, has_z, has_m)?;
        Ok(Point::new(coord))
    }

    /// Reads a linestring (without header)
    fn read_linestring(
        &mut self,
        byte_order: WkbByteOrder,
        has_z: bool,
        has_m: bool,
    ) -> Result<LineString> {
        let num_points = self.read_u32(byte_order)?;
        let mut coords = Vec::with_capacity(num_points as usize);
        for _ in 0..num_points {
            coords.push(self.read_coordinate(byte_order, has_z, has_m)?);
        }
        Ok(LineString::new(coords))
    }

    /// Reads a polygon (without header)
    fn read_polygon(
        &mut self,
        byte_order: WkbByteOrder,
        has_z: bool,
        has_m: bool,
    ) -> Result<Polygon> {
        let num_rings = self.read_u32(byte_order)?;
        if num_rings == 0 {
            return Err(GeoParquetError::invalid_wkb(
                "Polygon must have at least one ring",
            ));
        }

        // Read exterior ring
        let num_points = self.read_u32(byte_order)?;
        let mut exterior_coords = Vec::with_capacity(num_points as usize);
        for _ in 0..num_points {
            exterior_coords.push(self.read_coordinate(byte_order, has_z, has_m)?);
        }
        let exterior = LineString::new(exterior_coords);

        // Read interior rings
        let mut interiors = Vec::with_capacity((num_rings - 1) as usize);
        for _ in 1..num_rings {
            let num_points = self.read_u32(byte_order)?;
            let mut interior_coords = Vec::with_capacity(num_points as usize);
            for _ in 0..num_points {
                interior_coords.push(self.read_coordinate(byte_order, has_z, has_m)?);
            }
            interiors.push(LineString::new(interior_coords));
        }

        Ok(Polygon::new(exterior, interiors))
    }

    /// Reads a multipoint
    fn read_multipoint(&mut self, byte_order: WkbByteOrder) -> Result<MultiPoint> {
        let num_points = self.read_u32(byte_order)?;
        let mut points = Vec::with_capacity(num_points as usize);
        for _ in 0..num_points {
            let inner_order = self.read_byte_order()?;
            let (geom_type, has_z, has_m) = self.read_geometry_type(inner_order)?;
            if geom_type != GeometryType::Point {
                return Err(GeoParquetError::invalid_wkb(
                    "MultiPoint must contain only Points",
                ));
            }
            points.push(self.read_point(inner_order, has_z, has_m)?);
        }
        Ok(MultiPoint::new(points))
    }

    /// Reads a multilinestring
    fn read_multilinestring(&mut self, byte_order: WkbByteOrder) -> Result<MultiLineString> {
        let num_linestrings = self.read_u32(byte_order)?;
        let mut linestrings = Vec::with_capacity(num_linestrings as usize);
        for _ in 0..num_linestrings {
            let inner_order = self.read_byte_order()?;
            let (geom_type, has_z, has_m) = self.read_geometry_type(inner_order)?;
            if geom_type != GeometryType::LineString {
                return Err(GeoParquetError::invalid_wkb(
                    "MultiLineString must contain only LineStrings",
                ));
            }
            linestrings.push(self.read_linestring(inner_order, has_z, has_m)?);
        }
        Ok(MultiLineString::new(linestrings))
    }

    /// Reads a multipolygon
    fn read_multipolygon(&mut self, byte_order: WkbByteOrder) -> Result<MultiPolygon> {
        let num_polygons = self.read_u32(byte_order)?;
        let mut polygons = Vec::with_capacity(num_polygons as usize);
        for _ in 0..num_polygons {
            let inner_order = self.read_byte_order()?;
            let (geom_type, has_z, has_m) = self.read_geometry_type(inner_order)?;
            if geom_type != GeometryType::Polygon {
                return Err(GeoParquetError::invalid_wkb(
                    "MultiPolygon must contain only Polygons",
                ));
            }
            polygons.push(self.read_polygon(inner_order, has_z, has_m)?);
        }
        Ok(MultiPolygon::new(polygons))
    }

    /// Reads a geometry collection, enforcing the maximum nesting depth.
    fn read_geometrycollection(
        &mut self,
        byte_order: WkbByteOrder,
        depth: usize,
    ) -> Result<GeometryCollection> {
        if depth >= MAX_COLLECTION_DEPTH {
            return Err(GeoParquetError::invalid_wkb(format!(
                "geometry collection nesting exceeds maximum depth of {MAX_COLLECTION_DEPTH}"
            )));
        }
        let num_geometries = self.read_u32(byte_order)?;
        let mut geometries = Vec::with_capacity(num_geometries as usize);
        for _ in 0..num_geometries {
            geometries.push(self.read_geometry_depth(depth + 1)?);
        }
        Ok(GeometryCollection::new(geometries))
    }

    /// Reads byte order marker
    fn read_byte_order(&mut self) -> Result<WkbByteOrder> {
        let byte = self.cursor.read_u8()?;
        WkbByteOrder::from_byte(byte)
    }

    /// Reads geometry type and extracts Z/M flags
    fn read_geometry_type(
        &mut self,
        byte_order: WkbByteOrder,
    ) -> Result<(GeometryType, bool, bool)> {
        let code = self.read_u32(byte_order)?;

        // Extract Z and M flags
        let has_z = (code / 1000 == 1) || (code / 1000 == 3);
        let has_m = (code / 1000 == 2) || (code / 1000 == 3);

        let geom_type = GeometryType::from_wkb_code(code).ok_or_else(|| {
            GeoParquetError::invalid_wkb(format!("Unknown geometry type code: {code}"))
        })?;

        Ok((geom_type, has_z, has_m))
    }

    /// Reads a coordinate
    fn read_coordinate(
        &mut self,
        byte_order: WkbByteOrder,
        has_z: bool,
        has_m: bool,
    ) -> Result<Coordinate> {
        let x = self.read_f64(byte_order)?;
        let y = self.read_f64(byte_order)?;
        let z = if has_z {
            Some(self.read_f64(byte_order)?)
        } else {
            None
        };
        let m = if has_m {
            Some(self.read_f64(byte_order)?)
        } else {
            None
        };

        Ok(Coordinate { x, y, z, m })
    }

    /// Reads a u32
    fn read_u32(&mut self, byte_order: WkbByteOrder) -> Result<u32> {
        if byte_order.is_little_endian() {
            Ok(self.cursor.read_u32::<LittleEndian>()?)
        } else {
            Ok(self.cursor.read_u32::<BigEndian>()?)
        }
    }

    /// Reads a f64
    fn read_f64(&mut self, byte_order: WkbByteOrder) -> Result<f64> {
        if byte_order.is_little_endian() {
            Ok(self.cursor.read_f64::<LittleEndian>()?)
        } else {
            Ok(self.cursor.read_f64::<BigEndian>()?)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_point_encoding_decoding() -> Result<()> {
        let point = Point::new_2d(1.0, 2.0);
        let geom = Geometry::Point(point);

        let mut writer = WkbWriter::new(true);
        let wkb = writer.write_geometry(&geom)?;

        let mut reader = WkbReader::new(&wkb);
        let decoded = reader.read_geometry()?;

        assert_eq!(geom, decoded);
        Ok(())
    }

    #[test]
    fn test_linestring_encoding_decoding() -> Result<()> {
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(2.0, 2.0),
        ];
        let ls = LineString::new(coords);
        let geom = Geometry::LineString(ls);

        let mut writer = WkbWriter::new(true);
        let wkb = writer.write_geometry(&geom)?;

        let mut reader = WkbReader::new(&wkb);
        let decoded = reader.read_geometry()?;

        assert_eq!(geom, decoded);
        Ok(())
    }

    #[test]
    fn test_polygon_encoding_decoding() -> Result<()> {
        let exterior = LineString::new(vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 0.0),
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(0.0, 1.0),
            Coordinate::new_2d(0.0, 0.0),
        ]);
        let poly = Polygon::new_simple(exterior);
        let geom = Geometry::Polygon(poly);

        let mut writer = WkbWriter::new(true);
        let wkb = writer.write_geometry(&geom)?;

        let mut reader = WkbReader::new(&wkb);
        let decoded = reader.read_geometry()?;

        assert_eq!(geom, decoded);
        Ok(())
    }

    #[test]
    fn test_3d_point() -> Result<()> {
        let point = Point::new_3d(1.0, 2.0, 3.0);
        let geom = Geometry::Point(point);

        let mut writer = WkbWriter::new(true);
        let wkb = writer.write_geometry(&geom)?;

        let mut reader = WkbReader::new(&wkb);
        let decoded = reader.read_geometry()?;

        assert_eq!(geom, decoded);
        Ok(())
    }

    #[test]
    fn test_byte_order() -> Result<()> {
        let point = Point::new_2d(1.0, 2.0);
        let geom = Geometry::Point(point);

        // Test little endian
        let mut writer_le = WkbWriter::new(true);
        let wkb_le = writer_le.write_geometry(&geom)?;
        assert_eq!(wkb_le[0], WkbByteOrder::LittleEndian as u8);

        // Test big endian
        let mut writer_be = WkbWriter::new(false);
        let wkb_be = writer_be.write_geometry(&geom)?;
        assert_eq!(wkb_be[0], WkbByteOrder::BigEndian as u8);

        // Both should decode correctly
        let mut reader_le = WkbReader::new(&wkb_le);
        let decoded_le = reader_le.read_geometry()?;
        assert_eq!(geom, decoded_le);

        let mut reader_be = WkbReader::new(&wkb_be);
        let decoded_be = reader_be.read_geometry()?;
        assert_eq!(geom, decoded_be);

        Ok(())
    }

    // ── Z/M/ZM variant tests ──────────────────────────────────────────────────

    #[test]
    fn test_wkb_point_z_decoded() -> Result<()> {
        // WKB: 0x01 (LE) + type 1001 (PointZ) + x + y + z
        let mut buf = vec![0x01u8];
        buf.extend_from_slice(&1001u32.to_le_bytes());
        buf.extend_from_slice(&1.0f64.to_le_bytes());
        buf.extend_from_slice(&2.0f64.to_le_bytes());
        buf.extend_from_slice(&3.0f64.to_le_bytes());

        let mut reader = WkbReader::new(&buf);
        let geom = reader.read_geometry()?;

        assert!(geom.has_z(), "PointZ must report has_z = true");
        assert!(!geom.has_m(), "PointZ must report has_m = false");

        assert!(
            matches!(&geom, Geometry::Point(_)),
            "Expected Geometry::Point, got {:?}",
            geom
        );
        if let Geometry::Point(p) = &geom {
            assert!((p.coord.x - 1.0).abs() < f64::EPSILON);
            assert!((p.coord.y - 2.0).abs() < f64::EPSILON);
            assert!((p.coord.z.expect("z must be present") - 3.0).abs() < f64::EPSILON);
        }
        Ok(())
    }

    #[test]
    fn test_wkb_point_zm_decoded() -> Result<()> {
        // WKB: 0x01 (LE) + type 3001 (PointZM) + x + y + z + m
        let mut buf = vec![0x01u8];
        buf.extend_from_slice(&3001u32.to_le_bytes());
        for v in [1.0f64, 2.0, 3.0, 4.0] {
            buf.extend_from_slice(&v.to_le_bytes());
        }

        let mut reader = WkbReader::new(&buf);
        let geom = reader.read_geometry()?;

        assert!(geom.has_z(), "PointZM must report has_z = true");
        assert!(geom.has_m(), "PointZM must report has_m = true");

        assert!(
            matches!(&geom, Geometry::Point(_)),
            "Expected Geometry::Point, got {:?}",
            geom
        );
        if let Geometry::Point(p) = &geom {
            assert!((p.coord.x - 1.0).abs() < f64::EPSILON);
            assert!((p.coord.y - 2.0).abs() < f64::EPSILON);
            assert!((p.coord.z.expect("z") - 3.0).abs() < f64::EPSILON);
            assert!((p.coord.m.expect("m") - 4.0).abs() < f64::EPSILON);
        }
        Ok(())
    }

    #[test]
    fn test_wkb_geometry_collection_flat() -> Result<()> {
        // GeometryCollection with 3 2D points
        let mut buf = vec![0x01u8];
        buf.extend_from_slice(&7u32.to_le_bytes()); // GeometryCollection
        buf.extend_from_slice(&3u32.to_le_bytes()); // count = 3
        for (x, y) in [(1.0f64, 2.0f64), (3.0, 4.0), (5.0, 6.0)] {
            buf.push(0x01); // LE
            buf.extend_from_slice(&1u32.to_le_bytes()); // Point
            buf.extend_from_slice(&x.to_le_bytes());
            buf.extend_from_slice(&y.to_le_bytes());
        }

        let mut reader = WkbReader::new(&buf);
        let geom = reader.read_geometry()?;

        assert!(
            matches!(&geom, Geometry::GeometryCollection(_)),
            "Expected GeometryCollection, got {:?}",
            geom
        );
        if let Geometry::GeometryCollection(gc) = geom {
            assert_eq!(gc.len(), 3, "collection should have 3 members");
            for (i, child) in gc.geometries.iter().enumerate() {
                assert!(
                    matches!(child, Geometry::Point(_)),
                    "child {i} should be a Point"
                );
            }
        }
        Ok(())
    }

    #[test]
    fn test_wkb_geometry_collection_recursive() -> Result<()> {
        // Outer collection (type 7, count=1) wrapping inner collection (type 7, count=1) wrapping a Point
        fn encode_point_le(x: f64, y: f64) -> Vec<u8> {
            let mut b = vec![0x01u8];
            b.extend_from_slice(&1u32.to_le_bytes());
            b.extend_from_slice(&x.to_le_bytes());
            b.extend_from_slice(&y.to_le_bytes());
            b
        }
        fn encode_collection_le(inner: Vec<u8>) -> Vec<u8> {
            let mut b = vec![0x01u8];
            b.extend_from_slice(&7u32.to_le_bytes());
            b.extend_from_slice(&1u32.to_le_bytes()); // count = 1
            b.extend_from_slice(&inner);
            b
        }

        let point_wkb = encode_point_le(7.0, 8.0);
        let inner_coll = encode_collection_le(point_wkb);
        let outer_coll = encode_collection_le(inner_coll);

        let mut reader = WkbReader::new(&outer_coll);
        let geom = reader.read_geometry()?;

        assert!(
            matches!(&geom, Geometry::GeometryCollection(_)),
            "Expected outer GeometryCollection, got {:?}",
            geom
        );
        if let Geometry::GeometryCollection(outer) = geom {
            assert_eq!(outer.len(), 1);
            assert!(
                matches!(&outer.geometries[0], Geometry::GeometryCollection(_)),
                "Expected inner GeometryCollection, got {:?}",
                outer.geometries[0]
            );
            if let Geometry::GeometryCollection(inner) = &outer.geometries[0] {
                assert_eq!(inner.len(), 1);
                assert!(matches!(&inner.geometries[0], Geometry::Point(_)));
            }
        }
        Ok(())
    }

    #[test]
    fn test_wkb_nested_collection_depth_guard() {
        /// Builds a chain of nested GeometryCollections, terminating with a Point.
        fn make_nested(depth: usize) -> Vec<u8> {
            if depth == 0 {
                // Leaf: a single 2D Point
                let mut b = vec![0x01u8];
                b.extend_from_slice(&1u32.to_le_bytes());
                b.extend_from_slice(&0.0f64.to_le_bytes());
                b.extend_from_slice(&0.0f64.to_le_bytes());
                return b;
            }
            let inner = make_nested(depth - 1);
            let mut b = vec![0x01u8];
            b.extend_from_slice(&7u32.to_le_bytes()); // GeometryCollection
            b.extend_from_slice(&1u32.to_le_bytes()); // count = 1
            b.extend_from_slice(&inner);
            b
        }

        // 100 levels of nesting exceeds MAX_COLLECTION_DEPTH (64)
        let data = make_nested(100);
        let mut reader = WkbReader::new(&data);
        let result = reader.read_geometry();
        assert!(
            result.is_err(),
            "reading a 100-deep nested collection must return Err (depth guard)"
        );
    }

    #[test]
    fn test_wkb_multi_polygon_zm_decoded() -> Result<()> {
        // MultiPolygonZM (type 3006): 1 polygon, 1 ring, 4 XYZM points
        let mut buf = vec![0x01u8];
        buf.extend_from_slice(&3006u32.to_le_bytes()); // MultiPolygonZM

        // Outer count: 1 polygon sub-geometry
        buf.extend_from_slice(&1u32.to_le_bytes());

        // Sub-geometry: PolygonZM (type 3003)
        buf.push(0x01); // LE
        buf.extend_from_slice(&3003u32.to_le_bytes()); // PolygonZM
        buf.extend_from_slice(&1u32.to_le_bytes()); // 1 ring
        buf.extend_from_slice(&4u32.to_le_bytes()); // 4 points (closed ring)
        for _ in 0..4 {
            for v in [1.0f64, 2.0, 5.0, 0.0] {
                // x, y, z, m
                buf.extend_from_slice(&v.to_le_bytes());
            }
        }

        let mut reader = WkbReader::new(&buf);
        let result = reader.read_geometry();
        assert!(
            result.is_ok(),
            "MultiPolygonZM must decode without error: {:?}",
            result.err()
        );

        let geom = result?;
        assert!(geom.has_z(), "MultiPolygonZM must report has_z = true");
        assert!(geom.has_m(), "MultiPolygonZM must report has_m = true");
        Ok(())
    }
}
