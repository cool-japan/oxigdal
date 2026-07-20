//! Conic map projections.
//!
//! Includes:
//! - **Equidistant Conic** (`+proj=eqdc`): Distances along meridians are true.
//!
//! All angles are in **radians**.

use crate::error::{Error, Result};

// ─────────────────────────────────────────────────────────────────────────────
// Equidistant Conic
// ─────────────────────────────────────────────────────────────────────────────

/// Equidistant Conic forward projection (spherical form).
///
/// Both standard parallels have true scale; distances along meridians are correct.
///
/// Reference: Snyder (1987) p. 111.
///
/// # Parameters
/// * `lon`, `lat` – geodetic coordinates in radians
/// * `lon_0` – central meridian in radians
/// * `lat_0` – origin latitude in radians
/// * `lat_1`, `lat_2` – standard parallels in radians (lat_1 ≠ lat_2)
/// * `semi_major` – semi-major axis (metres)
///
/// # Errors
/// Returns an error for degenerate input (both standard parallels equal) or
/// non-finite coordinates.
pub fn equidistant_conic_forward(
    lon: f64,
    lat: f64,
    lon_0: f64,
    lat_0: f64,
    lat_1: f64,
    lat_2: f64,
    semi_major: f64,
) -> Result<(f64, f64)> {
    if !lon.is_finite() || !lat.is_finite() {
        return Err(Error::invalid_coordinate("eqdc: non-finite input"));
    }
    let (n, g, rho_0) = eqdc_constants(lat_0, lat_1, lat_2, semi_major)?;
    let rho = semi_major * (g - lat);
    let theta = n * (lon - lon_0);
    let x = rho * theta.sin();
    let y = rho_0 - rho * theta.cos();
    Ok((x, y))
}

/// Equidistant Conic inverse projection (spherical form).
///
/// # Errors
/// Returns an error for degenerate input or non-finite coordinates.
pub fn equidistant_conic_inverse(
    x: f64,
    y: f64,
    lon_0: f64,
    lat_0: f64,
    lat_1: f64,
    lat_2: f64,
    semi_major: f64,
) -> Result<(f64, f64)> {
    if !x.is_finite() || !y.is_finite() {
        return Err(Error::invalid_coordinate("eqdc: non-finite input"));
    }
    let (n, g, rho_0) = eqdc_constants(lat_0, lat_1, lat_2, semi_major)?;

    let rho_0_minus_y = rho_0 - y;
    let rho = (x * x + rho_0_minus_y * rho_0_minus_y).sqrt() * if n < 0.0 { -1.0 } else { 1.0 };

    let lat = g - rho / semi_major;
    let theta = if rho.abs() < 1e-15 {
        0.0
    } else {
        (x / rho).atan2(rho_0_minus_y / rho)
    };
    let lon = theta / n + lon_0;
    Ok((lon, lat))
}

/// Computes cone constant `n`, parameter `G`, and `ρ₀` for Equidistant Conic.
fn eqdc_constants(lat_0: f64, lat_1: f64, lat_2: f64, semi_major: f64) -> Result<(f64, f64, f64)> {
    // n = (cos φ₁ − cos φ₂) / (φ₂ − φ₁)
    let n = if (lat_2 - lat_1).abs() < 1e-12 {
        // Both standard parallels equal — degenerate to a single standard parallel
        lat_1.sin()
    } else {
        (lat_1.cos() - lat_2.cos()) / (lat_2 - lat_1)
    };
    if n.abs() < 1e-15 {
        return Err(Error::invalid_parameter(
            "eqdc",
            "cone constant n is zero — check standard parallels",
        ));
    }
    // G = cos(φ₁)/n + φ₁
    let g = lat_1.cos() / n + lat_1;
    // ρ₀ = R (G − φ₀)
    let rho_0 = semi_major * (g - lat_0);
    Ok((n, g, rho_0))
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    const R: f64 = 6_371_000.0;

    #[test]
    fn test_eqdc_roundtrip() {
        let lat_1 = 29.5_f64.to_radians();
        let lat_2 = 45.5_f64.to_radians();
        let lat_0 = 37.0_f64.to_radians();
        let lon_0 = (-96.0_f64).to_radians();

        let cases = [
            ((-96.0_f64).to_radians(), 37.0_f64.to_radians()),
            ((-90.0_f64).to_radians(), 40.0_f64.to_radians()),
            ((-80.0_f64).to_radians(), 35.0_f64.to_radians()),
        ];
        for (lon, lat) in cases {
            let (x, y) = equidistant_conic_forward(lon, lat, lon_0, lat_0, lat_1, lat_2, R)
                .expect("forward ok");
            let (lon2, lat2) =
                equidistant_conic_inverse(x, y, lon_0, lat_0, lat_1, lat_2, R).expect("inverse ok");
            assert!(
                (lon - lon2).abs() < 1e-9,
                "lon roundtrip: {lon:.5} vs {lon2:.5}"
            );
            assert!(
                (lat - lat2).abs() < 1e-9,
                "lat roundtrip: {lat:.5} vs {lat2:.5}"
            );
        }
    }

    #[test]
    fn test_eqdc_at_origin() {
        let lat_1 = 29.5_f64.to_radians();
        let lat_2 = 45.5_f64.to_radians();
        let lat_0 = 37.0_f64.to_radians();
        let lon_0 = (-96.0_f64).to_radians();

        // At the origin the x should be ~0, y should be ~0
        let (x, y) =
            equidistant_conic_forward(lon_0, lat_0, lon_0, lat_0, lat_1, lat_2, R).expect("ok");
        assert!(x.abs() < 1.0, "x at origin: {x}");
        assert!(y.abs() < 1.0, "y at origin: {y}");
    }
}
