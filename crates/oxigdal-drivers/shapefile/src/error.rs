//! Error types for Shapefile operations
//!
//! This module provides comprehensive error handling for all Shapefile operations,
//! including parsing .shp, .dbf, and .shx files, validation, reading, and writing.

use oxigdal_core::error::OxiGdalError;
use thiserror::Error;

/// Result type for Shapefile operations
pub type Result<T> = core::result::Result<T, ShapefileError>;

/// Comprehensive error type for Shapefile operations
#[derive(Debug, Error)]
pub enum ShapefileError {
    /// Invalid Shapefile header
    #[error("Invalid Shapefile header: {message}")]
    InvalidHeader {
        /// Error message
        message: String,
    },

    /// Invalid file code
    #[error("Invalid file code: expected 9994, got {actual}")]
    InvalidFileCode {
        /// Actual file code encountered
        actual: i32,
    },

    /// Invalid version
    #[error("Invalid version: {version}")]
    InvalidVersion {
        /// The invalid version number
        version: i32,
    },

    /// Unsupported shape type
    #[error("Unsupported shape type: {shape_type}")]
    UnsupportedShapeType {
        /// The unsupported shape type code
        shape_type: i32,
    },

    /// Invalid shape type
    #[error("Invalid shape type: {shape_type}")]
    InvalidShapeType {
        /// The invalid shape type code
        shape_type: i32,
    },

    /// Invalid geometry
    #[error("Invalid geometry: {message}")]
    InvalidGeometry {
        /// Error message
        message: String,
        /// Record number (if available)
        record: Option<usize>,
    },

    /// Invalid coordinates
    #[error("Invalid coordinates: {message}")]
    InvalidCoordinates {
        /// Error message
        message: String,
        /// Position in coordinate array (if known)
        position: Option<usize>,
    },

    /// Invalid bounding box
    #[error("Invalid bounding box: {message}")]
    InvalidBbox {
        /// Error message
        message: String,
    },

    /// DBF parsing error
    #[error("DBF error: {message}")]
    DbfError {
        /// Error message
        message: String,
        /// Field name (if applicable)
        field: Option<String>,
        /// Record number (if applicable)
        record: Option<usize>,
    },

    /// Invalid DBF header
    #[error("Invalid DBF header: {message}")]
    InvalidDbfHeader {
        /// Error message
        message: String,
    },

    /// Invalid field descriptor
    #[error("Invalid field descriptor: {message}")]
    InvalidFieldDescriptor {
        /// Error message
        message: String,
        /// Field name (if known)
        field: Option<String>,
    },

    /// Invalid field value
    #[error("Invalid field value: {message}")]
    InvalidFieldValue {
        /// Error message
        message: String,
        /// Field name
        field: String,
        /// Record number
        record: usize,
    },

    /// Encoding error
    #[error("Encoding error: {message}")]
    EncodingError {
        /// Error message
        message: String,
        /// Code page (if known)
        code_page: Option<u8>,
    },

    /// SHX index error
    #[error("SHX index error: {message}")]
    ShxError {
        /// Error message
        message: String,
        /// Record number (if applicable)
        record: Option<usize>,
    },

    /// Record mismatch between .shp and .dbf
    #[error("Record count mismatch: .shp has {shp_count} records, .dbf has {dbf_count} records")]
    RecordMismatch {
        /// Number of records in .shp file
        shp_count: usize,
        /// Number of records in .dbf file
        dbf_count: usize,
    },

    /// Missing required file
    #[error("Missing required file: {file_type}")]
    MissingFile {
        /// File type (e.g., ".shp", ".dbf", ".shx")
        file_type: String,
    },

    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// EOF (End of File) error
    #[error("Unexpected end of file: {message}")]
    UnexpectedEof {
        /// Error message
        message: String,
    },

    /// Validation error
    #[error("Validation error: {message}")]
    Validation {
        /// Error message
        message: String,
        /// Path to the invalid element
        path: Option<String>,
    },

    /// Topology error
    #[error("Topology error: {message}")]
    Topology {
        /// Error message
        message: String,
    },

    /// Out of memory error
    #[error("Out of memory: {message}")]
    OutOfMemory {
        /// Error message
        message: String,
    },

    /// Limit exceeded
    #[error("Limit exceeded: {message}")]
    LimitExceeded {
        /// Error message
        message: String,
        /// The limit that was exceeded
        limit: usize,
        /// The actual value
        actual: usize,
    },

    /// Generic OxiGDAL error
    #[error("OxiGDAL error: {0}")]
    OxiGdal(#[from] OxiGdalError),
}

impl ShapefileError {
    /// Creates a new invalid header error
    pub fn invalid_header<S: Into<String>>(message: S) -> Self {
        Self::InvalidHeader {
            message: message.into(),
        }
    }

    /// Creates a new invalid geometry error
    pub fn invalid_geometry<S: Into<String>>(message: S) -> Self {
        Self::InvalidGeometry {
            message: message.into(),
            record: None,
        }
    }

    /// Creates a new invalid geometry error with record number
    pub fn invalid_geometry_at<S: Into<String>>(message: S, record: usize) -> Self {
        Self::InvalidGeometry {
            message: message.into(),
            record: Some(record),
        }
    }

    /// Creates a new invalid coordinates error
    pub fn invalid_coordinates<S: Into<String>>(message: S) -> Self {
        Self::InvalidCoordinates {
            message: message.into(),
            position: None,
        }
    }

    /// Creates a new invalid coordinates error with position
    pub fn invalid_coordinates_at<S: Into<String>>(message: S, position: usize) -> Self {
        Self::InvalidCoordinates {
            message: message.into(),
            position: Some(position),
        }
    }

    /// Creates a new DBF error
    pub fn dbf_error<S: Into<String>>(message: S) -> Self {
        Self::DbfError {
            message: message.into(),
            field: None,
            record: None,
        }
    }

    /// Creates a new DBF error with field and record
    pub fn dbf_error_at<S: Into<String>, F: Into<String>>(
        message: S,
        field: F,
        record: usize,
    ) -> Self {
        Self::DbfError {
            message: message.into(),
            field: Some(field.into()),
            record: Some(record),
        }
    }

    /// Creates a new encoding error
    pub fn encoding_error<S: Into<String>>(message: S) -> Self {
        Self::EncodingError {
            message: message.into(),
            code_page: None,
        }
    }

    /// Creates a new validation error
    pub fn validation<S: Into<String>>(message: S) -> Self {
        Self::Validation {
            message: message.into(),
            path: None,
        }
    }

    /// Creates a new validation error with path
    pub fn validation_at<S: Into<String>, P: Into<String>>(message: S, path: P) -> Self {
        Self::Validation {
            message: message.into(),
            path: Some(path.into()),
        }
    }

    /// Creates a new topology error
    pub fn topology<S: Into<String>>(message: S) -> Self {
        Self::Topology {
            message: message.into(),
        }
    }

    /// Creates a new limit exceeded error
    pub fn limit_exceeded<S: Into<String>>(message: S, limit: usize, actual: usize) -> Self {
        Self::LimitExceeded {
            message: message.into(),
            limit,
            actual,
        }
    }

    /// Creates a new unexpected EOF error
    pub fn unexpected_eof<S: Into<String>>(message: S) -> Self {
        Self::UnexpectedEof {
            message: message.into(),
        }
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = ShapefileError::invalid_header("missing file code");
        assert!(err.to_string().contains("missing file code"));

        let err = ShapefileError::invalid_geometry_at("invalid polygon", 5);
        assert!(err.to_string().contains("invalid polygon"));

        let err = ShapefileError::InvalidFileCode { actual: 1234 };
        assert!(err.to_string().contains("9994"));
        assert!(err.to_string().contains("1234"));
    }

    #[test]
    fn test_record_mismatch() {
        let err = ShapefileError::RecordMismatch {
            shp_count: 100,
            dbf_count: 95,
        };
        assert!(err.to_string().contains("100"));
        assert!(err.to_string().contains("95"));
    }

    #[test]
    fn test_dbf_error_construction() {
        let err = ShapefileError::dbf_error_at("invalid value", "name", 42);
        if let ShapefileError::DbfError {
            field,
            record,
            message,
        } = err
        {
            assert_eq!(field, Some("name".to_string()));
            assert_eq!(record, Some(42));
            assert_eq!(message, "invalid value");
        } else {
            panic!("Expected DbfError");
        }
    }
}
