//! # OxiGeo — Pure Rust Geospatial Data Abstraction Library
//!
//! OxiGeo is the Rust-native alternative to [GDAL](https://gdal.org/),
//! providing a comprehensive geospatial data abstraction layer
//! with **zero C/Fortran dependencies**. 100% Pure Rust.
//!
//! ## Quick Start
//!
//! ```toml
//! [dependencies]
//! oxigeo = "0.2"  # includes GeoTIFF, GeoJSON, Shapefile by default
//! ```
//!
//! ```rust
//! use oxigeo::Dataset;
//!
//! # fn main() -> oxigeo::Result<()> {
//! let drivers = oxigeo::drivers();
//! println!("Enabled drivers: {:?}", drivers);
//! println!("OxiGeo version: {}", oxigeo::version());
//! # Ok(())
//! # }
//! ```
//!
//! ## Reading raster pixels — the fast path
//!
//! The idiomatic way to get a band's pixels into memory is to allocate the
//! destination **once** and let the driver decode straight into it, converting
//! the element type on the way.  This is the equivalent of GDAL's
//! `RasterBand::read_into_slice`, and it is what [`Dataset::read_band_into`]
//! does:
//!
//! ```rust,no_run
//! use oxigeo::Dataset;
//!
//! # fn main() -> oxigeo::Result<()> {
//! let ds = Dataset::open("dem.tif")?;
//!
//! // 1. The on-disk element type comes from the header — no pixel is read yet,
//! //    so the destination can be sized and typed before any I/O.
//! println!("on-disk type: {:?}", ds.data_type());   // e.g. Some(Float32)
//!
//! // 2. One allocation, of the type you actually want to compute in.
//! let (width, height) = (ds.width() as usize, ds.height() as usize);
//! let mut dem = vec![0.0f64; width * height];
//!
//! // 3. Decode + Float32 → f64 conversion, fused into a single pass.
//! ds.read_band_into(0, &mut dem)?;
//! # Ok(())
//! # }
//! ```
//!
//! `dem` is row-major, so it maps onto an
//! [`ndarray::Array2`](https://docs.rs/ndarray/latest/ndarray/type.Array2.html)
//! with no copy — or you can decode directly into an array you already own:
//!
//! ```rust,ignore
//! use ndarray::Array2;
//!
//! let mut grid = Array2::<f64>::zeros((height, width));
//! ds.read_band_into(0, grid.as_slice_mut().expect("standard layout"))?;
//! ```
//!
//! ### What each reader costs
//!
//! | Method | Bands | Allocates | Reads | Converts |
//! |---|---|---|---|---|
//! | [`Dataset::read_band`] | one | one `RasterBuffer` | every block of the band (or clip window) | no |
//! | [`Dataset::read_window`] | one | one `RasterBuffer` | only the blocks the window overlaps | no |
//! | [`Dataset::read_band_into`] | one | nothing — you own `dst` | every block of the band (or clip window) | yes, fused |
//! | [`Dataset::read_window_into`] | one | nothing — you own `dst` | only the blocks the window overlaps | yes, fused |
//! | [`Dataset::read_interleaved`] | many | one `Vec<T>` + fixed scratch | every block once, whatever the band count | yes, fused |
//! | [`Dataset::read_interleaved_into`] | many | fixed scratch — you own `dst` | every block once, whatever the band count | yes, fused |
//! | [`Dataset::read_window_interleaved`] | many | one `Vec<T>` + fixed scratch | only the blocks the window overlaps, once each | yes, fused |
//! | [`Dataset::read_window_interleaved_into`] | many | fixed scratch — you own `dst` | only the blocks the window overlaps, once each | yes, fused |
//!
//! The `*_into` readers keep peak extra memory at one tile/strip no matter how
//! large the raster is, which also makes them the right primitive for walking a
//! big file window by window with a reusable buffer.  The interleaved readers
//! hold to the same bound — their scratch is sized by the file's blocks and the
//! band count, never by the raster — and ask for all the bands together, so a
//! chunky file's blocks are decompressed once rather than once per band.  On a
//! dataset returned by [`Dataset::clip`] every reader works in the clipped pixel
//! grid and reads only the clipped blocks.
//!
//! ### Several bands at once
//!
//! [`Dataset::read_band`] returns **one** band — as of 0.2.2, where it used to
//! return the whole pixel-interleaved image no matter which band was asked for.
//! When you want the interleaved image, ask for it:
//!
//! ```rust,no_run
//! use oxigeo::Dataset;
//!
//! # fn main() -> oxigeo::Result<()> {
//! let ds = Dataset::open("scene.tif")?;
//! let (width, height) = (ds.width() as usize, ds.height() as usize);
//!
//! // `None` = every band in file order; `Some(&[..])` picks and orders them.
//! let mut rgb = vec![0u8; width * height * 3];
//! ds.read_interleaved_into(Some(&[2, 1, 0]), &mut rgb)?;
//! # Ok(())
//! # }
//! ```
//!
//! ### Turn on `parallel` for large rasters
//!
//! Block decoding is single-threaded by default (the crate is also compiled to
//! `wasm32`, which has no OS threads).  Enable the `parallel` feature to spread
//! the per-tile decode — and the fused element conversion with it — across rayon
//! workers:
//!
//! ```toml
//! oxigeo = { version = "0.2", features = ["parallel"] }
//! ```
//!
//! The result is bit-identical to the serial path; on a multi-megapixel DEM it
//! is what makes [`Dataset::read_band_into`] beat the decode-then-convert
//! workaround outright rather than merely halving its memory.
//!
//! ## Reading vector features
//!
//! Vector datasets are read through **layers**, the same way GDAL's OGR side
//! works: [`Dataset::layers`] enumerates them, and [`Layer::features`] yields
//! [`Feature`]s carrying a [`Geometry`] and a map of attribute [`FieldValue`]s.
//!
//! ```rust,no_run
//! use oxigeo::Dataset;
//!
//! # fn main() -> oxigeo::Result<()> {
//! let ds = Dataset::open("cities.gpkg")?;   // or .shp, or .geojson
//! println!("{} layer(s)", ds.layer_count());
//!
//! let layer = ds.layer(0)?;                 // also: ds.layer_by_name("cities")
//! println!("{}: {:?}, {:?} features, fields {:?}",
//!          layer.name(), layer.geometry_type(), layer.feature_count(),
//!          layer.field_names());
//!
//! for feature in layer.features()? {
//!     println!("{:?} — {:?}", feature.geometry, feature.properties);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! Layer reading is implemented for **ESRI Shapefile** and **GeoJSON** (both
//! default features) and for **GeoPackage** (feature `gpkg`, *not* on by
//! default — `oxigeo = { version = "0.2", features = ["gpkg"] }`).  Any other
//! format returns [`OxiGeoError::NotSupported`] naming the driver rather than
//! silently reporting zero layers.  For the remaining vector formats
//! (FlatGeobuf, GeoParquet, STAC) the streaming API
//! ([`streaming::StreamingExt`]) yields features with WKB geometry and JSON
//! properties.
//!
//! ## Feature Flags
//!
//! | Feature | Default | Description |
//! |---------|---------|-------------|
//! | `geotiff` | ✅ | GeoTIFF raster format (COG support) |
//! | `geojson` | ✅ | GeoJSON vector format |
//! | `shapefile` | ✅ | ESRI Shapefile |
//! | `geoparquet` | ❌ | GeoParquet (Apache Arrow columnar) |
//! | `netcdf` | ❌ | NetCDF scientific data format |
//! | `hdf5` | ❌ | HDF5 hierarchical data format |
//! | `zarr` | ❌ | Zarr cloud-native arrays |
//! | `grib` | ❌ | GRIB meteorological data format |
//! | `stac` | ❌ | SpatioTemporal Asset Catalog |
//! | `terrain` | ❌ | Terrain/elevation data |
//! | `vrt` | ❌ | Virtual Raster Tiles |
//! | `flatgeobuf` | ❌ | FlatGeobuf vector format |
//! | `jpeg2000` | ❌ | JPEG2000 raster format |
//! | `gpkg` | ❌ | GeoPackage (SQLite) — vector layers and tiles |
//! | `pmtiles` | ❌ | PMTiles v3 tile archive |
//! | `mbtiles` | ❌ | MBTiles tile archive |
//! | `copc` | ❌ | COPC / LAS / LAZ point clouds |
//! | `index` | ❌ | Spatial indexing (R-tree, grid) |
//! | `full` | ❌ | **All formats above** |
//! | `parallel` | ❌ | Multi-threaded (rayon) block decoding for raster reads |
//! | `cloud` | ❌ | Cloud storage (S3, GCS, Azure) |
//! | `proj` | ❌ | CRS transformations (Pure Rust proj) |
//! | `algorithms` | ❌ | Raster/vector algorithms |
//! | `analytics` | ❌ | Geospatial analytics |
//! | `streaming` | ❌ | Stream processing |
//! | `ml` | ❌ | Machine learning integration |
//! | `gpu` | ❌ | GPU-accelerated processing |
//! | `server` | ❌ | OGC-compliant tile server |
//! | `temporal` | ❌ | Temporal/time-series analysis |
//!
//! ## GDAL Compatibility
//!
//! OxiGeo aims to provide familiar concepts for GDAL users:
//!
//! | GDAL (C/C++) | OxiGeo (Rust) |
//! |---|---|
//! | `GDALOpen()` | [`Dataset::open()`] |
//! | `GDALGetRasterBand()` | `dataset.raster_band(n)` |
//! | `GDALDatasetGetLayerCount()` | [`Dataset::layer_count()`] |
//! | `GDALDatasetGetLayer()` | [`Dataset::layer()`] / [`Dataset::layer_by_name()`] |
//! | `OGRLayer::GetNextFeature()` | [`Layer::features()`] |
//! | `OGR_F_GetFieldAsString()` | `feature.properties.get(name)` |
//! | `GDALGetGeoTransform()` | [`Dataset::geotransform()`] |
//! | `GDALGetProjectionRef()` | [`Dataset::crs()`] |
//! | `GDALAllRegister()` | [`drivers()`] |
//! | `GDALVersionInfo()` | [`version()`] |
//! | `GDALWarp()` | `oxigeo::algorithms::warp()` (feature `algorithms`) |
//! | `ogr2ogr` | `oxigeo-cli convert` (crate `oxigeo-cli`) |
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────────────┐
//! │  oxigeo (this crate) — Unified API              │
//! │  Dataset::open() → auto-detect format            │
//! ├──────────────────────────────────────────────────┤
//! │  Drivers (feature-gated)                         │
//! │  ┌──────────┐ ┌──────────┐ ┌─────────────┐      │
//! │  │ GeoTIFF  │ │ GeoJSON  │ │  Shapefile  │ ...  │
//! │  └──────────┘ └──────────┘ └─────────────┘      │
//! ├──────────────────────────────────────────────────┤
//! │  oxigeo-core — Types, Buffers, Error, I/O       │
//! └──────────────────────────────────────────────────┘
//! ```
//!
//! ## Crate Ecosystem
//!
//! OxiGeo is a workspace of 65+ crates. This `oxigeo` crate serves as
//! the **unified entry point**. Individual crates can also be used directly:
//!
//! ```toml
//! # Use the unified API (recommended for most users)
//! oxigeo = { version = "0.2", features = ["full", "cloud", "proj"] }
//!
//! # Or pick individual crates for minimal dependencies
//! oxigeo-core = "0.2"
//! oxigeo-geotiff = "0.2"
//! ```
//!
//! ## Pure Rust — No C/Fortran Dependencies
//!
//! Unlike the original GDAL which requires C/C++ compilation and system
//! libraries (PROJ, GEOS, etc.), OxiGeo is **100% Pure Rust**:
//!
//! - No `bindgen`, no `cc`, no `cmake`
//! - Cross-compiles to WASM, embedded, mobile
//! - `cargo add oxigeo` — that's it
//!
//! Part of the [COOLJAPAN](https://github.com/cool-japan) ecosystem.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

// Re-export core types — always available
pub use oxigeo_core::error::OxiGeoError;
pub use oxigeo_core::error::Result;
pub use oxigeo_core::types::{BoundingBox, GeoTransform, RasterDataType, RasterMetadata};

/// Vector feature model — the currency of the layer API ([`Dataset::layers`],
/// [`Layer::features`]).
///
/// A [`Feature`] pairs an optional [`Geometry`] with a map of attribute
/// [`FieldValue`]s.  Re-exported here so that reading features never requires a
/// direct dependency on `oxigeo-core`.
pub use oxigeo_core::vector::{Feature, FeatureId, FieldValue, Geometry};

/// Element types a raster can be read into — the bound on
/// [`Dataset::read_band_into`] and [`Dataset::read_window_into`].
///
/// Implemented for `u8`, `i8`, `u16`, `i16`, `u32`, `i32`, `u64`, `i64`, `f32`
/// and `f64`, and sealed: it is a description of the ten primitive sample types,
/// not an extension point.  Re-exported here so a `T: RasterElement` bound can
/// be written without depending on `oxigeo-core` directly.
pub use oxigeo_core::buffer::RasterElement;

// ─── Ergonomic API modules ───────────────────────────────────────────────────

/// Universal dataset opener with automatic format detection.
pub mod open;

/// Builder patterns for dataset creation and opening.
pub mod builder;

/// Streaming / iterator API for large datasets.
pub mod streaming;

/// GeoPackage feature streaming helper (feature-gated: `gpkg`).
#[cfg(feature = "gpkg")]
pub(crate) mod streaming_geopackage;

/// GeoPackage table-schema parsing shared by the layer and streaming readers
/// (feature-gated: `gpkg`).
#[cfg(feature = "gpkg")]
pub(crate) mod gpkg_schema;

/// Vector layer access: [`Dataset::layers`], [`Layer`] and its features.
pub mod layer;
pub use layer::{Layer, LayerFeatures};

/// GeoParquet feature streaming helper (feature-gated: `geoparquet`).
#[cfg(feature = "geoparquet")]
pub(crate) mod streaming_geoparquet;

/// STAC feature streaming helper (feature-gated: `stac`).
#[cfg(feature = "stac")]
pub(crate) mod streaming_stac;

/// Format conversion planning and detection utilities.
pub mod convert;

/// Magic-byte signatures for binary format detection.
pub(crate) mod magic;

/// Cloud URI detection and transparent dispatch for `Dataset::open`.
pub mod cloud_detect;
pub use cloud_detect::is_cloud_uri;

/// Virtual Raster construction from multiple source datasets.
pub mod vrt_builder;

/// Dataset format detection enum and helpers.
mod format;
pub use format::DatasetFormat;

/// Format-agnostic dataset metadata descriptor.
mod dataset_info;
pub use dataset_info::DatasetInfo;

/// Raster pixel readers: full-band, windowed, and band iteration.
mod raster_read;
pub use raster_read::BandIter;
pub(crate) use raster_read::PixelWindow;
#[cfg(feature = "geotiff")]
pub(crate) use raster_read::crop_interleaved;

/// Per-band raster statistics.
mod band_stats;
pub use band_stats::BandStatistics;

/// Spatial operations on `Dataset`: clip and reproject.
mod dataset_ops;

/// Options accepted by `Dataset::convert`.
mod convert_options;
pub use convert_options::{Compression, ConversionOptions};

/// Format-conversion implementation for `Dataset::convert`.
mod convert_ops;

/// GDAL C API compatibility shim.
#[cfg(feature = "gdal-compat")]
#[cfg_attr(docsrs, doc(cfg(feature = "gdal-compat")))]
#[doc(hidden)]
pub mod gdal_compat;

pub use builder::{
    CompressionType, CreateOptions, DatasetCreateBuilder, DatasetOpenBuilder, DatasetWriter,
    OutputFormat,
};
pub use open::{CloudScheme, OpenedDataset, open};
pub use streaming::{FeatureStream, RasterTile, StreamingExt, StreamingFeature, TileStream};

/// Re-export the core crate for advanced usage
pub use oxigeo_core as core_types;

// ─── Driver re-exports (feature-gated) ──────────────────────────────────────

/// GeoTIFF raster driver (Cloud-Optimized GeoTIFF support)
#[cfg(feature = "geotiff")]
#[cfg_attr(docsrs, doc(cfg(feature = "geotiff")))]
pub use oxigeo_geotiff as geotiff;

/// GeoJSON vector driver
#[cfg(feature = "geojson")]
#[cfg_attr(docsrs, doc(cfg(feature = "geojson")))]
pub use oxigeo_geojson as geojson;

/// ESRI Shapefile driver
#[cfg(feature = "shapefile")]
#[cfg_attr(docsrs, doc(cfg(feature = "shapefile")))]
pub use oxigeo_shapefile as shapefile;

/// GeoParquet columnar format driver
#[cfg(feature = "geoparquet")]
#[cfg_attr(docsrs, doc(cfg(feature = "geoparquet")))]
pub use oxigeo_geoparquet as geoparquet;

/// NetCDF scientific format driver
#[cfg(feature = "netcdf")]
#[cfg_attr(docsrs, doc(cfg(feature = "netcdf")))]
pub use oxigeo_netcdf as netcdf;

/// HDF5 hierarchical data driver
#[cfg(feature = "hdf5")]
#[cfg_attr(docsrs, doc(cfg(feature = "hdf5")))]
pub use oxigeo_hdf5 as hdf5;

/// Zarr cloud-native array driver
#[cfg(feature = "zarr")]
#[cfg_attr(docsrs, doc(cfg(feature = "zarr")))]
pub use oxigeo_zarr as zarr;

/// GRIB meteorological data driver
#[cfg(feature = "grib")]
#[cfg_attr(docsrs, doc(cfg(feature = "grib")))]
pub use oxigeo_grib as grib;

/// SpatioTemporal Asset Catalog driver
#[cfg(feature = "stac")]
#[cfg_attr(docsrs, doc(cfg(feature = "stac")))]
pub use oxigeo_stac as stac;

/// Terrain/elevation data driver
#[cfg(feature = "terrain")]
#[cfg_attr(docsrs, doc(cfg(feature = "terrain")))]
pub use oxigeo_terrain as terrain;

/// Virtual Raster Tiles driver
#[cfg(feature = "vrt")]
#[cfg_attr(docsrs, doc(cfg(feature = "vrt")))]
pub use oxigeo_vrt as vrt;

/// FlatGeobuf vector format driver
#[cfg(feature = "flatgeobuf")]
#[cfg_attr(docsrs, doc(cfg(feature = "flatgeobuf")))]
pub use oxigeo_flatgeobuf as flatgeobuf;

/// JPEG2000 raster format driver
#[cfg(feature = "jpeg2000")]
#[cfg_attr(docsrs, doc(cfg(feature = "jpeg2000")))]
pub use oxigeo_jpeg2000 as jpeg2000;

// ─── Advanced capability re-exports (feature-gated) ─────────────────────────

/// Cloud storage backends (S3, GCS, Azure Blob)
#[cfg(feature = "cloud")]
#[cfg_attr(docsrs, doc(cfg(feature = "cloud")))]
pub use oxigeo_cloud as cloud;

/// Coordinate reference system transformations (Pure Rust proj)
#[cfg(feature = "proj")]
#[cfg_attr(docsrs, doc(cfg(feature = "proj")))]
pub use oxigeo_proj as proj;

/// Raster and vector algorithms (resampling, reprojection, etc.)
#[cfg(feature = "algorithms")]
#[cfg_attr(docsrs, doc(cfg(feature = "algorithms")))]
pub use oxigeo_algorithms as algorithms;

/// Geospatial analytics and statistics
#[cfg(feature = "analytics")]
#[cfg_attr(docsrs, doc(cfg(feature = "analytics")))]
pub use oxigeo_analytics as analytics;

/// Stream processing for large datasets (advanced streaming crate)
#[cfg(feature = "streaming")]
#[cfg_attr(docsrs, doc(cfg(feature = "streaming")))]
pub use oxigeo_streaming as streaming_ext;

/// Machine learning integration
#[cfg(feature = "ml")]
#[cfg_attr(docsrs, doc(cfg(feature = "ml")))]
pub use oxigeo_ml as ml;

/// GPU-accelerated geospatial processing
#[cfg(feature = "gpu")]
#[cfg_attr(docsrs, doc(cfg(feature = "gpu")))]
pub use oxigeo_gpu as gpu;

/// OGC-compliant geospatial tile/feature server
#[cfg(feature = "server")]
#[cfg_attr(docsrs, doc(cfg(feature = "server")))]
pub use oxigeo_server as server;

/// Temporal/time-series geospatial analysis
#[cfg(feature = "temporal")]
#[cfg_attr(docsrs, doc(cfg(feature = "temporal")))]
pub use oxigeo_temporal as temporal;

// ─── Tile / database / point-cloud re-exports (feature-gated) ───────────────

/// GeoPackage (SQLite-based) driver
#[cfg(feature = "gpkg")]
#[cfg_attr(docsrs, doc(cfg(feature = "gpkg")))]
pub use oxigeo_gpkg as gpkg;

/// PMTiles v3 tile archive driver
#[cfg(feature = "pmtiles")]
#[cfg_attr(docsrs, doc(cfg(feature = "pmtiles")))]
pub use oxigeo_pmtiles as pmtiles;

/// MBTiles tile archive driver
#[cfg(feature = "mbtiles")]
#[cfg_attr(docsrs, doc(cfg(feature = "mbtiles")))]
pub use oxigeo_mbtiles as mbtiles;

/// COPC (Cloud Optimized Point Cloud) driver
#[cfg(feature = "copc")]
#[cfg_attr(docsrs, doc(cfg(feature = "copc")))]
pub use oxigeo_copc as copc;

/// Spatial index (R-tree, grid) module
#[cfg(feature = "index")]
#[cfg_attr(docsrs, doc(cfg(feature = "index")))]
pub use oxigeo_index as index;

/// no_std fixed-size geometry primitives
#[cfg(feature = "noalloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "noalloc")))]
pub use oxigeo_noalloc as noalloc;

/// OGC Web Services (WFS, WCS, WPS, CSW)
#[cfg(feature = "services")]
#[cfg_attr(docsrs, doc(cfg(feature = "services")))]
pub use oxigeo_services as services;

// ─── Unified Dataset API ────────────────────────────────────────────────────

/// Unified dataset handle — the central abstraction (analogous to `GDALDataset`).
///
/// Opens any supported geospatial format and provides uniform access
/// to raster bands, vector layers, and metadata.
///
/// # Example
///
/// ```rust,no_run
/// use oxigeo::Dataset;
///
/// let ds = Dataset::open("elevation.tif").expect("failed to open");
/// println!("{}×{} pixels, {} bands", ds.width(), ds.height(), ds.band_count());
/// println!("Format: {}", ds.format());
/// if let Some(crs) = ds.crs() {
///     println!("CRS: {crs}");
/// }
/// ```
pub struct Dataset {
    path: String,
    info: DatasetInfo,
    /// Optional pixel window that constrains every real raster read of this
    /// dataset to a sub-rectangle of the on-disk file.
    ///
    /// Populated by [`Dataset::clip`] for raster datasets so that subsequent
    /// operations that re-open the underlying file ([`Dataset::read_band`],
    /// [`Dataset::statistics`], [`Dataset::convert`]) honour the clip instead of
    /// silently reprocessing the full raster. `None` means "read the whole
    /// file" (the default for freshly-opened datasets).
    clip_window: Option<PixelWindow>,
}

impl Dataset {
    /// Open a geospatial dataset from a file path — the universal entry point.
    ///
    /// Format is auto-detected from file extension (and in the future, magic bytes),
    /// just like `GDALOpen()` in C GDAL.
    ///
    /// # Supported Formats
    ///
    /// Which formats are available depends on enabled feature flags.
    /// With default features: GeoTIFF, GeoJSON, Shapefile.
    ///
    /// # Errors
    ///
    /// Returns [`OxiGeoError::NotSupported`] if the format is not recognized
    /// or the corresponding feature flag is not enabled.
    ///
    /// Returns [`OxiGeoError::Io`] if the file cannot be read.
    pub fn open(path: &str) -> Result<Self> {
        // Cloud URIs bypass the local file-detection path.
        if crate::cloud_detect::is_cloud_uri(path) {
            return crate::cloud_detect::open_cloud_dataset(path);
        }

        // Try magic-byte detection first for local files, fall back to extension.
        let p = std::path::Path::new(path);
        let format = if p.exists() {
            DatasetFormat::detect(p).unwrap_or_else(|_| DatasetFormat::from_extension(path))
        } else {
            DatasetFormat::from_extension(path)
        };
        Self::open_with_format(path, format)
    }

    /// Open a dataset with an explicitly specified format.
    ///
    /// Use this when auto-detection from extension is insufficient
    /// (e.g., `.json` files that could be GeoJSON or STAC).
    ///
    /// # Errors
    ///
    /// Returns error if the format's feature flag is not enabled or file is unreadable.
    pub fn open_with_format(path: &str, format: DatasetFormat) -> Result<Self> {
        match format {
            #[cfg(feature = "geotiff")]
            DatasetFormat::GeoTiff => Self::open_raster(path, DatasetFormat::GeoTiff),

            #[cfg(feature = "geojson")]
            DatasetFormat::GeoJson => Self::open_vector(path, DatasetFormat::GeoJson),

            #[cfg(feature = "shapefile")]
            DatasetFormat::Shapefile => Self::open_vector(path, DatasetFormat::Shapefile),

            #[cfg(feature = "geoparquet")]
            DatasetFormat::GeoParquet => Self::open_vector(path, DatasetFormat::GeoParquet),

            #[cfg(feature = "netcdf")]
            DatasetFormat::NetCdf => Self::open_raster(path, DatasetFormat::NetCdf),

            #[cfg(feature = "hdf5")]
            DatasetFormat::Hdf5 => Self::open_raster(path, DatasetFormat::Hdf5),

            #[cfg(feature = "zarr")]
            DatasetFormat::Zarr => Self::open_raster(path, DatasetFormat::Zarr),

            #[cfg(feature = "grib")]
            DatasetFormat::Grib => Self::open_raster(path, DatasetFormat::Grib),

            #[cfg(feature = "flatgeobuf")]
            DatasetFormat::FlatGeobuf => Self::open_vector(path, DatasetFormat::FlatGeobuf),

            #[cfg(feature = "jpeg2000")]
            DatasetFormat::Jpeg2000 => Self::open_raster(path, DatasetFormat::Jpeg2000),

            #[cfg(feature = "vrt")]
            DatasetFormat::Vrt => Self::open_raster(path, DatasetFormat::Vrt),

            #[cfg(feature = "gpkg")]
            DatasetFormat::GeoPackage => Self::open_vector(path, DatasetFormat::GeoPackage),

            #[cfg(feature = "pmtiles")]
            DatasetFormat::PMTiles => Self::open_raster(path, DatasetFormat::PMTiles),

            #[cfg(feature = "mbtiles")]
            DatasetFormat::MBTiles => Self::open_raster(path, DatasetFormat::MBTiles),

            #[cfg(feature = "copc")]
            DatasetFormat::Copc => Self::open_raster(path, DatasetFormat::Copc),

            // Plain LAS/LAZ point clouds route through the same point-cloud
            // reader as COPC (a COPC reader is a superset of a LAS reader).
            #[cfg(feature = "copc")]
            DatasetFormat::Las => Self::open_raster(path, DatasetFormat::Las),

            _ => Err(OxiGeoError::NotSupported {
                operation: format!(
                    "Format '{}' for '{}' — enable the corresponding feature flag or check the file extension",
                    format.driver_name(),
                    path,
                ),
            }),
        }
    }

    // -- Real openers: delegate header parsing to open.rs helpers -------------

    /// Open a raster dataset.
    ///
    /// Compiled only when at least one raster driver feature is enabled — with
    /// none of them, `open_with_format` has no arm that routes here.
    #[cfg(any(
        feature = "geotiff",
        feature = "netcdf",
        feature = "hdf5",
        feature = "zarr",
        feature = "grib",
        feature = "jpeg2000",
        feature = "vrt",
        feature = "pmtiles",
        feature = "mbtiles",
        feature = "copc",
    ))]
    fn open_raster(path: &str, format: DatasetFormat) -> Result<Self> {
        let p = std::path::Path::new(path);
        if !p.exists() {
            return Err(OxiGeoError::Io(oxigeo_core::error::IoError::NotFound {
                path: path.to_string(),
            }));
        }

        // For GeoTIFF, parse the real TIFF header for width / height /
        // band_count / data_type / geotransform / CRS.  A parse failure is
        // propagated as a typed error — reporting a zero-filled `DatasetInfo`
        // for a file that is really a raster is a silent-corruption bug, not a
        // graceful degradation (cool-japan/oxigeo#14).  Other raster formats
        // have no header probe yet and honestly report `None` everywhere.
        let mut info = match format {
            DatasetFormat::GeoTiff => crate::open::extract_tiff_info(p)?,
            #[cfg(feature = "vrt")]
            DatasetFormat::Vrt => crate::open::extract_vrt_info(p)?,
            _ => DatasetInfo {
                format,
                path: Some(path.to_string()),
                ..DatasetInfo::default()
            },
        };

        // Ensure path is populated even when extracted from helper
        info.path = Some(path.to_string());

        Ok(Self {
            path: path.to_string(),
            info,
            clip_window: None,
        })
    }

    /// Open a vector dataset.
    ///
    /// Compiled only when at least one vector driver feature is enabled — with
    /// none of them, `open_with_format` has no arm that routes here.
    #[cfg(any(
        feature = "geojson",
        feature = "shapefile",
        feature = "geoparquet",
        feature = "flatgeobuf",
        feature = "gpkg",
    ))]
    fn open_vector(path: &str, format: DatasetFormat) -> Result<Self> {
        let p = std::path::Path::new(path);
        if !p.exists() {
            return Err(OxiGeoError::Io(oxigeo_core::error::IoError::NotFound {
                path: path.to_string(),
            }));
        }

        // As for rasters, a probe failure is propagated rather than collapsed
        // into an empty descriptor: a corrupt `.shp` reporting
        // `feature_count = None, bounds = None` is indistinguishable from a
        // valid-but-sparse layer, which hides the real problem from the caller.
        let mut info = match format {
            DatasetFormat::GeoJson => crate::open::extract_geojson_info(p)?,
            #[cfg(feature = "shapefile")]
            DatasetFormat::Shapefile => crate::open::extract_shapefile_info(p)?,
            #[cfg(feature = "flatgeobuf")]
            DatasetFormat::FlatGeobuf => crate::open::extract_flatgeobuf_info(p)?,
            #[cfg(feature = "geoparquet")]
            DatasetFormat::GeoParquet => crate::open::extract_geoparquet_info(p)?,
            #[cfg(feature = "gpkg")]
            DatasetFormat::GeoPackage => crate::open::extract_gpkg_info(p)?,
            _ => DatasetInfo {
                format,
                path: Some(path.to_string()),
                ..DatasetInfo::default()
            },
        };

        // Ensure path is populated even when extracted from helper
        info.path = Some(path.to_string());

        Ok(Self {
            path: path.to_string(),
            info,
            clip_window: None,
        })
    }

    // -- Constructors from pre-built info ------------------------------------------

    /// Construct a `Dataset` directly from a path and a pre-parsed [`DatasetInfo`].
    ///
    /// Used internally by the cloud-detect module and by tests that need to
    /// create a `Dataset` without a file on disk.
    pub(crate) fn from_info(path: String, info: DatasetInfo) -> Self {
        Self {
            path,
            info,
            clip_window: None,
        }
    }

    /// Construct a `Dataset` carrying a pixel-space clip window that every real
    /// raster read must honour.  Used by [`Dataset::clip`].
    pub(crate) fn from_info_with_window(
        path: String,
        info: DatasetInfo,
        clip_window: Option<PixelWindow>,
    ) -> Self {
        Self {
            path,
            info,
            clip_window,
        }
    }

    /// The pixel window constraining reads of this dataset, if any.
    pub(crate) fn clip_window(&self) -> Option<PixelWindow> {
        self.clip_window
    }

    // -- Accessors (GDAL-like API) ------------------------------------------

    /// File path this dataset was opened from.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Detected dataset format.
    pub fn format(&self) -> DatasetFormat {
        self.info.format
    }

    /// Full dataset info.
    pub fn info(&self) -> &DatasetInfo {
        &self.info
    }

    /// Width in pixels (raster datasets). Returns 0 for vector-only datasets.
    pub fn width(&self) -> u32 {
        self.info.width.unwrap_or(0)
    }

    /// Height in pixels (raster datasets). Returns 0 for vector-only datasets.
    pub fn height(&self) -> u32 {
        self.info.height.unwrap_or(0)
    }

    /// Coordinate reference system (WKT, EPSG code, or PROJ string).
    pub fn crs(&self) -> Option<&str> {
        self.info.crs.as_deref()
    }

    /// Number of raster bands.
    pub fn band_count(&self) -> u32 {
        self.info.band_count
    }

    /// Element type of this dataset's raster bands, as declared by the file
    /// header — analogous to `GDALGetRasterDataType()`.
    ///
    /// Returns `None` for vector datasets (no pixels), and for raster formats
    /// whose header probe is not implemented yet.
    ///
    /// This is resolved at open time from the header alone, so it is available
    /// *before* any pixel is read — which is what makes it possible to size a
    /// destination buffer up front instead of reading a whole band just to
    /// discover its type.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use oxigeo::{Dataset, RasterDataType};
    ///
    /// # fn main() -> oxigeo::Result<()> {
    /// let ds = Dataset::open("elevation.tif")?;
    /// let dt = ds.data_type().unwrap_or(RasterDataType::UInt8);
    /// let bytes_needed =
    ///     ds.width() as usize * ds.height() as usize * dt.size_bytes();
    /// println!("band 0 needs {bytes_needed} bytes");
    /// # Ok(())
    /// # }
    /// ```
    pub fn data_type(&self) -> Option<RasterDataType> {
        self.info.data_type
    }

    /// Number of vector layers.
    pub fn layer_count(&self) -> u32 {
        self.info.layer_count
    }

    /// Geotransform coefficients.
    ///
    /// `[origin_x, pixel_width, rotation_x, origin_y, rotation_y, pixel_height]`
    pub fn geotransform(&self) -> Option<&GeoTransform> {
        self.info.geotransform.as_ref()
    }

    /// Number of features in the primary vector layer.
    ///
    /// Returns `None` when the format does not support cheap feature counting
    /// or the count was not available at open time.
    pub fn feature_count(&self) -> Option<u64> {
        self.info.feature_count
    }

    /// Spatial bounding box of the dataset in its native CRS.
    ///
    /// Returns `None` when extent information is unavailable.
    pub fn bounds(&self) -> Option<&BoundingBox> {
        self.info.bounds.as_ref()
    }

    /// Return a logical clip of this dataset cropped to the given bounding box.
    ///
    /// For **raster** datasets the method converts `bbox` from world coordinates
    /// to pixel coordinates using the stored [`GeoTransform`], clamps the window
    /// to the dataset extent, and returns a new `Dataset` whose `width`, `height`,
    /// and geo-transform origin reflect the cropped region.  No pixels are read at
    /// clip time, but the pixel window is recorded on the returned dataset so that
    /// every subsequent real read — [`Dataset::read_band`], [`Dataset::bands`],
    /// [`Dataset::statistics`], and [`Dataset::convert`] — crops the source file
    /// to the clipped region rather than reprocessing the full raster.
    ///
    /// For **vector** datasets (no geo-transform) the same bounding-box is stored
    /// and the returned `Dataset` records the reduced spatial extent without
    /// materialising any filtered features.
    ///
    /// # Errors
    ///
    /// Returns [`OxiGeoError::InvalidParameter`] if the bounding box does not
    /// intersect the dataset extent, or if the dataset has no geotransform and no
    /// raster dimensions.
    pub fn clip(&self, bbox: BoundingBox) -> Result<Dataset> {
        self.clip_to_bbox(bbox)
    }

    /// Reproject this dataset to the given target EPSG code.
    ///
    /// Requires the `proj` feature flag.  Without it the method always returns
    /// [`OxiGeoError::NotSupported`].
    ///
    /// When `proj` is enabled the dataset's bounding box is transformed from its
    /// current CRS (parsed as an EPSG code from the stored CRS string, defaulting
    /// to **EPSG:4326** if none is present) to `target_epsg`.  The result is a new
    /// `Dataset` whose geo-transform and CRS string reflect the target projection.
    ///
    /// The output dimensions are preserved (same pixel count); only the
    /// geo-referencing metadata changes.
    ///
    /// # Errors
    ///
    /// - [`OxiGeoError::NotSupported`] — `proj` feature is not enabled.
    /// - [`OxiGeoError::InvalidParameter`] — `target_epsg` is unknown, or the
    ///   dataset has no raster dimensions / geo-transform to reproject.
    /// - [`OxiGeoError::Crs`] — transformation fails (singular matrix, etc.).
    pub fn reproject(&self, target_epsg: u32) -> Result<Dataset> {
        self.reproject_to_epsg(target_epsg)
    }
}

/// Extract an EPSG code integer from a CRS identification string.
///
/// Recognises the following patterns (case-insensitive):
/// - `"EPSG:4326"` — standard authority:code form
/// - `"epsg:4326"` — lowercase variant
///
/// Returns `None` when no pattern matches.
///
/// Only the GeoTIFF writer (which stamps an EPSG code into the output) and the
/// `proj` reprojection path need this, so it is compiled with them.
#[cfg(any(feature = "geotiff", feature = "proj"))]
pub(crate) fn extract_epsg_from_crs_string(crs: &str) -> Option<u32> {
    let upper = crs.to_uppercase();
    let pos = upper.find("EPSG:")?;
    let after_colon = &crs[pos + 5..];
    // Collect leading ASCII digits
    let digits: String = after_colon
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

impl core::fmt::Debug for Dataset {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Dataset")
            .field("path", &self.path)
            .field("format", &self.info.format)
            .field("width", &self.info.width)
            .field("height", &self.info.height)
            .field("band_count", &self.info.band_count)
            .field("layer_count", &self.info.layer_count)
            .field("data_type", &self.info.data_type)
            .finish()
    }
}

// ─── Top-level functions ────────────────────────────────────────────────────

/// OxiGeo version string.
///
/// Equivalent to `GDALVersionInfo("RELEASE_NAME")` in C GDAL.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Human-readable driver names for every format feature compiled in.
///
/// Assembled at compile time: an element is present exactly when its feature is
/// enabled, so the list is empty under `--no-default-features` without any
/// runtime branching (and without a `mut` binding that nothing would mutate).
const ENABLED_DRIVERS: &[&str] = &[
    #[cfg(feature = "geotiff")]
    "GTiff",
    #[cfg(feature = "geojson")]
    "GeoJSON",
    #[cfg(feature = "shapefile")]
    "ESRI Shapefile",
    #[cfg(feature = "geoparquet")]
    "GeoParquet",
    #[cfg(feature = "netcdf")]
    "netCDF",
    #[cfg(feature = "hdf5")]
    "HDF5",
    #[cfg(feature = "zarr")]
    "Zarr",
    #[cfg(feature = "grib")]
    "GRIB",
    #[cfg(feature = "stac")]
    "STAC",
    #[cfg(feature = "terrain")]
    "Terrain",
    #[cfg(feature = "vrt")]
    "VRT",
    #[cfg(feature = "flatgeobuf")]
    "FlatGeobuf",
    #[cfg(feature = "jpeg2000")]
    "JPEG2000",
    #[cfg(feature = "gpkg")]
    "GPKG",
    #[cfg(feature = "pmtiles")]
    "PMTiles",
    #[cfg(feature = "mbtiles")]
    "MBTiles",
    #[cfg(feature = "copc")]
    "COPC",
];

/// List all enabled format drivers.
///
/// Equivalent to `GDALAllRegister()` + iterating registered drivers in C GDAL.
///
/// Returns a list of human-readable driver names for all features
/// currently compiled in.
///
/// # Example
///
/// ```rust
/// let drivers = oxigeo::drivers();
/// assert!(drivers.contains(&"GTiff"));     // default feature
/// assert!(drivers.contains(&"GeoJSON"));   // default feature
/// assert!(drivers.contains(&"ESRI Shapefile")); // default feature
/// ```
pub fn drivers() -> Vec<&'static str> {
    ENABLED_DRIVERS.to_vec()
}

/// Number of registered (enabled) format drivers.
///
/// Equivalent to `GDALGetDriverCount()` in C GDAL.
pub fn driver_count() -> usize {
    ENABLED_DRIVERS.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Per-test scratch fixture inside the system temp dir (house policy: no
    /// hardcoded absolute paths).
    ///
    /// The leaf name embeds the process id and a monotonic counter, so no two test
    /// binaries — nor two concurrent runs of this one — can ever land on the same
    /// file.  Dropping the guard removes the fixture, so a panicking test leaks
    /// nothing.
    struct TempPath(PathBuf);

    impl TempPath {
        fn new(name: &str) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
            Self(
                std::env::temp_dir()
                    .join(format!("oxigeo_lib_{}_{seq}_{name}", std::process::id())),
            )
        }
    }

    impl std::ops::Deref for TempPath {
        type Target = Path;

        fn deref(&self) -> &Path {
            &self.0
        }
    }

    impl AsRef<Path> for TempPath {
        fn as_ref(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempPath {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn test_version() {
        let v = version();
        assert!(!v.is_empty());
        assert!(v.starts_with("0."));
    }

    #[test]
    fn test_default_drivers() {
        let d = drivers();
        // Default features: geotiff, geojson, shapefile
        assert!(d.contains(&"GTiff"), "GeoTIFF should be a default driver");
        assert!(d.contains(&"GeoJSON"), "GeoJSON should be a default driver");
        assert!(
            d.contains(&"ESRI Shapefile"),
            "Shapefile should be a default driver"
        );
    }

    #[test]
    fn test_driver_count() {
        assert!(driver_count() >= 3, "At least 3 default drivers");
    }

    #[test]
    fn test_format_detection() {
        assert_eq!(
            DatasetFormat::from_extension("world.tif"),
            DatasetFormat::GeoTiff
        );
        assert_eq!(
            DatasetFormat::from_extension("data.geojson"),
            DatasetFormat::GeoJson
        );
        assert_eq!(
            DatasetFormat::from_extension("map.shp"),
            DatasetFormat::Shapefile
        );
        assert_eq!(
            DatasetFormat::from_extension("cloud.zarr"),
            DatasetFormat::Zarr
        );
        assert_eq!(
            DatasetFormat::from_extension("output.parquet"),
            DatasetFormat::GeoParquet
        );
        assert_eq!(
            DatasetFormat::from_extension("scene.vrt"),
            DatasetFormat::Vrt
        );
        assert_eq!(
            DatasetFormat::from_extension("README.md"),
            DatasetFormat::Unknown
        );
    }

    #[test]
    fn test_format_display() {
        assert_eq!(DatasetFormat::GeoTiff.to_string(), "GTiff");
        assert_eq!(DatasetFormat::GeoJson.to_string(), "GeoJSON");
    }

    #[test]
    fn test_open_nonexistent() {
        let result = Dataset::open("/nonexistent/file.tif");
        assert!(result.is_err());
    }

    #[test]
    fn test_open_unsupported_extension() {
        let result = Dataset::open("data.xyz");
        assert!(result.is_err());
    }

    #[test]
    fn test_open_with_format() {
        // Opening with explicit format for a nonexistent file should give IoError
        let result = Dataset::open_with_format("/no/such/file.tif", DatasetFormat::GeoTiff);
        assert!(result.is_err());
    }

    // ─── detect_from_magic_bytes ─────────────────────────────────────────────

    #[test]
    fn test_magic_bytes_tiff_le() {
        let bytes = [0x49u8, 0x49, 0x2A, 0x00, 0x00, 0x00, 0x00, 0x00];
        assert_eq!(
            DatasetFormat::detect_from_magic_bytes(&bytes),
            Some(DatasetFormat::GeoTiff)
        );
    }

    #[test]
    fn test_magic_bytes_tiff_be() {
        let bytes = [0x4Du8, 0x4D, 0x00, 0x2A, 0x00, 0x00, 0x00, 0x00];
        assert_eq!(
            DatasetFormat::detect_from_magic_bytes(&bytes),
            Some(DatasetFormat::GeoTiff)
        );
    }

    #[test]
    fn test_magic_bytes_jp2() {
        let bytes: [u8; 12] = [
            0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
        ];
        assert_eq!(
            DatasetFormat::detect_from_magic_bytes(&bytes),
            Some(DatasetFormat::Jpeg2000)
        );
    }

    #[test]
    fn test_magic_bytes_hdf5() {
        let bytes: [u8; 8] = [0x89, 0x48, 0x44, 0x46, 0x0D, 0x0A, 0x1A, 0x0A];
        assert_eq!(
            DatasetFormat::detect_from_magic_bytes(&bytes),
            Some(DatasetFormat::Hdf5)
        );
    }

    #[test]
    fn test_magic_bytes_netcdf() {
        let bytes = [0x43u8, 0x44, 0x46, 0x01];
        assert_eq!(
            DatasetFormat::detect_from_magic_bytes(&bytes),
            Some(DatasetFormat::NetCdf)
        );
    }

    #[test]
    fn test_magic_bytes_flatgeobuf() {
        let bytes: [u8; 8] = [0x66, 0x67, 0x62, 0x03, 0x66, 0x67, 0x62, 0x00];
        assert_eq!(
            DatasetFormat::detect_from_magic_bytes(&bytes),
            Some(DatasetFormat::FlatGeobuf)
        );
    }

    #[test]
    fn test_magic_bytes_pmtiles() {
        let bytes = b"PMTiles\x03";
        assert_eq!(
            DatasetFormat::detect_from_magic_bytes(bytes),
            Some(DatasetFormat::PMTiles)
        );
    }

    #[test]
    fn test_magic_bytes_las() {
        // The generic LASF magic maps to plain LAS, not COPC (the octree VLR
        // that identifies COPC sits beyond the magic window).
        let bytes = b"LASF";
        assert_eq!(
            DatasetFormat::detect_from_magic_bytes(bytes),
            Some(DatasetFormat::Las)
        );
    }

    #[test]
    fn test_magic_bytes_grib() {
        let bytes = b"GRIB";
        assert_eq!(
            DatasetFormat::detect_from_magic_bytes(bytes),
            Some(DatasetFormat::Grib)
        );
    }

    #[test]
    fn test_magic_bytes_geoparquet() {
        let bytes = b"PAR1";
        assert_eq!(
            DatasetFormat::detect_from_magic_bytes(bytes),
            Some(DatasetFormat::GeoParquet)
        );
    }

    #[test]
    fn test_magic_bytes_sqlite() {
        let bytes: [u8; 16] = [
            0x53, 0x51, 0x4C, 0x69, 0x74, 0x65, 0x20, 0x66, 0x6F, 0x72, 0x6D, 0x61, 0x74, 0x20,
            0x33, 0x00,
        ];
        assert_eq!(
            DatasetFormat::detect_from_magic_bytes(&bytes),
            Some(DatasetFormat::GeoPackage)
        );
    }

    #[test]
    fn test_magic_bytes_zip() {
        let bytes: [u8; 4] = [0x50, 0x4B, 0x03, 0x04];
        assert_eq!(
            DatasetFormat::detect_from_magic_bytes(&bytes),
            Some(DatasetFormat::GeoPackage)
        );
    }

    #[test]
    fn test_magic_bytes_empty_returns_none() {
        assert_eq!(DatasetFormat::detect_from_magic_bytes(&[]), None);
    }

    #[test]
    fn test_magic_bytes_unknown_returns_none() {
        let bytes = b"UNKNOWNFORMAT";
        assert_eq!(DatasetFormat::detect_from_magic_bytes(bytes), None);
    }

    // ─── DatasetFormat::detect (file I/O) ────────────────────────────────────

    #[test]
    fn test_detect_file_tiff() {
        use std::io::Write;
        let path = TempPath::new("test_detect_tiff.tif");
        let bytes: [u8; 8] = [0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00];
        std::fs::File::create(&path)
            .and_then(|mut f| f.write_all(&bytes))
            .expect("write tiff");
        let fmt = DatasetFormat::detect(&path).expect("detect");
        assert_eq!(fmt, DatasetFormat::GeoTiff);
    }

    #[test]
    fn test_detect_file_plain_las() {
        use std::io::Write;
        let path = TempPath::new("test_detect_las.las");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"LASF");
        bytes.extend_from_slice(&[0u8; 64]);
        std::fs::File::create(&path)
            .and_then(|mut f| f.write_all(&bytes))
            .expect("write las");
        // A plain `.las` file is reported as LAS, not COPC.
        let fmt = DatasetFormat::detect(&path).expect("detect");
        assert_eq!(fmt, DatasetFormat::Las);
    }

    #[test]
    fn test_detect_file_copc_laz_promoted() {
        use std::io::Write;
        let path = TempPath::new("test_detect_cloud.copc.laz");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"LASF");
        bytes.extend_from_slice(&[0u8; 64]);
        std::fs::File::create(&path)
            .and_then(|mut f| f.write_all(&bytes))
            .expect("write copc laz");
        // The compound `.copc.laz` extension promotes LASF magic to COPC.
        let fmt = DatasetFormat::detect(&path).expect("detect");
        assert_eq!(fmt, DatasetFormat::Copc);
    }

    #[test]
    fn test_detect_fallback_to_extension() {
        use std::io::Write;
        let path = TempPath::new("test_detect_ext_fallback.geojson");
        std::fs::File::create(&path)
            .and_then(|mut f| f.write_all(b"{}"))
            .expect("write");
        let fmt = DatasetFormat::detect(&path).expect("detect");
        assert_eq!(fmt, DatasetFormat::GeoJson);
    }

    // ─── Dataset::open() wired metadata ──────────────────────────────────────

    #[cfg(feature = "geojson")]
    #[test]
    fn test_open_geojson_layer_count() {
        use std::io::Write;
        let path = TempPath::new("test_open_layer_count.geojson");
        let content = br#"{"type":"FeatureCollection","features":[]}"#;
        std::fs::File::create(&path)
            .and_then(|mut f| f.write_all(content))
            .expect("write");
        let ds = Dataset::open(path.to_str().expect("path str")).expect("open");
        assert_eq!(ds.format(), DatasetFormat::GeoJson);
        assert_eq!(
            ds.layer_count(),
            1,
            "FeatureCollection should have layer_count=1"
        );
        assert_eq!(
            ds.info().path,
            Some(path.to_str().expect("path str").to_string())
        );
    }

    #[cfg(feature = "geotiff")]
    #[test]
    fn test_open_tiff_wires_metadata() {
        use std::io::Write;
        let path = TempPath::new("test_open_tiff_meta.tif");
        // Minimal TIFF LE header with 3 IFD entries: width=64, height=32, spp=1
        let mut buf: Vec<u8> = vec![0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00];
        buf.extend_from_slice(&3u16.to_le_bytes()); // 3 entries
        // ImageWidth=64 (LONG)
        buf.extend_from_slice(&256u16.to_le_bytes());
        buf.extend_from_slice(&4u16.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&64u32.to_le_bytes());
        // ImageLength=32 (LONG)
        buf.extend_from_slice(&257u16.to_le_bytes());
        buf.extend_from_slice(&4u16.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&32u32.to_le_bytes());
        // SamplesPerPixel=4 (SHORT)
        buf.extend_from_slice(&277u16.to_le_bytes());
        buf.extend_from_slice(&3u16.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&4u16.to_le_bytes());
        buf.extend_from_slice(&[0x00, 0x00]);
        buf.extend_from_slice(&0u32.to_le_bytes()); // next IFD=0

        std::fs::File::create(&path)
            .and_then(|mut f| f.write_all(&buf))
            .expect("write tiff");

        let ds = Dataset::open(path.to_str().expect("path str")).expect("open");
        assert_eq!(ds.format(), DatasetFormat::GeoTiff);
        assert_eq!(ds.width(), 64);
        assert_eq!(ds.height(), 32);
        assert_eq!(ds.band_count(), 4);
        assert_eq!(
            ds.info().path,
            Some(path.to_str().expect("path str").to_string())
        );
    }
}
