//! Tile-based streaming protocols for geospatial data.
//!
//! This module provides tile streaming capabilities following standard protocols
//! like XYZ, TMS, and WMTS.

pub mod cache;
pub mod protocol;
pub mod provider;
pub mod pyramid;

pub use cache::{TileCache, TileCacheConfig};
pub use protocol::{TileProtocol, TileRequest, TileResponse, TileCoordinate};
pub use provider::{TileProvider, TileSource};
pub use pyramid::{TilePyramid, TileMatrix, ZoomLevel};
