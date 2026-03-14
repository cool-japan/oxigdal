//! Conic map projections.
//!
//! This module implements conic projections:
//!
//! - **Equidistant Conic** (`+proj=eqdc`): Distances along meridians are preserved;
//!   two standard parallels, or one with a specific scale factor.
//! - **Lambert Conformal Conic** (`+proj=lcc`): Angles preserved; used in aviation
//!   charts and many national grids.
//!
//! All implementations here are sphere-based unless otherwise noted.

use crate::error::{Error, Result};

const DEFAULT_RADIUS: f64 = 6_378_137.0;
const TOLERANCE: f64 = 1e-12;

// ---------------------------------------------------------------------------
// Equidistant Conic projection
// ---------------------------------------------------------------------------

/// Equidistant Conic projection (`+proj=eqdc`).
///
/// Distances measured along all meridians are true. Two standard parallels φ₁
/// and φ₂ where the cone intersects the sphere.
///
/// **Forward (sphere, two standard parallels):**
/// ```text
/// n   = (cos φ₁ − cos φ₂) / (φ₂ − φ₁)   if φ₁ ≠ φ₂, else n = sin φ₁
/// G   = cos φ₁ / n + φ₁
/// ρ   = R · (G − φ)
/// ρ₀  = R · (G − φ₀)
/// θ   = n · (λ − λ₀)
/// x   = ρ · sin θ
/// y   = ρ₀ − ρ · cos θ
/// ```
#[derive(Debug, Clone)]
pub struct EquidistantConic {
    /// Central meridian λ₀ (degrees).
    pub lon_0: f64,
    /// Latitude of origin φ₀ (degrees).
    pub lat_0: f64,
    /// First standard parallel φ₁ (degrees).
    pub lat_1: f64,
    /// Second standard parallel φ₂ (degrees).
    pub lat_2: f64,
    /// False easting (metres).
    pub false_easting: f64,
    /// False northing (metres).
    pub false_northing: f64,
    /// Sphere radius (metres).
    pub radius: f64,
}

impl Default for EquidistantConic {
    fn default() -> Self {
        Self {
            lon_0: 0.0,
            lat_0: 0.0,
            lat_1: 30.0,
            lat_2: 60.0,
            false_easting: 0.0,
            false_northing: 0.0,
            radius: DEFAULT_RADIUS,
        }
    }
}

impl EquidistantConic {
    /// Creates an Equidistant Conic projection.
    pub fn new(
        lon_0: f64,
        lat_0: f64,
        lat_1: f64,
        lat_2: f64,
        false_easting: f64,
        false_northing: f64,
        radius: f64,
    ) -> Self {
        Self {
            lon_0,
            lat_0,
            lat_1,
            lat_2,
            false_easting,
            false_northing,
            radius,
        }
    }

    /// Computes the cone constant n and origin quantity G.
    fn cone_params(&self) -> Result<(f64, f64)> {
        let phi1 = self.lat_1.to_radians();
        let phi2 = self.lat_2.to_radians();

        let n = if (phi1 - phi2).abs() < TOLERANCE {
            phi1.sin()
        } else {
            (phi1.cos() - phi2.cos()) / (phi2 - phi1)
        };

        if n.abs() < TOLERANCE {
            return Err(Error::numerical_error(
                "equidistant conic: cone constant n is zero — invalid standard parallels",
            ));
        }

        let g = phi1.cos() / n + phi1;
        Ok((n, g))
    }

    /// Projects geographic coordinate (degrees) to projected metres.
    pub fn forward(&self, lon_deg: f64, lat_deg: f64) -> Result<(f64, f64)> {
        let phi = lat_deg.to_radians();
        let d_lam = (lon_deg - self.lon_0).to_radians();
        let phi_0 = self.lat_0.to_radians();

        let (n, g) = self.cone_params()?;
        let rho = self.radius * (g - phi);
        let rho_0 = self.radius * (g - phi_0);
        let theta = n * d_lam;

        let x = rho * theta.sin() + self.false_easting;
        let y = rho_0 - rho * theta.cos() + self.false_northing;

        if !x.is_finite() || !y.is_finite() {
            return Err(Error::numerical_error(
                "equidistant conic forward: non-finite result",
            ));
        }
        Ok((x, y))
    }

    /// Unprojects projected metres to geographic degrees.
    pub fn inverse(&self, x: f64, y: f64) -> Result<(f64, f64)> {
        let phi_0 = self.lat_0.to_radians();
        let (n, g) = self.cone_params()?;

        let xn = x - self.false_easting;
        let yn = y - self.false_northing;
        let rho_0 = self.radius * (g - phi_0);
        let y_adj = rho_0 - yn;

        let rho = (xn * xn + y_adj * y_adj).sqrt();
        let rho_signed = if n < 0.0 { -rho } else { rho };

        let phi = g - rho_signed / self.radius;
        let theta = xn.atan2(y_adj);
        let lam = self.lon_0 + (theta / n).to_degrees();

        Ok((lam, phi.to_degrees()))
    }
}

// ---------------------------------------------------------------------------
// Lambert Conformal Conic
// ---------------------------------------------------------------------------

/// Lambert Conformal Conic projection (`+proj=lcc`).
///
/// Preserves angles (conformal). Standard for aviation charts and national
/// coordinate systems in mid-latitudes.
///
/// **Forward (sphere, two standard parallels):**
/// ```text
/// n  = ln(cos φ₁ / cos φ₂) / ln[tan(π/4 + φ₂/2) / tan(π/4 + φ₁/2)]
/// F  = cos φ₁ · tan^n(π/4 + φ₁/2) / n
/// ρ₀ = R · F / tan^n(π/4 + φ₀/2)
/// ρ  = R · F / tan^n(π/4 + φ/2)
/// θ  = n · (λ − λ₀)
/// x  = ρ · sin θ
/// y  = ρ₀ − ρ · cos θ
/// ```
#[derive(Debug, Clone)]
pub struct LambertConformalConic {
    /// Central meridian λ₀ (degrees).
    pub lon_0: f64,
    /// Latitude of origin φ₀ (degrees).
    pub lat_0: f64,
    /// First standard parallel φ₁ (degrees).
    pub lat_1: f64,
    /// Second standard parallel φ₂ (degrees).
    pub lat_2: f64,
    /// False easting (metres).
    pub false_easting: f64,
    /// False northing (metres).
    pub false_northing: f64,
    /// Sphere radius (metres).
    pub radius: f64,
}

impl Default for LambertConformalConic {
    fn default() -> Self {
        Self {
            lon_0: 0.0,
            lat_0: 0.0,
            lat_1: 30.0,
            lat_2: 60.0,
            false_easting: 0.0,
            false_northing: 0.0,
            radius: DEFAULT_RADIUS,
        }
    }
}

impl LambertConformalConic {
    /// Creates a Lambert Conformal Conic projection.
    pub fn new(
        lon_0: f64,
        lat_0: f64,
        lat_1: f64,
        lat_2: f64,
        false_easting: f64,
        false_northing: f64,
        radius: f64,
    ) -> Self {
        Self {
            lon_0,
            lat_0,
            lat_1,
            lat_2,
            false_easting,
            false_northing,
            radius,
        }
    }

    /// Computes cone constant n, F, and ρ₀.
    fn cone_params(&self) -> Result<(f64, f64, f64)> {
        let phi0 = self.lat_0.to_radians();
        let phi1 = self.lat_1.to_radians();
        let phi2 = self.lat_2.to_radians();

        let t0 = ((std::f64::consts::FRAC_PI_4 + phi0 / 2.0).tan()).ln();
        let t1 = ((std::f64::consts::FRAC_PI_4 + phi1 / 2.0).tan()).ln();
        let t2 = ((std::f64::consts::FRAC_PI_4 + phi2 / 2.0).tan()).ln();

        let n = if (phi1 - phi2).abs() < TOLERANCE {
            phi1.sin()
        } else {
            // Snyder (1987) eq. 15-3: n = (ln cos φ₁ - ln cos φ₂) / (t₂ - t₁)
            // where t_i = ln tan(π/4 + φ_i/2)
            (phi1.cos().ln() - phi2.cos().ln()) / (t2 - t1)
        };

        if n.abs() < TOLERANCE {
            return Err(Error::numerical_error(
                "lambert conformal conic: n is zero — invalid standard parallels",
            ));
        }

        let f = phi1.cos() * (n * t1).exp() / n;
        let rho_0 = if t0.is_finite() {
            self.radius * f / (n * t0).exp()
        } else {
            0.0
        };

        Ok((n, f, rho_0))
    }

    /// Projects geographic coordinate (degrees) to projected metres.
    pub fn forward(&self, lon_deg: f64, lat_deg: f64) -> Result<(f64, f64)> {
        let phi = lat_deg.to_radians();
        let d_lam = (lon_deg - self.lon_0).to_radians();

        let (n, f, rho_0) = self.cone_params()?;
        let t = (std::f64::consts::FRAC_PI_4 + phi / 2.0).tan().ln();

        let rho = self.radius * f / (n * t).exp();
        let theta = n * d_lam;

        let x = rho * theta.sin() + self.false_easting;
        let y = rho_0 - rho * theta.cos() + self.false_northing;

        if !x.is_finite() || !y.is_finite() {
            return Err(Error::numerical_error(
                "lambert conformal conic forward: non-finite result",
            ));
        }
        Ok((x, y))
    }

    /// Unprojects projected metres to geographic degrees.
    pub fn inverse(&self, x: f64, y: f64) -> Result<(f64, f64)> {
        let (n, f, rho_0) = self.cone_params()?;

        let xn = x - self.false_easting;
        let yn = y - self.false_northing;

        let y_adj = rho_0 - yn;
        let rho = (xn * xn + y_adj * y_adj).sqrt();
        let rho_signed = if n < 0.0 { -rho } else { rho };

        let t_inv = (self.radius * f / rho_signed).ln() / n;
        let phi = 2.0 * t_inv.exp().atan() - std::f64::consts::FRAC_PI_2;

        let theta = xn.atan2(y_adj);
        let lam = self.lon_0 + (theta / n).to_degrees();

        Ok((lam, phi.to_degrees()))
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    const ROUND_TRIP_TOL: f64 = 1e-5; // degrees

    fn round_trip_eqdc(lon: f64, lat: f64) {
        let proj = EquidistantConic::default();
        let (x, y) = proj.forward(lon, lat).expect("forward ok");
        let (lon2, lat2) = proj.inverse(x, y).expect("inverse ok");
        assert!(
            (lon - lon2).abs() < ROUND_TRIP_TOL,
            "eqdc lon: {} vs {}",
            lon,
            lon2
        );
        assert!(
            (lat - lat2).abs() < ROUND_TRIP_TOL,
            "eqdc lat: {} vs {}",
            lat,
            lat2
        );
    }

    fn round_trip_lcc(proj: &LambertConformalConic, lon: f64, lat: f64) {
        let (x, y) = proj.forward(lon, lat).expect("forward ok");
        let (lon2, lat2) = proj.inverse(x, y).expect("inverse ok");
        assert!(
            (lon - lon2).abs() < ROUND_TRIP_TOL,
            "lcc lon: {} vs {}",
            lon,
            lon2
        );
        assert!(
            (lat - lat2).abs() < ROUND_TRIP_TOL,
            "lcc lat: {} vs {}",
            lat,
            lat2
        );
    }

    #[test]
    fn test_eqdc_origin() {
        let proj = EquidistantConic::default();
        let (x, y) = proj.forward(0.0, 0.0).expect("ok");
        assert!(x.abs() < 1e3, "x={}", x);
        assert!(y.is_finite(), "y={}", y);
    }

    #[test]
    fn test_eqdc_round_trips() {
        round_trip_eqdc(0.0, 45.0);
        round_trip_eqdc(10.0, 50.0);
        round_trip_eqdc(-50.0, 40.0);
        round_trip_eqdc(100.0, 55.0);
    }

    #[test]
    fn test_lcc_origin() {
        let proj = LambertConformalConic::default();
        let (x, y) = proj.forward(0.0, 45.0).expect("ok"); // at first standard parallel
        assert!(x.abs() < 1e3, "x={}", x);
        assert!(y.is_finite());
    }

    #[test]
    fn test_lcc_round_trips() {
        let proj = LambertConformalConic::default();
        round_trip_lcc(&proj, 0.0, 45.0);
        round_trip_lcc(&proj, 20.0, 50.0);
        round_trip_lcc(&proj, -30.0, 35.0);
    }

    #[test]
    fn test_lcc_etrs89_europe() {
        // EPSG:3034 parameters: lon_0=10, lat_0=52, lat_1=35, lat_2=65
        let proj = LambertConformalConic::new(
            10.0,
            52.0,
            35.0,
            65.0,
            4_000_000.0,
            2_800_000.0,
            DEFAULT_RADIUS,
        );
        let (x, y) = proj.forward(10.0, 52.0).expect("ok");
        // At the origin of the projection the result should be close to false easting/northing
        assert!((x - 4_000_000.0).abs() < 5000.0, "x at origin: {}", x);
        assert!((y - 2_800_000.0).abs() < 5000.0, "y at origin: {}", y);
    }
}
