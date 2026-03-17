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

use crate::{DatasetFormat, DatasetInfo, Result};

// ─── Magic byte signatures ───────────────────────────────────────────────────

/// TIFF little-endian byte order marker: `II` (0x49 0x49)
const TIFF_LE_MAGIC: [u8; 2] = [0x49, 0x49];
/// TIFF big-endian byte order marker: `MM` (0x4D 0x4D)
const TIFF_BE_MAGIC: [u8; 2] = [0x4D, 0x4D];
/// JPEG 2000 / JP2 file magic (first 12 bytes): `\x00\x00\x00\x0CjP  \r\n\x87\n`
const JP2_MAGIC: [u8; 12] = [
    0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
];
/// HDF5 superblock signature
const HDF5_MAGIC: [u8; 8] = [0x89, 0x48, 0x44, 0x46, 0x0D, 0x0A, 0x1A, 0x0A];
/// NetCDF classic/64-bit offset: `CDF\x01` or `CDF\x02`
const NETCDF_MAGIC: [u8; 3] = [0x43, 0x44, 0x46];
/// ZIP/GeoPackage/GPKG PK header
const ZIP_MAGIC: [u8; 4] = [0x50, 0x4B, 0x03, 0x04];
/// SQLite database file header
const SQLITE_MAGIC: [u8; 6] = [0x53, 0x51, 0x4C, 0x69, 0x74, 0x65];
/// GeoTIFF BigTIFF marker (version 43)
const BIGTIFF_VERSION: u16 = 43;
/// Standard TIFF version 42
const TIFF_VERSION: u16 = 42;

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

/// Attempt to detect the dataset format by inspecting magic bytes.
fn detect_from_magic(path: &Path) -> Result<MagicDetectionResult> {
    let buf = read_magic_bytes(path, 16)?;

    if buf.len() < 2 {
        return Ok(MagicDetectionResult::Unknown);
    }

    // TIFF / BigTIFF — little-endian or big-endian
    if buf.starts_with(&TIFF_LE_MAGIC) || buf.starts_with(&TIFF_BE_MAGIC) {
        if buf.len() >= 4 {
            let version = if buf[0] == 0x49 {
                // little-endian
                u16::from_le_bytes([buf[2], buf[3]])
            } else {
                // big-endian
                u16::from_be_bytes([buf[2], buf[3]])
            };
            if version == TIFF_VERSION || version == BIGTIFF_VERSION {
                return Ok(MagicDetectionResult::Detected(DatasetFormat::GeoTiff));
            }
        }
        return Ok(MagicDetectionResult::Detected(DatasetFormat::GeoTiff));
    }

    // JPEG2000 / JP2 box signature
    if buf.len() >= 12 && buf[..12] == JP2_MAGIC {
        return Ok(MagicDetectionResult::Detected(DatasetFormat::Jpeg2000));
    }

    // HDF5 superblock
    if buf.len() >= 8 && buf[..8] == HDF5_MAGIC {
        return Ok(MagicDetectionResult::Detected(DatasetFormat::Hdf5));
    }

    // NetCDF (CDF\x01 or CDF\x02)
    if buf.len() >= 4
        && buf[..3] == NETCDF_MAGIC
        && (buf[3] == 0x01 || buf[3] == 0x02 || buf[3] == 0x05)
    {
        return Ok(MagicDetectionResult::Detected(DatasetFormat::NetCdf));
    }

    // ZIP / GeoPackage / GPKG — PK header
    if buf.len() >= 4 && buf[..4] == ZIP_MAGIC {
        // Could be GPKG or other ZIP-based format — use extension to disambiguate
        return Ok(MagicDetectionResult::Detected(DatasetFormat::GeoPackage));
    }

    // SQLite database (could be GeoPackage)
    if buf.len() >= 6 && buf[..6] == SQLITE_MAGIC {
        return Ok(MagicDetectionResult::Detected(DatasetFormat::GeoPackage));
    }

    Ok(MagicDetectionResult::Unknown)
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
                // Special-case: .json might be GeoJSON or STAC
                let ext = path_ref
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(str::to_lowercase)
                    .unwrap_or_default();
                if ext == "json" {
                    DatasetFormat::GeoJson
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
/// For now this is metadata-only (no actual driver parsing).  Width/height and
/// CRS are populated lazily by the driver crates; here we return `None` for
/// all optional fields.
fn build_dataset_info(_path: &Path, format: DatasetFormat) -> DatasetInfo {
    DatasetInfo {
        format,
        width: None,
        height: None,
        band_count: 0,
        layer_count: 0,
        crs: None,
        geotransform: None,
    }
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
}
