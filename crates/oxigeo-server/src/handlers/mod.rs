//! HTTP request handlers
//!
//! This module contains handlers for various tile serving protocols:
//! - WMS (Web Map Service)
//! - WMTS (Web Map Tile Service)
//! - XYZ Tiles (simple tile serving)

pub mod rendering;
pub mod tiles;
pub mod wms;
pub mod wmts;

pub use tiles::{TileState, get_tile, get_tilejson};
pub use wms::{WmsState, get_capabilities as wms_get_capabilities, get_feature_info, get_map};
pub use wmts::{WmtsState, get_capabilities as wmts_get_capabilities, get_tile_kvp, get_tile_rest};
