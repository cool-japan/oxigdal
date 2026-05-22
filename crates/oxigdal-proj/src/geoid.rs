//! Geoid model loading + bilinear-interpolation undulation queries for vertical
//! CRS transformations.
//!
//! A *geoid* is the equipotential surface of Earth's gravity field that best
//! approximates mean sea level.  Orthometric heights are measured relative to
//! the geoid; ellipsoidal heights (what GNSS receivers report) are measured
//! relative to a reference ellipsoid (e.g. WGS84).  The two are related by the
//! geoid undulation `N`:
//!
//! ```text
//! h_ellipsoidal = h_orthometric + N
//! ```
//!
//! This module loads NGA-format little-endian f32 geoid grids (EGM96 and
//! EGM2008 are the most widely deployed models) and performs bilinear
//! interpolation to recover `N` at arbitrary (lat, lon) queries.  Longitude
//! queries wrap modulo 360°; latitude queries clamp to the grid extent.
//!
//! The crate ships with a **synthetic** geoid generator
//! ([`synthetic_grid`]) for testing and for use in environments where a real
//! grid file is not available.  Production code should load a vendor grid
//! (EGM96, EGM2008) via [`load_egm_grid`].
//!
//! # Examples
//!
//! ```
//! use oxigdal_proj::geoid::{GeoidModel, synthetic_grid};
//!
//! let grid = synthetic_grid(GeoidModel::Egm96);
//! let undulation_m = grid.geoid_height_m(35.0, 139.0);   // Tokyo (degrees)
//! let h_ortho = 50.0_f64;
//! let h_ellip = grid.orthometric_to_ellipsoidal(35.0, 139.0, h_ortho);
//! assert!((h_ellip - h_ortho - undulation_m).abs() < 1e-9);
//! ```

#[cfg(feature = "std")]
use std::path::Path;

use crate::error::Error;

#[cfg(not(feature = "std"))]
use alloc::format;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// Model identifier
// ---------------------------------------------------------------------------

/// Identifier for a known global geoid model.
///
/// EGM96 (15'×15' grid, ~5 cm RMS over land) and EGM2008 (1'×1' or 2.5'×2.5'
/// grids, ~1 cm RMS over land) are the most widely used global models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GeoidModel {
    /// Earth Gravitational Model 1996 (NGA / NASA).
    Egm96,
    /// Earth Gravitational Model 2008 (NGA).
    Egm2008,
}

impl GeoidModel {
    /// Returns the canonical short display name (`"EGM96"`, `"EGM2008"`).
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Egm96 => "EGM96",
            Self::Egm2008 => "EGM2008",
        }
    }

    /// Returns a reasonable default grid spacing in **degrees**.
    ///
    /// Both models support multiple grid resolutions; this returns a coarse
    /// value (0.25° = 15') suitable for synthetic-grid tests and for sizing
    /// expectations when no metadata is provided.
    pub fn default_grid_spacing_deg(&self) -> f64 {
        match self {
            Self::Egm96 => 0.25,
            Self::Egm2008 => 0.25,
        }
    }

    /// Returns the approximate accuracy of the model over land (centimetres).
    pub fn approx_accuracy_cm(&self) -> f64 {
        match self {
            Self::Egm96 => 50.0,
            Self::Egm2008 => 10.0,
        }
    }
}

impl core::fmt::Display for GeoidModel {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.display_name())
    }
}

// ---------------------------------------------------------------------------
// Grid container
// ---------------------------------------------------------------------------

/// A geoid undulation grid (north-to-south or south-to-north depending on
/// `lat_step_deg` sign; this container is agnostic and only requires
/// `lat_step_deg > 0` for the south-to-north convention used by
/// [`synthetic_grid`] and [`GeoidGrid::geoid_height_m`]).
///
/// Heights are stored in **metres**, row-major, indexed as
/// `heights_m[lat_idx * n_lon + lon_idx]`.  Latitude index `0` corresponds to
/// `lat_min_deg`; longitude index `0` corresponds to `lon_min_deg`.
#[derive(Debug, Clone)]
pub struct GeoidGrid {
    /// Source model identifier.
    pub model: GeoidModel,
    /// Latitude step between adjacent rows (degrees, positive ⇒ northward).
    pub lat_step_deg: f64,
    /// Longitude step between adjacent columns (degrees, positive ⇒ eastward).
    pub lon_step_deg: f64,
    /// Southernmost latitude of the grid (degrees).
    pub lat_min_deg: f64,
    /// Western longitude origin of the grid (degrees).
    pub lon_min_deg: f64,
    /// Number of latitude rows.
    pub n_lat: usize,
    /// Number of longitude columns.
    pub n_lon: usize,
    /// Row-major heights in metres, indexed by `[lat_idx * n_lon + lon_idx]`.
    pub heights_m: Vec<f32>,
}

// ---------------------------------------------------------------------------
// Synthetic grid (for tests and offline use)
// ---------------------------------------------------------------------------

/// Builds a synthetic 2°×2° global geoid grid using an analytic formula.
///
/// The height at `(lat, lon)` is computed as `30.0 * sin(lat) * cos(lon)`
/// (metres), which produces a smooth, plausible-looking but **non-physical**
/// undulation field suitable for round-trip testing.
///
/// The returned grid spans `lat ∈ [-90, 90]`, `lon ∈ [-180, 178]` (the
/// 180°-meridian wrap is handled by [`GeoidGrid::geoid_height_m`] via the
/// modulo-360° longitude operation).
pub fn synthetic_grid(model: GeoidModel) -> GeoidGrid {
    let lat_step_deg = 2.0_f64;
    let lon_step_deg = 2.0_f64;
    let lat_min_deg = -90.0_f64;
    let lon_min_deg = -180.0_f64;
    // South pole at row 0; north pole at row n_lat-1.  Inclusive bounds.
    let n_lat: usize = 91;
    let n_lon: usize = 180;
    let mut heights_m = Vec::with_capacity(n_lat * n_lon);
    for i in 0..n_lat {
        let lat_deg = lat_min_deg + (i as f64) * lat_step_deg;
        let lat_rad = lat_deg.to_radians();
        let sin_lat = lat_rad.sin();
        for j in 0..n_lon {
            let lon_deg = lon_min_deg + (j as f64) * lon_step_deg;
            let lon_rad = lon_deg.to_radians();
            let cos_lon = lon_rad.cos();
            heights_m.push((30.0_f64 * sin_lat * cos_lon) as f32);
        }
    }
    GeoidGrid {
        model,
        lat_step_deg,
        lon_step_deg,
        lat_min_deg,
        lon_min_deg,
        n_lat,
        n_lon,
        heights_m,
    }
}

/// Analytic synthetic height (m) used by [`synthetic_grid`] — exposed for
/// test verification.  This is **not** a physical geoid; it merely produces a
/// deterministic, smooth field.
pub fn synthetic_height_m(lat_deg: f64, lon_deg: f64) -> f64 {
    30.0_f64 * lat_deg.to_radians().sin() * lon_deg.to_radians().cos()
}

// ---------------------------------------------------------------------------
// File loader (NGA little-endian f32 layout)
// ---------------------------------------------------------------------------

/// Loads an NGA-format little-endian f32 geoid grid file.
///
/// The expected on-disk layout is row-major, with rows ordered south-to-north
/// (i.e. the first row corresponds to `lat_min_deg`) and columns ordered
/// west-to-east (the first column corresponds to `lon_min_deg`).  Each cell is
/// a little-endian IEEE-754 `f32`.
///
/// The file size **must** equal `n_lat * n_lon * 4` bytes; if it does not, an
/// [`Error::GeoidFileFormat`] is returned.  No header is assumed — callers
/// must supply the geometry parameters out-of-band (typically from a
/// companion `.bin.hdr`).
///
/// # Errors
///
/// * [`Error::GeoidFileFormat`] — file unreadable, or its size does not match
///   the declared geometry.
#[cfg(feature = "std")]
#[allow(clippy::too_many_arguments)]
pub fn load_egm_grid(
    path: &Path,
    model: GeoidModel,
    lat_min_deg: f64,
    lon_min_deg: f64,
    lat_step_deg: f64,
    lon_step_deg: f64,
    n_lat: usize,
    n_lon: usize,
) -> Result<GeoidGrid, Error> {
    let bytes = std::fs::read(path).map_err(|e| {
        Error::GeoidFileFormat(format!(
            "failed to read geoid grid file {}: {}",
            path.display(),
            e
        ))
    })?;
    let expected_len = n_lat * n_lon * 4;
    if bytes.len() != expected_len {
        return Err(Error::GeoidFileFormat(format!(
            "geoid grid file {} has {} bytes but declared geometry ({} × {} × 4 = {} bytes) does not match",
            path.display(),
            bytes.len(),
            n_lat,
            n_lon,
            expected_len
        )));
    }
    let mut heights_m: Vec<f32> = Vec::with_capacity(n_lat * n_lon);
    for chunk in bytes.chunks_exact(4) {
        let raw = [chunk[0], chunk[1], chunk[2], chunk[3]];
        heights_m.push(f32::from_le_bytes(raw));
    }
    Ok(GeoidGrid {
        model,
        lat_step_deg,
        lon_step_deg,
        lat_min_deg,
        lon_min_deg,
        n_lat,
        n_lon,
        heights_m,
    })
}

// ---------------------------------------------------------------------------
// Interpolation and height conversions
// ---------------------------------------------------------------------------

impl GeoidGrid {
    /// Returns the number of grid samples (`n_lat * n_lon`).
    pub fn len(&self) -> usize {
        self.n_lat.saturating_mul(self.n_lon)
    }

    /// Returns `true` when the grid contains no samples.
    pub fn is_empty(&self) -> bool {
        self.n_lat == 0 || self.n_lon == 0
    }

    /// Returns the height stored at exact grid node `(i_lat, j_lon)`.
    /// Out-of-bounds indices return `None`.
    pub fn height_at_node_m(&self, i_lat: usize, j_lon: usize) -> Option<f64> {
        if i_lat >= self.n_lat || j_lon >= self.n_lon {
            return None;
        }
        let idx = i_lat
            .checked_mul(self.n_lon)
            .and_then(|p| p.checked_add(j_lon))?;
        self.heights_m.get(idx).map(|h| *h as f64)
    }

    /// Bilinear-interpolated geoid undulation at `(lat_deg, lon_deg)`.
    ///
    /// * Longitude wraps modulo 360° relative to `lon_min_deg`, so queries
    ///   at +180° and −180° return identical values for a global grid.
    /// * Latitude **clamps** to the grid bounds — querying at 91° returns
    ///   the value at the northernmost row.
    /// * Returns `0.0` when the grid is empty (no samples).
    pub fn geoid_height_m(&self, lat_deg: f64, lon_deg: f64) -> f64 {
        if self.is_empty() {
            return 0.0;
        }

        // ----- latitude (clamp) ---------------------------------------------
        let lat_max = self.lat_min_deg + (self.n_lat.saturating_sub(1)) as f64 * self.lat_step_deg;
        let lat = lat_deg.clamp(self.lat_min_deg, lat_max);
        let lat_offset = lat - self.lat_min_deg;
        let lat_step = if self.lat_step_deg.abs() < f64::EPSILON {
            1.0
        } else {
            self.lat_step_deg
        };
        let lat_idx_f = lat_offset / lat_step;
        let lat_idx0_isize = lat_idx_f.floor() as isize;
        let lat_idx0 = if lat_idx0_isize < 0 {
            0
        } else {
            (lat_idx0_isize as usize).min(self.n_lat.saturating_sub(1))
        };
        let lat_idx1 = lat_idx0.saturating_add(1).min(self.n_lat.saturating_sub(1));
        let dt = (lat_idx_f - lat_idx0 as f64).clamp(0.0, 1.0);

        // ----- longitude (wrap modulo 360) ----------------------------------
        let lon_step = if self.lon_step_deg.abs() < f64::EPSILON {
            1.0
        } else {
            self.lon_step_deg
        };
        // Offset from origin, wrapped into [0, 360).
        let lon_offset = (lon_deg - self.lon_min_deg).rem_euclid(360.0);
        let lon_idx_f = lon_offset / lon_step;
        let lon_idx0_isize = lon_idx_f.floor() as isize;
        // Wrap the column index modulo n_lon (grids may not exactly cover
        // 360°; this matches the synthetic_grid layout of step=2°, n_lon=180).
        let n_lon = self.n_lon;
        let lon_idx0 = if n_lon == 0 {
            0
        } else {
            ((lon_idx0_isize.rem_euclid(n_lon as isize)) as usize) % n_lon
        };
        let lon_idx1 = if n_lon == 0 {
            0
        } else {
            (lon_idx0 + 1) % n_lon
        };
        let du = (lon_idx_f - lon_idx_f.floor()).clamp(0.0, 1.0);

        // ----- gather four corners ------------------------------------------
        let h00 = self.height_at_node_m(lat_idx0, lon_idx0).unwrap_or(0.0);
        let h01 = self.height_at_node_m(lat_idx0, lon_idx1).unwrap_or(0.0);
        let h10 = self.height_at_node_m(lat_idx1, lon_idx0).unwrap_or(0.0);
        let h11 = self.height_at_node_m(lat_idx1, lon_idx1).unwrap_or(0.0);

        // ----- bilinear interpolation ---------------------------------------
        let h0 = h00 * (1.0 - du) + h01 * du;
        let h1 = h10 * (1.0 - du) + h11 * du;
        h0 * (1.0 - dt) + h1 * dt
    }

    /// Converts an orthometric height (above the geoid) into an ellipsoidal
    /// height (above the reference ellipsoid) using `h_ellip = h_ortho + N`.
    pub fn orthometric_to_ellipsoidal(&self, lat_deg: f64, lon_deg: f64, h_ortho_m: f64) -> f64 {
        h_ortho_m + self.geoid_height_m(lat_deg, lon_deg)
    }

    /// Converts an ellipsoidal height into an orthometric height using
    /// `h_ortho = h_ellip − N`.
    pub fn ellipsoidal_to_orthometric(&self, lat_deg: f64, lon_deg: f64, h_ellip_m: f64) -> f64 {
        h_ellip_m - self.geoid_height_m(lat_deg, lon_deg)
    }
}

// ---------------------------------------------------------------------------
// Vertical-datum classification (used by the Transformer compound-CRS branch)
// ---------------------------------------------------------------------------

/// Coarse vertical-datum kind inferred from a CRS name / datum string.
///
/// Used by [`crate::transform::Transformer::transform_3d`] to decide whether a
/// geoid undulation correction is required between two compound CRS.  The
/// classification is **best-effort** and relies on simple substring matching
/// of well-known tokens (`"ellipsoid"`, `"geoid"`, `"egm"`, `"navd"`,
/// `"orthometric"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalDatumKind {
    /// Heights are referenced to the reference ellipsoid (e.g. WGS84).
    Ellipsoidal,
    /// Heights are referenced to a geoid (e.g. EGM96, EGM2008, NAVD88).
    Orthometric,
    /// Datum kind could not be classified from the available metadata.
    Unknown,
}

/// Classifies a vertical-CRS description string into a [`VerticalDatumKind`].
///
/// Matching is case-insensitive.  The first match wins in the order
/// orthometric → ellipsoidal, so a string containing both tokens is reported
/// as orthometric (the more specific category in practice).
pub fn classify_vertical_datum(description: &str) -> VerticalDatumKind {
    let lower = description.to_ascii_lowercase();
    const ORTHOMETRIC_TOKENS: &[&str] = &[
        "egm96",
        "egm2008",
        "egm",
        "navd88",
        "navd",
        "ngvd",
        "geoid",
        "orthometric",
        "mean sea level",
        "msl",
        "amsl",
    ];
    const ELLIPSOIDAL_TOKENS: &[&str] = &[
        "ellipsoid",
        "ellipsoidal",
        "wgs84 height",
        "wgs 84 height",
        "ellh",
    ];

    for tok in ORTHOMETRIC_TOKENS {
        if lower.contains(tok) {
            return VerticalDatumKind::Orthometric;
        }
    }
    for tok in ELLIPSOIDAL_TOKENS {
        if lower.contains(tok) {
            return VerticalDatumKind::Ellipsoidal;
        }
    }
    VerticalDatumKind::Unknown
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_geoid_model_display_name() {
        assert_eq!(GeoidModel::Egm96.display_name(), "EGM96");
        assert_eq!(GeoidModel::Egm2008.display_name(), "EGM2008");
        assert_eq!(format!("{}", GeoidModel::Egm96), "EGM96");
    }

    #[test]
    fn test_synthetic_grid_dimensions() {
        let g = synthetic_grid(GeoidModel::Egm96);
        assert_eq!(g.n_lat, 91);
        assert_eq!(g.n_lon, 180);
        assert_eq!(g.heights_m.len(), 91 * 180);
        assert!((g.lat_step_deg - 2.0).abs() < f64::EPSILON);
        assert!((g.lon_step_deg - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_grid_height_at_node_returns_stored_value() {
        let g = synthetic_grid(GeoidModel::Egm96);
        // Exact node at (lat=0, lon=0)
        let i_lat = ((0.0 - g.lat_min_deg) / g.lat_step_deg) as usize; // 45
        let j_lon = ((0.0 - g.lon_min_deg) / g.lon_step_deg) as usize; // 90
        let stored = g.height_at_node_m(i_lat, j_lon).expect("must be in bounds");
        let expected = synthetic_height_m(0.0, 0.0);
        assert!(
            (stored - expected).abs() < 1e-3,
            "stored={stored}, expected={expected}"
        );
    }

    #[test]
    fn test_classify_vertical_datum() {
        assert_eq!(
            classify_vertical_datum("EGM96 height"),
            VerticalDatumKind::Orthometric
        );
        assert_eq!(
            classify_vertical_datum("WGS 84 ellipsoidal height"),
            VerticalDatumKind::Ellipsoidal
        );
        assert_eq!(
            classify_vertical_datum("Some custom datum"),
            VerticalDatumKind::Unknown
        );
        assert_eq!(
            classify_vertical_datum("NAVD88"),
            VerticalDatumKind::Orthometric
        );
    }

    #[test]
    fn test_orthometric_ellipsoidal_round_trip_inline() {
        let g = synthetic_grid(GeoidModel::Egm2008);
        let h0 = 123.456_f64;
        let h_ellip = g.orthometric_to_ellipsoidal(30.0, 45.0, h0);
        let h_back = g.ellipsoidal_to_orthometric(30.0, 45.0, h_ellip);
        assert!((h_back - h0).abs() < 1e-9);
    }
}
