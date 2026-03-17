//! Pure Rust MBTiles tile archive reader and writer.
//!
//! Provides tile coordinate helpers ([`tile_coords`]), an in-memory
//! MBTiles store ([`mbtiles`]), a tile archive builder ([`writer`]),
//! and geographic coordinate conversion utilities ([`bbox_util`]).

pub mod bbox_util;
pub mod error;
pub mod mbtiles;
pub mod tile_coords;
pub mod writer;

pub use bbox_util::{
    bbox_to_tiles, lonlat_to_tile, tile_count_at_zoom, tile_resolution_degrees,
    tile_resolution_metres, tile_to_bbox, tile_to_lonlat,
};
pub use error::MbTilesError;
pub use mbtiles::{MBTiles, MBTilesMetadata};
pub use tile_coords::{TileCoord, TileFormat, tms_to_xyz, xyz_to_tms};
pub use writer::{
    FieldType, MBTilesData, MBTilesWriter, TileRange, TileRangeIter, TileScheme,
    TileStatsAggregator, VectorLayerSpec, ZoomStats,
};
