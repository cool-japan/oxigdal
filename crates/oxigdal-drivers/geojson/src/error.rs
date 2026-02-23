//! Error types for GeoJSON operations
//!
//! This module provides comprehensive error handling for all GeoJSON operations,
//! including parsing, validation, reading, and writing.

use oxigdal_core::error::OxiGdalError;
use thiserror::Error;

/// Result type for GeoJSON operations
pub type Result<T> = core::result::Result<T, GeoJsonError>;

/// Comprehensive error type for GeoJSON operations
#[derive(Debug, Error)]
pub enum GeoJsonError {
    /// JSON parsing error
    #[error("JSON parsing error: {message}")]
    JsonParse {
        /// Error message
        message: String,
        /// Line number (if available)
        line: Option<usize>,
        /// Column number (if available)
        column: Option<usize>,
    },

    /// Invalid GeoJSON structure
    #[error("Invalid GeoJSON: {message}")]
    InvalidStructure {
        /// Error message
        message: String,
    },

    /// Invalid geometry type
    #[error("Invalid geometry type: {geometry_type}")]
    InvalidGeometryType {
        /// The invalid geometry type string
        geometry_type: String,
    },

    /// Invalid coordinates
    #[error("Invalid coordinates: {message}")]
    InvalidCoordinates {
        /// Error message
        message: String,
        /// Position in coordinate array (if known)
        position: Option<usize>,
    },

    /// Invalid feature
    #[error("Invalid feature: {message}")]
    InvalidFeature {
        /// Error message
        message: String,
        /// Feature ID (if available)
        feature_id: Option<String>,
    },

    /// Invalid feature collection
    #[error("Invalid feature collection: {message}")]
    InvalidFeatureCollection {
        /// Error message
        message: String,
    },

    /// Invalid CRS (Coordinate Reference System)
    #[error("Invalid CRS: {message}")]
    InvalidCrs {
        /// Error message
        message: String,
    },

    /// Invalid bounding box
    #[error("Invalid bounding box: {message}")]
    InvalidBbox {
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

    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization error
    #[error("Serialization error: {message}")]
    Serialization {
        /// Error message
        message: String,
    },

    /// Deserialization error
    #[error("Deserialization error: {message}")]
    Deserialization {
        /// Error message
        message: String,
    },

    /// Unsupported feature
    #[error("Unsupported feature: {feature}")]
    Unsupported {
        /// The unsupported feature
        feature: String,
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

impl From<serde_json::Error> for GeoJsonError {
    fn from(err: serde_json::Error) -> Self {
        Self::JsonParse {
            message: err.to_string(),
            line: Some(err.line()),
            column: Some(err.column()),
        }
    }
}

impl GeoJsonError {
    /// Creates a new invalid structure error
    pub fn invalid_structure<S: Into<String>>(message: S) -> Self {
        Self::InvalidStructure {
            message: message.into(),
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

    /// Creates a new unsupported feature error
    pub fn unsupported<S: Into<String>>(feature: S) -> Self {
        Self::Unsupported {
            feature: feature.into(),
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
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = GeoJsonError::invalid_structure("missing type field");
        assert!(err.to_string().contains("missing type field"));

        let err = GeoJsonError::invalid_coordinates_at("NaN value", 5);
        assert!(err.to_string().contains("NaN value"));

        let err = GeoJsonError::validation_at("invalid polygon", "features/0/geometry");
        assert!(err.to_string().contains("invalid polygon"));
    }

    #[test]
    fn test_json_error_conversion() {
        let json_str = r#"{"invalid": json}"#;
        let err: std::result::Result<serde_json::Value, serde_json::Error> =
            serde_json::from_str(json_str);
        match err {
            Err(e) => {
                let geojson_err: GeoJsonError = e.into();
                if let GeoJsonError::JsonParse { line, column, .. } = geojson_err {
                    assert!(line.is_some());
                    assert!(column.is_some());
                } else {
                    panic!("Expected JsonParse error");
                }
            }
            Ok(_) => panic!("Expected error"),
        }
    }

    #[test]
    fn test_limit_exceeded() {
        let err = GeoJsonError::limit_exceeded("too many coordinates", 1000, 2000);
        if let GeoJsonError::LimitExceeded { limit, actual, .. } = err {
            assert_eq!(limit, 1000);
            assert_eq!(actual, 2000);
        } else {
            panic!("Expected LimitExceeded error");
        }
    }
}
