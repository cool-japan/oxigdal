//! Error types for oxigeo-pmtiles

use thiserror::Error;

/// Errors that can occur when parsing PMTiles files.
#[derive(Debug, Error)]
pub enum PmTilesError {
    /// The binary data does not conform to the expected format.
    #[error("Invalid format: {0}")]
    InvalidFormat(String),

    /// The archive structure is invalid (higher-level consistency check).
    #[error("Invalid archive: {0}")]
    InvalidArchive(String),

    /// Unsupported PMTiles spec version.
    #[error("Unsupported PMTiles version: {0}")]
    UnsupportedVersion(u8),

    /// The requested compression algorithm is not supported.
    #[error("Unsupported compression algorithm")]
    UnsupportedCompression,

    /// An I/O error occurred.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Decompression failed.
    #[error("Decompression error: {0}")]
    Decompression(String),

    /// JSON metadata could not be parsed.
    #[error("JSON parse error: {0}")]
    JsonParse(String),

    /// The requested tile was not found in the archive.
    #[error("Tile not found: z={0}, x={1}, y={2}")]
    TileNotFound(u8, u32, u32),

    /// The bounding box provided is invalid (e.g. antimeridian-crossing or
    /// degenerate bounds).
    #[error("Invalid bounds: {0}")]
    InvalidBounds(String),

    /// An I/O error that does not wrap `std::io::Error` directly (e.g. from a
    /// string message produced by an HTTP transport layer).
    #[error("IO error: {0}")]
    IoError(String),

    /// An HTTP-level error occurred while fetching a remote PMTiles archive
    /// (e.g. unexpected status code, connection failure, or URL parse error).
    #[error("HTTP error: {0}")]
    HttpError(String),

    /// An SQLite error that occurred during MBTiles export.
    #[cfg(feature = "mbtiles")]
    #[error("SQLite error: {0}")]
    SqliteError(String),

    /// Invalid PMTiles v2 header.
    #[error("invalid PMTiles v2 header: {0}")]
    InvalidV2Header(String),
}
