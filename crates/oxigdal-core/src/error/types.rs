//! Error type definitions for OxiGDAL

#[cfg(not(feature = "std"))]
use alloc::string::String;
#[cfg(not(feature = "std"))]
use core::fmt;

#[cfg(feature = "std")]
use thiserror::Error;

/// The main error type for `OxiGDAL`
#[derive(Debug)]
#[cfg_attr(feature = "std", derive(Error))]
pub enum OxiGdalError {
    /// I/O error occurred
    #[cfg_attr(feature = "std", error("I/O error: {0}"))]
    Io(IoError),

    /// Invalid data format
    #[cfg_attr(feature = "std", error("Format error: {0}"))]
    Format(FormatError),

    /// Coordinate reference system error
    #[cfg_attr(feature = "std", error("CRS error: {0}"))]
    Crs(CrsError),

    /// Compression/decompression error
    #[cfg_attr(feature = "std", error("Compression error: {0}"))]
    Compression(CompressionError),

    /// Invalid parameter
    #[cfg_attr(feature = "std", error("Invalid parameter '{parameter}': {message}"))]
    InvalidParameter {
        /// The parameter name
        parameter: &'static str,
        /// Error message
        message: String,
    },

    /// Operation not supported
    #[cfg_attr(feature = "std", error("Not supported: {operation}"))]
    NotSupported {
        /// The unsupported operation
        operation: String,
    },

    /// Out of bounds access
    #[cfg_attr(feature = "std", error("Out of bounds: {message}"))]
    OutOfBounds {
        /// Error message
        message: String,
    },

    /// Internal error
    #[cfg_attr(feature = "std", error("Internal error: {message}"))]
    Internal {
        /// Error message
        message: String,
    },
}

/// I/O related errors
#[derive(Debug)]
#[cfg_attr(feature = "std", derive(Error))]
pub enum IoError {
    /// File not found
    #[cfg_attr(feature = "std", error("File not found: {path}"))]
    NotFound {
        /// The path that was not found
        path: String,
    },

    /// Permission denied
    #[cfg_attr(feature = "std", error("Permission denied: {path}"))]
    PermissionDenied {
        /// The path that was denied
        path: String,
    },

    /// Network error
    #[cfg_attr(feature = "std", error("Network error: {message}"))]
    Network {
        /// Error message
        message: String,
    },

    /// End of file reached unexpectedly
    #[cfg_attr(feature = "std", error("Unexpected end of file at offset {offset}"))]
    UnexpectedEof {
        /// The offset where EOF was encountered
        offset: u64,
    },

    /// Read error
    #[cfg_attr(feature = "std", error("Read error: {message}"))]
    Read {
        /// Error message
        message: String,
    },

    /// Write error
    #[cfg_attr(feature = "std", error("Write error: {message}"))]
    Write {
        /// Error message
        message: String,
    },

    /// Seek error
    #[cfg_attr(feature = "std", error("Seek error: position {position}"))]
    Seek {
        /// The position that failed
        position: u64,
    },

    /// HTTP error
    #[cfg_attr(feature = "std", error("HTTP error {status}: {message}"))]
    Http {
        /// HTTP status code
        status: u16,
        /// Error message
        message: String,
    },
}

/// Format-related errors
#[derive(Debug)]
#[cfg_attr(feature = "std", derive(Error))]
pub enum FormatError {
    /// Invalid magic number
    #[cfg_attr(
        feature = "std",
        error("Invalid magic number: expected {expected:?}, got {actual:?}")
    )]
    InvalidMagic {
        /// Expected magic bytes
        expected: &'static [u8],
        /// Actual magic bytes
        actual: [u8; 4],
    },

    /// Invalid header
    #[cfg_attr(feature = "std", error("Invalid header: {message}"))]
    InvalidHeader {
        /// Error message
        message: String,
    },

    /// Unsupported version
    #[cfg_attr(feature = "std", error("Unsupported version: {version}"))]
    UnsupportedVersion {
        /// The unsupported version
        version: u32,
    },

    /// Invalid tag
    #[cfg_attr(feature = "std", error("Invalid tag {tag}: {message}"))]
    InvalidTag {
        /// Tag identifier
        tag: u16,
        /// Error message
        message: String,
    },

    /// Missing required tag
    #[cfg_attr(feature = "std", error("Missing required tag: {tag}"))]
    MissingTag {
        /// The missing tag name
        tag: &'static str,
    },

    /// Invalid data type
    #[cfg_attr(feature = "std", error("Invalid data type: {type_id}"))]
    InvalidDataType {
        /// The invalid type ID
        type_id: u16,
    },

    /// Corrupt data
    #[cfg_attr(feature = "std", error("Corrupt data at offset {offset}: {message}"))]
    CorruptData {
        /// Offset where corruption was detected
        offset: u64,
        /// Error message
        message: String,
    },

    /// Invalid `GeoKey`
    #[cfg_attr(feature = "std", error("Invalid GeoKey {key_id}: {message}"))]
    InvalidGeoKey {
        /// `GeoKey` identifier
        key_id: u16,
        /// Error message
        message: String,
    },
}

/// Coordinate reference system errors
#[derive(Debug)]
#[cfg_attr(feature = "std", derive(Error))]
pub enum CrsError {
    /// Unknown CRS
    #[cfg_attr(feature = "std", error("Unknown CRS: {identifier}"))]
    UnknownCrs {
        /// CRS identifier
        identifier: String,
    },

    /// Invalid WKT
    #[cfg_attr(feature = "std", error("Invalid WKT: {message}"))]
    InvalidWkt {
        /// Error message
        message: String,
    },

    /// Invalid EPSG code
    #[cfg_attr(feature = "std", error("Invalid EPSG code: {code}"))]
    InvalidEpsg {
        /// The invalid EPSG code
        code: u32,
    },

    /// Transformation error
    #[cfg_attr(
        feature = "std",
        error("Transformation error from {source_crs} to {target_crs}: {message}")
    )]
    TransformationError {
        /// Source CRS identifier
        source_crs: String,
        /// Target CRS identifier
        target_crs: String,
        /// Error message
        message: String,
    },

    /// Datum not found
    #[cfg_attr(feature = "std", error("Datum not found: {datum}"))]
    DatumNotFound {
        /// Datum name
        datum: String,
    },
}

/// Compression-related errors
#[derive(Debug)]
#[cfg_attr(feature = "std", derive(Error))]
pub enum CompressionError {
    /// Unknown compression method
    #[cfg_attr(feature = "std", error("Unknown compression method: {method}"))]
    UnknownMethod {
        /// Compression method identifier
        method: u16,
    },

    /// Decompression failed
    #[cfg_attr(feature = "std", error("Decompression failed: {message}"))]
    DecompressionFailed {
        /// Error message
        message: String,
    },

    /// Compression failed
    #[cfg_attr(feature = "std", error("Compression failed: {message}"))]
    CompressionFailed {
        /// Error message
        message: String,
    },

    /// Invalid compressed data
    #[cfg_attr(feature = "std", error("Invalid compressed data: {message}"))]
    InvalidData {
        /// Error message
        message: String,
    },
}

impl From<IoError> for OxiGdalError {
    fn from(err: IoError) -> Self {
        Self::Io(err)
    }
}

impl From<FormatError> for OxiGdalError {
    fn from(err: FormatError) -> Self {
        Self::Format(err)
    }
}

impl From<CrsError> for OxiGdalError {
    fn from(err: CrsError) -> Self {
        Self::Crs(err)
    }
}

impl From<CompressionError> for OxiGdalError {
    fn from(err: CompressionError) -> Self {
        Self::Compression(err)
    }
}

#[cfg(feature = "std")]
impl From<std::io::Error> for IoError {
    fn from(err: std::io::Error) -> Self {
        use std::io::ErrorKind;

        match err.kind() {
            ErrorKind::NotFound => Self::NotFound {
                path: err.to_string(),
            },
            ErrorKind::PermissionDenied => Self::PermissionDenied {
                path: err.to_string(),
            },
            ErrorKind::UnexpectedEof => Self::UnexpectedEof { offset: 0 },
            _ => Self::Read {
                message: err.to_string(),
            },
        }
    }
}

#[cfg(feature = "std")]
impl From<std::io::Error> for OxiGdalError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err.into())
    }
}

#[cfg(not(feature = "std"))]
impl fmt::Display for OxiGdalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Format(e) => write!(f, "Format error: {e}"),
            Self::Crs(e) => write!(f, "CRS error: {e}"),
            Self::Compression(e) => write!(f, "Compression error: {e}"),
            Self::InvalidParameter { parameter, message } => {
                write!(f, "Invalid parameter {parameter}: {message}")
            }
            Self::NotSupported { operation } => write!(f, "Not supported: {operation}"),
            Self::OutOfBounds { message } => write!(f, "Out of bounds: {message}"),
            Self::Internal { message } => write!(f, "Internal error: {message}"),
        }
    }
}

#[cfg(not(feature = "std"))]
impl fmt::Display for IoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { path } => write!(f, "File not found: {path}"),
            Self::PermissionDenied { path } => write!(f, "Permission denied: {path}"),
            Self::Network { message } => write!(f, "Network error: {message}"),
            Self::UnexpectedEof { offset } => write!(f, "Unexpected EOF at offset {offset}"),
            Self::Read { message } => write!(f, "Read error: {message}"),
            Self::Write { message } => write!(f, "Write error: {message}"),
            Self::Seek { position } => write!(f, "Seek error: position {position}"),
            Self::Http { status, message } => write!(f, "HTTP error {status}: {message}"),
        }
    }
}

#[cfg(not(feature = "std"))]
impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMagic { expected, actual } => {
                write!(f, "Invalid magic: expected {expected:?}, got {actual:?}")
            }
            Self::InvalidHeader { message } => write!(f, "Invalid header: {message}"),
            Self::UnsupportedVersion { version } => write!(f, "Unsupported version: {version}"),
            Self::InvalidTag { tag, message } => write!(f, "Invalid tag {tag}: {message}"),
            Self::MissingTag { tag } => write!(f, "Missing required tag: {tag}"),
            Self::InvalidDataType { type_id } => write!(f, "Invalid data type: {type_id}"),
            Self::CorruptData { offset, message } => {
                write!(f, "Corrupt data at offset {offset}: {message}")
            }
            Self::InvalidGeoKey { key_id, message } => {
                write!(f, "Invalid GeoKey {key_id}: {message}")
            }
        }
    }
}

#[cfg(not(feature = "std"))]
impl fmt::Display for CrsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownCrs { identifier } => write!(f, "Unknown CRS: {identifier}"),
            Self::InvalidWkt { message } => write!(f, "Invalid WKT: {message}"),
            Self::InvalidEpsg { code } => write!(f, "Invalid EPSG code: {code}"),
            Self::TransformationError {
                source_crs,
                target_crs,
                message,
            } => write!(
                f,
                "Transformation error from {source_crs} to {target_crs}: {message}"
            ),
            Self::DatumNotFound { datum } => write!(f, "Datum not found: {datum}"),
        }
    }
}

#[cfg(not(feature = "std"))]
impl fmt::Display for CompressionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownMethod { method } => write!(f, "Unknown compression method: {method}"),
            Self::DecompressionFailed { message } => write!(f, "Decompression failed: {message}"),
            Self::CompressionFailed { message } => write!(f, "Compression failed: {message}"),
            Self::InvalidData { message } => write!(f, "Invalid compressed data: {message}"),
        }
    }
}
