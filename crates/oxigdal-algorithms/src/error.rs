//! Error types for OxiGDAL algorithms
//!
//! # Error Codes
//!
//! Each error variant has an associated error code (e.g., A001, A002) for easier
//! debugging and documentation. Error codes are stable across versions.
//!
//! # Helper Methods
//!
//! All error types provide:
//! - `code()` - Returns the error code
//! - `suggestion()` - Returns helpful hints including parameter constraints
//! - `context()` - Returns additional context about the error

#[cfg(not(feature = "std"))]
use core::fmt;

#[cfg(feature = "std")]
use thiserror::Error;

use oxigdal_core::OxiGdalError;

/// Result type for algorithm operations
pub type Result<T> = core::result::Result<T, AlgorithmError>;

/// Algorithm-specific errors
#[derive(Debug)]
#[cfg_attr(feature = "std", derive(Error))]
pub enum AlgorithmError {
    /// Core OxiGDAL error
    #[cfg_attr(feature = "std", error("Core error: {0}"))]
    Core(#[from] OxiGdalError),

    /// Invalid dimensions
    #[cfg_attr(
        feature = "std",
        error("Invalid dimensions: {message} (got {actual}, expected {expected})")
    )]
    InvalidDimensions {
        /// Error message
        message: &'static str,
        /// Actual dimension
        actual: usize,
        /// Expected dimension
        expected: usize,
    },

    /// Empty input
    #[cfg_attr(feature = "std", error("Empty input: {operation}"))]
    EmptyInput {
        /// Operation name
        operation: &'static str,
    },

    /// Invalid input
    #[cfg_attr(feature = "std", error("Invalid input: {0}"))]
    InvalidInput(String),

    /// Invalid parameter
    #[cfg_attr(feature = "std", error("Invalid parameter '{parameter}': {message}"))]
    InvalidParameter {
        /// Parameter name
        parameter: &'static str,
        /// Error message
        message: String,
    },

    /// Invalid geometry
    #[cfg_attr(feature = "std", error("Invalid geometry: {0}"))]
    InvalidGeometry(String),

    /// Incompatible data types
    #[cfg_attr(
        feature = "std",
        error("Incompatible data types: {source_type} and {target_type}")
    )]
    IncompatibleTypes {
        /// Source data type
        source_type: &'static str,
        /// Target data type
        target_type: &'static str,
    },

    /// Insufficient data
    #[cfg_attr(feature = "std", error("Insufficient data for {operation}: {message}"))]
    InsufficientData {
        /// Operation name
        operation: &'static str,
        /// Error message
        message: String,
    },

    /// Numerical error
    #[cfg_attr(feature = "std", error("Numerical error in {operation}: {message}"))]
    NumericalError {
        /// Operation name
        operation: &'static str,
        /// Error message
        message: String,
    },

    /// Computation error
    #[cfg_attr(feature = "std", error("Computation error: {0}"))]
    ComputationError(String),

    /// Geometry error
    #[cfg_attr(feature = "std", error("Geometry error: {message}"))]
    GeometryError {
        /// Error message
        message: String,
    },

    /// Unsupported operation
    #[cfg_attr(feature = "std", error("Unsupported operation: {operation}"))]
    UnsupportedOperation {
        /// Operation description
        operation: String,
    },

    /// Allocation failed
    #[cfg_attr(feature = "std", error("Memory allocation failed: {message}"))]
    AllocationFailed {
        /// Error message
        message: String,
    },

    /// SIMD not available
    #[cfg_attr(
        feature = "std",
        error("SIMD instructions not available on this platform")
    )]
    SimdNotAvailable,

    /// Path not found
    #[cfg_attr(feature = "std", error("Path not found: {0}"))]
    PathNotFound(String),
}

#[cfg(not(feature = "std"))]
impl fmt::Display for AlgorithmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Core(e) => write!(f, "Core error: {e}"),
            Self::InvalidDimensions {
                message,
                actual,
                expected,
            } => write!(
                f,
                "Invalid dimensions: {message} (got {actual}, expected {expected})"
            ),
            Self::EmptyInput { operation } => write!(f, "Empty input: {operation}"),
            Self::InvalidInput(message) => write!(f, "Invalid input: {message}"),
            Self::InvalidParameter { parameter, message } => {
                write!(f, "Invalid parameter '{parameter}': {message}")
            }
            Self::InvalidGeometry(message) => write!(f, "Invalid geometry: {message}"),
            Self::IncompatibleTypes {
                source_type,
                target_type,
            } => write!(f, "Incompatible types: {source_type} and {target_type}"),
            Self::InsufficientData { operation, message } => {
                write!(f, "Insufficient data for {operation}: {message}")
            }
            Self::NumericalError { operation, message } => {
                write!(f, "Numerical error in {operation}: {message}")
            }
            Self::ComputationError(message) => {
                write!(f, "Computation error: {message}")
            }
            Self::GeometryError { message } => write!(f, "Geometry error: {message}"),
            Self::UnsupportedOperation { operation } => {
                write!(f, "Unsupported operation: {operation}")
            }
            Self::AllocationFailed { message } => {
                write!(f, "Memory allocation failed: {message}")
            }
            Self::SimdNotAvailable => write!(f, "SIMD instructions not available"),
            Self::PathNotFound(message) => write!(f, "Path not found: {message}"),
        }
    }
}

impl AlgorithmError {
    /// Get the error code for this algorithm error
    ///
    /// Error codes are stable across versions and can be used for documentation
    /// and error handling.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Core(_) => "A001",
            Self::InvalidDimensions { .. } => "A002",
            Self::EmptyInput { .. } => "A003",
            Self::InvalidInput(_) => "A004",
            Self::InvalidParameter { .. } => "A005",
            Self::InvalidGeometry(_) => "A006",
            Self::IncompatibleTypes { .. } => "A007",
            Self::InsufficientData { .. } => "A008",
            Self::NumericalError { .. } => "A009",
            Self::ComputationError(_) => "A010",
            Self::GeometryError { .. } => "A011",
            Self::UnsupportedOperation { .. } => "A012",
            Self::AllocationFailed { .. } => "A013",
            Self::SimdNotAvailable => "A014",
            Self::PathNotFound(_) => "A015",
        }
    }

    /// Get a helpful suggestion for fixing this algorithm error
    ///
    /// Returns a human-readable suggestion including parameter constraints and valid ranges.
    pub fn suggestion(&self) -> Option<&'static str> {
        match self {
            Self::Core(_) => Some("Check the underlying error for details"),
            Self::InvalidDimensions { message, .. } => {
                // Provide specific suggestions based on common dimension errors
                if message.contains("window") {
                    Some(
                        "Window size must be odd. Try adjusting to the nearest odd number (e.g., 3, 5, 7)",
                    )
                } else if message.contains("kernel") {
                    Some("Kernel size must be odd and positive. Common values are 3, 5, 7, or 9")
                } else {
                    Some("Check that array dimensions match the expected shape")
                }
            }
            Self::EmptyInput { .. } => Some("Provide at least one data point or feature"),
            Self::InvalidInput(_) => Some("Verify input data format and values are correct"),
            Self::InvalidParameter { parameter, message } => {
                // Provide specific suggestions based on parameter name
                if parameter.contains("window") || parameter.contains("kernel") {
                    Some("Window/kernel size must be odd and positive (e.g., 3, 5, 7)")
                } else if parameter.contains("threshold") {
                    Some("Threshold values are typically between 0.0 and 1.0")
                } else if parameter.contains("radius") {
                    Some("Radius must be positive and reasonable for your data resolution")
                } else if parameter.contains("iterations") {
                    Some("Number of iterations must be positive (typically 1-1000)")
                } else if message.contains("odd") {
                    Some("Value must be odd. Try the next odd number (current±1)")
                } else if message.contains("positive") {
                    Some("Value must be greater than zero")
                } else if message.contains("range") {
                    Some("Value must be within the specified range")
                } else {
                    Some("Check parameter documentation for valid values and constraints")
                }
            }
            Self::InvalidGeometry(_) => Some("Verify geometry is valid and not self-intersecting"),
            Self::IncompatibleTypes { .. } => {
                Some("Convert data to compatible types before processing")
            }
            Self::InsufficientData { .. } => Some("Provide more data points for reliable results"),
            Self::NumericalError { .. } => {
                Some("Check for division by zero, overflow, or invalid mathematical operations")
            }
            Self::ComputationError(_) => Some("Verify input data is within acceptable ranges"),
            Self::GeometryError { .. } => Some("Check geometry validity and topology"),
            Self::UnsupportedOperation { .. } => {
                Some("Use a different algorithm or enable required features")
            }
            Self::AllocationFailed { .. } => Some("Reduce data size or increase available memory"),
            Self::SimdNotAvailable => Some(
                "SIMD operations are not supported on this CPU. The algorithm will use scalar fallback",
            ),
            Self::PathNotFound(_) => Some("Verify the path exists and is accessible"),
        }
    }

    /// Get additional context about this algorithm error
    ///
    /// Returns structured context information including parameter names and values.
    pub fn context(&self) -> ErrorContext {
        match self {
            Self::Core(e) => ErrorContext::new("core_error").with_detail("error", e.to_string()),
            Self::InvalidDimensions {
                message,
                actual,
                expected,
            } => ErrorContext::new("invalid_dimensions")
                .with_detail("message", message.to_string())
                .with_detail("actual", actual.to_string())
                .with_detail("expected", expected.to_string()),
            Self::EmptyInput { operation } => {
                ErrorContext::new("empty_input").with_detail("operation", operation.to_string())
            }
            Self::InvalidInput(msg) => {
                ErrorContext::new("invalid_input").with_detail("message", msg.clone())
            }
            Self::InvalidParameter { parameter, message } => ErrorContext::new("invalid_parameter")
                .with_detail("parameter", parameter.to_string())
                .with_detail("message", message.clone()),
            Self::InvalidGeometry(msg) => {
                ErrorContext::new("invalid_geometry").with_detail("message", msg.clone())
            }
            Self::IncompatibleTypes {
                source_type,
                target_type,
            } => ErrorContext::new("incompatible_types")
                .with_detail("source_type", source_type.to_string())
                .with_detail("target_type", target_type.to_string()),
            Self::InsufficientData { operation, message } => ErrorContext::new("insufficient_data")
                .with_detail("operation", operation.to_string())
                .with_detail("message", message.clone()),
            Self::NumericalError { operation, message } => ErrorContext::new("numerical_error")
                .with_detail("operation", operation.to_string())
                .with_detail("message", message.clone()),
            Self::ComputationError(msg) => {
                ErrorContext::new("computation_error").with_detail("message", msg.clone())
            }
            Self::GeometryError { message } => {
                ErrorContext::new("geometry_error").with_detail("message", message.clone())
            }
            Self::UnsupportedOperation { operation } => ErrorContext::new("unsupported_operation")
                .with_detail("operation", operation.clone()),
            Self::AllocationFailed { message } => {
                ErrorContext::new("allocation_failed").with_detail("message", message.clone())
            }
            Self::SimdNotAvailable => ErrorContext::new("simd_not_available"),
            Self::PathNotFound(path) => {
                ErrorContext::new("path_not_found").with_detail("path", path.clone())
            }
        }
    }
}

/// Additional context information for algorithm errors
#[derive(Debug, Clone)]
pub struct ErrorContext {
    /// Error category for grouping similar errors
    pub category: &'static str,
    /// Additional details about the error
    pub details: Vec<(String, String)>,
}

impl ErrorContext {
    /// Create a new error context
    pub fn new(category: &'static str) -> Self {
        Self {
            category,
            details: Vec::new(),
        }
    }

    /// Add a detail to the context
    pub fn with_detail(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.details.push((key.into(), value.into()));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = AlgorithmError::InvalidParameter {
            parameter: "window_size",
            message: "must be positive".to_string(),
        };
        let s = format!("{err}");
        assert!(s.contains("window_size"));
        assert!(s.contains("must be positive"));
    }

    #[test]
    fn test_error_from_core() {
        let core_err = OxiGdalError::OutOfBounds {
            message: "test".to_string(),
        };
        let _alg_err: AlgorithmError = core_err.into();
    }

    #[test]
    fn test_invalid_input() {
        let err = AlgorithmError::InvalidInput("test input".to_string());
        let s = format!("{err}");
        assert!(s.contains("Invalid input"));
        assert!(s.contains("test input"));
    }

    #[test]
    fn test_invalid_geometry() {
        let err = AlgorithmError::InvalidGeometry("test geometry".to_string());
        let s = format!("{err}");
        assert!(s.contains("Invalid geometry"));
        assert!(s.contains("test geometry"));
    }

    #[test]
    fn test_computation_error() {
        let err = AlgorithmError::ComputationError("test error".to_string());
        let s = format!("{err}");
        assert!(s.contains("Computation error"));
        assert!(s.contains("test error"));
    }

    #[test]
    fn test_error_codes() {
        let err = AlgorithmError::InvalidParameter {
            parameter: "window_size",
            message: "must be odd".to_string(),
        };
        assert_eq!(err.code(), "A005");

        let err = AlgorithmError::InvalidDimensions {
            message: "mismatched",
            actual: 4,
            expected: 5,
        };
        assert_eq!(err.code(), "A002");

        let err = AlgorithmError::SimdNotAvailable;
        assert_eq!(err.code(), "A014");
    }

    #[test]
    fn test_error_suggestions() {
        let err = AlgorithmError::InvalidParameter {
            parameter: "window_size",
            message: "must be odd".to_string(),
        };
        assert!(err.suggestion().is_some());
        assert!(err.suggestion().is_some_and(|s| s.contains("odd")));

        let err = AlgorithmError::InvalidParameter {
            parameter: "kernel_size",
            message: "invalid".to_string(),
        };
        assert!(err.suggestion().is_some());
        assert!(err.suggestion().is_some_and(|s| s.contains("kernel")));

        let err = AlgorithmError::InvalidDimensions {
            message: "window size",
            actual: 4,
            expected: 3,
        };
        assert!(err.suggestion().is_some());
        assert!(
            err.suggestion()
                .is_some_and(|s| s.contains("Window size must be odd"))
        );
    }

    #[test]
    fn test_error_context() {
        let err = AlgorithmError::InvalidParameter {
            parameter: "window_size",
            message: "must be odd, got 4. Try 3 or 5".to_string(),
        };
        let ctx = err.context();
        assert_eq!(ctx.category, "invalid_parameter");
        assert!(
            ctx.details
                .iter()
                .any(|(k, v)| k == "parameter" && v == "window_size")
        );
        assert!(ctx.details.iter().any(|(k, _)| k == "message"));

        let err = AlgorithmError::InvalidDimensions {
            message: "array size mismatch",
            actual: 100,
            expected: 200,
        };
        let ctx = err.context();
        assert_eq!(ctx.category, "invalid_dimensions");
        assert!(ctx.details.iter().any(|(k, v)| k == "actual" && v == "100"));
        assert!(
            ctx.details
                .iter()
                .any(|(k, v)| k == "expected" && v == "200")
        );
    }

    #[test]
    fn test_parameter_suggestion_specificity() {
        // Test window parameter
        let err = AlgorithmError::InvalidParameter {
            parameter: "window_size",
            message: "test".to_string(),
        };
        let suggestion = err.suggestion();
        assert!(suggestion.is_some_and(|s| s.contains("Window") || s.contains("kernel")));

        // Test threshold parameter
        let err = AlgorithmError::InvalidParameter {
            parameter: "threshold",
            message: "test".to_string(),
        };
        let suggestion = err.suggestion();
        assert!(suggestion.is_some_and(|s| s.contains("0.0") && s.contains("1.0")));
    }
}
