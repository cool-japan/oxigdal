//! Cloud URI detection and transparent dispatch for [`crate::Dataset::open`].
//!
//! This module provides lightweight helpers that classify a path string as a
//! cloud URI (`s3://`, `gs://`, `az://`, `http://`, `https://`) and, when the
//! `cloud` feature is enabled, open a stub [`crate::Dataset`] descriptor that
//! records the cloud path for subsequent streaming operations.
//!
//! When `cloud` is **not** enabled `open_cloud_dataset` returns a
//! [`crate::OxiGeoError::NotSupported`] error with a helpful message.

use crate::{Dataset, OxiGeoError, Result};
#[cfg(feature = "cloud")]
use crate::{DatasetFormat, DatasetInfo};

// ─── Detection ───────────────────────────────────────────────────────────────

/// Returns `true` if `path` starts with a known cloud storage URI scheme.
///
/// Recognised schemes: `s3://`, `gs://`, `az://`, `abfs://`, `http://`, `https://`.
///
/// # Examples
///
/// ```rust
/// assert!(oxigeo::cloud_detect::is_cloud_uri("s3://my-bucket/data.tif"));
/// assert!(oxigeo::cloud_detect::is_cloud_uri("gs://bucket/path/to/file.geojson"));
/// assert!(!oxigeo::cloud_detect::is_cloud_uri("/local/path/to/file.tif"));
/// assert!(!oxigeo::cloud_detect::is_cloud_uri("relative/path.tif"));
/// ```
pub fn is_cloud_uri(path: &str) -> bool {
    path.starts_with("s3://")
        || path.starts_with("gs://")
        || path.starts_with("az://")
        || path.starts_with("abfs://")
        || path.starts_with("http://")
        || path.starts_with("https://")
}

// ─── Dispatch ─────────────────────────────────────────────────────────────────

/// Open a cloud-hosted dataset, returning a lightweight [`Dataset`] descriptor.
///
/// When the `cloud` feature is enabled this function constructs a `Dataset`
/// whose metadata is inferred from the URL path extension (e.g. `"*.tif"` →
/// `DatasetFormat::GeoTiff`, feature-gated).  Actual pixel/feature data must
/// be fetched via the `oxigeo-cloud` streaming interface.
///
/// When the `cloud` feature is **not** enabled, always returns
/// [`OxiGeoError::NotSupported`].
///
/// # Errors
///
/// - [`OxiGeoError::NotSupported`] — `cloud` feature not compiled in.
/// - [`OxiGeoError::InvalidParameter`] — `path` is not a valid cloud URI.
pub fn open_cloud_dataset(path: &str) -> Result<Dataset> {
    if !is_cloud_uri(path) {
        return Err(OxiGeoError::InvalidParameter {
            parameter: "path",
            message: format!("'{}' is not a recognised cloud URI", path),
        });
    }

    #[cfg(feature = "cloud")]
    {
        open_cloud_with_feature(path)
    }

    #[cfg(not(feature = "cloud"))]
    {
        let _ = path;
        Err(OxiGeoError::NotSupported {
            operation: format!(
                "Dataset::open(\"{path}\") — cloud URIs require the 'cloud' feature flag",
            ),
        })
    }
}

/// Feature-gated inner implementation that creates the cloud Dataset stub.
#[cfg(feature = "cloud")]
fn open_cloud_with_feature(path: &str) -> Result<Dataset> {
    // Infer format from the URL extension for metadata purposes.
    let guessed_format = DatasetFormat::from_extension(path);

    let info = DatasetInfo {
        format: guessed_format,
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

    // Validate that the oxigeo-cloud crate can at least parse the URI scheme
    // (scheme-level parsing only, no network I/O).
    // On error we surface the problem early with a good message.
    validate_cloud_uri(path)?;

    Ok(crate::Dataset::from_info(path.to_string(), info))
}

/// Validate the cloud URI via the oxigeo-cloud crate (scheme-level check only).
///
/// Returns `Ok(())` if the URI is well-formed for the detected scheme.
#[cfg(feature = "cloud")]
fn validate_cloud_uri(path: &str) -> Result<()> {
    // Use oxigeo-cloud's URL parser for scheme validation.
    // `CloudBackend::from_url` does not initiate network I/O — it only parses.
    let _backend =
        oxigeo_cloud::CloudBackend::from_url(path).map_err(|e| OxiGeoError::InvalidParameter {
            parameter: "path",
            message: format!("invalid cloud URI '{}': {e}", path),
        })?;
    Ok(())
}
