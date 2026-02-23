//! `FlatGeobuf` writer implementation
//!
//! Provides writing of `FlatGeobuf` files with support for:
//! - Sequential feature writing
//! - Optional spatial index generation
//! - All geometry types and property types

use crate::MAGIC_BYTES;
use crate::error::{FlatGeobufError, Result};
use crate::geometry::GeometryCodec;
use crate::header::{Column, ColumnType, Header};
use crate::index::{BoundingBox, PackedRTree};
use byteorder::{LittleEndian, WriteBytesExt};
use oxigdal_core::vector::{Feature, PropertyValue};
use std::io::{Seek, SeekFrom, Write};

/// `FlatGeobuf` writer
pub struct FlatGeobufWriter<W: Write + Seek> {
    writer: W,
    header: Header,
    geometry_codec: GeometryCodec,
    features: Vec<Vec<u8>>,
    bboxes: Vec<BoundingBox>,
    features_written: bool,
}

impl<W: Write + Seek> FlatGeobufWriter<W> {
    /// Creates a new `FlatGeobuf` writer
    pub fn new(writer: W, header: Header) -> Result<Self> {
        let geometry_codec = GeometryCodec::new(header.has_z, header.has_m);

        Ok(Self {
            writer,
            header,
            geometry_codec,
            features: Vec::new(),
            bboxes: Vec::new(),
            features_written: false,
        })
    }

    /// Adds a feature to be written
    pub fn add_feature(&mut self, feature: &Feature) -> Result<()> {
        if self.features_written {
            return Err(FlatGeobufError::NotSupported(
                "Cannot add features after writing has completed".to_string(),
            ));
        }

        // Encode feature to bytes
        let feature_bytes = self.encode_feature(feature)?;

        // Store bounding box if building index
        if self.header.has_index {
            if let Some(bounds) = feature.bounds() {
                self.bboxes
                    .push(BoundingBox::new(bounds.0, bounds.1, bounds.2, bounds.3));
            } else {
                self.bboxes.push(BoundingBox::empty());
            }
        }

        self.features.push(feature_bytes);

        Ok(())
    }

    /// Encodes a feature to bytes
    fn encode_feature(&self, feature: &Feature) -> Result<Vec<u8>> {
        let mut buffer = Vec::new();

        // Write geometry presence flag
        let has_geometry = feature.has_geometry();
        buffer.write_u8(u8::from(has_geometry))?;

        // Write geometry if present
        if let Some(ref geometry) = feature.geometry {
            self.geometry_codec.encode(&mut buffer, geometry)?;
        }

        // Write properties
        for column in &self.header.columns {
            let value = feature.get_property(&column.name);

            if let Some(value) = value {
                if value.is_null() {
                    buffer.write_u8(1)?; // null flag
                } else {
                    buffer.write_u8(0)?; // not null
                    self.write_property_value(&mut buffer, value, column)?;
                }
            } else {
                buffer.write_u8(1)?; // null flag
            }
        }

        Ok(buffer)
    }

    /// Writes a property value
    fn write_property_value(
        &self,
        writer: &mut Vec<u8>,
        value: &PropertyValue,
        column: &Column,
    ) -> Result<()> {
        match column.column_type {
            ColumnType::Byte => {
                if let Some(i) = value.as_i64() {
                    writer.write_i8(i as i8)?;
                } else {
                    writer.write_i8(0)?;
                }
            }
            ColumnType::UByte => {
                if let Some(u) = value.as_u64() {
                    writer.write_u8(u as u8)?;
                } else {
                    writer.write_u8(0)?;
                }
            }
            ColumnType::Bool => {
                if let Some(b) = value.as_bool() {
                    writer.write_u8(u8::from(b))?;
                } else {
                    writer.write_u8(0)?;
                }
            }
            ColumnType::Short => {
                if let Some(i) = value.as_i64() {
                    writer.write_i16::<LittleEndian>(i as i16)?;
                } else {
                    writer.write_i16::<LittleEndian>(0)?;
                }
            }
            ColumnType::UShort => {
                if let Some(u) = value.as_u64() {
                    writer.write_u16::<LittleEndian>(u as u16)?;
                } else {
                    writer.write_u16::<LittleEndian>(0)?;
                }
            }
            ColumnType::Int => {
                if let Some(i) = value.as_i64() {
                    writer.write_i32::<LittleEndian>(i as i32)?;
                } else {
                    writer.write_i32::<LittleEndian>(0)?;
                }
            }
            ColumnType::UInt => {
                if let Some(u) = value.as_u64() {
                    writer.write_u32::<LittleEndian>(u as u32)?;
                } else {
                    writer.write_u32::<LittleEndian>(0)?;
                }
            }
            ColumnType::Long => {
                if let Some(i) = value.as_i64() {
                    writer.write_i64::<LittleEndian>(i)?;
                } else {
                    writer.write_i64::<LittleEndian>(0)?;
                }
            }
            ColumnType::ULong => {
                if let Some(u) = value.as_u64() {
                    writer.write_u64::<LittleEndian>(u)?;
                } else {
                    writer.write_u64::<LittleEndian>(0)?;
                }
            }
            ColumnType::Float => {
                if let Some(f) = value.as_f64() {
                    writer.write_f32::<LittleEndian>(f as f32)?;
                } else {
                    writer.write_f32::<LittleEndian>(0.0)?;
                }
            }
            ColumnType::Double => {
                if let Some(f) = value.as_f64() {
                    writer.write_f64::<LittleEndian>(f)?;
                } else {
                    writer.write_f64::<LittleEndian>(0.0)?;
                }
            }
            ColumnType::String | ColumnType::Json | ColumnType::DateTime => {
                if let Some(s) = value.as_string() {
                    let bytes = s.as_bytes();
                    writer.write_u32::<LittleEndian>(bytes.len() as u32)?;
                    writer.write_all(bytes)?;
                } else {
                    writer.write_u32::<LittleEndian>(0)?;
                }
            }
            ColumnType::Binary => {
                // For now, binary is stored as string representation
                writer.write_u32::<LittleEndian>(0)?;
            }
        }

        Ok(())
    }

    /// Writes all accumulated features to the output
    pub fn finish(mut self) -> Result<W> {
        if self.features_written {
            return Ok(self.writer);
        }

        // Write magic bytes
        self.writer.write_all(MAGIC_BYTES)?;

        // Update header with feature count
        self.header.features_count = Some(self.features.len() as u64);

        // Calculate extent from all features
        if !self.bboxes.is_empty() {
            let mut extent = BoundingBox::empty();
            for bbox in &self.bboxes {
                extent.expand(bbox);
            }
            if extent.is_valid() {
                self.header.extent = Some([extent.min_x, extent.min_y, extent.max_x, extent.max_y]);
            }
        }

        // Write header size placeholder
        let header_size_pos = self.writer.stream_position()?;
        self.writer.write_u32::<LittleEndian>(0)?;

        // Write header
        let header_start = self.writer.stream_position()?;
        self.header.write(&mut self.writer)?;
        let header_end = self.writer.stream_position()?;
        let header_size = header_end - header_start;

        // Go back and write actual header size
        self.writer.seek(SeekFrom::Start(header_size_pos))?;
        self.writer.write_u32::<LittleEndian>(header_size as u32)?;
        self.writer.seek(SeekFrom::Start(header_end))?;

        // Write spatial index if enabled
        if self.header.has_index && !self.bboxes.is_empty() {
            let index = PackedRTree::build(self.bboxes.clone(), PackedRTree::DEFAULT_NODE_SIZE)?;
            index.write(&mut self.writer)?;
        }

        // Write features
        for feature_bytes in &self.features {
            // Write feature size
            self.writer
                .write_u32::<LittleEndian>(feature_bytes.len() as u32)?;
            // Write feature data
            self.writer.write_all(feature_bytes)?;
        }

        self.features_written = true;

        Ok(self.writer)
    }

    /// Returns the header
    #[must_use]
    pub const fn header(&self) -> &Header {
        &self.header
    }

    /// Returns the number of features added so far
    #[must_use]
    pub fn feature_count(&self) -> usize {
        self.features.len()
    }
}

/// Builder for creating `FlatGeobuf` files
pub struct FlatGeobufWriterBuilder {
    header: Header,
}

impl FlatGeobufWriterBuilder {
    /// Creates a new builder with the specified geometry type
    #[must_use]
    pub fn new(geometry_type: crate::header::GeometryType) -> Self {
        Self {
            header: Header::new(geometry_type),
        }
    }

    /// Sets the Z dimension flag
    #[must_use]
    pub fn with_z(mut self) -> Self {
        self.header = self.header.with_z();
        self
    }

    /// Sets the M dimension flag
    #[must_use]
    pub fn with_m(mut self) -> Self {
        self.header = self.header.with_m();
        self
    }

    /// Enables spatial index
    #[must_use]
    pub fn with_index(mut self) -> Self {
        self.header = self.header.with_index(true);
        self
    }

    /// Sets the CRS
    #[must_use]
    pub fn with_crs(mut self, crs: crate::header::CrsInfo) -> Self {
        self.header = self.header.with_crs(crs);
        self
    }

    /// Adds a column
    #[must_use]
    pub fn with_column(mut self, column: Column) -> Self {
        self.header.columns.push(column);
        self
    }

    /// Builds the writer
    pub fn build<W: Write + Seek>(self, writer: W) -> Result<FlatGeobufWriter<W>> {
        FlatGeobufWriter::new(writer, self.header)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::header::GeometryType;
    use oxigdal_core::vector::{Geometry, Point};
    use std::io::Cursor;

    #[test]
    fn test_writer_builder() {
        let builder = FlatGeobufWriterBuilder::new(GeometryType::Point)
            .with_z()
            .with_index()
            .with_column(Column::new("name", ColumnType::String));

        let cursor = Cursor::new(Vec::new());
        let writer = builder.build(cursor).ok();
        assert!(writer.is_some());
    }

    #[test]
    fn test_write_simple_feature() {
        let header = Header::new(GeometryType::Point);
        let cursor = Cursor::new(Vec::new());
        let writer = FlatGeobufWriter::new(cursor, header).ok();
        assert!(writer.is_some());

        let mut writer = writer.expect("writer creation failed");

        let point = Point::new(10.0, 20.0);
        let feature = Feature::new(Geometry::Point(point));

        let result = writer.add_feature(&feature);
        assert!(result.is_ok());

        assert_eq!(writer.feature_count(), 1);
    }

    #[test]
    fn test_write_feature_with_properties() {
        let mut header = Header::new(GeometryType::Point);
        header.add_column(Column::new("name", ColumnType::String));
        header.add_column(Column::new("value", ColumnType::Int));

        let cursor = Cursor::new(Vec::new());
        let writer = FlatGeobufWriter::new(cursor, header).ok();
        assert!(writer.is_some());

        let mut writer = writer.expect("writer creation failed");

        let point = Point::new(10.0, 20.0);
        let mut feature = Feature::new(Geometry::Point(point));
        feature.set_property("name", PropertyValue::String("Test".to_string()));
        feature.set_property("value", PropertyValue::Integer(42));

        let result = writer.add_feature(&feature);
        assert!(result.is_ok());

        assert_eq!(writer.feature_count(), 1);
    }
}
