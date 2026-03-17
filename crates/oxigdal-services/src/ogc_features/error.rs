//! Error types for the OGC Features API layer.

use thiserror::Error;

/// Errors produced by the OGC Features API layer
#[derive(Debug, Error)]
pub enum FeaturesError {
    /// Collection with the given ID was not found
    #[error("Collection not found: {0}")]
    CollectionNotFound(String),

    /// Bounding box is malformed or contains invalid values
    #[error("Invalid bbox: {0}")]
    InvalidBbox(String),

    /// Datetime string could not be parsed
    #[error("Invalid datetime: {0}")]
    InvalidDatetime(String),

    /// CRS URI is not supported or recognised
    #[error("Invalid CRS: {0}")]
    InvalidCrs(String),

    /// Client requested more features than the server allows
    #[error("Limit {requested} exceeds maximum allowed {max}")]
    LimitExceeded {
        /// Requested limit
        requested: u32,
        /// Server-side maximum
        max: u32,
    },

    /// Serde JSON deserialisation / serialisation failure
    #[error("JSON error: {0}")]
    SerdeError(#[from] serde_json::Error),

    /// CQL2 expression parse failure
    #[error("CQL2 parse error: {0}")]
    CqlParseError(String),
}
