//! Error types for oxigdal-copc

use thiserror::Error;

/// Errors that can occur when parsing COPC / LAS files.
#[derive(Debug, Error)]
pub enum CopcError {
    /// The binary data does not conform to the expected format.
    #[error("Invalid format: {0}")]
    InvalidFormat(String),

    /// Unsupported LAS version encountered.
    #[error("Unsupported LAS version: {0}.{1}")]
    UnsupportedVersion(u8, u8),

    /// An I/O error occurred.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
