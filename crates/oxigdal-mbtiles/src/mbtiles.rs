//! MBTiles format types and in-memory store.
//!
//! MBTiles is a SQLite-based tile archive defined at
//! <https://github.com/mapbox/mbtiles-spec>.

use std::collections::HashMap;

use crate::tile_coords::{TileCoord, TileFormat};

/// Metadata extracted from the `metadata` table of an MBTiles archive.
#[derive(Debug, Clone, Default)]
pub struct MBTilesMetadata {
    /// Human-readable name of the tileset.
    pub name: Option<String>,
    /// Tile image format.
    pub format: Option<TileFormat>,
    /// Bounding box `[west, south, east, north]` in decimal degrees.
    pub bounds: Option<[f64; 4]>,
    /// Default view point `[longitude, latitude, zoom]`.
    pub center: Option<[f64; 3]>,
    /// Minimum available zoom level.
    pub minzoom: Option<u8>,
    /// Maximum available zoom level.
    pub maxzoom: Option<u8>,
    /// Attribution text (may contain HTML).
    pub attribution: Option<String>,
    /// Free-form description of the tileset.
    pub description: Option<String>,
    /// Layer type: `"overlay"` or `"baselayer"`.
    pub tile_type: Option<String>,
    /// Spec version string.
    pub version: Option<String>,
    /// Serialised TileJSON extension data.
    pub json: Option<String>,
    /// Any additional metadata fields not covered above.
    pub extra: HashMap<String, String>,
}

impl MBTilesMetadata {
    /// Build metadata from a flat key/value map as read from the `metadata` table.
    pub fn from_map(map: HashMap<String, String>) -> Self {
        let mut meta = Self::default();
        for (k, v) in &map {
            match k.as_str() {
                "name" => meta.name = Some(v.clone()),
                "format" => meta.format = Some(TileFormat::parse_format(v)),
                "minzoom" => meta.minzoom = v.parse().ok(),
                "maxzoom" => meta.maxzoom = v.parse().ok(),
                "attribution" => meta.attribution = Some(v.clone()),
                "description" => meta.description = Some(v.clone()),
                "type" => meta.tile_type = Some(v.clone()),
                "version" => meta.version = Some(v.clone()),
                "json" => meta.json = Some(v.clone()),
                "bounds" => {
                    let parts: Vec<f64> =
                        v.split(',').filter_map(|s| s.trim().parse().ok()).collect();
                    if parts.len() == 4 {
                        meta.bounds = Some([parts[0], parts[1], parts[2], parts[3]]);
                    }
                }
                "center" => {
                    let parts: Vec<f64> =
                        v.split(',').filter_map(|s| s.trim().parse().ok()).collect();
                    if parts.len() == 3 {
                        meta.center = Some([parts[0], parts[1], parts[2]]);
                    }
                }
                _ => {
                    meta.extra.insert(k.clone(), v.clone());
                }
            }
        }
        meta
    }

    /// Return `(minzoom, maxzoom)` when both values are present.
    pub fn zoom_range(&self) -> Option<(u8, u8)> {
        self.minzoom.zip(self.maxzoom)
    }
}

/// In-memory MBTiles tile store.
///
/// In production use this would delegate to a SQLite backend; here it
/// provides a pure-Rust, dependency-free store suitable for testing.
#[derive(Debug, Default)]
pub struct MBTiles {
    /// Tileset metadata.
    pub metadata: MBTilesMetadata,
    tiles: HashMap<TileCoord, Vec<u8>>,
}

impl MBTiles {
    /// Create an empty store with the given metadata.
    pub fn new(metadata: MBTilesMetadata) -> Self {
        Self {
            metadata,
            tiles: HashMap::new(),
        }
    }

    /// Insert (or replace) a tile.
    pub fn insert_tile(&mut self, coord: TileCoord, data: Vec<u8>) {
        self.tiles.insert(coord, data);
    }

    /// Retrieve the raw tile bytes for `coord`, if present.
    pub fn get_tile(&self, coord: &TileCoord) -> Option<&Vec<u8>> {
        self.tiles.get(coord)
    }

    /// Return the total number of tiles stored.
    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }

    /// Return all tiles at the given zoom level.
    pub fn tiles_at_zoom(&self, z: u8) -> Vec<(&TileCoord, &Vec<u8>)> {
        self.tiles.iter().filter(|(c, _)| c.z == z).collect()
    }

    /// Return the sorted, deduplicated list of zoom levels present.
    pub fn zoom_levels(&self) -> Vec<u8> {
        let mut zooms: Vec<u8> = self.tiles.keys().map(|c| c.z).collect();
        zooms.sort_unstable();
        zooms.dedup();
        zooms
    }

    /// Return `true` when a tile with the given coordinate exists.
    pub fn has_tile(&self, coord: &TileCoord) -> bool {
        self.tiles.contains_key(coord)
    }
}
