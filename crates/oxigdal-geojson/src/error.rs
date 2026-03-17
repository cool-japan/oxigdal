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
}
