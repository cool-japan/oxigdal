//! Azimuthal map projections.
//!
//! Includes:
//! - **Azimuthal Equidistant** (`+proj=aeqd`): Distances from the central point are true.
//! - **Gnomonic** (`+proj=gnom`): All great circles appear as straight lines.
//!
//! All angles are in **radians**.

use crate::error::{Error, Result};
use std::f64::consts::PI;

// ─────────────────────────────────────────────────────────────────────────────
// Azimuthal Equidistant
// ─────────────────────────────────────────────────────────────────────────────

/// Azimuthal Equidistant forward projection (spherical).
///
/// The projection is centred at `(lon_0, lat_0)`.  Distances and directions
/// from the centre are preserved.
///
/// Reference: Snyder (1987) p. 191.
///
/// # Parameters
/// * `lon`, `lat` – input geodetic coordinates in radians
/// * `lon_0`, `lat_0` – centre of projection in radians
/// * `semi_major` – sphere radius / semi-major axis (metres)
///
/// # Errors
/// Returns an error for non-finite inputs or the antipodal point (undefined).
pub fn azimuthal_equidistant_forward(
    lon: f64,
    lat: f64,
    lon_0: f64,
    lat_0: f64,
    semi_major: f64,
) -> Result<(f64, f64)> {
    if !lon.is_finite() || !lat.is_finite() {
        return Err(Error::invalid_coordinate("aeqd: non-finite input"));
    }

    let cos_c = lat_0.sin() * lat.sin() + lat_0.cos() * lat.cos() * (lon - lon_0).cos();
    // Clamp to avoid acos domain errors from floating-point noise
    let cos_c = cos_c.clamp(-1.0, 1.0);

    // Check for the centre point itself
    if (cos_c - 1.0).abs() < 1e-12 {
        return Ok((0.0, 0.0));
    }
    // Check for the antipodal point (c = π) — the projection is undefined there
    if (cos_c + 1.0).abs() < 1e-12 {
        return Err(Error::numerical_error(
            "aeqd: antipodal point — projection undefined",
        ));
    }

    let c = cos_c.acos(); // angular distance from centre
    let k = c / c.sin(); // k = c / sin c

    let x = semi_major * k * lat.cos() * (lon - lon_0).sin();
    let y =
        semi_major * k * (lat_0.cos() * lat.sin() - lat_0.sin() * lat.cos() * (lon - lon_0).cos());

    Ok((x, y))
}

/// Azimuthal Equidistant inverse projection (spherical).
///
/// # Errors
/// Returns an error for non-finite inputs or points outside the projection disc.
pub fn azimuthal_equidistant_inverse(
    x: f64,
    y: f64,
    lon_0: f64,
    lat_0: f64,
    semi_major: f64,
) -> Result<(f64, f64)> {
    if !x.is_finite() || !y.is_finite() {
        return Err(Error::invalid_coordinate("aeqd: non-finite input"));
    }

    let rho = (x * x + y * y).sqrt();

    // At the centre
    if rho < 1e-10 {
        return Ok((lon_0, lat_0));
    }

    let c = rho / semi_major;
    if c > PI + 1e-10 {
        return Err(Error::coordinate_out_of_bounds(x, y));
    }

    let sin_c = c.sin();
    let cos_c = c.cos();

    let lat = (cos_c * lat_0.sin() + y * sin_c * lat_0.cos() / rho)
        .clamp(-1.0, 1.0)
        .asin();

    let lon = if (lat_0.abs() - PI / 2.0).abs() < 1e-10 {
        // Polar case: lat_0 ≈ ±90°
        if lat_0 > 0.0 {
            lon_0 + x.atan2(-y)
        } else {
            lon_0 + x.atan2(y)
        }
    } else {
        lon_0 + (x * sin_c).atan2(rho * lat_0.cos() * cos_c - y * lat_0.sin() * sin_c)
    };

    Ok((lon, lat))
}

// ─────────────────────────────────────────────────────────────────────────────
// Gnomonic
// ─────────────────────────────────────────────────────────────────────────────

/// Gnomonic forward projection (spherical).
///
/// Projects from the centre of the sphere — all great-circle arcs appear as
/// straight lines.  Only one hemisphere can be represented.
///
/// Reference: Snyder (1987) p. 164.
///
/// # Parameters
/// * `lon`, `lat` – input geodetic coordinates in radians
/// * `lon_0`, `lat_0` – centre of projection in radians
/// * `semi_major` – sphere radius (metres)
///
/// # Errors
/// Returns an error for points in the opposite hemisphere (cos c ≤ 0) or
/// non-finite input.
pub fn gnomonic_forward(
    lon: f64,
    lat: f64,
    lon_0: f64,
    lat_0: f64,
    semi_major: f64,
) -> Result<(f64, f64)> {
    if !lon.is_finite() || !lat.is_finite() {
        return Err(Error::invalid_coordinate("gnom: non-finite input"));
    }

    let cos_c = lat_0.sin() * lat.sin() + lat_0.cos() * lat.cos() * (lon - lon_0).cos();

    if cos_c <= 0.0 {
        return Err(Error::numerical_error(
            "gnom: point is on or beyond the horizon (cos(c) ≤ 0)",
        ));
    }

    let x = semi_major * lat.cos() * (lon - lon_0).sin() / cos_c;
    let y = semi_major * (lat_0.cos() * lat.sin() - lat_0.sin() * lat.cos() * (lon - lon_0).cos())
        / cos_c;

    Ok((x, y))
}

/// Gnomonic inverse projection (spherical).
///
/// # Errors
/// Returns an error for non-finite inputs.
pub fn gnomonic_inverse(
    x: f64,
    y: f64,
    lon_0: f64,
    lat_0: f64,
    semi_major: f64,
) -> Result<(f64, f64)> {
    if !x.is_finite() || !y.is_finite() {
        return Err(Error::invalid_coordinate("gnom: non-finite input"));
    }

    let rho = (x * x + y * y).sqrt();
    let c = (rho / semi_major).atan();
    let sin_c = c.sin();
    let cos_c = c.cos();

    if rho < 1e-10 {
        return Ok((lon_0, lat_0));
    }

    let lat = (cos_c * lat_0.sin() + y * sin_c * lat_0.cos() / rho)
        .clamp(-1.0, 1.0)
        .asin();

    let lon = if (lat_0.abs() - PI / 2.0).abs() < 1e-10 {
        if lat_0 > 0.0 {
            lon_0 + x.atan2(-y)
        } else {
            lon_0 + x.atan2(y)
        }
    } else {
        lon_0 + (x * sin_c).atan2(rho * lat_0.cos() * cos_c - y * lat_0.sin() * sin_c)
    };

    Ok((lon, lat))
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    const R: f64 = 6_371_000.0;

    #[test]
    fn test_aeqd_centre_is_origin() {
        let lon_0 = 0.0_f64;
        let lat_0 = 0.0_f64;
        let (x, y) = azimuthal_equidistant_forward(lon_0, lat_0, lon_0, lat_0, R).expect("ok");
        assert!(x.abs() < 1e-6);
        assert!(y.abs() < 1e-6);
    }

    #[test]
    fn test_aeqd_roundtrip() {
        let lon_0 = 0.0_f64;
        let lat_0 = 45.0_f64.to_radians();
        let cases = [
            (10.0_f64.to_radians(), 50.0_f64.to_radians()),
            (-30.0_f64.to_radians(), 20.0_f64.to_radians()),
            (90.0_f64.to_radians(), 0.0_f64),
        ];
        for (lon, lat) in cases {
            let (x, y) =
                azimuthal_equidistant_forward(lon, lat, lon_0, lat_0, R).expect("forward ok");
            let (lon2, lat2) =
                azimuthal_equidistant_inverse(x, y, lon_0, lat_0, R).expect("inverse ok");
            assert!((lon - lon2).abs() < 1e-9, "lon: {lon:.4} vs {lon2:.4}");
            assert!((lat - lat2).abs() < 1e-9, "lat: {lat:.4} vs {lat2:.4}");
        }
    }

    #[test]
    fn test_gnomonic_roundtrip() {
        let lon_0 = 0.0_f64;
        let lat_0 = 45.0_f64.to_radians();
        // Use points close to centre (within ~60° to stay in valid hemisphere)
        let cases = [
            (10.0_f64.to_radians(), 50.0_f64.to_radians()),
            (-10.0_f64.to_radians(), 40.0_f64.to_radians()),
        ];
        for (lon, lat) in cases {
            let (x, y) = gnomonic_forward(lon, lat, lon_0, lat_0, R).expect("forward ok");
            let (lon2, lat2) = gnomonic_inverse(x, y, lon_0, lat_0, R).expect("inverse ok");
            assert!((lon - lon2).abs() < 1e-9, "lon: {lon:.4} vs {lon2:.4}");
            assert!((lat - lat2).abs() < 1e-9, "lat: {lat:.4} vs {lat2:.4}");
        }
    }

    #[test]
    fn test_gnomonic_opposite_hemisphere_rejected() {
        // South pole should fail from north-pole centre
        let result = gnomonic_forward(0.0, (-PI / 2.0) + 0.01, 0.0, PI / 2.0, R);
        assert!(result.is_err());
    }
}
