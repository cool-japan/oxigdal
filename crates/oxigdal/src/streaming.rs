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
            OpenedDataset::GeoJson(_)
            | OpenedDataset::Shapefile(_)
            | OpenedDataset::GeoPackage(_)
            | OpenedDataset::GeoParquet(_)
            | OpenedDataset::FlatGeobuf(_)
            | OpenedDataset::Stac(_)
            | OpenedDataset::Unknown(_) => {
                // Return empty stream — real features loaded by driver
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
        let path = make_temp_file("stream_ext_geojson.geojson", b"{}");
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
        let path = make_temp_file("stream_collect_empty.geojson", b"{}");
        let ds = open(&path).expect("open");
        let features: Vec<_> = ds
            .features()
            .expect("features")
            .collect::<Result<Vec<_>>>()
            .expect("collect");
        assert_eq!(features.len(), 0, "empty driver stub returns no features");
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
