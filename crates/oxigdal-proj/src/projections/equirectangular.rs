//! Equirectangular (Plate Carrée) projection.
//!
//! The simplest cylindrical projection: latitude → y, longitude → x scaled by
//! `cos(lat_ts)` where `lat_ts` is the "true-scale" latitude (often 0°).
//!
//! PROJ identifier: `+proj=eqc`
//!
//! Reference: Snyder (1987) p. 90.
//!
//! All angles are in **radians**.

use crate::error::{Error, Result};

/// Equirectangular forward projection.
///
/// # Parameters
/// * `lon`, `lat` – geodetic coordinates in radians
/// * `lon_0` – central meridian in radians
/// * `lat_ts` – latitude of true scale in radians (typically 0)
/// * `semi_major` – semi-major axis (metres)
///
/// # Errors
/// Returns an error if coordinates are non-finite.
pub fn equirectangular_forward(
    lon: f64,
    lat: f64,
    lon_0: f64,
    lat_ts: f64,
    semi_major: f64,
) -> Result<(f64, f64)> {
    if !lon.is_finite() || !lat.is_finite() {
        return Err(Error::invalid_coordinate(
            "equirectangular: non-finite input",
        ));
    }
    let x = semi_major * (lon - lon_0) * lat_ts.cos();
    let y = semi_major * lat;
    Ok((x, y))
}

/// Equirectangular inverse projection.
///
/// # Errors
/// Returns an error if coordinates are non-finite or `cos(lat_ts)` is zero.
pub fn equirectangular_inverse(
    x: f64,
    y: f64,
    lon_0: f64,
    lat_ts: f64,
    semi_major: f64,
) -> Result<(f64, f64)> {
    if !x.is_finite() || !y.is_finite() {
        return Err(Error::invalid_coordinate(
            "equirectangular: non-finite input",
        ));
    }
    let cos_ts = lat_ts.cos();
    if cos_ts.abs() < 1e-15 {
        return Err(Error::invalid_parameter(
            "equirectangular",
            "lat_ts = ±90° gives zero cosine",
        ));
    }
    let lat = y / semi_major;
    let lon = x / (semi_major * cos_ts) + lon_0;
    Ok((lon, lat))
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    const R: f64 = 6_371_000.0;
    const EPSILON: f64 = 1e-6;

    #[test]
    fn test_equirectangular_at_origin() {
        let (x, y) = equirectangular_forward(0.0, 0.0, 0.0, 0.0, R).expect("ok");
        assert!(x.abs() < EPSILON);
        assert!(y.abs() < EPSILON);
    }

    #[test]
    fn test_equirectangular_roundtrip() {
        let lon_0 = 0.0;
        let lat_ts = 0.0;
        let cases = [
            (45.0_f64.to_radians(), 30.0_f64.to_radians()),
            ((-120.0_f64).to_radians(), (-45.0_f64).to_radians()),
            (180.0_f64.to_radians(), 0.0),
        ];
        for (lon, lat) in cases {
            let (x, y) = equirectangular_forward(lon, lat, lon_0, lat_ts, R).expect("fwd");
            let (lon2, lat2) = equirectangular_inverse(x, y, lon_0, lat_ts, R).expect("inv");
            assert!((lon - lon2).abs() < 1e-9, "lon roundtrip: {lon} vs {lon2}");
            assert!((lat - lat2).abs() < 1e-9, "lat roundtrip: {lat} vs {lat2}");
        }
    }

    #[test]
    fn test_equirectangular_true_scale_45() {
        let lat_ts = 45.0_f64.to_radians();
        let lon = 90.0_f64.to_radians();
        let lat = 0.0_f64;

        let (x, y) = equirectangular_forward(lon, lat, 0.0, lat_ts, R).expect("fwd");
        // x should be R * π/2 * cos(45°)
        let expected_x = R * lon * lat_ts.cos();
        assert!((x - expected_x).abs() < 1.0, "x={x}, expected={expected_x}");
        assert!(y.abs() < EPSILON, "y at equator should be ~0: {y}");
    }

    #[test]
    fn test_equirectangular_nonfinite() {
        assert!(equirectangular_forward(f64::NAN, 0.0, 0.0, 0.0, R).is_err());
        assert!(equirectangular_forward(f64::INFINITY, 0.0, 0.0, 0.0, R).is_err());
        assert!(equirectangular_inverse(f64::NAN, 0.0, 0.0, 0.0, R).is_err());
    }

    #[test]
    fn test_equirectangular_pole_lat_ts() {
        // lat_ts = 90° should fail (cos = 0)
        let lat_ts = std::f64::consts::FRAC_PI_2;
        assert!(equirectangular_inverse(1000.0, 1000.0, 0.0, lat_ts, R).is_err());
    }
}
