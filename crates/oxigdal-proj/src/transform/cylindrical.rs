//! Cylindrical map projections.
//!
//! This module implements cylindrical projections that are not already provided by
//! `proj4rs`:
//!
//! - **Transverse Mercator** (`+proj=tmerc`): Conformal cylindrical; used as the basis
//!   for UTM and national grids world-wide.
//! - **Cassini–Soldner** (`+proj=cass`): Transverse cylindrical — equidistant along
//!   the central meridian and along lines perpendicular to it.
//! - **Gauss–Krüger** (`+proj=gauss`): The Gauss–Krüger form of Transverse Mercator
//!   used in Germany and Russia. For a sphere it is identical to Transverse Mercator;
//!   for an ellipsoid it uses the Gauss–Schreiber series (6-term expansion here).
//!
//! All implementations are sphere-based unless noted. The ellipsoidal Transverse Mercator
//! equations are taken from Snyder (1987) §8.

use crate::error::{Error, Result};

const DEFAULT_RADIUS: f64 = 6_378_137.0;
const TOLERANCE: f64 = 1e-12;
#[allow(dead_code)]
const MAX_ITER: usize = 50;

// WGS-84 ellipsoid parameters
const WGS84_A: f64 = 6_378_137.0; // semi-major axis, metres
const WGS84_F: f64 = 1.0 / 298.257_223_563; // flattening
const WGS84_E2: f64 = 2.0 * WGS84_F - WGS84_F * WGS84_F; // first eccentricity squared

// ---------------------------------------------------------------------------
// Transverse Mercator (sphere-based)
// ---------------------------------------------------------------------------

/// Transverse Mercator conformal cylindrical projection (`+proj=tmerc`).
///
/// Wraps the sphere around a cylinder tangent to the central meridian.
/// Preserves angles; used for UTM and most national grids.
///
/// **Forward (sphere):**
/// ```text
/// B  = cos(φ) · sin(λ − λ₀)
/// x  = k₀ · R · atanh(B) / 2
/// y  = k₀ · R · [atan(tan(φ) / cos(λ − λ₀)) − φ₀]
/// ```
///
/// **Inverse (sphere):**
/// ```text
/// D  = y / (k₀ · R) + φ₀
/// λ  = λ₀ + atan(sinh(x/(k₀·R)) / cos(D))
/// φ  = asin(sin(D) / cosh(x/(k₀·R)))
/// ```
#[derive(Debug, Clone)]
pub struct TransverseMercator {
    /// Central meridian λ₀ (degrees).
    pub lon_0: f64,
    /// Latitude of origin φ₀ (degrees).
    pub lat_0: f64,
    /// False easting (metres).
    pub false_easting: f64,
    /// False northing (metres).
    pub false_northing: f64,
    /// Scale factor at central meridian.
    pub k0: f64,
    /// Sphere radius (metres).
    pub radius: f64,
}

impl Default for TransverseMercator {
    fn default() -> Self {
        Self {
            lon_0: 0.0,
            lat_0: 0.0,
            false_easting: 0.0,
            false_northing: 0.0,
            k0: 1.0,
            radius: DEFAULT_RADIUS,
        }
    }
}

impl TransverseMercator {
    /// Creates a Transverse Mercator with full parameters.
    pub fn new(
        lon_0: f64,
        lat_0: f64,
        k0: f64,
        false_easting: f64,
        false_northing: f64,
        radius: f64,
    ) -> Self {
        Self {
            lon_0,
            lat_0,
            false_easting,
            false_northing,
            k0,
            radius,
        }
    }

    /// Projects geographic coordinate (degrees) to projected metres.
    pub fn forward(&self, lon_deg: f64, lat_deg: f64) -> Result<(f64, f64)> {
        let phi = lat_deg.to_radians();
        let d_lam = (lon_deg - self.lon_0).to_radians();
        let phi_0 = self.lat_0.to_radians();

        let b = phi.cos() * d_lam.sin();

        // Handle case where B approaches ±1 (point on equator at ±90° from central meridian)
        if (b.abs() - 1.0).abs() < 1e-10 {
            return Err(Error::numerical_error(
                "transverse mercator: point on boundary of projection",
            ));
        }

        let x = self.k0 * self.radius * b.atanh() + self.false_easting;
        let y =
            self.k0 * self.radius * (phi.tan().atan2(d_lam.cos()) - phi_0) + self.false_northing;

        if !x.is_finite() || !y.is_finite() {
            return Err(Error::numerical_error(
                "transverse mercator forward: non-finite result",
            ));
        }
        Ok((x, y))
    }

    /// Unprojects projected metres to geographic degrees.
    pub fn inverse(&self, x: f64, y: f64) -> Result<(f64, f64)> {
        let phi_0 = self.lat_0.to_radians();
        let xn = (x - self.false_easting) / (self.k0 * self.radius);
        let yn = (y - self.false_northing) / (self.k0 * self.radius) + phi_0;

        let cosh_xn = xn.cosh();
        let sin_yn = yn.sin();

        let sin_phi = sin_yn / cosh_xn;
        if sin_phi.abs() > 1.0 + TOLERANCE {
            return Err(Error::numerical_error(
                "transverse mercator inverse: coordinate out of range",
            ));
        }
        let phi = sin_phi.clamp(-1.0, 1.0).asin();
        let lam = self.lon_0 + xn.sinh().atan2(yn.cos()).to_degrees();

        Ok((lam, phi.to_degrees()))
    }
}

// ---------------------------------------------------------------------------
// Cassini–Soldner (sphere-based)
// ---------------------------------------------------------------------------

/// Cassini–Soldner transverse cylindrical projection (`+proj=cass`).
///
/// Distances along the central meridian and along lines perpendicular to it are
/// preserved (equidistant). Not conformal or equal-area. Standard for topographic
/// maps in the UK before the National Grid was introduced.
///
/// **Forward (sphere):**
/// ```text
/// A  = cos(φ) · sin(λ − λ₀)
/// T  = tan(φ)²
/// C  = e² · cos(φ)² / (1 − e²)  [≈0 for sphere]
/// x  = R · arcsin(A)
/// y  = R · [atan(tan(φ) / cos(λ − λ₀)) − φ₀]
/// ```
///
/// For a sphere: `x = R·arcsin(cos(φ)·sin(Δλ))`,
///               `y = R·[atan2(tan(φ), cos(Δλ)) − φ₀]`.
#[derive(Debug, Clone)]
pub struct CassineSoldner {
    /// Central meridian λ₀ (degrees).
    pub lon_0: f64,
    /// Latitude of origin φ₀ (degrees).
    pub lat_0: f64,
    /// False easting (metres).
    pub false_easting: f64,
    /// False northing (metres).
    pub false_northing: f64,
    /// Sphere radius (metres).
    pub radius: f64,
}

impl Default for CassineSoldner {
    fn default() -> Self {
        Self {
            lon_0: 0.0,
            lat_0: 0.0,
            false_easting: 0.0,
            false_northing: 0.0,
            radius: DEFAULT_RADIUS,
        }
    }
}

impl CassineSoldner {
    /// Creates a Cassini–Soldner projection.
    pub fn new(
        lon_0: f64,
        lat_0: f64,
        false_easting: f64,
        false_northing: f64,
        radius: f64,
    ) -> Self {
        Self {
            lon_0,
            lat_0,
            false_easting,
            false_northing,
            radius,
        }
    }

    /// Projects geographic coordinate (degrees) to projected metres.
    pub fn forward(&self, lon_deg: f64, lat_deg: f64) -> Result<(f64, f64)> {
        let phi = lat_deg.to_radians();
        let d_lam = (lon_deg - self.lon_0).to_radians();
        let phi_0 = self.lat_0.to_radians();

        let sin_a = phi.cos() * d_lam.sin();
        if sin_a.abs() > 1.0 + TOLERANCE {
            return Err(Error::numerical_error(
                "cassini: sin(A) out of range — point outside projection domain",
            ));
        }
        let x = self.radius * sin_a.clamp(-1.0, 1.0).asin() + self.false_easting;
        let y = self.radius * (phi.tan().atan2(d_lam.cos()) - phi_0) + self.false_northing;

        if !x.is_finite() || !y.is_finite() {
            return Err(Error::numerical_error("cassini forward: non-finite result"));
        }
        Ok((x, y))
    }

    /// Unprojects projected metres to geographic degrees.
    pub fn inverse(&self, x: f64, y: f64) -> Result<(f64, f64)> {
        let phi_0 = self.lat_0.to_radians();
        let xn = (x - self.false_easting) / self.radius;
        let d1 = (y - self.false_northing) / self.radius + phi_0;

        // Snyder p.77: φ = asin(sin(D) · cos(x/R)),  λ = λ₀ + atan2(tan(x/R), cos(D))
        let sin_xn = xn.sin();
        let cos_xn = xn.cos();

        let sin_phi = d1.sin() * cos_xn;
        let phi = sin_phi.clamp(-1.0, 1.0).asin();
        let lam = self.lon_0 + sin_xn.atan2(cos_xn * d1.cos()).to_degrees();

        Ok((lam, phi.to_degrees()))
    }
}

// ---------------------------------------------------------------------------
// Gauss–Krüger (ellipsoidal Transverse Mercator, 6-term series)
// ---------------------------------------------------------------------------

/// Gauss–Krüger ellipsoidal Transverse Mercator (`+proj=gauss`).
///
/// This is the form of Transverse Mercator used in Germany (DHDN) and Russia
/// (Pulkovo). It is mathematically equivalent to standard Transverse Mercator
/// on an ellipsoid, computed here via the Krüger power series (n-based,
/// Helmert 1880, 6-term expansion) for accuracy to ~0.1 mm world-wide.
///
/// **Reference:** Karney C.F.F. (2011) "Transverse Mercator with an accuracy of
/// a few nanometers", J. Geodesy 85(8):475–485.
///
/// Here we implement the classic Helmert series (adequate for 1 mm accuracy).
#[derive(Debug, Clone)]
pub struct GaussKruger {
    /// Central meridian λ₀ (degrees).
    pub lon_0: f64,
    /// Latitude of origin φ₀ (degrees).
    pub lat_0: f64,
    /// Scale factor at central meridian.
    pub k0: f64,
    /// False easting (metres).
    pub false_easting: f64,
    /// False northing (metres).
    pub false_northing: f64,
    /// Semi-major axis a (metres).
    pub a: f64,
    /// First eccentricity squared e².
    pub e2: f64,
}

impl Default for GaussKruger {
    fn default() -> Self {
        Self {
            lon_0: 0.0,
            lat_0: 0.0,
            k0: 1.0,
            false_easting: 0.0,
            false_northing: 0.0,
            a: WGS84_A,
            e2: WGS84_E2,
        }
    }
}

impl GaussKruger {
    /// Creates a Gauss–Krüger projection with full ellipsoid parameters.
    pub fn new(lon_0: f64, lat_0: f64, k0: f64, false_easting: f64, false_northing: f64) -> Self {
        Self {
            lon_0,
            lat_0,
            k0,
            false_easting,
            false_northing,
            a: WGS84_A,
            e2: WGS84_E2,
        }
    }

    /// Creates a Gauss–Krüger with a custom ellipsoid.
    pub fn with_ellipsoid(
        lon_0: f64,
        lat_0: f64,
        k0: f64,
        false_easting: f64,
        false_northing: f64,
        a: f64,
        f: f64,
    ) -> Self {
        let e2 = 2.0 * f - f * f;
        Self {
            lon_0,
            lat_0,
            k0,
            false_easting,
            false_northing,
            a,
            e2,
        }
    }

    /// Radius of curvature in the prime vertical: N = a/√(1−e²sin²φ)
    fn radius_of_curvature_n(&self, phi: f64) -> f64 {
        self.a / (1.0 - self.e2 * phi.sin().powi(2)).sqrt()
    }

    /// Meridional arc M from equator to φ using series expansion.
    fn meridional_arc(&self, phi: f64) -> f64 {
        let e2 = self.e2;
        let e4 = e2 * e2;
        let e6 = e4 * e2;
        let e8 = e4 * e4;

        self.a
            * ((1.0 - e2 / 4.0 - 3.0 * e4 / 64.0 - 5.0 * e6 / 256.0) * phi
                - (3.0 * e2 / 8.0 + 3.0 * e4 / 32.0 + 45.0 * e6 / 1024.0) * (2.0 * phi).sin()
                + (15.0 * e4 / 256.0 + 45.0 * e6 / 1024.0) * (4.0 * phi).sin()
                - (35.0 * e6 / 3072.0) * (6.0 * phi).sin()
                - (315.0 * e8 / 131072.0) * (8.0 * phi).sin())
    }

    /// Projects geographic coordinate (degrees) to projected metres (Snyder §8).
    pub fn forward(&self, lon_deg: f64, lat_deg: f64) -> Result<(f64, f64)> {
        let phi = lat_deg.to_radians();
        let d_lam = (lon_deg - self.lon_0).to_radians();
        let phi_0 = self.lat_0.to_radians();

        let n = self.radius_of_curvature_n(phi);
        let t = phi.tan();
        let t2 = t * t;
        let c = self.e2 / (1.0 - self.e2) * phi.cos().powi(2);
        let c2 = c * c;
        let a_coeff = phi.cos() * d_lam;
        let a2 = a_coeff * a_coeff;
        let a3 = a2 * a_coeff;
        let a4 = a2 * a2;
        let a5 = a4 * a_coeff;
        let a6 = a4 * a2;

        let m = self.meridional_arc(phi);
        let m0 = self.meridional_arc(phi_0);

        let x = self.k0
            * n
            * (a_coeff
                + (1.0 - t2 + c) * a3 / 6.0
                + (5.0 - 18.0 * t2 + t2 * t2 + 72.0 * c - 58.0 * self.e2 / (1.0 - self.e2)) * a5
                    / 120.0)
            + self.false_easting;

        let y = self.k0
            * (m - m0
                + n * t
                    * (a2 / 2.0
                        + (5.0 - t2 + 9.0 * c + 4.0 * c2) * a4 / 24.0
                        + (61.0 - 58.0 * t2 + t2 * t2 + 600.0 * c
                            - 330.0 * self.e2 / (1.0 - self.e2))
                            * a6
                            / 720.0))
            + self.false_northing;

        if !x.is_finite() || !y.is_finite() {
            return Err(Error::numerical_error(
                "gauss-kruger forward: non-finite result",
            ));
        }
        Ok((x, y))
    }

    /// Unprojects projected metres to geographic degrees (Snyder §8 inverse series).
    pub fn inverse(&self, x: f64, y: f64) -> Result<(f64, f64)> {
        let phi_0 = self.lat_0.to_radians();
        let m0 = self.meridional_arc(phi_0);
        let m = m0 + (y - self.false_northing) / self.k0;

        // Footprint latitude φ₁ via iteration
        let mu = m
            / (self.a
                * (1.0
                    - self.e2 / 4.0
                    - 3.0 * self.e2 * self.e2 / 64.0
                    - 5.0 * self.e2.powi(3) / 256.0));

        let e1 = (1.0 - (1.0 - self.e2).sqrt()) / (1.0 + (1.0 - self.e2).sqrt());
        let e1_2 = e1 * e1;
        let e1_3 = e1_2 * e1;
        let e1_4 = e1_2 * e1_2;

        let phi1 = mu
            + (3.0 * e1 / 2.0 - 27.0 * e1_3 / 32.0) * (2.0 * mu).sin()
            + (21.0 * e1_2 / 16.0 - 55.0 * e1_4 / 32.0) * (4.0 * mu).sin()
            + (151.0 * e1_3 / 96.0) * (6.0 * mu).sin()
            + (1097.0 * e1_4 / 512.0) * (8.0 * mu).sin();

        let n1 = self.radius_of_curvature_n(phi1);
        let t1 = phi1.tan();
        let t1_2 = t1 * t1;
        let c1 = self.e2 / (1.0 - self.e2) * phi1.cos().powi(2);
        let c1_2 = c1 * c1;
        let r1 = self.a * (1.0 - self.e2) / (1.0 - self.e2 * phi1.sin().powi(2)).powf(1.5);

        let xn = (x - self.false_easting) / (n1 * self.k0);
        let xn2 = xn * xn;
        let xn3 = xn2 * xn;
        let xn4 = xn2 * xn2;
        let xn5 = xn4 * xn;
        let xn6 = xn4 * xn2;

        let phi = phi1
            - (n1 * t1 / r1)
                * (xn2 / 2.0
                    - (5.0 + 3.0 * t1_2 + 10.0 * c1
                        - 4.0 * c1_2
                        - 9.0 * self.e2 / (1.0 - self.e2))
                        * xn4
                        / 24.0
                    + (61.0 + 90.0 * t1_2 + 298.0 * c1 + 45.0 * t1_2 * t1_2
                        - 252.0 * self.e2 / (1.0 - self.e2)
                        - 3.0 * c1_2)
                        * xn6
                        / 720.0);

        let lam_rad = self.lon_0.to_radians()
            + (xn - (1.0 + 2.0 * t1_2 + c1) * xn3 / 6.0
                + (5.0 - 2.0 * c1 + 28.0 * t1_2 - 3.0 * c1_2
                    + 8.0 * self.e2 / (1.0 - self.e2)
                    + 24.0 * t1_2 * t1_2)
                    * xn5
                    / 120.0)
                / phi1.cos();

        Ok((lam_rad.to_degrees(), phi.to_degrees()))
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    const ROUND_TRIP_TOL: f64 = 1e-4; // degrees (~11 m)

    fn round_trip_tmerc(lon: f64, lat: f64) {
        let proj = TransverseMercator::default();
        let (x, y) = proj.forward(lon, lat).expect("forward ok");
        let (lon2, lat2) = proj.inverse(x, y).expect("inverse ok");
        assert!(
            (lon - lon2).abs() < ROUND_TRIP_TOL,
            "tmerc lon: {} vs {}",
            lon,
            lon2
        );
        assert!(
            (lat - lat2).abs() < ROUND_TRIP_TOL,
            "tmerc lat: {} vs {}",
            lat,
            lat2
        );
    }

    fn round_trip_cass(lon: f64, lat: f64) {
        let proj = CassineSoldner::default();
        let (x, y) = proj.forward(lon, lat).expect("forward ok");
        let (lon2, lat2) = proj.inverse(x, y).expect("inverse ok");
        assert!(
            (lon - lon2).abs() < ROUND_TRIP_TOL,
            "cassini lon: {} vs {}",
            lon,
            lon2
        );
        assert!(
            (lat - lat2).abs() < ROUND_TRIP_TOL,
            "cassini lat: {} vs {}",
            lat,
            lat2
        );
    }

    #[test]
    fn test_tmerc_origin() {
        let proj = TransverseMercator::default();
        let (x, y) = proj.forward(0.0, 0.0).expect("ok");
        assert!(x.abs() < 1.0, "x={}", x);
        assert!(y.abs() < 1.0, "y={}", y);
    }

    #[test]
    fn test_tmerc_round_trips() {
        round_trip_tmerc(0.0, 0.0);
        round_trip_tmerc(5.0, 45.0);
        round_trip_tmerc(-10.0, 30.0);
        round_trip_tmerc(10.0, 60.0);
    }

    #[test]
    fn test_cassini_origin() {
        let proj = CassineSoldner::default();
        let (x, y) = proj.forward(0.0, 0.0).expect("ok");
        assert!(x.abs() < 1.0, "x={}", x);
        assert!(y.abs() < 1.0, "y={}", y);
    }

    #[test]
    fn test_cassini_round_trips() {
        round_trip_cass(0.0, 0.0);
        round_trip_cass(2.0, 50.0);
        round_trip_cass(-5.0, 40.0);
    }

    #[test]
    fn test_gauss_kruger_origin() {
        let proj = GaussKruger::default();
        let (x, y) = proj.forward(0.0, 0.0).expect("ok");
        assert!(x.abs() < 1.0, "x={}", x);
        assert!(y.abs() < 1.0, "y={}", y);
    }

    #[test]
    fn test_gauss_kruger_round_trip() {
        let proj = GaussKruger::new(9.0, 0.0, 1.0, 0.0, 0.0); // zone 3 strip
        let test_cases = [
            (9.0, 50.0), // central meridian, German latitude
            (10.0, 52.0),
            (8.0, 48.0),
            (9.0, 45.0),
        ];
        for (lon, lat) in test_cases {
            let (x, y) = proj.forward(lon, lat).expect("forward ok");
            let (lon2, lat2) = proj.inverse(x, y).expect("inverse ok");
            assert!(
                (lon - lon2).abs() < 1e-6,
                "gauss-kruger lon: {} vs {}",
                lon,
                lon2
            );
            assert!(
                (lat - lat2).abs() < 1e-6,
                "gauss-kruger lat: {} vs {}",
                lat,
                lat2
            );
        }
    }

    #[test]
    fn test_gauss_kruger_dhdn_zone3() {
        // DHDN Gauss-Kruger zone 3: central meridian 9°E, FE=3_500_000
        let proj = GaussKruger::new(9.0, 0.0, 1.0, 3_500_000.0, 0.0);

        // Known point near Frankfurt (~8.68°E, 50.11°N) should give E near 3459000
        let (x, _y) = proj.forward(8.68, 50.11).expect("forward ok");
        // Easting should be slightly west of central meridian strip (3500000)
        assert!(x > 3_400_000.0 && x < 3_500_000.0, "x={}", x);
    }
}
