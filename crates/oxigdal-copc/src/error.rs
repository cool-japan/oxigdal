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

    /// A LAZ point format that this crate cannot yet decompress was requested.
    ///
    /// Slice 24 W3 ships PF0 and PF1 only.  PF6/7/8 (the COPC mandate) are
    /// deferred to a follow-up slice; until then, this variant flags them so
    /// callers can fail gracefully instead of producing garbage points.
    #[error("Unsupported LAZ point format: {format_id} (only formats 0 and 1 are supported)")]
    UnsupportedLazFormat {
        /// LAS point data record format ID (0-10) that is not yet supported.
        format_id: u8,
    },

    /// An internal LASzip decoder invariant was violated (malformed compressed stream).
    #[error("LAZ decoder error: {0}")]
    LazDecoderError(String),
}
