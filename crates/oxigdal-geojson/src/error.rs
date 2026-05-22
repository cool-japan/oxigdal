//! Error types for the streaming GeoJSON parser.

use thiserror::Error;

/// Errors produced by the streaming GeoJSON parser and writer.
#[derive(Debug, Error)]
pub enum GeoJsonError {
    /// Wraps a `serde_json` parse error.
    #[error("JSON parse error: {0}")]
    ParseError(#[from] serde_json::Error),

    /// The `"type"` or other field had an unexpected value.
    #[error("Invalid type: expected {expected}, got {got}")]
    InvalidType {
        /// The type that was expected.
        expected: String,
        /// The type that was found.
        got: String,
    },

    /// A required field was absent from the JSON object.
    #[error("Missing field: {0}")]
    MissingField(String),

    /// Coordinate data is malformed or non-representable.
    #[error("Invalid coordinates: {0}")]
    InvalidCoordinates(String),

    /// The JSON nesting exceeded the configured limit.
    #[error("Maximum nesting depth exceeded")]
    MaxDepthExceeded,

    /// A coordinate array was present but contained no elements.
    #[error("Empty coordinates array")]
    EmptyCoordinates,

    /// The JSON structure does not match expected GeoJSON layout.
    #[error("Invalid GeoJSON structure: {0}")]
    InvalidStructure(String),

    /// An I/O error occurred while reading.
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// A WKT string could not be parsed.
    #[error("WKT parse error: {0}")]
    WktParseError(String),

    /// An error occurred during TopoJSON encoding.
    #[error("TopoJSON error: {0}")]
    TopologyError(String),

    /// An error occurred during a dissolve / merge-by-property operation.
    #[error("dissolve error: {0}")]
    DissolveError(String),

    /// A CRS reprojection operation failed.
    #[cfg(feature = "reproject")]
    #[error("reprojection error: {0}")]
    ReprojectError(String),
}
