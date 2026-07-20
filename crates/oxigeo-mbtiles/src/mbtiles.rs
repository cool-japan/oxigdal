//! MBTiles format types and in-memory store.
//!
//! MBTiles is a SQLite-based tile archive defined at
//! <https://github.com/mapbox/mbtiles-spec>.

use std::collections::HashMap;

use crate::error::MbTilesError;
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
    /// Any additional metadata fields not covered by the canonical MBTiles 1.3
    /// keys.  Preserves arbitrary `(name, value)` rows from the `metadata`
    /// table so callers can round-trip unknown vendor extensions.
    pub extras: HashMap<String, String>,
}

impl MBTilesMetadata {
    /// Build metadata from a flat key/value map as read from the `metadata`
    /// table.
    ///
    /// This constructor is **lenient**: malformed numeric or CSV fields are
    /// silently dropped (left as `None`).  Use [`Self::from_map_strict`] when
    /// validation errors must surface.
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
                    meta.extras.insert(k.clone(), v.clone());
                }
            }
        }
        meta
    }

    /// Build metadata from a flat key/value map, returning a typed
    /// [`MbTilesError::InvalidMetadata`] if any canonical field is malformed.
    ///
    /// Used by `crate::reader::MBTilesReader` (feature-gated behind `sqlite`)
    /// so callers can distinguish a corrupted archive from a missing optional
    /// field.
    pub fn from_map_strict(map: HashMap<String, String>) -> Result<Self, MbTilesError> {
        let mut meta = Self::default();
        for (k, v) in &map {
            match k.as_str() {
                "name" => meta.name = Some(v.clone()),
                "format" => meta.format = Some(TileFormat::parse_format(v)),
                "minzoom" => {
                    meta.minzoom = Some(v.parse().map_err(|_| {
                        MbTilesError::InvalidMetadata(format!(
                            "minzoom must be an integer 0-30, got {v:?}"
                        ))
                    })?);
                }
                "maxzoom" => {
                    meta.maxzoom = Some(v.parse().map_err(|_| {
                        MbTilesError::InvalidMetadata(format!(
                            "maxzoom must be an integer 0-30, got {v:?}"
                        ))
                    })?);
                }
                "attribution" => meta.attribution = Some(v.clone()),
                "description" => meta.description = Some(v.clone()),
                "type" => meta.tile_type = Some(v.clone()),
                "version" => meta.version = Some(v.clone()),
                "json" => meta.json = Some(v.clone()),
                "bounds" => {
                    let parts: Result<Vec<f64>, _> =
                        v.split(',').map(|s| s.trim().parse::<f64>()).collect();
                    let parts = parts.map_err(|_| {
                        MbTilesError::InvalidMetadata(format!(
                            "bounds must be \"minlon,minlat,maxlon,maxlat\", got {v:?}"
                        ))
                    })?;
                    if parts.len() != 4 {
                        return Err(MbTilesError::InvalidMetadata(format!(
                            "bounds must have 4 comma-separated values, got {} in {:?}",
                            parts.len(),
                            v
                        )));
                    }
                    meta.bounds = Some([parts[0], parts[1], parts[2], parts[3]]);
                }
                "center" => {
                    let parts: Result<Vec<f64>, _> =
                        v.split(',').map(|s| s.trim().parse::<f64>()).collect();
                    let parts = parts.map_err(|_| {
                        MbTilesError::InvalidMetadata(format!(
                            "center must be \"lon,lat,zoom\", got {v:?}"
                        ))
                    })?;
                    if parts.len() != 3 {
                        return Err(MbTilesError::InvalidMetadata(format!(
                            "center must have 3 comma-separated values, got {} in {:?}",
                            parts.len(),
                            v
                        )));
                    }
                    meta.center = Some([parts[0], parts[1], parts[2]]);
                }
                _ => {
                    meta.extras.insert(k.clone(), v.clone());
                }
            }
        }
        Ok(meta)
    }

    /// Return `(minzoom, maxzoom)` when both values are present.
    pub fn zoom_range(&self) -> Option<(u8, u8)> {
        self.minzoom.zip(self.maxzoom)
    }

    /// Accessor for the non-canonical extra metadata keys.
    ///
    /// Equivalent to direct field access on [`Self::extras`]; provided so the
    /// reader API can expose the map without a public field reference.
    pub fn extras(&self) -> &HashMap<String, String> {
        &self.extras
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

// ─── MBTiles 1.3 metadata compliance validation ─────────────────────────────────

impl MBTilesMetadata {
    /// Validate this metadata against the MBTiles 1.3 specification.
    ///
    /// Returns every conformance issue found (empty when fully compliant).
    /// `scheme` records the declared tile-coordinate convention; see
    /// [`crate::validation::validate_metadata`] for the rule set. Only
    /// `&self`-readable fields are consulted, so the check is unaffected by any
    /// additional fields the struct may carry.
    #[must_use]
    pub fn validate(
        &self,
        scheme: crate::writer::TileScheme,
    ) -> Vec<crate::validation::ValidationIssue> {
        crate::validation::validate_metadata(self, scheme)
    }
}
