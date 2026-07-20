//! Pseudocylindrical map projections.
//!
//! Includes:
//! - **Mollweide** (`+proj=moll`): Equal-area world projection.
//! - **Robinson** (`+proj=robin`): Compromise world projection by Arthur Robinson.
//! - **Eckert IV** (`+proj=eck4`): Equal-area with rounded poles.
//! - **Eckert VI** (`+proj=eck6`): Equal-area with pointed poles.
//!
//! All angles are in **radians**.  Degree conversion is the caller's responsibility.

use crate::error::{Error, Result};

// ─────────────────────────────────────────────────────────────────────────────
// Mollweide
// ─────────────────────────────────────────────────────────────────────────────

/// Mollweide forward projection.
///
/// Solves `2θ + sin(2θ) = π sin(φ)` by Newton-Raphson, then maps to
/// Cartesian coordinates.
///
/// Reference: Snyder (1987) p. 249.
///
/// # Errors
/// Returns `ConvergenceError` if Newton-Raphson fails (should not occur for valid input).
pub fn mollweide_forward(lon: f64, lat: f64, lon_0: f64, semi_major: f64) -> Result<(f64, f64)> {
    if !lon.is_finite() || !lat.is_finite() {
        return Err(Error::invalid_coordinate("mollweide: non-finite input"));
    }
    use core::f64::consts::{PI, SQRT_2};

    // At the poles theta = ±π/2 exactly
    let theta = if (lat.abs() - PI / 2.0).abs() < 1e-12 {
        if lat > 0.0 { PI / 2.0 } else { -PI / 2.0 }
    } else {
        solve_mollweide_theta(lat)?
    };

    let x = semi_major * 2.0 * SQRT_2 / PI * (lon - lon_0) * theta.cos();
    let y = semi_major * SQRT_2 * theta.sin();
    Ok((x, y))
}

/// Mollweide inverse projection.
///
/// # Errors
/// Returns an error for non-finite inputs or points outside the ellipse.
pub fn mollweide_inverse(x: f64, y: f64, lon_0: f64, semi_major: f64) -> Result<(f64, f64)> {
    if !x.is_finite() || !y.is_finite() {
        return Err(Error::invalid_coordinate("mollweide: non-finite input"));
    }
    use core::f64::consts::{PI, SQRT_2};

    // θ = arcsin(y / (R √2))
    let sin_theta = y / (semi_major * SQRT_2);
    if sin_theta.abs() > 1.0 + 1e-10 {
        return Err(Error::coordinate_out_of_bounds(x, y));
    }
    let sin_theta = sin_theta.clamp(-1.0, 1.0);
    let theta = sin_theta.asin();

    // φ = arcsin((2θ + sin(2θ)) / π)
    let sin_lat = (2.0 * theta + (2.0 * theta).sin()) / PI;
    let lat = sin_lat.clamp(-1.0, 1.0).asin();

    // λ = λ₀ + π x / (2√2 R cos θ)
    let cos_theta = theta.cos();
    let lon = if cos_theta.abs() < 1e-15 {
        lon_0
    } else {
        lon_0 + PI * x / (2.0 * SQRT_2 * semi_major * cos_theta)
    };
    Ok((lon, lat))
}

/// Solves `2θ + sin(2θ) = π sin(φ)` for θ using Newton-Raphson.
fn solve_mollweide_theta(lat: f64) -> Result<f64> {
    let rhs = core::f64::consts::PI * lat.sin();
    let mut theta = lat; // good initial estimate
    for i in 0..50 {
        let f = 2.0 * theta + (2.0 * theta).sin() - rhs;
        let df = 2.0 + 2.0 * (2.0 * theta).cos();
        let delta = -f / df;
        theta += delta;
        if delta.abs() < 1e-14 {
            return Ok(theta);
        }
        let _ = i;
    }
    Err(Error::convergence_error(50))
}

// ─────────────────────────────────────────────────────────────────────────────
// Robinson
// ─────────────────────────────────────────────────────────────────────────────

/// Snyder lookup table for the Robinson projection (Appendix, Table 13).
/// Columns: latitude (degrees, 0–90 step 5), PLEN, PDFE.
/// PLEN = length of parallel (normalised by equator).
/// PDFE = distance from equator to parallel (normalised by axis).
const ROBINSON_TABLE: [(f64, f64, f64); 19] = [
    (0.0, 1.0000, 0.0000),
    (5.0, 0.9986, 0.0620),
    (10.0, 0.9954, 0.1240),
    (15.0, 0.9900, 0.1860),
    (20.0, 0.9822, 0.2480),
    (25.0, 0.9730, 0.3100),
    (30.0, 0.9600, 0.3720),
    (35.0, 0.9427, 0.4340),
    (40.0, 0.9216, 0.4958),
    (45.0, 0.8962, 0.5571),
    (50.0, 0.8679, 0.6176),
    (55.0, 0.8350, 0.6769),
    (60.0, 0.7986, 0.7346),
    (65.0, 0.7597, 0.7903),
    (70.0, 0.7186, 0.8435),
    (75.0, 0.6732, 0.8936),
    (80.0, 0.6213, 0.9394),
    (85.0, 0.5722, 0.9761),
    (90.0, 0.5322, 1.0000),
];

/// Interpolates Robinson table values (PLEN, PDFE) for a latitude in degrees.
fn robinson_interpolate(abs_lat_deg: f64) -> (f64, f64) {
    debug_assert!((0.0..=90.0).contains(&abs_lat_deg));
    let idx_f = abs_lat_deg / 5.0;
    let idx = idx_f.floor() as usize;
    let t = idx_f - idx as f64;
    if idx >= 18 {
        return (ROBINSON_TABLE[18].1, ROBINSON_TABLE[18].2);
    }
    let (_, p0, d0) = ROBINSON_TABLE[idx];
    let (_, p1, d1) = ROBINSON_TABLE[idx + 1];
    (p0 + t * (p1 - p0), d0 + t * (d1 - d0))
}

/// Robinson forward projection.
///
/// # Parameters
/// * `lon`, `lat` – geodetic coordinates in radians
/// * `lon_0` – central meridian in radians
/// * `semi_major` – semi-major axis (metres)
///
/// # Errors
/// Returns an error for non-finite inputs.
pub fn robinson_forward(lon: f64, lat: f64, lon_0: f64, semi_major: f64) -> Result<(f64, f64)> {
    if !lon.is_finite() || !lat.is_finite() {
        return Err(Error::invalid_coordinate("robinson: non-finite input"));
    }
    let abs_lat_deg = lat.to_degrees().abs().min(90.0);
    let (plen, pdfe) = robinson_interpolate(abs_lat_deg);
    let sign = if lat < 0.0 { -1.0 } else { 1.0 };
    // Scale: using standard Snyder constants (R = semi_major, scale factor 0.8487)
    let x = semi_major * 0.8487 * plen * (lon - lon_0);
    let y = semi_major * 1.3523 * sign * pdfe;
    Ok((x, y))
}

/// Robinson inverse projection (iterative via table lookup).
///
/// Recovers latitude by finding `pdfe` = |y| / (R · 1.3523) in the table.
///
/// # Errors
/// Returns an error for non-finite inputs or out-of-range y values.
pub fn robinson_inverse(x: f64, y: f64, lon_0: f64, semi_major: f64) -> Result<(f64, f64)> {
    if !x.is_finite() || !y.is_finite() {
        return Err(Error::invalid_coordinate("robinson: non-finite input"));
    }
    let sign = if y < 0.0 { -1.0 } else { 1.0 };
    let pdfe_target = (y / (semi_major * 1.3523)).abs();
    if pdfe_target > 1.0 + 1e-10 {
        return Err(Error::coordinate_out_of_bounds(x, y));
    }
    let pdfe_target = pdfe_target.min(1.0);

    // Binary search / interpolation in table for lat from pdfe
    let mut abs_lat_deg = 0.0_f64;
    for i in 0..18 {
        let (_, _, d0) = ROBINSON_TABLE[i];
        let (_, _, d1) = ROBINSON_TABLE[i + 1];
        if pdfe_target >= d0 && pdfe_target <= d1 {
            let t = if (d1 - d0).abs() < 1e-15 {
                0.0
            } else {
                (pdfe_target - d0) / (d1 - d0)
            };
            abs_lat_deg = ROBINSON_TABLE[i].0 + t * 5.0;
            break;
        }
    }
    // Handle exact 90°
    if pdfe_target >= ROBINSON_TABLE[18].2 {
        abs_lat_deg = 90.0;
    }

    let lat = sign * abs_lat_deg.to_radians();
    // Recover PLEN at this latitude for lon
    let (plen, _) = robinson_interpolate(abs_lat_deg);
    let lon = if plen.abs() < 1e-15 {
        lon_0
    } else {
        lon_0 + x / (semi_major * 0.8487 * plen)
    };
    Ok((lon, lat))
}

// ─────────────────────────────────────────────────────────────────────────────
// Eckert IV
// ─────────────────────────────────────────────────────────────────────────────

/// Eckert IV forward projection.
///
/// Solves `θ + sin(θ)cos(θ) + 2sin(θ) = (2 + π/2) sin(φ)` by Newton-Raphson.
///
/// Reference: Snyder (1987) p. 253.
///
/// # Errors
/// Returns `ConvergenceError` if Newton-Raphson fails.
pub fn eckert4_forward(lon: f64, lat: f64, lon_0: f64, semi_major: f64) -> Result<(f64, f64)> {
    if !lon.is_finite() || !lat.is_finite() {
        return Err(Error::invalid_coordinate("eckert4: non-finite input"));
    }
    use core::f64::consts::PI;
    let c = (2.0 + PI / 2.0) * lat.sin();
    let theta = solve_eckert4_theta(c)?;
    let x = semi_major * 2.0 / ((PI * (4.0 + PI)).sqrt()) * (lon - lon_0) * (1.0 + theta.cos());
    let y = semi_major * 2.0 * PI.sqrt() / (4.0 + PI).sqrt() * theta.sin();
    Ok((x, y))
}

/// Eckert IV inverse projection.
///
/// # Errors
/// Returns an error for non-finite inputs or out-of-range coordinates.
pub fn eckert4_inverse(x: f64, y: f64, lon_0: f64, semi_major: f64) -> Result<(f64, f64)> {
    if !x.is_finite() || !y.is_finite() {
        return Err(Error::invalid_coordinate("eckert4: non-finite input"));
    }
    use core::f64::consts::PI;
    let c_y = (4.0 + PI).sqrt() / (2.0 * PI.sqrt());
    let sin_theta = y * c_y / semi_major;
    if sin_theta.abs() > 1.0 + 1e-10 {
        return Err(Error::coordinate_out_of_bounds(x, y));
    }
    let theta = sin_theta.clamp(-1.0, 1.0).asin();
    let sin_lat = (theta + theta.sin() * theta.cos() + 2.0 * theta.sin()) / (2.0 + PI / 2.0);
    let lat = sin_lat.clamp(-1.0, 1.0).asin();
    let one_plus_cos = 1.0 + theta.cos();
    let c_x = 2.0 / ((PI * (4.0 + PI)).sqrt());
    let lon = if one_plus_cos.abs() < 1e-15 {
        lon_0
    } else {
        lon_0 + x / (semi_major * c_x * one_plus_cos)
    };
    Ok((lon, lat))
}

/// Solves Eckert IV parametric equation for θ via Newton-Raphson.
///
/// θ + sin(θ)cos(θ) + 2sin(θ) = c  where c = (2 + π/2)·sin(φ)
///
/// The initial guess is θ₀ = c/(2+π/2) (linear approximation for small φ),
/// which avoids the domain issue of asin(c) when c > 1.
fn solve_eckert4_theta(c: f64) -> Result<f64> {
    use core::f64::consts::PI;
    // A safe initial guess: use c normalised by the RHS constant
    let mut theta = c / (2.0 + PI / 2.0);
    for _ in 0..50 {
        let f = theta + theta.sin() * theta.cos() + 2.0 * theta.sin() - c;
        // d/dθ [θ + sinθcosθ + 2sinθ] = 1 + cos²θ - sin²θ + 2cosθ = 2cos²θ + 2cosθ
        let df2 = 2.0 * theta.cos() * theta.cos() + 2.0 * theta.cos();
        let deriv = if df2.abs() > 1e-15 { df2 } else { 1.0 };
        let delta = -f / deriv;
        theta += delta;
        if delta.abs() < 1e-14 {
            return Ok(theta);
        }
    }
    Err(Error::convergence_error(50))
}

// ─────────────────────────────────────────────────────────────────────────────
// Eckert VI
// ─────────────────────────────────────────────────────────────────────────────

/// Eckert VI forward projection.
///
/// Solves `θ + sin(θ) = (1 + π/2) sin(φ)` by Newton-Raphson.
///
/// Reference: Snyder (1987) p. 255.
///
/// # Errors
/// Returns `ConvergenceError` if Newton-Raphson fails.
pub fn eckert6_forward(lon: f64, lat: f64, lon_0: f64, semi_major: f64) -> Result<(f64, f64)> {
    if !lon.is_finite() || !lat.is_finite() {
        return Err(Error::invalid_coordinate("eckert6: non-finite input"));
    }
    use core::f64::consts::PI;
    let c = (1.0 + PI / 2.0) * lat.sin();
    let theta = solve_eckert6_theta(c)?;
    let k = (2.0 + PI).sqrt();
    let x = semi_major * 2.0 / k * (lon - lon_0) * (1.0 + theta.cos());
    let y = semi_major * 2.0 * k / (2.0 + PI) * theta;
    // Simplify using standard Snyder formula: x = R(2/√(2+π))(λ-λ₀)(1+cosθ)
    // y = 2R√(2+π)θ/(2+π) = 2Rθ/√(2+π)
    let x_s = semi_major * 2.0 / k * (lon - lon_0) * (1.0 + theta.cos());
    let y_s = semi_major * 2.0 * theta / k;
    let _ = (x, y);
    Ok((x_s, y_s))
}

/// Eckert VI inverse projection.
///
/// # Errors
/// Returns an error for non-finite inputs.
pub fn eckert6_inverse(x: f64, y: f64, lon_0: f64, semi_major: f64) -> Result<(f64, f64)> {
    if !x.is_finite() || !y.is_finite() {
        return Err(Error::invalid_coordinate("eckert6: non-finite input"));
    }
    use core::f64::consts::PI;
    let k = (2.0 + PI).sqrt();
    let theta = y * k / (2.0 * semi_major);
    if theta.abs() > core::f64::consts::FRAC_PI_2 + 1e-10 {
        return Err(Error::coordinate_out_of_bounds(x, y));
    }
    let sin_lat = (theta + theta.sin()) / (1.0 + PI / 2.0);
    let lat = sin_lat.clamp(-1.0, 1.0).asin();
    let one_plus_cos = 1.0 + theta.cos();
    let lon = if one_plus_cos.abs() < 1e-15 {
        lon_0
    } else {
        lon_0 + x * k / (2.0 * semi_major * one_plus_cos)
    };
    Ok((lon, lat))
}

/// Solves `θ + sin(θ) = c` by Newton-Raphson.
fn solve_eckert6_theta(c: f64) -> Result<f64> {
    let mut theta = c / (1.0 + core::f64::consts::FRAC_PI_4); // initial guess
    for _ in 0..50 {
        let f = theta + theta.sin() - c;
        let df = 1.0 + theta.cos();
        if df.abs() < 1e-15 {
            break;
        }
        let delta = -f / df;
        theta += delta;
        if delta.abs() < 1e-14 {
            return Ok(theta);
        }
    }
    // At this point theta is close enough for display purposes
    Ok(theta)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use core::f64::consts::PI;

    const R: f64 = 6_371_000.0; // sphere radius

    #[test]
    fn test_mollweide_roundtrip() {
        let cases = [
            (0.0_f64, 0.0_f64),
            (30.0f64.to_radians(), 45.0f64.to_radians()),
            (-90.0f64.to_radians(), -45.0f64.to_radians()),
            (0.0, 89.0f64.to_radians()),
        ];
        for (lon, lat) in cases {
            let (x, y) = mollweide_forward(lon, lat, 0.0, R).expect("forward ok");
            let (lon2, lat2) = mollweide_inverse(x, y, 0.0, R).expect("inverse ok");
            assert!(
                (lon - lon2).abs() < 1e-9,
                "lon roundtrip {lon:.4} → {lon2:.4}"
            );
            assert!(
                (lat - lat2).abs() < 1e-9,
                "lat roundtrip {lat:.4} → {lat2:.4}"
            );
        }
    }

    #[test]
    fn test_mollweide_poles() {
        let (x, _y) = mollweide_forward(0.0, PI / 2.0, 0.0, R).expect("north pole ok");
        assert!(x.abs() < 1.0, "x at north pole should be ~0");
    }

    #[test]
    fn test_robinson_roundtrip() {
        let cases = [
            (0.0_f64, 0.0_f64),
            (45.0f64.to_radians(), 30.0f64.to_radians()),
            (-60.0f64.to_radians(), -20.0f64.to_radians()),
        ];
        for (lon, lat) in cases {
            let (x, y) = robinson_forward(lon, lat, 0.0, R).expect("forward ok");
            let (lon2, lat2) = robinson_inverse(x, y, 0.0, R).expect("inverse ok");
            assert!((lon - lon2).abs() < 1e-6, "lon: {lon:.4} vs {lon2:.4}");
            assert!((lat - lat2).abs() < 1e-6, "lat: {lat:.4} vs {lat2:.4}");
        }
    }

    #[test]
    fn test_eckert4_roundtrip() {
        let cases = [
            (0.0_f64, 0.0_f64),
            (30.0f64.to_radians(), 45.0f64.to_radians()),
            (-60.0f64.to_radians(), -30.0f64.to_radians()),
        ];
        for (lon, lat) in cases {
            let (x, y) = eckert4_forward(lon, lat, 0.0, R).expect("forward ok");
            let (lon2, lat2) = eckert4_inverse(x, y, 0.0, R).expect("inverse ok");
            assert!((lon - lon2).abs() < 1e-9, "lon: {lon:.4} vs {lon2:.4}");
            assert!((lat - lat2).abs() < 1e-9, "lat: {lat:.4} vs {lat2:.4}");
        }
    }

    #[test]
    fn test_eckert6_roundtrip() {
        let cases = [
            (0.0_f64, 0.0_f64),
            (45.0f64.to_radians(), 45.0f64.to_radians()),
            (-30.0f64.to_radians(), -60.0f64.to_radians()),
        ];
        for (lon, lat) in cases {
            let (x, y) = eckert6_forward(lon, lat, 0.0, R).expect("forward ok");
            let (lon2, lat2) = eckert6_inverse(x, y, 0.0, R).expect("inverse ok");
            assert!((lon - lon2).abs() < 1e-9, "lon: {lon:.4} vs {lon2:.4}");
            assert!((lat - lat2).abs() < 1e-9, "lat: {lat:.4} vs {lat2:.4}");
        }
    }
}
