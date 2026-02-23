//! Error handling for Node.js bindings
//!
//! This module provides error conversion from OxiGDAL errors to JavaScript exceptions.

use napi::bindgen_prelude::*;
use napi_derive::napi;
use oxigdal_core::error::OxiGdalError;
use std::fmt;

/// Error type for Node.js bindings
#[derive(Debug)]
pub struct NodeError {
    /// Error message
    pub message: String,
    /// Error code for JavaScript
    pub code: String,
}

impl fmt::Display for NodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for NodeError {}

impl From<NodeError> for Error {
    fn from(err: NodeError) -> Self {
        Error::new(
            Status::GenericFailure,
            format!("{}: {}", err.code, err.message),
        )
    }
}

impl From<OxiGdalError> for NodeError {
    fn from(err: OxiGdalError) -> Self {
        let (code, message) = match err {
            OxiGdalError::Io(e) => ("IO_ERROR".to_string(), format!("I/O error: {}", e)),
            OxiGdalError::Format(e) => ("FORMAT_ERROR".to_string(), format!("Format error: {}", e)),
            OxiGdalError::Crs(e) => ("CRS_ERROR".to_string(), format!("CRS error: {}", e)),
            OxiGdalError::Compression(e) => (
                "COMPRESSION_ERROR".to_string(),
                format!("Compression error: {}", e),
            ),
            OxiGdalError::InvalidParameter { parameter, message } => (
                "INVALID_PARAMETER".to_string(),
                format!("Invalid parameter '{}': {}", parameter, message),
            ),
            OxiGdalError::NotSupported { operation } => (
                "NOT_SUPPORTED".to_string(),
                format!("Not supported: {}", operation),
            ),
            OxiGdalError::OutOfBounds { message } => (
                "OUT_OF_BOUNDS".to_string(),
                format!("Out of bounds: {}", message),
            ),
            OxiGdalError::Internal { message } => (
                "INTERNAL_ERROR".to_string(),
                format!("Internal error: {}", message),
            ),
        };

        NodeError { message, code }
    }
}

impl From<oxigdal_algorithms::error::AlgorithmError> for NodeError {
    fn from(err: oxigdal_algorithms::error::AlgorithmError) -> Self {
        let (code, message) = match err {
            oxigdal_algorithms::error::AlgorithmError::Core(e) => {
                return e.into();
            }
            oxigdal_algorithms::error::AlgorithmError::InvalidDimensions {
                message,
                actual,
                expected,
            } => (
                "INVALID_DIMENSIONS".to_string(),
                format!("{}: got {}, expected {}", message, actual, expected),
            ),
            oxigdal_algorithms::error::AlgorithmError::EmptyInput { operation } => (
                "EMPTY_INPUT".to_string(),
                format!("Empty input: {}", operation),
            ),
            oxigdal_algorithms::error::AlgorithmError::InvalidInput(msg) => {
                ("INVALID_INPUT".to_string(), msg)
            }
            oxigdal_algorithms::error::AlgorithmError::InvalidParameter { parameter, message } => (
                "INVALID_PARAMETER".to_string(),
                format!("Invalid parameter '{}': {}", parameter, message),
            ),
            oxigdal_algorithms::error::AlgorithmError::InvalidGeometry(msg) => {
                ("INVALID_GEOMETRY".to_string(), msg)
            }
            oxigdal_algorithms::error::AlgorithmError::IncompatibleTypes {
                source_type,
                target_type,
            } => (
                "INCOMPATIBLE_TYPES".to_string(),
                format!("Incompatible types: {} and {}", source_type, target_type),
            ),
            oxigdal_algorithms::error::AlgorithmError::InsufficientData { operation, message } => (
                "INSUFFICIENT_DATA".to_string(),
                format!("Insufficient data for {}: {}", operation, message),
            ),
            oxigdal_algorithms::error::AlgorithmError::NumericalError { operation, message } => (
                "NUMERICAL_ERROR".to_string(),
                format!("Numerical error in {}: {}", operation, message),
            ),
            oxigdal_algorithms::error::AlgorithmError::ComputationError(msg) => {
                ("COMPUTATION_ERROR".to_string(), msg)
            }
            oxigdal_algorithms::error::AlgorithmError::GeometryError { message } => {
                ("GEOMETRY_ERROR".to_string(), message)
            }
            oxigdal_algorithms::error::AlgorithmError::UnsupportedOperation { operation } => (
                "UNSUPPORTED_OPERATION".to_string(),
                format!("Unsupported operation: {}", operation),
            ),
            oxigdal_algorithms::error::AlgorithmError::AllocationFailed { message } => (
                "ALLOCATION_FAILED".to_string(),
                format!("Memory allocation failed: {}", message),
            ),
            oxigdal_algorithms::error::AlgorithmError::SimdNotAvailable => (
                "SIMD_NOT_AVAILABLE".to_string(),
                String::from("SIMD feature not available"),
            ),
            oxigdal_algorithms::error::AlgorithmError::PathNotFound(msg) => (
                "PATH_NOT_FOUND".to_string(),
                format!("Path not found: {}", msg),
            ),
        };

        NodeError { message, code }
    }
}

/// Helper trait for converting Results to napi Results
pub trait ToNapiResult<T> {
    /// Convert to napi Result
    fn to_napi(self) -> Result<T>;
}

impl<T, E> ToNapiResult<T> for std::result::Result<T, E>
where
    E: Into<NodeError>,
{
    fn to_napi(self) -> Result<T> {
        self.map_err(|e| {
            let node_err: NodeError = e.into();
            node_err.into()
        })
    }
}

/// Creates a JavaScript Error with code property
#[allow(dead_code)]
#[napi]
pub fn create_error(code: String, message: String) -> Error {
    Error::new(Status::GenericFailure, format!("{}: {}", code, message))
}

/// Error codes exposed to JavaScript
#[allow(dead_code)]
#[napi(object)]
pub struct ErrorCodes {
    pub io_error: String,
    pub format_error: String,
    pub crs_error: String,
    pub compression_error: String,
    pub invalid_parameter: String,
    pub not_supported: String,
    pub out_of_bounds: String,
    pub internal_error: String,
    pub invalid_input: String,
    pub computation_error: String,
    pub geometry_error: String,
    pub algorithm_error: String,
    pub unknown_error: String,
}

/// Returns all error codes
#[allow(dead_code)]
#[napi]
pub fn get_error_codes() -> ErrorCodes {
    ErrorCodes {
        io_error: "IO_ERROR".to_string(),
        format_error: "FORMAT_ERROR".to_string(),
        crs_error: "CRS_ERROR".to_string(),
        compression_error: "COMPRESSION_ERROR".to_string(),
        invalid_parameter: "INVALID_PARAMETER".to_string(),
        not_supported: "NOT_SUPPORTED".to_string(),
        out_of_bounds: "OUT_OF_BOUNDS".to_string(),
        internal_error: "INTERNAL_ERROR".to_string(),
        invalid_input: "INVALID_INPUT".to_string(),
        computation_error: "COMPUTATION_ERROR".to_string(),
        geometry_error: "GEOMETRY_ERROR".to_string(),
        algorithm_error: "ALGORITHM_ERROR".to_string(),
        unknown_error: "UNKNOWN_ERROR".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigdal_core::error::FormatError;

    #[test]
    fn test_error_conversion() {
        let core_err = OxiGdalError::Format(FormatError::InvalidHeader {
            message: "test".to_string(),
        });
        let node_err: NodeError = core_err.into();
        assert_eq!(node_err.code, "FORMAT_ERROR");
        assert!(node_err.message.contains("test"));
    }

    #[test]
    fn test_error_display() {
        let err = NodeError {
            message: "test message".to_string(),
            code: "TEST_CODE".to_string(),
        };
        assert_eq!(format!("{}", err), "TEST_CODE: test message");
    }
}
