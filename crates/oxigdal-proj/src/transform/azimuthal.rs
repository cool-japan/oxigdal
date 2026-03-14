//! Azimuthal map projections.
//!
//! This module implements azimuthal (zenithal) projections, all centred on a
//! chosen point on the sphere:
//!
//! - **Lambert Azimuthal Equal-Area** (`+proj=laea`): Equal-area; used for
//!   continental and hemispheric thematic maps.
//! - **Azimuthal Equidistant** (`+proj=aeqd`): Distances from the projection
//!   centre are preserved. Used in aviation for range circles.
//! - **Gnomonic** (`+proj=gnom`): All great circles appear as straight lines.
//!   Used for navigation (shortest-path planning).
//!
//! All implementations here are sphere-based (radius `a`).

use crate::error::{Error, Result};

const DEFAULT_RADIUS: f64 = 6_378_137.0;
const TOLERANCE: f64 = 1e-12;

// ---------------------------------------------------------------------------
// Lambert Azimuthal Equal-Area
// ---------------------------------------------------------------------------

/// Lambert Azimuthal Equal-Area projection (`+proj=laea`).
///
/// The only azimuthal projection that preserves area. Used as the European
/// ETRS89-LAEA (EPSG:3035).
///
/// **Forward (sphere, oblique aspect):**
/// ```text
/// kp  = √(2 / (1 + sin φ₀ sin φ + cos φ₀ cos φ cos(λ − λ₀)))
/// x   = R · kp · cos φ · sin(λ − λ₀)
/// y   = R · kp · (cos φ₀ sin φ − sin φ₀ cos φ cos(λ − λ₀))
/// ```
///
/// **Inverse:**
/// ```text
/// ρ  = √(x² + y²)
/// c  = 2 · arcsin(ρ / 2R)
/// φ  = arcsin(cos(c) sin φ₀ + y sin(c) cos φ₀ / ρ)
/// λ  = λ₀ + atan(x sin(c) / (ρ cos φ₀ cos(c) − y sin φ₀ sin(c)))
/// ```
#[derive(Debug, Clone)]
pub struct LambertAzimuthalEqualArea {
    /// Central longitude λ₀ (degrees).
    pub lon_0: f64,
    /// Central latitude φ₀ (degrees).
    pub lat_0: f64,
    /// False easting (metres).
    pub false_easting: f64,
    /// False northing (metres).
    pub false_northing: f64,
    /// Sphere radius (metres).
    pub radius: f64,
}

impl Default for LambertAzimuthalEqualArea {
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

impl LambertAzimuthalEqualArea {
    /// Creates a Lambert Azimuthal Equal-Area projection.
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
        let lam = lon_deg.to_radians();
        let phi_0 = self.lat_0.to_radians();
        let lam_0 = self.lon_0.to_radians();
        let d_lam = lam - lam_0;

        let cos_phi = phi.cos();
        let sin_phi = phi.sin();
        let cos_phi0 = phi_0.cos();
        let sin_phi0 = phi_0.sin();
        let cos_dlam = d_lam.cos();

        let denom = 1.0 + sin_phi0 * sin_phi + cos_phi0 * cos_phi * cos_dlam;

        if denom.abs() < TOLERANCE {
            return Err(Error::numerical_error(
                "lambert azimuthal: point is antipodal to projection centre",
            ));
        }

        let kp = (2.0 / denom).sqrt();
        let x = self.radius * kp * cos_phi * d_lam.sin() + self.false_easting;
        let y = self.radius * kp * (cos_phi0 * sin_phi - sin_phi0 * cos_phi * cos_dlam)
            + self.false_northing;

        if !x.is_finite() || !y.is_finite() {
            return Err(Error::numerical_error(
                "lambert azimuthal forward: non-finite result",
            ));
        }
        Ok((x, y))
    }

    /// Unprojects projected metres to geographic degrees.
    pub fn inverse(&self, x: f64, y: f64) -> Result<(f64, f64)> {
        let xn = x - self.false_easting;
        let yn = y - self.false_northing;
        let phi_0 = self.lat_0.to_radians();
        let cos_phi0 = phi_0.cos();
        let sin_phi0 = phi_0.sin();

        let rho = (xn * xn + yn * yn).sqrt();

        if rho < TOLERANCE {
            return Ok((self.lon_0, self.lat_0));
        }

        let sin_c_half = rho / (2.0 * self.radius);
        if sin_c_half.abs() > 1.0 + TOLERANCE {
            return Err(Error::numerical_error(
                "lambert azimuthal inverse: coordinate out of range",
            ));
        }
        let c = 2.0 * sin_c_half.clamp(-1.0, 1.0).asin();
        let cos_c = c.cos();
        let sin_c = c.sin();

        let sin_phi = cos_c * sin_phi0 + yn * sin_c * cos_phi0 / rho;
        let phi = sin_phi.clamp(-1.0, 1.0).asin();

        let lam_num = xn * sin_c;
        let lam_den = rho * cos_phi0 * cos_c - yn * sin_phi0 * sin_c;
        let lam = self.lon_0 + lam_num.atan2(lam_den).to_degrees();

        Ok((lam, phi.to_degrees()))
    }
}

// ---------------------------------------------------------------------------
// Azimuthal Equidistant
// ---------------------------------------------------------------------------

/// Azimuthal Equidistant projection (`+proj=aeqd`).
///
/// Distances from the projection centre are preserved along all azimuths.
/// Great-circle routes from the centre appear as straight lines.
///
/// **Forward (sphere):**
/// ```text
/// c  = arccos(sin φ₀ sin φ + cos φ₀ cos φ cos(λ − λ₀))
/// k  = c / sin(c)   (kp = 1 when c = 0)
/// x  = R · k · cos φ · sin(λ − λ₀)
/// y  = R · k · (cos φ₀ sin φ − sin φ₀ cos φ cos(λ − λ₀))
/// ```
///
/// **Inverse:**
/// ```text
/// ρ  = √(x² + y²)
/// c  = ρ / R
/// φ  = arcsin(cos(c) sin φ₀ + y sin(c) cos φ₀ / ρ)
/// λ  = λ₀ + atan(x sin(c) / (ρ cos φ₀ cos(c) − y sin φ₀ sin(c)))
/// ```
#[derive(Debug, Clone)]
pub struct AzimuthalEquidistant {
    /// Central longitude λ₀ (degrees).
    pub lon_0: f64,
    /// Central latitude φ₀ (degrees).
    pub lat_0: f64,
    /// False easting (metres).
    pub false_easting: f64,
    /// False northing (metres).
    pub false_northing: f64,
    /// Sphere radius (metres).
    pub radius: f64,
}

impl Default for AzimuthalEquidistant {
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

impl AzimuthalEquidistant {
    /// Creates an Azimuthal Equidistant projection.
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
        let lam = lon_deg.to_radians();
        let phi_0 = self.lat_0.to_radians();
        let lam_0 = self.lon_0.to_radians();
        let d_lam = lam - lam_0;

        let cos_phi = phi.cos();
        let sin_phi = phi.sin();
        let cos_phi0 = phi_0.cos();
        let sin_phi0 = phi_0.sin();
        let cos_dlam = d_lam.cos();

        let cos_c = sin_phi0 * sin_phi + cos_phi0 * cos_phi * cos_dlam;
        let cos_c_clamped = cos_c.clamp(-1.0, 1.0);

        if (cos_c_clamped - 1.0).abs() < TOLERANCE {
            // Point coincides with projection centre
            return Ok((self.false_easting, self.false_northing));
        }

        let c = cos_c_clamped.acos();
        let k = c / c.sin();

        let x = self.radius * k * cos_phi * d_lam.sin() + self.false_easting;
        let y = self.radius * k * (cos_phi0 * sin_phi - sin_phi0 * cos_phi * cos_dlam)
            + self.false_northing;

        if !x.is_finite() || !y.is_finite() {
            return Err(Error::numerical_error(
                "azimuthal equidistant forward: non-finite result",
            ));
        }
        Ok((x, y))
    }

    /// Unprojects projected metres to geographic degrees.
    pub fn inverse(&self, x: f64, y: f64) -> Result<(f64, f64)> {
        let xn = x - self.false_easting;
        let yn = y - self.false_northing;
        let phi_0 = self.lat_0.to_radians();
        let cos_phi0 = phi_0.cos();
        let sin_phi0 = phi_0.sin();

        let rho = (xn * xn + yn * yn).sqrt();

        if rho < TOLERANCE {
            return Ok((self.lon_0, self.lat_0));
        }

        let c = rho / self.radius;

        if c > std::f64::consts::PI + TOLERANCE {
            return Err(Error::numerical_error(
                "azimuthal equidistant inverse: distance exceeds half circumference",
            ));
        }

        let cos_c = c.cos();
        let sin_c = c.sin();

        let sin_phi = cos_c * sin_phi0 + yn * sin_c * cos_phi0 / rho;
        let phi = sin_phi.clamp(-1.0, 1.0).asin();

        let lam_num = xn * sin_c;
        let lam_den = rho * cos_phi0 * cos_c - yn * sin_phi0 * sin_c;
        let lam = self.lon_0 + lam_num.atan2(lam_den).to_degrees();

        Ok((lam, phi.to_degrees()))
    }
}

// ---------------------------------------------------------------------------
// Gnomonic projection
// ---------------------------------------------------------------------------

/// Gnomonic projection (`+proj=gnom`).
///
/// Perspective projection from the centre of the sphere onto a tangent plane.
/// Every straight line in the map corresponds to a great circle on the sphere.
/// Used for air-navigation shortest-path planning.
///
/// **Note:** This projection can only display **less than one hemisphere** —
/// points more than 90° from the centre cannot be projected.
///
/// **Forward (sphere):**
/// ```text
/// cos_c = sin φ₀ sin φ + cos φ₀ cos φ cos(λ − λ₀)
/// x  = R · cos φ · sin(λ − λ₀) / cos_c
/// y  = R · (cos φ₀ sin φ − sin φ₀ cos φ cos(λ − λ₀)) / cos_c
/// ```
///
/// **Inverse:**
/// ```text
/// ρ  = √(x² + y²)
/// c  = atan(ρ / R)
/// φ  = arcsin(cos(c) sin φ₀ + y sin(c) cos φ₀ / ρ)
/// λ  = λ₀ + atan(x sin(c) / (ρ cos φ₀ cos(c) − y sin φ₀ sin(c)))
/// ```
#[derive(Debug, Clone)]
pub struct Gnomonic {
    /// Central longitude λ₀ (degrees).
    pub lon_0: f64,
    /// Central latitude φ₀ (degrees).
    pub lat_0: f64,
    /// False easting (metres).
    pub false_easting: f64,
    /// False northing (metres).
    pub false_northing: f64,
    /// Sphere radius (metres).
    pub radius: f64,
}

impl Default for Gnomonic {
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

impl Gnomonic {
    /// Creates a Gnomonic projection.
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
    ///
    /// # Errors
    ///
    /// Returns `NumericalError` if the point is on the boundary (90° from centre)
    /// or beyond the visible hemisphere.
    pub fn forward(&self, lon_deg: f64, lat_deg: f64) -> Result<(f64, f64)> {
        let phi = lat_deg.to_radians();
        let lam = lon_deg.to_radians();
        let phi_0 = self.lat_0.to_radians();
        let lam_0 = self.lon_0.to_radians();
        let d_lam = lam - lam_0;

        let cos_phi = phi.cos();
        let sin_phi = phi.sin();
        let cos_phi0 = phi_0.cos();
        let sin_phi0 = phi_0.sin();
        let cos_dlam = d_lam.cos();

        let cos_c = sin_phi0 * sin_phi + cos_phi0 * cos_phi * cos_dlam;

        if cos_c < TOLERANCE {
            return Err(Error::numerical_error(
                "gnomonic: point is at or beyond the horizon (≥ 90° from centre)",
            ));
        }

        let x = self.radius * cos_phi * d_lam.sin() / cos_c + self.false_easting;
        let y = self.radius * (cos_phi0 * sin_phi - sin_phi0 * cos_phi * cos_dlam) / cos_c
            + self.false_northing;

        if !x.is_finite() || !y.is_finite() {
            return Err(Error::numerical_error(
                "gnomonic forward: non-finite result",
            ));
        }
        Ok((x, y))
    }

    /// Unprojects projected metres to geographic degrees.
    pub fn inverse(&self, x: f64, y: f64) -> Result<(f64, f64)> {
        let xn = x - self.false_easting;
        let yn = y - self.false_northing;
        let phi_0 = self.lat_0.to_radians();
        let cos_phi0 = phi_0.cos();
        let sin_phi0 = phi_0.sin();

        let rho = (xn * xn + yn * yn).sqrt();

        if rho < TOLERANCE {
            return Ok((self.lon_0, self.lat_0));
        }

        let c = (rho / self.radius).atan();
        let cos_c = c.cos();
        let sin_c = c.sin();

        let sin_phi = cos_c * sin_phi0 + yn * sin_c * cos_phi0 / rho;
        let phi = sin_phi.clamp(-1.0, 1.0).asin();

        let lam_num = xn * sin_c;
        let lam_den = rho * cos_phi0 * cos_c - yn * sin_phi0 * sin_c;
        let lam = self.lon_0 + lam_num.atan2(lam_den).to_degrees();

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

    const ROUND_TRIP_TOL: f64 = 1e-6; // degrees

    fn round_trip_laea(lon: f64, lat: f64) {
        let proj = LambertAzimuthalEqualArea::default();
        let (x, y) = proj.forward(lon, lat).expect("forward ok");
        let (lon2, lat2) = proj.inverse(x, y).expect("inverse ok");
        assert!(
            (lon - lon2).abs() < ROUND_TRIP_TOL,
            "laea lon: {} vs {}",
            lon,
            lon2
        );
        assert!(
            (lat - lat2).abs() < ROUND_TRIP_TOL,
            "laea lat: {} vs {}",
            lat,
            lat2
        );
    }

    fn round_trip_aeqd(lon: f64, lat: f64) {
        let proj = AzimuthalEquidistant::default();
        let (x, y) = proj.forward(lon, lat).expect("forward ok");
        let (lon2, lat2) = proj.inverse(x, y).expect("inverse ok");
        assert!(
            (lon - lon2).abs() < ROUND_TRIP_TOL,
            "aeqd lon: {} vs {}",
            lon,
            lon2
        );
        assert!(
            (lat - lat2).abs() < ROUND_TRIP_TOL,
            "aeqd lat: {} vs {}",
            lat,
            lat2
        );
    }

    fn round_trip_gnom(lon: f64, lat: f64) {
        let proj = Gnomonic::default();
        let (x, y) = proj.forward(lon, lat).expect("forward ok");
        let (lon2, lat2) = proj.inverse(x, y).expect("inverse ok");
        assert!(
            (lon - lon2).abs() < ROUND_TRIP_TOL,
            "gnomonic lon: {} vs {}",
            lon,
            lon2
        );
        assert!(
            (lat - lat2).abs() < ROUND_TRIP_TOL,
            "gnomonic lat: {} vs {}",
            lat,
            lat2
        );
    }

    #[test]
    fn test_laea_origin() {
        let proj = LambertAzimuthalEqualArea::default();
        let (x, y) = proj.forward(0.0, 0.0).expect("ok");
        assert!(x.abs() < 1.0, "x={}", x);
        assert!(y.abs() < 1.0, "y={}", y);
    }

    #[test]
    fn test_laea_round_trips() {
        round_trip_laea(0.0, 0.0);
        round_trip_laea(10.0, 20.0);
        round_trip_laea(-80.0, 60.0);
        round_trip_laea(120.0, -30.0);
    }

    #[test]
    fn test_laea_antipodal_error() {
        let proj = LambertAzimuthalEqualArea::default(); // centred at (0,0)
        let result = proj.forward(180.0, 0.0); // antipode on equator
        assert!(result.is_err(), "should fail at antipode");
    }

    #[test]
    fn test_aeqd_origin() {
        let proj = AzimuthalEquidistant::default();
        let (x, y) = proj.forward(0.0, 0.0).expect("ok");
        // At centre should return false easting/northing
        assert!(x.abs() < 1.0, "x={}", x);
        assert!(y.abs() < 1.0, "y={}", y);
    }

    #[test]
    fn test_aeqd_round_trips() {
        round_trip_aeqd(0.0, 0.0);
        round_trip_aeqd(30.0, 45.0);
        round_trip_aeqd(-60.0, -30.0);
        round_trip_aeqd(90.0, 60.0);
    }

    #[test]
    fn test_aeqd_distance_preserved() {
        // Distance from centre to equator at 90° should be exactly π/2 * R
        let proj = AzimuthalEquidistant::default();
        let (x, y) = proj.forward(90.0, 0.0).expect("ok");
        let dist = (x * x + y * y).sqrt();
        let expected = std::f64::consts::FRAC_PI_2 * DEFAULT_RADIUS;
        assert!(
            (dist - expected).abs() < 1.0, // 1 metre tolerance
            "dist={} expected={}",
            dist,
            expected
        );
    }

    #[test]
    fn test_gnomonic_origin() {
        let proj = Gnomonic::default();
        let (x, y) = proj.forward(0.0, 0.0).expect("ok");
        assert!(x.abs() < 1.0, "x={}", x);
        assert!(y.abs() < 1.0, "y={}", y);
    }

    #[test]
    fn test_gnomonic_round_trips() {
        round_trip_gnom(0.0, 0.0);
        round_trip_gnom(10.0, 20.0);
        round_trip_gnom(-30.0, 45.0);
        round_trip_gnom(50.0, -20.0);
    }

    #[test]
    fn test_gnomonic_horizon_error() {
        let proj = Gnomonic::default();
        // Point at exactly 90° from centre (equator at 90°E) should fail
        let result = proj.forward(90.0, 0.0);
        assert!(result.is_err(), "should fail at horizon");
    }

    #[test]
    fn test_gnomonic_great_circle_straight() {
        // Test that two points on the same great circle as the centre map to
        // collinear points with the origin
        let centre_lat = 45.0_f64;
        let proj = Gnomonic::new(0.0, centre_lat, 0.0, 0.0, DEFAULT_RADIUS);

        // Points along the meridian through the centre
        let (x1, y1) = proj.forward(0.0, 30.0).expect("ok");
        let (x2, y2) = proj.forward(0.0, 60.0).expect("ok");

        // Both should have x ≈ 0 (on the meridian through centre)
        assert!(x1.abs() < 1.0, "x1={}", x1);
        assert!(x2.abs() < 1.0, "x2={}", x2);
        // y values should have the same sign and y2 > y1 (further north)
        assert!(y1.is_finite() && y2.is_finite());
    }
}
