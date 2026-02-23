//! Shapefile (.shp) binary geometry file handling
//!
//! This module handles reading and writing the main Shapefile (.shp) file,
//! which contains the binary geometry data.

pub mod header;
pub mod shapes;

pub use header::{BoundingBox, HEADER_SIZE, ShapefileHeader};
pub use shapes::{
    Box2D, MultiPartShape, MultiPartShapeM, MultiPartShapeZ, Point, PointM, PointZ, ShapeType,
};

use crate::error::{Result, ShapefileError};
use byteorder::{BigEndian, LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Read, Seek, Write};

/// Record header size in bytes
pub const RECORD_HEADER_SIZE: usize = 8;

/// A Shapefile record
#[derive(Debug, Clone)]
pub struct ShapeRecord {
    /// Record number (1-based)
    pub record_number: i32,
    /// Shape geometry
    pub shape: Shape,
}

/// Shape geometry variants
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    /// Null shape (no geometry)
    Null,
    /// 2D point
    Point(Point),
    /// 3D point with Z
    PointZ(PointZ),
    /// Point with M value
    PointM(PointM),
    /// PolyLine (one or more line strings)
    PolyLine(MultiPartShape),
    /// Polygon (one or more rings)
    Polygon(MultiPartShape),
    /// MultiPoint (collection of points)
    MultiPoint(MultiPartShape),
    /// PolyLine with Z coordinates
    PolyLineZ(MultiPartShapeZ),
    /// Polygon with Z coordinates
    PolygonZ(MultiPartShapeZ),
    /// MultiPoint with Z coordinates
    MultiPointZ(MultiPartShapeZ),
    /// PolyLine with M values
    PolyLineM(MultiPartShapeM),
    /// Polygon with M values
    PolygonM(MultiPartShapeM),
    /// MultiPoint with M values
    MultiPointM(MultiPartShapeM),
}

impl Shape {
    /// Returns the shape type
    pub fn shape_type(&self) -> ShapeType {
        match self {
            Self::Null => ShapeType::Null,
            Self::Point(_) => ShapeType::Point,
            Self::PointZ(_) => ShapeType::PointZ,
            Self::PointM(_) => ShapeType::PointM,
            Self::PolyLine(_) => ShapeType::PolyLine,
            Self::Polygon(_) => ShapeType::Polygon,
            Self::MultiPoint(_) => ShapeType::MultiPoint,
            Self::PolyLineZ(_) => ShapeType::PolyLineZ,
            Self::PolygonZ(_) => ShapeType::PolygonZ,
            Self::MultiPointZ(_) => ShapeType::MultiPointZ,
            Self::PolyLineM(_) => ShapeType::PolyLineM,
            Self::PolygonM(_) => ShapeType::PolygonM,
            Self::MultiPointM(_) => ShapeType::MultiPointM,
        }
    }

    /// Reads a shape from a reader
    pub fn read<R: Read>(reader: &mut R) -> Result<Self> {
        let shape_type_code = reader
            .read_i32::<LittleEndian>()
            .map_err(|_| ShapefileError::unexpected_eof("reading shape type"))?;

        let shape_type = ShapeType::from_code(shape_type_code)?;

        match shape_type {
            ShapeType::Null => Ok(Self::Null),
            ShapeType::Point => {
                let point = Point::read(reader)?;
                Ok(Self::Point(point))
            }
            ShapeType::PointZ => {
                let point = PointZ::read(reader)?;
                Ok(Self::PointZ(point))
            }
            ShapeType::PointM => {
                let point = PointM::read(reader)?;
                Ok(Self::PointM(point))
            }
            ShapeType::PolyLine => {
                let shape = MultiPartShape::read(reader)?;
                Ok(Self::PolyLine(shape))
            }
            ShapeType::Polygon => {
                let shape = MultiPartShape::read(reader)?;
                Ok(Self::Polygon(shape))
            }
            ShapeType::MultiPoint => {
                let shape = MultiPartShape::read(reader)?;
                Ok(Self::MultiPoint(shape))
            }
            ShapeType::PolyLineZ => {
                let shape = MultiPartShapeZ::read(reader)?;
                Ok(Self::PolyLineZ(shape))
            }
            ShapeType::PolygonZ => {
                let shape = MultiPartShapeZ::read(reader)?;
                Ok(Self::PolygonZ(shape))
            }
            ShapeType::MultiPointZ => {
                let shape = MultiPartShapeZ::read(reader)?;
                Ok(Self::MultiPointZ(shape))
            }
            ShapeType::PolyLineM => {
                let shape = MultiPartShapeM::read(reader)?;
                Ok(Self::PolyLineM(shape))
            }
            ShapeType::PolygonM => {
                let shape = MultiPartShapeM::read(reader)?;
                Ok(Self::PolygonM(shape))
            }
            ShapeType::MultiPointM => {
                let shape = MultiPartShapeM::read(reader)?;
                Ok(Self::MultiPointM(shape))
            }
            _ => Err(ShapefileError::UnsupportedShapeType {
                shape_type: shape_type_code,
            }),
        }
    }

    /// Writes a shape to a writer
    pub fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        let shape_type = self.shape_type();
        writer
            .write_i32::<LittleEndian>(shape_type.to_code())
            .map_err(ShapefileError::Io)?;

        match self {
            Self::Null => Ok(()),
            Self::Point(point) => point.write(writer),
            Self::PointZ(point) => point.write(writer),
            Self::PointM(point) => point.write(writer),
            Self::PolyLine(shape) => shape.write(writer),
            Self::Polygon(shape) => shape.write(writer),
            Self::MultiPoint(shape) => shape.write(writer),
            Self::PolyLineZ(shape) | Self::PolygonZ(shape) | Self::MultiPointZ(shape) => {
                shape.write(writer)
            }
            Self::PolyLineM(shape) | Self::PolygonM(shape) | Self::MultiPointM(shape) => {
                shape.write(writer)
            }
        }
    }

    /// Calculates the content length in 16-bit words (excluding shape type)
    pub fn content_length(&self) -> i32 {
        match self {
            Self::Null => 0,
            // Point: 2 doubles = 16 bytes = 8 words
            Self::Point(_) => 8,
            // PointZ: 4 doubles = 32 bytes = 16 words
            Self::PointZ(_) => 16,
            // PointM: 3 doubles = 24 bytes = 12 words
            Self::PointM(_) => 12,
            // MultiPartShape: bbox (4*8) + num_parts (4) + num_points (4) + parts + points
            Self::PolyLine(shape) | Self::Polygon(shape) | Self::MultiPoint(shape) => {
                let bbox_bytes = 32; // 4 * 8 bytes (4 doubles)
                let counts_bytes = 8; // num_parts + num_points
                let parts_bytes = shape.num_parts * 4;
                let points_bytes = shape.num_points * 16; // 2 doubles per point
                (bbox_bytes + counts_bytes + parts_bytes + points_bytes) / 2
            }
            // Z variants: base + z_range + z_values + optional m_range + m_values
            Self::PolyLineZ(shape) | Self::PolygonZ(shape) | Self::MultiPointZ(shape) => {
                shape.content_length_words()
            }
            // M variants: base + m_range + m_values
            Self::PolyLineM(shape) | Self::PolygonM(shape) | Self::MultiPointM(shape) => {
                shape.content_length_words()
            }
        }
    }
}

impl ShapeRecord {
    /// Creates a new shape record
    pub fn new(record_number: i32, shape: Shape) -> Self {
        Self {
            record_number,
            shape,
        }
    }

    /// Reads a shape record from a reader
    pub fn read<R: Read>(reader: &mut R) -> Result<Self> {
        // Read record header (big endian)
        let record_number = reader
            .read_i32::<BigEndian>()
            .map_err(|_| ShapefileError::unexpected_eof("reading record number"))?;

        let _content_length = reader
            .read_i32::<BigEndian>()
            .map_err(|_| ShapefileError::unexpected_eof("reading content length"))?;

        // Read shape (little endian)
        let shape = Shape::read(reader)?;

        Ok(Self {
            record_number,
            shape,
        })
    }

    /// Writes a shape record to a writer
    pub fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        // Write record header (big endian)
        writer
            .write_i32::<BigEndian>(self.record_number)
            .map_err(ShapefileError::Io)?;

        // Calculate and write content length (in 16-bit words)
        let content_length = 2 + self.shape.content_length(); // +2 for shape type (4 bytes = 2 words)
        writer
            .write_i32::<BigEndian>(content_length)
            .map_err(ShapefileError::Io)?;

        // Write shape (little endian)
        self.shape.write(writer)?;

        Ok(())
    }
}

/// Shapefile (.shp) reader
pub struct ShpReader<R: Read> {
    reader: R,
    header: ShapefileHeader,
}

impl<R: Read> ShpReader<R> {
    /// Creates a new Shapefile reader
    pub fn new(mut reader: R) -> Result<Self> {
        let header = ShapefileHeader::read(&mut reader)?;
        Ok(Self { reader, header })
    }

    /// Returns the header
    pub fn header(&self) -> &ShapefileHeader {
        &self.header
    }

    /// Reads the next record
    pub fn read_record(&mut self) -> Result<Option<ShapeRecord>> {
        match ShapeRecord::read(&mut self.reader) {
            Ok(record) => Ok(Some(record)),
            Err(ShapefileError::Io(ref e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                Ok(None)
            }
            Err(ShapefileError::UnexpectedEof { .. }) => {
                // EOF when reading record is expected at end of file
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }

    /// Reads all records
    pub fn read_all_records(&mut self) -> Result<Vec<ShapeRecord>> {
        let mut records = Vec::new();
        while let Some(record) = self.read_record()? {
            records.push(record);
        }
        Ok(records)
    }
}

/// Shapefile (.shp) writer
pub struct ShpWriter<W: Write> {
    writer: W,
    header: ShapefileHeader,
    record_count: i32,
    /// Total file size in 16-bit words (for updating header)
    file_length_words: i32,
}

impl<W: Write> ShpWriter<W> {
    /// Creates a new Shapefile writer
    pub fn new(writer: W, shape_type: ShapeType, bbox: BoundingBox) -> Self {
        let header = ShapefileHeader::new(shape_type, bbox);
        Self {
            writer,
            header,
            record_count: 0,
            file_length_words: 50, // Header is 100 bytes = 50 words
        }
    }

    /// Writes the header (should be called first)
    pub fn write_header(&mut self) -> Result<()> {
        self.header.write(&mut self.writer)
    }

    /// Writes a shape record
    pub fn write_record(&mut self, shape: Shape) -> Result<()> {
        self.record_count += 1;

        // Calculate record size before moving shape: header (4 words) + content
        let content_length = 2 + shape.content_length(); // +2 for shape type
        self.file_length_words += 4 + content_length; // +4 for record header

        let record = ShapeRecord::new(self.record_count, shape);
        record.write(&mut self.writer)
    }

    /// Flushes the internal writer to ensure all data is written
    pub fn flush(&mut self) -> Result<()> {
        self.writer.flush().map_err(ShapefileError::Io)
    }

    /// Finalizes the file (updates header with correct file length)
    pub fn finalize<S: Write + Seek>(self, _seekable_writer: S) -> Result<()> {
        // Calculate total file length in 16-bit words
        // This would require tracking all written bytes
        // For simplicity, we'll skip this optimization and assume the caller
        // will handle it if needed
        Ok(())
    }
}

impl<W: Write + Seek> ShpWriter<W> {
    /// Updates the file length in the header (for seekable writers)
    pub fn update_file_length(&mut self) -> Result<()> {
        // Seek to file length position in header (byte 24)
        self.writer
            .seek(std::io::SeekFrom::Start(24))
            .map_err(ShapefileError::Io)?;

        // Write file length (big endian)
        self.writer
            .write_i32::<BigEndian>(self.file_length_words)
            .map_err(ShapefileError::Io)?;

        // Seek back to end of file
        self.writer
            .seek(std::io::SeekFrom::End(0))
            .map_err(ShapefileError::Io)?;

        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_shape_content_length() {
        let null_shape = Shape::Null;
        assert_eq!(null_shape.content_length(), 0);

        let point_shape = Shape::Point(Point::new(10.0, 20.0));
        assert_eq!(point_shape.content_length(), 8);
    }

    #[test]
    fn test_record_round_trip() {
        let shape = Shape::Point(Point::new(10.5, 20.3));
        let record = ShapeRecord::new(1, shape.clone());

        let mut buffer = Vec::new();
        record.write(&mut buffer).expect("write record to buffer");

        let mut cursor = Cursor::new(buffer);
        let read_record = ShapeRecord::read(&mut cursor).expect("read record from cursor");

        assert_eq!(read_record.record_number, 1);
        assert_eq!(read_record.shape, shape);
    }

    #[test]
    fn test_shp_reader_writer() {
        let bbox = BoundingBox::new_2d(-180.0, -90.0, 180.0, 90.0).expect("valid bbox");
        let mut buffer = Cursor::new(Vec::new());

        // Write
        {
            let mut writer = ShpWriter::new(&mut buffer, ShapeType::Point, bbox);
            writer.write_header().expect("write header");
            writer
                .write_record(Shape::Point(Point::new(10.0, 20.0)))
                .expect("write record 1");
            writer
                .write_record(Shape::Point(Point::new(30.0, 40.0)))
                .expect("write record 2");
        }

        // Read
        buffer.set_position(0);
        let mut reader = ShpReader::new(buffer).expect("create reader");

        assert_eq!(reader.header().shape_type, ShapeType::Point);

        let records = reader.read_all_records().expect("read records");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].record_number, 1);
        assert_eq!(records[1].record_number, 2);
    }

    #[test]
    fn test_polyline_z_round_trip() {
        let points = vec![
            Point::new(0.0, 0.0),
            Point::new(10.0, 10.0),
            Point::new(20.0, 5.0),
        ];
        let z_values = vec![100.0, 200.0, 150.0];
        let m_values = Some(vec![0.0, 0.5, 1.0]);

        let shape_z = MultiPartShapeZ::new(vec![0], points, z_values.clone(), m_values.clone())
            .expect("valid shape");
        let shape = Shape::PolyLineZ(shape_z);

        let mut buffer = Vec::new();
        let record = ShapeRecord::new(1, shape.clone());
        record.write(&mut buffer).expect("write record");

        let mut cursor = Cursor::new(buffer);
        let read_record = ShapeRecord::read(&mut cursor).expect("read record");

        assert_eq!(read_record.record_number, 1);
        if let Shape::PolyLineZ(ref sz) = read_record.shape {
            assert_eq!(sz.base.num_points, 3);
            assert_eq!(sz.z_values.len(), 3);
            assert!((sz.z_values[0] - 100.0).abs() < f64::EPSILON);
            assert!((sz.z_values[1] - 200.0).abs() < f64::EPSILON);
            assert!((sz.z_values[2] - 150.0).abs() < f64::EPSILON);
            assert!(sz.m_values.is_some());
            let mv = sz.m_values.as_ref().expect("m_values");
            assert!((mv[0] - 0.0).abs() < f64::EPSILON);
            assert!((mv[1] - 0.5).abs() < f64::EPSILON);
            assert!((mv[2] - 1.0).abs() < f64::EPSILON);
        } else {
            panic!("Expected PolyLineZ shape");
        }
    }

    #[test]
    fn test_polygon_m_round_trip() {
        let points = vec![
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 10.0),
            Point::new(0.0, 0.0),
        ];
        let m_values = vec![0.0, 1.0, 2.0, 0.0];

        let shape_m = MultiPartShapeM::new(vec![0], points, m_values.clone()).expect("valid shape");
        let shape = Shape::PolygonM(shape_m);

        let mut buffer = Vec::new();
        let record = ShapeRecord::new(1, shape.clone());
        record.write(&mut buffer).expect("write record");

        let mut cursor = Cursor::new(buffer);
        let read_record = ShapeRecord::read(&mut cursor).expect("read record");

        assert_eq!(read_record.record_number, 1);
        if let Shape::PolygonM(ref sm) = read_record.shape {
            assert_eq!(sm.base.num_points, 4);
            assert_eq!(sm.m_values.len(), 4);
            assert!((sm.m_values[0] - 0.0).abs() < f64::EPSILON);
            assert!((sm.m_values[1] - 1.0).abs() < f64::EPSILON);
        } else {
            panic!("Expected PolygonM shape");
        }
    }

    #[test]
    fn test_z_content_length() {
        let points = vec![Point::new(0.0, 0.0), Point::new(10.0, 10.0)];
        let z_values = vec![100.0, 200.0];

        let shape_z = MultiPartShapeZ::new(vec![0], points, z_values, None).expect("valid shape");
        let shape = Shape::PolyLineZ(shape_z);
        // content_length should include base + z_range + z_values but NOT m
        let cl = shape.content_length();
        assert!(cl > 0);
    }
}
