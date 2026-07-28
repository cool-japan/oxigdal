//! Additional world map projections (spherical forms).
//!
//! This module extends the native projection catalog with several classic
//! projections that were previously only reachable through the external
//! `oxiproj` PROJ-string engine (or not at all):
//!
//! - **Miller Cylindrical** (`+proj=mill`) — a compromise cylindrical
//!   projection derived from Mercator by compressing the latitude.
//! - **Craster Parabolic** / Putniņš P4 (`+proj=crast`) — an equal-area
//!   pseudocylindrical projection with parabolic meridians.
//! - **Bonne** (`+proj=bonne`) — an equal-area pseudoconic projection.
//! - **Werner** — the cordiform (heart-shaped) special case of Bonne with a
//!   polar standard parallel.
//! - **Hammer** / Hammer–Aitoff (`+proj=hammer`) — an equal-area modified
//!   azimuthal world projection.
//! - **Goode Homolosine** (uninterrupted) (`+proj=goode`) — an equal-area
//!   fusion of Sinusoidal (low latitudes) and Mollweide (high latitudes).
//!
//! All angles are in **radians**; the sphere radius is passed as `semi_major`
//! (metres). Reference: Snyder, *Map Projections — A Working Manual* (USGS
//! Professional Paper 1395, 1987).

use core::f64::consts::PI;

use crate::error::{Error, Result};

// ─────────────────────────────────────────────────────────────────────────────
// Miller Cylindrical (Snyder p. 88)
// ─────────────────────────────────────────────────────────────────────────────

/// Miller Cylindrical forward projection.
///
/// `x = R(λ − λ₀)`, `y = R·1.25·ln[tan(π/4 + 0.4φ)]`.
///
/// # Errors
/// Returns an error for non-finite inputs.
pub fn miller_forward(lon: f64, lat: f64, lon_0: f64, semi_major: f64) -> Result<(f64, f64)> {
    if !lon.is_finite() || !lat.is_finite() {
        return Err(Error::invalid_coordinate("miller: non-finite input"));
    }
    let x = semi_major * (lon - lon_0);
    let y = semi_major * 1.25 * (PI / 4.0 + 0.4 * lat).tan().ln();
    Ok((x, y))
}

/// Miller Cylindrical inverse projection.
///
/// `λ = λ₀ + x/R`, `φ = 2.5·[atan(e^{0.8y/R}) − π/4]`.
///
/// # Errors
/// Returns an error for non-finite inputs.
pub fn miller_inverse(x: f64, y: f64, lon_0: f64, semi_major: f64) -> Result<(f64, f64)> {
    if !x.is_finite() || !y.is_finite() {
        return Err(Error::invalid_coordinate("miller: non-finite input"));
    }
    let lon = lon_0 + x / semi_major;
    let lat = 2.5 * ((0.8 * y / semi_major).exp().atan() - PI / 4.0);
    Ok((lon, lat))
}

// ─────────────────────────────────────────────────────────────────────────────
// Craster Parabolic / Putniņš P4 (Snyder p. 231, equal-area)
// ─────────────────────────────────────────────────────────────────────────────

/// Craster Parabolic forward projection (equal-area).
///
/// `x = R·√(3/π)·(λ−λ₀)·[2cos(2φ/3) − 1]`, `y = R·√(3π)·sin(φ/3)`.
///
/// # Errors
/// Returns an error for non-finite inputs.
pub fn craster_forward(lon: f64, lat: f64, lon_0: f64, semi_major: f64) -> Result<(f64, f64)> {
    if !lon.is_finite() || !lat.is_finite() {
        return Err(Error::invalid_coordinate("craster: non-finite input"));
    }
    let sqrt_3_pi = (3.0 / PI).sqrt();
    let sqrt_3pi = (3.0 * PI).sqrt();
    let x = semi_major * sqrt_3_pi * (lon - lon_0) * (2.0 * (2.0 * lat / 3.0).cos() - 1.0);
    let y = semi_major * sqrt_3pi * (lat / 3.0).sin();
    Ok((x, y))
}

/// Craster Parabolic inverse projection.
///
/// # Errors
/// Returns an error for non-finite inputs or out-of-range `y`.
pub fn craster_inverse(x: f64, y: f64, lon_0: f64, semi_major: f64) -> Result<(f64, f64)> {
    if !x.is_finite() || !y.is_finite() {
        return Err(Error::invalid_coordinate("craster: non-finite input"));
    }
    let sqrt_3_pi = (3.0 / PI).sqrt();
    let sqrt_3pi = (3.0 * PI).sqrt();
    let s = y / (semi_major * sqrt_3pi);
    if s.abs() > 1.0 + 1e-12 {
        return Err(Error::coordinate_out_of_bounds(x, y));
    }
    let lat = 3.0 * s.clamp(-1.0, 1.0).asin();
    let denom = 2.0 * (2.0 * lat / 3.0).cos() - 1.0;
    let lon = if denom.abs() < 1e-15 {
        lon_0
    } else {
        lon_0 + x / (semi_major * sqrt_3_pi * denom)
    };
    Ok((lon, lat))
}

// ─────────────────────────────────────────────────────────────────────────────
// Bonne (Snyder p. 138, equal-area pseudoconic)
// ─────────────────────────────────────────────────────────────────────────────

/// Bonne forward projection (equal-area), standard parallel `lat_1` (radians).
///
/// For `lat_1 = ±π/2` this degenerates to the Werner projection (use
/// [`werner_forward`] which handles that case directly).
///
/// # Errors
/// Returns an error for non-finite inputs or `lat_1 == 0` (which degenerates
/// to the Sinusoidal projection — use that instead).
pub fn bonne_forward(
    lon: f64,
    lat: f64,
    lon_0: f64,
    lat_1: f64,
    semi_major: f64,
) -> Result<(f64, f64)> {
    if !lon.is_finite() || !lat.is_finite() {
        return Err(Error::invalid_coordinate("bonne: non-finite input"));
    }
    if lat_1.abs() < 1e-12 {
        return Err(Error::invalid_parameter(
            "lat_1",
            "Bonne is undefined for lat_1 = 0 (equator); use Sinusoidal",
        ));
    }
    let cot_phi1 = lat_1.cos() / lat_1.sin();
    let rho = cot_phi1 + lat_1 - lat;
    let e = if rho.abs() < 1e-15 {
        0.0
    } else {
        (lon - lon_0) * lat.cos() / rho
    };
    let x = semi_major * rho * e.sin();
    let y = semi_major * (cot_phi1 - rho * e.cos());
    Ok((x, y))
}

/// Bonne inverse projection.
///
/// # Errors
/// Returns an error for non-finite inputs or `lat_1 == 0`.
pub fn bonne_inverse(
    x: f64,
    y: f64,
    lon_0: f64,
    lat_1: f64,
    semi_major: f64,
) -> Result<(f64, f64)> {
    if !x.is_finite() || !y.is_finite() {
        return Err(Error::invalid_coordinate("bonne: non-finite input"));
    }
    if lat_1.abs() < 1e-12 {
        return Err(Error::invalid_parameter(
            "lat_1",
            "Bonne is undefined for lat_1 = 0 (equator); use Sinusoidal",
        ));
    }
    let cot_phi1 = lat_1.cos() / lat_1.sin();
    let xr = x / semi_major;
    let yr = y / semi_major;
    let dy = cot_phi1 - yr;
    // ρ takes the sign of lat_1 (Snyder eq. 10-8).
    let rho = (xr * xr + dy * dy).sqrt().copysign(lat_1);
    let lat = cot_phi1 + lat_1 - rho;
    let cos_lat = lat.cos();
    let lon = if cos_lat.abs() < 1e-12 {
        lon_0
    } else {
        // atan2 argument order per Snyder eq. 10-9 (sign folded through ρ).
        lon_0 + rho * xr.atan2(dy) / cos_lat
    };
    Ok((lon, lat))
}

// ─────────────────────────────────────────────────────────────────────────────
// Werner (cordiform Bonne, lat_1 = 90°)
// ─────────────────────────────────────────────────────────────────────────────

/// Werner cordiform forward projection (Bonne with `lat_1 = 90°`, equal-area).
///
/// # Errors
/// Returns an error for non-finite inputs.
pub fn werner_forward(lon: f64, lat: f64, lon_0: f64, semi_major: f64) -> Result<(f64, f64)> {
    if !lon.is_finite() || !lat.is_finite() {
        return Err(Error::invalid_coordinate("werner: non-finite input"));
    }
    // cot(90°) = 0, so ρ = π/2 − φ.
    let rho = PI / 2.0 - lat;
    let e = if rho.abs() < 1e-15 {
        0.0
    } else {
        (lon - lon_0) * lat.cos() / rho
    };
    let x = semi_major * rho * e.sin();
    let y = semi_major * (-rho * e.cos());
    Ok((x, y))
}

/// Werner cordiform inverse projection.
///
/// # Errors
/// Returns an error for non-finite inputs.
pub fn werner_inverse(x: f64, y: f64, lon_0: f64, semi_major: f64) -> Result<(f64, f64)> {
    if !x.is_finite() || !y.is_finite() {
        return Err(Error::invalid_coordinate("werner: non-finite input"));
    }
    let xr = x / semi_major;
    let yr = y / semi_major;
    let dy = -yr;
    let rho = (xr * xr + dy * dy).sqrt(); // lat_1 = +90° ⇒ ρ ≥ 0
    let lat = PI / 2.0 - rho;
    let cos_lat = lat.cos();
    let lon = if cos_lat.abs() < 1e-12 {
        lon_0
    } else {
        lon_0 + rho * xr.atan2(dy) / cos_lat
    };
    Ok((lon, lat))
}

// ─────────────────────────────────────────────────────────────────────────────
// Hammer / Hammer–Aitoff (Snyder p. 130, equal-area)
// ─────────────────────────────────────────────────────────────────────────────

/// Hammer forward projection (equal-area).
///
/// # Errors
/// Returns an error for non-finite inputs.
pub fn hammer_forward(lon: f64, lat: f64, lon_0: f64, semi_major: f64) -> Result<(f64, f64)> {
    if !lon.is_finite() || !lat.is_finite() {
        return Err(Error::invalid_coordinate("hammer: non-finite input"));
    }
    let dlon = lon - lon_0;
    let cos_lat = lat.cos();
    let d = (1.0 + cos_lat * (dlon / 2.0).cos()).sqrt();
    let x = semi_major * 2.0 * core::f64::consts::SQRT_2 * cos_lat * (dlon / 2.0).sin() / d;
    let y = semi_major * core::f64::consts::SQRT_2 * lat.sin() / d;
    Ok((x, y))
}

/// Hammer inverse projection.
///
/// # Errors
/// Returns an error for non-finite inputs or points outside the projection
/// ellipse.
pub fn hammer_inverse(x: f64, y: f64, lon_0: f64, semi_major: f64) -> Result<(f64, f64)> {
    if !x.is_finite() || !y.is_finite() {
        return Err(Error::invalid_coordinate("hammer: non-finite input"));
    }
    let u = x / semi_major;
    let v = y / semi_major;
    let inside = 1.0 - (u / 4.0).powi(2) - (v / 2.0).powi(2);
    if inside < -1e-12 {
        return Err(Error::coordinate_out_of_bounds(x, y));
    }
    let z = inside.max(0.0).sqrt();
    let two_z2m1 = 2.0 * z * z - 1.0;
    let lon = if two_z2m1.abs() < 1e-15 {
        lon_0
    } else {
        lon_0 + 2.0 * (z * u / (2.0 * two_z2m1)).atan()
    };
    let lat = (z * v).clamp(-1.0, 1.0).asin();
    Ok((lon, lat))
}

// ─────────────────────────────────────────────────────────────────────────────
// Goode Homolosine (uninterrupted) — Sinusoidal ∪ Mollweide (equal-area)
// ─────────────────────────────────────────────────────────────────────────────

/// Latitude (radians) at which the uninterrupted Goode Homolosine switches
/// from the Sinusoidal to the Mollweide projection: 40°44'11.8″.
const GOODE_TRANSITION_LAT: f64 = 0.710_987_989_993_15;

/// Goode Homolosine forward projection (uninterrupted, equal-area).
///
/// Uses the Sinusoidal projection for `|φ| ≤ 40°44′` and the Mollweide
/// projection (offset so the two join continuously) beyond it.
///
/// # Errors
/// Returns an error for non-finite inputs or Mollweide convergence failure.
pub fn goode_forward(lon: f64, lat: f64, lon_0: f64, semi_major: f64) -> Result<(f64, f64)> {
    if !lon.is_finite() || !lat.is_finite() {
        return Err(Error::invalid_coordinate("goode: non-finite input"));
    }
    if lat.abs() <= GOODE_TRANSITION_LAT {
        crate::projections::sinusoidal_forward(lon, lat, lon_0, semi_major)
    } else {
        let (x, y) = crate::projections::mollweide_forward(lon, lat, lon_0, semi_major)?;
        Ok((x, y - goode_offset(semi_major) * lat.signum()))
    }
}

/// Goode Homolosine inverse projection (uninterrupted).
///
/// # Errors
/// Returns an error for non-finite inputs or Mollweide domain violation.
pub fn goode_inverse(x: f64, y: f64, lon_0: f64, semi_major: f64) -> Result<(f64, f64)> {
    if !x.is_finite() || !y.is_finite() {
        return Err(Error::invalid_coordinate("goode: non-finite input"));
    }
    // Sinusoidal `y` at the transition latitude marks the seam.
    let seam_y = semi_major * GOODE_TRANSITION_LAT;
    if y.abs() <= seam_y {
        crate::projections::sinusoidal_inverse(x, y, lon_0, semi_major)
    } else {
        let y_moll = y + goode_offset(semi_major) * y.signum();
        crate::projections::mollweide_inverse(x, y_moll, lon_0, semi_major)
    }
}

/// Vertical offset applied to the Mollweide caps so they meet the Sinusoidal
/// belt continuously at the transition latitude.
fn goode_offset(semi_major: f64) -> f64 {
    // Sinusoidal y and Mollweide y both evaluated at the transition latitude;
    // their difference is the required cap shift.
    let sin_y = semi_major * GOODE_TRANSITION_LAT;
    let moll_y = crate::projections::mollweide_forward(0.0, GOODE_TRANSITION_LAT, 0.0, semi_major)
        .map(|(_, y)| y)
        .unwrap_or(sin_y);
    moll_y - sin_y
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    const R: f64 = 6_371_000.0;

    fn roundtrip_cases() -> [(f64, f64); 5] {
        [
            (0.0, 0.0),
            (30.0_f64.to_radians(), 45.0_f64.to_radians()),
            (-60.0_f64.to_radians(), -20.0_f64.to_radians()),
            (100.0_f64.to_radians(), 10.0_f64.to_radians()),
            (-15.0_f64.to_radians(), 70.0_f64.to_radians()),
        ]
    }

    #[test]
    fn test_miller_roundtrip() {
        for (lon, lat) in roundtrip_cases() {
            let (x, y) = miller_forward(lon, lat, 0.0, R).expect("fwd");
            let (lon2, lat2) = miller_inverse(x, y, 0.0, R).expect("inv");
            assert!((lon - lon2).abs() < 1e-9, "lon {lon} vs {lon2}");
            assert!((lat - lat2).abs() < 1e-9, "lat {lat} vs {lat2}");
        }
    }

    #[test]
    fn test_miller_reference_value() {
        // Independent hand computation at φ=45° with R=1:
        //   y = 1.25·ln[tan(π/4 + 0.4·45°)] = 1.25·ln[tan(63°)]
        //     = 1.25·ln(1.9626105) = 0.8428443…
        let (_, y) = miller_forward(0.0, 45.0_f64.to_radians(), 0.0, 1.0).expect("fwd");
        assert!((y - 0.842_844_3).abs() < 1e-6, "y/R = {y}");
    }

    #[test]
    fn test_craster_roundtrip() {
        for (lon, lat) in roundtrip_cases() {
            let (x, y) = craster_forward(lon, lat, 0.0, R).expect("fwd");
            let (lon2, lat2) = craster_inverse(x, y, 0.0, R).expect("inv");
            assert!((lon - lon2).abs() < 1e-9, "lon {lon} vs {lon2}");
            assert!((lat - lat2).abs() < 1e-9, "lat {lat} vs {lat2}");
        }
    }

    #[test]
    fn test_bonne_roundtrip() {
        let lat_1 = 40.0_f64.to_radians();
        for (lon, lat) in roundtrip_cases() {
            let (x, y) = bonne_forward(lon, lat, 0.0, lat_1, R).expect("fwd");
            let (lon2, lat2) = bonne_inverse(x, y, 0.0, lat_1, R).expect("inv");
            assert!((lon - lon2).abs() < 1e-8, "lon {lon} vs {lon2}");
            assert!((lat - lat2).abs() < 1e-8, "lat {lat} vs {lat2}");
        }
    }

    #[test]
    fn test_bonne_rejects_equator_parallel() {
        assert!(bonne_forward(0.1, 0.1, 0.0, 0.0, R).is_err());
    }

    #[test]
    fn test_werner_roundtrip() {
        for (lon, lat) in roundtrip_cases() {
            let (x, y) = werner_forward(lon, lat, 0.0, R).expect("fwd");
            let (lon2, lat2) = werner_inverse(x, y, 0.0, R).expect("inv");
            assert!((lon - lon2).abs() < 1e-8, "lon {lon} vs {lon2}");
            assert!((lat - lat2).abs() < 1e-8, "lat {lat} vs {lat2}");
        }
    }

    #[test]
    fn test_hammer_roundtrip() {
        for (lon, lat) in roundtrip_cases() {
            let (x, y) = hammer_forward(lon, lat, 0.0, R).expect("fwd");
            let (lon2, lat2) = hammer_inverse(x, y, 0.0, R).expect("inv");
            assert!((lon - lon2).abs() < 1e-8, "lon {lon} vs {lon2}");
            assert!((lat - lat2).abs() < 1e-8, "lat {lat} vs {lat2}");
        }
    }

    #[test]
    fn test_goode_roundtrip_both_regions() {
        // Low latitude (Sinusoidal region) and high latitude (Mollweide cap).
        for (lon, lat) in [
            (30.0_f64.to_radians(), 20.0_f64.to_radians()),
            (-45.0_f64.to_radians(), 60.0_f64.to_radians()),
            (10.0_f64.to_radians(), -75.0_f64.to_radians()),
        ] {
            let (x, y) = goode_forward(lon, lat, 0.0, R).expect("fwd");
            let (lon2, lat2) = goode_inverse(x, y, 0.0, R).expect("inv");
            assert!((lon - lon2).abs() < 1e-6, "lon {lon} vs {lon2}");
            assert!((lat - lat2).abs() < 1e-6, "lat {lat} vs {lat2}");
        }
    }
}
