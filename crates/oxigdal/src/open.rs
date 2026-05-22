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
//! use oxigdal::open::open;
//!
//! # fn main() -> oxigdal::Result<()> {
//! let dataset = open("elevation.tif")?;
//! match dataset {
//!     oxigdal::open::OpenedDataset::GeoTiff(info) => {
//!         println!("GeoTIFF: {}×{}", info.width.unwrap_or(0), info.height.unwrap_or(0));
//!     }
//!     _ => {}
//! }
//! # Ok(())
//! # }
//! ```

use std::path::{Path, PathBuf};

use oxigdal_core::error::{IoError, OxiGdalError};

use crate::magic::{BIGTIFF_VERSION, TIFF_VERSION};
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
        OxiGdalError::Io(IoError::Read {
            message: format!("cannot open '{}': {e}", path.display()),
        })
    })?;
    let mut buf = vec![0u8; n];
    let read_bytes = file.read(&mut buf).map_err(|e| {
        OxiGdalError::Io(IoError::Read {
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
fn is_stac_json(path: &Path) -> bool {
    use std::io::Read as _;
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut buf = vec![0u8; 4096];
    let n = file.read(&mut buf).unwrap_or(0);
    buf.truncate(n);
    let text = std::str::from_utf8(&buf).unwrap_or("");
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
/// Returns [`OxiGdalError::Io`] if the file cannot be read.
/// Returns [`OxiGdalError::NotSupported`] if the format cannot be determined.
///
/// # Examples
///
/// ```rust,no_run
/// use oxigdal::open::open;
///
/// # fn main() -> oxigdal::Result<()> {
/// let dataset = open("world.tif")?;
/// println!("format: {}", dataset.format());
/// # Ok(())
/// # }
/// ```
pub fn open(path: impl AsRef<Path>) -> Result<OpenedDataset> {
    let path_ref = path.as_ref();
    let path_str = path_ref.to_str().unwrap_or("").to_string();

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
        return Err(OxiGdalError::Io(IoError::NotFound {
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

    let info = build_dataset_info(path_ref, format);
    let opened = map_format_to_opened(format, info);
    Ok(opened)
}

/// Build a [`DatasetInfo`] for the given path and detected format.
///
/// For GeoTIFF datasets the first IFD is parsed to populate width, height,
/// band count, and (when present) the GeoTransform from ModelTiepointTag
/// and ModelPixelScaleTag.  Other formats fall back to empty metadata that
/// the driver crates populate lazily.
fn build_dataset_info(path: &Path, format: DatasetFormat) -> DatasetInfo {
    let path_str = path.to_str().map(str::to_string);

    let empty = |fmt: DatasetFormat| DatasetInfo {
        format: fmt,
        path: path_str.clone(),
        width: None,
        height: None,
        band_count: 0,
        layer_count: 0,
        crs: None,
        geotransform: None,
        feature_count: None,
        bounds: None,
    };

    // Attempt lightweight header parsing for formats we understand.
    match format {
        DatasetFormat::GeoTiff => {
            let mut info = extract_tiff_info(path).unwrap_or_else(|| empty(format));
            info.path = path_str;
            info
        }
        DatasetFormat::GeoJson => {
            let mut info = extract_geojson_info(path).unwrap_or_else(|| empty(format));
            info.path = path_str;
            info
        }
        #[cfg(feature = "shapefile")]
        DatasetFormat::Shapefile => {
            let mut info = extract_shapefile_info(path).unwrap_or_else(|| empty(format));
            info.path = path_str;
            info
        }
        #[cfg(feature = "flatgeobuf")]
        DatasetFormat::FlatGeobuf => {
            let mut info = extract_flatgeobuf_info(path).unwrap_or_else(|| empty(format));
            info.path = path_str;
            info
        }
        #[cfg(feature = "geoparquet")]
        DatasetFormat::GeoParquet => {
            let mut info = extract_geoparquet_info(path).unwrap_or_else(|| empty(format));
            info.path = path_str;
            info
        }
        _ => empty(format),
    }
}

// ─── TIFF IFD parsing (lightweight, no external deps) ────────────────────────

/// TIFF tag constants for the fields we extract.
const TAG_IMAGE_WIDTH: u16 = 256;
const TAG_IMAGE_LENGTH: u16 = 257;
const TAG_SAMPLES_PER_PIXEL: u16 = 277;
const TAG_MODEL_PIXEL_SCALE: u16 = 33550;
const TAG_MODEL_TIEPOINT: u16 = 33922;
const TAG_GEO_KEY_DIRECTORY: u16 = 34735;

/// Read a u16 from `buf` at `offset` respecting byte order.
fn tiff_read_u16(buf: &[u8], offset: usize, le: bool) -> Option<u16> {
    if offset + 2 > buf.len() {
        return None;
    }
    Some(if le {
        u16::from_le_bytes([buf[offset], buf[offset + 1]])
    } else {
        u16::from_be_bytes([buf[offset], buf[offset + 1]])
    })
}

/// Read a u32 from `buf` at `offset` respecting byte order.
fn tiff_read_u32(buf: &[u8], offset: usize, le: bool) -> Option<u32> {
    if offset + 4 > buf.len() {
        return None;
    }
    Some(if le {
        u32::from_le_bytes([
            buf[offset],
            buf[offset + 1],
            buf[offset + 2],
            buf[offset + 3],
        ])
    } else {
        u32::from_be_bytes([
            buf[offset],
            buf[offset + 1],
            buf[offset + 2],
            buf[offset + 3],
        ])
    })
}

/// Read a u64 from `buf` at `offset` respecting byte order.
fn tiff_read_u64(buf: &[u8], offset: usize, le: bool) -> Option<u64> {
    if offset + 8 > buf.len() {
        return None;
    }
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&buf[offset..offset + 8]);
    Some(if le {
        u64::from_le_bytes(bytes)
    } else {
        u64::from_be_bytes(bytes)
    })
}

/// Read an f64 from `buf` at `offset` respecting byte order.
fn tiff_read_f64(buf: &[u8], offset: usize, le: bool) -> Option<f64> {
    if offset + 8 > buf.len() {
        return None;
    }
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&buf[offset..offset + 8]);
    Some(if le {
        f64::from_le_bytes(bytes)
    } else {
        f64::from_be_bytes(bytes)
    })
}

/// Extract the IFD tag value as a u32.  For SHORT / LONG types the value
/// is stored inline in the 4-byte value/offset field of the IFD entry.
fn ifd_entry_value_u32(buf: &[u8], entry_offset: usize, le: bool) -> Option<u32> {
    let type_id = tiff_read_u16(buf, entry_offset + 2, le)?;
    match type_id {
        // SHORT (u16) — stored inline in first 2 bytes of the value field
        3 => tiff_read_u16(buf, entry_offset + 8, le).map(u32::from),
        // LONG (u32) — stored inline
        4 => tiff_read_u32(buf, entry_offset + 8, le),
        _ => None,
    }
}

/// Parse the first IFD of a TIFF file and extract basic metadata.
///
/// Reads up to 8 KiB — enough for the IFD plus any small inline values.
///
/// Exposed as `pub(crate)` so that `lib.rs` can reuse this logic without
/// duplicating it.
pub(crate) fn extract_tiff_info(path: &Path) -> Option<DatasetInfo> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; 8192];
    let n = file.read(&mut buf).ok()?;
    buf.truncate(n);
    if buf.len() < 8 {
        return None;
    }

    // Byte order
    let le = buf[0] == 0x49; // 'I' = little-endian

    // TIFF version
    let version = tiff_read_u16(&buf, 2, le)?;
    let (ifd_offset, entry_size) = if version == BIGTIFF_VERSION {
        // BigTIFF: offset size = 8, pad = 2, IFD at offset 8
        let off = tiff_read_u64(&buf, 8, le)? as usize;
        (off, 20usize) // BigTIFF entries are 20 bytes
    } else if version == TIFF_VERSION {
        let off = tiff_read_u32(&buf, 4, le)? as usize;
        (off, 12usize) // Classic: 12 bytes per entry
    } else {
        return None; // not a TIFF
    };

    // Number of IFD entries
    let num_entries = if version == BIGTIFF_VERSION {
        tiff_read_u64(&buf, ifd_offset, le)? as usize
    } else {
        tiff_read_u16(&buf, ifd_offset, le)? as usize
    };
    let entries_start = if version == BIGTIFF_VERSION {
        ifd_offset + 8
    } else {
        ifd_offset + 2
    };

    let mut width: Option<u32> = None;
    let mut height: Option<u32> = None;
    let mut samples_per_pixel: u32 = 1;
    let mut pixel_scale_offset: Option<usize> = None;
    let mut tiepoint_offset: Option<usize> = None;
    let mut _geo_keys_found = false;

    for i in 0..num_entries {
        let eo = entries_start + i * entry_size;
        if eo + entry_size > buf.len() {
            break;
        }
        let tag = tiff_read_u16(&buf, eo, le)?;

        match tag {
            TAG_IMAGE_WIDTH => {
                width = ifd_entry_value_u32(&buf, eo, le);
            }
            TAG_IMAGE_LENGTH => {
                height = ifd_entry_value_u32(&buf, eo, le);
            }
            TAG_SAMPLES_PER_PIXEL => {
                if let Some(v) = ifd_entry_value_u32(&buf, eo, le) {
                    samples_per_pixel = v;
                }
            }
            TAG_MODEL_PIXEL_SCALE => {
                // Value is a DOUBLE[3] — stored at offset pointed to by value field
                let off = tiff_read_u32(&buf, eo + 8, le)? as usize;
                pixel_scale_offset = Some(off);
            }
            TAG_MODEL_TIEPOINT => {
                let off = tiff_read_u32(&buf, eo + 8, le)? as usize;
                tiepoint_offset = Some(off);
            }
            TAG_GEO_KEY_DIRECTORY => {
                _geo_keys_found = true;
            }
            _ => {}
        }
    }

    // Construct GeoTransform from ModelPixelScale + ModelTiepoint if both present.
    // ModelPixelScale = [ScaleX, ScaleY, ScaleZ]
    // ModelTiepoint = [I, J, K, X, Y, Z] — typically I=J=K=0 for north-up
    let geotransform = match (pixel_scale_offset, tiepoint_offset) {
        (Some(ps_off), Some(tp_off)) if ps_off + 24 <= buf.len() && tp_off + 48 <= buf.len() => {
            let scale_x = tiff_read_f64(&buf, ps_off, le)?;
            let scale_y = tiff_read_f64(&buf, ps_off + 8, le)?;
            let _i = tiff_read_f64(&buf, tp_off, le)?;
            let _j = tiff_read_f64(&buf, tp_off + 8, le)?;
            let origin_x = tiff_read_f64(&buf, tp_off + 24, le)?;
            let origin_y = tiff_read_f64(&buf, tp_off + 32, le)?;
            if scale_x.is_finite() && scale_y.is_finite() && scale_x > 0.0 && scale_y > 0.0 {
                Some(oxigdal_core::types::GeoTransform::north_up(
                    origin_x, origin_y, scale_x, scale_y,
                ))
            } else {
                None
            }
        }
        _ => None,
    };

    Some(DatasetInfo {
        format: DatasetFormat::GeoTiff,
        path: None, // populated by callers that know the path
        width,
        height,
        band_count: samples_per_pixel,
        layer_count: 0,
        crs: None,
        geotransform,
        feature_count: None,
        bounds: None,
    })
}

// ─── GeoJSON lightweight sniffing ────────────────────────────────────────────

/// Read the first few kilobytes of a GeoJSON file and try to extract the
/// collection-level bbox and feature count.
///
/// This is intentionally approximate (string-level scanning + serde_json for
/// bbox/feature arrays) to avoid pulling in a full JSON parser in all paths.
///
/// Exposed as `pub(crate)` so that `lib.rs` can reuse this logic without
/// duplicating it.
pub(crate) fn extract_geojson_info(path: &Path) -> Option<DatasetInfo> {
    use std::io::Read;
    // Read a larger chunk — GeoJSON bbox may appear after features array header
    let mut file = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; 65536];
    let n = file.read(&mut buf).ok()?;
    buf.truncate(n);
    let text = std::str::from_utf8(&buf).ok()?;

    // Very lightweight: check if it's a FeatureCollection
    let is_collection = text.contains("\"FeatureCollection\"");
    let layer_count = if is_collection { 1 } else { 0 };

    // Count features: count occurrences of `"type":"Feature"` or `"type": "Feature"`
    let feature_count = if is_collection {
        let count = count_geojson_features(text);
        if count > 0 { Some(count as u64) } else { None }
    } else {
        None
    };

    // Extract top-level bbox if present: `"bbox":[minx,miny,maxx,maxy]`
    let bounds = extract_geojson_bbox(text);

    Some(DatasetInfo {
        format: DatasetFormat::GeoJson,
        path: None, // populated by callers that know the path
        width: None,
        height: None,
        band_count: 0,
        layer_count,
        crs: None,
        geotransform: None,
        feature_count,
        bounds,
    })
}

/// Count `"type":"Feature"` occurrences in a GeoJSON text snippet.
///
/// This is approximate for very large files where the features array is truncated,
/// but works correctly for files that fit in the initial read buffer.
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
#[cfg(feature = "shapefile")]
pub(crate) fn extract_shapefile_info(path: &Path) -> Option<DatasetInfo> {
    // Strip any extension from path to get the base path for ShapefileReader::open
    let base = path.with_extension("");
    let reader = oxigdal_shapefile::ShapefileReader::open(&base).ok()?;

    let header = reader.header();
    let bbox = &header.bbox;

    // Feature count comes from the .shx index when available.
    // If the .shx was not loaded, we cannot infer the count without a full scan.
    let feature_count = reader.index_entries().map(|entries| entries.len() as u64);

    let bounds = crate::BoundingBox::new(bbox.x_min, bbox.y_min, bbox.x_max, bbox.y_max).ok();

    let crs = reader.crs().map(str::to_string);

    Some(DatasetInfo {
        format: DatasetFormat::Shapefile,
        path: None,
        width: None,
        height: None,
        band_count: 0,
        layer_count: 1,
        crs,
        geotransform: None,
        feature_count,
        bounds,
    })
}

// ─── FlatGeobuf lightweight header parsing ───────────────────────────────────

/// Parse the FlatGeobuf header to populate `feature_count` and `bounds`.
///
/// Exposed as `pub(crate)` so that `lib.rs` can use it directly in `open_vector`.
#[cfg(feature = "flatgeobuf")]
pub(crate) fn extract_flatgeobuf_info(path: &Path) -> Option<DatasetInfo> {
    use std::io::BufReader;
    let file = std::fs::File::open(path).ok()?;
    let reader = oxigdal_flatgeobuf::FlatGeobufReader::new(BufReader::new(file)).ok()?;
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

    Some(DatasetInfo {
        format: DatasetFormat::FlatGeobuf,
        path: None,
        width: None,
        height: None,
        band_count: 0,
        layer_count: 1,
        crs,
        geotransform: None,
        feature_count,
        bounds,
    })
}

// ─── GeoParquet lightweight metadata parsing ─────────────────────────────────

/// Parse the GeoParquet file metadata to populate `feature_count` and `bounds`.
///
/// Exposed as `pub(crate)` so that `lib.rs` can use it directly in `open_vector`.
#[cfg(feature = "geoparquet")]
pub(crate) fn extract_geoparquet_info(path: &Path) -> Option<DatasetInfo> {
    let reader = oxigdal_geoparquet::GeoParquetReader::open(path).ok()?;

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

    Some(DatasetInfo {
        format: DatasetFormat::GeoParquet,
        path: None,
        width: None,
        height: None,
        band_count: 0,
        layer_count: 1,
        crs: None,
        geotransform: None,
        feature_count,
        bounds,
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
mod tests {
    use super::*;
    use crate::magic::{HDF5_MAGIC, JP2_MAGIC};
    use std::io::Write;

    // ── helper: create a temp file with given bytes ──────────────────────────
    fn write_temp_file(name: &str, content: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir();
        let path = dir.join(name);
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
            matches!(err, OxiGdalError::Io(IoError::NotFound { .. })),
            "expected NotFound, got {err:?}"
        );
    }

    // ── magic-byte detection ──────────────────────────────────────────────────

    #[test]
    fn test_magic_tiff_little_endian() {
        // Minimal TIFF LE header: II + version 42 LE
        let bytes = [0x49u8, 0x49, 0x2A, 0x00, 0x00, 0x00, 0x00, 0x00];
        let path = write_temp_file("test_magic_tiff_le.tif", &bytes);
        let ds = open(&path).expect("open tiff le");
        assert_eq!(ds.format(), DatasetFormat::GeoTiff);
        assert!(ds.is_raster());
    }

    #[test]
    fn test_magic_tiff_big_endian() {
        // Minimal TIFF BE header: MM + version 42 BE
        let bytes = [0x4Du8, 0x4D, 0x00, 0x2A, 0x00, 0x00, 0x00, 0x00];
        let path = write_temp_file("test_magic_tiff_be.tif", &bytes);
        let ds = open(&path).expect("open tiff be");
        assert_eq!(ds.format(), DatasetFormat::GeoTiff);
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
        let content = b"\x00\x00\x27\x0A"; // SHP magic (optional check)
        let path = write_temp_file("test_ext_shapefile.shp", content);
        let ds = open(&path).expect("open shp");
        assert_eq!(ds.format(), DatasetFormat::Shapefile);
        assert!(ds.is_vector());
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

    #[test]
    fn test_tiff_big_endian_extraction() {
        // MM + version 42 BE + IFD at offset 8
        let mut buf: Vec<u8> = vec![0x4D, 0x4D, 0x00, 0x2A, 0x00, 0x00, 0x00, 0x08];
        let num_entries: u16 = 2;
        buf.extend_from_slice(&num_entries.to_be_bytes());
        // ImageWidth as LONG BE
        buf.extend_from_slice(&256u16.to_be_bytes());
        buf.extend_from_slice(&4u16.to_be_bytes());
        buf.extend_from_slice(&1u32.to_be_bytes());
        buf.extend_from_slice(&800u32.to_be_bytes());
        // ImageLength as LONG BE
        buf.extend_from_slice(&257u16.to_be_bytes());
        buf.extend_from_slice(&4u16.to_be_bytes());
        buf.extend_from_slice(&1u32.to_be_bytes());
        buf.extend_from_slice(&600u32.to_be_bytes());
        buf.extend_from_slice(&0u32.to_be_bytes());
        let path = write_temp_file("test_meta_be.tif", &buf);
        let ds = open(&path).expect("open");
        let info = ds.info().expect("info");
        assert_eq!(info.width, Some(800));
        assert_eq!(info.height, Some(600));
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
