//! Pure Rust MBTiles tile archive reader and writer.
//!
//! Provides tile coordinate helpers ([`tile_coords`]), an in-memory
//! MBTiles store ([`mbtiles`]), a tile archive builder ([`writer`]),
//! geographic coordinate conversion utilities ([`bbox_util`]), and — when
//! the `sqlite` cargo feature is enabled — a real SQLite-backed reader
//! (`reader` module) *and* writer (`sqlite_writer` module, exposing
//! [`writer::MBTilesData::write_to_file`] /
//! [`writer::MBTilesWriter::write_to_file`]) for on-disk `.mbtiles` archives.

pub mod bbox_util;
pub mod error;
pub mod mbtiles;
#[cfg(feature = "sqlite")]
pub mod reader;
#[cfg(feature = "sqlite")]
pub mod sqlite_writer;
pub mod tile_coords;
pub mod validation;
pub mod writer;

pub use bbox_util::{
    bbox_to_tiles, lonlat_to_tile, tile_count_at_zoom, tile_resolution_degrees,
    tile_resolution_metres, tile_to_bbox, tile_to_lonlat,
};
pub use error::MbTilesError;
pub use mbtiles::{MBTiles, MBTilesMetadata};
#[cfg(feature = "sqlite")]
pub use reader::MBTilesReader;
pub use tile_coords::{TileCoord, TileFormat, tms_to_xyz, xyz_to_tms};
pub use validation::{IssueSeverity, ValidationIssue, validate_metadata};
pub use writer::{
    FieldType, MBTilesData, MBTilesWriter, TileRange, TileRangeIter, TileScheme,
    TileStatsAggregator, VectorLayerSpec, ZoomStats,
};
