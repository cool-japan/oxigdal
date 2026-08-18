//! Universal Polar Stereographic (UPS) projection.
//!
//! UPS is the polar complement to UTM and covers latitudes poleward of ±80°.
//! It uses an ellipsoidal polar stereographic projection with:
//! - Scale factor k₀ = 0.994
//! - False easting / false northing = 2 000 000 m
//! - WGS-84 ellipsoid (a = 6 378 137 m, e = 0.0818191908426215)
//!
//! The forward and inverse formulae follow Snyder (1987) Chapter 21
//! ("Polar Stereographic"), equations 21-11 to 21-14.

use crate::error::{Error, Result};
use alloc::format;

// On `no_std` the inherent `f64` transcendental methods do not exist; this
// brings in the `libm`-backed stand-ins with identical signatures. Under `std`
// the inherent methods are used and the trait is not compiled at all.
// `allow(unused_imports)`: rustc gathers the inherent impls of primitive types
// from every crate *loaded into the compilation*, so whenever something drags
// `std` back in — an optional dependency (`proj4rs-compat` → `proj4rs`), or a
// dev-dependency under `--all-targets` — the inherent `f64::sin`/`cos`/… come
// back and win over the trait, leaving this import redundant there.
#[cfg(not(feature = "std"))]
#[allow(unused_imports)]
use crate::math::FloatExt;

// ─── WGS-84 constants ────────────────────────────────────────────────────────

/// WGS-84 semi-major axis (metres).
pub const WGS84_A: f64 = 6_378_137.0;

/// WGS-84 first eccentricity.
pub const WGS84_E: f64 = 0.081_819_190_842_621_5;

/// UPS scale factor at the natural origin.
pub const UPS_K0: f64 = 0.994;

/// UPS false easting (metres).
pub const UPS_FALSE_EASTING: f64 = 2_000_000.0;

/// UPS false northing (metres).
pub const UPS_FALSE_NORTHING: f64 = 2_000_000.0;

/// Minimum absolute latitude (degrees) inside the UPS validity band.
pub const UPS_MIN_LAT_DEG: f64 = 80.0;

// ─── Types ───────────────────────────────────────────────────────────────────

/// Hemisphere indicator for polar stereographic / UPS coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpsHemisphere {
    /// Northern hemisphere (pole at φ = +90°).
    North,
    /// Southern hemisphere (pole at φ = −90°).
    South,
}

/// A UPS (Universal Polar Stereographic) grid coordinate.
#[derive(Debug, Clone, Copy)]
pub struct UpsCoordinate {
    /// Grid easting (metres).
    pub easting: f64,
    /// Grid northing (metres).
    pub northing: f64,
    /// Hemisphere — determines which pole is the projection origin.
    pub hemisphere: UpsHemisphere,
}

/// Parameters for an ellipsoidal polar stereographic projection.
///
/// The same struct is used both for the standard UPS configuration and for
/// arbitrary polar stereographic setups (different ellipsoids or scale factors).
#[derive(Debug, Clone)]
pub struct PolarStereographicParams {
    /// Semi-major axis of the reference ellipsoid (metres).
    pub semimajor_a: f64,
    /// First eccentricity of the reference ellipsoid (dimensionless).
    pub eccentricity_e: f64,
    /// Scale factor at the natural origin.
    pub scale_factor_k0: f64,
    /// False easting (metres).
    pub false_easting: f64,
    /// False northing (metres).
    pub false_northing: f64,
    /// Hemisphere — determines which pole acts as the projection origin.
    pub hemisphere: UpsHemisphere,
}

impl PolarStereographicParams {
    /// Standard WGS-84 UPS parameters for the **north** pole.
    pub fn wgs84_ups_north() -> Self {
        Self {
            semimajor_a: WGS84_A,
            eccentricity_e: WGS84_E,
            scale_factor_k0: UPS_K0,
            false_easting: UPS_FALSE_EASTING,
            false_northing: UPS_FALSE_NORTHING,
            hemisphere: UpsHemisphere::North,
        }
    }

    /// Standard WGS-84 UPS parameters for the **south** pole.
    pub fn wgs84_ups_south() -> Self {
        Self {
            semimajor_a: WGS84_A,
            eccentricity_e: WGS84_E,
            scale_factor_k0: UPS_K0,
            false_easting: UPS_FALSE_EASTING,
            false_northing: UPS_FALSE_NORTHING,
            hemisphere: UpsHemisphere::South,
        }
    }
}

// ─── Core functions ───────────────────────────────────────────────────────────

/// Computes the denominator constant W used in the polar stereographic formulae.
///
/// W = √((1+e)^(1+e) · (1−e)^(1−e))
///
/// For WGS-84 this is approximately 1.003_24.
/// Snyder §21, eq. before (21-14).
pub fn polar_stereo_w(e: f64) -> f64 {
    let term1 = (1.0 + e).powf(1.0 + e);
    let term2 = (1.0 - e).powf(1.0 - e);
    (term1 * term2).sqrt()
}

/// Forward projection: geographic (lon, lat) in degrees → projected (E, N) in metres.
///
/// Follows Snyder (1987) §21, eqs. 21-11 to 21-14.
///
/// The south hemisphere variant works by negating the latitude before computing
/// the projection and then applying the appropriate northing sign convention.
pub fn polar_stereographic_forward(
    params: &PolarStereographicParams,
    lon_deg: f64,
    lat_deg: f64,
) -> (f64, f64) {
    let a = params.semimajor_a;
    let e = params.eccentricity_e;
    let k0 = params.scale_factor_k0;
    let fe = params.false_easting;
    let fn_ = params.false_northing;

    let lambda = lon_deg.to_radians();

    let phi_abs = match params.hemisphere {
        UpsHemisphere::North => lat_deg.to_radians(),
        UpsHemisphere::South => (-lat_deg).to_radians(),
    };

    let sin_phi = phi_abs.sin();
    let e_conformal = ((1.0 + e * sin_phi) / (1.0 - e * sin_phi)).powf(e / 2.0);

    let t = (core::f64::consts::FRAC_PI_4 - phi_abs / 2.0).tan() * e_conformal;

    let w = polar_stereo_w(e);
    let rho = 2.0 * a * k0 * t / w;

    let sin_lambda = lambda.sin();
    let cos_lambda = lambda.cos();

    let easting = fe + rho * sin_lambda;
    let northing = match params.hemisphere {
        UpsHemisphere::North => fn_ - rho * cos_lambda,
        UpsHemisphere::South => fn_ + rho * cos_lambda,
    };

    (easting, northing)
}

/// Inverse projection: projected (E, N) in metres → geographic (lon, lat) in degrees.
///
/// Follows Snyder (1987) §21, inverse formulae after eq. 21-14.
/// Latitude is found by fixed-point iteration (≤ 10 steps, tolerance 1e-12 rad).
pub fn polar_stereographic_inverse(
    params: &PolarStereographicParams,
    easting: f64,
    northing: f64,
) -> (f64, f64) {
    let a = params.semimajor_a;
    let e = params.eccentricity_e;
    let k0 = params.scale_factor_k0;
    let fe = params.false_easting;
    let fn_ = params.false_northing;

    let de = easting - fe;
    let dn = northing - fn_;

    let rho = (de * de + dn * dn).sqrt();
    let w = polar_stereo_w(e);
    let t = rho * w / (2.0 * a * k0);

    let mut phi = core::f64::consts::FRAC_PI_2 - 2.0 * t.atan();

    for _ in 0..10 {
        let sin_phi = phi.sin();
        let factor = ((1.0 - e * sin_phi) / (1.0 + e * sin_phi)).powf(e / 2.0);
        let phi_new = core::f64::consts::FRAC_PI_2 - 2.0 * (t * factor).atan();
        let delta = (phi_new - phi).abs();
        phi = phi_new;
        if delta < 1e-12 {
            break;
        }
    }

    let (lambda, lat_rad) = match params.hemisphere {
        UpsHemisphere::North => (de.atan2(-dn), phi),
        UpsHemisphere::South => (de.atan2(dn), -phi),
    };

    (lambda.to_degrees(), lat_rad.to_degrees())
}

// ─── Convenience functions ────────────────────────────────────────────────────

/// Converts WGS-84 geographic coordinates (degrees) to a UPS grid coordinate.
///
/// Returns an error if the point lies outside the UPS validity band
/// (i.e. |latitude| < 80°).
pub fn ups_from_geographic(lon_deg: f64, lat_deg: f64) -> Result<UpsCoordinate> {
    if lat_deg.abs() < UPS_MIN_LAT_DEG {
        return Err(Error::InvalidCoordinate {
            reason: format!(
                "UPS coverage requires |latitude| >= 80°, got ({}, {})",
                lon_deg, lat_deg
            ),
        });
    }

    let hemisphere = if lat_deg >= 0.0 {
        UpsHemisphere::North
    } else {
        UpsHemisphere::South
    };

    let params = match hemisphere {
        UpsHemisphere::North => PolarStereographicParams::wgs84_ups_north(),
        UpsHemisphere::South => PolarStereographicParams::wgs84_ups_south(),
    };

    let (easting, northing) = polar_stereographic_forward(&params, lon_deg, lat_deg);

    Ok(UpsCoordinate {
        easting,
        northing,
        hemisphere,
    })
}

/// Converts a UPS grid coordinate back to WGS-84 geographic coordinates (degrees).
///
/// Returns `(longitude_deg, latitude_deg)`.
pub fn ups_to_geographic(coord: &UpsCoordinate) -> (f64, f64) {
    let params = match coord.hemisphere {
        UpsHemisphere::North => PolarStereographicParams::wgs84_ups_north(),
        UpsHemisphere::South => PolarStereographicParams::wgs84_ups_south(),
    };
    polar_stereographic_inverse(&params, coord.easting, coord.northing)
}

/// Returns the MGRS / UPS zone letter for a geographic position.
///
/// - Lat ≥ 84°: 'Y' (lon < 0) or 'Z' (lon ≥ 0) — north UPS zone
/// - Lat ≤ −80°: 'A' (lon < 0) or 'B' (lon ≥ 0) — south UPS zone
/// - Otherwise: `None` (inside the UTM band)
pub fn ups_zone_letter(lon_deg: f64, lat_deg: f64) -> Option<char> {
    if lat_deg >= 84.0 {
        Some(if lon_deg < 0.0 { 'Y' } else { 'Z' })
    } else if lat_deg <= -80.0 {
        Some(if lon_deg < 0.0 { 'A' } else { 'B' })
    } else {
        None
    }
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_polar_stereo_w_sanity() {
        let w = polar_stereo_w(WGS84_E);
        assert!(
            (1.003..1.004).contains(&w),
            "W for WGS-84 should be ≈1.003xx, got {}",
            w
        );
    }

    #[test]
    fn test_north_pole_forward() {
        let params = PolarStereographicParams::wgs84_ups_north();
        let (e, n) = polar_stereographic_forward(&params, 0.0, 90.0);
        assert!((e - 2_000_000.0).abs() < 1.0, "easting at N pole: {}", e);
        assert!((n - 2_000_000.0).abs() < 1.0, "northing at N pole: {}", n);
    }

    #[test]
    fn test_south_pole_forward() {
        let params = PolarStereographicParams::wgs84_ups_south();
        let (e, n) = polar_stereographic_forward(&params, 0.0, -90.0);
        assert!((e - 2_000_000.0).abs() < 1.0, "easting at S pole: {}", e);
        assert!((n - 2_000_000.0).abs() < 1.0, "northing at S pole: {}", n);
    }

    #[test]
    fn test_round_trip_north() {
        let params = PolarStereographicParams::wgs84_ups_north();
        let (lon0, lat0) = (45.0_f64, 85.0_f64);
        let (e, n) = polar_stereographic_forward(&params, lon0, lat0);
        let (lon1, lat1) = polar_stereographic_inverse(&params, e, n);
        assert!(
            (lon1 - lon0).abs() < 1e-9,
            "longitude drift: {}",
            lon1 - lon0
        );
        assert!(
            (lat1 - lat0).abs() < 1e-9,
            "latitude drift: {}",
            lat1 - lat0
        );
    }

    #[test]
    fn test_round_trip_south() {
        let params = PolarStereographicParams::wgs84_ups_south();
        let (lon0, lat0) = (45.0_f64, -85.0_f64);
        let (e, n) = polar_stereographic_forward(&params, lon0, lat0);
        let (lon1, lat1) = polar_stereographic_inverse(&params, e, n);
        assert!(
            (lon1 - lon0).abs() < 1e-9,
            "longitude drift: {}",
            lon1 - lon0
        );
        assert!(
            (lat1 - lat0).abs() < 1e-9,
            "latitude drift: {}",
            lat1 - lat0
        );
    }

    #[test]
    fn test_ups_from_geographic_rejects_low_lat() {
        assert!(ups_from_geographic(0.0, 45.0).is_err());
        assert!(ups_from_geographic(0.0, -45.0).is_err());
        assert!(ups_from_geographic(0.0, 79.9).is_err());
    }

    #[test]
    fn test_ups_from_geographic_accepts_polar() {
        assert!(ups_from_geographic(0.0, 80.0).is_ok());
        assert!(ups_from_geographic(0.0, -80.0).is_ok());
        assert!(ups_from_geographic(0.0, 90.0).is_ok());
        assert!(ups_from_geographic(0.0, -90.0).is_ok());
    }
}
