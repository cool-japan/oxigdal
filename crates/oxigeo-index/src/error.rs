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

    /// The serialized data does not start with the expected magic bytes.
    #[error("invalid magic bytes in serialized R-tree data")]
    InvalidMagic,

    /// The version byte in the serialized data is not supported.
    #[error("unsupported serialization version: {0}")]
    UnsupportedVersion(u8),

    /// The serialized data was truncated or otherwise incomplete.
    #[error("truncated or corrupt serialized data at offset {0}")]
    TruncatedData(usize),

    /// The entry was not found in the R-tree during removal.
    #[error("entry not found in R-tree")]
    EntryNotFound,

    /// The input geometry or point set is degenerate and the operation cannot
    /// proceed (e.g. all points collinear, fewer than 3 distinct points).
    #[error("invalid input: {0}")]
    InvalidInput(String),
}
