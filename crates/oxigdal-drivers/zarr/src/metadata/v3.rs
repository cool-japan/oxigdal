//! Zarr v3 metadata structures
//!
//! This module provides comprehensive metadata types for Zarr v3 specification,
//! including codec pipelines, storage transformers, and advanced chunk grids.

use super::NodeType;
use crate::error::{MetadataError, Result, ZarrError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Zarr v3 array metadata
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArrayMetadataV3 {
    /// Zarr format version (always 3)
    pub zarr_format: u8,
    /// Node type
    pub node_type: NodeType,
    /// Array shape
    pub shape: Vec<usize>,
    /// Data type
    pub data_type: DataType,
    /// Chunk grid
    pub chunk_grid: ChunkGrid,
    /// Chunk key encoding
    pub chunk_key_encoding: ChunkKeyEncoding,
    /// Fill value
    pub fill_value: FillValue,
    /// Codec pipeline
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codecs: Option<Vec<CodecMetadata>>,
    /// Storage transformers
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_transformers: Option<Vec<StorageTransformer>>,
    /// Dimension names
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dimension_names: Option<Vec<Option<String>>>,
    /// Attributes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attributes: Option<serde_json::Map<String, serde_json::Value>>,
    /// Extensions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Vec<Extension>>,
}

/// Data type specification
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DataType {
    /// Simple data type (e.g., "int32", "float64")
    Simple(String),
    /// Structured data type
    Structured(StructuredDataType),
}

impl DataType {
    /// Creates a simple data type
    #[must_use]
    pub fn simple(dtype: impl Into<String>) -> Self {
        Self::Simple(dtype.into())
    }

    /// Returns the data type as a string
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Simple(s) => s.as_str(),
            Self::Structured(_) => "structured",
        }
    }

    /// Returns the item size in bytes
    ///
    /// # Errors
    /// Returns error if data type is unsupported or structured
    pub fn item_size(&self) -> Result<usize> {
        match self {
            Self::Simple(s) => parse_dtype_size(s),
            Self::Structured(st) => Ok(st.total_size()),
        }
    }
}

impl From<String> for DataType {
    fn from(s: String) -> Self {
        Self::Simple(s)
    }
}

impl From<&str> for DataType {
    fn from(s: &str) -> Self {
        Self::Simple(s.to_string())
    }
}

/// Structured data type (for complex types)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructuredDataType {
    /// Field definitions
    pub fields: Vec<FieldDefinition>,
    /// Item size in bytes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub itemsize: Option<usize>,
}

impl StructuredDataType {
    /// Creates a new structured data type
    #[must_use]
    pub fn new(fields: Vec<FieldDefinition>) -> Self {
        Self {
            fields,
            itemsize: None,
        }
    }

    /// Returns the total size in bytes
    #[must_use]
    pub fn total_size(&self) -> usize {
        if let Some(size) = self.itemsize {
            return size;
        }
        self.fields.iter().map(|f| f.size).sum()
    }
}

/// Field definition for structured types
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldDefinition {
    /// Field name
    pub name: String,
    /// Field data type
    pub dtype: String,
    /// Field size in bytes
    pub size: usize,
    /// Field offset in bytes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<usize>,
}

/// Chunk grid configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "name")]
pub enum ChunkGrid {
    /// Regular grid with uniform chunk sizes
    #[serde(rename = "regular")]
    Regular {
        /// Chunk shape configuration
        configuration: RegularGridConfig,
    },
    /// Rectangular grid with varying chunk sizes per dimension
    #[serde(rename = "rectangular")]
    Rectangular {
        /// Chunk shape configuration
        configuration: RectangularGridConfig,
    },
    /// Variable grid with arbitrary chunk boundaries
    #[serde(rename = "variable")]
    Variable {
        /// Chunk boundaries configuration
        configuration: VariableGridConfig,
    },
}

impl ChunkGrid {
    /// Creates a new regular grid
    #[must_use]
    pub fn regular(chunk_shape: Vec<usize>) -> Self {
        Self::Regular {
            configuration: RegularGridConfig { chunk_shape },
        }
    }

    /// Creates a new rectangular grid
    #[must_use]
    pub fn rectangular(chunk_shape: Vec<Vec<usize>>) -> Self {
        Self::Rectangular {
            configuration: RectangularGridConfig { chunk_shape },
        }
    }

    /// Creates a new variable grid
    #[must_use]
    pub fn variable(chunk_boundaries: Vec<Vec<usize>>) -> Self {
        Self::Variable {
            configuration: VariableGridConfig { chunk_boundaries },
        }
    }

    /// Returns the chunk shape for a regular grid
    ///
    /// # Errors
    /// Returns error if this is not a regular grid
    pub fn regular_chunk_shape(&self) -> Result<&[usize]> {
        match self {
            Self::Regular { configuration } => Ok(&configuration.chunk_shape),
            _ => Err(ZarrError::Metadata(MetadataError::InvalidChunkGrid {
                reason: "Not a regular grid".to_string(),
            })),
        }
    }
}

/// Regular grid configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegularGridConfig {
    /// Chunk shape (same for all chunks)
    pub chunk_shape: Vec<usize>,
}

/// Rectangular grid configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RectangularGridConfig {
    /// Chunk shape per dimension (can vary per chunk)
    pub chunk_shape: Vec<Vec<usize>>,
}

/// Variable grid configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VariableGridConfig {
    /// Chunk boundaries per dimension
    pub chunk_boundaries: Vec<Vec<usize>>,
}

/// Chunk key encoding
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "name")]
pub enum ChunkKeyEncoding {
    /// Default encoding (e.g., "c/0/1/2")
    #[serde(rename = "default")]
    Default {
        /// Configuration
        #[serde(skip_serializing_if = "Option::is_none")]
        configuration: Option<DefaultKeyConfig>,
    },
    /// V2 encoding (e.g., "0.1.2")
    #[serde(rename = "v2")]
    V2 {
        /// Configuration
        #[serde(skip_serializing_if = "Option::is_none")]
        configuration: Option<V2KeyConfig>,
    },
}

impl ChunkKeyEncoding {
    /// Creates a default encoding
    #[must_use]
    pub fn default_with_separator(separator: impl Into<String>) -> Self {
        Self::Default {
            configuration: Some(DefaultKeyConfig {
                separator: separator.into(),
            }),
        }
    }

    /// Creates a v2 encoding
    #[must_use]
    pub fn v2_with_separator(separator: impl Into<String>) -> Self {
        Self::V2 {
            configuration: Some(V2KeyConfig {
                separator: separator.into(),
            }),
        }
    }
}

impl Default for ChunkKeyEncoding {
    fn default() -> Self {
        Self::Default {
            configuration: Some(DefaultKeyConfig {
                separator: "/".to_string(),
            }),
        }
    }
}

/// Default key encoding configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DefaultKeyConfig {
    /// Separator character
    pub separator: String,
}

/// V2 key encoding configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct V2KeyConfig {
    /// Separator character
    pub separator: String,
}

/// Fill value specification
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
#[derive(Default)]
pub enum FillValue {
    /// Null fill value
    #[default]
    Null,
    /// Boolean fill value
    Bool(bool),
    /// Integer fill value
    Int(i64),
    /// Float fill value
    Float(f64),
    /// String fill value
    String(String),
    /// Byte array fill value
    Bytes(Vec<u8>),
    /// Structured fill value
    Object(serde_json::Map<String, serde_json::Value>),
}

impl FillValue {
    /// Returns true if this is a null fill value
    #[must_use]
    pub const fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Converts the fill value to a byte representation for the given item size
    ///
    /// Returns a byte vector of exactly `item_size` bytes representing this fill
    /// value. Numeric values are encoded in little-endian byte order.
    ///
    /// # Errors
    /// Returns error if the fill value cannot be represented in `item_size` bytes
    pub fn to_bytes(&self, item_size: usize) -> Result<Vec<u8>> {
        match self {
            Self::Null => Ok(vec![0u8; item_size]),
            Self::Bool(b) => {
                let mut bytes = vec![0u8; item_size];
                if *b && !bytes.is_empty() {
                    bytes[0] = 1;
                }
                Ok(bytes)
            }
            Self::Int(i) => {
                // Encode as little-endian bytes, fitting into item_size
                let full_bytes = i.to_le_bytes();
                let mut bytes = vec![0u8; item_size];
                let copy_len = item_size.min(full_bytes.len());
                bytes[..copy_len].copy_from_slice(&full_bytes[..copy_len]);
                Ok(bytes)
            }
            Self::Float(f) => {
                match item_size {
                    2 => {
                        // float16 approximation: cast to f32 then truncate
                        let f32_val = *f as f32;
                        let bits = f32_val.to_bits();
                        // Simple float16 conversion (IEEE 754 half precision)
                        let sign = (bits >> 31) & 1;
                        let exp = ((bits >> 23) & 0xFF) as i32 - 127 + 15;
                        let mantissa = (bits >> 13) & 0x3FF;
                        let half = if exp <= 0 {
                            (sign << 15) as u16
                        } else if exp >= 31 {
                            ((sign << 15) | 0x7C00) as u16
                        } else {
                            ((sign << 15) | ((exp as u32) << 10) | mantissa) as u16
                        };
                        Ok(half.to_le_bytes().to_vec())
                    }
                    4 => {
                        let f32_val = *f as f32;
                        Ok(f32_val.to_le_bytes().to_vec())
                    }
                    8 => Ok(f.to_le_bytes().to_vec()),
                    _ => {
                        // Default: zero-fill
                        Ok(vec![0u8; item_size])
                    }
                }
            }
            Self::String(s) => {
                let mut bytes = vec![0u8; item_size];
                let copy_len = item_size.min(s.len());
                bytes[..copy_len].copy_from_slice(&s.as_bytes()[..copy_len]);
                Ok(bytes)
            }
            Self::Bytes(b) => {
                let mut bytes = vec![0u8; item_size];
                let copy_len = item_size.min(b.len());
                bytes[..copy_len].copy_from_slice(&b[..copy_len]);
                Ok(bytes)
            }
            Self::Object(_) => {
                // Objects default to zero-fill
                Ok(vec![0u8; item_size])
            }
        }
    }
}

impl<'de> serde::Deserialize<'de> for FillValue {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error as _;
        let value = serde_json::Value::deserialize(deserializer)?;
        match value {
            serde_json::Value::Null => Ok(FillValue::Null),
            serde_json::Value::Bool(b) => Ok(FillValue::Bool(b)),
            serde_json::Value::Number(ref n) => {
                // Prefer Int if the number is an exact integer (no decimal point).
                // serde_json: as_i64() returns None for floats like 42.5.
                if let Some(i) = n.as_i64() {
                    Ok(FillValue::Int(i))
                } else if let Some(f) = n.as_f64() {
                    Ok(FillValue::Float(f))
                } else {
                    Err(D::Error::custom("FillValue number is out of range"))
                }
            }
            serde_json::Value::String(s) => Ok(FillValue::String(s)),
            serde_json::Value::Array(arr) => {
                let bytes = arr
                    .iter()
                    .map(|v| {
                        v.as_u64()
                            .and_then(|n| u8::try_from(n).ok())
                            .ok_or_else(|| {
                                D::Error::custom("expected u8 value in FillValue bytes array")
                            })
                    })
                    .collect::<std::result::Result<Vec<u8>, _>>()?;
                Ok(FillValue::Bytes(bytes))
            }
            serde_json::Value::Object(m) => Ok(FillValue::Object(m)),
        }
    }
}

/// Codec metadata in v3 pipeline
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "name")]
pub enum CodecMetadata {
    /// Transpose codec (array-to-array)
    #[serde(rename = "transpose")]
    Transpose {
        /// Configuration
        configuration: TransposeConfig,
    },
    /// Bytes codec (array-to-bytes)
    #[serde(rename = "bytes")]
    Bytes {
        /// Configuration
        #[serde(skip_serializing_if = "Option::is_none")]
        configuration: Option<BytesConfig>,
    },
    /// Endian codec (array-to-array, changes byte order)
    #[serde(rename = "endian")]
    Endian {
        /// Configuration
        configuration: EndianConfig,
    },
    /// GZip compression (bytes-to-bytes)
    #[serde(rename = "gzip")]
    Gzip {
        /// Configuration
        #[serde(skip_serializing_if = "Option::is_none")]
        configuration: Option<GzipConfig>,
    },
    /// Zstd compression (bytes-to-bytes)
    #[serde(rename = "zstd")]
    Zstd {
        /// Configuration
        #[serde(skip_serializing_if = "Option::is_none")]
        configuration: Option<ZstdConfig>,
    },
    /// Blosc compression (bytes-to-bytes)
    #[serde(rename = "blosc")]
    Blosc {
        /// Configuration
        configuration: BloscConfig,
    },
    /// Sharding codec (bytes-to-bytes with internal structure)
    #[serde(rename = "sharding_indexed")]
    ShardingIndexed {
        /// Configuration
        configuration: ShardingConfig,
    },
    /// CRC32 checksum (bytes-to-bytes)
    #[serde(rename = "crc32c")]
    Crc32c {
        /// Configuration
        #[serde(skip_serializing_if = "Option::is_none")]
        configuration: Option<serde_json::Map<String, serde_json::Value>>,
    },
    /// Generic codec
    #[serde(other)]
    Generic,
}

/// Transpose codec configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransposeConfig {
    /// Axis order
    pub order: Vec<usize>,
}

/// Bytes codec configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BytesConfig {
    /// Endianness ("little" or "big")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endian: Option<String>,
}

/// Endian codec configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EndianConfig {
    /// Endianness ("little" or "big")
    pub endian: String,
}

/// GZip codec configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GzipConfig {
    /// Compression level (0-9)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<u32>,
}

/// Zstd codec configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ZstdConfig {
    /// Compression level (1-22)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<i32>,
    /// Use checksum
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum: Option<bool>,
}

/// Blosc codec configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BloscConfig {
    /// Compression algorithm (e.g., "lz4", "zstd", "zlib")
    pub cname: String,
    /// Compression level (0-9)
    pub clevel: u8,
    /// Shuffle mode (0=noshuffle, 1=shuffle, 2=bitshuffle)
    pub shuffle: u8,
    /// Type size for shuffle
    #[serde(skip_serializing_if = "Option::is_none")]
    pub typesize: Option<usize>,
    /// Block size
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocksize: Option<usize>,
}

/// Sharding codec configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShardingConfig {
    /// Chunks per shard
    pub chunk_shape: Vec<usize>,
    /// Codecs for sub-chunks
    pub codecs: Vec<CodecMetadata>,
    /// Index codecs
    pub index_codecs: Vec<CodecMetadata>,
    /// Index location ("start" or "end")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_location: Option<String>,
}

/// Storage transformer (e.g., encryption, checksums)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StorageTransformer {
    /// Encryption transformer
    #[serde(rename = "encryption")]
    Encryption {
        /// Configuration
        configuration: EncryptionConfig,
    },
    /// Checksum transformer
    #[serde(rename = "checksum")]
    Checksum {
        /// Configuration
        configuration: ChecksumConfig,
    },
    /// Generic transformer
    #[serde(other)]
    Generic,
}

/// Encryption configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EncryptionConfig {
    /// Algorithm (e.g., "AES-256-GCM")
    pub algorithm: String,
    /// Key identifier
    pub key_id: String,
    /// Additional parameters
    #[serde(flatten)]
    pub params: HashMap<String, serde_json::Value>,
}

/// Checksum configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChecksumConfig {
    /// Algorithm (e.g., "CRC32", "SHA256")
    pub algorithm: String,
    /// Digest format ("hex", "base64")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
}

/// Zarr extension metadata
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Extension {
    /// Extension name/URI
    pub extension: String,
    /// Must understand flag
    #[serde(skip_serializing_if = "Option::is_none")]
    pub must_understand: Option<bool>,
    /// Configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configuration: Option<serde_json::Map<String, serde_json::Value>>,
}

/// Zarr v3 group metadata
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupMetadataV3 {
    /// Zarr format version (always 3)
    pub zarr_format: u8,
    /// Node type
    pub node_type: NodeType,
    /// Attributes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attributes: Option<serde_json::Map<String, serde_json::Value>>,
    /// Extensions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Vec<Extension>>,
}

impl ArrayMetadataV3 {
    /// Creates new v3 array metadata
    #[must_use]
    pub fn new(shape: Vec<usize>, chunks: Vec<usize>, data_type: impl Into<DataType>) -> Self {
        Self {
            zarr_format: 3,
            node_type: NodeType::Array,
            shape,
            data_type: data_type.into(),
            chunk_grid: ChunkGrid::regular(chunks),
            chunk_key_encoding: ChunkKeyEncoding::default(),
            fill_value: FillValue::default(),
            codecs: None,
            storage_transformers: None,
            dimension_names: None,
            attributes: None,
            extensions: None,
        }
    }

    /// Sets the fill value
    #[must_use]
    pub fn with_fill_value(mut self, fill_value: FillValue) -> Self {
        self.fill_value = fill_value;
        self
    }

    /// Sets the codecs
    #[must_use]
    pub fn with_codecs(mut self, codecs: Vec<CodecMetadata>) -> Self {
        self.codecs = Some(codecs);
        self
    }

    /// Sets the storage transformers
    #[must_use]
    pub fn with_storage_transformers(mut self, transformers: Vec<StorageTransformer>) -> Self {
        self.storage_transformers = Some(transformers);
        self
    }

    /// Sets the dimension names
    #[must_use]
    pub fn with_dimension_names(mut self, names: Vec<Option<String>>) -> Self {
        self.dimension_names = Some(names);
        self
    }

    /// Sets the attributes
    #[must_use]
    pub fn with_attributes(mut self, attrs: serde_json::Map<String, serde_json::Value>) -> Self {
        self.attributes = Some(attrs);
        self
    }

    /// Returns the number of dimensions
    #[must_use]
    pub fn ndim(&self) -> usize {
        self.shape.len()
    }

    /// Returns the total number of elements
    #[must_use]
    pub fn size(&self) -> usize {
        self.shape.iter().product()
    }

    /// Validates the metadata
    ///
    /// # Errors
    /// Returns error if metadata is invalid
    pub fn validate(&self) -> Result<()> {
        if self.zarr_format != 3 {
            return Err(ZarrError::UnsupportedVersion {
                version: self.zarr_format,
            });
        }

        if self.shape.is_empty() {
            return Err(ZarrError::Metadata(MetadataError::InvalidShape {
                reason: "Shape cannot be empty".to_string(),
            }));
        }

        if let Some(names) = &self.dimension_names {
            if names.len() != self.shape.len() {
                return Err(ZarrError::Metadata(MetadataError::InvalidDimensionNames {
                    expected: self.shape.len(),
                    found: names.len(),
                }));
            }
        }

        Ok(())
    }
}

impl GroupMetadataV3 {
    /// Creates new v3 group metadata
    #[must_use]
    pub fn new() -> Self {
        Self {
            zarr_format: 3,
            node_type: NodeType::Group,
            attributes: None,
            extensions: None,
        }
    }

    /// Sets the attributes
    #[must_use]
    pub fn with_attributes(mut self, attrs: serde_json::Map<String, serde_json::Value>) -> Self {
        self.attributes = Some(attrs);
        self
    }
}

impl Default for GroupMetadataV3 {
    fn default() -> Self {
        Self::new()
    }
}

/// Parses data type string to get item size
fn parse_dtype_size(dtype: &str) -> Result<usize> {
    // Parse numpy-style dtype strings (e.g., "<f4", ">i8", "|S10")
    let dtype = dtype.trim();

    // Handle endianness prefix
    let dtype = if dtype.starts_with('<') || dtype.starts_with('>') || dtype.starts_with('|') {
        &dtype[1..]
    } else {
        dtype
    };

    // Parse type and size
    if let Some(size_str) = dtype.get(1..) {
        if let Ok(size) = size_str.parse::<usize>() {
            return Ok(size);
        }
    }

    // Handle common types
    match dtype {
        "bool" => Ok(1),
        "int8" | "uint8" | "i1" | "u1" | "b1" => Ok(1),
        "int16" | "uint16" | "i2" | "u2" => Ok(2),
        "int32" | "uint32" | "i4" | "u4" | "f4" => Ok(4),
        "int64" | "uint64" | "i8" | "u8" | "f8" => Ok(8),
        "float16" | "f2" => Ok(2),
        "float32" => Ok(4),
        "float64" => Ok(8),
        "complex64" | "c8" => Ok(8),
        "complex128" | "c16" => Ok(16),
        _ => Err(ZarrError::Metadata(MetadataError::UnsupportedDataType {
            dtype: dtype.to_string(),
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_array_metadata_v3_creation() {
        let metadata = ArrayMetadataV3::new(vec![100, 200, 300], vec![10, 20, 30], "float32");

        assert_eq!(metadata.zarr_format, 3);
        assert_eq!(metadata.shape, vec![100, 200, 300]);
        assert_eq!(metadata.ndim(), 3);
        assert_eq!(metadata.size(), 100 * 200 * 300);
    }

    #[test]
    fn test_array_metadata_v3_validation() {
        let metadata = ArrayMetadataV3::new(vec![100, 200], vec![10, 20], "int32");

        assert!(metadata.validate().is_ok());
    }

    #[test]
    fn test_chunk_grid_regular() {
        let grid = ChunkGrid::regular(vec![10, 20, 30]);
        assert!(matches!(grid, ChunkGrid::Regular { .. }));
        assert_eq!(grid.regular_chunk_shape().expect("shape"), &[10, 20, 30]);
    }

    #[test]
    fn test_fill_value() {
        assert!(FillValue::Null.is_null());
        assert!(!FillValue::Int(0).is_null());
        assert!(!FillValue::Float(0.0).is_null());
    }

    #[test]
    fn test_parse_dtype_size() {
        assert_eq!(parse_dtype_size("<f4").expect("f4"), 4);
        assert_eq!(parse_dtype_size(">i8").expect("i8"), 8);
        assert_eq!(parse_dtype_size("float32").expect("float32"), 4);
        assert_eq!(parse_dtype_size("int64").expect("int64"), 8);
        assert_eq!(parse_dtype_size("bool").expect("bool"), 1);
    }

    #[test]
    fn test_data_type() {
        let dt = DataType::simple("float32");
        assert_eq!(dt.as_str(), "float32");
        assert_eq!(dt.item_size().expect("size"), 4);
    }

    #[test]
    fn test_chunk_key_encoding() {
        let encoding = ChunkKeyEncoding::default_with_separator("/");
        assert!(matches!(encoding, ChunkKeyEncoding::Default { .. }));
    }

    #[test]
    fn test_group_metadata_v3() {
        let metadata = GroupMetadataV3::new();
        assert_eq!(metadata.zarr_format, 3);
        assert_eq!(metadata.node_type, NodeType::Group);
    }

    #[test]
    fn test_fill_value_to_bytes_null() {
        let fv = FillValue::Null;
        let bytes = fv.to_bytes(4).expect("to_bytes");
        assert_eq!(bytes, vec![0, 0, 0, 0]);
    }

    #[test]
    fn test_fill_value_to_bytes_int() {
        let fv = FillValue::Int(42);
        let bytes = fv.to_bytes(4).expect("to_bytes");
        assert_eq!(bytes, 42i64.to_le_bytes()[..4].to_vec());
    }

    #[test]
    fn test_fill_value_to_bytes_float32() {
        let fv = FillValue::Float(2.78);
        let bytes = fv.to_bytes(4).expect("to_bytes");
        let expected = (2.78f32).to_le_bytes();
        assert_eq!(bytes, expected.to_vec());
    }

    #[test]
    fn test_fill_value_to_bytes_float64() {
        let fv = FillValue::Float(2.78);
        let bytes = fv.to_bytes(8).expect("to_bytes");
        let expected = 2.78f64.to_le_bytes();
        assert_eq!(bytes, expected.to_vec());
    }

    #[test]
    fn test_fill_value_to_bytes_bool() {
        let fv = FillValue::Bool(true);
        let bytes = fv.to_bytes(1).expect("to_bytes");
        assert_eq!(bytes, vec![1]);

        let fv = FillValue::Bool(false);
        let bytes = fv.to_bytes(1).expect("to_bytes");
        assert_eq!(bytes, vec![0]);
    }

    #[test]
    fn test_dimension_names_validation() {
        let mut metadata = ArrayMetadataV3::new(vec![100, 200], vec![10, 20], "float32");

        // Wrong number of dimension names
        metadata.dimension_names = Some(vec![Some("x".to_string())]);
        assert!(metadata.validate().is_err());

        // Correct number of dimension names
        metadata.dimension_names = Some(vec![Some("x".to_string()), Some("y".to_string())]);
        assert!(metadata.validate().is_ok());
    }
}
