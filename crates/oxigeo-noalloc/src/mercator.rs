//! Web Mercator (EPSG:3857) forward and inverse projection.
//!
//! Operates entirely in `no_std` / `no_alloc` — uses the crate's built-in
//! `libm_sin`, `libm_ln`, `libm_exp`, and `libm_atan` implementations.
//!
//! ## Formulas
//!
//! **Forward** (lon/lat in degrees to metres):
//! ```text
//! x = R * lon_rad
//! y = R * ln(tan(pi/4 + lat_rad/2))
//! ```
//!
//! **Inverse** (metres to lon/lat in degrees):
//! ```text
//! lon = x / R   (in radians, then to degrees)
//! lat = 2 * atan(exp(y / R)) - pi/2   (in radians, then to degrees)
//! ```
//!
//! where `R = 6_378_137.0` m is the WGS 84 semi-major axis used by EPSG:3857.

use crate::{libm_atan, libm_exp, libm_ln, libm_sin};

/// WGS 84 semi-major axis (metres), used as the sphere radius in Web Mercator.
const EARTH_RADIUS: f64 = 6_378_137.0;

/// Degrees-to-radians conversion factor.
const DEG_TO_RAD: f64 = core::f64::consts::PI / 180.0;

/// Radians-to-degrees conversion factor.
const RAD_TO_DEG: f64 = 180.0 / core::f64::consts::PI;

/// Projects a geographic coordinate (latitude, longitude in **degrees**) to
/// Web Mercator (EPSG:3857) easting/northing in **metres**.
///
/// Returns `(x, y)` where `x` is easting and `y` is northing.
///
/// # Clamping
///
/// Latitude is clamped to `[-85.06, 85.06]` degrees (the standard Web Mercator
/// extent), preventing infinite Y values near the poles.
#[must_use]
pub fn mercator_forward(lat_deg: f64, lon_deg: f64) -> (f64, f64) {
    // Clamp latitude to the practical Web Mercator range
    let lat = clamp(lat_deg, -85.06, 85.06) * DEG_TO_RAD;
    let lon = lon_deg * DEG_TO_RAD;

    let x = EARTH_RADIUS * lon;

    // y = R * ln(tan(pi/4 + lat/2))
    // Equivalent to: y = R * ln((1 + sin(lat)) / (1 - sin(lat))) / 2
    // The second form avoids the tan(near pi/2) singularity and is more stable.
    let sin_lat = libm_sin(lat);
    let y = EARTH_RADIUS * 0.5 * libm_ln((1.0 + sin_lat) / (1.0 - sin_lat));

    (x, y)
}

/// Converts Web Mercator (EPSG:3857) easting/northing in **metres** back to
/// geographic coordinates (latitude, longitude in **degrees**).
///
/// Returns `(latitude, longitude)` in degrees.
#[must_use]
pub fn mercator_inverse(x: f64, y: f64) -> (f64, f64) {
    let lon_rad = x / EARTH_RADIUS;
    // lat = 2 * atan(exp(y/R)) - pi/2
    let lat_rad = 2.0 * libm_atan(libm_exp(y / EARTH_RADIUS)) - core::f64::consts::FRAC_PI_2;

    let lat_deg = lat_rad * RAD_TO_DEG;
    let lon_deg = lon_rad * RAD_TO_DEG;

    (lat_deg, lon_deg)
}

/// Clamp a value to `[lo, hi]` without std.
#[inline(always)]
fn clamp(x: f64, lo: f64, hi: f64) -> f64 {
    if x < lo {
        lo
    } else if x > hi {
        hi
    } else {
        x
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: check that two f64 values are within `eps` of each other.
    fn approx_eq(a: f64, b: f64, eps: f64) -> bool {
        let diff = if a > b { a - b } else { b - a };
        diff < eps
    }

    // ── libm function tests ──────────────────────────────────────────────

    #[test]
    fn test_libm_ln_basic() {
        // ln(1) = 0
        assert!(approx_eq(libm_ln(1.0), 0.0, 1e-12));
        // ln(e) = 1
        assert!(approx_eq(libm_ln(core::f64::consts::E), 1.0, 1e-12));
        // ln(e^2) = 2
        assert!(approx_eq(
            libm_ln(core::f64::consts::E * core::f64::consts::E),
            2.0,
            1e-11
        ));
    }

    #[test]
    fn test_libm_ln_various() {
        // ln(2) ≈ 0.6931471805599453
        assert!(approx_eq(libm_ln(2.0), core::f64::consts::LN_2, 1e-12));
        // ln(10) ≈ 2.302585092994046
        assert!(approx_eq(libm_ln(10.0), core::f64::consts::LN_10, 1e-11));
        // ln(0.5) = -ln(2)
        assert!(approx_eq(libm_ln(0.5), -core::f64::consts::LN_2, 1e-12));
    }

    #[test]
    fn test_libm_ln_edge_cases() {
        assert!(libm_ln(-1.0).is_nan());
        assert!(libm_ln(0.0) == f64::NEG_INFINITY);
        assert!(libm_ln(f64::NAN).is_nan());
        assert!(libm_ln(f64::INFINITY) == f64::INFINITY);
    }

    #[test]
    fn test_libm_exp_basic() {
        // exp(0) = 1
        assert!(approx_eq(libm_exp(0.0), 1.0, 1e-14));
        // exp(1) = e
        assert!(approx_eq(libm_exp(1.0), core::f64::consts::E, 1e-12));
        // exp(-1) = 1/e
        assert!(approx_eq(libm_exp(-1.0), 1.0 / core::f64::consts::E, 1e-12));
    }

    #[test]
    fn test_libm_exp_various() {
        // exp(2) ≈ 7.389056
        assert!(approx_eq(libm_exp(2.0), 7.389_056_098_930_65, 1e-10));
        // exp(ln(5)) = 5
        assert!(approx_eq(libm_exp(libm_ln(5.0)), 5.0, 1e-10));
    }

    #[test]
    fn test_libm_exp_edge_cases() {
        assert!(libm_exp(f64::NAN).is_nan());
        assert!(libm_exp(f64::INFINITY) == f64::INFINITY);
        assert!(approx_eq(libm_exp(f64::NEG_INFINITY), 0.0, f64::EPSILON));
        // Very large positive
        assert!(libm_exp(1000.0) == f64::INFINITY);
        // Very large negative
        assert!(approx_eq(libm_exp(-1000.0), 0.0, f64::EPSILON));
    }

    #[test]
    fn test_libm_atan_basic() {
        // atan(0) = 0
        assert!(approx_eq(libm_atan(0.0), 0.0, 1e-14));
        // atan(1) = pi/4
        assert!(approx_eq(
            libm_atan(1.0),
            core::f64::consts::FRAC_PI_4,
            1e-10
        ));
        // atan(-1) = -pi/4
        assert!(approx_eq(
            libm_atan(-1.0),
            -core::f64::consts::FRAC_PI_4,
            1e-10
        ));
    }

    #[test]
    fn test_libm_atan_large() {
        // atan(very large) → pi/2
        assert!(approx_eq(
            libm_atan(1e15),
            core::f64::consts::FRAC_PI_2,
            1e-10
        ));
        // atan(very negative) → -pi/2
        assert!(approx_eq(
            libm_atan(-1e15),
            -core::f64::consts::FRAC_PI_2,
            1e-10
        ));
    }

    #[test]
    fn test_libm_atan_nan() {
        assert!(libm_atan(f64::NAN).is_nan());
    }

    // ── Mercator projection tests ────────────────────────────────────────

    #[test]
    fn test_forward_origin() {
        let (x, y) = mercator_forward(0.0, 0.0);
        assert!(approx_eq(x, 0.0, 1.0));
        assert!(approx_eq(y, 0.0, 1.0));
    }

    #[test]
    fn test_forward_london() {
        // London: lat 51.5074, lon -0.1278
        // Expected (approx): x ≈ -14221.45, y ≈ 6711542.84
        let (x, y) = mercator_forward(51.5074, -0.1278);
        assert!(approx_eq(x, -14_226.5, 100.0)); // within 100 m
        assert!(approx_eq(y, 6_711_542.0, 500.0));
    }

    #[test]
    fn test_forward_tokyo() {
        // Tokyo: lat 35.6762, lon 139.6503
        // Expected: x ≈ 15545800.29, y ≈ 4256157.98 (Web Mercator EPSG:3857)
        let (x, y) = mercator_forward(35.6762, 139.6503);
        assert!(approx_eq(x, 15_545_800.0, 500.0));
        assert!(approx_eq(y, 4_256_158.0, 500.0));
    }

    #[test]
    fn test_forward_southern_hemisphere() {
        // Sydney: lat -33.8688, lon 151.2093
        let (x, y) = mercator_forward(-33.8688, 151.2093);
        assert!(x > 0.0, "Sydney has positive easting");
        assert!(y < 0.0, "Southern hemisphere has negative northing");
    }

    #[test]
    fn test_forward_clamping() {
        // Latitude beyond 85.06 should be clamped
        let (_, y1) = mercator_forward(85.06, 0.0);
        let (_, y2) = mercator_forward(90.0, 0.0);
        // Both should produce the same Y because 90 is clamped to 85.06
        assert!(approx_eq(y1, y2, 1.0));
    }

    #[test]
    fn test_round_trip() {
        let test_cases = [
            (0.0, 0.0),
            (45.0, 90.0),
            (-33.0, 151.0),
            (51.5, -0.1),
            (35.7, 139.7),
            (-22.9, -43.2),
            (85.0, 179.0),
            (-85.0, -179.0),
        ];
        for (lat, lon) in test_cases {
            let (x, y) = mercator_forward(lat, lon);
            let (lat2, lon2) = mercator_inverse(x, y);
            assert!(
                approx_eq(lat, lat2, 1e-6),
                "lat round-trip failed: {lat} vs {lat2}"
            );
            assert!(
                approx_eq(lon, lon2, 1e-6),
                "lon round-trip failed: {lon} vs {lon2}"
            );
        }
    }

    #[test]
    fn test_inverse_origin() {
        let (lat, lon) = mercator_inverse(0.0, 0.0);
        assert!(approx_eq(lat, 0.0, 1e-10));
        assert!(approx_eq(lon, 0.0, 1e-10));
    }

    #[test]
    fn test_symmetry() {
        // Symmetric latitude should produce symmetric Y
        let (_, y_pos) = mercator_forward(45.0, 0.0);
        let (_, y_neg) = mercator_forward(-45.0, 0.0);
        assert!(approx_eq(y_pos, -y_neg, 1.0));
    }

    #[test]
    fn test_equator_y_zero() {
        let (_, y) = mercator_forward(0.0, 45.0);
        assert!(approx_eq(y, 0.0, 0.01));
    }
}
