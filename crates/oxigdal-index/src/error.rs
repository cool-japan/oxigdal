//! Error types for the spatial index crate.

use thiserror::Error;

/// Errors that can occur during spatial index operations.
#[derive(Debug, Error)]
pub enum IndexError {
    /// A bounding box was invalid (min > max in some dimension).
    #[error("invalid bounding box: {0}")]
    InvalidBbox(String),

    /// An operation received an empty input where at least one element was
    /// required.
    #[error("input is empty")]
    EmptyInput,

    /// A grid index was requested with invalid dimensions.
    ///
    /// The two fields are `(cols, rows)`.
    #[error("invalid grid size: cols={0}, rows={1}; both must be > 0")]
    InvalidGridSize(usize, usize),
}
