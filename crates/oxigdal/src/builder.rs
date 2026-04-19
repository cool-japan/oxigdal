//! Builder patterns for ergonomic dataset creation and opening.
//!
//! This module provides two main builders:
//!
//! - [`DatasetOpenBuilder`] — opens an existing dataset with various options
//! - [`DatasetCreateBuilder`] — creates / configures a new dataset for writing
//!
//! Both builders use the fluent / method-chaining pattern and produce a final
//! value via `.open()` or `.create()` respectively.
//!
//! # Examples
//!
//! ```rust,no_run
//! use oxigdal::builder::{DatasetOpenBuilder, DatasetCreateBuilder, OutputFormat, CompressionType};
//!
//! # fn main() -> oxigdal::Result<()> {
//! // ── opening ───────────────────────────────────────────────────────────────
//! let ds = DatasetOpenBuilder::new("elevation.tif")
//!     .read_only(true)
//!     .with_overview_level(2)
//!     .with_tile_cache_mb(128)
//!     .open()?;
//!
//! // ── creating ──────────────────────────────────────────────────────────────
//! let writer = DatasetCreateBuilder::new("/tmp/out.tif", OutputFormat::GeoTiff)
//!     .with_crs("EPSG:4326")
//!     .with_compression(CompressionType::Deflate)
//!     .with_tile_size(256, 256)
//!     .create()?;
//! println!("Writing to: {}", writer.path().display());
//! # Ok(())
//! # }
//! ```

use std::path::{Path, PathBuf};

use crate::{DatasetFormat, Result, open::OpenedDataset, open::open};
use oxigdal_core::error::OxiGdalError;
use oxigdal_core::types::{GeoTransform, RasterDataType};

// ─── Output / Compression enums ──────────────────────────────────────────────

/// Supported output formats for dataset creation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum OutputFormat {
    /// GeoTIFF / Cloud-Optimized GeoTIFF
    GeoTiff,
    /// GeoJSON vector format
    GeoJson,
    /// ESRI Shapefile
    Shapefile,
    /// GeoPackage (SQLite-based)
    GeoPackage,
    /// GeoParquet (Apache Parquet with geometry extension)
    GeoParquet,
    /// FlatGeobuf
    FlatGeobuf,
    /// Virtual Raster Tiles (VRT)
    Vrt,
}

impl OutputFormat {
    /// Return a human-readable driver name (mirrors GDAL naming convention).
    pub fn driver_name(&self) -> &'static str {
        match self {
            Self::GeoTiff => "GTiff",
            Self::GeoJson => "GeoJSON",
            Self::Shapefile => "ESRI Shapefile",
            Self::GeoPackage => "GPKG",
            Self::GeoParquet => "GeoParquet",
            Self::FlatGeobuf => "FlatGeobuf",
            Self::Vrt => "VRT",
        }
    }

    /// Return the canonical file extension (without the leading dot).
    pub fn default_extension(&self) -> &'static str {
        match self {
            Self::GeoTiff => "tif",
            Self::GeoJson => "geojson",
            Self::Shapefile => "shp",
            Self::GeoPackage => "gpkg",
            Self::GeoParquet => "parquet",
            Self::FlatGeobuf => "fgb",
            Self::Vrt => "vrt",
        }
    }

    /// Derive an [`OutputFormat`] from a [`DatasetFormat`], if possible.
    pub fn from_dataset_format(fmt: DatasetFormat) -> Option<Self> {
        match fmt {
            DatasetFormat::GeoTiff => Some(Self::GeoTiff),
            DatasetFormat::GeoJson => Some(Self::GeoJson),
            DatasetFormat::Shapefile => Some(Self::Shapefile),
            DatasetFormat::GeoParquet => Some(Self::GeoParquet),
            DatasetFormat::FlatGeobuf => Some(Self::FlatGeobuf),
            DatasetFormat::Vrt => Some(Self::Vrt),
            _ => None,
        }
    }
}

impl core::fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.driver_name())
    }
}

/// Compression algorithm for raster / columnar outputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum CompressionType {
    /// No compression
    #[default]
    None,
    /// DEFLATE (zlib/gzip compatible)
    Deflate,
    /// LZW (lossless, fast decode)
    Lzw,
    /// Zstandard (excellent ratio + speed balance)
    Zstd,
    /// LZ4 (fastest compress/decompress)
    Lz4,
    /// JPEG (lossy, for imagery)
    Jpeg,
    /// WebP (lossy/lossless for imagery)
    WebP,
}

impl CompressionType {
    /// GDAL-compatible compression tag name.
    pub fn tag_name(&self) -> &'static str {
        match self {
            Self::None => "NONE",
            Self::Deflate => "DEFLATE",
            Self::Lzw => "LZW",
            Self::Zstd => "ZSTD",
            Self::Lz4 => "LZ4",
            Self::Jpeg => "JPEG",
            Self::WebP => "WEBP",
        }
    }
}

impl core::fmt::Display for CompressionType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.tag_name())
    }
}

// ─── DatasetOpenBuilder ───────────────────────────────────────────────────────

/// Builder for opening an existing geospatial dataset with configurable options.
///
/// Uses the fluent / method-chaining pattern.  Finalise with `.open()`.
///
/// # Example
///
/// ```rust,no_run
/// use oxigdal::builder::DatasetOpenBuilder;
///
/// # fn main() -> oxigdal::Result<()> {
/// let ds = DatasetOpenBuilder::new("world.tif")
///     .read_only(true)
///     .with_overview_level(1)
///     .with_tile_cache_mb(64)
///     .with_crs_override("EPSG:4326")
///     .open()?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct DatasetOpenBuilder {
    path: PathBuf,
    read_only: bool,
    overview_level: Option<u32>,
    tile_cache_mb: Option<u32>,
    crs_override: Option<String>,
    format_hint: Option<DatasetFormat>,
}

impl DatasetOpenBuilder {
    /// Create a new builder targeting the given `path`.
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            read_only: true,
            overview_level: None,
            tile_cache_mb: None,
            crs_override: None,
            format_hint: None,
        }
    }

    /// Set whether the dataset should be opened read-only (default: `true`).
    ///
    /// When `false` the dataset is opened for read-write access.  Not all
    /// drivers support write access, and those that do not will return an error
    /// from `.open()`.
    #[must_use]
    pub fn read_only(mut self, val: bool) -> Self {
        self.read_only = val;
        self
    }

    /// Request a specific overview / pyramid level (0 = native resolution).
    ///
    /// Higher values access lower-resolution overviews, which is significantly
    /// faster for display and thumbnail generation.
    #[must_use]
    pub fn with_overview_level(mut self, level: u32) -> Self {
        self.overview_level = Some(level);
        self
    }

    /// Set the tile/block cache size in megabytes.
    ///
    /// A larger cache reduces disk I/O when reading many tiles.
    #[must_use]
    pub fn with_tile_cache_mb(mut self, mb: u32) -> Self {
        self.tile_cache_mb = Some(mb);
        self
    }

    /// Override the CRS reported by the file.
    ///
    /// `wkt` can be an EPSG code string (`"EPSG:4326"`), a WKT2 string, or a
    /// PROJ definition string.  This is useful when the file is missing CRS
    /// metadata.
    #[must_use]
    pub fn with_crs_override(mut self, wkt: impl Into<String>) -> Self {
        self.crs_override = Some(wkt.into());
        self
    }

    /// Provide a format hint to skip magic-byte detection.
    ///
    /// Only needed for files with non-standard or missing extensions.
    #[must_use]
    pub fn with_format_hint(mut self, format: DatasetFormat) -> Self {
        self.format_hint = Some(format);
        self
    }

    // ── accessors (for inspection / testing) ─────────────────────────────────

    /// The configured path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether read-only mode is enabled.
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Configured overview level, if any.
    pub fn overview_level(&self) -> Option<u32> {
        self.overview_level
    }

    /// Configured tile cache size in MB, if any.
    pub fn tile_cache_mb(&self) -> Option<u32> {
        self.tile_cache_mb
    }

    /// Configured CRS override string, if any.
    pub fn crs_override(&self) -> Option<&str> {
        self.crs_override.as_deref()
    }

    // ── terminal method ───────────────────────────────────────────────────────

    /// Open the dataset with the configured options.
    ///
    /// Internally calls [`open()`] for format detection, then applies the
    /// configured options to the returned handle.
    ///
    /// # Errors
    ///
    /// Propagates any error from [`open()`].  Additionally returns
    /// [`OxiGdalError::NotSupported`] if `read_only = false` is requested for
    /// a format that is currently read-only.
    pub fn open(self) -> Result<OpenedDataset> {
        // Perform the actual format detection and file opening
        let opened = open(&self.path)?;

        // Apply CRS override if present — currently stored in info.
        // Full driver integration would pass these options to the driver.
        // For now we return the dataset as-is; options are validated here.
        if !self.read_only {
            // Validate that the format supports write access.
            // GeoTIFF and GeoJSON do; others are read-only stubs.
            match opened.format() {
                DatasetFormat::GeoTiff | DatasetFormat::GeoJson => {}
                fmt => {
                    return Err(OxiGdalError::NotSupported {
                        operation: format!(
                            "Write access for format '{}' is not yet supported",
                            fmt.driver_name()
                        ),
                    });
                }
            }
        }

        Ok(opened)
    }
}

// ─── DatasetCreateBuilder ─────────────────────────────────────────────────────

/// Configuration snapshot captured by [`DatasetCreateBuilder`].
///
/// Stored inside [`DatasetWriter`] for later inspection.
#[derive(Debug, Clone)]
pub struct CreateOptions {
    /// Output format
    pub format: OutputFormat,
    /// CRS string (EPSG code, WKT2, or PROJ definition)
    pub crs: Option<String>,
    /// Compression algorithm
    pub compression: CompressionType,
    /// Tile / block size `(width, height)` in pixels
    pub tile_size: Option<(u32, u32)>,
    /// Number of decimal places for vector coordinate precision
    pub decimal_precision: Option<u8>,
    /// Nodata value (for raster outputs)
    pub nodata: Option<f64>,
    /// Predictor for LZW/DEFLATE (1 = none, 2 = horizontal, 3 = floating-point)
    pub predictor: Option<u8>,
}

impl CreateOptions {
    fn default_for(format: OutputFormat) -> Self {
        Self {
            format,
            crs: None,
            compression: CompressionType::None,
            tile_size: None,
            decimal_precision: None,
            nodata: None,
            predictor: None,
        }
    }
}

/// Builder for creating / configuring a new geospatial dataset for writing.
///
/// Uses the fluent / method-chaining pattern.  Finalise with `.create()`.
///
/// # Example
///
/// ```rust,no_run
/// use oxigdal::builder::{DatasetCreateBuilder, OutputFormat, CompressionType};
///
/// # fn main() -> oxigdal::Result<()> {
/// let writer = DatasetCreateBuilder::new("/tmp/cog.tif", OutputFormat::GeoTiff)
///     .with_crs("EPSG:32654")
///     .with_compression(CompressionType::Zstd)
///     .with_tile_size(512, 512)
///     .with_decimal_precision(6)
///     .create()?;
/// println!("path: {}", writer.path().display());
/// println!("format: {}", writer.format());
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct DatasetCreateBuilder {
    path: PathBuf,
    options: CreateOptions,
}

impl DatasetCreateBuilder {
    /// Create a new builder writing to `path` in the given `format`.
    pub fn new(path: impl AsRef<Path>, format: OutputFormat) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            options: CreateOptions::default_for(format),
        }
    }

    /// Set the coordinate reference system.
    ///
    /// `epsg_or_wkt` can be `"EPSG:4326"`, a WKT2 string, or a PROJ string.
    #[must_use]
    pub fn with_crs(mut self, epsg_or_wkt: impl Into<String>) -> Self {
        self.options.crs = Some(epsg_or_wkt.into());
        self
    }

    /// Set the compression algorithm.
    #[must_use]
    pub fn with_compression(mut self, compression: CompressionType) -> Self {
        self.options.compression = compression;
        self
    }

    /// Set the tile / block size for raster outputs (in pixels).
    ///
    /// Typically `(256, 256)` or `(512, 512)`.
    #[must_use]
    pub fn with_tile_size(mut self, width: u32, height: u32) -> Self {
        self.options.tile_size = Some((width, height));
        self
    }

    /// Set the number of decimal places for vector coordinate precision.
    ///
    /// Only meaningful for text-based vector formats (GeoJSON, etc.).
    #[must_use]
    pub fn with_decimal_precision(mut self, decimals: u8) -> Self {
        self.options.decimal_precision = Some(decimals);
        self
    }

    /// Set the nodata / fill value for raster outputs.
    #[must_use]
    pub fn with_nodata(mut self, nodata: f64) -> Self {
        self.options.nodata = Some(nodata);
        self
    }

    /// Set the TIFF predictor (1 = none, 2 = horizontal, 3 = floating-point).
    ///
    /// Only meaningful for LZW and DEFLATE compressed GeoTIFFs.
    #[must_use]
    pub fn with_predictor(mut self, predictor: u8) -> Self {
        self.options.predictor = Some(predictor);
        self
    }

    // ── accessors ─────────────────────────────────────────────────────────────

    /// The configured output path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The configured output format.
    pub fn format(&self) -> OutputFormat {
        self.options.format
    }

    /// The configured options snapshot.
    pub fn options(&self) -> &CreateOptions {
        &self.options
    }

    // ── validation ────────────────────────────────────────────────────────────

    fn validate(&self) -> Result<()> {
        // tile_size: both dimensions must be non-zero
        if let Some((w, h)) = self.options.tile_size {
            if w == 0 || h == 0 {
                return Err(OxiGdalError::InvalidParameter {
                    parameter: "tile_size",
                    message: format!("tile dimensions must be non-zero, got ({w}, {h})"),
                });
            }
        }

        // predictor: only valid values are 1, 2, 3
        if let Some(p) = self.options.predictor {
            if p == 0 || p > 3 {
                return Err(OxiGdalError::InvalidParameter {
                    parameter: "predictor",
                    message: format!(
                        "predictor must be 1 (none), 2 (horizontal), or 3 (float), got {p}"
                    ),
                });
            }
        }

        // JPEG compression is only sensible for GeoTIFF
        if self.options.compression == CompressionType::Jpeg
            && self.options.format != OutputFormat::GeoTiff
        {
            return Err(OxiGdalError::NotSupported {
                operation: format!(
                    "JPEG compression is only supported for GeoTIFF, not '{}'",
                    self.options.format
                ),
            });
        }

        Ok(())
    }

    // ── terminal method ───────────────────────────────────────────────────────

    /// Validate options and create a [`DatasetWriter`] handle.
    ///
    /// Does **not** create the output file yet — that is the driver's
    /// responsibility once the user starts writing data.
    ///
    /// # Errors
    ///
    /// Returns [`OxiGdalError::InvalidParameter`] for invalid option
    /// combinations (e.g., zero tile size).
    pub fn create(self) -> Result<DatasetWriter> {
        self.validate()?;
        let path = self.path.clone();
        let options = self.options.clone();
        Ok(DatasetWriter {
            path,
            options,
            width: None,
            height: None,
            band_count: None,
            data_type: None,
            geo_transform: None,
            bands: Vec::new(),
            finalized: false,
        })
    }
}

// ─── DatasetWriter ────────────────────────────────────────────────────────────

/// Handle returned by [`DatasetCreateBuilder::create`].
///
/// Carries the validated path and creation options.  Raster dimensions,
/// data type, and geo-transform are configured after construction; then
/// bands are written via [`write_band`] or [`write_all_bands`], and the
/// output is finalised with [`finalize`].
///
/// [`write_band`]: DatasetWriter::write_band
/// [`write_all_bands`]: DatasetWriter::write_all_bands
/// [`finalize`]: DatasetWriter::finalize
#[derive(Debug)]
pub struct DatasetWriter {
    path: PathBuf,
    options: CreateOptions,
    /// Raster dimensions (set via `set_dimensions`).
    width: Option<u32>,
    height: Option<u32>,
    band_count: Option<u32>,
    /// Pixel data type.
    data_type: Option<RasterDataType>,
    /// Spatial positioning.
    geo_transform: Option<GeoTransform>,
    /// Per-band byte buffers (band index 1-based, stored 0-based).
    bands: Vec<Vec<u8>>,
    /// Whether `finalize()` has been called.
    finalized: bool,
}

impl DatasetWriter {
    /// Output file path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Output format.
    pub fn format(&self) -> OutputFormat {
        self.options.format
    }

    /// The full set of creation options.
    pub fn options(&self) -> &CreateOptions {
        &self.options
    }

    /// CRS string, if configured.
    pub fn crs(&self) -> Option<&str> {
        self.options.crs.as_deref()
    }

    /// Compression type.
    pub fn compression(&self) -> CompressionType {
        self.options.compression
    }

    /// Tile size `(width, height)`, if configured.
    pub fn tile_size(&self) -> Option<(u32, u32)> {
        self.options.tile_size
    }

    /// Decimal precision for vector coordinates, if configured.
    pub fn decimal_precision(&self) -> Option<u8> {
        self.options.decimal_precision
    }

    // ── raster configuration ──────────────────────────────────────────────────

    /// Set the raster dimensions and number of bands.
    ///
    /// Must be called before `write_band` or `write_all_bands`.
    ///
    /// # Errors
    ///
    /// Returns an error if any dimension is zero.
    pub fn set_dimensions(&mut self, width: u32, height: u32, band_count: u32) -> Result<()> {
        if width == 0 || height == 0 || band_count == 0 {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "dimensions",
                message: format!(
                    "all dimensions must be non-zero, got ({width} x {height} x {band_count})"
                ),
            });
        }
        self.width = Some(width);
        self.height = Some(height);
        self.band_count = Some(band_count);
        Ok(())
    }

    /// Set the pixel data type.
    pub fn set_data_type(&mut self, data_type: RasterDataType) {
        self.data_type = Some(data_type);
    }

    /// Set the geo-transform (spatial positioning).
    pub fn set_geo_transform(&mut self, gt: GeoTransform) {
        self.geo_transform = Some(gt);
    }

    /// Configured raster width (pixels), if set.
    pub fn width(&self) -> Option<u32> {
        self.width
    }

    /// Configured raster height (pixels), if set.
    pub fn height(&self) -> Option<u32> {
        self.height
    }

    /// Configured number of bands, if set.
    pub fn band_count(&self) -> Option<u32> {
        self.band_count
    }

    /// Configured data type, if set.
    pub fn data_type(&self) -> Option<RasterDataType> {
        self.data_type
    }

    /// Configured geo-transform, if set.
    pub fn geo_transform(&self) -> Option<&GeoTransform> {
        self.geo_transform.as_ref()
    }

    // ── write operations ──────────────────────────────────────────────────────

    /// Write raw bytes for a single band.
    ///
    /// `band` is **1-based** (as in GDAL convention).  The data length must
    /// equal `width × height × data_type.size_bytes()`.
    ///
    /// # Errors
    ///
    /// Returns an error if dimensions/data-type are not yet configured, the
    /// band index is out of range, or the data length is wrong.
    pub fn write_band(&mut self, band: u32, data: &[u8]) -> Result<()> {
        if self.finalized {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "finalized",
                message: "cannot write after finalize".to_string(),
            });
        }
        let (w, h, bc, dt) = self.require_raster_config()?;
        if band == 0 || band > bc {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "band",
                message: format!("band index {band} out of range [1, {bc}]"),
            });
        }
        let expected = w as usize * h as usize * dt.size_bytes();
        if data.len() != expected {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "data",
                message: format!(
                    "data length {} does not match expected {} ({w}×{h}×{})",
                    data.len(),
                    expected,
                    dt.size_bytes()
                ),
            });
        }

        // Ensure band vector is large enough
        let idx = (band - 1) as usize;
        if self.bands.len() <= idx {
            self.bands.resize_with(idx + 1, Vec::new);
        }
        self.bands[idx] = data.to_vec();
        Ok(())
    }

    /// Write all bands at once from a contiguous BSQ (band-sequential) buffer.
    ///
    /// The data length must equal `width × height × band_count × data_type.size_bytes()`.
    ///
    /// # Errors
    ///
    /// Returns an error if dimensions/data-type are not yet configured or the
    /// data length is wrong.
    pub fn write_all_bands(&mut self, data: &[u8]) -> Result<()> {
        if self.finalized {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "finalized",
                message: "cannot write after finalize".to_string(),
            });
        }
        let (w, h, bc, dt) = self.require_raster_config()?;
        let band_bytes = w as usize * h as usize * dt.size_bytes();
        let total = band_bytes * bc as usize;
        if data.len() != total {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "data",
                message: format!(
                    "data length {} does not match expected {} ({w}×{h}×{bc}×{})",
                    data.len(),
                    total,
                    dt.size_bytes()
                ),
            });
        }
        self.bands.clear();
        for i in 0..bc as usize {
            let start = i * band_bytes;
            self.bands.push(data[start..start + band_bytes].to_vec());
        }
        Ok(())
    }

    /// Finalise the dataset, writing to disk.
    ///
    /// Currently supports GeoJSON (empty feature collection) and stores raw
    /// raster bytes for other formats.  Future versions will dispatch to
    /// format-specific driver crates.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be created.
    pub fn finalize(&mut self) -> Result<()> {
        if self.finalized {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "finalized",
                message: "already finalized".to_string(),
            });
        }
        self.finalized = true;

        match self.options.format {
            OutputFormat::GeoJson => {
                // Write a minimal GeoJSON FeatureCollection
                let precision = self.options.decimal_precision.unwrap_or(6);
                let content = format!(
                    "{{\"type\":\"FeatureCollection\",\"features\":[],\"metadata\":{{\"crs\":{crs},\"precision\":{precision}}}}}",
                    crs = match &self.options.crs {
                        Some(c) => format!("\"{c}\""),
                        None => "null".to_string(),
                    },
                );
                std::fs::write(&self.path, content.as_bytes())
                    .map_err(|e| OxiGdalError::io_error(e.to_string()))?;
            }
            _ => {
                // For raster formats: write a simple binary format (header + bands)
                // This is a placeholder; real drivers will produce GeoTIFF/etc.
                if !self.bands.is_empty() {
                    let mut buf = Vec::new();
                    // Simple header: magic + dimensions
                    buf.extend_from_slice(b"OXIG"); // magic
                    let w = self.width.unwrap_or(0);
                    let h = self.height.unwrap_or(0);
                    let bc = self.band_count.unwrap_or(0);
                    let dt = self.data_type.unwrap_or_default() as u8;
                    buf.extend_from_slice(&w.to_le_bytes());
                    buf.extend_from_slice(&h.to_le_bytes());
                    buf.extend_from_slice(&bc.to_le_bytes());
                    buf.push(dt);
                    // Band data
                    for band in &self.bands {
                        buf.extend_from_slice(band);
                    }
                    std::fs::write(&self.path, &buf)
                        .map_err(|e| OxiGdalError::io_error(e.to_string()))?;
                }
            }
        }
        Ok(())
    }

    /// Whether this writer has been finalized.
    pub fn is_finalized(&self) -> bool {
        self.finalized
    }

    // ── helpers ───────────────────────────────────────────────────────────────

    fn require_raster_config(&self) -> Result<(u32, u32, u32, RasterDataType)> {
        let w = self.width.ok_or_else(|| OxiGdalError::InvalidParameter {
            parameter: "width",
            message: "dimensions not set; call set_dimensions() first".to_string(),
        })?;
        let h = self.height.ok_or_else(|| OxiGdalError::InvalidParameter {
            parameter: "height",
            message: "dimensions not set; call set_dimensions() first".to_string(),
        })?;
        let bc = self
            .band_count
            .ok_or_else(|| OxiGdalError::InvalidParameter {
                parameter: "band_count",
                message: "dimensions not set; call set_dimensions() first".to_string(),
            })?;
        let dt = self
            .data_type
            .ok_or_else(|| OxiGdalError::InvalidParameter {
                parameter: "data_type",
                message: "data type not set; call set_data_type() first".to_string(),
            })?;
        Ok((w, h, bc, dt))
    }
}

impl core::fmt::Display for DatasetWriter {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "DatasetWriter {{ path: {}, format: {}, compression: {} }}",
            self.path.display(),
            self.options.format,
            self.options.compression,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_temp_geojson(name: &str) -> PathBuf {
        let dir = std::env::temp_dir();
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).expect("create");
        f.write_all(b"{}").expect("write");
        path
    }

    // ── DatasetOpenBuilder ────────────────────────────────────────────────────

    #[test]
    fn test_open_builder_default_read_only() {
        let builder = DatasetOpenBuilder::new("world.tif");
        assert!(builder.is_read_only());
    }

    #[test]
    fn test_open_builder_set_read_only_false() {
        let builder = DatasetOpenBuilder::new("world.tif").read_only(false);
        assert!(!builder.is_read_only());
    }

    #[test]
    fn test_open_builder_stores_overview_level() {
        let builder = DatasetOpenBuilder::new("world.tif").with_overview_level(3);
        assert_eq!(builder.overview_level(), Some(3));
    }

    #[test]
    fn test_open_builder_stores_tile_cache_mb() {
        let builder = DatasetOpenBuilder::new("world.tif").with_tile_cache_mb(256);
        assert_eq!(builder.tile_cache_mb(), Some(256));
    }

    #[test]
    fn test_open_builder_stores_crs_override() {
        let builder = DatasetOpenBuilder::new("world.tif").with_crs_override("EPSG:4326");
        assert_eq!(builder.crs_override(), Some("EPSG:4326"));
    }

    #[test]
    fn test_open_builder_chaining() {
        let builder = DatasetOpenBuilder::new("world.tif")
            .read_only(true)
            .with_overview_level(2)
            .with_tile_cache_mb(64)
            .with_crs_override("EPSG:32654");
        assert!(builder.is_read_only());
        assert_eq!(builder.overview_level(), Some(2));
        assert_eq!(builder.tile_cache_mb(), Some(64));
        assert_eq!(builder.crs_override(), Some("EPSG:32654"));
    }

    #[test]
    fn test_open_builder_opens_existing_file() {
        let path = make_temp_geojson("builder_open_test.geojson");
        let result = DatasetOpenBuilder::new(&path).read_only(true).open();
        assert!(result.is_ok(), "should open existing file: {result:?}");
    }

    #[test]
    fn test_open_builder_nonexistent_file_errors() {
        let result = DatasetOpenBuilder::new("/nonexistent/data.tif").open();
        assert!(result.is_err());
    }

    #[test]
    fn test_open_builder_write_unsupported_format_errors() {
        let path = make_temp_geojson("builder_write_fgb.fgb");
        let result = DatasetOpenBuilder::new(&path).read_only(false).open();
        // FlatGeobuf is read-only stub; expect error
        assert!(result.is_err(), "write on unsupported format should error");
    }

    // ── DatasetCreateBuilder ──────────────────────────────────────────────────

    #[test]
    fn test_create_builder_stores_format() {
        let path = std::env::temp_dir().join("oxigdal_out_test.tif");
        let builder = DatasetCreateBuilder::new(&path, OutputFormat::GeoTiff);
        assert_eq!(builder.format(), OutputFormat::GeoTiff);
    }

    #[test]
    fn test_create_builder_stores_crs() {
        let path = std::env::temp_dir().join("oxigdal_out_test.tif");
        let builder = DatasetCreateBuilder::new(&path, OutputFormat::GeoTiff).with_crs("EPSG:4326");
        assert_eq!(builder.options().crs.as_deref(), Some("EPSG:4326"));
    }

    #[test]
    fn test_create_builder_stores_compression() {
        let path = std::env::temp_dir().join("oxigdal_out_test.tif");
        let builder = DatasetCreateBuilder::new(&path, OutputFormat::GeoTiff)
            .with_compression(CompressionType::Zstd);
        assert_eq!(builder.options().compression, CompressionType::Zstd);
    }

    #[test]
    fn test_create_builder_stores_tile_size() {
        let path = std::env::temp_dir().join("oxigdal_out_test.tif");
        let builder =
            DatasetCreateBuilder::new(&path, OutputFormat::GeoTiff).with_tile_size(512, 512);
        assert_eq!(builder.options().tile_size, Some((512, 512)));
    }

    #[test]
    fn test_create_builder_stores_decimal_precision() {
        let path = std::env::temp_dir().join("oxigdal_out_test.geojson");
        let builder =
            DatasetCreateBuilder::new(&path, OutputFormat::GeoJson).with_decimal_precision(7);
        assert_eq!(builder.options().decimal_precision, Some(7));
    }

    #[test]
    fn test_create_builder_zero_tile_size_error() {
        let path = std::env::temp_dir().join("oxigdal_out_test.tif");
        let result = DatasetCreateBuilder::new(&path, OutputFormat::GeoTiff)
            .with_tile_size(0, 256)
            .create();
        assert!(result.is_err(), "zero tile width should fail validation");
    }

    #[test]
    fn test_create_builder_invalid_predictor_error() {
        let path = std::env::temp_dir().join("oxigdal_out_test.tif");
        let result = DatasetCreateBuilder::new(&path, OutputFormat::GeoTiff)
            .with_predictor(5)
            .create();
        assert!(result.is_err(), "predictor 5 is invalid");
    }

    #[test]
    fn test_create_builder_jpeg_non_geotiff_error() {
        let path = std::env::temp_dir().join("oxigdal_out_test.geojson");
        let result = DatasetCreateBuilder::new(&path, OutputFormat::GeoJson)
            .with_compression(CompressionType::Jpeg)
            .create();
        assert!(result.is_err(), "JPEG compression on GeoJSON should fail");
    }

    #[test]
    fn test_create_builder_valid_create() {
        let path = std::env::temp_dir().join("oxigdal_valid_out_test.tif");
        let writer = DatasetCreateBuilder::new(&path, OutputFormat::GeoTiff)
            .with_crs("EPSG:4326")
            .with_compression(CompressionType::Deflate)
            .with_tile_size(256, 256)
            .create()
            .expect("valid create");
        assert_eq!(writer.format(), OutputFormat::GeoTiff);
        assert_eq!(writer.crs(), Some("EPSG:4326"));
        assert_eq!(writer.compression(), CompressionType::Deflate);
        assert_eq!(writer.tile_size(), Some((256, 256)));
    }

    // ── OutputFormat helpers ──────────────────────────────────────────────────

    #[test]
    fn test_output_format_driver_name() {
        assert_eq!(OutputFormat::GeoTiff.driver_name(), "GTiff");
        assert_eq!(OutputFormat::GeoJson.driver_name(), "GeoJSON");
        assert_eq!(OutputFormat::GeoPackage.driver_name(), "GPKG");
    }

    #[test]
    fn test_output_format_default_extension() {
        assert_eq!(OutputFormat::GeoTiff.default_extension(), "tif");
        assert_eq!(OutputFormat::GeoJson.default_extension(), "geojson");
        assert_eq!(OutputFormat::GeoPackage.default_extension(), "gpkg");
    }

    #[test]
    fn test_compression_type_tag_names() {
        assert_eq!(CompressionType::None.tag_name(), "NONE");
        assert_eq!(CompressionType::Deflate.tag_name(), "DEFLATE");
        assert_eq!(CompressionType::Lzw.tag_name(), "LZW");
        assert_eq!(CompressionType::Zstd.tag_name(), "ZSTD");
        assert_eq!(CompressionType::Lz4.tag_name(), "LZ4");
    }

    #[test]
    fn test_dataset_writer_display() {
        let path = std::env::temp_dir().join("oxigdal_disp_test.tif");
        let writer = DatasetCreateBuilder::new(&path, OutputFormat::GeoTiff)
            .with_compression(CompressionType::Lzw)
            .create()
            .expect("create");
        let s = writer.to_string();
        assert!(s.contains("GTiff"), "display should contain format: {s}");
        assert!(s.contains("LZW"), "display should contain compression: {s}");
    }

    // ── DatasetWriter write operations ────────────────────────────────────────

    #[test]
    fn test_writer_set_dimensions() {
        let path = std::env::temp_dir().join("oxigdal_w_test.tif");
        let mut w = DatasetCreateBuilder::new(&path, OutputFormat::GeoTiff)
            .create()
            .expect("create");
        w.set_dimensions(256, 256, 3).expect("set dims");
        assert_eq!(w.width(), Some(256));
        assert_eq!(w.height(), Some(256));
        assert_eq!(w.band_count(), Some(3));
    }

    #[test]
    fn test_writer_zero_dimensions_error() {
        let path = std::env::temp_dir().join("oxigdal_w_test.tif");
        let mut w = DatasetCreateBuilder::new(&path, OutputFormat::GeoTiff)
            .create()
            .expect("create");
        assert!(w.set_dimensions(0, 256, 1).is_err());
        assert!(w.set_dimensions(256, 0, 1).is_err());
        assert!(w.set_dimensions(256, 256, 0).is_err());
    }

    #[test]
    fn test_writer_write_band_requires_config() {
        let path = std::env::temp_dir().join("oxigdal_w_test.tif");
        let mut w = DatasetCreateBuilder::new(&path, OutputFormat::GeoTiff)
            .create()
            .expect("create");
        let data = vec![0u8; 4];
        assert!(w.write_band(1, &data).is_err(), "no dimensions set");
    }

    #[test]
    fn test_writer_write_band_validates_size() {
        let path = std::env::temp_dir().join("oxigdal_w_test.tif");
        let mut w = DatasetCreateBuilder::new(&path, OutputFormat::GeoTiff)
            .create()
            .expect("create");
        w.set_dimensions(2, 2, 1).expect("dims");
        w.set_data_type(RasterDataType::UInt8);
        // Correct size = 2*2*1 = 4
        assert!(w.write_band(1, &[0u8; 3]).is_err(), "wrong size");
        assert!(w.write_band(1, &[0u8; 4]).is_ok(), "correct size");
    }

    #[test]
    fn test_writer_write_band_validates_index() {
        let path = std::env::temp_dir().join("oxigdal_w_test.tif");
        let mut w = DatasetCreateBuilder::new(&path, OutputFormat::GeoTiff)
            .create()
            .expect("create");
        w.set_dimensions(2, 2, 2).expect("dims");
        w.set_data_type(RasterDataType::UInt8);
        assert!(w.write_band(0, &[0u8; 4]).is_err(), "band 0 invalid");
        assert!(w.write_band(3, &[0u8; 4]).is_err(), "band 3 out of range");
        assert!(w.write_band(1, &[0u8; 4]).is_ok());
        assert!(w.write_band(2, &[0u8; 4]).is_ok());
    }

    #[test]
    fn test_writer_write_all_bands() {
        let path = std::env::temp_dir().join("oxigdal_w_test.tif");
        let mut w = DatasetCreateBuilder::new(&path, OutputFormat::GeoTiff)
            .create()
            .expect("create");
        w.set_dimensions(2, 2, 3).expect("dims");
        w.set_data_type(RasterDataType::UInt8);
        // 2*2*3 = 12 bytes
        assert!(w.write_all_bands(&[0u8; 12]).is_ok());
        assert!(w.write_all_bands(&[0u8; 11]).is_err(), "wrong size");
    }

    #[test]
    fn test_writer_finalize_geojson() {
        let dir = std::env::temp_dir();
        let path = dir.join("writer_finalize_test.geojson");
        let mut w = DatasetCreateBuilder::new(&path, OutputFormat::GeoJson)
            .with_crs("EPSG:4326")
            .with_decimal_precision(6)
            .create()
            .expect("create");
        w.finalize().expect("finalize");
        assert!(w.is_finalized());
        let content = std::fs::read_to_string(&path).expect("read");
        assert!(content.contains("FeatureCollection"));
        assert!(content.contains("EPSG:4326"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_writer_finalize_raster() {
        let dir = std::env::temp_dir();
        let path = dir.join("writer_finalize_raster_test.bin");
        let mut w = DatasetCreateBuilder::new(&path, OutputFormat::GeoTiff)
            .create()
            .expect("create");
        w.set_dimensions(2, 2, 1).expect("dims");
        w.set_data_type(RasterDataType::UInt8);
        w.write_band(1, &[10, 20, 30, 40]).expect("write band");
        w.finalize().expect("finalize");
        assert!(w.is_finalized());

        let bytes = std::fs::read(&path).expect("read");
        // Header: OXIG (4) + width (4) + height (4) + band_count (4) + dt (1) = 17
        assert!(bytes.len() >= 17 + 4);
        assert_eq!(&bytes[0..4], b"OXIG");
        // Band data starts at offset 17
        assert_eq!(&bytes[17..21], &[10, 20, 30, 40]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_writer_double_finalize_error() {
        let dir = std::env::temp_dir();
        let path = dir.join("writer_double_finalize.geojson");
        let mut w = DatasetCreateBuilder::new(&path, OutputFormat::GeoJson)
            .create()
            .expect("create");
        w.finalize().expect("finalize");
        assert!(w.finalize().is_err(), "double finalize should error");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_writer_write_after_finalize_error() {
        let dir = std::env::temp_dir();
        let path = dir.join("writer_write_after_fin.geojson");
        let mut w = DatasetCreateBuilder::new(&path, OutputFormat::GeoJson)
            .create()
            .expect("create");
        w.set_dimensions(2, 2, 1).expect("dims");
        w.set_data_type(RasterDataType::UInt8);
        w.finalize().expect("finalize");
        assert!(
            w.write_band(1, &[0u8; 4]).is_err(),
            "write after finalize should error"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_writer_geo_transform() {
        let path = std::env::temp_dir().join("oxigdal_w_test.tif");
        let mut w = DatasetCreateBuilder::new(&path, OutputFormat::GeoTiff)
            .create()
            .expect("create");
        let gt = GeoTransform::north_up(100.0, 50.0, 0.001, 0.001);
        w.set_geo_transform(gt);
        assert!(w.geo_transform().is_some());
    }
}
