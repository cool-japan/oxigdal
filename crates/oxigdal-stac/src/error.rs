//! Error types for STAC operations.

/// Result type for STAC operations.
pub type Result<T> = core::result::Result<T, StacError>;

/// Error types that can occur during STAC operations.
#[derive(Debug, thiserror::Error)]
pub enum StacError {
    /// Invalid STAC type (not a Catalog, Collection, or Item).
    #[error("Invalid STAC type: expected {expected}, found {found}")]
    InvalidType {
        /// Expected STAC type.
        expected: String,
        /// Found STAC type.
        found: String,
    },

    /// Invalid STAC version.
    #[error("Invalid STAC version: {0}")]
    InvalidVersion(String),

    /// Missing required field.
    #[error("Missing required field: {0}")]
    MissingField(String),

    /// Invalid field value.
    #[error("Invalid field value for {field}: {reason}")]
    InvalidFieldValue {
        /// Field name.
        field: String,
        /// Reason for invalidity.
        reason: String,
    },

    /// Invalid URL.
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    /// Invalid geometry.
    #[error("Invalid geometry: {0}")]
    InvalidGeometry(String),

    /// Invalid datetime.
    #[error("Invalid datetime: {0}")]
    InvalidDatetime(String),

    /// Invalid bbox.
    #[error("Invalid bbox: {0}")]
    InvalidBbox(String),

    /// Serialization error.
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Deserialization error.
    #[error("Deserialization error: {0}")]
    Deserialization(String),

    /// HTTP request error.
    #[cfg(feature = "reqwest")]
    #[error("HTTP request error: {0}")]
    Http(String),

    /// Asset not found.
    #[error("Asset not found: {0}")]
    AssetNotFound(String),

    /// Link not found.
    #[error("Link not found: {0}")]
    LinkNotFound(String),

    /// Extension not found.
    #[error("Extension not found: {0}")]
    ExtensionNotFound(String),

    /// Invalid extension data.
    #[error("Invalid extension data for {extension}: {reason}")]
    InvalidExtension {
        /// Extension identifier.
        extension: String,
        /// Reason for invalidity.
        reason: String,
    },

    /// IO error.
    #[error("IO error: {0}")]
    Io(String),

    /// Invalid search parameters.
    #[error("Invalid search parameters: {0}")]
    InvalidSearchParams(String),

    /// API response error.
    #[error("API response error: {0}")]
    ApiResponse(String),

    /// Builder error.
    #[error("Builder error: {0}")]
    Builder(String),

    /// Other error.
    #[error("{0}")]
    Other(String),
}

impl From<serde_json::Error> for StacError {
    fn from(err: serde_json::Error) -> Self {
        if err.is_data() {
            StacError::Deserialization(err.to_string())
        } else {
            StacError::Serialization(err.to_string())
        }
    }
}

impl From<url::ParseError> for StacError {
    fn from(err: url::ParseError) -> Self {
        StacError::InvalidUrl(err.to_string())
    }
}

impl From<chrono::ParseError> for StacError {
    fn from(err: chrono::ParseError) -> Self {
        StacError::InvalidDatetime(err.to_string())
    }
}

#[cfg(feature = "reqwest")]
impl From<reqwest::Error> for StacError {
    fn from(err: reqwest::Error) -> Self {
        StacError::Http(err.to_string())
    }
}

impl From<geojson::Error> for StacError {
    fn from(err: geojson::Error) -> Self {
        StacError::InvalidGeometry(err.to_string())
    }
}

#[cfg(feature = "std")]
impl From<std::io::Error> for StacError {
    fn from(err: std::io::Error) -> Self {
        StacError::Io(err.to_string())
    }
}
