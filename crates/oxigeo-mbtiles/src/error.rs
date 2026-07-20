//! Error types for oxigeo-mbtiles

use thiserror::Error;

/// Errors that can occur when working with MBTiles archives.
#[derive(Debug, Error)]
pub enum MbTilesError {
    /// The data does not conform to the expected format.
    #[error("Invalid format: {0}")]
    InvalidFormat(String),

    /// The requested tile does not exist.
    #[error("Tile not found: z={0} x={1} y={2}")]
    TileNotFound(u8, u32, u32),

    /// An I/O error occurred.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// A SQLite error surfaced from the underlying `.mbtiles` database.
    ///
    /// Only constructible when the `sqlite` cargo feature is enabled.
    #[cfg(feature = "sqlite")]
    #[error("SQLite error: {0}")]
    Sqlite(String),

    /// Metadata loaded from the archive failed validation.
    ///
    /// Typical causes: malformed `bounds` / `center` CSV strings, or
    /// non-numeric values for `minzoom` / `maxzoom`.
    #[error("Invalid metadata: {0}")]
    InvalidMetadata(String),
}

// No rusqlite From impl — the sqlite backend now uses the Pure-Rust OxiSQL engine.
