//! # OxiGDAL — Pure Rust Geospatial Data Abstraction Library
//!
//! OxiGDAL is the Rust-native alternative to [GDAL](https://gdal.org/),
//! providing a comprehensive geospatial data abstraction layer
//! with **zero C/Fortran dependencies**. 100% Pure Rust.
//!
//! ## Quick Start
//!
//! ```toml
//! [dependencies]
//! oxigdal = "0.1"  # includes GeoTIFF, GeoJSON, Shapefile by default
//! ```
//!
//! ```rust
//! use oxigdal::Dataset;
//!
//! # fn main() -> oxigdal::Result<()> {
//! let drivers = oxigdal::drivers();
//! println!("Enabled drivers: {:?}", drivers);
//! println!("OxiGDAL version: {}", oxigdal::version());
//! # Ok(())
//! # }
//! ```
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
//! | `full` | ❌ | **All formats above** |
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
//! OxiGDAL aims to provide familiar concepts for GDAL users:
//!
//! | GDAL (C/C++) | OxiGDAL (Rust) |
//! |---|---|
//! | `GDALOpen()` | [`Dataset::open()`] |
//! | `GDALGetRasterBand()` | `dataset.raster_band(n)` |
//! | `GDALGetGeoTransform()` | [`Dataset::geotransform()`] |
//! | `GDALGetProjectionRef()` | [`Dataset::crs()`] |
//! | `GDALAllRegister()` | [`drivers()`] |
//! | `GDALVersionInfo()` | [`version()`] |
//! | `GDALWarp()` | `oxigdal::algorithms::warp()` (feature `algorithms`) |
//! | `ogr2ogr` | `oxigdal-cli convert` (crate `oxigdal-cli`) |
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────────────┐
//! │  oxigdal (this crate) — Unified API              │
//! │  Dataset::open() → auto-detect format            │
//! ├──────────────────────────────────────────────────┤
//! │  Drivers (feature-gated)                         │
//! │  ┌──────────┐ ┌──────────┐ ┌─────────────┐      │
//! │  │ GeoTIFF  │ │ GeoJSON  │ │  Shapefile  │ ...  │
//! │  └──────────┘ └──────────┘ └─────────────┘      │
//! ├──────────────────────────────────────────────────┤
//! │  oxigdal-core — Types, Buffers, Error, I/O       │
//! └──────────────────────────────────────────────────┘
//! ```
//!
//! ## Crate Ecosystem
//!
//! OxiGDAL is a workspace of 65+ crates. This `oxigdal` crate serves as
//! the **unified entry point**. Individual crates can also be used directly:
//!
//! ```toml
//! # Use the unified API (recommended for most users)
//! oxigdal = { version = "0.1", features = ["full", "cloud", "proj"] }
//!
//! # Or pick individual crates for minimal dependencies
//! oxigdal-core = "0.1"
//! oxigdal-geotiff = "0.1"
//! ```
//!
//! ## Pure Rust — No C/Fortran Dependencies
//!
//! Unlike the original GDAL which requires C/C++ compilation and system
//! libraries (PROJ, GEOS, etc.), OxiGDAL is **100% Pure Rust**:
//!
//! - No `bindgen`, no `cc`, no `cmake`
//! - Cross-compiles to WASM, embedded, mobile
//! - `cargo add oxigdal` — that's it
//!
//! Part of the [COOLJAPAN](https://github.com/cool-japan) ecosystem.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

// Re-export core types — always available
pub use oxigdal_core::error::OxiGdalError;
pub use oxigdal_core::error::Result;
pub use oxigdal_core::types::{BoundingBox, GeoTransform, RasterDataType, RasterMetadata};

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

/// Spatial operations on `Dataset`: clip and reproject.
mod dataset_ops;

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
pub use oxigdal_core as core_types;

// ─── Driver re-exports (feature-gated) ──────────────────────────────────────

/// GeoTIFF raster driver (Cloud-Optimized GeoTIFF support)
#[cfg(feature = "geotiff")]
#[cfg_attr(docsrs, doc(cfg(feature = "geotiff")))]
pub use oxigdal_geotiff as geotiff;

/// GeoJSON vector driver
#[cfg(feature = "geojson")]
#[cfg_attr(docsrs, doc(cfg(feature = "geojson")))]
pub use oxigdal_geojson as geojson;

/// ESRI Shapefile driver
#[cfg(feature = "shapefile")]
#[cfg_attr(docsrs, doc(cfg(feature = "shapefile")))]
pub use oxigdal_shapefile as shapefile;

/// GeoParquet columnar format driver
#[cfg(feature = "geoparquet")]
#[cfg_attr(docsrs, doc(cfg(feature = "geoparquet")))]
pub use oxigdal_geoparquet as geoparquet;

/// NetCDF scientific format driver
#[cfg(feature = "netcdf")]
#[cfg_attr(docsrs, doc(cfg(feature = "netcdf")))]
pub use oxigdal_netcdf as netcdf;

/// HDF5 hierarchical data driver
#[cfg(feature = "hdf5")]
#[cfg_attr(docsrs, doc(cfg(feature = "hdf5")))]
pub use oxigdal_hdf5 as hdf5;

/// Zarr cloud-native array driver
#[cfg(feature = "zarr")]
#[cfg_attr(docsrs, doc(cfg(feature = "zarr")))]
pub use oxigdal_zarr as zarr;

/// GRIB meteorological data driver
#[cfg(feature = "grib")]
#[cfg_attr(docsrs, doc(cfg(feature = "grib")))]
pub use oxigdal_grib as grib;

/// SpatioTemporal Asset Catalog driver
#[cfg(feature = "stac")]
#[cfg_attr(docsrs, doc(cfg(feature = "stac")))]
pub use oxigdal_stac as stac;

/// Terrain/elevation data driver
#[cfg(feature = "terrain")]
#[cfg_attr(docsrs, doc(cfg(feature = "terrain")))]
pub use oxigdal_terrain as terrain;

/// Virtual Raster Tiles driver
#[cfg(feature = "vrt")]
#[cfg_attr(docsrs, doc(cfg(feature = "vrt")))]
pub use oxigdal_vrt as vrt;

/// FlatGeobuf vector format driver
#[cfg(feature = "flatgeobuf")]
#[cfg_attr(docsrs, doc(cfg(feature = "flatgeobuf")))]
pub use oxigdal_flatgeobuf as flatgeobuf;

/// JPEG2000 raster format driver
#[cfg(feature = "jpeg2000")]
#[cfg_attr(docsrs, doc(cfg(feature = "jpeg2000")))]
pub use oxigdal_jpeg2000 as jpeg2000;

// ─── Advanced capability re-exports (feature-gated) ─────────────────────────

/// Cloud storage backends (S3, GCS, Azure Blob)
#[cfg(feature = "cloud")]
#[cfg_attr(docsrs, doc(cfg(feature = "cloud")))]
pub use oxigdal_cloud as cloud;

/// Coordinate reference system transformations (Pure Rust proj)
#[cfg(feature = "proj")]
#[cfg_attr(docsrs, doc(cfg(feature = "proj")))]
pub use oxigdal_proj as proj;

/// Raster and vector algorithms (resampling, reprojection, etc.)
#[cfg(feature = "algorithms")]
#[cfg_attr(docsrs, doc(cfg(feature = "algorithms")))]
pub use oxigdal_algorithms as algorithms;

/// Geospatial analytics and statistics
#[cfg(feature = "analytics")]
#[cfg_attr(docsrs, doc(cfg(feature = "analytics")))]
pub use oxigdal_analytics as analytics;

/// Stream processing for large datasets (advanced streaming crate)
#[cfg(feature = "streaming")]
#[cfg_attr(docsrs, doc(cfg(feature = "streaming")))]
pub use oxigdal_streaming as streaming_ext;

/// Machine learning integration
#[cfg(feature = "ml")]
#[cfg_attr(docsrs, doc(cfg(feature = "ml")))]
pub use oxigdal_ml as ml;

/// GPU-accelerated geospatial processing
#[cfg(feature = "gpu")]
#[cfg_attr(docsrs, doc(cfg(feature = "gpu")))]
pub use oxigdal_gpu as gpu;

/// OGC-compliant geospatial tile/feature server
#[cfg(feature = "server")]
#[cfg_attr(docsrs, doc(cfg(feature = "server")))]
pub use oxigdal_server as server;

/// Temporal/time-series geospatial analysis
#[cfg(feature = "temporal")]
#[cfg_attr(docsrs, doc(cfg(feature = "temporal")))]
pub use oxigdal_temporal as temporal;

// ─── Tile / database / point-cloud re-exports (feature-gated) ───────────────

/// GeoPackage (SQLite-based) driver
#[cfg(feature = "gpkg")]
#[cfg_attr(docsrs, doc(cfg(feature = "gpkg")))]
pub use oxigdal_gpkg as gpkg;

/// PMTiles v3 tile archive driver
#[cfg(feature = "pmtiles")]
#[cfg_attr(docsrs, doc(cfg(feature = "pmtiles")))]
pub use oxigdal_pmtiles as pmtiles;

/// MBTiles tile archive driver
#[cfg(feature = "mbtiles")]
#[cfg_attr(docsrs, doc(cfg(feature = "mbtiles")))]
pub use oxigdal_mbtiles as mbtiles;

/// COPC (Cloud Optimized Point Cloud) driver
#[cfg(feature = "copc")]
#[cfg_attr(docsrs, doc(cfg(feature = "copc")))]
pub use oxigdal_copc as copc;

/// Spatial index (R-tree, grid) module
#[cfg(feature = "index")]
#[cfg_attr(docsrs, doc(cfg(feature = "index")))]
pub use oxigdal_index as index;

/// no_std fixed-size geometry primitives
#[cfg(feature = "noalloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "noalloc")))]
pub use oxigdal_noalloc as noalloc;

/// OGC Web Services (WFS, WCS, WPS, CSW)
#[cfg(feature = "services")]
#[cfg_attr(docsrs, doc(cfg(feature = "services")))]
pub use oxigdal_services as services;

// ─── Unified Dataset API ────────────────────────────────────────────────────

/// Basic dataset metadata — analogous to `GDALDataset` info.
#[derive(Debug, Clone)]
pub struct DatasetInfo {
    /// Detected format
    pub format: DatasetFormat,
    /// Filesystem path this dataset was opened from, if known.
    ///
    /// `None` for cloud/remote datasets and programmatically-created datasets.
    pub path: Option<String>,
    /// Width in pixels (raster) or `None` (vector-only)
    pub width: Option<u32>,
    /// Height in pixels (raster) or `None` (vector-only)
    pub height: Option<u32>,
    /// Number of raster bands
    pub band_count: u32,
    /// Number of vector layers
    pub layer_count: u32,
    /// Coordinate reference system (WKT, EPSG code, or PROJ string)
    pub crs: Option<String>,
    /// Geotransform: `[origin_x, pixel_width, rotation_x, origin_y, rotation_y, pixel_height]`
    pub geotransform: Option<GeoTransform>,
    /// Number of features in the primary vector layer.
    ///
    /// `None` when the format does not support cheap feature counting (e.g. streaming formats).
    pub feature_count: Option<u64>,
    /// Spatial extent of the dataset in the dataset's native CRS.
    ///
    /// Computed from the geotransform for raster datasets, or from the GeoJSON `bbox`
    /// field for vector datasets.  `None` when extent information is unavailable.
    pub bounds: Option<BoundingBox>,
}

/// Unified dataset handle — the central abstraction (analogous to `GDALDataset`).
///
/// Opens any supported geospatial format and provides uniform access
/// to raster bands, vector layers, and metadata.
///
/// # Example
///
/// ```rust,no_run
/// use oxigdal::Dataset;
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
    /// Returns [`OxiGdalError::NotSupported`] if the format is not recognized
    /// or the corresponding feature flag is not enabled.
    ///
    /// Returns [`OxiGdalError::Io`] if the file cannot be read.
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

            _ => Err(OxiGdalError::NotSupported {
                operation: format!(
                    "Format '{}' for '{}' — enable the corresponding feature flag or check the file extension",
                    format.driver_name(),
                    path,
                ),
            }),
        }
    }

    // -- Real openers: delegate header parsing to open.rs helpers -------------

    fn open_raster(path: &str, format: DatasetFormat) -> Result<Self> {
        let p = std::path::Path::new(path);
        if !p.exists() {
            return Err(OxiGdalError::Io(oxigdal_core::error::IoError::NotFound {
                path: path.to_string(),
            }));
        }

        // For GeoTIFF, parse the IFD header for width / height / band_count /
        // geotransform.  Other raster formats fall back to empty metadata.
        let mut info = match format {
            DatasetFormat::GeoTiff => {
                crate::open::extract_tiff_info(p).unwrap_or_else(|| DatasetInfo {
                    format,
                    path: Some(path.to_string()),
                    width: None,
                    height: None,
                    band_count: 0,
                    layer_count: 0,
                    crs: None,
                    geotransform: None,
                    feature_count: None,
                    bounds: None,
                })
            }
            _ => DatasetInfo {
                format,
                path: Some(path.to_string()),
                width: None,
                height: None,
                band_count: 0,
                layer_count: 0,
                crs: None,
                geotransform: None,
                feature_count: None,
                bounds: None,
            },
        };

        // Ensure path is populated even when extracted from helper
        info.path = Some(path.to_string());

        Ok(Self {
            path: path.to_string(),
            info,
        })
    }

    fn open_vector(path: &str, format: DatasetFormat) -> Result<Self> {
        let p = std::path::Path::new(path);
        if !p.exists() {
            return Err(OxiGdalError::Io(oxigdal_core::error::IoError::NotFound {
                path: path.to_string(),
            }));
        }

        let empty_info = || DatasetInfo {
            format,
            path: Some(path.to_string()),
            width: None,
            height: None,
            band_count: 0,
            layer_count: 0,
            crs: None,
            geotransform: None,
            feature_count: None,
            bounds: None,
        };

        let mut info = match format {
            DatasetFormat::GeoJson => {
                crate::open::extract_geojson_info(p).unwrap_or_else(empty_info)
            }
            #[cfg(feature = "shapefile")]
            DatasetFormat::Shapefile => {
                crate::open::extract_shapefile_info(p).unwrap_or_else(empty_info)
            }
            #[cfg(feature = "flatgeobuf")]
            DatasetFormat::FlatGeobuf => {
                crate::open::extract_flatgeobuf_info(p).unwrap_or_else(empty_info)
            }
            #[cfg(feature = "geoparquet")]
            DatasetFormat::GeoParquet => {
                crate::open::extract_geoparquet_info(p).unwrap_or_else(empty_info)
            }
            _ => empty_info(),
        };

        // Ensure path is populated even when extracted from helper
        info.path = Some(path.to_string());

        Ok(Self {
            path: path.to_string(),
            info,
        })
    }

    // -- Constructors from pre-built info ------------------------------------------

    /// Construct a `Dataset` directly from a path and a pre-parsed [`DatasetInfo`].
    ///
    /// Used internally by the cloud-detect module and by tests that need to
    /// create a `Dataset` without a file on disk.
    pub(crate) fn from_info(path: String, info: DatasetInfo) -> Self {
        Self { path, info }
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

    // ── Convenience methods ───────────────────────────────────────────────────

    /// Read a single raster band by 0-based index and return its pixel data as
    /// a `RasterBuffer`.
    ///
    /// Requires the `geotiff` feature for GeoTIFF datasets.  Other formats
    /// return [`OxiGdalError::NotSupported`].
    ///
    /// `band` is **0-based**: band 0 is the first raster band.
    ///
    /// # Errors
    ///
    /// - [`OxiGdalError::NotSupported`] — format is not supported.
    /// - [`OxiGdalError::InvalidParameter`] — `band` index is out of range.
    /// - [`OxiGdalError::Io`] / [`OxiGdalError::Format`] — underlying read failure.
    pub fn read_band(&self, band: u32) -> Result<oxigdal_core::buffer::RasterBuffer> {
        self.read_band_impl(band)
    }

    /// Return a lazy iterator over all raster bands.
    ///
    /// Each call to `Iterator::next()` reads the next band from the underlying
    /// file.  For multi-band GeoTIFF datasets this avoids loading all bands
    /// into memory simultaneously.
    ///
    /// The iterator yields `Result<RasterBuffer>` so that per-band read errors
    /// are propagated without aborting the iteration.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use oxigdal::Dataset;
    ///
    /// # fn main() -> oxigdal::Result<()> {
    /// let ds = Dataset::open("elevation.tif")?;
    /// for band_result in ds.bands() {
    ///     let buf = band_result?;
    ///     println!("band pixels: {}", buf.pixel_count());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn bands(&self) -> BandIter<'_> {
        BandIter {
            dataset: self,
            next_band: 0,
            band_count: self.info.band_count,
        }
    }

    /// Inner implementation for [`Self::read_band`].
    fn read_band_impl(&self, band: u32) -> Result<oxigdal_core::buffer::RasterBuffer> {
        if self.info.band_count > 0 && band >= self.info.band_count {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "band",
                message: format!(
                    "band index {} is out of range (dataset has {} bands)",
                    band, self.info.band_count
                ),
            });
        }

        #[cfg(feature = "geotiff")]
        if matches!(self.info.format, DatasetFormat::GeoTiff) {
            return self.read_band_geotiff(band);
        }

        Err(OxiGdalError::NotSupported {
            operation: format!(
                "read_band() is not supported for format '{}' (enable the 'geotiff' feature for GeoTIFF support)",
                self.info.format.driver_name()
            ),
        })
    }

    /// GeoTIFF-specific band reader.
    #[cfg(feature = "geotiff")]
    fn read_band_geotiff(&self, band: u32) -> Result<oxigdal_core::buffer::RasterBuffer> {
        use oxigdal_core::buffer::RasterBuffer;
        use oxigdal_core::io::FileDataSource;
        use oxigdal_core::types::NoDataValue;
        use oxigdal_geotiff::GeoTiffReader;

        let source = FileDataSource::open(&self.path).map_err(|e| {
            OxiGdalError::Io(oxigdal_core::error::IoError::Read {
                message: format!("failed to open '{}': {e}", self.path),
            })
        })?;
        let reader = GeoTiffReader::open(source)?;

        let width = reader.width();
        let height = reader.height();

        let raw_bytes = reader.read_band(0, band as usize)?;

        let data_type = reader
            .data_type()
            .unwrap_or(oxigdal_core::types::RasterDataType::UInt8);

        RasterBuffer::new(raw_bytes, width, height, data_type, NoDataValue::None).map_err(|e| {
            OxiGdalError::Internal {
                message: format!("failed to create RasterBuffer: {e}"),
            }
        })
    }

    /// Compute per-band raster statistics (min / max / mean / std_dev / valid_count).
    ///
    /// Currently supported for GeoTIFF datasets (requires the `geotiff` feature).
    /// For all other formats or when the feature flag is absent the method returns
    /// [`OxiGdalError::NotSupported`].
    ///
    /// `band` is **0-based**: band 0 is the first raster band.
    ///
    /// # Errors
    ///
    /// - [`OxiGdalError::NotSupported`] — format is not a supported raster type or
    ///   the required feature flag is disabled.
    /// - [`OxiGdalError::InvalidParameter`] — `band` index is out of range.
    /// - [`OxiGdalError::Io`] / [`OxiGdalError::Format`] — underlying read failure.
    pub fn statistics(&self, band: u32) -> Result<BandStatistics> {
        self.compute_band_statistics(band)
    }

    /// Return a logical clip of this dataset cropped to the given bounding box.
    ///
    /// For **raster** datasets the method converts `bbox` from world coordinates
    /// to pixel coordinates using the stored [`GeoTransform`], clamps the window
    /// to the dataset extent, and returns a new `Dataset` whose `width`, `height`,
    /// and geo-transform origin reflect the cropped region.  No pixels are read or
    /// written — the result is a lightweight metadata view.
    ///
    /// For **vector** datasets (no geo-transform) the same bounding-box is stored
    /// and the returned `Dataset` records the reduced spatial extent without
    /// materialising any filtered features.
    ///
    /// # Errors
    ///
    /// Returns [`OxiGdalError::InvalidParameter`] if the bounding box does not
    /// intersect the dataset extent, or if the dataset has no geotransform and no
    /// raster dimensions.
    pub fn clip(&self, bbox: BoundingBox) -> Result<Dataset> {
        self.clip_to_bbox(bbox)
    }

    /// Reproject this dataset to the given target EPSG code.
    ///
    /// Requires the `proj` feature flag.  Without it the method always returns
    /// [`OxiGdalError::NotSupported`].
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
    /// - [`OxiGdalError::NotSupported`] — `proj` feature is not enabled.
    /// - [`OxiGdalError::InvalidParameter`] — `target_epsg` is unknown, or the
    ///   dataset has no raster dimensions / geo-transform to reproject.
    /// - [`OxiGdalError::Crs`] — transformation fails (singular matrix, etc.).
    pub fn reproject(&self, target_epsg: u32) -> Result<Dataset> {
        self.reproject_to_epsg(target_epsg)
    }

    // ── Private implementation helpers ────────────────────────────────────────

    /// Inner implementation for [`Self::statistics`].
    fn compute_band_statistics(&self, band: u32) -> Result<BandStatistics> {
        // Validate band range against known band count (only when we have metadata)
        if self.info.band_count > 0 && band >= self.info.band_count {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "band",
                message: format!(
                    "band index {} is out of range (dataset has {} bands)",
                    band, self.info.band_count
                ),
            });
        }

        // Dispatch to the GeoTIFF reader path when the feature is compiled in.
        #[cfg(feature = "geotiff")]
        if matches!(self.info.format, DatasetFormat::GeoTiff) {
            return self.statistics_geotiff(band);
        }

        Err(OxiGdalError::NotSupported {
            operation: format!(
                "statistics() is not supported for format '{}' (enable the 'geotiff' feature for GeoTIFF support)",
                self.info.format.driver_name()
            ),
        })
    }

    /// GeoTIFF-specific statistics reader.
    #[cfg(feature = "geotiff")]
    fn statistics_geotiff(&self, band: u32) -> Result<BandStatistics> {
        use oxigdal_core::buffer::RasterBuffer;
        use oxigdal_core::io::FileDataSource;
        use oxigdal_core::types::NoDataValue;
        use oxigdal_geotiff::GeoTiffReader;

        let source = FileDataSource::open(&self.path).map_err(|e| {
            OxiGdalError::Io(oxigdal_core::error::IoError::Read {
                message: format!("failed to open '{}': {e}", self.path),
            })
        })?;
        let reader = GeoTiffReader::open(source)?;

        let width = reader.width();
        let height = reader.height();

        // read_band takes (level, band_index) — level 0 is full resolution
        let raw_bytes = reader.read_band(0, band as usize)?;

        let data_type = reader
            .data_type()
            .unwrap_or(oxigdal_core::types::RasterDataType::UInt8);

        let buf = RasterBuffer::new(raw_bytes, width, height, data_type, NoDataValue::None)
            .map_err(|e| OxiGdalError::Internal {
                message: format!("failed to create RasterBuffer: {e}"),
            })?;

        let buf_stats = buf.compute_statistics()?;

        Ok(BandStatistics {
            band,
            min: buf_stats.min,
            max: buf_stats.max,
            mean: buf_stats.mean,
            std_dev: buf_stats.std_dev,
            valid_count: buf_stats.valid_count,
        })
    }
}

/// Statistics for a single raster band.
///
/// Returned by [`Dataset::statistics`].
#[derive(Debug, Clone, PartialEq)]
pub struct BandStatistics {
    /// 0-based band index.
    pub band: u32,
    /// Minimum valid pixel value (non-nodata, finite).
    pub min: f64,
    /// Maximum valid pixel value (non-nodata, finite).
    pub max: f64,
    /// Arithmetic mean of valid pixels.
    pub mean: f64,
    /// Population standard deviation of valid pixels.
    pub std_dev: f64,
    /// Count of valid (non-nodata, finite) pixels.
    pub valid_count: u64,
}

/// Lazy iterator over raster bands of a [`Dataset`].
///
/// Created by [`Dataset::bands`].  Each call to [`Iterator::next`] reads the
/// next band from the underlying file and returns `Ok(RasterBuffer)` on
/// success or an `Err` on I/O or format failure.
pub struct BandIter<'a> {
    /// Reference to the dataset being iterated.
    pub(crate) dataset: &'a Dataset,
    /// Index of the next band to yield.
    pub(crate) next_band: u32,
    /// Total number of bands (cached to avoid repeated accessors).
    pub(crate) band_count: u32,
}

impl<'a> Iterator for BandIter<'a> {
    type Item = Result<oxigdal_core::buffer::RasterBuffer>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next_band >= self.band_count {
            return None;
        }
        let band = self.next_band;
        self.next_band += 1;
        Some(self.dataset.read_band(band))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = (self.band_count.saturating_sub(self.next_band)) as usize;
        (remaining, Some(remaining))
    }
}

impl<'a> core::iter::ExactSizeIterator for BandIter<'a> {}

// ─── ConversionOptions ───────────────────────────────────────────────────────

/// Output compression codec for [`Dataset::convert`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Compression {
    /// No compression (default).
    #[default]
    None,
    /// DEFLATE / zlib compression.
    Deflate,
    /// LZW compression.
    Lzw,
    /// PackBits run-length encoding.
    PackBits,
    /// ZSTD compression (not universally supported).
    Zstd,
}

/// Options controlling [`Dataset::convert`].
///
/// All fields are optional — `ConversionOptions::default()` produces
/// a lossless identity conversion.
#[derive(Debug, Clone, Default)]
pub struct ConversionOptions {
    /// Output compression codec.  Defaults to [`Compression::None`].
    pub compression: Option<Compression>,
    /// Compression level 0–9 (format-specific meaning).
    pub compression_level: Option<u8>,
    /// Write as Cloud-Optimized GeoTIFF (COG) when `true`.
    pub cog: bool,
    /// Overview decimation factors to embed (e.g. `[2, 4, 8, 16]`).
    pub overviews: Vec<u32>,
    /// Output tile size in pixels (square); uses strip layout when `None`.
    pub tile_size: Option<u32>,
    /// Arbitrary driver creation options (e.g. `("PHOTOMETRIC", "RGB")`).
    pub creation_options: Vec<(String, String)>,
}

/// Extract an EPSG code integer from a CRS identification string.
///
/// Recognises the following patterns (case-insensitive):
/// - `"EPSG:4326"` — standard authority:code form
/// - `"epsg:4326"` — lowercase variant
///
/// Returns `None` when no pattern matches.
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
            .finish()
    }
}

// ─── Top-level functions ────────────────────────────────────────────────────

/// OxiGDAL version string.
///
/// Equivalent to `GDALVersionInfo("RELEASE_NAME")` in C GDAL.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

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
/// let drivers = oxigdal::drivers();
/// assert!(drivers.contains(&"GTiff"));     // default feature
/// assert!(drivers.contains(&"GeoJSON"));   // default feature
/// assert!(drivers.contains(&"ESRI Shapefile")); // default feature
/// ```
#[allow(clippy::vec_init_then_push)]
pub fn drivers() -> Vec<&'static str> {
    let mut list = Vec::new();

    #[cfg(feature = "geotiff")]
    list.push("GTiff");
    #[cfg(feature = "geojson")]
    list.push("GeoJSON");
    #[cfg(feature = "shapefile")]
    list.push("ESRI Shapefile");
    #[cfg(feature = "geoparquet")]
    list.push("GeoParquet");
    #[cfg(feature = "netcdf")]
    list.push("netCDF");
    #[cfg(feature = "hdf5")]
    list.push("HDF5");
    #[cfg(feature = "zarr")]
    list.push("Zarr");
    #[cfg(feature = "grib")]
    list.push("GRIB");
    #[cfg(feature = "stac")]
    list.push("STAC");
    #[cfg(feature = "terrain")]
    list.push("Terrain");
    #[cfg(feature = "vrt")]
    list.push("VRT");
    #[cfg(feature = "flatgeobuf")]
    list.push("FlatGeobuf");
    #[cfg(feature = "jpeg2000")]
    list.push("JPEG2000");
    #[cfg(feature = "gpkg")]
    list.push("GPKG");
    #[cfg(feature = "pmtiles")]
    list.push("PMTiles");
    #[cfg(feature = "mbtiles")]
    list.push("MBTiles");
    #[cfg(feature = "copc")]
    list.push("COPC");

    list
}

/// Number of registered (enabled) format drivers.
///
/// Equivalent to `GDALGetDriverCount()` in C GDAL.
pub fn driver_count() -> usize {
    drivers().len()
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let bytes = b"LASF";
        assert_eq!(
            DatasetFormat::detect_from_magic_bytes(bytes),
            Some(DatasetFormat::Copc)
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
        let dir = std::env::temp_dir();
        let path = dir.join("test_detect_tiff.tif");
        let bytes: [u8; 8] = [0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00];
        std::fs::File::create(&path)
            .and_then(|mut f| f.write_all(&bytes))
            .expect("write tiff");
        let fmt = DatasetFormat::detect(&path).expect("detect");
        assert_eq!(fmt, DatasetFormat::GeoTiff);
    }

    #[test]
    fn test_detect_file_las_as_copc() {
        use std::io::Write;
        let dir = std::env::temp_dir();
        let path = dir.join("test_detect_las.las");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"LASF");
        bytes.extend_from_slice(&[0u8; 64]);
        std::fs::File::create(&path)
            .and_then(|mut f| f.write_all(&bytes))
            .expect("write las");
        let fmt = DatasetFormat::detect(&path).expect("detect");
        assert_eq!(fmt, DatasetFormat::Copc);
    }

    #[test]
    fn test_detect_fallback_to_extension() {
        use std::io::Write;
        let dir = std::env::temp_dir();
        let path = dir.join("test_detect_ext_fallback.geojson");
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
        let dir = std::env::temp_dir();
        let path = dir.join("test_open_layer_count.geojson");
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
        let dir = std::env::temp_dir();
        let path = dir.join("test_open_tiff_meta.tif");
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
