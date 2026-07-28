//! Tile-based streaming protocols for geospatial data.
//!
//! This module provides tile streaming capabilities following standard protocols
//! like XYZ, TMS, and WMTS.

pub mod cache;
pub mod protocol;
pub mod provider;
pub mod pyramid;

pub use cache::{TileCache, TileCacheConfig};
pub use protocol::{
    FileSystemTileProtocol, TileCoordinate, TileProtocol, TileRequest, TileResponse,
};
pub use provider::{TileGenerator, TileProvider, TileSource};
pub use pyramid::{TileMatrix, TilePyramid, ZoomLevel};
