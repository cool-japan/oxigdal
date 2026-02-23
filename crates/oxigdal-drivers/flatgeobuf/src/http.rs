//! HTTP range request support for cloud-native `FlatGeobuf` access
//!
//! Enables efficient reading of `FlatGeobuf` files from HTTP sources using
//! range requests to fetch only needed portions of the file.

use crate::MAGIC_BYTES;
use crate::error::{FlatGeobufError, Result};
use crate::geometry::GeometryCodec;
use crate::header::Header;
use crate::index::{BoundingBox, PackedRTree};
use byteorder::{LittleEndian, ReadBytesExt};
use oxigdal_core::vector::Feature;
use std::io::{Cursor, Read};

/// HTTP reader for `FlatGeobuf` files
#[cfg(feature = "http")]
pub struct HttpReader {
    url: String,
    client: reqwest::blocking::Client,
    header: Header,
    geometry_codec: GeometryCodec,
    index: Option<PackedRTree>,
    features_offset: u64,
    file_size: Option<u64>,
}

#[cfg(feature = "http")]
impl HttpReader {
    /// Creates a new HTTP reader for the given URL
    pub fn new(url: String) -> Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .build()
            .map_err(|e| FlatGeobufError::Http(format!("Failed to create HTTP client: {e}")))?;

        // Read header and index using range requests
        let mut reader = Self {
            url: url.clone(),
            client: client.clone(),
            header: Header::default(),
            geometry_codec: GeometryCodec::new(false, false),
            index: None,
            features_offset: 0,
            file_size: None,
        };

        reader.initialize()?;

        Ok(reader)
    }

    /// Initializes by reading header and index
    fn initialize(&mut self) -> Result<()> {
        // Get file size using HEAD request
        let head_response = self
            .client
            .head(&self.url)
            .send()
            .map_err(|e| FlatGeobufError::Http(format!("HEAD request failed: {e}")))?;

        self.file_size = head_response
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v: &reqwest::header::HeaderValue| v.to_str().ok())
            .and_then(|s: &str| s.parse::<u64>().ok());

        // Read first chunk to get header (magic + header size + header + potential index)
        // Request first 1MB which should be enough for most headers and indices
        let initial_chunk = self.read_range(0, 1024 * 1024)?;
        let mut cursor = Cursor::new(&initial_chunk);

        // Verify magic bytes
        let mut magic = [0u8; 8];
        cursor.read_exact(&mut magic)?;

        if &magic != MAGIC_BYTES {
            return Err(FlatGeobufError::InvalidMagic {
                expected: MAGIC_BYTES,
                actual: magic.to_vec(),
            });
        }

        // Read header size
        let _header_size = cursor.read_u32::<LittleEndian>()?;

        // Read header
        self.header = Header::read(&mut cursor)?;

        // Update geometry codec
        self.geometry_codec = GeometryCodec::new(self.header.has_z, self.header.has_m);

        // Read spatial index if present
        if self.header.has_index {
            let feature_count = self.header.features_count.ok_or_else(|| {
                FlatGeobufError::InvalidHeader(
                    "Feature count required when index is present".to_string(),
                )
            })?;

            self.index = Some(PackedRTree::read(&mut cursor, feature_count)?);
        }

        // Record features offset
        self.features_offset = cursor.position();

        Ok(())
    }

    /// Reads a byte range from the URL
    fn read_range(&self, start: u64, length: u64) -> Result<Vec<u8>> {
        let end = start + length - 1;
        let range_header = format!("bytes={start}-{end}");

        let response = self
            .client
            .get(&self.url)
            .header(reqwest::header::RANGE, range_header)
            .send()
            .map_err(|e| FlatGeobufError::Http(format!("Range request failed: {e}")))?;

        if !response.status().is_success()
            && response.status() != reqwest::StatusCode::PARTIAL_CONTENT
        {
            return Err(FlatGeobufError::Http(format!(
                "HTTP error: {}",
                response.status()
            )));
        }

        let bytes = response
            .bytes()
            .map_err(|e| FlatGeobufError::Http(format!("Failed to read response: {e}")))?;

        Ok(bytes.to_vec())
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

    /// Reads a feature by index
    pub fn read_feature_by_index(&self, index: u64) -> Result<Feature> {
        // If we have a spatial index, use it to find the feature offset
        if let Some(ref spatial_index) = self.index {
            if index >= spatial_index.nodes.len() as u64 {
                return Err(FlatGeobufError::FeatureNotFound(index));
            }

            let node = &spatial_index.nodes[index as usize];
            let offset = self.features_offset + node.offset;

            // Read feature size first (4 bytes)
            let size_bytes = self.read_range(offset, 4)?;
            let mut cursor = Cursor::new(&size_bytes);
            let feature_size = cursor.read_u32::<LittleEndian>()?;

            // Read feature data
            let feature_bytes = self.read_range(offset + 4, u64::from(feature_size))?;
            let feature = self.parse_feature(&feature_bytes)?;

            Ok(feature)
        } else {
            Err(FlatGeobufError::NotSupported(
                "Reading by index requires spatial index".to_string(),
            ))
        }
    }

    /// Queries features in a bounding box
    pub fn query_bbox(&self, bbox: &BoundingBox) -> Result<Vec<Feature>> {
        if let Some(ref index) = self.index {
            let offsets = index.search(bbox);
            let mut features = Vec::with_capacity(offsets.len());

            for offset in offsets {
                match self.read_feature_by_index(offset) {
                    Ok(feature) => features.push(feature),
                    Err(e) => {
                        // Log error but continue with other features
                        eprintln!("Warning: Failed to read feature {offset}: {e}");
                    }
                }
            }

            Ok(features)
        } else {
            Err(FlatGeobufError::NotSupported(
                "Spatial queries require spatial index".to_string(),
            ))
        }
    }

    /// Parses feature from bytes
    fn parse_feature(&self, data: &[u8]) -> Result<Feature> {
        let mut cursor = Cursor::new(data);

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
                feature.set_property(
                    column.name.clone(),
                    oxigdal_core::vector::PropertyValue::Null,
                );
                continue;
            }

            let value = self.read_property_value(&mut cursor, column)?;
            feature.set_property(column.name.clone(), value);
        }

        Ok(feature)
    }

    /// Reads a property value
    fn read_property_value<R: std::io::Read>(
        &self,
        reader: &mut R,
        column: &crate::header::Column,
    ) -> Result<oxigdal_core::vector::PropertyValue> {
        use crate::header::ColumnType;
        use oxigdal_core::vector::PropertyValue;

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

/// Async HTTP reader for `FlatGeobuf` files
#[cfg(all(feature = "http", feature = "async"))]
pub struct AsyncHttpReader {
    url: String,
    client: reqwest::Client,
    header: Header,
    geometry_codec: GeometryCodec,
    index: Option<PackedRTree>,
    features_offset: u64,
}

#[cfg(all(feature = "http", feature = "async"))]
impl AsyncHttpReader {
    /// Creates a new async HTTP reader
    pub async fn new(url: String) -> Result<Self> {
        let client = reqwest::Client::builder()
            .build()
            .map_err(|e| FlatGeobufError::Http(format!("Failed to create HTTP client: {e}")))?;

        let mut reader = Self {
            url: url.clone(),
            client: client.clone(),
            header: Header::default(),
            geometry_codec: GeometryCodec::new(false, false),
            index: None,
            features_offset: 0,
        };

        reader.initialize().await?;

        Ok(reader)
    }

    /// Initializes by reading header and index
    async fn initialize(&mut self) -> Result<()> {
        // Read initial chunk
        let initial_chunk = self.read_range(0, 1024 * 1024).await?;
        let mut cursor = Cursor::new(&initial_chunk);

        // Verify magic bytes
        let mut magic = [0u8; 8];
        cursor.read_exact(&mut magic)?;

        if &magic != MAGIC_BYTES {
            return Err(FlatGeobufError::InvalidMagic {
                expected: MAGIC_BYTES,
                actual: magic.to_vec(),
            });
        }

        // Read header
        let _header_size = cursor.read_u32::<LittleEndian>()?;
        self.header = Header::read(&mut cursor)?;
        self.geometry_codec = GeometryCodec::new(self.header.has_z, self.header.has_m);

        // Read index if present
        if self.header.has_index {
            let feature_count = self.header.features_count.ok_or_else(|| {
                FlatGeobufError::InvalidHeader(
                    "Feature count required when index is present".to_string(),
                )
            })?;

            self.index = Some(PackedRTree::read(&mut cursor, feature_count)?);
        }

        self.features_offset = cursor.position();

        Ok(())
    }

    /// Reads a byte range
    async fn read_range(&self, start: u64, length: u64) -> Result<Vec<u8>> {
        let end = start + length - 1;
        let range_header = format!("bytes={start}-{end}");

        let response = self
            .client
            .get(&self.url)
            .header(reqwest::header::RANGE, range_header)
            .send()
            .await
            .map_err(|e| FlatGeobufError::Http(format!("Range request failed: {e}")))?;

        if !response.status().is_success()
            && response.status() != reqwest::StatusCode::PARTIAL_CONTENT
        {
            return Err(FlatGeobufError::Http(format!(
                "HTTP error: {}",
                response.status()
            )));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| FlatGeobufError::Http(format!("Failed to read response: {e}")))?;

        Ok(bytes.to_vec())
    }

    /// Returns the header
    #[must_use]
    pub const fn header(&self) -> &Header {
        &self.header
    }

    /// Queries features in a bounding box
    pub async fn query_bbox(&self, bbox: &BoundingBox) -> Result<Vec<Feature>> {
        if let Some(ref index) = self.index {
            let offsets = index.search(bbox);
            let mut features = Vec::with_capacity(offsets.len());

            for offset in offsets {
                match self.read_feature_by_index(offset).await {
                    Ok(feature) => features.push(feature),
                    Err(e) => {
                        eprintln!("Warning: Failed to read feature {offset}: {e}");
                    }
                }
            }

            Ok(features)
        } else {
            Err(FlatGeobufError::NotSupported(
                "Spatial queries require spatial index".to_string(),
            ))
        }
    }

    /// Reads a feature by index
    async fn read_feature_by_index(&self, index: u64) -> Result<Feature> {
        if let Some(ref spatial_index) = self.index {
            if index >= spatial_index.nodes.len() as u64 {
                return Err(FlatGeobufError::FeatureNotFound(index));
            }

            let node = &spatial_index.nodes[index as usize];
            let offset = self.features_offset + node.offset;

            // Read feature size
            let size_bytes = self.read_range(offset, 4).await?;
            let mut cursor = Cursor::new(&size_bytes);
            let feature_size = cursor.read_u32::<LittleEndian>()?;

            // Read feature data
            let feature_bytes = self.read_range(offset + 4, u64::from(feature_size)).await?;

            // Parse feature (same as sync version)
            let mut cursor = Cursor::new(&feature_bytes);
            let has_geometry = cursor.read_u8()? != 0;
            let geometry = if has_geometry {
                Some(
                    self.geometry_codec
                        .decode(&mut cursor, self.header.geometry_type)?,
                )
            } else {
                None
            };

            let feature = if let Some(geom) = geometry {
                Feature::new(geom)
            } else {
                Feature::new_attribute_only()
            };

            Ok(feature)
        } else {
            Err(FlatGeobufError::NotSupported(
                "Reading by index requires spatial index".to_string(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_http_reader_placeholder() {
        // HTTP tests require actual server or mocking
        // Placeholder for future implementation
    }
}
