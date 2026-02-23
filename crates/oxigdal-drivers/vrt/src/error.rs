//! Error types for VRT operations

use oxigdal_core::error::OxiGdalError;
use thiserror::Error;

/// Result type for VRT operations
pub type Result<T> = core::result::Result<T, VrtError>;

/// VRT-specific error types
#[derive(Debug, Error)]
pub enum VrtError {
    /// XML parsing error
    #[error("XML parsing error: {message}")]
    XmlParse {
        /// Error message
        message: String,
    },

    /// Invalid VRT structure
    #[error("Invalid VRT structure: {message}")]
    InvalidStructure {
        /// Error message
        message: String,
    },

    /// Source file not found
    #[error("Source file not found: {path}")]
    SourceNotFound {
        /// Source file path
        path: String,
    },

    /// Source file error
    #[error("Source file error '{path}': {message}")]
    SourceError {
        /// Source file path
        path: String,
        /// Error message
        message: String,
    },

    /// Invalid source configuration
    #[error("Invalid source configuration: {message}")]
    InvalidSource {
        /// Error message
        message: String,
    },

    /// Invalid band configuration
    #[error("Invalid band configuration: {message}")]
    InvalidBand {
        /// Error message
        message: String,
    },

    /// Band out of range
    #[error("Band {band} out of range (0-{max})")]
    BandOutOfRange {
        /// Band index
        band: usize,
        /// Maximum band index
        max: usize,
    },

    /// Invalid extent
    #[error("Invalid extent: {message}")]
    InvalidExtent {
        /// Error message
        message: String,
    },

    /// Invalid window
    #[error("Invalid window: {message}")]
    InvalidWindow {
        /// Error message
        message: String,
    },

    /// Invalid pixel function
    #[error("Invalid pixel function: {function}")]
    InvalidPixelFunction {
        /// Function name
        function: String,
    },

    /// Missing required attribute
    #[error("Missing required attribute: {attribute}")]
    MissingAttribute {
        /// Attribute name
        attribute: String,
    },

    /// Path resolution error
    #[error("Path resolution error for '{path}': {message}")]
    PathResolution {
        /// Path that failed to resolve
        path: String,
        /// Error message
        message: String,
    },

    /// Cache error
    #[error("Cache error: {message}")]
    CacheError {
        /// Error message
        message: String,
    },

    /// Incompatible sources
    #[error("Incompatible sources: {message}")]
    IncompatibleSources {
        /// Error message
        message: String,
    },

    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// OxiGDAL core error
    #[error("OxiGDAL error: {0}")]
    Core(#[from] OxiGdalError),
}

impl VrtError {
    /// Creates an XML parsing error
    pub fn xml_parse<S: Into<String>>(message: S) -> Self {
        Self::XmlParse {
            message: message.into(),
        }
    }

    /// Creates an invalid structure error
    pub fn invalid_structure<S: Into<String>>(message: S) -> Self {
        Self::InvalidStructure {
            message: message.into(),
        }
    }

    /// Creates a source not found error
    pub fn source_not_found<S: Into<String>>(path: S) -> Self {
        Self::SourceNotFound { path: path.into() }
    }

    /// Creates a source error
    pub fn source_error<S: Into<String>, M: Into<String>>(path: S, message: M) -> Self {
        Self::SourceError {
            path: path.into(),
            message: message.into(),
        }
    }

    /// Creates an invalid source error
    pub fn invalid_source<S: Into<String>>(message: S) -> Self {
        Self::InvalidSource {
            message: message.into(),
        }
    }

    /// Creates an invalid band error
    pub fn invalid_band<S: Into<String>>(message: S) -> Self {
        Self::InvalidBand {
            message: message.into(),
        }
    }

    /// Creates a band out of range error
    pub fn band_out_of_range(band: usize, max: usize) -> Self {
        Self::BandOutOfRange { band, max }
    }

    /// Creates an invalid extent error
    pub fn invalid_extent<S: Into<String>>(message: S) -> Self {
        Self::InvalidExtent {
            message: message.into(),
        }
    }

    /// Creates an invalid window error
    pub fn invalid_window<S: Into<String>>(message: S) -> Self {
        Self::InvalidWindow {
            message: message.into(),
        }
    }

    /// Creates a missing attribute error
    pub fn missing_attribute<S: Into<String>>(attribute: S) -> Self {
        Self::MissingAttribute {
            attribute: attribute.into(),
        }
    }

    /// Creates a path resolution error
    pub fn path_resolution<S: Into<String>, M: Into<String>>(path: S, message: M) -> Self {
        Self::PathResolution {
            path: path.into(),
            message: message.into(),
        }
    }

    /// Creates a cache error
    pub fn cache_error<S: Into<String>>(message: S) -> Self {
        Self::CacheError {
            message: message.into(),
        }
    }

    /// Creates an incompatible sources error
    pub fn incompatible_sources<S: Into<String>>(message: S) -> Self {
        Self::IncompatibleSources {
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let err = VrtError::xml_parse("test error");
        assert!(matches!(err, VrtError::XmlParse { .. }));

        let err = VrtError::source_not_found("/path/to/file.tif");
        assert!(matches!(err, VrtError::SourceNotFound { .. }));

        let err = VrtError::band_out_of_range(5, 3);
        assert!(matches!(err, VrtError::BandOutOfRange { band: 5, max: 3 }));
    }

    #[test]
    fn test_error_display() {
        let err = VrtError::xml_parse("invalid XML");
        assert_eq!(err.to_string(), "XML parsing error: invalid XML");

        let err = VrtError::source_not_found("/test.tif");
        assert_eq!(err.to_string(), "Source file not found: /test.tif");

        let err = VrtError::band_out_of_range(5, 3);
        assert_eq!(err.to_string(), "Band 5 out of range (0-3)");
    }
}
