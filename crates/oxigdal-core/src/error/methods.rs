//! Method implementations for error types

use super::builder::ErrorContext;
use super::types::*;

#[cfg(feature = "std")]
use std::path::Path;

impl IoError {
    /// Get the error code for this I/O error
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound { .. } => "E100",
            Self::PermissionDenied { .. } => "E101",
            Self::Network { .. } => "E102",
            Self::UnexpectedEof { .. } => "E103",
            Self::Read { .. } => "E104",
            Self::Write { .. } => "E105",
            Self::Seek { .. } => "E106",
            Self::Http { .. } => "E107",
        }
    }

    /// Get a helpful suggestion for fixing this I/O error
    pub fn suggestion(&self) -> Option<&'static str> {
        match self {
            Self::NotFound { .. } => Some("Verify the file path is correct and the file exists"),
            Self::PermissionDenied { .. } => {
                Some("Check file permissions or run with appropriate privileges")
            }
            Self::Network { .. } => Some("Check network connectivity and firewall settings"),
            Self::UnexpectedEof { .. } => Some("The file may be truncated or corrupted"),
            Self::Read { .. } => {
                Some("Ensure the file is accessible and not locked by another process")
            }
            Self::Write { .. } => Some("Check available disk space and write permissions"),
            Self::Seek { .. } => Some("The file position may be invalid for this file type"),
            Self::Http { status, .. } if *status == 404 => {
                Some("The requested resource was not found on the server")
            }
            Self::Http { status, .. } if *status == 403 => {
                Some("Access to this resource is forbidden. Check authentication credentials")
            }
            Self::Http { status, .. } if *status >= 500 => {
                Some("The server is experiencing issues. Try again later")
            }
            Self::Http { .. } => Some("Check the HTTP request parameters and server status"),
        }
    }

    /// Get additional context about this I/O error
    pub fn context(&self) -> ErrorContext {
        match self {
            Self::NotFound { path } => {
                ErrorContext::new("file_not_found").with_detail("path", path.clone())
            }
            Self::PermissionDenied { path } => {
                ErrorContext::new("permission_denied").with_detail("path", path.clone())
            }
            Self::Network { message } => {
                ErrorContext::new("network_error").with_detail("message", message.clone())
            }
            Self::UnexpectedEof { offset } => {
                ErrorContext::new("unexpected_eof").with_detail("offset", offset.to_string())
            }
            Self::Read { message } => {
                ErrorContext::new("read_error").with_detail("message", message.clone())
            }
            Self::Write { message } => {
                ErrorContext::new("write_error").with_detail("message", message.clone())
            }
            Self::Seek { position } => {
                ErrorContext::new("seek_error").with_detail("position", position.to_string())
            }
            Self::Http { status, message } => ErrorContext::new("http_error")
                .with_detail("status", status.to_string())
                .with_detail("message", message.clone()),
        }
    }
}

impl FormatError {
    /// Get the error code for this format error
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidMagic { .. } => "E200",
            Self::InvalidHeader { .. } => "E201",
            Self::UnsupportedVersion { .. } => "E202",
            Self::InvalidTag { .. } => "E203",
            Self::MissingTag { .. } => "E204",
            Self::InvalidDataType { .. } => "E205",
            Self::CorruptData { .. } => "E206",
            Self::InvalidGeoKey { .. } => "E207",
        }
    }

    /// Get a helpful suggestion for fixing this format error
    pub fn suggestion(&self) -> Option<&'static str> {
        match self {
            Self::InvalidMagic { .. } => {
                Some("The file may not be in the expected format. Verify the file type")
            }
            Self::InvalidHeader { .. } => {
                Some("The file header is corrupted or invalid. Try opening a different file")
            }
            Self::UnsupportedVersion { .. } => Some(
                "This file version is not supported. Consider converting to a newer or older version",
            ),
            Self::InvalidTag { .. } => {
                Some("The file contains invalid metadata tags. The file may be corrupted")
            }
            Self::MissingTag { .. } => {
                Some("Required metadata is missing. The file may be incomplete or corrupted")
            }
            Self::InvalidDataType { .. } => {
                Some("The data type is not recognized. The file may be from a newer version")
            }
            Self::CorruptData { .. } => {
                Some("Data corruption detected. Try recovering from a backup")
            }
            Self::InvalidGeoKey { .. } => {
                Some("Geographic metadata is invalid. Check the coordinate reference system")
            }
        }
    }

    /// Get additional context about this format error
    pub fn context(&self) -> ErrorContext {
        match self {
            Self::InvalidMagic { expected, actual } => ErrorContext::new("invalid_magic")
                .with_detail("expected", format!("{:?}", expected))
                .with_detail("actual", format!("{:?}", actual)),
            Self::InvalidHeader { message } => {
                ErrorContext::new("invalid_header").with_detail("message", message.clone())
            }
            Self::UnsupportedVersion { version } => {
                ErrorContext::new("unsupported_version").with_detail("version", version.to_string())
            }
            Self::InvalidTag { tag, message } => ErrorContext::new("invalid_tag")
                .with_detail("tag", tag.to_string())
                .with_detail("message", message.clone()),
            Self::MissingTag { tag } => {
                ErrorContext::new("missing_tag").with_detail("tag", tag.to_string())
            }
            Self::InvalidDataType { type_id } => {
                ErrorContext::new("invalid_data_type").with_detail("type_id", type_id.to_string())
            }
            Self::CorruptData { offset, message } => ErrorContext::new("corrupt_data")
                .with_detail("offset", offset.to_string())
                .with_detail("message", message.clone()),
            Self::InvalidGeoKey { key_id, message } => ErrorContext::new("invalid_geokey")
                .with_detail("key_id", key_id.to_string())
                .with_detail("message", message.clone()),
        }
    }
}

impl CrsError {
    /// Get the error code for this CRS error
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnknownCrs { .. } => "E300",
            Self::InvalidWkt { .. } => "E301",
            Self::InvalidEpsg { .. } => "E302",
            Self::TransformationError { .. } => "E303",
            Self::DatumNotFound { .. } => "E304",
        }
    }

    /// Get a helpful suggestion for fixing this CRS error
    pub fn suggestion(&self) -> Option<&'static str> {
        match self {
            Self::UnknownCrs { .. } => {
                Some("Verify the CRS identifier or use a standard EPSG code")
            }
            Self::InvalidWkt { .. } => {
                Some("Check the WKT string syntax. Ensure proper bracketing and spacing")
            }
            Self::InvalidEpsg { .. } => Some("Use a valid EPSG code from https://epsg.io"),
            Self::TransformationError { .. } => {
                Some("Ensure both CRS are compatible and transformation parameters are available")
            }
            Self::DatumNotFound { .. } => {
                Some("The datum definition may be missing. Check CRS database installation")
            }
        }
    }

    /// Get additional context about this CRS error
    pub fn context(&self) -> ErrorContext {
        match self {
            Self::UnknownCrs { identifier } => {
                ErrorContext::new("unknown_crs").with_detail("identifier", identifier.clone())
            }
            Self::InvalidWkt { message } => {
                ErrorContext::new("invalid_wkt").with_detail("message", message.clone())
            }
            Self::InvalidEpsg { code } => {
                ErrorContext::new("invalid_epsg").with_detail("code", code.to_string())
            }
            Self::TransformationError {
                source_crs,
                target_crs,
                message,
            } => ErrorContext::new("transformation_error")
                .with_detail("source_crs", source_crs.clone())
                .with_detail("target_crs", target_crs.clone())
                .with_detail("message", message.clone()),
            Self::DatumNotFound { datum } => {
                ErrorContext::new("datum_not_found").with_detail("datum", datum.clone())
            }
        }
    }
}

impl CompressionError {
    /// Get the error code for this compression error
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnknownMethod { .. } => "E400",
            Self::DecompressionFailed { .. } => "E401",
            Self::CompressionFailed { .. } => "E402",
            Self::InvalidData { .. } => "E403",
        }
    }

    /// Get a helpful suggestion for fixing this compression error
    pub fn suggestion(&self) -> Option<&'static str> {
        match self {
            Self::UnknownMethod { .. } => Some(
                "The compression method is not supported. Check available compression features",
            ),
            Self::DecompressionFailed { .. } => {
                Some("The compressed data may be corrupted. Try a backup or re-download the file")
            }
            Self::CompressionFailed { .. } => Some(
                "Compression failed. Try a different compression method or lower compression level",
            ),
            Self::InvalidData { .. } => Some("The compressed data is invalid or corrupted"),
        }
    }

    /// Get additional context about this compression error
    pub fn context(&self) -> ErrorContext {
        match self {
            Self::UnknownMethod { method } => {
                ErrorContext::new("unknown_compression").with_detail("method", method.to_string())
            }
            Self::DecompressionFailed { message } => {
                ErrorContext::new("decompression_failed").with_detail("message", message.clone())
            }
            Self::CompressionFailed { message } => {
                ErrorContext::new("compression_failed").with_detail("message", message.clone())
            }
            Self::InvalidData { message } => {
                ErrorContext::new("invalid_compressed_data").with_detail("message", message.clone())
            }
        }
    }
}

impl OxiGdalError {
    /// Create an allocation error
    pub fn allocation_error(message: impl Into<String>) -> Self {
        Self::Internal {
            message: format!("Allocation error: {}", message.into()),
        }
    }

    /// Create an allocation error builder with rich context
    pub fn allocation_error_builder(message: impl Into<String>) -> crate::error::ErrorBuilder {
        crate::error::ErrorBuilder::new(Self::allocation_error(message))
    }

    /// Create an invalid state error
    pub fn invalid_state(message: impl Into<String>) -> Self {
        Self::Internal {
            message: format!("Invalid state: {}", message.into()),
        }
    }

    /// Create an invalid state error builder with rich context
    pub fn invalid_state_builder(message: impl Into<String>) -> crate::error::ErrorBuilder {
        crate::error::ErrorBuilder::new(Self::invalid_state(message))
    }

    /// Create an invalid operation error
    pub fn invalid_operation(message: impl Into<String>) -> Self {
        Self::NotSupported {
            operation: message.into(),
        }
    }

    /// Create an invalid operation error builder with rich context
    pub fn invalid_operation_builder(message: impl Into<String>) -> crate::error::ErrorBuilder {
        crate::error::ErrorBuilder::new(Self::invalid_operation(message))
    }

    /// Create an I/O error from a message
    pub fn io_error(message: impl Into<String>) -> Self {
        Self::Io(IoError::Read {
            message: message.into(),
        })
    }

    /// Create an I/O error builder with rich context
    pub fn io_error_builder(message: impl Into<String>) -> crate::error::ErrorBuilder {
        crate::error::ErrorBuilder::new(Self::io_error(message))
    }

    /// Create an invalid parameter error with parameter name
    pub fn invalid_parameter(parameter: &'static str, message: impl Into<String>) -> Self {
        Self::InvalidParameter {
            parameter,
            message: message.into(),
        }
    }

    /// Create an invalid parameter error builder with rich context
    pub fn invalid_parameter_builder(
        parameter: &'static str,
        message: impl Into<String>,
    ) -> crate::error::ErrorBuilder {
        crate::error::ErrorBuilder::new(Self::invalid_parameter(parameter, message))
    }

    /// Create a not supported error
    pub fn not_supported(operation: impl Into<String>) -> Self {
        Self::NotSupported {
            operation: operation.into(),
        }
    }

    /// Create a not supported error builder with rich context
    pub fn not_supported_builder(operation: impl Into<String>) -> crate::error::ErrorBuilder {
        crate::error::ErrorBuilder::new(Self::not_supported(operation))
    }

    /// Create an I/O error from a file path
    #[cfg(feature = "std")]
    pub fn from_path(path: &Path, kind: std::io::ErrorKind) -> Self {
        let path_str = path.display().to_string();
        match kind {
            std::io::ErrorKind::NotFound => Self::Io(IoError::NotFound { path: path_str }),
            std::io::ErrorKind::PermissionDenied => {
                Self::Io(IoError::PermissionDenied { path: path_str })
            }
            _ => Self::Io(IoError::Read {
                message: format!("Error accessing {}", path_str),
            }),
        }
    }

    /// Create an I/O error builder from a file path with rich context
    #[cfg(feature = "std")]
    pub fn from_path_builder(path: &Path, kind: std::io::ErrorKind) -> crate::error::ErrorBuilder {
        crate::error::ErrorBuilder::new(Self::from_path(path, kind)).with_path(path)
    }

    /// Get the error code for this error
    ///
    /// Error codes are stable across versions and can be used for documentation
    /// and error handling.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Io(e) => e.code(),
            Self::Format(e) => e.code(),
            Self::Crs(e) => e.code(),
            Self::Compression(e) => e.code(),
            Self::InvalidParameter { .. } => "E001",
            Self::NotSupported { .. } => "E002",
            Self::OutOfBounds { .. } => "E003",
            Self::Internal { .. } => "E004",
        }
    }

    /// Get a helpful suggestion for fixing this error
    ///
    /// Returns a human-readable suggestion that can help users resolve the error.
    pub fn suggestion(&self) -> Option<&'static str> {
        match self {
            Self::Io(e) => e.suggestion(),
            Self::Format(e) => e.suggestion(),
            Self::Crs(e) => e.suggestion(),
            Self::Compression(e) => e.suggestion(),
            Self::InvalidParameter { .. } => {
                Some("Check the parameter documentation for valid values")
            }
            Self::NotSupported { .. } => {
                Some("Check if the feature is enabled or use an alternative approach")
            }
            Self::OutOfBounds { .. } => Some("Verify the indices are within valid range"),
            Self::Internal { .. } => {
                Some("This is likely a bug. Please report it with steps to reproduce")
            }
        }
    }

    /// Get additional context about this error
    ///
    /// Returns structured context information that can help with debugging.
    pub fn context(&self) -> ErrorContext {
        match self {
            Self::Io(e) => e.context(),
            Self::Format(e) => e.context(),
            Self::Crs(e) => e.context(),
            Self::Compression(e) => e.context(),
            Self::InvalidParameter { parameter, message } => {
                ErrorContext::new("parameter_validation")
                    .with_detail("parameter", *parameter)
                    .with_detail("reason", message.clone())
            }
            Self::NotSupported { operation } => ErrorContext::new("unsupported_operation")
                .with_detail("operation", operation.clone()),
            Self::OutOfBounds { message } => {
                ErrorContext::new("bounds_check").with_detail("reason", message.clone())
            }
            Self::Internal { message } => {
                ErrorContext::new("internal_error").with_detail("details", message.clone())
            }
        }
    }
}
