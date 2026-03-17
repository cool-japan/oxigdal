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

    /// The GeoPackage geometry blob does not start with the expected magic bytes (0x47 0x50).
    #[error("Invalid GeoPackage geometry magic bytes")]
    InvalidGeometryMagic,

    /// A WKB geometry could not be parsed.
    #[error("WKB parse error: {0}")]
    WkbParseError(String),

    /// The WKB type code is not recognised.
    #[error("Unknown WKB geometry type: {0}")]
    UnknownWkbType(u32),

    /// A parse operation needed more bytes than were available.
    #[error("Insufficient data: needed {needed} bytes, available {available}")]
    InsufficientData {
        /// Number of bytes required by the operation.
        needed: usize,
        /// Number of bytes actually present in the buffer.
        available: usize,
    },
}
