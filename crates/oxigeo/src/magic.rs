//! Magic-byte signatures for binary geospatial formats.
//!
//! This module centralises every format fingerprint used by
//! [`DatasetFormat::detect_from_magic_bytes`] and
//! [`DatasetFormat::detect`].  All constants are `pub(crate)` so they
//! can be referenced from `open.rs` without duplication.
//!
//! # Adding a new format
//!
//! 1. Define a `pub(crate) const` for the signature bytes.
//! 2. Add a match arm in [`crate::DatasetFormat::detect_from_magic_bytes`].
//! 3. Update the doc-comment table here.
//!
//! # Format signatures table
//!
//! | Format          | Offset | Bytes (hex)                                   |
//! |-----------------|--------|-----------------------------------------------|
//! | TIFF LE         | 0      | `49 49`                                       |
//! | TIFF BE         | 0      | `4D 4D`                                       |
//! | JPEG 2000       | 0      | `00 00 00 0C 6A 50 20 20 0D 0A 87 0A`         |
//! | HDF5            | 0      | `89 48 44 46 0D 0A 1A 0A`                     |
//! | NetCDF classic  | 0      | `43 44 46 01` (or `02` / `05`)                |
//! | SQLite / GPKG   | 0      | `53 51 4C 69 74 65`                           |
//! | ZIP / GPKG      | 0      | `50 4B 03 04`                                 |
//! | FlatGeobuf      | 0      | `66 67 62 03 66 67 62 00`  (`fgb\x03fgb\x00`) |
//! | PMTiles v3      | 0      | `50 4D 54 69 6C 65 73`     (`PMTiles`)        |
//! | LAS / LAZ       | 0      | `4C 41 53 46`              (`LASF`)           |
//! | GRIB / GRIB2    | 0      | `47 52 49 42`              (`GRIB`)           |
//! | GeoParquet      | 0      | `50 41 52 31`              (`PAR1`)           |

// ─── TIFF ────────────────────────────────────────────────────────────────────

/// TIFF little-endian byte-order marker: `II`
pub(crate) const TIFF_LE_MAGIC: [u8; 2] = [0x49, 0x49];

/// TIFF big-endian byte-order marker: `MM`
pub(crate) const TIFF_BE_MAGIC: [u8; 2] = [0x4D, 0x4D];

/// Standard TIFF IFD version field (42)
pub(crate) const TIFF_VERSION: u16 = 42;

/// BigTIFF IFD version field (43)
pub(crate) const BIGTIFF_VERSION: u16 = 43;

// ─── JPEG 2000 ────────────────────────────────────────────────────────────────

/// JPEG 2000 / JP2 box signature (12 bytes)
pub(crate) const JP2_MAGIC: [u8; 12] = [
    0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
];

// ─── HDF5 ─────────────────────────────────────────────────────────────────────

/// HDF5 superblock signature (8 bytes)
pub(crate) const HDF5_MAGIC: [u8; 8] = [0x89, 0x48, 0x44, 0x46, 0x0D, 0x0A, 0x1A, 0x0A];

// ─── NetCDF ──────────────────────────────────────────────────────────────────

/// NetCDF classic / 64-bit offset prefix: `CDF` (bytes 0-2)
pub(crate) const NETCDF_MAGIC: [u8; 3] = [0x43, 0x44, 0x46];

// ─── SQLite / GeoPackage ─────────────────────────────────────────────────────

/// SQLite database file header magic (the full 16-byte `"SQLite format 3\0"`
/// string). The classic 6-byte `"SQLite"` prefix is extended here to the full
/// header for stricter matching.
/// Used for GeoPackage (.gpkg) and MBTiles (.mbtiles).
pub(crate) const SQLITE_MAGIC: [u8; 16] = [
    0x53, 0x51, 0x4C, 0x69, 0x74, 0x65, 0x20, 0x66, 0x6F, 0x72, 0x6D, 0x61, 0x74, 0x20, 0x33, 0x00,
];

/// ZIP local file header: `PK\x03\x04`.
/// ZIP-based formats include ODS/XLSX as well as some GPKG variants.
pub(crate) const ZIP_MAGIC: [u8; 4] = [0x50, 0x4B, 0x03, 0x04];

// ─── FlatGeobuf ───────────────────────────────────────────────────────────────

/// FlatGeobuf magic bytes: `fgb\x03fgb\x00` (8 bytes)
pub(crate) const FLATGEOBUF_MAGIC: [u8; 8] = [0x66, 0x67, 0x62, 0x03, 0x66, 0x67, 0x62, 0x00];

// ─── PMTiles ─────────────────────────────────────────────────────────────────

/// PMTiles v3 magic: `PMTiles` (7 bytes)
pub(crate) const PMTILES_MAGIC: [u8; 7] = [0x50, 0x4D, 0x54, 0x69, 0x6C, 0x65, 0x73];

// ─── LAS / LAZ (COPC) ────────────────────────────────────────────────────────

/// LAS/LAZ file signature: `LASF` (4 bytes)
pub(crate) const LAS_MAGIC: [u8; 4] = [0x4C, 0x41, 0x53, 0x46];

// ─── GRIB ────────────────────────────────────────────────────────────────────

/// GRIB/GRIB2 edition indicator: `GRIB` (4 bytes)
pub(crate) const GRIB_MAGIC: [u8; 4] = [0x47, 0x52, 0x49, 0x42];

// ─── GeoParquet ──────────────────────────────────────────────────────────────

/// Parquet file magic: `PAR1` at both the start and end of the file.
/// We only check the start here; end-of-file checks are left to the driver.
pub(crate) const GEOPARQUET_MAGIC: [u8; 4] = [0x50, 0x41, 0x52, 0x31];

// ─── Minimum read size ───────────────────────────────────────────────────────

/// Minimum bytes to read for reliable magic detection.
/// 72 bytes covers: SQLite (16), PMTiles (7), JP2 (12), HDF5 (8),
/// TIFF (8), LAS (4), GRIB (4), FlatGeobuf (8), Parquet (4), NetCDF (4).
pub(crate) const MAGIC_READ_SIZE: usize = 72;
