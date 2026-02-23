//! Error types for GRIB format parsing and processing.
//!
//! This module provides comprehensive error handling for GRIB1 and GRIB2 format operations,
//! including parsing errors, validation errors, and I/O errors.

use std::io;

/// Result type for GRIB operations.
pub type Result<T> = std::result::Result<T, GribError>;

/// Comprehensive error type for GRIB operations.
#[derive(Debug, thiserror::Error)]
pub enum GribError {
    /// I/O error occurred during file operations
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// Invalid GRIB magic number or header
    #[error("Invalid GRIB header: expected 'GRIB' magic bytes, found {0:?}")]
    InvalidHeader(Vec<u8>),

    /// Unsupported GRIB edition
    #[error("Unsupported GRIB edition: {0} (only GRIB1 and GRIB2 are supported)")]
    UnsupportedEdition(u8),

    /// Invalid section number
    #[error("Invalid section number: {0}")]
    InvalidSection(u8),

    /// Missing required section
    #[error("Missing required section: {0}")]
    MissingSection(String),

    /// Invalid section length
    #[error("Invalid section length: expected at least {expected}, got {actual}")]
    InvalidSectionLength {
        /// Expected minimum length
        expected: usize,
        /// Actual length found
        actual: usize,
    },

    /// Unsupported grid definition template
    #[error("Unsupported grid definition template: {0}")]
    UnsupportedGridTemplate(u16),

    /// Unsupported product definition template
    #[error("Unsupported product definition template: {0}")]
    UnsupportedProductTemplate(u16),

    /// Unsupported data representation template
    #[error("Unsupported data representation template: {0}")]
    UnsupportedDataTemplate(u16),

    /// Invalid parameter code
    #[error("Invalid parameter: discipline={discipline}, category={category}, number={number}")]
    InvalidParameter {
        /// WMO discipline code
        discipline: u8,
        /// Parameter category
        category: u8,
        /// Parameter number
        number: u8,
    },

    /// Invalid grid definition
    #[error("Invalid grid definition: {0}")]
    InvalidGrid(String),

    /// Data decoding error
    #[error("Data decoding error: {0}")]
    DecodingError(String),

    /// Invalid data representation
    #[error("Invalid data representation: {0}")]
    InvalidDataRepresentation(String),

    /// Invalid bitmap
    #[error("Invalid bitmap: {0}")]
    InvalidBitmap(String),

    /// Invalid level/layer specification
    #[error("Invalid level: type={level_type}, value={value}")]
    InvalidLevel {
        /// Level type code
        level_type: u8,
        /// Level value
        value: f64,
    },

    /// Invalid time specification
    #[error("Invalid time specification: {0}")]
    InvalidTime(String),

    /// Message truncated or incomplete
    #[error("Truncated message: expected {expected} bytes, got {actual} bytes")]
    TruncatedMessage {
        /// Expected message size
        expected: usize,
        /// Actual size found
        actual: usize,
    },

    /// Invalid end marker
    #[error("Invalid end marker: expected '7777', found {0:?}")]
    InvalidEndMarker(Vec<u8>),

    /// Feature not implemented
    #[error("Feature not implemented: {0}")]
    NotImplemented(String),

    /// Unsupported compression or packing method
    #[error("Unsupported packing method: {0}")]
    UnsupportedPacking(String),

    /// Invalid bit offset or bit length
    #[error("Invalid bit operation: {0}")]
    InvalidBitOperation(String),

    /// Coordinate conversion error
    #[error("Coordinate conversion error: {0}")]
    CoordinateError(String),

    /// Generic parsing error
    #[error("Parse error: {0}")]
    ParseError(String),

    /// Value out of valid range
    #[error("Value out of range: {0}")]
    OutOfRange(String),

    /// UTF-8 decoding error
    #[error("UTF-8 error: {0}")]
    Utf8Error(#[from] std::str::Utf8Error),

    /// Serialization/deserialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Integration error with oxigdal-core
    #[error("OxiGDAL integration error: {0}")]
    IntegrationError(String),

    /// Other errors
    #[error("Other error: {0}")]
    Other(String),
}

impl GribError {
    /// Create a new parsing error
    pub fn parse<S: Into<String>>(msg: S) -> Self {
        Self::ParseError(msg.into())
    }

    /// Create a new decoding error
    pub fn decode<S: Into<String>>(msg: S) -> Self {
        Self::DecodingError(msg.into())
    }

    /// Create a new grid error
    pub fn grid<S: Into<String>>(msg: S) -> Self {
        Self::InvalidGrid(msg.into())
    }

    /// Create a new data representation error
    pub fn data_repr<S: Into<String>>(msg: S) -> Self {
        Self::InvalidDataRepresentation(msg.into())
    }

    /// Create a new "not implemented" error
    pub fn not_impl<S: Into<String>>(feature: S) -> Self {
        Self::NotImplemented(feature.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = GribError::InvalidHeader(vec![0x47, 0x52, 0x49, 0x41]);
        assert!(err.to_string().contains("GRIB"));

        let err = GribError::UnsupportedEdition(3);
        assert!(err.to_string().contains("GRIB1 and GRIB2"));

        let err = GribError::InvalidParameter {
            discipline: 0,
            category: 1,
            number: 255,
        };
        assert!(err.to_string().contains("discipline=0"));
    }

    #[test]
    fn test_error_constructors() {
        let err = GribError::parse("test message");
        assert!(matches!(err, GribError::ParseError(_)));

        let err = GribError::decode("test decode");
        assert!(matches!(err, GribError::DecodingError(_)));

        let err = GribError::not_impl("complex packing");
        assert!(matches!(err, GribError::NotImplemented(_)));
    }
}
