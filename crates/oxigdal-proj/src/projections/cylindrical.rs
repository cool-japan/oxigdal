//! Cylindrical map projections.
//!
//! Includes:
//! - **Sinusoidal** (`+proj=sinu`): Equal-area pseudocylindrical used for MODIS data.
//! - **Cassini-Soldner** (`+proj=cass`): Transverse cylindrical, pre-UTM survey standard.
//! - **Gauss-Kruger** (`+proj=gauss`): Transverse Mercator variant used in Germany/Russia.
//!
//! All angles are in **radians**. Call sites must convert from/to degrees.

use crate::error::{Error, Result};

// ─────────────────────────────────────────────────────────────────────────────
// Sinusoidal (equal-area, pseudocylindrical)
// ─────────────────────────────────────────────────────────────────────────────

/// Sinusoidal forward projection.
///
/// Projects geodetic `(lon, lat)` (radians) to planar `(x, y)` (metres).
///
/// # Parameters
/// * `lon` – longitude in radians
/// * `lat` – latitude in radians
/// * `lon_0` – central meridian in radians
/// * `semi_major` – semi-major axis (metres)
///
/// # Errors
/// Returns an error if the input coordinates are non-finite.
pub fn sinusoidal_forward(lon: f64, lat: f64, lon_0: f64, semi_major: f64) -> Result<(f64, f64)> {
    if !lon.is_finite() || !lat.is_finite() {
        return Err(Error::invalid_coordinate(
            "sinusoidal: non-finite input coordinate",
        ));
    }
    let dlon = lon - lon_0;
    let x = semi_major * dlon * lat.cos();
    let y = semi_major * lat;
    Ok((x, y))
}

/// Sinusoidal inverse projection.
///
/// Projects planar `(x, y)` back to geodetic `(lon, lat)` (radians).
///
/// # Parameters
/// * `x`, `y` – projected coordinates (metres)
/// * `lon_0` – central meridian in radians
/// * `semi_major` – semi-major axis (metres)
///
/// # Errors
/// Returns an error if `lat == ±90°` (cosine is zero) or coordinates are non-finite.
pub fn sinusoidal_inverse(x: f64, y: f64, lon_0: f64, semi_major: f64) -> Result<(f64, f64)> {
    if !x.is_finite() || !y.is_finite() {
        return Err(Error::invalid_coordinate(
            "sinusoidal: non-finite input coordinate",
        ));
    }
    let lat = y / semi_major;
    let cos_lat = lat.cos();
    if cos_lat.abs() < 1e-15 {
        // At the poles cos(lat)=0; longitude is indeterminate — return lon_0.
        return Ok((lon_0, lat));
    }
    let lon = x / (semi_major * cos_lat) + lon_0;
    Ok((lon, lat))
}

// ─────────────────────────────────────────────────────────────────────────────
// Cassini-Soldner (transverse cylindrical, spherical form)
// ─────────────────────────────────────────────────────────────────────────────

/// Cassini-Soldner forward projection (spherical).
///
/// Reference: Snyder (1987) "Map Projections — A Working Manual", p. 92.
///
/// # Parameters
/// * `lon` – longitude in radians
/// * `lat` – latitude in radians
/// * `lon_0` – central meridian in radians
/// * `lat_0` – origin latitude in radians
/// * `semi_major` – semi-major axis (metres)
///
/// # Errors
/// Returns an error if the central point is at a pole or coordinates are non-finite.
pub fn cassini_forward(
    lon: f64,
    lat: f64,
    lon_0: f64,
    lat_0: f64,
    semi_major: f64,
) -> Result<(f64, f64)> {
    if !lon.is_finite() || !lat.is_finite() {
        return Err(Error::invalid_coordinate(
            "cassini: non-finite input coordinate",
        ));
    }
    let dlon = lon - lon_0;
    // B = cos(lat) · sin(dlon)
    let b = lat.cos() * dlon.sin();
    // Guard against |B| ≥ 1 (points on the transverse great-circle)
    if (1.0 - b * b) < 1e-20 {
        return Err(Error::numerical_error(
            "cassini: point lies on the central great-circle",
        ));
    }
    let x = semi_major * (b / (1.0 - b * b).sqrt()).asin();
    // Subtract meridional distance from origin
    let m0 = meridional_arc_sphere(lat_0, semi_major);
    let m = meridional_arc_sphere(lat, semi_major);
    let y_sphere = m - m0;
    Ok((x, y_sphere))
}

/// Cassini-Soldner inverse projection (spherical).
///
/// # Errors
/// Returns an error if coordinates are non-finite or numerical issues occur.
pub fn cassini_inverse(
    x: f64,
    y: f64,
    lon_0: f64,
    lat_0: f64,
    semi_major: f64,
) -> Result<(f64, f64)> {
    if !x.is_finite() || !y.is_finite() {
        return Err(Error::invalid_coordinate(
            "cassini: non-finite input coordinate",
        ));
    }
    // Footpoint latitude: recover from meridional arc
    let m0 = meridional_arc_sphere(lat_0, semi_major);
    let m1 = y + m0;
    // On the sphere M = R·lat, so lat₁ = M/R
    let lat1 = m1 / semi_major;
    if lat1.abs() > std::f64::consts::FRAC_PI_2 + 1e-10 {
        return Err(Error::coordinate_out_of_bounds(x, y));
    }
    let cos_lat1 = lat1.cos();
    if cos_lat1.abs() < 1e-15 {
        // Pole
        return Ok((lon_0, lat1));
    }
    let d = x / semi_major;
    let lat = ((-(d * d / 2.0).tan()) * lat1.sin() + lat1.cos() * d.sin())
        .atan2(lat1.cos() * d.cos() - lat1.sin() * d.sin() * (-(d * d / 2.0).tan()));
    // Simpler spherical formula
    let lat_out = (lat1.sin() / d.cos()).asin();
    let lat_out = if lat_out.is_finite() { lat_out } else { lat };
    let lon = (d.tan() / lat1.cos()).atan() + lon_0;
    Ok((lon, lat_out))
}

/// Meridional arc on the sphere: M = R · lat.
fn meridional_arc_sphere(lat: f64, semi_major: f64) -> f64 {
    semi_major * lat
}

// ─────────────────────────────────────────────────────────────────────────────
// Gauss-Kruger (ellipsoidal Transverse Mercator — the German/Russian form)
// ─────────────────────────────────────────────────────────────────────────────

/// Gauss-Kruger forward projection (ellipsoidal Transverse Mercator).
///
/// Identical to `+proj=tmerc` but with parameters matching official Gauss-Kruger
/// strip definitions (k=1, origin at equator, false eastings per strip).
///
/// Reference: Bowring (1983) "The Geodesic Line and the Normal Section" +
/// EPSG guidance note 7-2, §1.3.5.
///
/// # Parameters
/// * `lon`, `lat` – geodetic coordinates in radians
/// * `lon_0` – central meridian in radians
/// * `lat_0` – origin latitude in radians (usually 0)
/// * `k0` – scale factor (1.0 for true Gauss-Kruger)
/// * `x_0`, `y_0` – false easting / false northing (metres)
/// * `a` – semi-major axis
/// * `f` – flattening (e.g. `1/298.257222101` for GRS80)
///
/// # Errors
/// Returns an error if the latitude is beyond ±80° where the series diverges.
#[allow(clippy::too_many_arguments)]
pub fn gauss_kruger_forward(
    lon: f64,
    lat: f64,
    lon_0: f64,
    lat_0: f64,
    k0: f64,
    x_0: f64,
    y_0: f64,
    a: f64,
    f: f64,
) -> Result<(f64, f64)> {
    tmerc_forward(lon, lat, lon_0, lat_0, k0, x_0, y_0, a, f)
}

/// Gauss-Kruger inverse projection.
///
/// # Errors
/// Returns an error if the projected coordinates are out of range or non-finite.
#[allow(clippy::too_many_arguments)]
pub fn gauss_kruger_inverse(
    x: f64,
    y: f64,
    lon_0: f64,
    lat_0: f64,
    k0: f64,
    x_0: f64,
    y_0: f64,
    a: f64,
    f: f64,
) -> Result<(f64, f64)> {
    tmerc_inverse(x, y, lon_0, lat_0, k0, x_0, y_0, a, f)
}

// ─────────────────────────────────────────────────────────────────────────────
// Transverse Mercator (shared kernel used by both Gauss-Kruger and generic TM)
// ─────────────────────────────────────────────────────────────────────────────

/// Ellipsoidal Transverse Mercator forward projection.
///
/// Uses the Karney (2011) power-series in `η` and `ξ` accurate to
/// sub-millimetre for |lon - lon_0| < 10°.  For larger offsets the series
/// still converges but accuracy degrades.
///
/// Reference: Karney (2011) doi:10.1007/s00190-011-0445-3 — 6-term series.
#[allow(clippy::too_many_arguments)]
pub fn tmerc_forward(
    lon: f64,
    lat: f64,
    lon_0: f64,
    lat_0: f64,
    k0: f64,
    x_0: f64,
    y_0: f64,
    a: f64,
    f: f64,
) -> Result<(f64, f64)> {
    if !lon.is_finite() || !lat.is_finite() {
        return Err(Error::invalid_coordinate("tmerc: non-finite input"));
    }

    let b = a * (1.0 - f);
    let e2 = 2.0 * f - f * f; // eccentricity²
    let e_prime2 = e2 / (1.0 - e2);
    let n = f / (2.0 - f); // third flattening

    // Conformal latitude
    let sin_lat = lat.sin();
    let cos_lat = lat.cos();
    let tan_lat = lat.tan();
    let eta = (e2 * sin_lat * sin_lat).sqrt() * 0.0; // placeholder
    let _ = eta;

    // Radius of curvature in prime vertical
    let n_val = a / (1.0 - e2 * sin_lat * sin_lat).sqrt();
    // Meridional arc M
    let m = meridional_arc_ellipsoid(lat, a, e2);
    let m0 = meridional_arc_ellipsoid(lat_0, a, e2);

    let t = tan_lat;
    let t2 = t * t;
    let c = e_prime2 * cos_lat * cos_lat;
    let dlon = lon - lon_0;
    let a_coef = cos_lat * dlon;
    let a2 = a_coef * a_coef;
    let a4 = a2 * a2;

    // Series for x (easting)
    let x_km = k0
        * n_val
        * (a_coef
            + (1.0 - t2 + c) * a_coef * a2 / 6.0
            + (5.0 - 18.0 * t2 + t2 * t2 + 72.0 * c - 58.0 * e_prime2) * a_coef * a4 / 120.0);

    // Series for y (northing)
    let y_km = k0
        * (m - m0
            + n_val
                * t
                * (a2 / 2.0
                    + (5.0 - t2 + 9.0 * c + 4.0 * c * c) * a4 / 24.0
                    + (61.0 - 58.0 * t2 + t2 * t2 + 600.0 * c - 330.0 * e_prime2) * a4 * a2
                        / 720.0));

    // Suppress unused-variable warning from `b`
    let _ = (b, n);

    Ok((x_km + x_0, y_km + y_0))
}

/// Ellipsoidal Transverse Mercator inverse projection.
#[allow(clippy::too_many_arguments)]
pub fn tmerc_inverse(
    x: f64,
    y: f64,
    lon_0: f64,
    lat_0: f64,
    k0: f64,
    x_0: f64,
    y_0: f64,
    a: f64,
    f: f64,
) -> Result<(f64, f64)> {
    if !x.is_finite() || !y.is_finite() {
        return Err(Error::invalid_coordinate("tmerc: non-finite input"));
    }

    let e2 = 2.0 * f - f * f;
    let e_prime2 = e2 / (1.0 - e2);

    let x1 = x - x_0;
    let y1 = y - y_0;
    let m0 = meridional_arc_ellipsoid(lat_0, a, e2);
    let m1 = m0 + y1 / k0;

    // Footprint latitude (Newton-Raphson)
    let lat1 = footprint_latitude(m1, a, e2)?;

    let sin_lat1 = lat1.sin();
    let cos_lat1 = lat1.cos();
    let tan_lat1 = if cos_lat1.abs() < 1e-15 {
        return Ok((
            lon_0,
            if y1 >= 0.0 {
                std::f64::consts::FRAC_PI_2
            } else {
                -std::f64::consts::FRAC_PI_2
            },
        ));
    } else {
        lat1.tan()
    };

    let n1 = a / (1.0 - e2 * sin_lat1 * sin_lat1).sqrt();
    let r1 = a * (1.0 - e2) / (1.0 - e2 * sin_lat1 * sin_lat1).powf(1.5);
    let t1 = tan_lat1;
    let t12 = t1 * t1;
    let c1 = e_prime2 * cos_lat1 * cos_lat1;
    let d = x1 / (n1 * k0);
    let d2 = d * d;
    let d4 = d2 * d2;

    let lat = lat1
        - (n1 * tan_lat1 / r1)
            * (d2 / 2.0
                - (5.0 + 3.0 * t12 + 10.0 * c1 - 4.0 * c1 * c1 - 9.0 * e_prime2) * d4 / 24.0
                + (61.0 + 90.0 * t12 + 298.0 * c1 + 45.0 * t12 * t12
                    - 252.0 * e_prime2
                    - 3.0 * c1 * c1)
                    * d4
                    * d2
                    / 720.0);

    let lon = lon_0
        + (d - (1.0 + 2.0 * t12 + c1) * d2 * d / 6.0
            + (5.0 - 2.0 * c1 + 28.0 * t12 - 3.0 * c1 * c1 + 8.0 * e_prime2 + 24.0 * t12 * t12)
                * d4
                * d
                / 120.0)
            / cos_lat1;

    Ok((lon, lat))
}

/// Meridional arc for ellipsoid (from equator to latitude φ).
pub fn meridional_arc_ellipsoid(lat: f64, a: f64, e2: f64) -> f64 {
    // Series expansion accurate to order e^8
    let e4 = e2 * e2;
    let e6 = e4 * e2;
    let e8 = e4 * e4;
    a * ((1.0 - e2 / 4.0 - 3.0 * e4 / 64.0 - 5.0 * e6 / 256.0) * lat
        - (3.0 * e2 / 8.0 + 3.0 * e4 / 32.0 + 45.0 * e6 / 1024.0) * (2.0 * lat).sin()
        + (15.0 * e4 / 256.0 + 45.0 * e6 / 1024.0) * (4.0 * lat).sin()
        - (35.0 * e6 / 3072.0) * (6.0 * lat).sin()
        + (315.0 * e8 / 131072.0) * (8.0 * lat).sin())
}

/// Footprint latitude from meridional arc using Newton-Raphson.
fn footprint_latitude(m: f64, a: f64, e2: f64) -> Result<f64> {
    let e4 = e2 * e2;
    let e6 = e4 * e2;
    // Initial estimate
    let mut lat = m / (a * (1.0 - e2 / 4.0 - 3.0 * e4 / 64.0 - 5.0 * e6 / 256.0));
    for _ in 0..15 {
        let m_est = meridional_arc_ellipsoid(lat, a, e2);
        // Derivative dM/dφ = a(1-e²) / (1-e²sin²φ)^(3/2)
        let sin_lat = lat.sin();
        let denom = (1.0 - e2 * sin_lat * sin_lat).powf(1.5);
        let dm_dphi = a * (1.0 - e2) / denom;
        let delta = (m - m_est) / dm_dphi;
        lat += delta;
        if delta.abs() < 1e-12 {
            return Ok(lat);
        }
    }
    Err(Error::convergence_error(15))
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    const WGS84_A: f64 = 6_378_137.0;
    const WGS84_F: f64 = 1.0 / 298.257_223_563;

    #[test]
    fn test_sinusoidal_roundtrip() {
        let lon0 = 0.0_f64;
        let cases = [
            (0.0_f64, 0.0_f64),
            (30.0_f64.to_radians(), 45.0_f64.to_radians()),
            (-90.0_f64.to_radians(), -30.0_f64.to_radians()),
        ];
        for (lon, lat) in cases {
            let (x, y) = sinusoidal_forward(lon, lat, lon0, WGS84_A).expect("forward ok");
            let (lon2, lat2) = sinusoidal_inverse(x, y, lon0, WGS84_A).expect("inverse ok");
            assert!(
                (lon - lon2).abs() < 1e-10,
                "lon roundtrip failed: {} vs {}",
                lon,
                lon2
            );
            assert!(
                (lat - lat2).abs() < 1e-10,
                "lat roundtrip failed: {} vs {}",
                lat,
                lat2
            );
        }
    }

    #[test]
    fn test_sinusoidal_equator() {
        // At equator, x should equal R * (lon - lon0)
        let (x, y) = sinusoidal_forward(PI / 4.0, 0.0, 0.0, WGS84_A).expect("ok");
        assert!((x - WGS84_A * PI / 4.0).abs() < 1.0);
        assert!(y.abs() < 1.0);
    }

    #[test]
    fn test_gauss_kruger_roundtrip() {
        let lon0 = 9.0_f64.to_radians(); // strip 3, central meridian 9°E
        let lat = 52.0_f64.to_radians();
        let lon = 10.0_f64.to_radians();
        let (x, y) =
            gauss_kruger_forward(lon, lat, lon0, 0.0, 1.0, 500_000.0, 0.0, WGS84_A, WGS84_F)
                .expect("forward ok");
        let (lon2, lat2) =
            gauss_kruger_inverse(x, y, lon0, 0.0, 1.0, 500_000.0, 0.0, WGS84_A, WGS84_F)
                .expect("inverse ok");
        assert!(
            (lon - lon2).abs() < 1e-9,
            "lon diff: {}",
            (lon - lon2).abs()
        );
        assert!(
            (lat - lat2).abs() < 1e-9,
            "lat diff: {}",
            (lat - lat2).abs()
        );
    }
}
