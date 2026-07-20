//! Albers Equal-Area Conic projection.
//!
//! A conic, equal-area projection widely used for thematic and atlas maps of
//! countries with large east-west extent. The standard form used by USGS for
//! continental US maps uses standard parallels 29°30'N and 45°30'N.
//!
//! PROJ identifier: `+proj=aea`
//!
//! Reference: Snyder (1987) p. 98–103 (spherical form).
//!
//! All angles are in **radians**.

use crate::error::{Error, Result};

/// Albers Equal-Area Conic forward projection (spherical form).
///
/// # Parameters
/// * `lon`, `lat` – geodetic coordinates in radians
/// * `lon_0` – central meridian in radians
/// * `lat_0` – origin latitude in radians
/// * `lat_1`, `lat_2` – standard parallels in radians
/// * `semi_major` – semi-major axis (metres)
///
/// # Errors
/// Returns an error for non-finite coordinates or degenerate cone constant.
pub fn albers_forward(
    lon: f64,
    lat: f64,
    lon_0: f64,
    lat_0: f64,
    lat_1: f64,
    lat_2: f64,
    semi_major: f64,
) -> Result<(f64, f64)> {
    if !lon.is_finite() || !lat.is_finite() {
        return Err(Error::invalid_coordinate("albers: non-finite input"));
    }
    let (n, c, rho_0) = albers_constants(lat_0, lat_1, lat_2, semi_major)?;
    let rho = albers_rho(lat, n, c, semi_major);
    let theta = n * (lon - lon_0);

    let x = rho * theta.sin();
    let y = rho_0 - rho * theta.cos();
    Ok((x, y))
}

/// Albers Equal-Area Conic inverse projection (spherical form).
///
/// # Errors
/// Returns an error for non-finite coordinates or degenerate parameters.
pub fn albers_inverse(
    x: f64,
    y: f64,
    lon_0: f64,
    lat_0: f64,
    lat_1: f64,
    lat_2: f64,
    semi_major: f64,
) -> Result<(f64, f64)> {
    if !x.is_finite() || !y.is_finite() {
        return Err(Error::invalid_coordinate("albers: non-finite input"));
    }
    let (n, c, rho_0) = albers_constants(lat_0, lat_1, lat_2, semi_major)?;

    let rho_0_minus_y = rho_0 - y;
    let rho = (x * x + rho_0_minus_y * rho_0_minus_y).sqrt() * if n < 0.0 { -1.0 } else { 1.0 };

    let sign_n = if n >= 0.0 { 1.0 } else { -1.0 };
    let theta = if rho.abs() < 1e-15 {
        0.0
    } else {
        (x * sign_n).atan2(rho_0_minus_y * sign_n)
    };

    let lon = theta / n + lon_0;

    // lat = arcsin((C − ρ²n²/R²) / (2n))
    let r2 = semi_major * semi_major;
    let sin_lat = (c - (rho * rho * n * n) / r2) / (2.0 * n);

    if sin_lat.abs() > 1.0 + 1e-10 {
        return Err(Error::invalid_coordinate(
            "albers inverse: sin(lat) out of range",
        ));
    }
    let lat = sin_lat.clamp(-1.0, 1.0).asin();
    Ok((lon, lat))
}

/// Compute Albers constants: cone constant `n`, `C`, and `ρ₀`.
///
/// n = (sin φ₁ + sin φ₂) / 2
/// C = cos²φ₁ + 2n sin φ₁
/// ρ₀ = R √(C − 2n sin φ₀) / n
fn albers_constants(
    lat_0: f64,
    lat_1: f64,
    lat_2: f64,
    semi_major: f64,
) -> Result<(f64, f64, f64)> {
    let n = (lat_1.sin() + lat_2.sin()) / 2.0;
    if n.abs() < 1e-15 {
        return Err(Error::invalid_parameter(
            "albers",
            "cone constant n is near zero — check standard parallels",
        ));
    }

    let c = lat_1.cos() * lat_1.cos() + 2.0 * n * lat_1.sin();

    let val = c - 2.0 * n * lat_0.sin();
    if val < 0.0 {
        return Err(Error::invalid_parameter(
            "albers",
            "negative value under sqrt for rho_0",
        ));
    }
    let rho_0 = semi_major * val.sqrt() / n;
    Ok((n, c, rho_0))
}

/// Compute ρ for a given latitude.
fn albers_rho(lat: f64, n: f64, c: f64, semi_major: f64) -> f64 {
    let val = c - 2.0 * n * lat.sin();
    if val < 0.0 {
        // Clamp to zero — can happen near poles with extreme parallels
        return 0.0;
    }
    semi_major * val.sqrt() / n
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    const R: f64 = 6_371_000.0;

    /// Standard US Albers parameters (29.5°N, 45.5°N, origin 37°N / −96°W).
    fn us_params() -> (f64, f64, f64, f64) {
        (
            (-96.0_f64).to_radians(), // lon_0
            37.0_f64.to_radians(),    // lat_0
            29.5_f64.to_radians(),    // lat_1
            45.5_f64.to_radians(),    // lat_2
        )
    }

    #[test]
    fn test_albers_at_origin() {
        let (lon_0, lat_0, lat_1, lat_2) = us_params();
        let (x, y) = albers_forward(lon_0, lat_0, lon_0, lat_0, lat_1, lat_2, R).expect("ok");
        assert!(x.abs() < 1.0, "x at origin: {x}");
        assert!(y.abs() < 1.0, "y at origin: {y}");
    }

    #[test]
    fn test_albers_roundtrip() {
        let (lon_0, lat_0, lat_1, lat_2) = us_params();
        let cases = [
            ((-96.0_f64).to_radians(), 37.0_f64.to_radians()),
            ((-90.0_f64).to_radians(), 40.0_f64.to_radians()),
            ((-80.0_f64).to_radians(), 35.0_f64.to_radians()),
            ((-110.0_f64).to_radians(), 30.0_f64.to_radians()),
            ((-75.0_f64).to_radians(), 45.0_f64.to_radians()),
        ];
        for (lon, lat) in cases {
            let (x, y) = albers_forward(lon, lat, lon_0, lat_0, lat_1, lat_2, R).expect("fwd");
            let (lon2, lat2) = albers_inverse(x, y, lon_0, lat_0, lat_1, lat_2, R).expect("inv");
            assert!(
                (lon - lon2).abs() < 1e-9,
                "lon roundtrip: {:.6} vs {:.6}",
                lon.to_degrees(),
                lon2.to_degrees()
            );
            assert!(
                (lat - lat2).abs() < 1e-9,
                "lat roundtrip: {:.6} vs {:.6}",
                lat.to_degrees(),
                lat2.to_degrees()
            );
        }
    }

    #[test]
    fn test_albers_equal_area_property() {
        // Two small patches at different latitudes should map to equal areas
        // if they span the same geographic area (by definition of equal-area).
        let (lon_0, lat_0, lat_1, lat_2) = us_params();

        let delta = 0.001_f64; // ~0.057°
        let lon_c = (-96.0_f64).to_radians();

        // Patch at 35°N
        let lat_a = 35.0_f64.to_radians();
        let (x1, y1) = albers_forward(lon_c, lat_a, lon_0, lat_0, lat_1, lat_2, R).expect("ok");
        let (x2, y2) = albers_forward(lon_c + delta, lat_a + delta, lon_0, lat_0, lat_1, lat_2, R)
            .expect("ok");
        let area_a = (x2 - x1).abs() * (y2 - y1).abs();

        // Patch at 42°N
        let lat_b = 42.0_f64.to_radians();
        let (x3, y3) = albers_forward(lon_c, lat_b, lon_0, lat_0, lat_1, lat_2, R).expect("ok");
        let (x4, y4) = albers_forward(lon_c + delta, lat_b + delta, lon_0, lat_0, lat_1, lat_2, R)
            .expect("ok");
        let area_b = (x4 - x3).abs() * (y4 - y3).abs();

        // Areas should be similar (not identical due to curvature, but within ~5%)
        let ratio = area_a / area_b;
        assert!(
            (0.8..1.2).contains(&ratio),
            "area ratio should be near 1.0: {ratio}"
        );
    }

    #[test]
    fn test_albers_nonfinite() {
        let (lon_0, lat_0, lat_1, lat_2) = us_params();
        assert!(albers_forward(f64::NAN, 0.0, lon_0, lat_0, lat_1, lat_2, R).is_err());
        assert!(albers_forward(f64::INFINITY, 0.0, lon_0, lat_0, lat_1, lat_2, R).is_err());
        assert!(albers_inverse(f64::NAN, 0.0, lon_0, lat_0, lat_1, lat_2, R).is_err());
    }

    #[test]
    fn test_albers_southern_hemisphere() {
        // Australian parameters: 18°S, 36°S, origin 27°S / 132°E
        let lon_0 = 132.0_f64.to_radians();
        let lat_0 = (-27.0_f64).to_radians();
        let lat_1 = (-18.0_f64).to_radians();
        let lat_2 = (-36.0_f64).to_radians();

        let lon = 151.0_f64.to_radians(); // Sydney
        let lat = (-33.9_f64).to_radians();

        let (x, y) = albers_forward(lon, lat, lon_0, lat_0, lat_1, lat_2, R).expect("fwd");
        let (lon2, lat2) = albers_inverse(x, y, lon_0, lat_0, lat_1, lat_2, R).expect("inv");
        assert!((lon - lon2).abs() < 1e-9);
        assert!((lat - lat2).abs() < 1e-9);
    }

    #[test]
    fn test_albers_constants_degenerate() {
        // lat_1 = -lat_2 ⇒ n = 0
        let lat_1 = 30.0_f64.to_radians();
        let lat_2 = (-30.0_f64).to_radians();
        assert!(albers_constants(0.0, lat_1, lat_2, R).is_err());
    }
}
