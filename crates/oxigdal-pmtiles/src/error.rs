//! Error types for oxigdal-pmtiles

use thiserror::Error;

/// Errors that can occur when parsing PMTiles files.
#[derive(Debug, Error)]
pub enum PmTilesError {
    /// The binary data does not conform to the expected format.
    #[error("Invalid format: {0}")]
    InvalidFormat(String),

    /// Unsupported PMTiles spec version.
    #[error("Unsupported PMTiles version: {0}")]
    UnsupportedVersion(u8),

    /// An I/O error occurred.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
