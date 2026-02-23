//! Error types for NetCDF operations.
//!
//! This module provides comprehensive error handling for NetCDF file operations,
//! including I/O errors, format errors, dimension errors, variable errors, and
//! attribute errors.
//!
//! # Error Codes
//!
//! Each error variant has an associated error code (e.g., N001, N002) for easier
//! debugging and documentation. Error codes are stable across versions.
//!
//! # Helper Methods
//!
//! All error types provide:
//! - `code()` - Returns the error code
//! - `suggestion()` - Returns helpful hints for fixing the error
//! - `context()` - Returns additional context about the error (variable/dimension names, etc.)

use core::fmt;

#[cfg(feature = "std")]
use std::error::Error as StdError;

use oxigdal_core::error::OxiGdalError;

/// Result type for NetCDF operations.
pub type Result<T> = core::result::Result<T, NetCdfError>;

/// NetCDF-specific error types.
#[derive(Debug)]
pub enum NetCdfError {
    /// I/O error occurred
    Io(String),

    /// Invalid NetCDF format
    InvalidFormat(String),

    /// Version not supported
    UnsupportedVersion { version: u8, message: String },

    /// Dimension error
    DimensionError(String),

    /// Dimension not found
    DimensionNotFound { name: String },

    /// Variable error
    VariableError(String),

    /// Variable not found
    VariableNotFound { name: String },

    /// Attribute error
    AttributeError(String),

    /// Attribute not found
    AttributeNotFound { name: String },

    /// Data type mismatch
    DataTypeMismatch { expected: String, found: String },

    /// Invalid shape or dimensions
    InvalidShape { message: String },

    /// Unlimited dimension error
    UnlimitedDimensionError(String),

    /// Index out of bounds
    IndexOutOfBounds {
        index: usize,
        length: usize,
        dimension: String,
    },

    /// String encoding error
    StringEncodingError(String),

    /// Feature not enabled
    FeatureNotEnabled { feature: String, message: String },

    /// NetCDF-4 not available (requires feature flag)
    NetCdf4NotAvailable,

    /// Compression not supported
    CompressionNotSupported { compression: String },

    /// Invalid compression parameters
    InvalidCompressionParams(String),

    /// CF conventions error
    CfConventionsError(String),

    /// Coordinate variable error
    CoordinateError(String),

    /// File already exists
    FileAlreadyExists { path: String },

    /// File not found
    FileNotFound { path: String },

    /// Permission denied
    PermissionDenied { path: String },

    /// Invalid file mode
    InvalidFileMode { mode: String },

    /// NetCDF library error (for netcdf4 feature)
    #[cfg(feature = "netcdf4")]
    NetCdfLibError(String),

    /// Generic error
    Other(String),

    /// Error from oxigdal-core
    Core(OxiGdalError),
}

impl fmt::Display for NetCdfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(msg) => write!(f, "I/O error: {msg}"),
            Self::InvalidFormat(msg) => write!(f, "Invalid NetCDF format: {msg}"),
            Self::UnsupportedVersion { version, message } => {
                write!(f, "Unsupported NetCDF version {version}: {message}")
            }
            Self::DimensionError(msg) => write!(f, "Dimension error: {msg}"),
            Self::DimensionNotFound { name } => write!(f, "Dimension not found: {name}"),
            Self::VariableError(msg) => write!(f, "Variable error: {msg}"),
            Self::VariableNotFound { name } => write!(f, "Variable not found: {name}"),
            Self::AttributeError(msg) => write!(f, "Attribute error: {msg}"),
            Self::AttributeNotFound { name } => write!(f, "Attribute not found: {name}"),
            Self::DataTypeMismatch { expected, found } => {
                write!(f, "Data type mismatch: expected {expected}, found {found}")
            }
            Self::InvalidShape { message } => write!(f, "Invalid shape: {message}"),
            Self::UnlimitedDimensionError(msg) => {
                write!(f, "Unlimited dimension error: {msg}")
            }
            Self::IndexOutOfBounds {
                index,
                length,
                dimension,
            } => {
                write!(
                    f,
                    "Index {index} out of bounds for dimension '{dimension}' with length {length}"
                )
            }
            Self::StringEncodingError(msg) => write!(f, "String encoding error: {msg}"),
            Self::FeatureNotEnabled { feature, message } => {
                write!(f, "Feature '{feature}' not enabled: {message}")
            }
            Self::NetCdf4NotAvailable => {
                write!(
                    f,
                    "NetCDF-4 support not available. Enable 'netcdf4' feature to use HDF5-based NetCDF-4 files. \
                     Note: This requires C dependencies (libnetcdf, libhdf5) and is not Pure Rust."
                )
            }
            Self::CompressionNotSupported { compression } => {
                write!(f, "Compression not supported: {compression}")
            }
            Self::InvalidCompressionParams(msg) => {
                write!(f, "Invalid compression parameters: {msg}")
            }
            Self::CfConventionsError(msg) => write!(f, "CF conventions error: {msg}"),
            Self::CoordinateError(msg) => write!(f, "Coordinate error: {msg}"),
            Self::FileAlreadyExists { path } => write!(f, "File already exists: {path}"),
            Self::FileNotFound { path } => write!(f, "File not found: {path}"),
            Self::PermissionDenied { path } => write!(f, "Permission denied: {path}"),
            Self::InvalidFileMode { mode } => write!(f, "Invalid file mode: {mode}"),
            #[cfg(feature = "netcdf4")]
            Self::NetCdfLibError(msg) => write!(f, "NetCDF library error: {msg}"),
            Self::Other(msg) => write!(f, "{msg}"),
            Self::Core(err) => write!(f, "Core error: {err}"),
        }
    }
}

#[cfg(feature = "std")]
impl StdError for NetCdfError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Core(err) => Some(err),
            _ => None,
        }
    }
}

impl From<OxiGdalError> for NetCdfError {
    fn from(err: OxiGdalError) -> Self {
        Self::Core(err)
    }
}

#[cfg(feature = "std")]
impl From<std::io::Error> for NetCdfError {
    fn from(err: std::io::Error) -> Self {
        use std::io::ErrorKind;
        match err.kind() {
            ErrorKind::NotFound => Self::Io(format!("File not found: {err}")),
            ErrorKind::PermissionDenied => Self::Io(format!("Permission denied: {err}")),
            ErrorKind::AlreadyExists => Self::Io(format!("File already exists: {err}")),
            _ => Self::Io(err.to_string()),
        }
    }
}

#[cfg(feature = "std")]
impl From<std::string::FromUtf8Error> for NetCdfError {
    fn from(err: std::string::FromUtf8Error) -> Self {
        Self::StringEncodingError(err.to_string())
    }
}

impl From<core::str::Utf8Error> for NetCdfError {
    fn from(err: core::str::Utf8Error) -> Self {
        Self::StringEncodingError(format!("UTF-8 error: {err}"))
    }
}

#[cfg(feature = "netcdf4")]
impl From<netcdf::error::Error> for NetCdfError {
    fn from(err: netcdf::error::Error) -> Self {
        Self::NetCdfLibError(err.to_string())
    }
}

impl From<serde_json::Error> for NetCdfError {
    fn from(err: serde_json::Error) -> Self {
        Self::Other(format!("JSON error: {err}"))
    }
}

#[cfg(feature = "netcdf3")]
impl From<netcdf3::InvalidDataSet> for NetCdfError {
    fn from(err: netcdf3::InvalidDataSet) -> Self {
        Self::Other(format!("Invalid DataSet: {err}"))
    }
}

#[cfg(feature = "netcdf3")]
impl From<netcdf3::WriteError> for NetCdfError {
    fn from(err: netcdf3::WriteError) -> Self {
        Self::Io(format!("Write error: {err:?}"))
    }
}

#[cfg(feature = "netcdf3")]
impl From<netcdf3::ReadError> for NetCdfError {
    fn from(err: netcdf3::ReadError) -> Self {
        Self::Io(format!("Read error: {err:?}"))
    }
}

impl NetCdfError {
    /// Get the error code for this NetCDF error
    ///
    /// Error codes are stable across versions and can be used for documentation
    /// and error handling.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Io(_) => "N001",
            Self::InvalidFormat(_) => "N002",
            Self::UnsupportedVersion { .. } => "N003",
            Self::DimensionError(_) => "N004",
            Self::DimensionNotFound { .. } => "N005",
            Self::VariableError(_) => "N006",
            Self::VariableNotFound { .. } => "N007",
            Self::AttributeError(_) => "N008",
            Self::AttributeNotFound { .. } => "N009",
            Self::DataTypeMismatch { .. } => "N010",
            Self::InvalidShape { .. } => "N011",
            Self::UnlimitedDimensionError(_) => "N012",
            Self::IndexOutOfBounds { .. } => "N013",
            Self::StringEncodingError(_) => "N014",
            Self::FeatureNotEnabled { .. } => "N015",
            Self::NetCdf4NotAvailable => "N016",
            Self::CompressionNotSupported { .. } => "N017",
            Self::InvalidCompressionParams(_) => "N018",
            Self::CfConventionsError(_) => "N019",
            Self::CoordinateError(_) => "N020",
            Self::FileAlreadyExists { .. } => "N021",
            Self::FileNotFound { .. } => "N022",
            Self::PermissionDenied { .. } => "N023",
            Self::InvalidFileMode { .. } => "N024",
            #[cfg(feature = "netcdf4")]
            Self::NetCdfLibError(_) => "N025",
            Self::Other(_) => "N099",
            Self::Core(_) => "N100",
        }
    }

    /// Get a helpful suggestion for fixing this NetCDF error
    ///
    /// Returns a human-readable suggestion that can help users resolve the error.
    pub fn suggestion(&self) -> Option<&'static str> {
        match self {
            Self::Io(_) => Some("Check file permissions and network connectivity"),
            Self::InvalidFormat(_) => {
                Some("Verify the file is a valid NetCDF file. Try using ncdump")
            }
            Self::UnsupportedVersion { .. } => {
                Some("This NetCDF version is not supported. Try NetCDF-3 Classic format")
            }
            Self::DimensionError(_) => Some("Check dimension definitions and sizes"),
            Self::DimensionNotFound { .. } => Some("Use ncdump -h to list available dimensions"),
            Self::VariableError(_) => Some("Check variable definitions and data types"),
            Self::VariableNotFound { .. } => Some("Use ncdump -h to list available variables"),
            Self::AttributeError(_) => Some("Check attribute name and type"),
            Self::AttributeNotFound { .. } => Some("Use ncdump -h to list available attributes"),
            Self::DataTypeMismatch { .. } => {
                Some("Ensure the data type matches the variable definition")
            }
            Self::InvalidShape { .. } => {
                Some("Verify the data dimensions match the variable shape")
            }
            Self::UnlimitedDimensionError(_) => Some("Check unlimited dimension is defined first"),
            Self::IndexOutOfBounds { .. } => Some("Verify indices are within dimension bounds"),
            Self::StringEncodingError(_) => Some("Ensure string data is valid UTF-8"),
            Self::FeatureNotEnabled { .. } => {
                Some("Enable the required feature flag in Cargo.toml")
            }
            Self::NetCdf4NotAvailable => {
                Some("Enable the 'netcdf4' feature for HDF5-based NetCDF-4 support")
            }
            Self::CompressionNotSupported { .. } => {
                Some("Use a supported compression algorithm or disable compression")
            }
            Self::InvalidCompressionParams(_) => Some("Check compression level is between 0-9"),
            Self::CfConventionsError(_) => {
                Some("Ensure the file follows CF conventions. See https://cfconventions.org")
            }
            Self::CoordinateError(_) => Some("Check coordinate variable definitions and values"),
            Self::FileAlreadyExists { .. } => {
                Some("Choose a different filename or delete the existing file")
            }
            Self::FileNotFound { .. } => {
                Some("Verify the file path is correct and the file exists")
            }
            Self::PermissionDenied { .. } => {
                Some("Check file permissions or run with appropriate privileges")
            }
            Self::InvalidFileMode { .. } => {
                Some("Use a valid file mode: 'r' (read), 'w' (write), or 'a' (append)")
            }
            #[cfg(feature = "netcdf4")]
            Self::NetCdfLibError(_) => Some("Check the NetCDF-C library is properly installed"),
            Self::Other(_) => Some("Check the error message for details"),
            Self::Core(_) => Some("Check the underlying error message for details"),
        }
    }

    /// Get additional context about this NetCDF error
    ///
    /// Returns structured context information including variable/dimension names.
    pub fn context(&self) -> ErrorContext {
        match self {
            Self::Io(msg) => ErrorContext::new("io_error").with_detail("message", msg.clone()),
            Self::InvalidFormat(msg) => {
                ErrorContext::new("invalid_format").with_detail("message", msg.clone())
            }
            Self::UnsupportedVersion { version, message } => {
                ErrorContext::new("unsupported_version")
                    .with_detail("version", version.to_string())
                    .with_detail("message", message.clone())
            }
            Self::DimensionError(msg) => {
                ErrorContext::new("dimension_error").with_detail("message", msg.clone())
            }
            Self::DimensionNotFound { name } => {
                ErrorContext::new("dimension_not_found").with_detail("dimension", name.clone())
            }
            Self::VariableError(msg) => {
                ErrorContext::new("variable_error").with_detail("message", msg.clone())
            }
            Self::VariableNotFound { name } => {
                ErrorContext::new("variable_not_found").with_detail("variable", name.clone())
            }
            Self::AttributeError(msg) => {
                ErrorContext::new("attribute_error").with_detail("message", msg.clone())
            }
            Self::AttributeNotFound { name } => {
                ErrorContext::new("attribute_not_found").with_detail("attribute", name.clone())
            }
            Self::DataTypeMismatch { expected, found } => ErrorContext::new("data_type_mismatch")
                .with_detail("expected", expected.clone())
                .with_detail("found", found.clone()),
            Self::InvalidShape { message } => {
                ErrorContext::new("invalid_shape").with_detail("message", message.clone())
            }
            Self::UnlimitedDimensionError(msg) => {
                ErrorContext::new("unlimited_dimension_error").with_detail("message", msg.clone())
            }
            Self::IndexOutOfBounds {
                index,
                length,
                dimension,
            } => ErrorContext::new("index_out_of_bounds")
                .with_detail("index", index.to_string())
                .with_detail("length", length.to_string())
                .with_detail("dimension", dimension.clone()),
            Self::StringEncodingError(msg) => {
                ErrorContext::new("string_encoding_error").with_detail("message", msg.clone())
            }
            Self::FeatureNotEnabled { feature, message } => {
                ErrorContext::new("feature_not_enabled")
                    .with_detail("feature", feature.clone())
                    .with_detail("message", message.clone())
            }
            Self::NetCdf4NotAvailable => ErrorContext::new("netcdf4_not_available"),
            Self::CompressionNotSupported { compression } => {
                ErrorContext::new("compression_not_supported")
                    .with_detail("compression", compression.clone())
            }
            Self::InvalidCompressionParams(msg) => {
                ErrorContext::new("invalid_compression_params").with_detail("message", msg.clone())
            }
            Self::CfConventionsError(msg) => {
                ErrorContext::new("cf_conventions_error").with_detail("message", msg.clone())
            }
            Self::CoordinateError(msg) => {
                ErrorContext::new("coordinate_error").with_detail("message", msg.clone())
            }
            Self::FileAlreadyExists { path } => {
                ErrorContext::new("file_already_exists").with_detail("path", path.clone())
            }
            Self::FileNotFound { path } => {
                ErrorContext::new("file_not_found").with_detail("path", path.clone())
            }
            Self::PermissionDenied { path } => {
                ErrorContext::new("permission_denied").with_detail("path", path.clone())
            }
            Self::InvalidFileMode { mode } => {
                ErrorContext::new("invalid_file_mode").with_detail("mode", mode.clone())
            }
            #[cfg(feature = "netcdf4")]
            Self::NetCdfLibError(msg) => {
                ErrorContext::new("netcdf_lib_error").with_detail("message", msg.clone())
            }
            Self::Other(msg) => ErrorContext::new("other").with_detail("message", msg.clone()),
            Self::Core(e) => ErrorContext::new("core_error").with_detail("error", e.to_string()),
        }
    }
}

/// Additional context information for NetCDF errors
#[derive(Debug, Clone)]
pub struct ErrorContext {
    /// Error category for grouping similar errors
    pub category: &'static str,
    /// Additional details about the error (variable/dimension names, etc.)
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
        let err = NetCdfError::DimensionNotFound {
            name: "time".to_string(),
        };
        assert_eq!(err.to_string(), "Dimension not found: time");

        let err = NetCdfError::DataTypeMismatch {
            expected: "f32".to_string(),
            found: "f64".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Data type mismatch: expected f32, found f64"
        );

        let err = NetCdfError::IndexOutOfBounds {
            index: 10,
            length: 5,
            dimension: "time".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Index 10 out of bounds for dimension 'time' with length 5"
        );
    }

    #[test]
    fn test_netcdf4_not_available() {
        let err = NetCdfError::NetCdf4NotAvailable;
        let msg = err.to_string();
        assert!(msg.contains("NetCDF-4"));
        assert!(msg.contains("netcdf4"));
        assert!(msg.contains("Pure Rust"));
    }

    #[test]
    fn test_error_codes() {
        let err = NetCdfError::VariableNotFound {
            name: "temperature".to_string(),
        };
        assert_eq!(err.code(), "N007");

        let err = NetCdfError::DimensionNotFound {
            name: "time".to_string(),
        };
        assert_eq!(err.code(), "N005");

        let err = NetCdfError::AttributeNotFound {
            name: "units".to_string(),
        };
        assert_eq!(err.code(), "N009");
    }

    #[test]
    fn test_error_suggestions() {
        let err = NetCdfError::VariableNotFound {
            name: "temperature".to_string(),
        };
        assert!(err.suggestion().is_some());
        assert!(err.suggestion().is_some_and(|s| s.contains("ncdump")));

        let err = NetCdfError::DimensionNotFound {
            name: "time".to_string(),
        };
        assert!(err.suggestion().is_some());
        assert!(err.suggestion().is_some_and(|s| s.contains("ncdump")));
    }

    #[test]
    fn test_error_context() {
        let err = NetCdfError::VariableNotFound {
            name: "temperature".to_string(),
        };
        let ctx = err.context();
        assert_eq!(ctx.category, "variable_not_found");
        assert!(
            ctx.details
                .iter()
                .any(|(k, v)| k == "variable" && v == "temperature")
        );

        let err = NetCdfError::DimensionNotFound {
            name: "time".to_string(),
        };
        let ctx = err.context();
        assert_eq!(ctx.category, "dimension_not_found");
        assert!(
            ctx.details
                .iter()
                .any(|(k, v)| k == "dimension" && v == "time")
        );

        let err = NetCdfError::IndexOutOfBounds {
            index: 10,
            length: 5,
            dimension: "time".to_string(),
        };
        let ctx = err.context();
        assert_eq!(ctx.category, "index_out_of_bounds");
        assert!(ctx.details.iter().any(|(k, v)| k == "index" && v == "10"));
        assert!(
            ctx.details
                .iter()
                .any(|(k, v)| k == "dimension" && v == "time")
        );
    }
}
