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

/// Stream processing for large datasets
#[cfg(feature = "streaming")]
#[cfg_attr(docsrs, doc(cfg(feature = "streaming")))]
pub use oxigdal_streaming as streaming;

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

// ─── Unified Dataset API ────────────────────────────────────────────────────

/// Detected format of a geospatial dataset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatasetFormat {
    /// GeoTIFF / Cloud-Optimized GeoTIFF (.tif, .tiff)
    GeoTiff,
    /// GeoJSON (.geojson, .json)
    GeoJson,
    /// ESRI Shapefile (.shp)
    Shapefile,
    /// GeoParquet (.parquet, .geoparquet)
    GeoParquet,
    /// NetCDF (.nc, .nc4)
    NetCdf,
    /// HDF5 (.h5, .hdf5, .he5)
    Hdf5,
    /// Zarr (.zarr directory)
    Zarr,
    /// GRIB/GRIB2 (.grib, .grib2, .grb, .grb2)
    Grib,
    /// STAC catalog (.json with STAC metadata)
    Stac,
    /// Terrain formats
    Terrain,
    /// Virtual Raster Tiles (.vrt)
    Vrt,
    /// FlatGeobuf (.fgb)
    FlatGeobuf,
    /// JPEG2000 (.jp2, .j2k)
    Jpeg2000,
    /// Unknown / user-specified
    Unknown,
}

impl DatasetFormat {
    /// Detect format from file extension.
    ///
    /// Returns `DatasetFormat::Unknown` if the extension is not recognized.
    pub fn from_extension(path: &str) -> Self {
        let ext = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();

        match ext.as_str() {
            "tif" | "tiff" => Self::GeoTiff,
            "geojson" => Self::GeoJson,
            "shp" => Self::Shapefile,
            "parquet" | "geoparquet" => Self::GeoParquet,
            "nc" | "nc4" => Self::NetCdf,
            "h5" | "hdf5" | "he5" => Self::Hdf5,
            "zarr" => Self::Zarr,
            "grib" | "grib2" | "grb" | "grb2" => Self::Grib,
            "vrt" => Self::Vrt,
            "fgb" => Self::FlatGeobuf,
            "jp2" | "j2k" => Self::Jpeg2000,
            _ => Self::Unknown,
        }
    }

    /// Human-readable driver name (matches GDAL naming convention).
    pub fn driver_name(&self) -> &'static str {
        match self {
            Self::GeoTiff => "GTiff",
            Self::GeoJson => "GeoJSON",
            Self::Shapefile => "ESRI Shapefile",
            Self::GeoParquet => "GeoParquet",
            Self::NetCdf => "netCDF",
            Self::Hdf5 => "HDF5",
            Self::Zarr => "Zarr",
            Self::Grib => "GRIB",
            Self::Stac => "STAC",
            Self::Terrain => "Terrain",
            Self::Vrt => "VRT",
            Self::FlatGeobuf => "FlatGeobuf",
            Self::Jpeg2000 => "JPEG2000",
            Self::Unknown => "Unknown",
        }
    }
}

impl core::fmt::Display for DatasetFormat {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.driver_name())
    }
}

/// Basic dataset metadata — analogous to `GDALDataset` info.
#[derive(Debug, Clone)]
pub struct DatasetInfo {
    /// Detected format
    pub format: DatasetFormat,
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
        let format = DatasetFormat::from_extension(path);
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
            DatasetFormat::GeoTiff => Self::open_raster_stub(path, DatasetFormat::GeoTiff),

            #[cfg(feature = "geojson")]
            DatasetFormat::GeoJson => Self::open_vector_stub(path, DatasetFormat::GeoJson),

            #[cfg(feature = "shapefile")]
            DatasetFormat::Shapefile => Self::open_vector_stub(path, DatasetFormat::Shapefile),

            #[cfg(feature = "geoparquet")]
            DatasetFormat::GeoParquet => Self::open_vector_stub(path, DatasetFormat::GeoParquet),

            #[cfg(feature = "netcdf")]
            DatasetFormat::NetCdf => Self::open_raster_stub(path, DatasetFormat::NetCdf),

            #[cfg(feature = "hdf5")]
            DatasetFormat::Hdf5 => Self::open_raster_stub(path, DatasetFormat::Hdf5),

            #[cfg(feature = "zarr")]
            DatasetFormat::Zarr => Self::open_raster_stub(path, DatasetFormat::Zarr),

            #[cfg(feature = "grib")]
            DatasetFormat::Grib => Self::open_raster_stub(path, DatasetFormat::Grib),

            #[cfg(feature = "flatgeobuf")]
            DatasetFormat::FlatGeobuf => Self::open_vector_stub(path, DatasetFormat::FlatGeobuf),

            #[cfg(feature = "jpeg2000")]
            DatasetFormat::Jpeg2000 => Self::open_raster_stub(path, DatasetFormat::Jpeg2000),

            #[cfg(feature = "vrt")]
            DatasetFormat::Vrt => Self::open_raster_stub(path, DatasetFormat::Vrt),

            _ => Err(OxiGdalError::NotSupported {
                operation: format!(
                    "Format '{}' for '{}' — enable the corresponding feature flag or check the file extension",
                    format.driver_name(),
                    path,
                ),
            }),
        }
    }

    // -- Stub openers (delegate to driver crates in the future) ---------------

    fn open_raster_stub(path: &str, format: DatasetFormat) -> Result<Self> {
        // Verify file exists
        if !std::path::Path::new(path).exists() {
            return Err(OxiGdalError::Io(oxigdal_core::error::IoError::NotFound {
                path: path.to_string(),
            }));
        }

        Ok(Self {
            path: path.to_string(),
            info: DatasetInfo {
                format,
                width: None,
                height: None,
                band_count: 0,
                layer_count: 0,
                crs: None,
                geotransform: None,
            },
        })
    }

    fn open_vector_stub(path: &str, format: DatasetFormat) -> Result<Self> {
        if !std::path::Path::new(path).exists() {
            return Err(OxiGdalError::Io(oxigdal_core::error::IoError::NotFound {
                path: path.to_string(),
            }));
        }

        Ok(Self {
            path: path.to_string(),
            info: DatasetInfo {
                format,
                width: None,
                height: None,
                band_count: 0,
                layer_count: 0,
                crs: None,
                geotransform: None,
            },
        })
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
}
