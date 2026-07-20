//! American Polyconic projection.
//!
//! Each parallel is a non-concentric circular arc — hence "poly-conic".
//! Neither conformal nor equal-area, but useful for maps of moderate extent
//! along a central meridian. Historically used by the USGS for topographic
//! quadrangles.
//!
//! PROJ identifier: `+proj=poly`
//!
//! Reference: Snyder (1987) p. 124–130 (spherical form).
//!
//! All angles are in **radians**.

use crate::error::{Error, Result};

/// Polyconic forward projection (spherical form).
///
/// # Parameters
/// * `lon`, `lat` – geodetic coordinates in radians
/// * `lon_0` – central meridian in radians
/// * `lat_0` – origin latitude in radians
/// * `semi_major` – semi-major axis (metres)
///
/// # Errors
/// Returns an error if coordinates are non-finite.
pub fn polyconic_forward(
    lon: f64,
    lat: f64,
    lon_0: f64,
    lat_0: f64,
    semi_major: f64,
) -> Result<(f64, f64)> {
    if !lon.is_finite() || !lat.is_finite() {
        return Err(Error::invalid_coordinate("polyconic: non-finite input"));
    }

    let dlon = lon - lon_0;

    if lat.abs() < 1e-12 {
        // At equator: x = R * Δλ, y = -R * φ₀
        let x = semi_major * dlon;
        let y = -semi_major * lat_0;
        return Ok((x, y));
    }

    let cot_lat = lat.cos() / lat.sin();
    let e_val = dlon * lat.sin();

    let x = semi_major * cot_lat * e_val.sin();
    let y = semi_major * (lat - lat_0 + cot_lat * (1.0 - e_val.cos()));
    Ok((x, y))
}

/// Polyconic inverse projection (spherical form, iterative).
///
/// Uses Newton-Raphson iteration to invert the forward equations.
///
/// # Errors
/// Returns an error if coordinates are non-finite or convergence fails.
pub fn polyconic_inverse(
    x: f64,
    y: f64,
    lon_0: f64,
    lat_0: f64,
    semi_major: f64,
) -> Result<(f64, f64)> {
    if !x.is_finite() || !y.is_finite() {
        return Err(Error::invalid_coordinate("polyconic: non-finite input"));
    }

    let xr = x / semi_major;
    let yr = y / semi_major;
    let m0 = lat_0; // M₀/R = φ₀ for sphere

    // If y + M₀ ≈ 0 and x ≈ 0, we're near the equatorial singularity
    let b = yr + m0;

    if b.abs() < 1e-12 {
        // Near equator
        let lat = 0.0;
        let lon = xr + lon_0;
        return Ok((lon, lat));
    }

    // Newton-Raphson iteration
    // Start with initial guess: φ = B = y/R + φ₀
    let mut lat = b;
    let mut converged = false;

    const MAX_ITER: usize = 20;
    const TOL: f64 = 1e-12;

    for _ in 0..MAX_ITER {
        if lat.abs() < 1e-14 {
            // Converged near equator
            lat = 0.0;
            converged = true;
            break;
        }

        let sin_lat = lat.sin();
        let cos_lat = lat.cos();
        let tan_lat = sin_lat / cos_lat;
        let cot_lat = cos_lat / sin_lat;

        // A = tan(φ) * (x²·tan²(φ)/R² + 1) - 2(y/R + φ₀ - φ)·tan²(φ)
        // (Snyder eq 18-18 adapted for sphere)
        // Using the simplified approach from proj4:
        // F(φ) = φ - φ₀ + cot(φ)(1 - cos(E)) - y/R = 0, where E = (λ-λ₀)sin(φ)
        // And (x/R) = cot(φ) sin(E)

        // From x equation: sin(E) = (x/R) * tan(φ)
        let sin_e = xr * tan_lat;
        if sin_e.abs() > 1.0 + 1e-10 {
            // x is too large for this latitude — (x, y) is outside the
            // projection's valid domain for a genuine convergent solution.
            // Do NOT fabricate a result from the stale `lat`; leave
            // `converged = false` so the caller gets an error below.
            break;
        }
        let sin_e_clamped = sin_e.clamp(-1.0, 1.0);
        let e_val = sin_e_clamped.asin();
        let cos_e = e_val.cos();

        // f(φ) = φ - φ₀ + cot(φ)(1 - cos(E)) - yr
        let f = lat - lat_0 + cot_lat * (1.0 - cos_e) - yr;

        // f'(φ) = 1 - csc²(φ)(1 - cos(E)) + cot(φ)·cos(φ)·E'
        // where E' = dE/dφ
        // E = arcsin(xr · tan(φ))
        // dE/dφ = xr / (cos²(φ) · √(1 - xr²tan²(φ))) = xr·sec²(φ) / √(1 - sin²(E))
        let sec2 = 1.0 / (cos_lat * cos_lat);
        let de_dphi = xr * sec2 / (1.0 - sin_e_clamped * sin_e_clamped).sqrt().max(1e-15);

        let csc2 = 1.0 / (sin_lat * sin_lat);
        let fp = 1.0 - csc2 * (1.0 - cos_e) + cot_lat * sin_e_clamped * de_dphi;

        if fp.abs() < 1e-15 {
            // Degenerate derivative — cannot proceed with Newton-Raphson.
            break;
        }

        let delta = f / fp;
        lat -= delta;

        if delta.abs() < TOL {
            converged = true;
            break;
        }
    }

    if !converged {
        return Err(Error::invalid_coordinate(
            "polyconic inverse: failed to converge",
        ));
    }

    // Recover longitude from E = (λ − λ₀) sin(φ)
    let lon = if lat.abs() < 1e-12 {
        xr + lon_0
    } else {
        let sin_lat = lat.sin();
        let cos_lat = lat.cos();
        if cos_lat.abs() < 1e-15 {
            // Near pole — longitude is indeterminate
            lon_0
        } else {
            // sin(E) = (x/R) * tan(φ)  (from forward equation x = R cot φ sin E)
            let sin_e = (xr * sin_lat / cos_lat).clamp(-1.0, 1.0);
            let e_val = sin_e.asin();
            e_val / sin_lat + lon_0
        }
    };

    Ok((lon, lat))
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    const R: f64 = 6_371_000.0;

    #[test]
    fn test_polyconic_at_origin() {
        let lon_0 = (-96.0_f64).to_radians();
        let lat_0 = 37.0_f64.to_radians();

        let (x, y) = polyconic_forward(lon_0, lat_0, lon_0, lat_0, R).expect("ok");
        assert!(x.abs() < 1.0, "x at origin: {x}");
        assert!(y.abs() < 1.0, "y at origin: {y}");
    }

    #[test]
    fn test_polyconic_on_central_meridian() {
        let lon_0 = 0.0;
        let lat_0 = 0.0;
        let lat = 45.0_f64.to_radians();

        let (x, y) = polyconic_forward(lon_0, lat, lon_0, lat_0, R).expect("ok");
        // On central meridian, x should be ~0
        assert!(x.abs() < 1.0, "x on central meridian: {x}");
        // y should be R * (lat - lat_0)
        let expected_y = R * lat;
        assert!(
            (y - expected_y).abs() < 1.0,
            "y on meridian: {y} vs expected {expected_y}"
        );
    }

    #[test]
    fn test_polyconic_at_equator() {
        let lon_0 = 0.0;
        let lat_0 = 0.0;
        let lon = 30.0_f64.to_radians();

        let (x, y) = polyconic_forward(lon, 0.0, lon_0, lat_0, R).expect("ok");
        // At equator: x = R * Δλ, y = 0
        let expected_x = R * lon;
        assert!((x - expected_x).abs() < 1.0, "x at equator: {x}");
        assert!(y.abs() < 1.0, "y at equator: {y}");
    }

    #[test]
    fn test_polyconic_roundtrip() {
        let lon_0 = (-96.0_f64).to_radians();
        let lat_0 = 37.0_f64.to_radians();

        let cases = [
            ((-96.0_f64).to_radians(), 37.0_f64.to_radians()),
            ((-94.0_f64).to_radians(), 38.0_f64.to_radians()),
            ((-98.0_f64).to_radians(), 36.0_f64.to_radians()),
            ((-96.0_f64).to_radians(), 42.0_f64.to_radians()),
            ((-90.0_f64).to_radians(), 37.0_f64.to_radians()),
        ];
        for (lon, lat) in cases {
            let (x, y) = polyconic_forward(lon, lat, lon_0, lat_0, R).expect("fwd");
            let (lon2, lat2) = polyconic_inverse(x, y, lon_0, lat_0, R).expect("inv");
            assert!(
                (lon - lon2).abs() < 1e-6,
                "lon roundtrip: {:.5}° vs {:.5}°",
                lon.to_degrees(),
                lon2.to_degrees()
            );
            assert!(
                (lat - lat2).abs() < 1e-6,
                "lat roundtrip: {:.5}° vs {:.5}°",
                lat.to_degrees(),
                lat2.to_degrees()
            );
        }
    }

    #[test]
    fn test_polyconic_nonfinite() {
        assert!(polyconic_forward(f64::NAN, 0.0, 0.0, 0.0, R).is_err());
        assert!(polyconic_forward(f64::INFINITY, 0.0, 0.0, 0.0, R).is_err());
        assert!(polyconic_inverse(f64::NAN, 0.0, 0.0, 0.0, R).is_err());
    }

    #[test]
    fn test_polyconic_inverse_out_of_domain_errors_instead_of_garbage() {
        // Construct raw (x, y) — not from `polyconic_forward` — far outside
        // the projection's valid domain: at the initial-guess latitude
        // (~1 radian ≈ 57.3°), x is 10× the planetary radius, which makes
        // `sin_e = xr * tan(lat)` blow far past the [-1, 1] domain of
        // `asin`. Before the fix this silently returned `Ok` with a
        // plausible-looking but numerically meaningless coordinate (the
        // stale initial guess); it must now return an error instead.
        let lon_0 = 0.0;
        let lat_0 = 0.0;
        let x = 10.0 * R;
        let y = R; // yr = 1.0 rad -> initial lat guess ~57.3°

        let result = polyconic_inverse(x, y, lon_0, lat_0, R);
        assert!(
            result.is_err(),
            "expected an error for an out-of-domain (x, y), got {result:?}"
        );
    }

    #[test]
    fn test_polyconic_inverse_valid_roundtrip_still_converges() {
        // Sanity check that the `converged` bookkeeping did not break the
        // ordinary convergence path used by every other round-trip test.
        let lon_0 = (-96.0_f64).to_radians();
        let lat_0 = 37.0_f64.to_radians();
        let lon = (-95.0_f64).to_radians();
        let lat = 39.0_f64.to_radians();

        let (x, y) = polyconic_forward(lon, lat, lon_0, lat_0, R).expect("fwd");
        let (lon2, lat2) = polyconic_inverse(x, y, lon_0, lat_0, R).expect("inv must converge");
        assert!((lon - lon2).abs() < 1e-6);
        assert!((lat - lat2).abs() < 1e-6);
    }

    #[test]
    fn test_polyconic_equator_roundtrip() {
        let lon_0 = 0.0;
        let lat_0 = 0.0;
        let lon = 10.0_f64.to_radians();
        let lat = 0.0;

        let (x, y) = polyconic_forward(lon, lat, lon_0, lat_0, R).expect("fwd");
        let (lon2, lat2) = polyconic_inverse(x, y, lon_0, lat_0, R).expect("inv");
        assert!((lon - lon2).abs() < 1e-9, "equator lon: {lon} vs {lon2}");
        assert!((lat - lat2).abs() < 1e-9, "equator lat: {lat} vs {lat2}");
    }
}
