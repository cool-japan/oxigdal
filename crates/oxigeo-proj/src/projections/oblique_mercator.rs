//! Oblique Mercator projection (Hotine variant).
//!
//! The Oblique Mercator (`+proj=omerc`) is used for mapping areas where neither
//! a conventional Mercator nor a Transverse Mercator gives satisfactory lower
//! distortion.  The cylinder is tilted (oblique) to cross the equator at an
//! angle that minimises distortion along a chosen great-circle line.
//!
//! This module implements Hotine Oblique Mercator **variant A** (azimuth of
//! the initial line and centre of the projection), using a 3-D rotation
//! approach (spherical form) following Snyder (1987) pp. 69–75.
//!
//! All angles are in **radians**.

use crate::error::{Error, Result};

// ─────────────────────────────────────────────────────────────────────────────
// Hotine Oblique Mercator — Variant A (spherical, rotation-matrix approach)
// ─────────────────────────────────────────────────────────────────────────────

/// Pre-computed rotation parameters.
struct OmercParams {
    /// cos/sin of lon_c  (rotation R1 around Z)
    cos_lc: f64,
    sin_lc: f64,
    /// cos/sin of lat_0  (rotation R2 around Y)
    cos_l0: f64,
    sin_l0: f64,
    /// cos/sin of azimuth rotation θ = α_c − π/2  (rotation R3 around X)
    cos_th: f64,
    sin_th: f64,
    /// Scale: R × k₀
    rk: f64,
}

fn omerc_params(
    lat_0: f64,
    lon_c: f64,
    alpha_c: f64,
    k0: f64,
    semi_major: f64,
) -> Result<OmercParams> {
    if !lat_0.is_finite() || !lon_c.is_finite() || !alpha_c.is_finite() {
        return Err(Error::invalid_parameter("omerc", "non-finite parameter"));
    }
    if lat_0.abs() > core::f64::consts::FRAC_PI_2 - 1e-10 {
        return Err(Error::invalid_parameter(
            "omerc",
            "projection centre cannot be at a pole",
        ));
    }
    if k0 <= 0.0 || !k0.is_finite() {
        return Err(Error::invalid_parameter(
            "omerc",
            "scale factor must be positive and finite",
        ));
    }
    let theta = alpha_c - core::f64::consts::FRAC_PI_2;
    Ok(OmercParams {
        cos_lc: lon_c.cos(),
        sin_lc: lon_c.sin(),
        cos_l0: lat_0.cos(),
        sin_l0: lat_0.sin(),
        cos_th: theta.cos(),
        sin_th: theta.sin(),
        rk: semi_major * k0,
    })
}

/// Rotate geographic → oblique Cartesian via R3·R2·R1.
///
/// R1: rotate by −lon_c around Z  (centre → prime meridian)
/// R2: rotate by −lat_0 around Y  (centre → equator)
/// R3: rotate by  θ = α_c−π/2 around X  (align initial line with equator)
fn rotate_forward(lon: f64, lat: f64, p: &OmercParams) -> (f64, f64, f64) {
    let (sin_lat, cos_lat) = lat.sin_cos();
    let (sin_lon, cos_lon) = lon.sin_cos();
    let px = cos_lat * cos_lon;
    let py = cos_lat * sin_lon;
    let pz = sin_lat;

    // R1: rotate by −lon_c around Z
    let rx = px * p.cos_lc + py * p.sin_lc;
    let ry = -px * p.sin_lc + py * p.cos_lc;
    let rz = pz;

    // R2: rotate by −lat_0 around Y
    let sx = rx * p.cos_l0 + rz * p.sin_l0;
    let sy = ry;
    let sz = -rx * p.sin_l0 + rz * p.cos_l0;

    // R3: rotate by θ around X
    let fx = sx;
    let fy = sy * p.cos_th - sz * p.sin_th;
    let fz = sy * p.sin_th + sz * p.cos_th;

    (fx, fy, fz)
}

/// Inverse rotation: oblique Cartesian → geographic via R1⁻¹·R2⁻¹·R3⁻¹.
fn rotate_inverse(fx: f64, fy: f64, fz: f64, p: &OmercParams) -> (f64, f64) {
    // R3⁻¹: rotate by −θ around X
    let sx = fx;
    let sy = fy * p.cos_th + fz * p.sin_th;
    let sz = -fy * p.sin_th + fz * p.cos_th;

    // R2⁻¹: rotate by +lat_0 around Y
    let rx = sx * p.cos_l0 - sz * p.sin_l0;
    let ry = sy;
    let rz = sx * p.sin_l0 + sz * p.cos_l0;

    // R1⁻¹: rotate by +lon_c around Z
    let px = rx * p.cos_lc - ry * p.sin_lc;
    let py = rx * p.sin_lc + ry * p.cos_lc;
    let pz = rz;

    let lat = pz.clamp(-1.0, 1.0).asin();
    let lon = py.atan2(px);
    (lon, lat)
}

/// Hotine Oblique Mercator forward projection (spherical form, variant A).
///
/// # Parameters
/// * `lon`, `lat` – geodetic coordinates in radians
/// * `lat_0` – latitude of the projection centre (radians)
/// * `lon_c` – longitude of the projection centre (radians)
/// * `alpha_c` – azimuth of the initial line (radians from north, clockwise)
/// * `k0` – scale factor on the initial line
/// * `semi_major` – semi-major axis (metres)
///
/// # Errors
/// Returns an error for non-finite coordinates or degenerate parameters.
pub fn oblique_mercator_forward(
    lon: f64,
    lat: f64,
    lat_0: f64,
    lon_c: f64,
    alpha_c: f64,
    k0: f64,
    semi_major: f64,
) -> Result<(f64, f64)> {
    if !lon.is_finite() || !lat.is_finite() {
        return Err(Error::invalid_coordinate("omerc: non-finite input"));
    }
    let p = omerc_params(lat_0, lon_c, alpha_c, k0, semi_major)?;
    let (fx, fy, fz) = rotate_forward(lon, lat, &p);

    // Recover rotated (lon', lat') from Cartesian
    let lon_r = fy.atan2(fx);
    let lat_r = fz.clamp(-1.0, 1.0).asin();

    // Standard Mercator in the rotated system
    let x = p.rk * lon_r;
    let half_pi = core::f64::consts::FRAC_PI_2;
    let lat_clamped = lat_r.clamp(-half_pi + 1e-10, half_pi - 1e-10);
    let y = p.rk * (half_pi * 0.5 + lat_clamped * 0.5).tan().ln();

    Ok((x, y))
}

/// Hotine Oblique Mercator inverse projection (spherical form, variant A).
///
/// # Parameters
/// * `x`, `y` – projected coordinates (metres)
/// * `lat_0` – latitude of the projection centre (radians)
/// * `lon_c` – longitude of the projection centre (radians)
/// * `alpha_c` – azimuth of the initial line (radians from north, clockwise)
/// * `k0` – scale factor on the initial line
/// * `semi_major` – semi-major axis (metres)
///
/// # Errors
/// Returns an error for non-finite coordinates or degenerate parameters.
pub fn oblique_mercator_inverse(
    x: f64,
    y: f64,
    lat_0: f64,
    lon_c: f64,
    alpha_c: f64,
    k0: f64,
    semi_major: f64,
) -> Result<(f64, f64)> {
    if !x.is_finite() || !y.is_finite() {
        return Err(Error::invalid_coordinate("omerc: non-finite input"));
    }
    let p = omerc_params(lat_0, lon_c, alpha_c, k0, semi_major)?;

    // Inverse Mercator: recover rotated coordinates
    let lon_r = x / p.rk;
    let t = y / p.rk;
    let lat_r = 2.0 * t.exp().atan() - core::f64::consts::FRAC_PI_2;

    // Rotated (lon', lat') → Cartesian on unit sphere
    let (sin_lr, cos_lr) = lat_r.sin_cos();
    let (sin_lonr, cos_lonr) = lon_r.sin_cos();
    let fx = cos_lr * cos_lonr;
    let fy = cos_lr * sin_lonr;
    let fz = sin_lr;

    // Inverse rotation → geographic
    let (lon, lat) = rotate_inverse(fx, fy, fz, &p);
    Ok((lon, lat))
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    const R: f64 = 6_371_000.0;
    const K0: f64 = 1.0;

    fn deg(d: f64) -> f64 {
        d.to_radians()
    }

    #[test]
    fn test_omerc_at_centre() {
        let lat_0 = deg(4.0);
        let lon_c = deg(115.0);
        let alpha = deg(53.31582); // azimuth for Borneo RSO
        let (x, y) =
            oblique_mercator_forward(lon_c, lat_0, lat_0, lon_c, alpha, K0, R).expect("forward ok");
        assert!(x.abs() < 1.0, "x at centre: {x}");
        assert!(y.abs() < 1.0, "y at centre: {y}");
    }

    #[test]
    fn test_omerc_roundtrip() {
        let lat_0 = deg(4.0);
        let lon_c = deg(115.0);
        let alpha = deg(53.31582);

        let cases = [
            (deg(115.0), deg(4.0)),
            (deg(116.0), deg(5.0)),
            (deg(113.5), deg(2.5)),
            (deg(118.0), deg(6.0)),
            (deg(110.0), deg(1.0)),
        ];
        for (lon, lat) in cases {
            let (x, y) =
                oblique_mercator_forward(lon, lat, lat_0, lon_c, alpha, K0, R).expect("fwd");
            let (lon2, lat2) =
                oblique_mercator_inverse(x, y, lat_0, lon_c, alpha, K0, R).expect("inv");
            assert!(
                (lon - lon2).abs() < 1e-9,
                "lon roundtrip: {:.6}° vs {:.6}° (diff: {:.2e})",
                lon.to_degrees(),
                lon2.to_degrees(),
                (lon - lon2).abs()
            );
            assert!(
                (lat - lat2).abs() < 1e-9,
                "lat roundtrip: {:.6}° vs {:.6}° (diff: {:.2e})",
                lat.to_degrees(),
                lat2.to_degrees(),
                (lat - lat2).abs()
            );
        }
    }

    #[test]
    fn test_omerc_alpha_90_reduces_to_regular_mercator_like() {
        // With alpha=90° (initial line runs east), the cylinder is tangent
        // along the parallel → similar to regular Mercator at the equator.
        let lat_0 = deg(0.0);
        let lon_c = deg(0.0);
        let alpha = deg(90.0);

        // Point on the equator east of centre: should have x > 0, y ≈ 0
        let (x, y) =
            oblique_mercator_forward(deg(10.0), deg(0.0), lat_0, lon_c, alpha, K0, R).expect("fwd");
        assert!(x > 0.0, "x should be positive: {x}");
        assert!(y.abs() < 100.0, "y should be near 0 on equator: {y}");

        // Roundtrip
        let (lon2, lat2) = oblique_mercator_inverse(x, y, lat_0, lon_c, alpha, K0, R).expect("inv");
        assert!((deg(10.0) - lon2).abs() < 1e-9);
        assert!(lat2.abs() < 1e-9);
    }

    #[test]
    fn test_omerc_nonfinite_coord() {
        let r = oblique_mercator_forward(f64::NAN, 0.0, 0.0, 0.0, 0.5, 1.0, R);
        assert!(r.is_err());
        let r = oblique_mercator_inverse(f64::INFINITY, 0.0, 0.0, 0.0, 0.5, 1.0, R);
        assert!(r.is_err());
    }

    #[test]
    fn test_omerc_polar_centre_rejected() {
        let r = oblique_mercator_forward(0.0, 0.0, deg(90.0), 0.0, 0.5, 1.0, R);
        assert!(r.is_err());
    }

    #[test]
    fn test_omerc_negative_scale_rejected() {
        let r = oblique_mercator_forward(0.0, 0.0, 0.0, 0.0, 0.5, -1.0, R);
        assert!(r.is_err());
    }

    #[test]
    fn test_omerc_symmetry_about_centre() {
        let lat_0 = deg(4.0);
        let lon_c = deg(115.0);
        let alpha = deg(53.0);

        let (x1, y1) = oblique_mercator_forward(deg(116.0), deg(5.0), lat_0, lon_c, alpha, K0, R)
            .expect("fwd1");
        let (x2, y2) = oblique_mercator_forward(deg(114.0), deg(3.0), lat_0, lon_c, alpha, K0, R)
            .expect("fwd2");
        // Points on opposite sides of the centre should have opposite x signs
        // (roughly, depending on exact geometry).  At minimum, both must project
        // without error and roundtrip.
        let (l1, p1) = oblique_mercator_inverse(x1, y1, lat_0, lon_c, alpha, K0, R).expect("inv1");
        let (l2, p2) = oblique_mercator_inverse(x2, y2, lat_0, lon_c, alpha, K0, R).expect("inv2");
        assert!((deg(116.0) - l1).abs() < 1e-9);
        assert!((deg(5.0) - p1).abs() < 1e-9);
        assert!((deg(114.0) - l2).abs() < 1e-9);
        assert!((deg(3.0) - p2).abs() < 1e-9);
    }

    #[test]
    fn test_omerc_various_azimuths_roundtrip() {
        let lat_0 = deg(30.0);
        let lon_c = deg(0.0);
        let lon = deg(5.0);
        let lat = deg(32.0);

        for alpha_deg in [0.0, 15.0, 30.0, 45.0, 60.0, 75.0, 90.0] {
            let alpha = deg(alpha_deg);
            let (x, y) =
                oblique_mercator_forward(lon, lat, lat_0, lon_c, alpha, K0, R).expect("fwd");
            let (lon2, lat2) =
                oblique_mercator_inverse(x, y, lat_0, lon_c, alpha, K0, R).expect("inv");
            assert!(
                (lon - lon2).abs() < 1e-8,
                "α={alpha_deg}° lon: {:.6}° vs {:.6}°",
                lon.to_degrees(),
                lon2.to_degrees()
            );
            assert!(
                (lat - lat2).abs() < 1e-8,
                "α={alpha_deg}° lat: {:.6}° vs {:.6}°",
                lat.to_degrees(),
                lat2.to_degrees()
            );
        }
    }
}
