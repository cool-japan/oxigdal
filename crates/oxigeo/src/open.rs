//! Universal dataset opener with automatic format detection.
//!
//! This module provides the [`open()`] function and [`OpenedDataset`] enum for
//! ergonomic access to geospatial datasets without needing to know the format
//! in advance.
//!
//! # Detection Order
//!
//! 1. URL scheme: `s3://`, `gs://`, `az://` → cloud storage paths
//! 2. Magic bytes: reads first 16 bytes to identify binary formats
//! 3. File extension fallback: `.tif`, `.geojson`, `.shp`, etc.
//!
//! # Examples
//!
//! ```rust,no_run
//! use oxigeo::open::open;
//!
//! # fn main() -> oxigeo::Result<()> {
//! let dataset = open("elevation.tif")?;
//! match dataset {
//!     oxigeo::open::OpenedDataset::GeoTiff(info) => {
//!         println!("GeoTIFF: {}×{}", info.width.unwrap_or(0), info.height.unwrap_or(0));
//!     }
//!     _ => {}
//! }
//! # Ok(())
//! # }
//! ```

use std::path::{Path, PathBuf};

use oxigeo_core::error::{IoError, OxiGeoError};

use crate::{DatasetFormat, DatasetInfo, Result};

// ─── Cloud-scheme detection ──────────────────────────────────────────────────

/// Detect if the path string uses a cloud storage URL scheme.
///
/// Returns `Some(scheme)` for `s3://`, `gs://`, `az://`, etc.
fn detect_cloud_scheme(path_str: &str) -> Option<CloudScheme> {
    if path_str.starts_with("s3://") {
        Some(CloudScheme::S3)
    } else if path_str.starts_with("gs://") {
        Some(CloudScheme::Gcs)
    } else if path_str.starts_with("az://") || path_str.starts_with("abfs://") {
        Some(CloudScheme::Azure)
    } else if path_str.starts_with("http://") || path_str.starts_with("https://") {
        Some(CloudScheme::Http)
    } else {
        None
    }
}

/// Cloud storage URL scheme.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloudScheme {
    /// Amazon S3 (`s3://`)
    S3,
    /// Google Cloud Storage (`gs://`)
    Gcs,
    /// Azure Blob Storage (`az://` or `abfs://`)
    Azure,
    /// HTTP/HTTPS remote file
    Http,
}

// ─── Magic-byte detection ─────────────────────────────────────────────────────

/// Result of reading and classifying the magic bytes from a file.
#[derive(Debug, Clone, PartialEq, Eq)]
enum MagicDetectionResult {
    /// Matched a known binary format
    Detected(DatasetFormat),
    /// Could not determine format from magic bytes
    Unknown,
}

/// Read up to `n` bytes from the beginning of a file, returning fewer if the
/// file is shorter.
fn read_magic_bytes(path: &Path, n: usize) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|e| {
        OxiGeoError::Io(IoError::Read {
            message: format!("cannot open '{}': {e}", path.display()),
        })
    })?;
    let mut buf = vec![0u8; n];
    let read_bytes = file.read(&mut buf).map_err(|e| {
        OxiGeoError::Io(IoError::Read {
            message: format!("cannot read magic bytes from '{}': {e}", path.display()),
        })
    })?;
    buf.truncate(read_bytes);
    Ok(buf)
}

/// Return `true` when a `.json` file looks like a STAC document.
///
/// STAC Items, ItemCollections, Catalogs, and Collections all carry the
/// `"stac_version"` field.  We read a small prefix (4 KiB) of the file and
/// look for that key without full JSON parsing — cheap and allocation-light.
///
/// The prefix is decoded with [`String::from_utf8_lossy`] rather than a strict
/// [`std::str::from_utf8`]: a fixed-size read can land in the middle of a
/// multi-byte UTF-8 sequence, and a strict decode of such a prefix fails
/// wholesale — which previously made a perfectly valid STAC document containing
/// non-ASCII text (a `title`, a `description`, …) fall through to the GeoJSON
/// branch.
fn is_stac_json(path: &Path) -> bool {
    use std::io::Read as _;
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut buf = vec![0u8; 4096];
    let Ok(n) = file.read(&mut buf) else {
        return false;
    };
    buf.truncate(n);
    let text = String::from_utf8_lossy(&buf);
    text.contains("\"stac_version\"") || text.contains("\"stac_extensions\"")
}

/// Attempt to detect the dataset format by inspecting magic bytes.
///
/// This is a thin wrapper around [`DatasetFormat::detect_from_magic_bytes`].
fn detect_from_magic(path: &Path) -> Result<MagicDetectionResult> {
    use crate::magic::MAGIC_READ_SIZE;
    let buf = read_magic_bytes(path, MAGIC_READ_SIZE)?;

    match DatasetFormat::detect_from_magic_bytes(&buf) {
        Some(fmt) => Ok(MagicDetectionResult::Detected(fmt)),
        None => Ok(MagicDetectionResult::Unknown),
    }
}

// ─── OpenedDataset ────────────────────────────────────────────────────────────

/// Handle returned by [`open()`], wrapping the detected dataset type and its
/// basic metadata.
///
/// Each variant carries a [`DatasetInfo`] with the path, format, geometry
/// extents, CRS, etc.  Additional format-specific operations are delegated to
/// the corresponding driver crates.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum OpenedDataset {
    /// A GeoTIFF (or Cloud-Optimized GeoTIFF) raster dataset.
    GeoTiff(DatasetInfo),
    /// A GeoJSON vector dataset.
    GeoJson(DatasetInfo),
    /// An ESRI Shapefile vector dataset.
    Shapefile(DatasetInfo),
    /// A GeoPackage (SQLite-based) vector/raster dataset.
    GeoPackage(DatasetInfo),
    /// A GeoParquet columnar vector dataset.
    GeoParquet(DatasetInfo),
    /// A NetCDF scientific dataset.
    NetCdf(DatasetInfo),
    /// An HDF5 hierarchical dataset.
    Hdf5(DatasetInfo),
    /// A Zarr cloud-native array dataset.
    Zarr(DatasetInfo),
    /// A GRIB/GRIB2 meteorological dataset.
    Grib(DatasetInfo),
    /// A FlatGeobuf vector dataset.
    FlatGeobuf(DatasetInfo),
    /// A JPEG2000 raster dataset.
    Jpeg2000(DatasetInfo),
    /// A Virtual Raster Tiles (VRT) dataset.
    Vrt(DatasetInfo),
    /// A STAC catalog entry.
    Stac(DatasetInfo),
    /// A dataset residing on cloud storage (s3://, gs://, az://).
    Cloud {
        /// The cloud URL scheme that was detected.
        scheme: CloudScheme,
        /// Path / URL as originally provided.
        path: PathBuf,
        /// Best-guess format based on the URL path extension, if any.
        guessed_format: DatasetFormat,
    },
    /// An unknown / unrecognised format.
    Unknown(DatasetInfo),
}

impl OpenedDataset {
    /// Return the [`DatasetInfo`] for this dataset, if available.
    ///
    /// Returns `None` only for the [`OpenedDataset::Cloud`] variant (the
    /// metadata cannot be fetched without a network call).
    pub fn info(&self) -> Option<&DatasetInfo> {
        match self {
            Self::GeoTiff(i)
            | Self::GeoJson(i)
            | Self::Shapefile(i)
            | Self::GeoPackage(i)
            | Self::GeoParquet(i)
            | Self::NetCdf(i)
            | Self::Hdf5(i)
            | Self::Zarr(i)
            | Self::Grib(i)
            | Self::FlatGeobuf(i)
            | Self::Jpeg2000(i)
            | Self::Vrt(i)
            | Self::Stac(i)
            | Self::Unknown(i) => Some(i),
            Self::Cloud { .. } => None,
        }
    }

    /// Return the detected [`DatasetFormat`].
    pub fn format(&self) -> DatasetFormat {
        match self {
            Self::GeoTiff(_) => DatasetFormat::GeoTiff,
            Self::GeoJson(_) => DatasetFormat::GeoJson,
            Self::Shapefile(_) => DatasetFormat::Shapefile,
            Self::GeoPackage(_) => DatasetFormat::GeoPackage,
            Self::GeoParquet(_) => DatasetFormat::GeoParquet,
            Self::NetCdf(_) => DatasetFormat::NetCdf,
            Self::Hdf5(_) => DatasetFormat::Hdf5,
            Self::Zarr(_) => DatasetFormat::Zarr,
            Self::Grib(_) => DatasetFormat::Grib,
            Self::FlatGeobuf(_) => DatasetFormat::FlatGeobuf,
            Self::Jpeg2000(_) => DatasetFormat::Jpeg2000,
            Self::Vrt(_) => DatasetFormat::Vrt,
            Self::Stac(_) => DatasetFormat::Stac,
            Self::Cloud { guessed_format, .. } => *guessed_format,
            Self::Unknown(_) => DatasetFormat::Unknown,
        }
    }

    /// Whether this dataset is a cloud-hosted remote resource.
    pub fn is_cloud(&self) -> bool {
        matches!(self, Self::Cloud { .. })
    }

    /// Whether the detected format is a raster format.
    pub fn is_raster(&self) -> bool {
        matches!(
            self,
            Self::GeoTiff(_)
                | Self::Jpeg2000(_)
                | Self::NetCdf(_)
                | Self::Hdf5(_)
                | Self::Zarr(_)
                | Self::Grib(_)
                | Self::Vrt(_)
        )
    }

    /// Whether the detected format is a vector format.
    pub fn is_vector(&self) -> bool {
        matches!(
            self,
            Self::GeoJson(_)
                | Self::Shapefile(_)
                | Self::GeoPackage(_)
                | Self::GeoParquet(_)
                | Self::FlatGeobuf(_)
                | Self::Stac(_)
        )
    }
}

// ─── GeoPackage in DatasetFormat ─────────────────────────────────────────────
// NOTE: DatasetFormat doesn't yet have GeoPackage — we handle it by mapping
// both SQLite and ZIP magic to a new variant.  For now we tunnel it through
// the Unknown variant at the DatasetFormat level and carry the real enum
// in OpenedDataset directly.

// ─── Public API ───────────────────────────────────────────────────────────────

/// Universal dataset opener with automatic format detection.
///
/// Detection order:
/// 1. **URL scheme**: `s3://`, `gs://`, `az://`, `http://` → cloud/remote
/// 2. **Magic bytes**: reads the first 16 bytes for binary format signatures
///    (TIFF, JP2, HDF5, NetCDF, ZIP/GPKG, SQLite/GPKG)
/// 3. **File extension fallback**: `.tif`, `.geojson`, `.shp`, `.gpkg`, etc.
///
/// # Errors
///
/// Returns [`OxiGeoError::Io`] if the file cannot be read.
/// Returns [`OxiGeoError::NotSupported`] if the format cannot be determined.
///
/// # Examples
///
/// ```rust,no_run
/// use oxigeo::open::open;
///
/// # fn main() -> oxigeo::Result<()> {
/// let dataset = open("world.tif")?;
/// println!("format: {}", dataset.format());
/// # Ok(())
/// # }
/// ```
pub fn open(path: impl AsRef<Path>) -> Result<OpenedDataset> {
    let path_ref = path.as_ref();
    // Lossy rather than `unwrap_or("")`: a path that is not valid UTF-8 still
    // has a usable scheme prefix and file extension, and collapsing it to the
    // empty string silently classified every such file as `Unknown`.
    let path_str = path_ref.to_string_lossy().into_owned();

    // 1 — Cloud/remote URL scheme check (no filesystem access needed)
    if let Some(scheme) = detect_cloud_scheme(&path_str) {
        let guessed_format = DatasetFormat::from_extension(&path_str);
        return Ok(OpenedDataset::Cloud {
            scheme,
            path: path_ref.to_path_buf(),
            guessed_format,
        });
    }

    // 2 — Verify the file exists before doing anything else
    if !path_ref.exists() {
        return Err(OxiGeoError::Io(IoError::NotFound {
            path: path_str.clone(),
        }));
    }

    // 3 — Detect from magic bytes
    let magic_result = detect_from_magic(path_ref)?;

    // Resolve the final DatasetFormat — magic takes priority over extension,
    // but for ZIP/SQLite we refine with the extension (GPKG vs ZIP plain).
    let format = match magic_result {
        MagicDetectionResult::Detected(fmt) => {
            // For ZIP-based formats, cross-check with extension to tell GPKG from generic ZIP
            if fmt == DatasetFormat::GeoPackage {
                let ext_fmt = DatasetFormat::from_extension(&path_str);
                match ext_fmt {
                    DatasetFormat::Unknown => DatasetFormat::GeoPackage,
                    other => other,
                }
            } else {
                fmt
            }
        }
        MagicDetectionResult::Unknown => {
            // 4 — Fall back to extension
            let ext_fmt = DatasetFormat::from_extension(&path_str);
            if ext_fmt == DatasetFormat::Unknown {
                // Special-case: .json might be GeoJSON or STAC.
                // Peek at content: STAC items/collections carry "stac_version".
                let ext = path_ref
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(str::to_lowercase)
                    .unwrap_or_default();
                if ext == "json" {
                    if is_stac_json(path_ref) {
                        DatasetFormat::Stac
                    } else {
                        DatasetFormat::GeoJson
                    }
                } else {
                    DatasetFormat::Unknown
                }
            } else {
                ext_fmt
            }
        }
    };

    let info = build_dataset_info(path_ref, format)?;
    let opened = map_format_to_opened(format, info);
    Ok(opened)
}

/// Build a [`DatasetInfo`] for the given path and detected format.
///
/// For every format that has a header probe the probe is *authoritative*: when
/// it fails, the failure is propagated as a typed [`OxiGeoError`] instead of
/// being collapsed into an all-zero `DatasetInfo`.  Reporting `0×0`,
/// `band_count = 0` for a file that is really an 8000×8000 single-band raster
/// is worse than an error — it makes the caller's own validation
/// (`bands().next().ok_or("no bands")?`) fire with a nonsense diagnosis and
/// makes `width()`/`height()` silently wrong (cool-japan/oxigeo#14).
///
/// Formats without a probe (`NetCdf`, `Hdf5`, `Zarr`, `Grib`, `Jpeg2000`,
/// `Vrt`, tile archives, `Unknown`, …) still yield an empty descriptor — that
/// is "nothing was parsed", not "a parse failed", and every unknown field is
/// honestly `None`.
///
/// # Errors
///
/// Propagates the probe error for [`DatasetFormat::GeoTiff`],
/// [`DatasetFormat::GeoJson`], [`DatasetFormat::Shapefile`],
/// [`DatasetFormat::FlatGeobuf`], [`DatasetFormat::GeoParquet`] and
/// [`DatasetFormat::GeoPackage`].
fn build_dataset_info(path: &Path, format: DatasetFormat) -> Result<DatasetInfo> {
    let path_str = path.to_str().map(str::to_string);

    // Attempt lightweight header parsing for formats we understand.
    let mut info = match format {
        DatasetFormat::GeoTiff => extract_tiff_info(path)?,
        DatasetFormat::GeoJson => extract_geojson_info(path)?,
        #[cfg(feature = "shapefile")]
        DatasetFormat::Shapefile => extract_shapefile_info(path)?,
        #[cfg(feature = "flatgeobuf")]
        DatasetFormat::FlatGeobuf => extract_flatgeobuf_info(path)?,
        #[cfg(feature = "geoparquet")]
        DatasetFormat::GeoParquet => extract_geoparquet_info(path)?,
        #[cfg(feature = "gpkg")]
        DatasetFormat::GeoPackage => extract_gpkg_info(path)?,
        other => DatasetInfo {
            format: other,
            ..DatasetInfo::default()
        },
    };
    info.path = path_str;
    Ok(info)
}

// ─── GeoTIFF header probe ────────────────────────────────────────────────────

/// Wrap a GeoTIFF header-parsing failure in a typed, self-describing error.
///
/// Used for every stage of [`extract_tiff_info`] so that a file which *looks*
/// like a GeoTIFF (magic bytes matched) but whose metadata cannot be recovered
/// surfaces as an error rather than as an all-zero [`DatasetInfo`].
#[cfg(feature = "geotiff")]
fn tiff_header_error(path: &Path, detail: impl core::fmt::Display) -> OxiGeoError {
    OxiGeoError::Format(oxigeo_core::error::FormatError::InvalidHeader {
        message: format!(
            "'{}' was detected as a GeoTIFF but its metadata could not be extracted: {detail}",
            path.display()
        ),
    })
}

/// Parse a GeoTIFF header and extract dataset-level metadata.
///
/// # Why this delegates to the real TIFF parser
///
/// This probe used to hand-roll an IFD parse over a fixed **8 KiB** window read
/// from the front of the file.  That is only correct for files whose first IFD
/// happens to sit inside those 8 KiB.  TIFF places no such requirement on
/// writers, and in particular a writer that emits *pixel data first and the IFD
/// last* — which is exactly what OxiGeo's own
/// [`oxigeo_geotiff::GeoTiffWriter`] does — puts the IFD megabytes into the
/// file.  For every such file the old probe found no IFD entries at all and
/// silently reported `0×0` with `band_count = 0`, so `Dataset::bands()` yielded
/// nothing and `Dataset::width()`/`height()` returned `0` with no error raised
/// anywhere (cool-japan/oxigeo#14).  OxiGeo could not correctly re-open a
/// GeoTIFF it had just written.
///
/// Rather than widen the window (there is no window that is always big enough —
/// the IFD offset is a full `u32`/`u64`), the probe now uses the crate's real
/// TIFF reader, which seeks to the recorded IFD offset like any conformant
/// reader.  This is still a *header-only* operation: [`GeoTiffReader::open`]
/// parses the IFD chain and the block-offset arrays via ranged reads and does
/// not touch, let alone decode, any pixel data.
///
/// # Errors
///
/// Returns [`OxiGeoError::Io`] when the file cannot be opened, and
/// [`OxiGeoError::Format`] when the TIFF structure cannot be parsed or declares
/// a degenerate geometry (zero width/height/band count, or dimensions that do
/// not fit the `u32` fields of [`DatasetInfo`]).  It never returns a
/// zero-filled `DatasetInfo`.
#[cfg(feature = "geotiff")]
pub(crate) fn extract_tiff_info(path: &Path) -> Result<DatasetInfo> {
    use oxigeo_core::io::FileDataSource;
    use oxigeo_geotiff::GeoTiffReader;

    // `FileDataSource::open` already produces a typed `OxiGeoError::Io` that
    // names the path, so propagate it unchanged.
    let source = FileDataSource::open(path)?;
    let reader = GeoTiffReader::open(source).map_err(|e| tiff_header_error(path, e))?;

    let width_u64 = reader.width();
    let height_u64 = reader.height();
    let width = tiff_dimension(path, width_u64, "width")?;
    let height = tiff_dimension(path, height_u64, "height")?;

    let band_count = reader.band_count();
    if band_count == 0 {
        return Err(tiff_header_error(
            path,
            "the IFD declares SamplesPerPixel = 0, so the file has no raster bands",
        ));
    }

    let geotransform = reader.geo_transform().copied();

    // EPSG code 0 is not a real authority code — treat it as "absent" rather
    // than emitting the meaningless string "EPSG:0".
    let crs = reader
        .epsg_code()
        .filter(|&code| code != 0)
        .map(|code| format!("EPSG:{code}"));

    // Derive the bounding box the same way `gdalinfo` derives its "Corner
    // Coordinates": from the geo-transform and the raster extent.
    let bounds = geotransform.map(|gt| gt.compute_bounds(width_u64, height_u64));

    Ok(DatasetInfo {
        format: DatasetFormat::GeoTiff,
        path: None, // populated by callers that know the path
        width: Some(width),
        height: Some(height),
        band_count,
        layer_count: 0,
        crs,
        geotransform,
        feature_count: None,
        bounds,
        // `None` only when the BitsPerSample / SampleFormat combination has no
        // `RasterDataType` equivalent; the raster itself is still openable.
        data_type: reader.data_type(),
    })
}

/// Validate one raster dimension read from a TIFF header and narrow it to the
/// `u32` used by [`DatasetInfo`].
///
/// Rejects `0` (a degenerate raster that would reproduce the very "silent
/// zeros" this probe exists to prevent) and values that do not fit in `u32`
/// (silently truncating a BigTIFF dimension would corrupt every derived
/// window / bounds computation).
#[cfg(feature = "geotiff")]
fn tiff_dimension(path: &Path, value: u64, what: &str) -> Result<u32> {
    if value == 0 {
        return Err(tiff_header_error(
            path,
            format!("the IFD declares a raster {what} of 0 pixels"),
        ));
    }
    u32::try_from(value).map_err(|_| {
        tiff_header_error(
            path,
            format!("the IFD declares a raster {what} of {value} pixels, which exceeds u32"),
        )
    })
}

/// Stub used when the `geotiff` feature is disabled.
///
/// Without the feature the crate has no TIFF parser at all, so the only honest
/// answer is a typed error naming the missing feature — never a zero-filled
/// [`DatasetInfo`].
///
/// # Errors
///
/// Always returns [`OxiGeoError::NotSupported`].
#[cfg(not(feature = "geotiff"))]
pub(crate) fn extract_tiff_info(path: &Path) -> Result<DatasetInfo> {
    Err(OxiGeoError::NotSupported {
        operation: format!(
            "'{}' was detected as a GeoTIFF but the 'geotiff' feature is disabled — \
             enable it to read GeoTIFF metadata",
            path.display()
        ),
    })
}

// ─── VRT header probe ────────────────────────────────────────────────────────

/// Parse a VRT's XML header into a [`DatasetInfo`].
///
/// Every `.vrt` used to route through the `_ =>` arm of `open_raster`, which
/// produces a **zero-filled** descriptor: `width()`/`height()` returned `0`,
/// `band_count()` returned `0` and `geotransform()` returned `None`, for a file
/// whose whole point is to state exactly those things in plain XML
/// (cool-japan/oxigeo#15).  A caller that asked a VRT for its geotransform got
/// silence, not an error.
///
/// This is a header-only operation: [`oxigeo_vrt::VrtReader::open`] parses the
/// XML and validates the structure without reading a pixel from any source
/// file.
///
/// # Errors
///
/// Returns [`OxiGeoError::Format`] when the XML cannot be parsed or describes a
/// degenerate raster.  It never returns a zero-filled `DatasetInfo`.
#[cfg(feature = "vrt")]
pub(crate) fn extract_vrt_info(path: &Path) -> Result<DatasetInfo> {
    let reader = oxigeo_vrt::VrtReader::open(path).map_err(|e| vrt_header_error(path, e))?;

    let width_u64 = reader.width();
    let height_u64 = reader.height();
    let width = vrt_dimension(path, width_u64, "width")?;
    let height = vrt_dimension(path, height_u64, "height")?;

    let band_count = u32::try_from(reader.band_count())
        .map_err(|_| vrt_header_error(path, "it declares more bands than u32 can hold"))?;
    if band_count == 0 {
        return Err(vrt_header_error(path, "it declares no raster bands"));
    }

    let geotransform = reader.geo_transform().copied();
    let bounds = geotransform.map(|gt| gt.compute_bounds(width_u64, height_u64));

    Ok(DatasetInfo {
        format: DatasetFormat::Vrt,
        path: None, // populated by callers that know the path
        width: Some(width),
        height: Some(height),
        band_count,
        layer_count: 0,
        crs: reader.srs().map(str::to_string),
        geotransform,
        feature_count: None,
        bounds,
        data_type: reader.primary_data_type(),
    })
}

/// Build the typed error a VRT header probe failure produces.
#[cfg(feature = "vrt")]
fn vrt_header_error(path: &Path, detail: impl core::fmt::Display) -> OxiGeoError {
    OxiGeoError::Format(oxigeo_core::error::FormatError::InvalidHeader {
        message: format!(
            "'{}' was detected as a VRT but its metadata could not be extracted: {detail}",
            path.display()
        ),
    })
}

/// Validate one raster dimension declared by a VRT and narrow it to `u32`.
#[cfg(feature = "vrt")]
fn vrt_dimension(path: &Path, value: u64, what: &str) -> Result<u32> {
    if value == 0 {
        return Err(vrt_header_error(
            path,
            format!("it declares a raster {what} of 0 pixels"),
        ));
    }
    u32::try_from(value).map_err(|_| {
        vrt_header_error(
            path,
            format!("it declares a raster {what} of {value} pixels, which exceeds u32"),
        )
    })
}

// ─── GeoJSON lightweight sniffing ────────────────────────────────────────────

/// Size of the prefix read by [`extract_geojson_info`].
///
/// A GeoJSON `FeatureCollection` puts `"type"` and (when present) the
/// collection-level `"bbox"` near the front of the document, so a bounded peek
/// is enough to classify the file and recover its extent without streaming
/// gigabytes at open time.
const GEOJSON_PEEK_BYTES: usize = 65536;

/// Read the first few kilobytes of a GeoJSON file and try to extract the
/// collection-level bbox and feature count.
///
/// This is intentionally approximate (string-level scanning) to avoid running a
/// full JSON parse over a potentially multi-gigabyte document at open time.
///
/// The feature count is only reported when the *whole* document fitted in the
/// peek window.  Counting `"type":"Feature"` occurrences inside a truncated
/// prefix yields a number that is silently too small — and a wrong count is
/// worse than no count, so a document larger than the window reports
/// `feature_count = None` ("not cheaply countable"), which is exactly what that
/// field is documented to mean.
///
/// Exposed as `pub(crate)` so that `lib.rs` can reuse this logic without
/// duplicating it.
///
/// # Errors
///
/// Returns [`OxiGeoError::Io`] when the file cannot be opened or read.  A file
/// that simply is not a `FeatureCollection` is not an error — it yields an
/// honest descriptor with `layer_count = 0`.
pub(crate) fn extract_geojson_info(path: &Path) -> Result<DatasetInfo> {
    use std::io::Read;
    // Read a larger chunk — GeoJSON bbox may appear after features array header
    let mut file = std::fs::File::open(path).map_err(|e| {
        OxiGeoError::Io(IoError::Read {
            message: format!("cannot open GeoJSON '{}': {e}", path.display()),
        })
    })?;
    // Read one byte more than the peek window so that "the document ended
    // inside the window" can be distinguished from "the document was cut off".
    let mut buf = vec![0u8; GEOJSON_PEEK_BYTES + 1];
    let mut filled = 0usize;
    loop {
        match file.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => {
                filled += n;
                if filled == buf.len() {
                    break;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => {
                return Err(OxiGeoError::Io(IoError::Read {
                    message: format!("cannot read GeoJSON '{}': {e}", path.display()),
                }));
            }
        }
    }
    let complete = filled <= GEOJSON_PEEK_BYTES;
    buf.truncate(filled.min(GEOJSON_PEEK_BYTES));
    // A fixed-size read can split a multi-byte UTF-8 sequence; a strict decode
    // of such a prefix fails wholesale and would drop *all* metadata for an
    // otherwise perfectly valid document that merely contains non-ASCII text.
    let text = String::from_utf8_lossy(&buf);

    // Very lightweight: check if it's a FeatureCollection
    let is_collection = text.contains("\"FeatureCollection\"");
    let layer_count = if is_collection { 1 } else { 0 };

    // Count features: count occurrences of `"type":"Feature"` or `"type": "Feature"`
    let feature_count = if is_collection && complete {
        let count = count_geojson_features(&text);
        if count > 0 { Some(count as u64) } else { None }
    } else {
        None
    };

    // Extract top-level bbox if present: `"bbox":[minx,miny,maxx,maxy]`
    let bounds = extract_geojson_bbox(&text);

    Ok(DatasetInfo {
        format: DatasetFormat::GeoJson,
        path: None, // populated by callers that know the path
        layer_count,
        feature_count,
        bounds,
        ..DatasetInfo::default()
    })
}

/// Count `"type":"Feature"` occurrences in a GeoJSON text snippet.
///
/// Exact for a complete document; callers must not use it on a truncated
/// prefix (see [`extract_geojson_info`]).
fn count_geojson_features(text: &str) -> usize {
    // Accept both `"type":"Feature"` and `"type": "Feature"` variants.
    let mut count = 0usize;
    let needle1 = "\"type\":\"Feature\"";
    let needle2 = "\"type\": \"Feature\"";
    let mut pos = 0;
    while pos < text.len() {
        if let Some(idx) = text[pos..].find(needle1) {
            count += 1;
            pos += idx + needle1.len();
        } else if let Some(idx) = text[pos..].find(needle2) {
            count += 1;
            pos += idx + needle2.len();
        } else {
            break;
        }
    }
    count
}

/// Try to parse `"bbox":[minx,miny,maxx,maxy]` from a GeoJSON text snippet.
fn extract_geojson_bbox(text: &str) -> Option<crate::BoundingBox> {
    // Locate `"bbox":[`
    let start = text.find("\"bbox\":")?;
    let after_key = &text[start + 7..]; // skip `"bbox":`
    let bracket = after_key.find('[')? + 1;
    let inner_start = bracket;
    let inner_end = after_key.find(']')?;
    if inner_end <= inner_start {
        return None;
    }
    let inner = &after_key[inner_start..inner_end];
    // Parse comma-separated floats
    let nums: Vec<f64> = inner
        .split(',')
        .filter_map(|s| s.trim().parse::<f64>().ok())
        .collect();
    // GeoJSON bbox is [west, south, east, north] for 2D
    if nums.len() >= 4 {
        crate::BoundingBox::new(nums[0], nums[1], nums[2], nums[3]).ok()
    } else {
        None
    }
}

// ─── Shapefile lightweight header parsing ────────────────────────────────────

/// Parse the Shapefile header (.shp) and optionally .shx to populate
/// `feature_count` and `bounds`.
///
/// Exposed as `pub(crate)` so that `lib.rs` can use it directly in `open_vector`.
///
/// # Errors
///
/// Returns [`OxiGeoError::Format`] when the `.shp` header cannot be parsed, so
/// that a corrupt or truncated shapefile is reported instead of being collapsed
/// into an all-zero [`DatasetInfo`].
#[cfg(feature = "shapefile")]
pub(crate) fn extract_shapefile_info(path: &Path) -> Result<DatasetInfo> {
    // Strip any extension from path to get the base path for ShapefileReader::open
    let base = path.with_extension("");
    let reader = oxigeo_shapefile::ShapefileReader::open(&base).map_err(|e| {
        OxiGeoError::Format(oxigeo_core::error::FormatError::InvalidHeader {
            message: format!(
                "'{}' was detected as an ESRI Shapefile but its header could not be read: {e}",
                path.display()
            ),
        })
    })?;

    let header = reader.header();
    let bbox = &header.bbox;

    // Feature count comes from the .shx index when available.
    // If the .shx was not loaded, we cannot infer the count without a full scan.
    let feature_count = reader.index_entries().map(|entries| entries.len() as u64);

    let bounds = crate::BoundingBox::new(bbox.x_min, bbox.y_min, bbox.x_max, bbox.y_max).ok();

    let crs = reader.crs().map(str::to_string);

    Ok(DatasetInfo {
        format: DatasetFormat::Shapefile,
        path: None,
        layer_count: 1,
        crs,
        feature_count,
        bounds,
        ..DatasetInfo::default()
    })
}

// ─── FlatGeobuf lightweight header parsing ───────────────────────────────────

/// Parse the FlatGeobuf header to populate `feature_count` and `bounds`.
///
/// Exposed as `pub(crate)` so that `lib.rs` can use it directly in `open_vector`.
///
/// # Errors
///
/// Returns [`OxiGeoError::Io`] when the file cannot be opened and
/// [`OxiGeoError::Format`] when the FlatGeobuf header cannot be parsed.
#[cfg(feature = "flatgeobuf")]
pub(crate) fn extract_flatgeobuf_info(path: &Path) -> Result<DatasetInfo> {
    use std::io::BufReader;
    let file = std::fs::File::open(path).map_err(|e| {
        OxiGeoError::Io(IoError::Read {
            message: format!("cannot open FlatGeobuf '{}': {e}", path.display()),
        })
    })?;
    let reader = oxigeo_flatgeobuf::FlatGeobufReader::new(BufReader::new(file)).map_err(|e| {
        OxiGeoError::Format(oxigeo_core::error::FormatError::InvalidHeader {
            message: format!(
                "'{}' was detected as FlatGeobuf but its header could not be read: {e}",
                path.display()
            ),
        })
    })?;
    let header = reader.header();

    let feature_count = header.features_count;

    let bounds = header.extent.and_then(|ext| {
        // ext = [min_x, min_y, max_x, max_y]
        crate::BoundingBox::new(ext[0], ext[1], ext[2], ext[3]).ok()
    });

    let crs = header
        .crs
        .as_ref()
        .and_then(|c| c.organization_code)
        .map(|code| format!("EPSG:{code}"));

    Ok(DatasetInfo {
        format: DatasetFormat::FlatGeobuf,
        path: None,
        layer_count: 1,
        crs,
        feature_count,
        bounds,
        ..DatasetInfo::default()
    })
}

// ─── GeoParquet lightweight metadata parsing ─────────────────────────────────

/// Parse the GeoParquet file metadata to populate `feature_count` and `bounds`.
///
/// Exposed as `pub(crate)` so that `lib.rs` can use it directly in `open_vector`.
///
/// # Errors
///
/// Returns [`OxiGeoError::Format`] when the Parquet footer / GeoParquet
/// metadata cannot be read.
#[cfg(feature = "geoparquet")]
pub(crate) fn extract_geoparquet_info(path: &Path) -> Result<DatasetInfo> {
    let reader = oxigeo_geoparquet::GeoParquetReader::open(path).map_err(|e| {
        OxiGeoError::Format(oxigeo_core::error::FormatError::InvalidHeader {
            message: format!(
                "'{}' was detected as GeoParquet but its metadata could not be read: {e}",
                path.display()
            ),
        })
    })?;

    let feature_count = {
        let n = reader.num_rows();
        if n >= 0 { Some(n as u64) } else { None }
    };

    let bounds = {
        let meta = reader.metadata();
        meta.columns
            .get(&meta.primary_column)
            .and_then(|col| col.bbox.as_ref())
            .filter(|bbox| bbox.len() >= 4)
            .and_then(|bbox| crate::BoundingBox::new(bbox[0], bbox[1], bbox[2], bbox[3]).ok())
    };

    Ok(DatasetInfo {
        format: DatasetFormat::GeoParquet,
        path: None,
        layer_count: 1,
        feature_count,
        bounds,
        ..DatasetInfo::default()
    })
}

// ─── GeoPackage metadata probe ───────────────────────────────────────────────

/// Open and parse the SQLite container behind a `.gpkg` path.
///
/// Shared by the metadata probe below and by the layer readers in
/// [`crate::layer`], so that both report identical errors for an unreadable or
/// non-GeoPackage file.
///
/// # Errors
///
/// Returns [`OxiGeoError::Io`] when the file cannot be read and
/// [`OxiGeoError::Format`] when it is not a parseable SQLite/GeoPackage
/// container.
#[cfg(feature = "gpkg")]
pub(crate) fn open_geopackage(path: &str) -> Result<oxigeo_gpkg::GeoPackage> {
    let data = std::fs::read(path).map_err(|e| {
        OxiGeoError::Io(IoError::Read {
            message: format!("cannot read GeoPackage '{path}': {e}"),
        })
    })?;

    oxigeo_gpkg::GeoPackage::from_bytes(data).map_err(|e| {
        OxiGeoError::Format(oxigeo_core::error::FormatError::InvalidHeader {
            message: format!(
                "'{path}' was detected as a GeoPackage but its SQLite container \
                 could not be parsed: {e}"
            ),
        })
    })
}

/// Read the GeoPackage system tables and describe the package.
///
/// `layer_count` is the number of `gpkg_contents` rows of type `features`, and
/// `feature_count` / `bounds` / `crs` describe the **first** of those layers —
/// the "primary layer" convention the rest of [`DatasetInfo`] uses.  Per-layer
/// metadata is available from [`Dataset::layers`](crate::Dataset::layers).
///
/// Before this probe existed, a `.gpkg` opened through the facade produced an
/// empty descriptor — `layer_count = 0` for a package that clearly had layers
/// (cool-japan/oxigeo#16).
///
/// # Errors
///
/// Returns [`OxiGeoError::Format`] when the container or its `gpkg_contents`
/// table cannot be read, rather than reporting an all-zero descriptor for a
/// file that really is a GeoPackage.
#[cfg(feature = "gpkg")]
pub(crate) fn extract_gpkg_info(path: &Path) -> Result<DatasetInfo> {
    use oxigeo_gpkg::GpkgDataType;

    let path_str = path.to_string_lossy().to_string();
    let mut gpkg = open_geopackage(&path_str)?;

    gpkg.load_contents().map_err(|e| {
        OxiGeoError::Format(oxigeo_core::error::FormatError::InvalidHeader {
            message: format!(
                "'{}' was detected as a GeoPackage but its gpkg_contents table \
                 could not be read: {e}",
                path.display()
            ),
        })
    })?;

    let feature_layers: Vec<&oxigeo_gpkg::GpkgContents> = gpkg
        .contents
        .iter()
        .filter(|entry| entry.data_type == GpkgDataType::Features)
        .collect();

    let primary = feature_layers.first();

    let feature_count = match primary {
        Some(entry) => gpkg.count_table_rows(&entry.table_name).map_err(|e| {
            OxiGeoError::Format(oxigeo_core::error::FormatError::InvalidHeader {
                message: format!(
                    "cannot count rows of feature table '{}' in '{}': {e}",
                    entry.table_name,
                    path.display()
                ),
            })
        })?,
        None => None,
    };

    let bounds = primary.and_then(|entry| {
        crate::BoundingBox::new(entry.min_x, entry.min_y, entry.max_x, entry.max_y).ok()
    });

    // srs_id 0 (undefined geographic) and -1 (undefined cartesian) are the
    // OGC "no CRS" sentinels — reporting them as `EPSG:0` would be a lie.
    let crs = primary
        .filter(|entry| entry.srs_id > 0)
        .map(|entry| format!("EPSG:{}", entry.srs_id));

    Ok(DatasetInfo {
        format: DatasetFormat::GeoPackage,
        path: None,
        layer_count: feature_layers.len() as u32,
        feature_count,
        bounds,
        crs,
        ..DatasetInfo::default()
    })
}

/// Map a resolved [`DatasetFormat`] + [`DatasetInfo`] to the corresponding
/// [`OpenedDataset`] variant.
fn map_format_to_opened(format: DatasetFormat, info: DatasetInfo) -> OpenedDataset {
    match format {
        DatasetFormat::GeoTiff => OpenedDataset::GeoTiff(info),
        DatasetFormat::GeoJson => OpenedDataset::GeoJson(info),
        DatasetFormat::Shapefile => OpenedDataset::Shapefile(info),
        DatasetFormat::GeoParquet => OpenedDataset::GeoParquet(info),
        DatasetFormat::GeoPackage => OpenedDataset::GeoPackage(info),
        DatasetFormat::NetCdf => OpenedDataset::NetCdf(info),
        DatasetFormat::Hdf5 => OpenedDataset::Hdf5(info),
        DatasetFormat::Zarr => OpenedDataset::Zarr(info),
        DatasetFormat::Grib => OpenedDataset::Grib(info),
        DatasetFormat::FlatGeobuf => OpenedDataset::FlatGeobuf(info),
        DatasetFormat::Jpeg2000 => OpenedDataset::Jpeg2000(info),
        DatasetFormat::Vrt => OpenedDataset::Vrt(info),
        DatasetFormat::Stac => OpenedDataset::Stac(info),
        DatasetFormat::PMTiles
        | DatasetFormat::MBTiles
        | DatasetFormat::Copc
        | DatasetFormat::Las
        | DatasetFormat::Terrain
        | DatasetFormat::Unknown => OpenedDataset::Unknown(info),
    }
}

// ─── GeoPackage DatasetFormat extension ──────────────────────────────────────

// We extend `DatasetFormat` (defined in lib.rs) with a `GeoPackage` concept by
// intercepting it here.  Since we cannot add a new variant to the enum in lib.rs
// from this module without touching lib.rs, we handle it purely via
// `OpenedDataset::GeoPackage`.

impl DatasetFormat {
    /// Returns `true` if this format is likely a GeoPackage (GPKG).
    pub fn is_geopackage(path: &Path) -> bool {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_lowercase)
            .unwrap_or_default();
        ext == "gpkg"
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use crate::magic::{HDF5_MAGIC, JP2_MAGIC};
    use std::io::Write;
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
                    .join(format!("oxigeo_open_{}_{seq}_{name}", std::process::id())),
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

    // ── helper: create a temp file with given bytes ──────────────────────────
    fn write_temp_file(name: &str, content: &[u8]) -> TempPath {
        let path = TempPath::new(name);
        let mut f = std::fs::File::create(&path).expect("create temp file");
        f.write_all(content).expect("write temp file");
        path
    }

    // ── cloud scheme detection ────────────────────────────────────────────────

    #[test]
    fn test_cloud_s3_scheme_detected() {
        let result = open("s3://my-bucket/data/world.tif");
        assert!(result.is_ok(), "s3:// should succeed");
        let ds = result.expect("s3 opened");
        assert!(ds.is_cloud(), "should be cloud dataset");
        if let OpenedDataset::Cloud { scheme, .. } = &ds {
            assert_eq!(*scheme, CloudScheme::S3);
        } else {
            panic!("expected Cloud variant");
        }
    }

    #[test]
    fn test_cloud_gs_scheme_detected() {
        let result = open("gs://bucket/raster.tif");
        assert!(result.is_ok());
        let ds = result.expect("gs opened");
        assert!(ds.is_cloud());
        if let OpenedDataset::Cloud { scheme, .. } = &ds {
            assert_eq!(*scheme, CloudScheme::Gcs);
        } else {
            panic!("expected Cloud variant");
        }
    }

    #[test]
    fn test_cloud_az_scheme_detected() {
        let result = open("az://container/layer.gpkg");
        assert!(result.is_ok());
        let ds = result.expect("az opened");
        assert!(ds.is_cloud());
    }

    #[test]
    fn test_cloud_http_scheme_detected() {
        let result = open("https://example.com/layer.geojson");
        assert!(result.is_ok());
        let ds = result.expect("https opened");
        assert!(ds.is_cloud());
        if let OpenedDataset::Cloud { scheme, .. } = &ds {
            assert_eq!(*scheme, CloudScheme::Http);
        } else {
            panic!("expected Cloud variant");
        }
    }

    #[test]
    fn test_cloud_guessed_format_from_extension() {
        let result = open("s3://bucket/elevation.tif").expect("open");
        if let OpenedDataset::Cloud { guessed_format, .. } = result {
            assert_eq!(guessed_format, DatasetFormat::GeoTiff);
        } else {
            panic!("expected Cloud");
        }
    }

    // ── non-existent file ─────────────────────────────────────────────────────

    #[test]
    fn test_open_nonexistent_file_returns_io_error() {
        let result = open("/nonexistent/path/file.tif");
        assert!(result.is_err(), "nonexistent file should error");
        let err = result.expect_err("should be error");
        assert!(
            matches!(err, OxiGeoError::Io(IoError::NotFound { .. })),
            "expected NotFound, got {err:?}"
        );
    }

    // ── magic-byte detection ──────────────────────────────────────────────────

    #[test]
    fn test_magic_tiff_little_endian() {
        // A real (if minimal) little-endian TIFF: `II` + version 42 + a
        // parseable IFD.  A bare 8-byte header is *not* a TIFF and is covered
        // separately by `test_tiff_header_without_ifd_is_rejected`.
        let path = write_temp_file("test_magic_tiff_le.tif", &build_minimal_tiff_le(8, 4, 1));
        let ds = open(&path).expect("open tiff le");
        assert_eq!(ds.format(), DatasetFormat::GeoTiff);
        assert!(ds.is_raster());
    }

    #[test]
    fn test_magic_tiff_big_endian() {
        // Minimal big-endian TIFF: `MM` + version 42 BE + a parseable IFD.
        let path = write_temp_file("test_magic_tiff_be.tif", &build_minimal_tiff_be(800, 600));
        let ds = open(&path).expect("open tiff be");
        assert_eq!(ds.format(), DatasetFormat::GeoTiff);
    }

    /// A file that carries the TIFF magic but no reachable IFD must be
    /// reported as an error, not as a `0×0` / `band_count = 0` dataset.
    ///
    /// Before the cool-japan/oxigeo#14 fix this returned `Ok` with an all-zero
    /// [`DatasetInfo`], so nothing downstream could tell a corrupt file from a
    /// legitimately empty one.
    #[test]
    fn test_tiff_header_without_ifd_is_rejected() {
        let bytes = [0x49u8, 0x49, 0x2A, 0x00, 0x00, 0x00, 0x00, 0x00];
        let path = write_temp_file("test_magic_tiff_no_ifd.tif", &bytes);
        let err = open(&path).expect_err("header-only TIFF must not open silently");
        assert!(
            matches!(err, OxiGeoError::Format(_)) || matches!(err, OxiGeoError::Io(_)),
            "expected a typed Format/Io error, got {err:?}"
        );
    }

    #[test]
    fn test_magic_hdf5() {
        let path = write_temp_file("test_magic_hdf5.h5", &HDF5_MAGIC);
        let ds = open(&path).expect("open hdf5");
        assert_eq!(ds.format(), DatasetFormat::Hdf5);
        assert!(ds.is_raster());
    }

    #[test]
    fn test_magic_netcdf() {
        // CDF\x01
        let bytes = [0x43u8, 0x44, 0x46, 0x01, 0x00, 0x00, 0x00, 0x00];
        let path = write_temp_file("test_magic_netcdf.nc", &bytes);
        let ds = open(&path).expect("open netcdf");
        assert_eq!(ds.format(), DatasetFormat::NetCdf);
        assert!(ds.is_raster());
    }

    #[test]
    fn test_magic_jp2() {
        let path = write_temp_file("test_magic_jp2.jp2", &JP2_MAGIC);
        let ds = open(&path).expect("open jp2");
        assert_eq!(ds.format(), DatasetFormat::Jpeg2000);
        assert!(ds.is_raster());
    }

    // ── extension fallback ────────────────────────────────────────────────────

    #[test]
    fn test_extension_geojson_fallback() {
        // Plain JSON content — no magic match; extension should take over
        let content = b"{}";
        let path = write_temp_file("test_ext_fallback.geojson", content);
        let ds = open(&path).expect("open geojson");
        assert_eq!(ds.format(), DatasetFormat::GeoJson);
        assert!(ds.is_vector());
    }

    #[test]
    fn test_extension_shapefile_fallback() {
        // `.shp` carries no magic bytes we match on, so the extension decides.
        let content = b"\x00\x00\x27\x0A"; // SHP magic (optional check)
        let path = write_temp_file("test_ext_shapefile.shp", content);
        assert_eq!(
            DatasetFormat::from_extension(&path.to_string_lossy()),
            DatasetFormat::Shapefile
        );
    }

    /// A truncated `.shp` must surface the header failure instead of yielding
    /// an all-zero descriptor that looks like a valid empty layer.
    #[cfg(feature = "shapefile")]
    #[test]
    fn test_truncated_shapefile_is_rejected() {
        let content = b"\x00\x00\x27\x0A";
        let path = write_temp_file("test_truncated_shapefile.shp", content);
        let err = open(&path).expect_err("truncated .shp must not open silently");
        assert!(
            matches!(err, OxiGeoError::Format(_)) || matches!(err, OxiGeoError::Io(_)),
            "expected a typed Format/Io error, got {err:?}"
        );
    }

    #[test]
    fn test_extension_vrt_fallback() {
        let content = b"<VRTDataset />";
        let path = write_temp_file("test_ext_vrt.vrt", content);
        let ds = open(&path).expect("open vrt");
        assert_eq!(ds.format(), DatasetFormat::Vrt);
        assert!(ds.is_raster());
    }

    #[test]
    fn test_extension_grib_fallback() {
        let content = b"GRIB";
        let path = write_temp_file("test_ext_grib.grib", content);
        let ds = open(&path).expect("open grib");
        assert_eq!(ds.format(), DatasetFormat::Grib);
    }

    // ── OpenedDataset helpers ─────────────────────────────────────────────────

    #[test]
    fn test_opened_dataset_not_cloud_for_local() {
        let content = b"{}";
        let path = write_temp_file("test_not_cloud.geojson", content);
        let ds = open(&path).expect("open");
        assert!(!ds.is_cloud());
    }

    #[test]
    fn test_opened_dataset_info_present_for_local() {
        let content = b"{}";
        let path = write_temp_file("test_info_present.geojson", content);
        let ds = open(&path).expect("open");
        assert!(ds.info().is_some(), "local file should have info");
    }

    #[test]
    fn test_is_geopackage_extension_check() {
        let path = Path::new("layer.gpkg");
        assert!(DatasetFormat::is_geopackage(path));
        let path2 = Path::new("world.tif");
        assert!(!DatasetFormat::is_geopackage(path2));
    }

    #[test]
    fn test_format_display_all_variants() {
        assert_eq!(DatasetFormat::GeoTiff.to_string(), "GTiff");
        assert_eq!(DatasetFormat::GeoJson.to_string(), "GeoJSON");
        assert_eq!(DatasetFormat::Shapefile.to_string(), "ESRI Shapefile");
        assert_eq!(DatasetFormat::Hdf5.to_string(), "HDF5");
        assert_eq!(DatasetFormat::Vrt.to_string(), "VRT");
        assert_eq!(DatasetFormat::Unknown.to_string(), "Unknown");
    }

    // ── TIFF metadata extraction ──────────────────────────────────────────────

    /// Build a minimal but valid TIFF file (classic, little-endian) with IFD
    /// entries for ImageWidth, ImageLength, SamplesPerPixel.
    fn build_minimal_tiff_le(width: u32, height: u32, spp: u16) -> Vec<u8> {
        // Header: II (LE), version 42, IFD offset = 8
        let mut buf: Vec<u8> = vec![0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00];
        // IFD at offset 8: 3 entries
        let num_entries: u16 = 3;
        buf.extend_from_slice(&num_entries.to_le_bytes());
        // Entry 1: ImageWidth (tag 256, LONG type 4, count 1, value inline)
        buf.extend_from_slice(&256u16.to_le_bytes()); // tag
        buf.extend_from_slice(&4u16.to_le_bytes()); // type = LONG
        buf.extend_from_slice(&1u32.to_le_bytes()); // count
        buf.extend_from_slice(&width.to_le_bytes()); // value
        // Entry 2: ImageLength (tag 257, LONG type 4, count 1, value inline)
        buf.extend_from_slice(&257u16.to_le_bytes());
        buf.extend_from_slice(&4u16.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&height.to_le_bytes());
        // Entry 3: SamplesPerPixel (tag 277, SHORT type 3, count 1, value inline)
        buf.extend_from_slice(&277u16.to_le_bytes());
        buf.extend_from_slice(&3u16.to_le_bytes()); // type = SHORT
        buf.extend_from_slice(&1u32.to_le_bytes()); // count
        buf.extend_from_slice(&spp.to_le_bytes()); // value
        buf.extend_from_slice(&[0x00, 0x00]); // pad to 4 bytes
        // Next IFD offset = 0 (end)
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf
    }

    /// Build a minimal but valid TIFF file (classic, **big**-endian) with IFD
    /// entries for ImageWidth and ImageLength.
    fn build_minimal_tiff_be(width: u32, height: u32) -> Vec<u8> {
        // MM + version 42 BE + IFD at offset 8
        let mut buf: Vec<u8> = vec![0x4D, 0x4D, 0x00, 0x2A, 0x00, 0x00, 0x00, 0x08];
        let num_entries: u16 = 2;
        buf.extend_from_slice(&num_entries.to_be_bytes());
        // ImageWidth as LONG BE
        buf.extend_from_slice(&256u16.to_be_bytes());
        buf.extend_from_slice(&4u16.to_be_bytes());
        buf.extend_from_slice(&1u32.to_be_bytes());
        buf.extend_from_slice(&width.to_be_bytes());
        // ImageLength as LONG BE
        buf.extend_from_slice(&257u16.to_be_bytes());
        buf.extend_from_slice(&4u16.to_be_bytes());
        buf.extend_from_slice(&1u32.to_be_bytes());
        buf.extend_from_slice(&height.to_be_bytes());
        // Next IFD = 0
        buf.extend_from_slice(&0u32.to_be_bytes());
        buf
    }

    #[test]
    fn test_tiff_metadata_extraction_width_height_bands() {
        let tiff = build_minimal_tiff_le(1024, 768, 3);
        let path = write_temp_file("test_meta_extract.tif", &tiff);
        let ds = open(&path).expect("open tiff");
        let info = ds.info().expect("should have info");
        assert_eq!(info.format, DatasetFormat::GeoTiff);
        assert_eq!(info.width, Some(1024));
        assert_eq!(info.height, Some(768));
        assert_eq!(info.band_count, 3);
    }

    #[test]
    fn test_tiff_metadata_extraction_single_band() {
        let tiff = build_minimal_tiff_le(512, 512, 1);
        let path = write_temp_file("test_meta_extract_1band.tif", &tiff);
        let ds = open(&path).expect("open tiff");
        let info = ds.info().expect("info");
        assert_eq!(info.width, Some(512));
        assert_eq!(info.height, Some(512));
        assert_eq!(info.band_count, 1);
    }

    #[test]
    fn test_tiff_metadata_short_width() {
        // Width stored as SHORT (type 3) instead of LONG
        let mut buf: Vec<u8> = vec![0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00];
        let num_entries: u16 = 2;
        buf.extend_from_slice(&num_entries.to_le_bytes());
        // ImageWidth as SHORT
        buf.extend_from_slice(&256u16.to_le_bytes());
        buf.extend_from_slice(&3u16.to_le_bytes()); // SHORT
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&640u16.to_le_bytes());
        buf.extend_from_slice(&[0x00, 0x00]); // pad
        // ImageLength as SHORT
        buf.extend_from_slice(&257u16.to_le_bytes());
        buf.extend_from_slice(&3u16.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&480u16.to_le_bytes());
        buf.extend_from_slice(&[0x00, 0x00]);
        // Next IFD = 0
        buf.extend_from_slice(&0u32.to_le_bytes());
        let path = write_temp_file("test_meta_short_width.tif", &buf);
        let ds = open(&path).expect("open");
        let info = ds.info().expect("info");
        assert_eq!(info.width, Some(640));
        assert_eq!(info.height, Some(480));
    }

    /// Build a TIFF with GeoTransform (ModelPixelScale + ModelTiepoint).
    fn build_geotiff_with_transform(
        width: u32,
        height: u32,
        origin_x: f64,
        origin_y: f64,
        scale_x: f64,
        scale_y: f64,
    ) -> Vec<u8> {
        // ModelPixelScale data at offset 200: [ScaleX, ScaleY, 0.0]
        // ModelTiepoint data at offset 224: [0, 0, 0, OriginX, OriginY, 0]
        let ps_offset: u32 = 200;
        let tp_offset: u32 = 224;

        let mut buf: Vec<u8> = vec![0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00];
        let num_entries: u16 = 5;
        buf.extend_from_slice(&num_entries.to_le_bytes());
        // Tag 256: ImageWidth
        buf.extend_from_slice(&256u16.to_le_bytes());
        buf.extend_from_slice(&4u16.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&width.to_le_bytes());
        // Tag 257: ImageLength
        buf.extend_from_slice(&257u16.to_le_bytes());
        buf.extend_from_slice(&4u16.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&height.to_le_bytes());
        // Tag 277: SamplesPerPixel
        buf.extend_from_slice(&277u16.to_le_bytes());
        buf.extend_from_slice(&3u16.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.extend_from_slice(&[0x00, 0x00]);
        // Tag 33550: ModelPixelScaleTag (DOUBLE, count=3, offset)
        buf.extend_from_slice(&33550u16.to_le_bytes());
        buf.extend_from_slice(&12u16.to_le_bytes()); // DOUBLE type
        buf.extend_from_slice(&3u32.to_le_bytes());
        buf.extend_from_slice(&ps_offset.to_le_bytes());
        // Tag 33922: ModelTiepointTag (DOUBLE, count=6, offset)
        buf.extend_from_slice(&33922u16.to_le_bytes());
        buf.extend_from_slice(&12u16.to_le_bytes()); // DOUBLE type
        buf.extend_from_slice(&6u32.to_le_bytes());
        buf.extend_from_slice(&tp_offset.to_le_bytes());
        // Next IFD = 0
        buf.extend_from_slice(&0u32.to_le_bytes());
        // Pad to offset 200
        while buf.len() < ps_offset as usize {
            buf.push(0);
        }
        // ModelPixelScale: [ScaleX, ScaleY, 0.0]
        buf.extend_from_slice(&scale_x.to_le_bytes());
        buf.extend_from_slice(&scale_y.to_le_bytes());
        buf.extend_from_slice(&0.0_f64.to_le_bytes());
        // ModelTiepoint: [0, 0, 0, OriginX, OriginY, 0]
        buf.extend_from_slice(&0.0_f64.to_le_bytes());
        buf.extend_from_slice(&0.0_f64.to_le_bytes());
        buf.extend_from_slice(&0.0_f64.to_le_bytes());
        buf.extend_from_slice(&origin_x.to_le_bytes());
        buf.extend_from_slice(&origin_y.to_le_bytes());
        buf.extend_from_slice(&0.0_f64.to_le_bytes());
        buf
    }

    #[test]
    fn test_tiff_geotransform_extraction() {
        let tiff = build_geotiff_with_transform(256, 256, -180.0, 90.0, 0.703125, 0.703125);
        let path = write_temp_file("test_meta_geotransform.tif", &tiff);
        let ds = open(&path).expect("open");
        let info = ds.info().expect("info");
        assert_eq!(info.width, Some(256));
        assert_eq!(info.height, Some(256));
        let gt = info.geotransform.expect("should have geotransform");
        assert!(
            (gt.origin_x - (-180.0)).abs() < 1e-10,
            "origin_x: {}",
            gt.origin_x
        );
        assert!(
            (gt.origin_y - 90.0).abs() < 1e-10,
            "origin_y: {}",
            gt.origin_y
        );
        assert!(
            (gt.pixel_width - 0.703125).abs() < 1e-10,
            "pixel_width: {}",
            gt.pixel_width
        );
    }

    /// Build a TIFF whose georeferencing tag *values* (`ModelPixelScaleTag`,
    /// `ModelTiepointTag`, `GeoKeyDirectoryTag`) are stored well past the
    /// initial 8 KiB peek window used by [`extract_tiff_info`]. Real striped
    /// GeoTIFFs with many rows per strip (e.g. `RowsPerStrip=1`) write large
    /// out-of-line `StripOffsets`/`StripByteCounts` arrays that incidentally
    /// push the small geo-tag value arrays past that window; this fixture
    /// reproduces the same *shape* directly via padding, without needing a
    /// multi-gigabyte source file.
    ///
    /// GeoKeys encode `GTModelTypeGeoKey`=1 (Projected) and
    /// `ProjectedCSTypeGeoKey`=32721 (WGS 84 / UTM zone 21S — a
    /// southern-hemisphere EPSG code), matching the reporter's `buenos.tif`.
    fn build_geotiff_with_far_offset_georeferencing(width: u32, height: u32) -> Vec<u8> {
        // Every out-of-line tag value array starts past the 8 KiB peek window,
        // and is packed contiguously with the next.
        let ps_offset: u32 = 9000;
        let tp_offset: u32 = ps_offset + 24; // ModelPixelScale is 3 DOUBLEs
        let geokey_offset: u32 = tp_offset + 48; // ModelTiepoint is 6 DOUBLEs

        let mut buf: Vec<u8> = vec![0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00];
        let num_entries: u16 = 6;
        buf.extend_from_slice(&num_entries.to_le_bytes());
        // Tag 256: ImageWidth
        buf.extend_from_slice(&256u16.to_le_bytes());
        buf.extend_from_slice(&4u16.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&width.to_le_bytes());
        // Tag 257: ImageLength
        buf.extend_from_slice(&257u16.to_le_bytes());
        buf.extend_from_slice(&4u16.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&height.to_le_bytes());
        // Tag 277: SamplesPerPixel
        buf.extend_from_slice(&277u16.to_le_bytes());
        buf.extend_from_slice(&3u16.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.extend_from_slice(&[0x00, 0x00]);
        // Tag 33550: ModelPixelScaleTag (DOUBLE, count=3, far offset)
        buf.extend_from_slice(&33550u16.to_le_bytes());
        buf.extend_from_slice(&12u16.to_le_bytes()); // DOUBLE type
        buf.extend_from_slice(&3u32.to_le_bytes());
        buf.extend_from_slice(&ps_offset.to_le_bytes());
        // Tag 33922: ModelTiepointTag (DOUBLE, count=6, far offset)
        buf.extend_from_slice(&33922u16.to_le_bytes());
        buf.extend_from_slice(&12u16.to_le_bytes()); // DOUBLE type
        buf.extend_from_slice(&6u32.to_le_bytes());
        buf.extend_from_slice(&tp_offset.to_le_bytes());
        // Tag 34735: GeoKeyDirectoryTag (SHORT, count=12, far offset)
        let geokey_shorts: u32 = 12; // 4-short header + 2 key entries * 4 shorts
        buf.extend_from_slice(&34735u16.to_le_bytes());
        buf.extend_from_slice(&3u16.to_le_bytes()); // SHORT type
        buf.extend_from_slice(&geokey_shorts.to_le_bytes());
        buf.extend_from_slice(&geokey_offset.to_le_bytes());
        // Next IFD = 0
        buf.extend_from_slice(&0u32.to_le_bytes());

        // Pad past the peek window before any tag value data, standing in for
        // the large out-of-line arrays a real striped GeoTIFF would have here.
        while buf.len() < ps_offset as usize {
            buf.push(0);
        }
        // ModelPixelScale: [ScaleX, ScaleY, 0.0] = [0.5, 0.5, 0.0]
        buf.extend_from_slice(&0.5_f64.to_le_bytes());
        buf.extend_from_slice(&0.5_f64.to_le_bytes());
        buf.extend_from_slice(&0.0_f64.to_le_bytes());
        // ModelTiepoint: [I, J, K, X, Y, Z] = [0, 0, 0, 369065.5, 6174601.0, 0]
        buf.extend_from_slice(&0.0_f64.to_le_bytes());
        buf.extend_from_slice(&0.0_f64.to_le_bytes());
        buf.extend_from_slice(&0.0_f64.to_le_bytes());
        buf.extend_from_slice(&369_065.5_f64.to_le_bytes());
        buf.extend_from_slice(&6_174_601.0_f64.to_le_bytes());
        buf.extend_from_slice(&0.0_f64.to_le_bytes());
        // GeoKeyDirectory header [Version=1, Rev=1, MinorRev=0, NumKeys=2]
        // followed by GTModelTypeGeoKey=1 (Projected) and
        // ProjectedCSTypeGeoKey=32721 (WGS 84 / UTM zone 21S).
        debug_assert_eq!(buf.len(), geokey_offset as usize);
        for v in [1u16, 1, 0, 2, 1024, 0, 1, 1, 3072, 0, 1, 32721] {
            buf.extend_from_slice(&v.to_le_bytes());
        }
        buf
    }

    /// Regression test for issue #12: "Metadata missing when reading geotif"
    ///
    /// A real-world GeoTIFF (`buenos.tif`: 16738x18250 px, striped with
    /// `RowsPerStrip=1`, EPSG:32721 = WGS 84 / UTM zone 21S) opened fine —
    /// width/height/band_count were all correct — but `crs()`,
    /// `geotransform()`, and `bounds()` all silently returned `None`, even
    /// though `gdalinfo` proved the tags were present and well-formed.
    ///
    /// Root cause: `extract_tiff_info` reads only the first 8 KiB of the
    /// file, then resolved `ModelPixelScaleTag`/`ModelTiepointTag`/
    /// `GeoKeyDirectoryTag` *values* — which TIFF stores out-of-line at an
    /// absolute file offset — by indexing directly into that 8 KiB buffer.
    /// Any offset past 8 KiB (routine for real rasters with many strips or
    /// tags) made the bounds check fail and the value was dropped as if
    /// absent, while `bounds` was additionally hardcoded to `None`
    /// unconditionally. This is unrelated to the EPSG code's hemisphere —
    /// the same file with a northern-hemisphere zone would fail identically.
    #[test]
    fn test_issue_12_far_offset_georeferencing() {
        let tiff = build_geotiff_with_far_offset_georeferencing(4, 4);
        assert!(
            tiff.len() > 8192,
            "fixture must exceed the peek window to reproduce the bug"
        );
        let path = write_temp_file("test_issue_12_far_offsets.tif", &tiff);
        let ds = open(&path).expect("open geotiff");
        let info = ds.info().expect("info");

        assert_eq!(info.width, Some(4));
        assert_eq!(info.height, Some(4));

        let crs = info.crs.as_deref().expect("CRS should be resolved");
        assert_eq!(crs, "EPSG:32721", "southern-hemisphere UTM EPSG code");

        let gt = info.geotransform.expect("geotransform should be resolved");
        assert!(
            (gt.origin_x - 369_065.5).abs() < 1e-6,
            "origin_x: {}",
            gt.origin_x
        );
        assert!(
            (gt.origin_y - 6_174_601.0).abs() < 1e-6,
            "origin_y: {}",
            gt.origin_y
        );
        assert!(
            (gt.pixel_width - 0.5).abs() < 1e-12,
            "pixel_width: {}",
            gt.pixel_width
        );
        assert!(
            (gt.pixel_height - (-0.5)).abs() < 1e-12,
            "pixel_height: {}",
            gt.pixel_height
        );

        let bounds = info
            .bounds
            .expect("bounds should be derived from the geotransform");
        assert!((bounds.min_x - 369_065.5).abs() < 1e-6);
        assert!((bounds.max_y - 6_174_601.0).abs() < 1e-6);
    }

    #[test]
    fn test_tiff_big_endian_extraction() {
        let path = write_temp_file("test_meta_be.tif", &build_minimal_tiff_be(800, 600));
        let ds = open(&path).expect("open");
        let info = ds.info().expect("info");
        assert_eq!(info.width, Some(800));
        assert_eq!(info.height, Some(600));
    }

    /// A TIFF whose IFD declares `ImageWidth = 0` is degenerate: reporting it as
    /// a `0`-wide dataset is exactly the silent-zeros failure mode that
    /// cool-japan/oxigeo#14 was about, so it must be an error instead.
    #[test]
    fn test_tiff_zero_dimension_is_rejected() {
        let path = write_temp_file("test_meta_zero_dim.tif", &build_minimal_tiff_le(0, 16, 1));
        let err = open(&path).expect_err("zero-width TIFF must not open silently");
        assert!(
            matches!(err, OxiGeoError::Format(_)),
            "expected a typed Format error, got {err:?}"
        );
    }

    // ── GeoJSON lightweight extraction ─────────────────────────────────────────

    #[test]
    fn test_geojson_feature_collection_detected() {
        let content = br#"{"type":"FeatureCollection","features":[{"type":"Feature","geometry":null,"properties":{}}]}"#;
        let path = write_temp_file("test_meta_fc.geojson", content);
        let ds = open(&path).expect("open");
        let info = ds.info().expect("info");
        assert_eq!(info.format, DatasetFormat::GeoJson);
        assert_eq!(info.layer_count, 1);
    }

    #[test]
    fn test_geojson_single_feature_no_collection() {
        let content = br#"{"type":"Feature","geometry":null,"properties":{}}"#;
        let path = write_temp_file("test_meta_single.geojson", content);
        let ds = open(&path).expect("open");
        let info = ds.info().expect("info");
        assert_eq!(info.format, DatasetFormat::GeoJson);
        assert_eq!(info.layer_count, 0);
    }
}
