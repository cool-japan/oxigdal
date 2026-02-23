//! Visualization modules for web-based 3D mapping
//!
//! This module provides support for:
//! - 3D Tiles (Cesium) format
//! - Tileset JSON generation
//! - B3DM (Batched 3D Model) tiles
//! - PNTS (Point Cloud) tiles
//! - Hierarchical LOD structures

pub mod tiles3d;

// Re-exports
pub use tiles3d::{
    BoundingVolume, Tile, TileContent, Tileset, TilesetOptions, create_3d_tileset, write_3d_tiles,
};
