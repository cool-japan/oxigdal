//! Error types for oxigdal-mbtiles

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
}
