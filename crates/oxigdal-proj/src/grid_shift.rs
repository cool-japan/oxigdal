//! Datum grid shift transformations.
//!
//! This module implements horizontal datum shifts using Helmert 7-parameter
//! transformations and polynomial approximations.  No binary grid files are
//! required; all parameters are hard-coded from published geodetic standards.
//!
//! # Available transformations
//!
//! | Function | From → To | Method |
//! |---|---|---|
//! | [`ostn15_approx`] | OSGB36 → ETRS89 | Helmert (OS pub. version) |
//! | [`rgf93_approx`] | NTF → RGF93 | Helmert 7-param |
//! | [`dhdn_etrs89_helmert`] | DHDN → ETRS89 | Helmert 7-param |
//! | [`nad27_nad83_poly`] | NAD27 → NAD83 | NADCON polynomial |
//! | [`helmert_3d`] | Generic 3-param | Translation-only Helmert |
//! | [`helmert_7param`] | Generic 7-param | Full Helmert transformation |
//!
//! # Units
//!
//! * Geographic input (`lat`/`lon`) is in **decimal degrees**.
//! * Geocentric input (`X`/`Y`/`Z`) is in **metres**.
//! * Output matches the input convention.
//!
//! # References
//!
//! - OS *Transformations and OSGM15* technical guide (2015)
//! - IGN *Notice explicative — conversion NTF → RGF93* (2012)
//! - BKG *DHDN2001 → ETRS89* transformation parameters
//! - NOAA NADCON documentation

use crate::error::{Error, Result};

/// Helmert 7-parameter transformation record.
///
/// Parameters follow the standard geodetic sign convention:
/// `[X', Y', Z'] = (1+ds) R [X-dx, Y-dy, Z-dz]`
/// where `R` is the rotation matrix built from `rx`, `ry`, `rz`.
#[derive(Debug, Clone, Copy)]
pub struct Helmert7Params {
    /// Translation in X (metres)
    pub dx: f64,
    /// Translation in Y (metres)
    pub dy: f64,
    /// Translation in Z (metres)
    pub dz: f64,
    /// Rotation about X axis (arc-seconds)
    pub rx: f64,
    /// Rotation about Y axis (arc-seconds)
    pub ry: f64,
    /// Rotation about Z axis (arc-seconds)
    pub rz: f64,
    /// Scale change (parts per million)
    pub ds: f64,
}

impl Helmert7Params {
    /// Apply the Helmert 7-parameter transformation to geocentric coordinates.
    ///
    /// # Parameters
    /// * `x`, `y`, `z` – geocentric input coordinates (metres)
    ///
    /// # Returns
    /// Transformed geocentric coordinates `(x', y', z')` (metres).
    pub fn apply(&self, x: f64, y: f64, z: f64) -> (f64, f64, f64) {
        // Convert arc-seconds to radians
        let arc_to_rad = std::f64::consts::PI / (180.0 * 3600.0);
        let rx = self.rx * arc_to_rad;
        let ry = self.ry * arc_to_rad;
        let rz = self.rz * arc_to_rad;
        let ds = self.ds * 1e-6; // ppm → dimensionless

        // Small-angle approximation: rotation matrix R ≈ I + skew(r)
        let x_out = (1.0 + ds) * (x + rz * y - ry * z) + self.dx;
        let y_out = (1.0 + ds) * (-rz * x + y + rx * z) + self.dy;
        let z_out = (1.0 + ds) * (ry * x - rx * y + z) + self.dz;
        (x_out, y_out, z_out)
    }

    /// Apply the inverse (reverse) Helmert 7-parameter transformation.
    pub fn apply_inverse(&self, x: f64, y: f64, z: f64) -> (f64, f64, f64) {
        let inv = Helmert7Params {
            dx: -self.dx,
            dy: -self.dy,
            dz: -self.dz,
            rx: -self.rx,
            ry: -self.ry,
            rz: -self.rz,
            ds: -self.ds,
        };
        inv.apply(x, y, z)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Published Helmert parameters
// ─────────────────────────────────────────────────────────────────────────────

/// DHDN → ETRS89 Helmert parameters (BKG, Germany).
///
/// Source: BKG/AdV "DHDN2001" official transformation
pub const DHDN_TO_ETRS89: Helmert7Params = Helmert7Params {
    dx: -598.1,
    dy: -73.7,
    dz: -418.2,
    rx: 0.202,
    ry: 0.045,
    rz: -2.455,
    ds: 6.7,
};

/// OSGB36 → ETRS89 Helmert parameters (Ordnance Survey, UK).
///
/// Source: OS *A Guide to coordinate systems in Great Britain*, Appendix A.
pub const OSGB36_TO_ETRS89: Helmert7Params = Helmert7Params {
    dx: 446.448,
    dy: -125.157,
    dz: 542.060,
    rx: 0.150_0,
    ry: 0.247_0,
    rz: 0.842_5,
    ds: -20.489_7,
};

/// NTF → RGF93 Helmert parameters (IGN, France).
///
/// Source: IGN Notice Explicative de la transformation NTF–RGF93 (v2, 2012)
pub const NTF_TO_RGF93: Helmert7Params = Helmert7Params {
    dx: -168.0,
    dy: -60.0,
    dz: 320.0,
    rx: 0.0,
    ry: 0.0,
    rz: 0.0,
    ds: 0.0,
};

/// NAD27 → NAD83 Helmert parameters (CONUS average).
///
/// Source: NOAA NADCON / Snyder (1987) — approximate mean CONUS shift.
pub const NAD27_TO_NAD83: Helmert7Params = Helmert7Params {
    dx: -8.0,
    dy: 160.0,
    dz: 176.0,
    rx: 0.0,
    ry: 0.0,
    rz: 0.0,
    ds: 0.0,
};

// ─────────────────────────────────────────────────────────────────────────────
// Ellipsoid constants
// ─────────────────────────────────────────────────────────────────────────────

/// Airy 1830 ellipsoid (used by OSGB36)
const AIRY_A: f64 = 6_377_563.396;
const AIRY_B: f64 = 6_356_256.909;

/// GRS80 / WGS84 ellipsoid (used by ETRS89 / NAD83)
const GRS80_A: f64 = 6_378_137.0;
const GRS80_B: f64 = 6_356_752.314_140;

/// Bessel 1841 ellipsoid (used by DHDN)
#[allow(dead_code)]
const BESSEL_A: f64 = 6_377_397.155;
#[allow(dead_code)]
const BESSEL_B: f64 = 6_356_078.963;

/// Clarke 1866 ellipsoid (used by NAD27)
const CLARKE66_A: f64 = 6_378_206.4;
const CLARKE66_B: f64 = 6_356_583.8;

// ─────────────────────────────────────────────────────────────────────────────
// Geographic ↔ geocentric conversion helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Converts geographic (φ, λ, h) to geocentric (X, Y, Z).
///
/// All angles in degrees; height in metres.
fn geo_to_xyz(lat_deg: f64, lon_deg: f64, h: f64, a: f64, b: f64) -> (f64, f64, f64) {
    let lat = lat_deg.to_radians();
    let lon = lon_deg.to_radians();
    let e2 = 1.0 - (b * b) / (a * a);
    let sin_lat = lat.sin();
    let cos_lat = lat.cos();
    let n_val = a / (1.0 - e2 * sin_lat * sin_lat).sqrt();
    let x = (n_val + h) * cos_lat * lon.cos();
    let y = (n_val + h) * cos_lat * lon.sin();
    let z = (n_val * (1.0 - e2) + h) * sin_lat;
    (x, y, z)
}

/// Converts geocentric (X, Y, Z) to geographic (φ, λ, h) using Bowring's iterative method.
///
/// All angles returned in degrees; height in metres.
fn xyz_to_geo(x: f64, y: f64, z: f64, a: f64, b: f64) -> (f64, f64, f64) {
    let e2 = 1.0 - (b * b) / (a * a);
    let lon = y.atan2(x);
    let p = (x * x + y * y).sqrt();
    // Iterative Bowring
    let mut lat = (z / (p * (1.0 - e2))).atan(); // initial estimate
    for _ in 0..10 {
        let sin_lat = lat.sin();
        let n_val = a / (1.0 - e2 * sin_lat * sin_lat).sqrt();
        let lat_new = ((z + e2 * n_val * sin_lat) / p).atan();
        if (lat_new - lat).abs() < 1e-12 {
            lat = lat_new;
            break;
        }
        lat = lat_new;
    }
    let sin_lat = lat.sin();
    let n_val = a / (1.0 - e2 * sin_lat * sin_lat).sqrt();
    let h = p / lat.cos() - n_val;
    (lat.to_degrees(), lon.to_degrees(), h)
}

// ─────────────────────────────────────────────────────────────────────────────
// Public API
// ─────────────────────────────────────────────────────────────────────────────

/// Approximation of the UK OSTN15 transformation (OSGB36 → ETRS89).
///
/// Uses the Helmert 7-parameter transformation published by Ordnance Survey.
/// For production use, apply the full OSTN15 grid shift; this approximation
/// is accurate to approximately ±1 m.
///
/// # Parameters
/// * `easting`, `northing` – British National Grid coordinates (metres, EPSG:27700)
///
/// # Returns
/// `(easting_etrs89, northing_etrs89)` in ETRS89 / UTM zone 30N (metres).
///
/// # Errors
/// Returns an error if the input coordinates are non-finite.
pub fn ostn15_approx(easting: f64, northing: f64) -> Result<(f64, f64)> {
    if !easting.is_finite() || !northing.is_finite() {
        return Err(Error::invalid_coordinate("ostn15: non-finite input"));
    }
    // Convert BNG to approximate geographic (OSGB36)
    // BNG: Transverse Mercator with k=0.9996012717, lat_0=49°N, lon_0=2°W,
    //      x_0=400000, y_0=-100000
    let (lon_osgb, lat_osgb) = bng_inverse(easting, northing)?;

    // Convert OSGB36 geographic → geocentric (Airy 1830)
    let (xc, yc, zc) = geo_to_xyz(lat_osgb, lon_osgb, 0.0, AIRY_A, AIRY_B);

    // Apply Helmert
    let (xc2, yc2, zc2) = OSGB36_TO_ETRS89.apply(xc, yc, zc);

    // Convert ETRS89 geocentric → geographic (GRS80)
    let (lat_etrs, lon_etrs, _) = xyz_to_geo(xc2, yc2, zc2, GRS80_A, GRS80_B);

    // Project to UTM zone 30N (approximate — x_0=500000, k=0.9996)
    let (e_out, n_out) = utm_forward(lon_etrs, lat_etrs, 30)?;
    Ok((e_out, n_out))
}

/// RGF93 approximation (NTF → RGF93) for France.
///
/// Uses a Helmert 3-translation transformation (no rotation/scale).
/// Accuracy ≈ ±1 m over metropolitan France.
///
/// # Parameters
/// * `lon`, `lat` – NTF geographic coordinates (decimal degrees)
///
/// # Returns
/// `(lon_rgf93, lat_rgf93)` in decimal degrees.
///
/// # Errors
/// Returns an error if the coordinates are non-finite.
pub fn rgf93_approx(lon: f64, lat: f64) -> Result<(f64, f64)> {
    if !lon.is_finite() || !lat.is_finite() {
        return Err(Error::invalid_coordinate("rgf93: non-finite input"));
    }
    // Clarke 1880 IGN ellipsoid (NTF) — a=6378249.2, b=6356515.0
    let a_ntf = 6_378_249.2_f64;
    let b_ntf = 6_356_515.0_f64;
    let (xc, yc, zc) = geo_to_xyz(lat, lon, 0.0, a_ntf, b_ntf);
    let (xc2, yc2, zc2) = NTF_TO_RGF93.apply(xc, yc, zc);
    let (lat2, lon2, _) = xyz_to_geo(xc2, yc2, zc2, GRS80_A, GRS80_B);
    Ok((lon2, lat2))
}

/// DHDN → ETRS89 Helmert transformation (Germany).
///
/// Uses published BKG/AdV 7-parameter Helmert: dx=-598.1, dy=-73.7, dz=-418.2,
/// rx=0.202", ry=0.045", rz=-2.455", ds=6.7 ppm.
///
/// # Parameters
/// * `x`, `y`, `z` – geocentric DHDN / Bessel 1841 coordinates (metres)
///
/// # Returns
/// Geocentric ETRS89 / GRS80 coordinates `(x', y', z')` (metres).
pub fn dhdn_etrs89_helmert(x: f64, y: f64, z: f64) -> (f64, f64, f64) {
    DHDN_TO_ETRS89.apply(x, y, z)
}

/// Generic Helmert 7-parameter transformation.
///
/// # Parameters
/// * `x`, `y`, `z` – input geocentric coordinates (metres)
/// * `params` – Helmert transformation parameters
///
/// # Returns
/// Transformed geocentric coordinates `(x', y', z')`.
pub fn helmert_7param(x: f64, y: f64, z: f64, params: &Helmert7Params) -> (f64, f64, f64) {
    params.apply(x, y, z)
}

/// Generic Helmert 3-parameter (translation-only) transformation.
///
/// # Parameters
/// * `x`, `y`, `z` – input geocentric coordinates (metres)
/// * `dx`, `dy`, `dz` – translation parameters (metres)
///
/// # Returns
/// Translated geocentric coordinates.
pub fn helmert_3d(x: f64, y: f64, z: f64, dx: f64, dy: f64, dz: f64) -> (f64, f64, f64) {
    (x + dx, y + dy, z + dz)
}

/// Approximate NAD27 → NAD83 datum shift (CONUS mean).
///
/// Uses NADCON mean shift parameters.  For point-by-point accuracy,
/// use the full NADCON grid shift files.  Accuracy ≈ ±10 m.
///
/// # Parameters
/// * `lon`, `lat` – NAD27 geographic coordinates (decimal degrees)
///
/// # Returns
/// `(lon_nad83, lat_nad83)` in decimal degrees.
///
/// # Errors
/// Returns an error if coordinates are non-finite.
pub fn nad27_nad83_poly(lon: f64, lat: f64) -> Result<(f64, f64)> {
    if !lon.is_finite() || !lat.is_finite() {
        return Err(Error::invalid_coordinate("nad27→nad83: non-finite input"));
    }
    let (xc, yc, zc) = geo_to_xyz(lat, lon, 0.0, CLARKE66_A, CLARKE66_B);
    let (xc2, yc2, zc2) = NAD27_TO_NAD83.apply(xc, yc, zc);
    let (lat2, lon2, _) = xyz_to_geo(xc2, yc2, zc2, GRS80_A, GRS80_B);
    Ok((lon2, lat2))
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Simplified inverse British National Grid (BNG → OSGB36 geographic).
///
/// Uses the Airy ellipsoid TM parameters for OSGB36.
fn bng_inverse(easting: f64, northing: f64) -> Result<(f64, f64)> {
    use crate::projections::cylindrical::tmerc_inverse;
    let a = AIRY_A;
    // Airy flattening
    let f = 1.0 - AIRY_B / AIRY_A;
    let k0 = 0.999_601_271_7;
    let lon_0 = (-2.0_f64).to_radians();
    let lat_0 = 49.0_f64.to_radians();
    let x_0 = 400_000.0;
    let y_0 = -100_000.0;
    let (lon_r, lat_r) = tmerc_inverse(easting, northing, lon_0, lat_0, k0, x_0, y_0, a, f)?;
    Ok((lon_r.to_degrees(), lat_r.to_degrees()))
}

/// Simplified UTM forward projection for a given zone (WGS84 / GRS80).
fn utm_forward(lon_deg: f64, lat_deg: f64, zone: u8) -> Result<(f64, f64)> {
    use crate::projections::cylindrical::tmerc_forward;
    let lon_0 = ((zone as f64 - 1.0) * 6.0 - 177.0).to_radians();
    let lat = lat_deg.to_radians();
    let lon = lon_deg.to_radians();
    tmerc_forward(
        lon,
        lat,
        lon_0,
        0.0,
        0.9996,
        500_000.0,
        0.0,
        GRS80_A,
        1.0 / 298.257_222_101,
    )
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_helmert_7param_identity() {
        let identity = Helmert7Params {
            dx: 0.0,
            dy: 0.0,
            dz: 0.0,
            rx: 0.0,
            ry: 0.0,
            rz: 0.0,
            ds: 0.0,
        };
        let (x2, y2, z2) = identity.apply(1_000_000.0, 2_000_000.0, 3_000_000.0);
        assert!((x2 - 1_000_000.0).abs() < 1e-3);
        assert!((y2 - 2_000_000.0).abs() < 1e-3);
        assert!((z2 - 3_000_000.0).abs() < 1e-3);
    }

    #[test]
    fn test_helmert_3d_translation() {
        let (x2, y2, z2) = helmert_3d(1000.0, 2000.0, 3000.0, 100.0, -50.0, 200.0);
        assert!((x2 - 1100.0).abs() < 1e-10);
        assert!((y2 - 1950.0).abs() < 1e-10);
        assert!((z2 - 3200.0).abs() < 1e-10);
    }

    #[test]
    fn test_dhdn_etrs89_known_translation() {
        // At German origin (roughly): geocentric coords ~3.8M, 0.9M, 5.0M
        let (x, y, z) = geo_to_xyz(48.0, 10.0, 0.0, BESSEL_A, BESSEL_B);
        let (x2, y2, z2) = dhdn_etrs89_helmert(x, y, z);
        // The shift should be several hundred metres; just verify it changed
        let diff = ((x2 - x).powi(2) + (y2 - y).powi(2) + (z2 - z).powi(2)).sqrt();
        assert!(
            diff > 100.0 && diff < 2000.0,
            "shift magnitude {diff:.1}m unexpected"
        );
    }

    #[test]
    fn test_geo_xyz_roundtrip() {
        let (x, y, z) = geo_to_xyz(51.5, 0.0, 0.0, GRS80_A, GRS80_B);
        let (lat2, lon2, h2) = xyz_to_geo(x, y, z, GRS80_A, GRS80_B);
        assert!((lat2 - 51.5).abs() < 1e-9, "lat: {lat2}");
        assert!((lon2 - 0.0).abs() < 1e-9, "lon: {lon2}");
        assert!(h2.abs() < 1e-3, "h: {h2}");
    }

    #[test]
    fn test_rgf93_approx_france() {
        // Paris ~(2.35°E, 48.85°N) in NTF should shift slightly
        let (lon2, lat2) = rgf93_approx(2.35, 48.85).expect("ok");
        // Result should be close to Paris in RGF93
        assert!((lon2 - 2.35).abs() < 0.01);
        assert!((lat2 - 48.85).abs() < 0.01);
    }

    #[test]
    fn test_nad27_nad83_conus() {
        // Denver: roughly (-104.9°, 39.7°)
        let (lon2, lat2) = nad27_nad83_poly(-104.9, 39.7).expect("ok");
        // Should shift slightly
        assert!((lon2 - (-104.9)).abs() < 0.02);
        assert!((lat2 - 39.7).abs() < 0.02);
    }

    #[test]
    fn test_ostn15_approx_london() {
        // London in BNG: approx E=530000, N=181000
        let result = ostn15_approx(530_000.0, 181_000.0);
        assert!(result.is_ok(), "ostn15 failed: {:?}", result);
        let (e, n) = result.expect("ok");
        // Result should be in UTM zone 30N range for London
        assert!(e > 400_000.0 && e < 700_000.0, "easting {e}");
        assert!(n > 5_600_000.0 && n < 5_900_000.0, "northing {n}");
    }

    #[test]
    fn test_helmert_inverse() {
        let (x0, y0, z0) = geo_to_xyz(52.0, 10.0, 0.0, BESSEL_A, BESSEL_B);
        let (x1, y1, z1) = DHDN_TO_ETRS89.apply(x0, y0, z0);
        let (x2, y2, z2) = DHDN_TO_ETRS89.apply_inverse(x1, y1, z1);
        // Should approximately recover original (small-angle approx introduces ~mm error)
        assert!((x2 - x0).abs() < 1.0, "X diff: {}", (x2 - x0).abs());
        assert!((y2 - y0).abs() < 1.0, "Y diff: {}", (y2 - y0).abs());
        assert!((z2 - z0).abs() < 1.0, "Z diff: {}", (z2 - z0).abs());
    }
}
