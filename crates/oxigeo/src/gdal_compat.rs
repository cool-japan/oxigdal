//! GDAL C API compatibility shim.
//!
//! This module exposes Rust functions with the same *names* as the most-used
//! GDAL C API entry points.  It is intended to ease migration of code that
//! calls GDAL via FFI or via thin Rust wrappers that merely shadow the C names.
//!
//! All functions are pure-Rust safe implementations that delegate to the
//! unified [`crate::Dataset`] API.
//!
//! # Enabling
//!
//! This module is **off by default**.  Enable it with the `gdal-compat` feature:
//!
//! ```toml
//! oxigeo = { version = "0.2", features = ["gdal-compat"] }
//! ```
//!
//! # Naming
//!
//! GDAL uses `PascalCase` function names with a `GDAL` prefix.  Rust's
//! linter (`clippy`) would normally warn about this style; the entire module
//! carries `#[allow(non_snake_case)]` to suppress those warnings.
//!
//! # Limitations
//!
//! - This shim is **read-only**: write functions (`GDALCreate`, etc.) are not
//!   provided.
//! - There is **no global dataset registry** (`GDALAllRegister` is a no-op).
//! - Return types are idiomatic Rust (`Option<_>`, `Result<_>`) rather than
//!   C pointers.

#![allow(non_snake_case)]

use crate::{Dataset, DatasetFormat, GeoTransform, OxiGeoError, Result};

// ─── Registration / Version ───────────────────────────────────────────────────

/// No-op placeholder for `GDALAllRegister()`.
///
/// In C GDAL this function registers all available format drivers.  In
/// OxiGeo, driver registration is performed automatically via Cargo feature
/// flags at compile time.
pub fn GDALAllRegister() {
    // No-op: OxiGeo drivers are compiled in, not dynamically registered.
}

/// Return OxiGeo's version string, analogous to `GDALVersionInfo("VERSION_NUM")`.
///
/// Returns the string form of the OxiGeo crate version.
pub fn GDALVersionInfo() -> &'static str {
    crate::version()
}

// ─── Dataset open / close ─────────────────────────────────────────────────────

/// Open a geospatial dataset, analogous to `GDALOpen(pszFilename, GA_ReadOnly)`.
///
/// Returns `Ok(Dataset)` on success.  In C GDAL `GDALOpen` returns a raw
/// pointer; here we return an owned [`Dataset`] (RAII — closes automatically
/// on drop).
///
/// # Errors
///
/// Propagates any error from [`Dataset::open`].
pub fn GDALOpen(path: &str) -> Result<Dataset> {
    Dataset::open(path)
}

/// Open a dataset with an explicit format hint.
///
/// Analogous to `GDALOpenEx(pszFilename, 0, papszAllowedDrivers, ...)`.
/// The `format` parameter selects the driver; pass [`DatasetFormat::Unknown`]
/// to auto-detect.
///
/// # Errors
///
/// Propagates any error from [`Dataset::open`] or [`Dataset::open_with_format`].
pub fn GDALOpenEx(path: &str, format: DatasetFormat) -> Result<Dataset> {
    if format == DatasetFormat::Unknown {
        Dataset::open(path)
    } else {
        Dataset::open_with_format(path, format)
    }
}

/// Close a dataset.
///
/// In C GDAL `GDALClose(hDS)` frees the dataset handle.  In OxiGeo
/// `Dataset` is RAII — dropping it closes any open resources.  This
/// function is provided for call-site symmetry; it simply drops the value.
pub fn GDALClose(dataset: Dataset) {
    drop(dataset);
}

// ─── Driver name ──────────────────────────────────────────────────────────────

/// Return the GDAL driver name for a dataset, analogous to
/// `GDALGetDatasetDriver(hDS) → GDALGetDriverShortName(hDriver)`.
pub fn GDALGetDatasetDriver(dataset: &Dataset) -> &'static str {
    dataset.format().driver_name()
}

// ─── Raster dimensions ────────────────────────────────────────────────────────

/// Raster width in pixels, analogous to `GDALGetRasterXSize(hDS)`.
///
/// Returns 0 for pure-vector datasets.
pub fn GDALGetRasterXSize(dataset: &Dataset) -> u32 {
    dataset.width()
}

/// Raster height in pixels, analogous to `GDALGetRasterYSize(hDS)`.
///
/// Returns 0 for pure-vector datasets.
pub fn GDALGetRasterYSize(dataset: &Dataset) -> u32 {
    dataset.height()
}

/// Number of raster bands, analogous to `GDALGetRasterCount(hDS)`.
pub fn GDALGetRasterCount(dataset: &Dataset) -> u32 {
    dataset.band_count()
}

// ─── CRS / Projection ─────────────────────────────────────────────────────────

/// Return the dataset's CRS / projection reference string.
///
/// Analogous to `GDALGetProjectionRef(hDS)`.
///
/// Returns `None` when no CRS is stored.
pub fn GDALGetProjectionRef(dataset: &Dataset) -> Option<&str> {
    dataset.crs()
}

// ─── GeoTransform ─────────────────────────────────────────────────────────────

/// Retrieve the affine geo-transform.
///
/// Analogous to `GDALGetGeoTransform(hDS, padfTransform)`.
///
/// Returns `Ok(GeoTransform)` when a geo-transform is available, or
/// [`OxiGeoError::NotSupported`] when the dataset has no geo-transform.
pub fn GDALGetGeoTransform(dataset: &Dataset) -> Result<GeoTransform> {
    dataset
        .geotransform()
        .copied()
        .ok_or_else(|| OxiGeoError::NotSupported {
            operation:
                "GDALGetGeoTransform: dataset has no geo-transform (vector-only or unknown CRS)"
                    .to_string(),
        })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

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
            Self(std::env::temp_dir().join(format!(
                "oxigeo_gdal_compat_{}_{seq}_{name}",
                std::process::id()
            )))
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

    /// Opening a nonexistent path returns an error (not a panic).
    #[test]
    fn test_gdal_compat_nonexistent_path_errors() {
        let result = GDALOpen("/nonexistent/path/does_not_exist.tif");
        assert!(
            result.is_err(),
            "GDALOpen on nonexistent path should return Err"
        );
    }

    /// `GDALAllRegister` is a no-op — calling it should not panic.
    #[test]
    fn test_gdal_compat_all_register_noop() {
        GDALAllRegister(); // must not panic
    }

    /// `GDALVersionInfo` returns a non-empty string.
    #[test]
    fn test_gdal_compat_version_info() {
        let v = GDALVersionInfo();
        assert!(!v.is_empty(), "version string should not be empty");
    }

    /// `GDALGetRasterXSize` / `GDALGetRasterYSize` / `GDALGetRasterCount`
    /// compile and return sensible values for a minimal TIFF.
    #[cfg(feature = "geotiff")]
    #[test]
    fn test_gdal_compat_open_close_tiff() {
        use std::io::Write;
        let path = TempPath::new("test_gdal_compat_tiff.tif");
        // Minimal LE TIFF header: width=128, height=64, bands=3
        let mut buf: Vec<u8> = vec![0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00];
        buf.extend_from_slice(&3u16.to_le_bytes()); // 3 IFD entries
        // ImageWidth=128 (LONG)
        buf.extend_from_slice(&256u16.to_le_bytes());
        buf.extend_from_slice(&4u16.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&128u32.to_le_bytes());
        // ImageLength=64 (LONG)
        buf.extend_from_slice(&257u16.to_le_bytes());
        buf.extend_from_slice(&4u16.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&64u32.to_le_bytes());
        // SamplesPerPixel=3 (SHORT)
        buf.extend_from_slice(&277u16.to_le_bytes());
        buf.extend_from_slice(&3u16.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&3u16.to_le_bytes());
        buf.extend_from_slice(&[0x00, 0x00]);
        buf.extend_from_slice(&0u32.to_le_bytes()); // next IFD=0

        std::fs::File::create(&path)
            .and_then(|mut f| f.write_all(&buf))
            .expect("write tiff");

        let ds = GDALOpen(path.to_str().expect("path")).expect("GDALOpen");
        assert_eq!(GDALGetRasterXSize(&ds), 128);
        assert_eq!(GDALGetRasterYSize(&ds), 64);
        assert_eq!(GDALGetRasterCount(&ds), 3);
        assert_eq!(GDALGetDatasetDriver(&ds), "GTiff");
        GDALClose(ds); // explicit close — moves ds into the function
    }
}
