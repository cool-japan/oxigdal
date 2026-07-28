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
const TAG_BITS_PER_SAMPLE: u16 = 258;
const TAG_SAMPLE_FORMAT: u16 = 339;
const TAG_MODEL_PIXEL_SCALE: u16 = 33550;
const TAG_MODEL_TIEPOINT: u16 = 33922;
const TAG_GEO_KEY_DIRECTORY: u16 = 34735;

/// Upper bound on how far [`extract_tiff_info`] will extend its initial 8 KiB
/// peek window to resolve out-of-line georeferencing tag values (see
/// [`extract_tiff_info`] for why this extension is needed).
///
/// Real-world GeoTIFF georeferencing arrays (`ModelPixelScaleTag` = 24 bytes,
/// `ModelTiepointTag` = 48 bytes, `GeoKeyDirectoryTag` = a few KiB even for
/// directories with hundreds of keys) are always tiny, so 1 MiB is generous
/// headroom. This also bounds worst-case I/O/memory for a hostile file that
/// advertises a huge `GeoKeyDirectoryTag` count or offset — such files fall
/// back to the existing "value out of reach" `None` behavior instead of
/// forcing a multi-gigabyte read.
const MAX_GEOREF_PEEK_BYTES: usize = 1024 * 1024;

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

/// Read the first element of a SHORT-typed IFD entry, handling both the inline
/// storage form and the out-of-line offset form.
///
/// Returns `None` for non-SHORT entries or when the referenced data lies beyond
/// the prefix buffer we read.  Used for `BitsPerSample` / `SampleFormat`, whose
/// per-sample arrays are stored out-of-line once `samples_per_pixel` exceeds the
/// inline capacity (2 SHORTs for classic TIFF, 4 for BigTIFF).
fn ifd_entry_first_short(buf: &[u8], entry_offset: usize, le: bool, bigtiff: bool) -> Option<u16> {
    let type_id = tiff_read_u16(buf, entry_offset + 2, le)?;
    if type_id != 3 {
        return None; // only SHORT is handled here
    }
    let (count, value_field, inline_cap) = if bigtiff {
        (
            tiff_read_u64(buf, entry_offset + 4, le)?,
            entry_offset + 12,
            8usize,
        )
    } else {
        (
            u64::from(tiff_read_u32(buf, entry_offset + 4, le)?),
            entry_offset + 8,
            4usize,
        )
    };
    // 2 bytes per SHORT element.
    let total = count.saturating_mul(2);
    if total as usize <= inline_cap {
        tiff_read_u16(buf, value_field, le)
    } else {
        let off = if bigtiff {
            tiff_read_u64(buf, value_field, le)? as usize
        } else {
            tiff_read_u32(buf, value_field, le)? as usize
        };
        tiff_read_u16(buf, off, le)
    }
}

/// Parse the first IFD of a TIFF file and determine its pixel [`RasterDataType`]
/// from the `BitsPerSample` and `SampleFormat` tags.
///
/// Falls back to `UInt8` (8-bit unsigned — the dominant imagery case and the
/// TIFF-spec default sample format) when the tags are absent but the file is a
/// valid TIFF.  Returns `None` only when the file cannot be parsed as a TIFF.
///
/// Exposed as `pub(crate)` so that `vrt_builder.rs` can emit the correct GDAL
/// `dataType` for each source band instead of hardcoding `Float32`.
pub(crate) fn extract_tiff_data_type(path: &Path) -> Option<oxigeo_core::types::RasterDataType> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; 8192];
    let n = file.read(&mut buf).ok()?;
    buf.truncate(n);
    if buf.len() < 8 {
        return None;
    }

    let le = buf[0] == 0x49;
    let version = tiff_read_u16(&buf, 2, le)?;
    let bigtiff = version == BIGTIFF_VERSION;
    let (ifd_offset, entry_size) = if bigtiff {
        (tiff_read_u64(&buf, 8, le)? as usize, 20usize)
    } else if version == TIFF_VERSION {
        (tiff_read_u32(&buf, 4, le)? as usize, 12usize)
    } else {
        return None;
    };

    let num_entries = if bigtiff {
        tiff_read_u64(&buf, ifd_offset, le)? as usize
    } else {
        tiff_read_u16(&buf, ifd_offset, le)? as usize
    };
    let entries_start = if bigtiff {
        ifd_offset + 8
    } else {
        ifd_offset + 2
    };

    let mut bits_per_sample: Option<u16> = None;
    let mut sample_format: u16 = 1; // TIFF default: unsigned integer

    for i in 0..num_entries {
        let eo = entries_start + i * entry_size;
        if eo + entry_size > buf.len() {
            break;
        }
        let tag = tiff_read_u16(&buf, eo, le)?;
        match tag {
            TAG_BITS_PER_SAMPLE => {
                if let Some(v) = ifd_entry_first_short(&buf, eo, le, bigtiff) {
                    bits_per_sample = Some(v);
                }
            }
            TAG_SAMPLE_FORMAT => {
                if let Some(v) = ifd_entry_first_short(&buf, eo, le, bigtiff) {
                    sample_format = v;
                }
            }
            _ => {}
        }
    }

    let bits = bits_per_sample.unwrap_or(8);
    Some(
        oxigeo_core::types::RasterDataType::from_tiff_sample_format(sample_format, bits)
            .unwrap_or(oxigeo_core::types::RasterDataType::UInt8),
    )
}

/// Parse the first IFD of a TIFF file and extract basic metadata.
///
/// Reads an initial 8 KiB peek window, which covers the IFD entry list and
/// any small inline tag values for the vast majority of files. Some
/// georeferencing tag *values* (`ModelPixelScaleTag`, `ModelTiepointTag`,
/// `GeoKeyDirectoryTag`) are stored out-of-line at a file offset recorded in
/// the IFD entry rather than inline — for real-world rasters with many tags
/// or strips (e.g. a striped GeoTIFF with `RowsPerStrip=1`, which pushes huge
/// `StripOffsets`/`StripByteCounts` arrays ahead of the small geo-tag value
/// arrays) that offset routinely lands past the initial 8 KiB window. When
/// that happens, the peek buffer is extended with a second, bounded read
/// (see [`MAX_GEOREF_PEEK_BYTES`]) so georeferencing is not silently dropped.
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
    // GeoKeyDirectory: (value offset into `buf`, number of SHORT entries).
    let mut geo_key_dir: Option<(usize, usize)> = None;

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
                // GeoKeyDirectory is a SHORT[] whose length (>4 bytes) forces
                // out-of-line storage: the value field holds a file offset.
                let (count, value_off) = if version == BIGTIFF_VERSION {
                    (
                        tiff_read_u64(&buf, eo + 4, le)? as usize,
                        tiff_read_u64(&buf, eo + 12, le)? as usize,
                    )
                } else {
                    (
                        tiff_read_u32(&buf, eo + 4, le)? as usize,
                        tiff_read_u32(&buf, eo + 8, le)? as usize,
                    )
                };
                geo_key_dir = Some((value_off, count));
            }
            _ => {}
        }
    }

    // The scan above only guarantees the *IFD entries* are visible in the
    // initial peek — the DOUBLE/SHORT arrays they point to (pixel scale,
    // tiepoint, GeoKey directory) live out-of-line at those file offsets,
    // which for real-world rasters can fall past the first 8 KiB (see the
    // doc comment on this function). Extend `buf` with a bounded, best-effort
    // read so those values aren't silently treated as absent; the file
    // cursor is already positioned at `buf.len()` from the initial read, so
    // this simply continues reading forward.
    let mut required_end = buf.len();
    if let Some(off) = pixel_scale_offset {
        required_end = required_end.max(off.saturating_add(24));
    }
    if let Some(off) = tiepoint_offset {
        required_end = required_end.max(off.saturating_add(48));
    }
    if let Some((off, count)) = geo_key_dir {
        required_end = required_end.max(off.saturating_add(count.saturating_mul(2)));
    }
    if required_end > buf.len() && required_end <= MAX_GEOREF_PEEK_BYTES {
        let extra_needed = (required_end - buf.len()) as u64;
        let _ = (&mut file).take(extra_needed).read_to_end(&mut buf);
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
                // `ModelPixelScaleTag`'s Y scale is always stored as a positive
                // magnitude (GeoTIFF spec); `GeoTransform::north_up` expects a
                // *negative* pixel_height for north-up rasters (Y increases
                // downward in pixel space, upward in world space), matching
                // the driver crate's `geokeys::extract_geo_transform`. Passing
                // the raw positive value here previously flipped the Y axis,
                // which would in turn have made the derived `bounds()` land on
                // the wrong side of the origin.
                Some(oxigeo_core::types::GeoTransform::north_up(
                    origin_x, origin_y, scale_x, -scale_y,
                ))
            } else {
                None
            }
        }
        _ => None,
    };

    // Decode the GeoKeyDirectory (if any) into an `EPSG:<code>` CRS string.
    let crs = geo_key_dir
        .and_then(|(off, count)| decode_geokey_epsg(&buf, off, count, le))
        .map(|code| format!("EPSG:{code}"));

    // Derive the bounding box from the geotransform + raster dimensions,
    // matching GDAL's `gdalinfo` "Corner Coordinates" derivation. Previously
    // this was hardcoded to `None` even when a geotransform was available.
    let bounds = match (&geotransform, width, height) {
        (Some(gt), Some(w), Some(h)) if w > 0 && h > 0 => {
            Some(gt.compute_bounds(u64::from(w), u64::from(h)))
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
        crs,
        geotransform,
        feature_count: None,
        bounds,
    })
}

/// GeoTIFF GeoKey IDs we care about for CRS identification.
const GEOKEY_GT_MODEL_TYPE: u16 = 1024;
const GEOKEY_GEOGRAPHIC_TYPE: u16 = 2048;
const GEOKEY_PROJECTED_CS_TYPE: u16 = 3072;
/// GeoKey sentinel value meaning "user-defined" (no EPSG code available).
const GEOKEY_USER_DEFINED: u16 = 32767;

/// Decode an EPSG code from a GeoTIFF GeoKeyDirectory SHORT array.
///
/// The directory layout (GeoTIFF spec) is a `SHORT[]`:
/// `[KeyDirectoryVersion, KeyRevision, MinorRevision, NumberOfKeys]` followed by
/// `NumberOfKeys` entries of `[KeyID, TIFFTagLocation, Count, ValueOffset]`.
/// When `TIFFTagLocation == 0`, `ValueOffset` holds the value inline — which is
/// exactly how the EPSG code is stored for `ProjectedCSTypeGeoKey` (3072) and
/// `GeographicTypeGeoKey` (2048).
///
/// Prefers the projected CRS code over the geographic one; returns `None` when
/// neither is present, the code is user-defined (32767), or the buffer is too
/// short to hold the declared entries.
fn decode_geokey_epsg(buf: &[u8], value_off: usize, num_shorts: usize, le: bool) -> Option<u32> {
    // Need at least the 4-short header.
    if num_shorts < 4 {
        return None;
    }
    // Bytes required for the whole directory.
    let bytes_needed = value_off.checked_add(num_shorts.checked_mul(2)?)?;
    if bytes_needed > buf.len() {
        return None;
    }

    let read_short = |idx: usize| tiff_read_u16(buf, value_off + idx * 2, le);
    let number_of_keys = read_short(3)? as usize;

    let mut projected: Option<u16> = None;
    let mut geographic: Option<u16> = None;
    let mut model_type: Option<u16> = None;

    for k in 0..number_of_keys {
        let base = 4 + k * 4;
        // Ensure the 4 shorts of this key entry are within the declared array.
        if base + 4 > num_shorts {
            break;
        }
        let key_id = read_short(base)?;
        let tag_location = read_short(base + 1)?;
        let value = read_short(base + 3)?;
        // Only inline values (TIFFTagLocation == 0) carry a direct EPSG code.
        if tag_location != 0 {
            continue;
        }
        match key_id {
            GEOKEY_GT_MODEL_TYPE => model_type = Some(value),
            GEOKEY_PROJECTED_CS_TYPE => projected = Some(value),
            GEOKEY_GEOGRAPHIC_TYPE => geographic = Some(value),
            _ => {}
        }
    }

    let valid = |code: u16| -> Option<u32> {
        if code == 0 || code == GEOKEY_USER_DEFINED {
            None
        } else {
            Some(u32::from(code))
        }
    };

    // ModelType 2 == geographic; prefer the geographic key in that case.
    // Otherwise prefer the projected code, then fall back to geographic.
    if model_type == Some(2) {
        geographic
            .and_then(valid)
            .or_else(|| projected.and_then(valid))
    } else {
        projected
            .and_then(valid)
            .or_else(|| geographic.and_then(valid))
    }
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
    let reader = oxigeo_shapefile::ShapefileReader::open(&base).ok()?;

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
    let reader = oxigeo_flatgeobuf::FlatGeobufReader::new(BufReader::new(file)).ok()?;
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
    let reader = oxigeo_geoparquet::GeoParquetReader::open(path).ok()?;

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

    // ── GeoKeyDirectory EPSG decode ──────────────────────────────────────────

    /// Serialize a GeoKeyDirectory SHORT array (little-endian) into bytes at a
    /// given `offset`, returning the full buffer.
    fn make_geokey_buf(offset: usize, shorts: &[u16]) -> Vec<u8> {
        let mut buf = vec![0u8; offset + shorts.len() * 2];
        for (i, &s) in shorts.iter().enumerate() {
            let b = s.to_le_bytes();
            buf[offset + i * 2] = b[0];
            buf[offset + i * 2 + 1] = b[1];
        }
        buf
    }

    #[test]
    fn test_decode_geokey_projected_epsg() {
        // Header: version 1, rev 1.0, 2 keys.
        // Key 1: GTModelType (1024), loc 0, count 1, value 1 (projected).
        // Key 2: ProjectedCSType (3072), loc 0, count 1, value 32633 (UTM 33N).
        let shorts = [1u16, 1, 0, 2, 1024, 0, 1, 1, 3072, 0, 1, 32633];
        let buf = make_geokey_buf(16, &shorts);
        assert_eq!(
            decode_geokey_epsg(&buf, 16, shorts.len(), true),
            Some(32633)
        );
    }

    #[test]
    fn test_decode_geokey_geographic_epsg() {
        // GTModelType=2 (geographic), GeographicType (2048)=4326.
        let shorts = [1u16, 1, 0, 2, 1024, 0, 1, 2, 2048, 0, 1, 4326];
        let buf = make_geokey_buf(0, &shorts);
        assert_eq!(decode_geokey_epsg(&buf, 0, shorts.len(), true), Some(4326));
    }

    #[test]
    fn test_decode_geokey_user_defined_is_none() {
        // ProjectedCSType user-defined (32767) → no EPSG code.
        let shorts = [1u16, 1, 0, 1, 3072, 0, 1, 32767];
        let buf = make_geokey_buf(0, &shorts);
        assert_eq!(decode_geokey_epsg(&buf, 0, shorts.len(), true), None);
    }

    #[test]
    fn test_decode_geokey_truncated_is_none() {
        // Declared 4 shorts but buffer only holds 3 → None, no panic.
        let buf = make_geokey_buf(0, &[1u16, 1, 0]);
        assert_eq!(decode_geokey_epsg(&buf, 0, 8, true), None);
    }

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
            matches!(err, OxiGeoError::Io(IoError::NotFound { .. })),
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
