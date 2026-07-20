//! Streaming / iterator APIs for large geospatial datasets.
//!
//! This module provides two iterator types and an extension trait:
//!
//! - [`FeatureStream`] — lazy iterator over vector features (WKB + properties)
//! - [`TileStream`] — iterator over raster tile coordinates at a given zoom level
//! - [`StreamingExt`] — extension trait on [`OpenedDataset`]
//!
//! # Tile coordinate conventions
//!
//! [`TileStream`] follows the Web Map Tile Service (WMTS / XYZ) slippy-map
//! convention:
//!
//! - Zoom level 0: one tile covering the whole world
//! - At zoom `z`, there are `2^z × 2^z` tiles
//! - Tile `(x, y)` covers the rectangle
//!   `[x/2^z … (x+1)/2^z] × [y/2^z … (y+1)/2^z]` in normalised coordinates
//!
//! # Examples
//!
//! ```rust,no_run
//! use oxigdal::open::open;
//! use oxigdal::streaming::StreamingExt;
//!
//! # fn main() -> oxigdal::Result<()> {
//! let ds = open("world.geojson")?;
//! let mut stream = ds.features()?;
//! while let Some(feat) = stream.next() {
//!     let feat = feat?;
//!     println!("feature has {} properties", feat.properties.len());
//! }
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;

use oxigdal_core::error::OxiGdalError;
use serde_json::Value as JsonValue;

use crate::{Result, open::OpenedDataset};

// ─── StreamingFeature ─────────────────────────────────────────────────────────

/// A single vector feature returned by a [`FeatureStream`].
///
/// The geometry is encoded as WKB (Well-Known Binary) bytes, which is a compact
/// binary representation understood by all major GIS tools.  If the feature has
/// no geometry (attribute-only), `geometry` is `None`.
///
/// Properties are stored as a `HashMap<String, serde_json::Value>` mirroring
/// the GeoJSON feature properties object.
#[derive(Debug, Clone)]
pub struct StreamingFeature {
    /// Optional WKB-encoded geometry bytes.
    ///
    /// `None` when the feature carries attribute data only.
    pub geometry: Option<Vec<u8>>,

    /// Feature attribute values keyed by field name.
    ///
    /// Values use `serde_json::Value` to represent any JSON-compatible type
    /// (string, number, boolean, null, array, object).
    pub properties: HashMap<String, JsonValue>,

    /// Optional feature identifier (from FID, `@id`, etc.)
    pub id: Option<String>,
}

impl StreamingFeature {
    /// Create a new feature with the given geometry and properties.
    pub fn new(geometry: Option<Vec<u8>>, properties: HashMap<String, JsonValue>) -> Self {
        Self {
            geometry,
            properties,
            id: None,
        }
    }

    /// Create a feature with an identifier.
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Return `true` if this feature carries a geometry.
    pub fn has_geometry(&self) -> bool {
        self.geometry.is_some()
    }

    /// Return the WKB geometry length in bytes, or 0 if no geometry.
    pub fn geometry_byte_len(&self) -> usize {
        self.geometry.as_ref().map_or(0, |g| g.len())
    }
}

// ─── FeatureStream ────────────────────────────────────────────────────────────

/// Lazy iterator over features in a vector dataset.
///
/// Each call to [`Iterator::next`] yields `Some(Result<StreamingFeature>)`.
/// Errors propagate naturally through the `Result` wrapper, allowing consumers
/// to decide whether to abort or skip on error.
///
/// Obtained via [`StreamingExt::features`].
pub struct FeatureStream {
    /// Internal buffer of pre-loaded features.
    ///
    /// In a real driver implementation this would be replaced with a cursor
    /// into the underlying file/database.  For now features are buffered in
    /// memory at construction time.
    inner: std::vec::IntoIter<StreamingFeature>,
    /// Total number of features this stream was created with.
    total_count: usize,
    /// How many features have been yielded so far.
    yielded: usize,
}

impl FeatureStream {
    /// Create a [`FeatureStream`] from a pre-built `Vec<StreamingFeature>`.
    ///
    /// Used by driver implementations and tests.
    pub fn from_vec(features: Vec<StreamingFeature>) -> Self {
        let total_count = features.len();
        Self {
            inner: features.into_iter(),
            total_count,
            yielded: 0,
        }
    }

    /// Create an empty feature stream.
    pub fn empty() -> Self {
        Self::from_vec(Vec::new())
    }

    /// Return the total number of features this stream was seeded with.
    ///
    /// Note: for streaming sources where the total is unknown, this returns 0.
    pub fn total_count(&self) -> usize {
        self.total_count
    }

    /// Return the number of features that have been yielded so far.
    pub fn yielded_count(&self) -> usize {
        self.yielded
    }

    /// Return the number of remaining features, if known.
    pub fn remaining(&self) -> usize {
        self.total_count.saturating_sub(self.yielded)
    }
}

impl Iterator for FeatureStream {
    type Item = Result<StreamingFeature>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.inner.next() {
            Some(feature) => {
                self.yielded += 1;
                Some(Ok(feature))
            }
            None => None,
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.remaining();
        (remaining, Some(remaining))
    }
}

// ─── RasterTile ──────────────────────────────────────────────────────────────

/// A single raster tile at a specific zoom level and tile coordinate.
///
/// Follows the XYZ / WMTS slippy-map convention used by web mapping libraries
/// (Leaflet, MapLibre, OpenLayers, etc.).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RasterTile {
    /// Tile column index (0 … 2^zoom − 1, left to right)
    pub x: u32,
    /// Tile row index (0 … 2^zoom − 1, top to bottom)
    pub y: u32,
    /// Zoom level (0 = world overview, higher = more detail)
    pub zoom: u8,
    /// Raw tile image bytes (PNG, JPEG, WebP, etc.)
    pub data: Vec<u8>,
}

impl RasterTile {
    /// Return the number of tiles per axis at this zoom level: `2^zoom`.
    ///
    /// Saturates at [`u32::MAX`] for zoom ≥ 32 (which is never useful in
    /// practice, but avoids overflow).
    pub fn tiles_per_axis(zoom: u8) -> u32 {
        if zoom >= 32 { u32::MAX } else { 1u32 << zoom }
    }

    /// Return the normalised bounding box `(min_x, min_y, max_x, max_y)` for
    /// this tile, where coordinates are in the range `[0.0, 1.0]`.
    ///
    /// Useful for converting to geographic coordinates when combined with the
    /// dataset's [`crate::DatasetInfo::geotransform`].
    pub fn normalised_bbox(&self) -> (f64, f64, f64, f64) {
        let n = Self::tiles_per_axis(self.zoom) as f64;
        let min_x = self.x as f64 / n;
        let min_y = self.y as f64 / n;
        let max_x = (self.x + 1) as f64 / n;
        let max_y = (self.y + 1) as f64 / n;
        (min_x, min_y, max_x, max_y)
    }

    /// Return `true` if the tile data is non-empty.
    pub fn has_data(&self) -> bool {
        !self.data.is_empty()
    }
}

// ─── TileStream ───────────────────────────────────────────────────────────────

/// Iterator over raster tile coordinates at a fixed zoom level.
///
/// Tiles are yielded in row-major order: all columns for row 0, then row 1,
/// etc. (top-left to bottom-right).
///
/// The `data` field of each [`RasterTile`] is populated with empty bytes by
/// default.  Real raster data is filled in by the driver crate when the tiles
/// are actually read from disk.
///
/// Obtained via [`StreamingExt::tiles`].
pub struct TileStream {
    /// Fixed zoom level
    zoom: u8,
    /// Current tile column
    current_x: u32,
    /// Current tile row
    current_y: u32,
    /// Maximum tile column (exclusive)
    max_x: u32,
    /// Maximum tile row (exclusive)
    max_y: u32,
    /// Number of tiles yielded so far
    yielded: u64,
}

impl TileStream {
    /// Create a new [`TileStream`] that covers all tiles at the given `zoom`.
    ///
    /// At zoom `z`, there are `2^z × 2^z` tiles.
    pub fn full_zoom(zoom: u8) -> Self {
        let dim = RasterTile::tiles_per_axis(zoom);
        Self {
            zoom,
            current_x: 0,
            current_y: 0,
            max_x: dim,
            max_y: dim,
            yielded: 0,
        }
    }

    /// Create a [`TileStream`] covering a sub-rectangle of tiles at `zoom`.
    ///
    /// `x_range` and `y_range` are `(start, end)` half-open ranges
    /// (i.e., `start..end`).
    ///
    /// # Errors
    ///
    /// Returns [`OxiGdalError::OutOfBounds`] if the range exceeds `2^zoom`.
    pub fn from_range(zoom: u8, x_range: (u32, u32), y_range: (u32, u32)) -> Result<Self> {
        let dim = RasterTile::tiles_per_axis(zoom);
        let (x_start, x_end) = x_range;
        let (y_start, y_end) = y_range;

        if x_end > dim || y_end > dim {
            return Err(OxiGdalError::OutOfBounds {
                message: format!(
                    "tile range ({x_start}..{x_end}, {y_start}..{y_end}) exceeds 2^{zoom} = {dim}"
                ),
            });
        }
        if x_start >= x_end || y_start >= y_end {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "tile_range",
                message: format!(
                    "empty or inverted tile range: x={x_start}..{x_end}, y={y_start}..{y_end}"
                ),
            });
        }

        Ok(Self {
            zoom,
            current_x: x_start,
            current_y: y_start,
            max_x: x_end,
            max_y: y_end,
            yielded: 0,
        })
    }

    /// Total number of tiles this stream will produce.
    ///
    /// Returns `(max_x - start_x) × (max_y - start_y)` which may be large for
    /// high zoom levels.
    pub fn total_tiles(&self) -> u64 {
        (self.max_x - self.current_x) as u64 * (self.max_y - self.current_y) as u64 + self.yielded
    }

    /// Number of tiles yielded so far.
    pub fn yielded_count(&self) -> u64 {
        self.yielded
    }

    /// Current zoom level.
    pub fn zoom(&self) -> u8 {
        self.zoom
    }
}

impl Iterator for TileStream {
    type Item = Result<RasterTile>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_y >= self.max_y {
            return None;
        }

        let tile = RasterTile {
            x: self.current_x,
            y: self.current_y,
            zoom: self.zoom,
            data: Vec::new(), // populated by driver when reading from disk
        };

        // Advance column; wrap to next row when at max_x
        self.current_x += 1;
        if self.current_x >= self.max_x {
            self.current_x = 0; // reset column to start of range
            self.current_y += 1;
        }

        self.yielded += 1;
        Some(Ok(tile))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = (self.max_x.saturating_sub(self.current_x) as u64
            + (self.max_y.saturating_sub(self.current_y).saturating_sub(1)) as u64
                * (self.max_x as u64)) as usize;
        (remaining, Some(remaining))
    }
}

// ─── StreamingExt ─────────────────────────────────────────────────────────────

// ─── GeoJSON streaming helper ─────────────────────────────────────────────────

/// Stream features from a GeoJSON file path stored in `info.path`.
///
/// When the `geojson` feature is enabled and `info.path` points to a readable
/// FeatureCollection, this returns a [`FeatureStream`] with real feature data.
/// Properties are already `serde_json::Value` in the GeoJSON driver — no
/// conversion is needed.  Geometry is encoded as ISO WKB (little-endian) using
/// [`oxigdal_geojson::Geometry::to_wkb`].
///
/// Falls back to an empty stream when:
/// - `info.path` is `None` (e.g., programmatic dataset)
/// - the `geojson` feature is disabled
/// - the file is valid JSON but not a FeatureCollection (single Feature /
///   Geometry)
///
/// Genuine failures — I/O errors and malformed / truncated JSON — are surfaced
/// as [`OxiGdalError`] rather than silently reported as an empty dataset.
fn stream_geojson_features(info: &crate::DatasetInfo) -> Result<FeatureStream> {
    #[cfg(feature = "geojson")]
    {
        let path = match &info.path {
            Some(p) => p.clone(),
            None => return Ok(FeatureStream::empty()),
        };

        let file = std::fs::File::open(&path).map_err(|e| {
            OxiGdalError::Io(oxigdal_core::error::IoError::Read {
                message: format!("cannot open GeoJSON for streaming '{path}': {e}"),
            })
        })?;

        use oxigdal_geojson::GeoJsonReader;
        let buf_reader = std::io::BufReader::new(file);
        let mut reader = GeoJsonReader::without_validation(buf_reader);

        // We load the FeatureCollection into memory; for very large files the
        // caller should use the oxigdal_streaming crate instead.
        let fc = match reader.read_feature_collection() {
            Ok(fc) => fc,
            Err(read_err) => {
                // `read_feature_collection` collapses I/O errors, JSON syntax
                // errors, and "valid JSON but not a FeatureCollection" into one
                // Result. Only the last of these is a documented empty-stream
                // fallback — distinguish it by re-parsing the raw bytes as a
                // generic JSON value and inspecting the top-level "type".
                drop(reader);
                let raw = std::fs::read(&path).map_err(|e| {
                    OxiGdalError::Io(oxigdal_core::error::IoError::Read {
                        message: format!("cannot re-read GeoJSON '{path}': {e}"),
                    })
                })?;
                match serde_json::from_slice::<JsonValue>(&raw) {
                    Ok(value) => match value.get("type").and_then(JsonValue::as_str) {
                        // A single Feature / Geometry (or any non-collection
                        // GeoJSON object): documented empty-stream fallback.
                        Some(ty) if ty != "FeatureCollection" => {
                            return Ok(FeatureStream::empty());
                        }
                        // Declares itself a FeatureCollection yet failed to
                        // parse as one — a real structural error.
                        Some(_) => {
                            return Err(OxiGdalError::Io(oxigdal_core::error::IoError::Read {
                                message: format!(
                                    "failed to parse GeoJSON FeatureCollection '{path}': {read_err}"
                                ),
                            }));
                        }
                        // No top-level "type": malformed GeoJSON.
                        None => {
                            return Err(OxiGdalError::Io(oxigdal_core::error::IoError::Read {
                                message: format!(
                                    "GeoJSON '{path}' has no top-level \"type\" field: {read_err}"
                                ),
                            }));
                        }
                    },
                    // Not even valid JSON (truncated / corrupt) — surface it.
                    Err(parse_err) => {
                        return Err(OxiGdalError::Io(oxigdal_core::error::IoError::Read {
                            message: format!("invalid GeoJSON '{path}': {parse_err}"),
                        }));
                    }
                }
            }
        };

        let features = fc
            .features
            .into_iter()
            .map(|f| {
                let geometry = f.geometry.and_then(|g| g.to_wkb());
                let properties: HashMap<String, JsonValue> =
                    f.properties.unwrap_or_default().into_iter().collect();
                let mut sf = StreamingFeature::new(geometry, properties);
                if let Some(id) = f.id {
                    sf = sf.with_id(id.as_string());
                }
                sf
            })
            .collect::<Vec<_>>();

        Ok(FeatureStream::from_vec(features))
    }

    #[cfg(not(feature = "geojson"))]
    {
        let _ = info;
        Ok(FeatureStream::empty())
    }
}

// ─── FlatGeobuf streaming helper ─────────────────────────────────────────────

/// Stream features from a FlatGeobuf file path stored in `info.path`.
///
/// When the `flatgeobuf` feature is enabled and `info.path` points to a valid
/// FlatGeobuf file, this returns a [`FeatureStream`] with real feature data.
/// Geometry is encoded as ISO WKB (little-endian) via
/// [`oxigdal_core::vector::Geometry::to_wkb`].
/// Properties are converted from [`oxigdal_core::vector::FieldValue`] via
/// [`oxigdal_core::vector::FieldValue::to_json_value`].
///
/// Falls back to an empty stream when:
/// - `info.path` is `None`
/// - the `flatgeobuf` feature is disabled
fn stream_flatgeobuf_features(info: &crate::DatasetInfo) -> Result<FeatureStream> {
    #[cfg(feature = "flatgeobuf")]
    {
        use oxigdal_core::error::IoError;
        use oxigdal_flatgeobuf::FlatGeobufReader;

        let path = match &info.path {
            Some(p) => p.clone(),
            None => return Ok(FeatureStream::empty()),
        };

        let file = std::fs::File::open(&path).map_err(|e| {
            OxiGdalError::Io(IoError::Read {
                message: format!("cannot open FlatGeobuf for streaming '{path}': {e}"),
            })
        })?;

        let mut reader = FlatGeobufReader::new(file).map_err(|e| OxiGdalError::Internal {
            message: e.to_string(),
        })?;

        let iter = reader.features().map_err(|e| OxiGdalError::Internal {
            message: e.to_string(),
        })?;

        let features = iter
            .filter_map(|result| {
                result
                    .map_err(|e| OxiGdalError::Internal {
                        message: e.to_string(),
                    })
                    .ok()
            })
            .map(|f| {
                let geometry = f.geometry.map(|g| g.to_wkb());
                let properties: HashMap<String, JsonValue> = f
                    .properties
                    .into_iter()
                    .map(|(k, v)| (k, v.to_json_value()))
                    .collect();
                StreamingFeature::new(geometry, properties)
            })
            .collect::<Vec<_>>();

        Ok(FeatureStream::from_vec(features))
    }

    #[cfg(not(feature = "flatgeobuf"))]
    {
        let _ = info;
        Ok(FeatureStream::empty())
    }
}

// ─── Shapefile streaming helper ───────────────────────────────────────────────

/// Stream features from a Shapefile path stored in `info.path`.
///
/// When the `shapefile` feature is enabled and `info.path` points to a valid
/// Shapefile, this returns a [`FeatureStream`] with real feature data.
/// `info.path` may carry the `.shp` extension; it is stripped before
/// passing to [`oxigdal_shapefile::ShapefileReader::open`], which appends
/// the correct per-file extensions itself.
/// Geometry is encoded as ISO WKB (little-endian).
/// Properties are converted from [`oxigdal_core::vector::FieldValue`] via
/// [`oxigdal_core::vector::FieldValue::to_json_value`].
///
/// Falls back to an empty stream when:
/// - `info.path` is `None`
/// - the `shapefile` feature is disabled
fn stream_shapefile_features(info: &crate::DatasetInfo) -> Result<FeatureStream> {
    #[cfg(feature = "shapefile")]
    {
        use oxigdal_core::error::IoError;
        use oxigdal_shapefile::ShapefileReader;

        let raw_path = match &info.path {
            Some(p) => p.clone(),
            None => return Ok(FeatureStream::empty()),
        };

        // ShapefileReader::open() expects a base path without extension
        // (it appends .shp, .dbf, .shx itself). Strip a trailing .shp
        // extension if present.
        let base_path = {
            let p = std::path::Path::new(&raw_path);
            match p.extension().and_then(|e| e.to_str()) {
                Some("shp") | Some("SHP") => p.with_extension("").to_string_lossy().into_owned(),
                _ => raw_path.clone(),
            }
        };

        let reader = ShapefileReader::open(&base_path).map_err(|e| {
            OxiGdalError::Io(IoError::Read {
                message: format!("cannot open Shapefile for streaming '{base_path}': {e}"),
            })
        })?;

        let shapefile_features = reader.read_features().map_err(|e| OxiGdalError::Internal {
            message: e.to_string(),
        })?;

        let features = shapefile_features
            .into_iter()
            .map(|sf| {
                let geometry = sf.geometry.map(|g| g.to_wkb());
                let properties: HashMap<String, JsonValue> = sf
                    .attributes
                    .into_iter()
                    .map(|(k, v)| (k, v.to_json_value()))
                    .collect();
                StreamingFeature::new(geometry, properties)
            })
            .collect::<Vec<_>>();

        Ok(FeatureStream::from_vec(features))
    }

    #[cfg(not(feature = "shapefile"))]
    {
        let _ = info;
        Ok(FeatureStream::empty())
    }
}

/// Extension trait that adds streaming iterators to [`OpenedDataset`].
///
/// Import this trait to call `.features()` and `.tiles()` on an opened dataset.
///
/// ```rust,no_run
/// use oxigdal::open::open;
/// use oxigdal::streaming::StreamingExt;
///
/// # fn main() -> oxigdal::Result<()> {
/// let ds = open("world.geojson")?;
/// let count = ds.features()?.count();
/// println!("{count} features");
/// # Ok(())
/// # }
/// ```
pub trait StreamingExt {
    /// Return a streaming iterator over vector features in this dataset.
    ///
    /// # Errors
    ///
    /// Returns [`OxiGdalError::NotSupported`] when called on a raster-only
    /// dataset.
    fn features(&self) -> Result<FeatureStream>;

    /// Return an iterator over tile coordinates at the given `zoom` level.
    ///
    /// The data field of each returned [`RasterTile`] will be empty — actual
    /// pixel data is filled in by the driver crate.
    ///
    /// # Errors
    ///
    /// Returns [`OxiGdalError::NotSupported`] when called on a vector-only
    /// dataset.
    fn tiles(&self, zoom: u8) -> Result<TileStream>;
}

impl StreamingExt for OpenedDataset {
    fn features(&self) -> Result<FeatureStream> {
        match self {
            OpenedDataset::GeoJson(info) => stream_geojson_features(info),
            OpenedDataset::Shapefile(info) => stream_shapefile_features(info),
            OpenedDataset::FlatGeobuf(info) => stream_flatgeobuf_features(info),
            OpenedDataset::GeoPackage(info) => stream_geopackage_features_dispatch(info),
            OpenedDataset::GeoParquet(info) => stream_geoparquet_features_dispatch(info),
            OpenedDataset::Stac(info) => stream_stac_features_dispatch(info),
            OpenedDataset::Unknown(info) => {
                // Format detection failed at open time; nothing can be streamed.
                let path = info.path.as_deref().unwrap_or("<unknown>");
                tracing::warn!(
                    "OpenedDataset::Unknown — features() returns empty stream; \
                     format detection failed for '{path}'"
                );
                Ok(FeatureStream::empty())
            }
            other => Err(OxiGdalError::NotSupported {
                operation: format!(
                    "features() is not supported for raster format '{}'",
                    other.format().driver_name()
                ),
            }),
        }
    }

    fn tiles(&self, zoom: u8) -> Result<TileStream> {
        match self {
            OpenedDataset::GeoTiff(_)
            | OpenedDataset::Jpeg2000(_)
            | OpenedDataset::NetCdf(_)
            | OpenedDataset::Hdf5(_)
            | OpenedDataset::Zarr(_)
            | OpenedDataset::Grib(_)
            | OpenedDataset::Vrt(_)
            | OpenedDataset::Unknown(_) => Ok(TileStream::full_zoom(zoom)),
            other => Err(OxiGdalError::NotSupported {
                operation: format!(
                    "tiles() is not supported for vector format '{}'",
                    other.format().driver_name()
                ),
            }),
        }
    }
}

// ── Feature-gated streaming dispatch helpers ──────────────────────────────────

fn stream_geopackage_features_dispatch(info: &crate::DatasetInfo) -> Result<FeatureStream> {
    #[cfg(feature = "gpkg")]
    {
        crate::streaming_geopackage::stream_geopackage_features(info)
    }
    #[cfg(not(feature = "gpkg"))]
    {
        let _ = info;
        Ok(FeatureStream::empty())
    }
}

fn stream_geoparquet_features_dispatch(info: &crate::DatasetInfo) -> Result<FeatureStream> {
    #[cfg(feature = "geoparquet")]
    {
        crate::streaming_geoparquet::stream_geoparquet_features(info)
    }
    #[cfg(not(feature = "geoparquet"))]
    {
        let _ = info;
        Ok(FeatureStream::empty())
    }
}

fn stream_stac_features_dispatch(info: &crate::DatasetInfo) -> Result<FeatureStream> {
    #[cfg(feature = "stac")]
    {
        crate::streaming_stac::stream_stac_features(info)
    }
    #[cfg(not(feature = "stac"))]
    {
        let _ = info;
        Ok(FeatureStream::empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::open::open;
    use std::io::Write;

    fn make_temp_file(name: &str, content: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir();
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).expect("create");
        f.write_all(content).expect("write");
        path
    }

    // ── FeatureStream ─────────────────────────────────────────────────────────

    #[test]
    fn test_feature_stream_empty() {
        let stream = FeatureStream::empty();
        assert_eq!(stream.total_count(), 0);
        assert_eq!(stream.remaining(), 0);
    }

    #[test]
    fn test_feature_stream_from_vec_yields_all() {
        let features = vec![
            StreamingFeature::new(None, HashMap::new()),
            StreamingFeature::new(None, HashMap::new()),
            StreamingFeature::new(None, HashMap::new()),
        ];
        let mut stream = FeatureStream::from_vec(features);
        assert_eq!(stream.total_count(), 3);
        assert_eq!(stream.yielded_count(), 0);

        let first = stream.next().expect("has first").expect("no error");
        assert!(first.geometry.is_none());
        assert_eq!(stream.yielded_count(), 1);
        assert_eq!(stream.remaining(), 2);

        stream.next().expect("second").expect("no error");
        stream.next().expect("third").expect("no error");
        assert!(stream.next().is_none(), "stream exhausted");
    }

    #[test]
    fn test_feature_stream_with_properties() {
        let mut props = HashMap::new();
        props.insert("name".to_string(), JsonValue::String("Tokyo".to_string()));
        props.insert(
            "pop".to_string(),
            JsonValue::Number(serde_json::Number::from(9_273_000u64)),
        );

        let feature = StreamingFeature::new(None, props);
        assert_eq!(feature.properties["name"], "Tokyo");
        assert!(!feature.has_geometry());
        assert_eq!(feature.geometry_byte_len(), 0);
    }

    #[test]
    fn test_feature_stream_with_geometry() {
        // Minimal WKB point: byte order (1) + geometry type (1=Point) + x + y
        let wkb: Vec<u8> = vec![
            0x01, // little-endian
            0x01, 0x00, 0x00, 0x00, // WKBPoint
            0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x5E, 0x40, // x = 120.0
            0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x35, 0x40, // y = 35.0
        ];
        let feature = StreamingFeature::new(Some(wkb.clone()), HashMap::new());
        assert!(feature.has_geometry());
        assert_eq!(feature.geometry_byte_len(), wkb.len());
    }

    #[test]
    fn test_feature_with_id() {
        let feature = StreamingFeature::new(None, HashMap::new()).with_id("feature-001");
        assert_eq!(feature.id.as_deref(), Some("feature-001"));
    }

    #[test]
    fn test_feature_stream_size_hint() {
        let features = vec![
            StreamingFeature::new(None, HashMap::new()),
            StreamingFeature::new(None, HashMap::new()),
        ];
        let mut stream = FeatureStream::from_vec(features);
        assert_eq!(stream.size_hint(), (2, Some(2)));
        stream.next();
        assert_eq!(stream.size_hint(), (1, Some(1)));
    }

    // ── RasterTile ────────────────────────────────────────────────────────────

    #[test]
    fn test_raster_tile_tiles_per_axis() {
        assert_eq!(RasterTile::tiles_per_axis(0), 1);
        assert_eq!(RasterTile::tiles_per_axis(1), 2);
        assert_eq!(RasterTile::tiles_per_axis(8), 256);
        assert_eq!(RasterTile::tiles_per_axis(16), 65_536);
    }

    #[test]
    fn test_raster_tile_normalised_bbox_zoom0() {
        let tile = RasterTile {
            x: 0,
            y: 0,
            zoom: 0,
            data: vec![],
        };
        let (min_x, min_y, max_x, max_y) = tile.normalised_bbox();
        assert!((min_x - 0.0).abs() < 1e-9);
        assert!((min_y - 0.0).abs() < 1e-9);
        assert!((max_x - 1.0).abs() < 1e-9);
        assert!((max_y - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_raster_tile_normalised_bbox_zoom1() {
        let tile = RasterTile {
            x: 1,
            y: 0,
            zoom: 1,
            data: vec![],
        };
        let (min_x, _min_y, max_x, _max_y) = tile.normalised_bbox();
        assert!((min_x - 0.5).abs() < 1e-9);
        assert!((max_x - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_raster_tile_has_data() {
        let empty_tile = RasterTile {
            x: 0,
            y: 0,
            zoom: 1,
            data: vec![],
        };
        assert!(!empty_tile.has_data());

        let data_tile = RasterTile {
            x: 0,
            y: 0,
            zoom: 1,
            data: vec![0xFF],
        };
        assert!(data_tile.has_data());
    }

    // ── TileStream ────────────────────────────────────────────────────────────

    #[test]
    fn test_tile_stream_zoom0_yields_one_tile() {
        let mut stream = TileStream::full_zoom(0);
        assert_eq!(stream.zoom(), 0);
        let tile = stream.next().expect("has tile").expect("no error");
        assert_eq!((tile.x, tile.y, tile.zoom), (0, 0, 0));
        assert!(stream.next().is_none(), "only one tile at zoom 0");
    }

    #[test]
    fn test_tile_stream_zoom1_yields_four_tiles() {
        let stream = TileStream::full_zoom(1);
        let tiles: Vec<_> = stream.map(|t| t.expect("ok")).collect();
        assert_eq!(tiles.len(), 4, "2^1 × 2^1 = 4 tiles");
    }

    #[test]
    fn test_tile_stream_row_major_order() {
        let stream = TileStream::full_zoom(1);
        let tiles: Vec<_> = stream.map(|t| t.expect("ok")).collect();
        assert_eq!((tiles[0].x, tiles[0].y), (0, 0));
        assert_eq!((tiles[1].x, tiles[1].y), (1, 0));
        assert_eq!((tiles[2].x, tiles[2].y), (0, 1));
        assert_eq!((tiles[3].x, tiles[3].y), (1, 1));
    }

    #[test]
    fn test_tile_stream_zoom2_total() {
        let stream = TileStream::full_zoom(2);
        assert_eq!(stream.count(), 16, "2^2 × 2^2 = 16");
    }

    #[test]
    fn test_tile_stream_from_range_valid() {
        let stream = TileStream::from_range(3, (0, 2), (0, 2)).expect("valid range");
        let tiles: Vec<_> = stream.map(|t| t.expect("ok")).collect();
        assert_eq!(tiles.len(), 4, "2×2 sub-range");
    }

    #[test]
    fn test_tile_stream_from_range_out_of_bounds() {
        let result = TileStream::from_range(1, (0, 5), (0, 2));
        assert!(result.is_err(), "5 exceeds 2^1=2");
    }

    #[test]
    fn test_tile_stream_from_range_empty_range_error() {
        let result = TileStream::from_range(2, (1, 1), (0, 2));
        assert!(result.is_err(), "empty range start==end should fail");
    }

    #[test]
    fn test_tile_stream_yielded_count() {
        let mut stream = TileStream::full_zoom(1);
        assert_eq!(stream.yielded_count(), 0);
        stream.next();
        assert_eq!(stream.yielded_count(), 1);
        stream.next();
        assert_eq!(stream.yielded_count(), 2);
    }

    // ── StreamingExt on OpenedDataset ─────────────────────────────────────────

    #[test]
    fn test_streaming_ext_features_on_vector() {
        // `{}` has no top-level "type" field, so it is malformed GeoJSON (not
        // the documented "single Feature/Geometry" empty-stream fallback) and
        // now correctly errors — see `stream_geojson_features`. Use a valid
        // single Feature (non-FeatureCollection) to exercise the intended
        // "features() on GeoJSON should succeed" path.
        let path = make_temp_file(
            "stream_ext_geojson.geojson",
            br#"{"type":"Feature","geometry":null,"properties":{}}"#,
        );
        let ds = open(&path).expect("open");
        let stream_result = ds.features();
        assert!(
            stream_result.is_ok(),
            "features() on GeoJSON should succeed"
        );
    }

    #[test]
    fn test_streaming_ext_features_on_raster_errors() {
        // Write a minimal TIFF LE header
        let bytes = [0x49u8, 0x49, 0x2A, 0x00, 0x00, 0x00, 0x00, 0x00];
        let path = make_temp_file("stream_ext_tiff.tif", &bytes);
        let ds = open(&path).expect("open tiff");
        let result = ds.features();
        assert!(result.is_err(), "features() on raster dataset should error");
    }

    #[test]
    fn test_streaming_ext_tiles_on_raster() {
        let bytes = [0x49u8, 0x49, 0x2A, 0x00, 0x00, 0x00, 0x00, 0x00];
        let path = make_temp_file("stream_ext_tiles_tiff.tif", &bytes);
        let ds = open(&path).expect("open tiff");
        let result = ds.tiles(2);
        assert!(result.is_ok(), "tiles() on raster should succeed");
        let stream = result.expect("stream");
        assert_eq!(stream.zoom(), 2);
    }

    #[test]
    fn test_streaming_ext_tiles_on_vector_errors() {
        let path = make_temp_file("stream_ext_tiles_geojson.geojson", b"{}");
        let ds = open(&path).expect("open");
        let result = ds.tiles(2);
        assert!(result.is_err(), "tiles() on vector should error");
    }

    // ── integration: feature stream from opened dataset ───────────────────────

    #[test]
    fn test_feature_stream_collect_empty() {
        // A single Feature (not a FeatureCollection) is the documented
        // empty-stream fallback, so the reader returns an empty stream rather
        // than an error. `{}` is excluded: it has no top-level "type" field
        // and is malformed GeoJSON per `stream_geojson_features`, which now
        // surfaces it as an `Err` instead of silently swallowing it.
        let path = make_temp_file(
            "stream_collect_empty.geojson",
            br#"{"type":"Feature","geometry":null,"properties":{}}"#,
        );
        let ds = open(&path).expect("open");
        let features: Vec<_> = ds
            .features()
            .expect("features")
            .collect::<Result<Vec<_>>>()
            .expect("collect");
        assert_eq!(
            features.len(),
            0,
            "non-FeatureCollection GeoJSON returns no features"
        );
    }

    // ── GeoJSON real feature streaming ────────────────────────────────────────

    #[cfg(feature = "geojson")]
    #[test]
    fn test_geojson_streaming_feature_collection_count() {
        let content = br#"{"type":"FeatureCollection","features":[
            {"type":"Feature","geometry":{"type":"Point","coordinates":[139.7,35.7]},"properties":{"name":"Tokyo"}},
            {"type":"Feature","geometry":{"type":"Point","coordinates":[2.35,48.85]},"properties":{"name":"Paris"}}
        ]}"#;
        let path = make_temp_file("stream_fc_count.geojson", content);
        let ds = open(&path).expect("open");
        let features: Vec<_> = ds
            .features()
            .expect("features")
            .collect::<Result<Vec<_>>>()
            .expect("collect");
        assert_eq!(features.len(), 2, "should stream 2 features");
    }

    #[cfg(feature = "geojson")]
    #[test]
    fn test_geojson_streaming_properties_present() {
        let content = br#"{"type":"FeatureCollection","features":[
            {"type":"Feature","geometry":null,"properties":{"city":"Berlin","pop":3600000}}
        ]}"#;
        let path = make_temp_file("stream_props.geojson", content);
        let ds = open(&path).expect("open");
        let mut stream = ds.features().expect("features");
        let feat = stream.next().expect("first feature").expect("no error");
        assert_eq!(feat.properties["city"], serde_json::json!("Berlin"));
        assert_eq!(feat.properties["pop"], serde_json::json!(3600000));
    }

    #[cfg(feature = "geojson")]
    #[test]
    fn test_geojson_streaming_geometry_wkb() {
        let content = br#"{"type":"FeatureCollection","features":[
            {"type":"Feature","geometry":{"type":"Point","coordinates":[139.7,35.7]},"properties":{}}
        ]}"#;
        let path = make_temp_file("stream_wkb.geojson", content);
        let ds = open(&path).expect("open");
        let mut stream = ds.features().expect("features");
        let feat = stream.next().expect("first feature").expect("no error");
        assert!(
            feat.has_geometry(),
            "Point feature should have WKB geometry"
        );
        // WKB Point: 1 byte order + 4 type + 8 x + 8 y = 21 bytes
        assert_eq!(feat.geometry_byte_len(), 21, "WKB Point should be 21 bytes");
        // Verify byte-order flag (0x01 = LE) and geometry type (0x01000000 = Point)
        let wkb = feat.geometry.as_ref().expect("geometry");
        assert_eq!(wkb[0], 0x01, "little-endian byte-order flag");
        assert_eq!(&wkb[1..5], &1u32.to_le_bytes(), "WKB type = Point (1)");
    }

    #[cfg(feature = "geojson")]
    #[test]
    fn test_geojson_streaming_null_geometry_is_none() {
        let content = br#"{"type":"FeatureCollection","features":[
            {"type":"Feature","geometry":null,"properties":{"note":"no geom"}}
        ]}"#;
        let path = make_temp_file("stream_null_geom.geojson", content);
        let ds = open(&path).expect("open");
        let mut stream = ds.features().expect("features");
        let feat = stream.next().expect("first feature").expect("no error");
        assert!(!feat.has_geometry(), "null geometry should produce None");
    }

    #[cfg(feature = "geojson")]
    #[test]
    fn test_geojson_streaming_feature_id() {
        let content = br#"{"type":"FeatureCollection","features":[
            {"type":"Feature","id":"feat-001","geometry":null,"properties":{}}
        ]}"#;
        let path = make_temp_file("stream_id.geojson", content);
        let ds = open(&path).expect("open");
        let mut stream = ds.features().expect("features");
        let feat = stream.next().expect("first feature").expect("no error");
        assert_eq!(feat.id.as_deref(), Some("feat-001"));
    }

    #[test]
    fn test_tile_stream_all_coordinates_in_range() {
        let zoom = 3u8;
        let dim = RasterTile::tiles_per_axis(zoom);
        let stream = TileStream::full_zoom(zoom);
        for tile_result in stream {
            let tile = tile_result.expect("ok");
            assert!(tile.x < dim, "x={} should be < {dim}", tile.x);
            assert!(tile.y < dim, "y={} should be < {dim}", tile.y);
        }
    }
}
