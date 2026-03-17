//! MBTiles writer and in-memory tile archive builder.
//!
//! Provides [`MBTilesWriter`] for constructing tile archives in memory,
//! [`TileScheme`] for coordinate-system handling, [`TileRange`] for batch
//! tile-range iteration, [`VectorLayerSpec`] for vector-layer metadata, and
//! [`TileStatsAggregator`] for computing statistics across a tile set.

use std::collections::HashMap;

use serde_json::Value as JsonValue;

use crate::mbtiles::MBTilesMetadata;
use crate::tile_coords::{TileCoord, TileFormat};

// ── TileScheme ───────────────────────────────────────────────────────────────

/// Tile coordinate scheme — determines the direction of the y axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TileScheme {
    /// TMS scheme: y = 0 at the south (default for MBTiles spec).
    #[default]
    Tms,
    /// XYZ scheme: y = 0 at the north (Google / OSM / Mapbox convention).
    Xyz,
}

impl TileScheme {
    /// Flip a y coordinate from one scheme to the other.
    ///
    /// Both conversions follow the same formula: `2^z − 1 − y`.
    /// The operation is its own inverse.
    pub fn flip_y(&self, y: u32, z: u8) -> u32 {
        let n = 1u32 << z; // 2^z
        n.saturating_sub(1).saturating_sub(y)
    }
}

// ── TileRange ────────────────────────────────────────────────────────────────

/// A rectangular range of tiles across a set of zoom levels.
#[derive(Debug, Clone)]
pub struct TileRange {
    /// Minimum zoom level (inclusive).
    pub min_zoom: u8,
    /// Maximum zoom level (inclusive).
    pub max_zoom: u8,
    /// Minimum x tile coordinate (inclusive).
    pub min_x: u32,
    /// Maximum x tile coordinate (inclusive).
    pub max_x: u32,
    /// Minimum y tile coordinate (inclusive, XYZ scheme).
    pub min_y: u32,
    /// Maximum y tile coordinate (inclusive, XYZ scheme).
    pub max_y: u32,
}

impl TileRange {
    /// Create a [`TileRange`] covering the given WGS84 bounding box at the
    /// specified zoom levels.
    ///
    /// Coordinates are in the XYZ scheme (y = 0 at north).
    pub fn from_bbox(
        west: f64,
        south: f64,
        east: f64,
        north: f64,
        min_zoom: u8,
        max_zoom: u8,
    ) -> Self {
        use crate::bbox_util::bbox_to_tiles;

        // Use the maximum zoom to establish the tightest tile grid.  At lower
        // zoom levels every tile in the zoom-0 range is always included, so we
        // just use the bounding-box helper at each zoom but store the
        // per-level extents as the union across all zoom levels.  For the
        // stored range we therefore use min/max zoom-level tile extents.
        let (min_x, min_y, max_x, max_y) = if min_zoom == max_zoom {
            bbox_to_tiles(west, south, east, north, min_zoom)
        } else {
            // At the coarsest zoom the range is fewest tiles; at the finest it
            // is most tiles.  The TileRange captures the *per-zoom* min/max
            // via the iterator; we store the max-zoom extents so the iterator
            // can compute per-zoom from those.
            bbox_to_tiles(west, south, east, north, max_zoom)
        };

        Self {
            min_zoom,
            max_zoom,
            min_x,
            max_x,
            min_y,
            max_y,
        }
    }

    /// Total number of tiles in this range (across all zoom levels).
    pub fn tile_count(&self) -> u64 {
        let mut total: u64 = 0;
        for z in self.min_zoom..=self.max_zoom {
            let factor = 1u64 << (self.max_zoom.saturating_sub(z));
            let x_count =
                (self.max_x / factor as u32).saturating_sub(self.min_x / factor as u32) + 1;
            let y_count =
                (self.max_y / factor as u32).saturating_sub(self.min_y / factor as u32) + 1;
            total += x_count as u64 * y_count as u64;
        }
        total
    }

    /// Return an iterator over all `(z, x, y)` coordinates in this range.
    pub fn iter(&self) -> TileRangeIter {
        let (start_x, start_y) = if self.min_zoom == self.max_zoom {
            (self.min_x, self.min_y)
        } else {
            let factor = 1u32 << (self.max_zoom.saturating_sub(self.min_zoom));
            (self.min_x / factor, self.min_y / factor)
        };

        TileRangeIter {
            range: self.clone(),
            current_z: self.min_zoom,
            current_x: start_x,
            current_y: start_y,
        }
    }
}

/// Iterator produced by [`TileRange::iter`].
pub struct TileRangeIter {
    range: TileRange,
    current_z: u8,
    current_x: u32,
    current_y: u32,
}

impl TileRangeIter {
    /// Compute the x/y bounds for `z` by scaling the max-zoom extents.
    fn bounds_for_zoom(&self, z: u8) -> (u32, u32, u32, u32) {
        let shift = self.range.max_zoom.saturating_sub(z);
        let factor = 1u32 << shift;
        let min_x = self.range.min_x / factor;
        let max_x = self.range.max_x / factor;
        let min_y = self.range.min_y / factor;
        let max_y = self.range.max_y / factor;
        (min_x, min_y, max_x, max_y)
    }
}

impl Iterator for TileRangeIter {
    type Item = (u8, u32, u32);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.current_z > self.range.max_zoom {
                return None;
            }

            let (min_x, min_y, max_x, max_y) = self.bounds_for_zoom(self.current_z);

            // Guard: if zoom has no tiles (degenerate range) advance
            if min_x > max_x || min_y > max_y {
                self.current_z += 1;
                if self.current_z <= self.range.max_zoom {
                    let (nx, ny, _, _) = self.bounds_for_zoom(self.current_z);
                    self.current_x = nx;
                    self.current_y = ny;
                }
                continue;
            }

            // Ensure we start inside the valid range
            if self.current_x < min_x {
                self.current_x = min_x;
            }
            if self.current_y < min_y {
                self.current_y = min_y;
            }

            if self.current_x > max_x {
                // Move to next zoom level
                self.current_z += 1;
                if self.current_z <= self.range.max_zoom {
                    let (nx, ny, _, _) = self.bounds_for_zoom(self.current_z);
                    self.current_x = nx;
                    self.current_y = ny;
                }
                continue;
            }

            if self.current_y > max_y {
                // Move to next x column
                self.current_x += 1;
                self.current_y = min_y;
                continue;
            }

            let result = (self.current_z, self.current_x, self.current_y);
            // Advance to next position
            self.current_y += 1;
            if self.current_y > max_y {
                self.current_y = min_y;
                self.current_x += 1;
            }
            return Some(result);
        }
    }
}

// ── VectorLayerSpec ──────────────────────────────────────────────────────────

/// Field type for a vector layer attribute.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldType {
    /// Numeric field (integer or floating-point).
    Number,
    /// Boolean field.
    Boolean,
    /// String / text field.
    String,
}

impl FieldType {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Number => "Number",
            Self::Boolean => "Boolean",
            Self::String => "String",
        }
    }
}

/// Specification for a single vector layer stored inside MBTiles JSON metadata.
#[derive(Debug, Clone)]
pub struct VectorLayerSpec {
    /// Unique layer identifier.
    pub id: String,
    /// Optional human-readable description.
    pub description: Option<String>,
    /// Minimum zoom level at which the layer is present.
    pub minzoom: u8,
    /// Maximum zoom level at which the layer is present.
    pub maxzoom: u8,
    /// Attribute fields: name → type.
    pub fields: HashMap<String, FieldType>,
}

impl VectorLayerSpec {
    /// Create a new layer spec with the given id and zoom range.
    pub fn new(id: impl Into<String>, minzoom: u8, maxzoom: u8) -> Self {
        Self {
            id: id.into(),
            description: None,
            minzoom,
            maxzoom,
            fields: HashMap::new(),
        }
    }

    /// Add a field to the layer spec (builder pattern).
    pub fn with_field(mut self, name: impl Into<String>, field_type: FieldType) -> Self {
        self.fields.insert(name.into(), field_type);
        self
    }

    /// Serialize the spec to a [`serde_json::Value`] following the TileJSON
    /// `vector_layers` element schema.
    pub fn to_json(&self) -> JsonValue {
        let mut obj = serde_json::json!({
            "id": self.id,
            "minzoom": self.minzoom,
            "maxzoom": self.maxzoom,
        });

        if let Some(desc) = &self.description {
            obj["description"] = JsonValue::String(desc.clone());
        }

        let mut fields_obj = serde_json::Map::new();
        let mut sorted_keys: Vec<&String> = self.fields.keys().collect();
        sorted_keys.sort();
        for key in sorted_keys {
            fields_obj.insert(
                key.clone(),
                JsonValue::String(self.fields[key].as_str().to_string()),
            );
        }
        obj["fields"] = JsonValue::Object(fields_obj);

        obj
    }
}

// ── TileStatsAggregator ──────────────────────────────────────────────────────

/// Per-zoom-level tile statistics.
#[derive(Debug, Clone)]
pub struct ZoomStats {
    /// Zoom level.
    pub zoom: u8,
    /// Number of tiles at this zoom level.
    pub tile_count: u64,
    /// Total byte size of all tiles at this zoom level.
    pub total_bytes: u64,
    /// Mean byte size per tile.
    pub mean_bytes: f64,
}

/// Accumulates tile statistics across an entire tile set.
#[derive(Debug, Clone)]
pub struct TileStatsAggregator {
    /// Total number of tiles processed.
    pub total_tiles: u64,
    /// Total byte size of all tiles.
    pub total_bytes: u64,
    /// Smallest single-tile byte size seen (u64::MAX when no tiles added).
    pub min_bytes: u64,
    /// Largest single-tile byte size seen.
    pub max_bytes: u64,
    /// Per-zoom statistics.
    pub per_zoom: HashMap<u8, ZoomStats>,
}

impl TileStatsAggregator {
    /// Create an empty aggregator.
    pub fn new() -> Self {
        Self {
            total_tiles: 0,
            total_bytes: 0,
            min_bytes: u64::MAX,
            max_bytes: 0,
            per_zoom: HashMap::new(),
        }
    }

    /// Record one tile with the given zoom level and byte size.
    pub fn add_tile(&mut self, z: u8, size_bytes: u64) {
        self.total_tiles += 1;
        self.total_bytes += size_bytes;
        if size_bytes < self.min_bytes {
            self.min_bytes = size_bytes;
        }
        if size_bytes > self.max_bytes {
            self.max_bytes = size_bytes;
        }

        let entry = self.per_zoom.entry(z).or_insert_with(|| ZoomStats {
            zoom: z,
            tile_count: 0,
            total_bytes: 0,
            mean_bytes: 0.0,
        });
        entry.tile_count += 1;
        entry.total_bytes += size_bytes;
        entry.mean_bytes = entry.total_bytes as f64 / entry.tile_count as f64;
    }

    /// Mean byte size across all tiles, or 0.0 when no tiles have been added.
    pub fn mean_bytes(&self) -> f64 {
        if self.total_tiles == 0 {
            0.0
        } else {
            self.total_bytes as f64 / self.total_tiles as f64
        }
    }

    /// Ratio of `uncompressed_size` to the stored `total_bytes`.
    ///
    /// Returns 1.0 when `total_bytes` is zero to avoid division by zero.
    pub fn compression_ratio(&self, uncompressed_size: u64) -> f64 {
        if self.total_bytes == 0 {
            1.0
        } else {
            uncompressed_size as f64 / self.total_bytes as f64
        }
    }
}

impl Default for TileStatsAggregator {
    fn default() -> Self {
        Self::new()
    }
}

// ── MBTilesData ──────────────────────────────────────────────────────────────

/// Immutable snapshot of a built tile archive.
///
/// Produced by [`MBTilesWriter::build`].
#[derive(Debug)]
pub struct MBTilesData {
    /// Archive metadata.
    pub metadata: MBTilesMetadata,
    /// All tiles keyed by their [`TileCoord`] (TMS y-coordinate scheme).
    pub tiles: HashMap<TileCoord, Vec<u8>>,
    /// Tile image/vector format.
    pub format: TileFormat,
}

impl MBTilesData {
    /// Retrieve tile bytes at `(z, x, y)` using the TMS y-coordinate.
    ///
    /// Returns `None` if the tile does not exist.
    pub fn get_tile(&self, z: u8, x: u32, y: u32) -> Option<&Vec<u8>> {
        self.tiles.get(&TileCoord { z, x, y })
    }

    /// Total number of tiles in the archive.
    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }

    /// Return `(min_zoom, max_zoom)` across all stored tiles, or `None` when
    /// the archive is empty.
    pub fn zoom_range(&self) -> Option<(u8, u8)> {
        let mut min_z: Option<u8> = None;
        let mut max_z: Option<u8> = None;
        for coord in self.tiles.keys() {
            min_z = Some(min_z.map_or(coord.z, |m: u8| m.min(coord.z)));
            max_z = Some(max_z.map_or(coord.z, |m: u8| m.max(coord.z)));
        }
        min_z.zip(max_z)
    }

    /// Sum of the byte sizes of all stored tiles.
    pub fn total_size_bytes(&self) -> u64 {
        self.tiles.values().map(|v| v.len() as u64).sum()
    }
}

// ── MBTilesWriter ────────────────────────────────────────────────────────────

/// Builder for in-memory MBTiles tile archives.
///
/// Tiles are stored using the TMS y-coordinate convention (y = 0 at south) as
/// required by the MBTiles specification.  Use [`add_tile_xyz`] when your
/// input data uses the XYZ convention (y = 0 at north).
///
/// [`add_tile_xyz`]: MBTilesWriter::add_tile_xyz
#[derive(Debug)]
pub struct MBTilesWriter {
    metadata: MBTilesMetadata,
    tiles: HashMap<TileCoord, Vec<u8>>,
    format: TileFormat,
}

impl MBTilesWriter {
    /// Create a new writer for the given tileset name and tile format.
    pub fn new(name: impl Into<String>, format: TileFormat) -> Self {
        let name_str = name.into();
        let metadata = MBTilesMetadata {
            name: Some(name_str),
            format: Some(format.clone()),
            ..Default::default()
        };
        Self {
            metadata,
            tiles: HashMap::new(),
            format,
        }
    }

    /// Replace the metadata (builder pattern).
    pub fn with_metadata(mut self, meta: MBTilesMetadata) -> Self {
        self.metadata = meta;
        self
    }

    /// Add a tile at `(z, x, y)` using the TMS y-coordinate (y = 0 at south).
    pub fn add_tile(&mut self, z: u8, x: u32, y: u32, data: Vec<u8>) {
        self.tiles.insert(TileCoord { z, x, y }, data);
    }

    /// Add a tile at `(z, x, y)` using the XYZ y-coordinate (y = 0 at north).
    ///
    /// The y value is automatically flipped to TMS before storage.
    pub fn add_tile_xyz(&mut self, z: u8, x: u32, y: u32, data: Vec<u8>) {
        let scheme = TileScheme::Xyz;
        let tms_y = scheme.flip_y(y, z);
        self.tiles.insert(TileCoord { z, x, y: tms_y }, data);
    }

    /// Remove the tile at `coord`.  Returns `true` if it existed.
    pub fn remove_tile(&mut self, coord: &TileCoord) -> bool {
        self.tiles.remove(coord).is_some()
    }

    /// Count tiles stored at zoom level `zoom`.
    pub fn count_at_zoom(&self, zoom: u8) -> u32 {
        self.tiles.keys().filter(|c| c.z == zoom).count() as u32
    }

    /// Return the sorted, deduplicated list of zoom levels present.
    pub fn zoom_levels(&self) -> Vec<u8> {
        let mut zooms: Vec<u8> = self.tiles.keys().map(|c| c.z).collect();
        zooms.sort_unstable();
        zooms.dedup();
        zooms
    }

    /// Consume the writer and produce an [`MBTilesData`] snapshot.
    pub fn build(self) -> MBTilesData {
        MBTilesData {
            metadata: self.metadata,
            tiles: self.tiles,
            format: self.format,
        }
    }
}
