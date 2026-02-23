//! Error types for `FlatGeobuf` driver

use thiserror::Error;

/// Result type for `FlatGeobuf` operations
pub type Result<T> = core::result::Result<T, FlatGeobufError>;

/// Error types for `FlatGeobuf` operations
#[derive(Debug, Error)]
pub enum FlatGeobufError {
    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// `OxiGDAL` core error
    #[error("OxiGDAL error: {0}")]
    OxiGdal(#[from] oxigdal_core::OxiGdalError),

    /// Invalid magic bytes
    #[error("Invalid magic bytes: expected {:?}, got {:?}", expected, actual)]
    InvalidMagic {
        /// Expected magic bytes
        expected: &'static [u8],
        /// Actual magic bytes read
        actual: Vec<u8>,
    },

    /// Unsupported version
    #[error("Unsupported FlatGeobuf version: {0}")]
    UnsupportedVersion(u8),

    /// Invalid header
    #[error("Invalid header: {0}")]
    InvalidHeader(String),

    /// Invalid geometry
    #[error("Invalid geometry: {0}")]
    InvalidGeometry(String),

    /// Invalid feature
    #[error("Invalid feature: {0}")]
    InvalidFeature(String),

    /// Index error
    #[error("Index error: {0}")]
    IndexError(String),

    /// Unsupported geometry type
    #[error("Unsupported geometry type: {0}")]
    UnsupportedGeometryType(u8),

    /// Unsupported column type
    #[error("Unsupported column type: {0}")]
    UnsupportedColumnType(u8),

    /// Missing required field
    #[error("Missing required field: {0}")]
    MissingField(&'static str),

    /// Invalid index
    #[error("Invalid spatial index: {0}")]
    InvalidIndex(String),

    /// Feature not found
    #[error("Feature not found with ID: {0}")]
    FeatureNotFound(u64),

    /// Buffer too small
    #[error("Buffer too small: need {needed} bytes, have {available}")]
    BufferTooSmall {
        /// Bytes needed
        needed: usize,
        /// Bytes available
        available: usize,
    },

    /// Invalid offset
    #[error("Invalid offset: {0}")]
    InvalidOffset(u64),

    /// HTTP error
    #[cfg(feature = "http")]
    #[error("HTTP error: {0}")]
    Http(String),

    /// Reqwest error
    #[cfg(feature = "http")]
    #[error("Request error: {0}")]
    Reqwest(#[from] reqwest::Error),

    /// `FlatBuffers` error
    #[error("FlatBuffers error: {0}")]
    FlatBuffers(String),

    /// UTF-8 conversion error
    #[error("UTF-8 error: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Not supported
    #[error("Not supported: {0}")]
    NotSupported(String),
}

impl From<flatbuffers::InvalidFlatbuffer> for FlatGeobufError {
    fn from(err: flatbuffers::InvalidFlatbuffer) -> Self {
        Self::FlatBuffers(format!("{err:?}"))
    }
}

impl From<std::str::Utf8Error> for FlatGeobufError {
    fn from(err: std::str::Utf8Error) -> Self {
        Self::FlatBuffers(format!("UTF-8 error: {err}"))
    }
}
