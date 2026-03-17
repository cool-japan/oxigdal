//! Pure Rust PMTiles v3 reader and writer.
//!
//! Parses the 127-byte fixed header ([`header`]), varint-encoded directory
//! entries ([`directory`]), and provides a high-level reader ([`pmtiles`])
//! and writer ([`writer`]).  Tile IDs are computed via the Hilbert curve
//! ([`hilbert`]).

pub mod directory;
pub mod error;
pub mod header;
pub mod hilbert;
pub mod pmtiles;
pub mod varint;
pub mod writer;

pub use directory::{DirectoryEntry, decode_directory, decode_varint};
pub use error::PmTilesError;
pub use header::{Compression, PmTilesHeader, TileType};
pub use hilbert::{hilbert_to_xy, tile_id_to_zxy, xy_to_hilbert, zxy_to_tile_id};
pub use pmtiles::PmTilesReader;
pub use varint::encode_varint;
pub use writer::PmTilesBuilder;
