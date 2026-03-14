//! Pure Rust PMTiles v3 reader.
//!
//! Parses the 127-byte fixed header ([`header`]), varint-encoded directory
//! entries ([`directory`]), and provides a high-level reader ([`pmtiles`]).

pub mod directory;
pub mod error;
pub mod header;
pub mod pmtiles;

pub use directory::{DirectoryEntry, decode_directory, decode_varint};
pub use error::PmTilesError;
pub use header::{Compression, PmTilesHeader, TileType};
pub use pmtiles::PmTilesReader;
