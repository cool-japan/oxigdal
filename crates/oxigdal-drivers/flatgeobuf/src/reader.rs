//! `FlatGeobuf` reader implementation
//!
//! Provides streaming reading of `FlatGeobuf` files with support for:
//! - Sequential feature iteration
//! - Spatial filtering using R-tree index
//! - HTTP range requests for cloud-native access

use crate::MAGIC_BYTES;
use crate::error::{FlatGeobufError, Result};
use crate::geometry::GeometryCodec;
use crate::header::{Column, Header};
use crate::index::{BoundingBox, PackedRTree};
use byteorder::{LittleEndian, ReadBytesExt};
use oxigdal_core::vector::{Feature, PropertyValue};
use std::io::{BufReader, Read, Seek, SeekFrom};

/// `FlatGeobuf` reader for synchronous I/O
pub struct FlatGeobufReader<R: Read + Seek> {
    reader: BufReader<R>,
    header: Header,
    geometry_codec: GeometryCodec,
    index: Option<PackedRTree>,
    features_offset: u64,
    current_position: u64,
    feature_count: Option<u64>,
}

impl<R: Read + Seek> FlatGeobufReader<R> {
    /// Creates a new `FlatGeobuf` reader
    pub fn new(reader: R) -> Result<Self> {
        let mut buf_reader = BufReader::new(reader);

        // Read and verify magic bytes
        let mut magic = [0u8; 8];
        buf_reader.read_exact(&mut magic)?;

        if &magic != MAGIC_BYTES {
            return Err(FlatGeobufError::InvalidMagic {
                expected: MAGIC_BYTES,
                actual: magic.to_vec(),
            });
        }

        // Read header size
        let _header_size = buf_reader.read_u32::<LittleEndian>()?;

        // Read header
        let header = Header::read(&mut buf_reader)?;

        // Create geometry codec
        let geometry_codec = GeometryCodec::new(header.has_z, header.has_m);

        // Read spatial index if present
        let index = if header.has_index {
            let feature_count = header.features_count.ok_or_else(|| {
                FlatGeobufError::InvalidHeader(
                    "Feature count required when index is present".to_string(),
                )
            })?;
            Some(PackedRTree::read(&mut buf_reader, feature_count)?)
        } else {
            None
        };

        // Record current position as start of features
        let features_offset = buf_reader.stream_position()?;

        Ok(Self {
            reader: buf_reader,
            header,
            geometry_codec,
            index,
            features_offset,
            current_position: features_offset,
            feature_count: None,
        })
    }

    /// Returns the header
    #[must_use]
    pub const fn header(&self) -> &Header {
        &self.header
    }

    /// Returns the spatial index if present
    #[must_use]
    pub const fn index(&self) -> Option<&PackedRTree> {
        self.index.as_ref()
    }

    /// Returns the feature count if available from the header
    #[must_use]
    pub fn feature_count(&self) -> Option<u64> {
        self.feature_count
    }

    /// Reads a single feature at the current position
    pub fn read_feature(&mut self) -> Result<Option<Feature>> {
        // Read feature size
        let size = match self.reader.read_u32::<LittleEndian>() {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e.into()),
        };

        if size == 0 {
            return Ok(None);
        }

        // Read feature data
        let mut feature_data = vec![0u8; size as usize];
        self.reader.read_exact(&mut feature_data)?;

        // Parse feature
        let feature = self.parse_feature(&feature_data)?;

        Ok(Some(feature))
    }

    /// Parses feature from bytes
    fn parse_feature(&self, data: &[u8]) -> Result<Feature> {
        let mut cursor = std::io::Cursor::new(data);

        // Read geometry if present
        let has_geometry = cursor.read_u8()? != 0;
        let geometry = if has_geometry {
            Some(
                self.geometry_codec
                    .decode(&mut cursor, self.header.geometry_type)?,
            )
        } else {
            None
        };

        let mut feature = if let Some(geom) = geometry {
            Feature::new(geom)
        } else {
            Feature::new_attribute_only()
        };

        // Read properties
        for column in &self.header.columns {
            let is_null = cursor.read_u8()? != 0;
            if is_null {
                feature.set_property(column.name.clone(), PropertyValue::Null);
                continue;
            }

            let value = self.read_property_value(&mut cursor, column)?;
            feature.set_property(column.name.clone(), value);
        }

        Ok(feature)
    }

    /// Reads a property value based on column type
    fn read_property_value<D: Read>(
        &self,
        reader: &mut D,
        column: &Column,
    ) -> Result<PropertyValue> {
        use crate::header::ColumnType;

        match column.column_type {
            ColumnType::Byte => Ok(PropertyValue::Integer(i64::from(reader.read_i8()?))),
            ColumnType::UByte => Ok(PropertyValue::UInteger(u64::from(reader.read_u8()?))),
            ColumnType::Bool => Ok(PropertyValue::Bool(reader.read_u8()? != 0)),
            ColumnType::Short => Ok(PropertyValue::Integer(i64::from(
                reader.read_i16::<LittleEndian>()?,
            ))),
            ColumnType::UShort => Ok(PropertyValue::UInteger(u64::from(
                reader.read_u16::<LittleEndian>()?,
            ))),
            ColumnType::Int => Ok(PropertyValue::Integer(i64::from(
                reader.read_i32::<LittleEndian>()?,
            ))),
            ColumnType::UInt => Ok(PropertyValue::UInteger(u64::from(
                reader.read_u32::<LittleEndian>()?,
            ))),
            ColumnType::Long => Ok(PropertyValue::Integer(reader.read_i64::<LittleEndian>()?)),
            ColumnType::ULong => Ok(PropertyValue::UInteger(reader.read_u64::<LittleEndian>()?)),
            ColumnType::Float => Ok(PropertyValue::Float(f64::from(
                reader.read_f32::<LittleEndian>()?,
            ))),
            ColumnType::Double => Ok(PropertyValue::Float(reader.read_f64::<LittleEndian>()?)),
            ColumnType::String | ColumnType::Json | ColumnType::DateTime => {
                let len = reader.read_u32::<LittleEndian>()?;
                let mut bytes = vec![0u8; len as usize];
                reader.read_exact(&mut bytes)?;
                let s = String::from_utf8(bytes)?;
                Ok(PropertyValue::String(s))
            }
            ColumnType::Binary => {
                let len = reader.read_u32::<LittleEndian>()?;
                let mut bytes = vec![0u8; len as usize];
                reader.read_exact(&mut bytes)?;
                // Store as string for now - could be improved
                Ok(PropertyValue::String(format!("Binary({len} bytes)")))
            }
        }
    }

    /// Returns an iterator over all features
    pub fn features(&mut self) -> Result<FeatureIterator<'_, R>> {
        // Reset to beginning of features
        self.reader.seek(SeekFrom::Start(self.features_offset))?;
        self.current_position = self.features_offset;

        Ok(FeatureIterator { reader: self })
    }

    /// Returns an iterator over features matching the spatial filter
    pub fn features_in_bbox(
        &mut self,
        bbox: BoundingBox,
    ) -> Result<FilteredFeatureIterator<'_, R>> {
        let offsets = if let Some(ref index) = self.index {
            // Use spatial index to find matching features
            index.search(&bbox)
        } else {
            // No index - need to scan all features
            // Return empty for now - full implementation would scan
            return Err(FlatGeobufError::NotSupported(
                "Spatial filtering without index not yet implemented".to_string(),
            ));
        };

        Ok(FilteredFeatureIterator {
            reader: self,
            offsets,
            current_offset_index: 0,
        })
    }

    /// Seeks to a specific feature by index
    pub fn seek_feature(&mut self, index: u64) -> Result<()> {
        if let Some(ref spatial_index) = self.index {
            // Use index to find feature offset
            if index >= spatial_index.nodes.len() as u64 {
                return Err(FlatGeobufError::FeatureNotFound(index));
            }

            let offset = spatial_index.nodes[index as usize].offset;
            self.reader
                .seek(SeekFrom::Start(self.features_offset + offset))?;
            self.current_position = self.features_offset + offset;
            Ok(())
        } else {
            // Without index, need to skip features sequentially
            self.reader.seek(SeekFrom::Start(self.features_offset))?;
            self.current_position = self.features_offset;

            for _ in 0..index {
                let size = self.reader.read_u32::<LittleEndian>()?;
                self.reader.seek(SeekFrom::Current(i64::from(size)))?;
            }

            Ok(())
        }
    }
}

/// Iterator over all features
pub struct FeatureIterator<'a, R: Read + Seek> {
    reader: &'a mut FlatGeobufReader<R>,
}

impl<R: Read + Seek> Iterator for FeatureIterator<'_, R> {
    type Item = Result<Feature>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.reader.read_feature() {
            Ok(Some(feature)) => Some(Ok(feature)),
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        }
    }
}

/// Iterator over features matching a spatial filter
pub struct FilteredFeatureIterator<'a, R: Read + Seek> {
    reader: &'a mut FlatGeobufReader<R>,
    offsets: Vec<u64>,
    current_offset_index: usize,
}

impl<R: Read + Seek> Iterator for FilteredFeatureIterator<'_, R> {
    type Item = Result<Feature>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_offset_index >= self.offsets.len() {
            return None;
        }

        let offset = self.offsets[self.current_offset_index];
        self.current_offset_index += 1;

        match self.reader.seek_feature(offset) {
            Ok(()) => match self.reader.read_feature() {
                Ok(Some(feature)) => Some(Ok(feature)),
                Ok(None) => None,
                Err(e) => Some(Err(e)),
            },
            Err(e) => Some(Err(e)),
        }
    }
}

/// Async `FlatGeobuf` reader
#[cfg(feature = "async")]
pub struct AsyncFlatGeobufReader<R: tokio::io::AsyncRead + tokio::io::AsyncSeek + Unpin> {
    reader: R,
    header: Header,
    geometry_codec: GeometryCodec,
    #[allow(dead_code)] // Will be used in future spatial query methods
    index: Option<PackedRTree>,
    #[allow(dead_code)] // Will be used in future seek operations
    features_offset: u64,
}

#[cfg(feature = "async")]
impl<R: tokio::io::AsyncRead + tokio::io::AsyncSeek + Unpin> AsyncFlatGeobufReader<R> {
    /// Creates a new async `FlatGeobuf` reader
    pub async fn new(mut reader: R) -> Result<Self> {
        use tokio::io::{AsyncReadExt, AsyncSeekExt};

        // Read and verify magic bytes
        let mut magic = [0u8; 8];
        reader.read_exact(&mut magic).await?;

        if &magic != MAGIC_BYTES {
            return Err(FlatGeobufError::InvalidMagic {
                expected: MAGIC_BYTES,
                actual: magic.to_vec(),
            });
        }

        // Read header size
        let mut header_size_bytes = [0u8; 4];
        reader.read_exact(&mut header_size_bytes).await?;
        let header_size = u32::from_le_bytes(header_size_bytes);

        // Read header - simplified for now
        // In full implementation, would use async header reading
        let mut header_bytes = vec![0u8; header_size as usize];
        reader.read_exact(&mut header_bytes).await?;

        let header = {
            let mut cursor = std::io::Cursor::new(&header_bytes);
            Header::read(&mut cursor)?
        };

        let geometry_codec = GeometryCodec::new(header.has_z, header.has_m);

        // Read spatial index if present
        let index = if header.has_index {
            let feature_count = header.features_count.ok_or_else(|| {
                FlatGeobufError::InvalidHeader(
                    "Feature count required when index is present".to_string(),
                )
            })?;

            let index_size = PackedRTree::calculate_node_count(
                feature_count as usize,
                PackedRTree::DEFAULT_NODE_SIZE,
            ) * crate::index::Node::NODE_SIZE;

            let mut index_bytes = vec![0u8; index_size];
            reader.read_exact(&mut index_bytes).await?;

            let mut cursor = std::io::Cursor::new(&index_bytes);
            Some(PackedRTree::read(&mut cursor, feature_count)?)
        } else {
            None
        };

        // Record features offset
        let features_offset = reader.stream_position().await?;

        Ok(Self {
            reader,
            header,
            geometry_codec,
            index,
            features_offset,
        })
    }

    /// Returns the header
    #[must_use]
    pub const fn header(&self) -> &Header {
        &self.header
    }

    /// Reads a single feature
    pub async fn read_feature(&mut self) -> Result<Option<Feature>> {
        use tokio::io::AsyncReadExt;

        // Read feature size
        let mut size_bytes = [0u8; 4];
        match self.reader.read_exact(&mut size_bytes).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e.into()),
        }

        let size = u32::from_le_bytes(size_bytes);
        if size == 0 {
            return Ok(None);
        }

        // Read feature data
        let mut feature_data = vec![0u8; size as usize];
        self.reader.read_exact(&mut feature_data).await?;

        // Parse feature synchronously
        let feature = self.parse_feature(&feature_data)?;

        Ok(Some(feature))
    }

    /// Parses feature from bytes (same as sync version)
    fn parse_feature(&self, data: &[u8]) -> Result<Feature> {
        let mut cursor = std::io::Cursor::new(data);

        let has_geometry = cursor.read_u8()? != 0;
        let geometry = if has_geometry {
            Some(
                self.geometry_codec
                    .decode(&mut cursor, self.header.geometry_type)?,
            )
        } else {
            None
        };

        let mut feature = if let Some(geom) = geometry {
            Feature::new(geom)
        } else {
            Feature::new_attribute_only()
        };

        // Read properties
        for column in &self.header.columns {
            let is_null = cursor.read_u8()? != 0;
            if is_null {
                feature.set_property(column.name.clone(), PropertyValue::Null);
                continue;
            }

            let value = self.read_property_value(&mut cursor, column)?;
            feature.set_property(column.name.clone(), value);
        }

        Ok(feature)
    }

    /// Reads property value (same as sync version)
    fn read_property_value<D: Read>(
        &self,
        reader: &mut D,
        column: &Column,
    ) -> Result<PropertyValue> {
        use crate::header::ColumnType;

        match column.column_type {
            ColumnType::Byte => Ok(PropertyValue::Integer(i64::from(reader.read_i8()?))),
            ColumnType::UByte => Ok(PropertyValue::UInteger(u64::from(reader.read_u8()?))),
            ColumnType::Bool => Ok(PropertyValue::Bool(reader.read_u8()? != 0)),
            ColumnType::Short => Ok(PropertyValue::Integer(i64::from(
                reader.read_i16::<LittleEndian>()?,
            ))),
            ColumnType::UShort => Ok(PropertyValue::UInteger(u64::from(
                reader.read_u16::<LittleEndian>()?,
            ))),
            ColumnType::Int => Ok(PropertyValue::Integer(i64::from(
                reader.read_i32::<LittleEndian>()?,
            ))),
            ColumnType::UInt => Ok(PropertyValue::UInteger(u64::from(
                reader.read_u32::<LittleEndian>()?,
            ))),
            ColumnType::Long => Ok(PropertyValue::Integer(reader.read_i64::<LittleEndian>()?)),
            ColumnType::ULong => Ok(PropertyValue::UInteger(reader.read_u64::<LittleEndian>()?)),
            ColumnType::Float => Ok(PropertyValue::Float(f64::from(
                reader.read_f32::<LittleEndian>()?,
            ))),
            ColumnType::Double => Ok(PropertyValue::Float(reader.read_f64::<LittleEndian>()?)),
            ColumnType::String | ColumnType::Json | ColumnType::DateTime => {
                let len = reader.read_u32::<LittleEndian>()?;
                let mut bytes = vec![0u8; len as usize];
                reader.read_exact(&mut bytes)?;
                let s = String::from_utf8(bytes)?;
                Ok(PropertyValue::String(s))
            }
            ColumnType::Binary => {
                let len = reader.read_u32::<LittleEndian>()?;
                let mut bytes = vec![0u8; len as usize];
                reader.read_exact(&mut bytes)?;
                Ok(PropertyValue::String(format!("Binary({len} bytes)")))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_bytes() {
        assert_eq!(MAGIC_BYTES.len(), 8);
    }
}
