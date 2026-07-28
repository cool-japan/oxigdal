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

    /// Flatten this metadata back into `(name, value)` rows suitable for
    /// insertion into an MBTiles `metadata` table.
    ///
    /// This is the inverse of [`Self::from_map`] / [`Self::from_map_strict`]:
    /// every canonical field that is `Some` is emitted under its MBTiles 1.3
    /// key, followed by every `extras` entry verbatim. Row order is
    /// deterministic (canonical fields first in a fixed order, then extras
    /// sorted by key) so callers get reproducible output.
    pub fn to_rows(&self) -> Vec<(String, String)> {
        let mut rows = Vec::new();

        if let Some(name) = &self.name {
            rows.push(("name".to_string(), name.clone()));
        }
        if let Some(format) = &self.format {
            rows.push(("format".to_string(), format.as_metadata_str().into_owned()));
        }
        if let Some(bounds) = &self.bounds {
            rows.push((
                "bounds".to_string(),
                format!("{},{},{},{}", bounds[0], bounds[1], bounds[2], bounds[3]),
            ));
        }
        if let Some(center) = &self.center {
            rows.push((
                "center".to_string(),
                format!("{},{},{}", center[0], center[1], center[2]),
            ));
        }
        if let Some(minzoom) = &self.minzoom {
            rows.push(("minzoom".to_string(), minzoom.to_string()));
        }
        if let Some(maxzoom) = &self.maxzoom {
            rows.push(("maxzoom".to_string(), maxzoom.to_string()));
        }
        if let Some(attribution) = &self.attribution {
            rows.push(("attribution".to_string(), attribution.clone()));
        }
        if let Some(description) = &self.description {
            rows.push(("description".to_string(), description.clone()));
        }
        if let Some(tile_type) = &self.tile_type {
            rows.push(("type".to_string(), tile_type.clone()));
        }
        if let Some(version) = &self.version {
            rows.push(("version".to_string(), version.clone()));
        }
        if let Some(json) = &self.json {
            rows.push(("json".to_string(), json.clone()));
        }

        let mut extra_keys: Vec<&String> = self.extras.keys().collect();
        extra_keys.sort();
        for key in extra_keys {
            rows.push((key.clone(), self.extras[key].clone()));
        }

        rows
    }
}

/// In-memory MBTiles tile store.
///
/// A pure-Rust, dependency-free in-memory representation of an MBTiles tile
/// set. To persist an archive to a real on-disk SQLite `.mbtiles` file, build
/// an [`crate::writer::MBTilesData`] via [`crate::writer::MBTilesWriter`] and
/// call [`crate::writer::MBTilesData::write_to_file`] (requires the `sqlite`
/// cargo feature). [`MBTilesReader`](crate::reader::MBTilesReader) reads such
/// files back (also `sqlite`-gated).
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
