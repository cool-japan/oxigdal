//! Error types for Zarr operations
//!
//! This module provides a comprehensive error hierarchy for all Zarr-related operations,
//! including storage, codecs, metadata parsing, and data access.

use oxigdal_core::error::{CompressionError, FormatError, IoError, OxiGdalError};

/// Result type for Zarr operations
pub type Result<T> = core::result::Result<T, ZarrError>;

/// Main error type for Zarr operations
#[derive(Debug, thiserror::Error)]
pub enum ZarrError {
    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] IoError),

    /// Storage error
    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),

    /// Metadata error
    #[error("Metadata error: {0}")]
    Metadata(#[from] MetadataError),

    /// Codec error
    #[error("Codec error: {0}")]
    Codec(#[from] CodecError),

    /// Filter error
    #[error("Filter error: {0}")]
    Filter(#[from] FilterError),

    /// Chunk error
    #[error("Chunk error: {0}")]
    Chunk(#[from] ChunkError),

    /// Shard error (v3 sharding extension)
    #[error("Shard error: {0}")]
    Shard(#[from] ShardError),

    /// Invalid array dimension
    #[error("Invalid dimension: {message}")]
    InvalidDimension {
        /// Error message
        message: String,
    },

    /// Invalid shape
    #[error("Invalid shape: expected {expected:?}, got {actual:?}")]
    InvalidShape {
        /// Expected shape
        expected: Vec<usize>,
        /// Actual shape
        actual: Vec<usize>,
    },

    /// Unsupported Zarr version
    #[error("Unsupported Zarr version: {version}")]
    UnsupportedVersion {
        /// Version number
        version: u8,
    },

    /// Operation not supported
    #[error("Operation not supported: {operation}")]
    NotSupported {
        /// Operation description
        operation: String,
    },

    /// Out of bounds access
    #[error("Out of bounds: {message}")]
    OutOfBounds {
        /// Error message
        message: String,
    },

    /// Internal error
    #[error("Internal error: {message}")]
    Internal {
        /// Error message
        message: String,
    },
}

/// Storage-related errors
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// Key not found in store
    #[error("Key not found: {key}")]
    KeyNotFound {
        /// The missing key
        key: String,
    },

    /// Store is read-only
    #[error("Store is read-only")]
    ReadOnly,

    /// Store is write-only
    #[error("Store is write-only")]
    WriteOnly,

    /// Invalid store key
    #[error("Invalid key: {key}")]
    InvalidKey {
        /// The invalid key
        key: String,
    },

    /// Store not found
    #[error("Store not found: {path}")]
    StoreNotFound {
        /// Store path
        path: String,
    },

    /// Concurrent modification
    #[error("Concurrent modification detected for key: {key}")]
    ConcurrentModification {
        /// The key that was modified
        key: String,
    },

    /// Network error
    #[error("Network error: {message}")]
    Network {
        /// Error message
        message: String,
    },

    /// S3 error
    #[error("S3 error: {message}")]
    S3 {
        /// Error message
        message: String,
    },

    /// HTTP error
    #[error("HTTP error {status}: {message}")]
    Http {
        /// HTTP status code
        status: u16,
        /// Error message
        message: String,
    },

    /// Cache error
    #[error("Cache error: {message}")]
    Cache {
        /// Error message
        message: String,
    },

    /// Operation not supported
    #[error("Operation not supported: {operation}")]
    NotSupported {
        /// Operation description
        operation: String,
    },
}

/// Metadata-related errors
#[derive(Debug, thiserror::Error)]
pub enum MetadataError {
    /// Invalid JSON
    #[error("Invalid JSON: {message}")]
    InvalidJson {
        /// Error message
        message: String,
    },

    /// Missing required field
    #[error("Missing required field: {field}")]
    MissingField {
        /// Field name
        field: &'static str,
    },

    /// Invalid field value
    #[error("Invalid field '{field}': {message}")]
    InvalidField {
        /// Field name
        field: &'static str,
        /// Error message
        message: String,
    },

    /// Unsupported data type
    #[error("Unsupported data type: {dtype}")]
    UnsupportedDataType {
        /// Data type string
        dtype: String,
    },

    /// Invalid byte order
    #[error("Invalid byte order: {order}")]
    InvalidByteOrder {
        /// Byte order character
        order: char,
    },

    /// Invalid array order
    #[error("Invalid array order: {order}")]
    InvalidArrayOrder {
        /// Array order character
        order: char,
    },

    /// Invalid dimension separator
    #[error("Invalid dimension separator: {separator}")]
    InvalidDimensionSeparator {
        /// Separator character
        separator: char,
    },

    /// Incompatible metadata versions
    #[error("Incompatible metadata: v2 and v3 cannot be mixed")]
    IncompatibleVersions,

    /// Invalid zarr format
    #[error("Invalid zarr format version: {version}")]
    InvalidZarrFormat {
        /// Format version
        version: String,
    },

    /// Invalid shape
    #[error("Invalid shape: {reason}")]
    InvalidShape {
        /// Error reason
        reason: String,
    },

    /// Invalid chunk grid
    #[error("Invalid chunk grid: {reason}")]
    InvalidChunkGrid {
        /// Error reason
        reason: String,
    },

    /// Invalid dimension names
    #[error("Invalid dimension names: expected {expected}, got {found}")]
    InvalidDimensionNames {
        /// Expected number of names
        expected: usize,
        /// Actual number of names
        found: usize,
    },
}

/// Codec-related errors
#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    /// Unknown codec
    #[error("Unknown codec: {codec}")]
    UnknownCodec {
        /// Codec identifier
        codec: String,
    },

    /// Codec not available
    #[error("Codec not available: {codec} (feature not enabled)")]
    CodecNotAvailable {
        /// Codec name
        codec: String,
    },

    /// Compression failed
    #[error("Compression failed: {message}")]
    CompressionFailed {
        /// Error message
        message: String,
    },

    /// Decompression failed
    #[error("Decompression failed: {message}")]
    DecompressionFailed {
        /// Error message
        message: String,
    },

    /// Invalid codec configuration
    #[error("Invalid codec configuration for {codec}: {message}")]
    InvalidConfiguration {
        /// Codec name
        codec: String,
        /// Error message
        message: String,
    },

    /// Blosc error
    #[error("Blosc error: {message}")]
    Blosc {
        /// Error message
        message: String,
    },

    /// Zstd error
    #[error("Zstd error: {message}")]
    Zstd {
        /// Error message
        message: String,
    },

    /// Gzip error
    #[error("Gzip error: {message}")]
    Gzip {
        /// Error message
        message: String,
    },

    /// LZ4 error
    #[error("LZ4 error: {message}")]
    Lz4 {
        /// Error message
        message: String,
    },
}

/// Filter-related errors
#[derive(Debug, thiserror::Error)]
pub enum FilterError {
    /// Unknown filter
    #[error("Unknown filter: {filter}")]
    UnknownFilter {
        /// Filter identifier
        filter: String,
    },

    /// Filter not available
    #[error("Filter not available: {filter} (feature not enabled)")]
    FilterNotAvailable {
        /// Filter name
        filter: String,
    },

    /// Filter encode failed
    #[error("Filter encode failed: {message}")]
    EncodeFailed {
        /// Error message
        message: String,
    },

    /// Filter decode failed
    #[error("Filter decode failed: {message}")]
    DecodeFailed {
        /// Error message
        message: String,
    },

    /// Invalid filter configuration
    #[error("Invalid filter configuration for {filter}: {message}")]
    InvalidConfiguration {
        /// Filter name
        filter: String,
        /// Error message
        message: String,
    },

    /// Invalid element size for shuffle
    #[error("Invalid element size for shuffle: {size}")]
    InvalidElementSize {
        /// Element size
        size: usize,
    },

    /// Invalid delta dtype
    #[error("Invalid delta dtype: {dtype}")]
    InvalidDeltaDtype {
        /// Data type
        dtype: String,
    },
}

/// Chunk-related errors
#[derive(Debug, thiserror::Error)]
pub enum ChunkError {
    /// Invalid chunk coordinates
    #[error("Invalid chunk coordinates: {coords:?} for shape {shape:?}")]
    InvalidCoordinates {
        /// Chunk coordinates
        coords: Vec<usize>,
        /// Array shape
        shape: Vec<usize>,
    },

    /// Invalid chunk shape
    #[error("Invalid chunk shape: {chunk_shape:?} for array shape {array_shape:?}")]
    InvalidChunkShape {
        /// Chunk shape
        chunk_shape: Vec<usize>,
        /// Array shape
        array_shape: Vec<usize>,
    },

    /// Chunk not found
    #[error("Chunk not found at coordinates: {coords:?}")]
    ChunkNotFound {
        /// Chunk coordinates
        coords: Vec<usize>,
    },

    /// Invalid chunk data size
    #[error("Invalid chunk data size: expected {expected}, got {actual}")]
    InvalidDataSize {
        /// Expected size
        expected: usize,
        /// Actual size
        actual: usize,
    },

    /// Chunk decode error
    #[error("Chunk decode error: {message}")]
    DecodeError {
        /// Error message
        message: String,
    },

    /// Chunk encode error
    #[error("Chunk encode error: {message}")]
    EncodeError {
        /// Error message
        message: String,
    },
}

/// Shard-related errors (Zarr v3 sharding extension)
#[derive(Debug, thiserror::Error)]
pub enum ShardError {
    /// Invalid shard index entry
    #[error("Invalid shard index entry: {reason}")]
    InvalidIndexEntry {
        /// Error reason
        reason: String,
    },

    /// Invalid shard index size
    #[error("Invalid shard index size: expected {expected} bytes, got {found}")]
    InvalidIndexSize {
        /// Expected size
        expected: usize,
        /// Actual size
        found: usize,
    },

    /// Shard index encode failed
    #[error("Shard index encode failed")]
    IndexEncodeFailed {
        /// Source error
        #[source]
        source: std::io::Error,
    },

    /// Shard index decode failed
    #[error("Shard index decode failed")]
    IndexDecodeFailed {
        /// Source error
        #[source]
        source: std::io::Error,
    },

    /// Invalid chunk coordinates
    #[error("Invalid chunk coordinates: expected {expected_dims} dimensions, got {found_dims}")]
    InvalidChunkCoords {
        /// Expected number of dimensions
        expected_dims: usize,
        /// Actual number of dimensions
        found_dims: usize,
    },

    /// Chunk out of bounds
    #[error("Chunk coordinate out of bounds: dim {dim}, coord {coord}, max {max}")]
    ChunkOutOfBounds {
        /// Dimension index
        dim: usize,
        /// Coordinate value
        coord: usize,
        /// Maximum value
        max: usize,
    },

    /// Invalid shard data
    #[error("Invalid shard data: {reason}")]
    InvalidShardData {
        /// Error reason
        reason: String,
    },

    /// Invalid chunk range
    #[error("Invalid chunk range: offset {offset}, size {size}, shard size {shard_size}")]
    InvalidChunkRange {
        /// Chunk offset
        offset: usize,
        /// Chunk size
        size: usize,
        /// Shard size
        shard_size: usize,
    },

    /// Unsupported index location
    #[error("Unsupported index location: {location}")]
    UnsupportedIndexLocation {
        /// Index location
        location: String,
    },

    /// Invalid index location
    #[error("Invalid index location: {location}")]
    InvalidIndexLocation {
        /// Index location string
        location: String,
    },
}

// Conversions from OxiGDAL errors
impl From<OxiGdalError> for ZarrError {
    fn from(err: OxiGdalError) -> Self {
        match err {
            OxiGdalError::Io(e) => Self::Io(e),
            OxiGdalError::Compression(e) => Self::Codec(e.into()),
            OxiGdalError::Format(e) => Self::Metadata(e.into()),
            OxiGdalError::InvalidParameter { parameter, message } => Self::Internal {
                message: format!("Invalid parameter '{parameter}': {message}"),
            },
            OxiGdalError::NotSupported { operation } => Self::NotSupported { operation },
            OxiGdalError::OutOfBounds { message } => Self::OutOfBounds { message },
            OxiGdalError::Internal { message } => Self::Internal { message },
            OxiGdalError::Crs(_) => Self::Internal {
                message: "CRS error in Zarr context".to_string(),
            },
        }
    }
}

impl From<CompressionError> for CodecError {
    fn from(err: CompressionError) -> Self {
        match err {
            CompressionError::UnknownMethod { method } => Self::UnknownCodec {
                codec: format!("{method}"),
            },
            CompressionError::DecompressionFailed { message } => {
                Self::DecompressionFailed { message }
            }
            CompressionError::CompressionFailed { message } => Self::CompressionFailed { message },
            CompressionError::InvalidData { message } => Self::DecompressionFailed { message },
        }
    }
}

impl From<FormatError> for MetadataError {
    fn from(err: FormatError) -> Self {
        match err {
            FormatError::InvalidHeader { message } => Self::InvalidJson { message },
            FormatError::UnsupportedVersion { version } => Self::InvalidZarrFormat {
                version: format!("{version}"),
            },
            FormatError::MissingTag { tag } => Self::MissingField { field: tag },
            FormatError::InvalidDataType { type_id } => Self::UnsupportedDataType {
                dtype: format!("{type_id}"),
            },
            _ => Self::InvalidJson {
                message: format!("{err}"),
            },
        }
    }
}

#[cfg(feature = "std")]
impl From<std::io::Error> for ZarrError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err.into())
    }
}

impl From<serde_json::Error> for ZarrError {
    fn from(err: serde_json::Error) -> Self {
        Self::Metadata(MetadataError::InvalidJson {
            message: err.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = ZarrError::InvalidDimension {
            message: "negative dimension".to_string(),
        };
        assert!(err.to_string().contains("negative dimension"));
    }

    #[test]
    fn test_storage_error() {
        let err = StorageError::KeyNotFound {
            key: "test/key".to_string(),
        };
        assert!(err.to_string().contains("test/key"));
    }

    #[test]
    fn test_codec_error() {
        let err = CodecError::UnknownCodec {
            codec: "unknown".to_string(),
        };
        assert!(err.to_string().contains("unknown"));
    }
}
