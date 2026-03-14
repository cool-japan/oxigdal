//! Error types for oxigdal-gpkg

use thiserror::Error;

/// Errors that can occur when parsing GeoPackage / SQLite files.
#[derive(Debug, Error)]
pub enum GpkgError {
    /// The binary data does not conform to the expected format.
    #[error("Invalid format: {0}")]
    InvalidFormat(String),

    /// An I/O error occurred while reading.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
