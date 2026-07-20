//! Error handling for Node.js bindings
//!
//! This module provides error conversion from OxiGeo errors to JavaScript exceptions.

use napi::bindgen_prelude::*;
use napi_derive::napi;
use oxigeo_core::error::OxiGeoError;
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

impl From<OxiGeoError> for NodeError {
    fn from(err: OxiGeoError) -> Self {
        let (code, message) = match err {
            OxiGeoError::Io(e) => ("IO_ERROR".to_string(), format!("I/O error: {}", e)),
            OxiGeoError::Format(e) => ("FORMAT_ERROR".to_string(), format!("Format error: {}", e)),
            OxiGeoError::Crs(e) => ("CRS_ERROR".to_string(), format!("CRS error: {}", e)),
            OxiGeoError::Compression(e) => (
                "COMPRESSION_ERROR".to_string(),
                format!("Compression error: {}", e),
            ),
            OxiGeoError::InvalidParameter { parameter, message } => (
                "INVALID_PARAMETER".to_string(),
                format!("Invalid parameter '{}': {}", parameter, message),
            ),
            OxiGeoError::NotSupported { operation } => (
                "NOT_SUPPORTED".to_string(),
                format!("Not supported: {}", operation),
            ),
            OxiGeoError::OutOfBounds { message } => (
                "OUT_OF_BOUNDS".to_string(),
                format!("Out of bounds: {}", message),
            ),
            OxiGeoError::Internal { message } => (
                "INTERNAL_ERROR".to_string(),
                format!("Internal error: {}", message),
            ),
        };

        NodeError { message, code }
    }
}

impl From<oxigeo_algorithms::error::AlgorithmError> for NodeError {
    fn from(err: oxigeo_algorithms::error::AlgorithmError) -> Self {
        let (code, message) = match err {
            oxigeo_algorithms::error::AlgorithmError::Core(e) => {
                return e.into();
            }
            oxigeo_algorithms::error::AlgorithmError::InvalidDimensions {
                message,
                actual,
                expected,
            } => (
                "INVALID_DIMENSIONS".to_string(),
                format!("{}: got {}, expected {}", message, actual, expected),
            ),
            oxigeo_algorithms::error::AlgorithmError::EmptyInput { operation } => (
                "EMPTY_INPUT".to_string(),
                format!("Empty input: {}", operation),
            ),
            oxigeo_algorithms::error::AlgorithmError::InvalidInput(msg) => {
                ("INVALID_INPUT".to_string(), msg)
            }
            oxigeo_algorithms::error::AlgorithmError::InvalidParameter { parameter, message } => (
                "INVALID_PARAMETER".to_string(),
                format!("Invalid parameter '{}': {}", parameter, message),
            ),
            oxigeo_algorithms::error::AlgorithmError::InvalidGeometry(msg) => {
                ("INVALID_GEOMETRY".to_string(), msg)
            }
            oxigeo_algorithms::error::AlgorithmError::IncompatibleTypes {
                source_type,
                target_type,
            } => (
                "INCOMPATIBLE_TYPES".to_string(),
                format!("Incompatible types: {} and {}", source_type, target_type),
            ),
            oxigeo_algorithms::error::AlgorithmError::InsufficientData { operation, message } => (
                "INSUFFICIENT_DATA".to_string(),
                format!("Insufficient data for {}: {}", operation, message),
            ),
            oxigeo_algorithms::error::AlgorithmError::NumericalError { operation, message } => (
                "NUMERICAL_ERROR".to_string(),
                format!("Numerical error in {}: {}", operation, message),
            ),
            oxigeo_algorithms::error::AlgorithmError::ComputationError(msg) => {
                ("COMPUTATION_ERROR".to_string(), msg)
            }
            oxigeo_algorithms::error::AlgorithmError::GeometryError { message } => {
                ("GEOMETRY_ERROR".to_string(), message)
            }
            oxigeo_algorithms::error::AlgorithmError::UnsupportedOperation { operation } => (
                "UNSUPPORTED_OPERATION".to_string(),
                format!("Unsupported operation: {}", operation),
            ),
            oxigeo_algorithms::error::AlgorithmError::AllocationFailed { message } => (
                "ALLOCATION_FAILED".to_string(),
                format!("Memory allocation failed: {}", message),
            ),
            oxigeo_algorithms::error::AlgorithmError::SimdNotAvailable => (
                "SIMD_NOT_AVAILABLE".to_string(),
                String::from("SIMD feature not available"),
            ),
            oxigeo_algorithms::error::AlgorithmError::PathNotFound(msg) => (
                "PATH_NOT_FOUND".to_string(),
                format!("Path not found: {}", msg),
            ),
            oxigeo_algorithms::error::AlgorithmError::NestingTooDeep {
                context,
                depth,
                max,
            } => (
                "NESTING_TOO_DEEP".to_string(),
                format!(
                    "Expression nesting too deep in {}: depth {} exceeds the maximum of {}",
                    context, depth, max
                ),
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
    use oxigeo_core::error::FormatError;

    #[test]
    fn test_error_conversion() {
        let core_err = OxiGeoError::Format(FormatError::InvalidHeader {
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

    #[test]
    fn test_error_from_io_error() {
        use oxigeo_core::error::{IoError, OxiGeoError};
        let core_err = OxiGeoError::Io(IoError::NotFound {
            path: "file_missing.tif".to_string(),
        });
        let node_err: NodeError = core_err.into();
        assert_eq!(node_err.code, "IO_ERROR");
    }

    #[test]
    fn test_error_from_crs_error() {
        use oxigeo_core::error::{CrsError, OxiGeoError};
        let core_err = OxiGeoError::Crs(CrsError::UnknownCrs {
            identifier: "EPSG:999999".to_string(),
        });
        let node_err: NodeError = core_err.into();
        assert_eq!(node_err.code, "CRS_ERROR");
    }

    #[test]
    fn test_error_from_invalid_parameter() {
        use oxigeo_core::error::OxiGeoError;
        let core_err = OxiGeoError::InvalidParameter {
            parameter: "band_index",
            message: "out of range".to_string(),
        };
        let node_err: NodeError = core_err.into();
        assert_eq!(node_err.code, "INVALID_PARAMETER");
        assert!(node_err.message.contains("band_index"));
    }

    #[test]
    fn test_error_from_not_supported() {
        use oxigeo_core::error::OxiGeoError;
        let core_err = OxiGeoError::NotSupported {
            operation: "JPEG2000 writing".to_string(),
        };
        let node_err: NodeError = core_err.into();
        assert_eq!(node_err.code, "NOT_SUPPORTED");
        assert!(node_err.message.contains("JPEG2000"));
    }

    #[test]
    fn test_error_from_out_of_bounds() {
        use oxigeo_core::error::OxiGeoError;
        let core_err = OxiGeoError::OutOfBounds {
            message: "pixel (100, 100) outside 50x50 raster".to_string(),
        };
        let node_err: NodeError = core_err.into();
        assert_eq!(node_err.code, "OUT_OF_BOUNDS");
    }

    #[test]
    fn test_error_from_internal() {
        use oxigeo_core::error::OxiGeoError;
        let core_err = OxiGeoError::Internal {
            message: "unexpected null pointer".to_string(),
        };
        let node_err: NodeError = core_err.into();
        assert_eq!(node_err.code, "INTERNAL_ERROR");
    }

    #[test]
    fn test_to_napi_result_ok() {
        use oxigeo_core::error::OxiGeoError;
        let result: std::result::Result<i32, OxiGeoError> = Ok(42);
        let napi_result = result.to_napi();
        assert!(napi_result.is_ok());
        assert_eq!(napi_result.expect("should be ok"), 42);
    }

    #[test]
    fn test_to_napi_result_err() {
        use oxigeo_core::error::OxiGeoError;
        let err = OxiGeoError::Internal {
            message: "test".to_string(),
        };
        let result: std::result::Result<i32, OxiGeoError> = Err(err);
        let napi_result = result.to_napi();
        assert!(napi_result.is_err());
    }

    #[test]
    fn test_node_error_is_std_error() {
        let err = NodeError {
            message: "std error test".to_string(),
            code: "STD_ERROR".to_string(),
        };
        // Just ensure it implements std::error::Error
        let _: &dyn std::error::Error = &err;
    }

    #[test]
    fn test_error_from_algorithm_empty_input() {
        use oxigeo_algorithms::error::AlgorithmError;
        let alg_err = AlgorithmError::EmptyInput {
            operation: "hillshade",
        };
        let node_err: NodeError = alg_err.into();
        assert_eq!(node_err.code, "EMPTY_INPUT");
        assert!(node_err.message.contains("hillshade"));
    }

    #[test]
    fn test_error_from_algorithm_invalid_dimensions() {
        use oxigeo_algorithms::error::AlgorithmError;
        let alg_err = AlgorithmError::InvalidDimensions {
            message: "wrong size",
            actual: 100,
            expected: 200,
        };
        let node_err: NodeError = alg_err.into();
        assert_eq!(node_err.code, "INVALID_DIMENSIONS");
    }

    #[test]
    fn test_error_from_algorithm_geometry_error() {
        use oxigeo_algorithms::error::AlgorithmError;
        let alg_err = AlgorithmError::GeometryError {
            message: "self-intersecting polygon".to_string(),
        };
        let node_err: NodeError = alg_err.into();
        assert_eq!(node_err.code, "GEOMETRY_ERROR");
    }

    #[test]
    fn test_error_from_algorithm_unsupported() {
        use oxigeo_algorithms::error::AlgorithmError;
        let alg_err = AlgorithmError::UnsupportedOperation {
            operation: "3D buffering".to_string(),
        };
        let node_err: NodeError = alg_err.into();
        assert_eq!(node_err.code, "UNSUPPORTED_OPERATION");
        assert!(node_err.message.contains("3D buffering"));
    }

    #[test]
    fn test_error_from_algorithm_numerical_error() {
        use oxigeo_algorithms::error::AlgorithmError;
        let alg_err = AlgorithmError::NumericalError {
            operation: "inverse matrix",
            message: "singular matrix".to_string(),
        };
        let node_err: NodeError = alg_err.into();
        assert_eq!(node_err.code, "NUMERICAL_ERROR");
    }

    #[test]
    fn test_error_from_algorithm_allocation_failed() {
        use oxigeo_algorithms::error::AlgorithmError;
        let alg_err = AlgorithmError::AllocationFailed {
            message: "out of memory".to_string(),
        };
        let node_err: NodeError = alg_err.into();
        assert_eq!(node_err.code, "ALLOCATION_FAILED");
    }
}
