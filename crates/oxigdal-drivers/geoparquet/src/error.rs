//! Error types for GeoParquet operations
//!
//! This module provides error handling for the GeoParquet driver,
//! including errors from Arrow/Parquet operations, geometry encoding,
//! and metadata validation.

#[cfg(not(feature = "std"))]
use alloc::string::String;

/// Result type for GeoParquet operations
pub type Result<T> = core::result::Result<T, GeoParquetError>;

/// Errors that can occur during GeoParquet operations
#[derive(Debug, thiserror::Error)]
pub enum GeoParquetError {
    /// Error from the Arrow library
    #[cfg(feature = "std")]
    #[error("Arrow error: {0}")]
    Arrow(#[from] arrow::error::ArrowError),

    /// Error from the Parquet library
    #[cfg(feature = "std")]
    #[error("Parquet error: {0}")]
    Parquet(#[from] parquet::errors::ParquetError),

    /// Error from oxigdal-core
    #[cfg(feature = "std")]
    #[error("OxiGDAL core error: {0}")]
    Core(#[from] oxigdal_core::error::OxiGdalError),

    /// I/O error
    #[cfg(feature = "std")]
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization/deserialization error
    #[cfg(feature = "std")]
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Invalid GeoParquet metadata
    #[error("Invalid GeoParquet metadata: {message}")]
    InvalidMetadata {
        /// Error message
        message: String,
    },

    /// Invalid geometry encoding
    #[error("Invalid geometry encoding: {message}")]
    InvalidGeometry {
        /// Error message
        message: String,
    },

    /// Unsupported feature
    #[error("Unsupported feature: {feature}")]
    Unsupported {
        /// Feature name
        feature: String,
    },

    /// Invalid schema
    #[error("Invalid schema: {message}")]
    InvalidSchema {
        /// Error message
        message: String,
    },

    /// Missing required field
    #[error("Missing required field: {field}")]
    MissingField {
        /// Field name
        field: String,
    },

    /// Invalid WKB geometry
    #[error("Invalid WKB: {message}")]
    InvalidWkb {
        /// Error message
        message: String,
    },

    /// Invalid CRS specification
    #[error("Invalid CRS: {message}")]
    InvalidCrs {
        /// Error message
        message: String,
    },

    /// Invalid bounding box
    #[error("Invalid bounding box: {message}")]
    InvalidBoundingBox {
        /// Error message
        message: String,
    },

    /// Type mismatch
    #[error("Type mismatch: expected {expected}, found {found}")]
    TypeMismatch {
        /// Expected type
        expected: String,
        /// Found type
        found: String,
    },

    /// Out of bounds access
    #[error("Index out of bounds: {index} >= {length}")]
    OutOfBounds {
        /// Index
        index: usize,
        /// Length
        length: usize,
    },

    /// Internal error
    #[error("Internal error: {message}")]
    Internal {
        /// Error message
        message: String,
    },
}

impl GeoParquetError {
    /// Creates an invalid metadata error
    pub fn invalid_metadata(message: impl Into<String>) -> Self {
        Self::InvalidMetadata {
            message: message.into(),
        }
    }

    /// Creates an invalid geometry error
    pub fn invalid_geometry(message: impl Into<String>) -> Self {
        Self::InvalidGeometry {
            message: message.into(),
        }
    }

    /// Creates an unsupported feature error
    pub fn unsupported(feature: impl Into<String>) -> Self {
        Self::Unsupported {
            feature: feature.into(),
        }
    }

    /// Creates an invalid schema error
    pub fn invalid_schema(message: impl Into<String>) -> Self {
        Self::InvalidSchema {
            message: message.into(),
        }
    }

    /// Creates a missing field error
    pub fn missing_field(field: impl Into<String>) -> Self {
        Self::MissingField {
            field: field.into(),
        }
    }

    /// Creates an invalid WKB error
    pub fn invalid_wkb(message: impl Into<String>) -> Self {
        Self::InvalidWkb {
            message: message.into(),
        }
    }

    /// Creates an invalid CRS error
    pub fn invalid_crs(message: impl Into<String>) -> Self {
        Self::InvalidCrs {
            message: message.into(),
        }
    }

    /// Creates an invalid bounding box error
    pub fn invalid_bbox(message: impl Into<String>) -> Self {
        Self::InvalidBoundingBox {
            message: message.into(),
        }
    }

    /// Creates a type mismatch error
    pub fn type_mismatch(expected: impl Into<String>, found: impl Into<String>) -> Self {
        Self::TypeMismatch {
            expected: expected.into(),
            found: found.into(),
        }
    }

    /// Creates an out of bounds error
    pub fn out_of_bounds(index: usize, length: usize) -> Self {
        Self::OutOfBounds { index, length }
    }

    /// Creates an internal error
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let err = GeoParquetError::invalid_metadata("test");
        assert!(matches!(err, GeoParquetError::InvalidMetadata { .. }));

        let err = GeoParquetError::unsupported("feature");
        assert!(matches!(err, GeoParquetError::Unsupported { .. }));
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_error_display() {
        let err = GeoParquetError::invalid_metadata("test message");
        let display = format!("{err}");
        assert!(display.contains("test message"));
    }
}
