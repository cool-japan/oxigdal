//! Error types for metadata operations.

use thiserror::Error;

/// Result type for metadata operations.
pub type Result<T> = core::result::Result<T, MetadataError>;

/// Errors that can occur during metadata operations.
#[derive(Debug, Error)]
pub enum MetadataError {
    /// Invalid metadata format
    #[error("Invalid metadata format: {0}")]
    InvalidFormat(String),

    /// Missing required field
    #[error("Missing required field: {0}")]
    MissingField(String),

    /// Invalid field value
    #[error("Invalid field value for {field}: {reason}")]
    InvalidValue {
        /// Field name
        field: String,
        /// Reason for invalidity
        reason: String,
    },

    /// Validation error
    #[error("Validation failed: {0}")]
    ValidationError(String),

    /// XML parsing error
    #[cfg(feature = "xml")]
    #[error("XML error: {0}")]
    XmlError(String),

    /// JSON parsing error
    #[error("JSON error: {0}")]
    JsonError(String),

    /// Transformation error
    #[error("Transformation error: {0}")]
    TransformError(String),

    /// Unsupported operation
    #[error("Unsupported operation: {0}")]
    Unsupported(String),

    /// I/O error
    #[cfg(feature = "std")]
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// URL parse error
    #[error("URL parse error: {0}")]
    UrlError(#[from] url::ParseError),

    /// Date/time parse error
    #[error("DateTime parse error: {0}")]
    DateTimeError(String),

    /// Vocabulary error
    #[error("Invalid vocabulary term: {0}")]
    VocabularyError(String),

    /// Internal error
    #[error("Internal error: {0}")]
    Internal(String),
}

#[cfg(feature = "xml")]
impl From<quick_xml::Error> for MetadataError {
    fn from(err: quick_xml::Error) -> Self {
        MetadataError::XmlError(err.to_string())
    }
}

#[cfg(feature = "xml")]
impl From<quick_xml::DeError> for MetadataError {
    fn from(err: quick_xml::DeError) -> Self {
        MetadataError::XmlError(err.to_string())
    }
}

impl From<serde_json::Error> for MetadataError {
    fn from(err: serde_json::Error) -> Self {
        MetadataError::JsonError(err.to_string())
    }
}

impl From<chrono::ParseError> for MetadataError {
    fn from(err: chrono::ParseError) -> Self {
        MetadataError::DateTimeError(err.to_string())
    }
}
