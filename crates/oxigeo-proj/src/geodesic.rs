//! Vincenty geodetic distance and azimuth calculations on an arbitrary ellipsoid.
//!
//! Implements Vincenty's (1975) inverse and direct formulae for geodesic distance/azimuth.
//! Reference: Vincenty, T. (1975). "Direct and inverse solutions of geodesics on the ellipsoid
//! with application of nested equations". Survey Review 23(176):88–93.
//!
//! The inverse formula computes the geodesic distance and forward/reverse azimuths between two
//! geographic coordinates. The direct formula computes the destination point and reverse azimuth
//! given a starting point, forward azimuth, and distance.
//!
//! # Accuracy
//!
//! The Vincenty formulae are accurate to within 0.5 mm for all points on the ellipsoid except
//! for nearly antipodal points where convergence may fail.
//!
//! # Examples
//!
//! ```
//! use oxigeo_proj::geodesic::wgs84_inverse;
//!
//! let result = wgs84_inverse(51.5074, -0.1278, 48.8566, 2.3522).unwrap();
//! println!("London-Paris: {:.0} m", result.distance_m);
//! ```

#[cfg(not(feature = "std"))]
use alloc::string::{String, ToString};

use core::fmt;

// ─────────────────────────────────────────────────────────────────────────────
// WGS84 constants
// ─────────────────────────────────────────────────────────────────────────────

/// WGS84 semi-major axis in metres.
pub const WGS84_A: f64 = 6_378_137.0;

/// WGS84 semi-minor axis in metres.
pub const WGS84_B: f64 = 6_356_752.314_245_179;

/// WGS84 mean radius used for haversine calculations (metres).
/// Defined as (2a + b) / 3, the authalic mean radius of WGS84.
pub const WGS84_MEAN_RADIUS: f64 = 6_371_008.8;

/// Default maximum iterations for Vincenty convergence.
pub const DEFAULT_MAX_ITER: u32 = 100;

/// Default convergence tolerance for Vincenty (radians).
pub const DEFAULT_TOL: f64 = 1e-12;

// ─────────────────────────────────────────────────────────────────────────────
// Ellipsoid parameters
// ─────────────────────────────────────────────────────────────────────────────

/// Reference ellipsoid parameters used in Vincenty geodesic computations.
///
/// Groups the semi-major axis, semi-minor axis, and convergence controls so that
/// the Vincenty functions stay within the clippy `too_many_arguments` limit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeodesicParams {
    /// Semi-major axis in metres (equatorial radius).
    pub a: f64,
    /// Semi-minor axis in metres (polar radius).
    pub b: f64,
    /// Maximum number of iterations for convergence.
    pub max_iter: u32,
    /// Convergence tolerance in radians.
    pub tol: f64,
}

impl GeodesicParams {
    /// Construct custom ellipsoid parameters with default convergence settings.
    pub fn new(a: f64, b: f64) -> Self {
        Self {
            a,
            b,
            max_iter: DEFAULT_MAX_ITER,
            tol: DEFAULT_TOL,
        }
    }

    /// Construct with explicit convergence controls.
    pub fn with_convergence(a: f64, b: f64, max_iter: u32, tol: f64) -> Self {
        Self {
            a,
            b,
            max_iter,
            tol,
        }
    }

    /// WGS84 ellipsoid with default convergence settings.
    pub fn wgs84() -> Self {
        Self::new(WGS84_A, WGS84_B)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Public result types
// ─────────────────────────────────────────────────────────────────────────────

/// Result of a Vincenty inverse (geodesic distance + azimuths) computation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VincentyResult {
    /// Geodesic distance in metres.
    pub distance_m: f64,
    /// Forward azimuth (bearing from point 1 to point 2), degrees [0, 360).
    pub azimuth_fwd_deg: f64,
    /// Reverse azimuth (bearing from point 2 towards point 1), degrees [0, 360).
    pub azimuth_rev_deg: f64,
    /// Number of iterations needed to reach convergence.
    pub iterations: u32,
}

impl fmt::Display for VincentyResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "distance={:.3} m, fwd_az={:.6}°, rev_az={:.6}°, iters={}",
            self.distance_m, self.azimuth_fwd_deg, self.azimuth_rev_deg, self.iterations
        )
    }
}

/// Result of a Vincenty direct computation (destination + reverse azimuth).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VincentyDirectResult {
    /// Latitude of the destination point in degrees.
    pub lat2_deg: f64,
    /// Longitude of the destination point in degrees.
    pub lon2_deg: f64,
    /// Reverse azimuth at the destination (bearing back toward origin), degrees [0, 360).
    pub azimuth_rev_deg: f64,
}

impl fmt::Display for VincentyDirectResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "lat={:.8}°, lon={:.8}°, rev_az={:.6}°",
            self.lat2_deg, self.lon2_deg, self.azimuth_rev_deg
        )
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Error type
// ─────────────────────────────────────────────────────────────────────────────

/// Errors that can occur during geodesic computations.
#[derive(Debug, Clone, PartialEq)]
pub enum GeodesicError {
    /// Vincenty inverse failed to converge — points are (nearly) antipodal.
    AntipodalPoints,
    /// Input values are out of valid range or otherwise invalid.
    InvalidInput(String),
}

impl fmt::Display for GeodesicError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GeodesicError::AntipodalPoints => write!(
                f,
                "Vincenty inverse failed to converge: points are nearly antipodal"
            ),
            GeodesicError::InvalidInput(msg) => write!(f, "Invalid input: {msg}"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for GeodesicError {}

// ─────────────────────────────────────────────────────────────────────────────
// Internal helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Normalise an azimuth in **radians** to degrees in [0, 360).
#[inline]
fn azi_to_deg(azi_rad: f64) -> f64 {
    let deg = azi_rad.to_degrees();
    (deg % 360.0 + 360.0) % 360.0
}

/// Validate geographic coordinates (degrees).
fn validate_lat_lon(lat: f64, lon: f64, label: &str) -> Result<(), GeodesicError> {
    if !lat.is_finite() || !lon.is_finite() {
        return Err(GeodesicError::InvalidInput(
            #[cfg(feature = "std")]
            format!("{label}: lat/lon must be finite numbers (got lat={lat}, lon={lon})"),
            #[cfg(not(feature = "std"))]
            "lat/lon must be finite numbers".to_string(),
        ));
    }
    if !(-90.0..=90.0).contains(&lat) {
        return Err(GeodesicError::InvalidInput(
            #[cfg(feature = "std")]
            format!("{label}: latitude {lat} is outside [-90, 90]"),
            #[cfg(not(feature = "std"))]
            "latitude out of range [-90, 90]".to_string(),
        ));
    }
    if !(-180.0..=180.0).contains(&lon) {
        return Err(GeodesicError::InvalidInput(
            #[cfg(feature = "std")]
            format!("{label}: longitude {lon} is outside [-180, 180]"),
            #[cfg(not(feature = "std"))]
            "longitude out of range [-180, 180]".to_string(),
        ));
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Vincenty inverse
// ─────────────────────────────────────────────────────────────────────────────

/// Vincenty inverse formula: geodesic distance and azimuths between two (lat, lon) points.
///
/// # Parameters
/// - `lat1_deg`, `lon1_deg` — first point in decimal degrees
/// - `lat2_deg`, `lon2_deg` — second point in decimal degrees
/// - `params` — ellipsoid parameters and convergence controls (see [`GeodesicParams`])
///
/// # Errors
/// Returns [`GeodesicError::AntipodalPoints`] if convergence fails (nearly antipodal case).
/// Returns [`GeodesicError::InvalidInput`] if coordinates are out of valid range.
pub fn vincenty_inverse(
    lat1_deg: f64,
    lon1_deg: f64,
    lat2_deg: f64,
    lon2_deg: f64,
    params: GeodesicParams,
) -> Result<VincentyResult, GeodesicError> {
    validate_lat_lon(lat1_deg, lon1_deg, "point 1")?;
    validate_lat_lon(lat2_deg, lon2_deg, "point 2")?;
    let GeodesicParams {
        a,
        b,
        max_iter,
        tol,
    } = params;

    // Coincident-point short-circuit
    if (lat1_deg - lat2_deg).abs() < f64::EPSILON && (lon1_deg - lon2_deg).abs() < f64::EPSILON {
        return Ok(VincentyResult {
            distance_m: 0.0,
            azimuth_fwd_deg: 0.0,
            azimuth_rev_deg: 0.0,
            iterations: 0,
        });
    }

    let f = (a - b) / a; // flattening

    // Convert to radians
    let phi1 = lat1_deg.to_radians();
    let phi2 = lat2_deg.to_radians();
    let l = (lon2_deg - lon1_deg).to_radians();

    // Reduced latitudes (latitude on the auxiliary sphere)
    let one_minus_f = 1.0 - f;
    let u1 = (one_minus_f * phi1.tan()).atan();
    let u2 = (one_minus_f * phi2.tan()).atan();

    let sin_u1 = u1.sin();
    let cos_u1 = u1.cos();
    let sin_u2 = u2.sin();
    let cos_u2 = u2.cos();

    /// State captured from each Vincenty inverse iteration.
    struct IterState {
        sin_sigma: f64,
        cos_sigma: f64,
        sigma: f64,
        cos2_alpha: f64,
        cos2_sigma_m: f64,
        lambda: f64,
    }

    /// Run one Vincenty inverse iteration given the current λ, returning updated state.
    #[inline]
    fn vincenty_iter_step(
        lambda: f64,
        l: f64,
        f: f64,
        sin_u1: f64,
        cos_u1: f64,
        sin_u2: f64,
        cos_u2: f64,
    ) -> IterState {
        let sin_lambda = lambda.sin();
        let cos_lambda = lambda.cos();

        let term_a = cos_u2 * sin_lambda;
        let term_b = cos_u1 * sin_u2 - sin_u1 * cos_u2 * cos_lambda;
        let sin_sigma = (term_a * term_a + term_b * term_b).sqrt();
        let cos_sigma = sin_u1 * sin_u2 + cos_u1 * cos_u2 * cos_lambda;
        let sigma = sin_sigma.atan2(cos_sigma);

        let sin_alpha = if sin_sigma.abs() < f64::EPSILON {
            0.0
        } else {
            cos_u1 * cos_u2 * sin_lambda / sin_sigma
        };
        let cos2_alpha = 1.0 - sin_alpha * sin_alpha;

        let cos2_sigma_m = if cos2_alpha.abs() < f64::EPSILON {
            0.0
        } else {
            cos_sigma - 2.0 * sin_u1 * sin_u2 / cos2_alpha
        };

        let c = f / 16.0 * cos2_alpha * (4.0 + f * (4.0 - 3.0 * cos2_alpha));
        let new_lambda = l
            + (1.0 - c)
                * f
                * sin_alpha
                * (sigma
                    + c * sin_sigma
                        * (cos2_sigma_m
                            + c * cos_sigma * (-1.0 + 2.0 * cos2_sigma_m * cos2_sigma_m)));

        IterState {
            sin_sigma,
            cos_sigma,
            sigma,
            cos2_alpha,
            cos2_sigma_m,
            lambda: new_lambda,
        }
    }

    // Iterative solution — λ starts at L
    let mut lambda = l;
    let mut iter: u32 = 0;
    let state = loop {
        iter += 1;
        let state = vincenty_iter_step(lambda, l, f, sin_u1, cos_u1, sin_u2, cos_u2);
        let delta = (state.lambda - lambda).abs();
        lambda = state.lambda;

        if delta < tol {
            break state;
        }

        if iter >= max_iter {
            return Err(GeodesicError::AntipodalPoints);
        }
    };

    let sin_sigma = state.sin_sigma;
    let cos_sigma = state.cos_sigma;
    let sigma = state.sigma;
    let cos2_alpha = state.cos2_alpha;
    let cos2_sigma_m = state.cos2_sigma_m;
    lambda = state.lambda;

    // Post-convergence: compute distance
    let u2_sq = cos2_alpha * (a * a - b * b) / (b * b);

    let big_a =
        1.0 + u2_sq / 16384.0 * (4096.0 + u2_sq * (-768.0 + u2_sq * (320.0 - 175.0 * u2_sq)));
    let big_b = u2_sq / 1024.0 * (256.0 + u2_sq * (-128.0 + u2_sq * (74.0 - 47.0 * u2_sq)));

    let cos2_sigma_m_sq = cos2_sigma_m * cos2_sigma_m;
    let sin_sigma_sq = sin_sigma * sin_sigma;

    let delta_sigma = big_b
        * sin_sigma
        * (cos2_sigma_m
            + big_b / 4.0
                * (cos_sigma * (-1.0 + 2.0 * cos2_sigma_m_sq)
                    - big_b / 6.0
                        * cos2_sigma_m
                        * (-3.0 + 4.0 * sin_sigma_sq)
                        * (-3.0 + 4.0 * cos2_sigma_m_sq)));

    let distance = b * big_a * (sigma - delta_sigma);

    // Azimuths
    // alpha1 = forward azimuth at P1 (bearing from P1 toward P2)
    // alpha2 = Vincenty's geodesic azimuth at P2 in the *forward* direction of travel.
    //          The "reverse azimuth" (bearing from P2 back to P1) is alpha2 + 180°.
    let sin_lambda = lambda.sin();
    let cos_lambda = lambda.cos();

    let alpha1 = (cos_u2 * sin_lambda).atan2(cos_u1 * sin_u2 - sin_u1 * cos_u2 * cos_lambda);
    let alpha2_fwd = (cos_u1 * sin_lambda).atan2(-sin_u1 * cos_u2 + cos_u1 * sin_u2 * cos_lambda);

    // Convert alpha2 to the back-bearing (from P2 toward P1)
    let alpha2_back_deg = (azi_to_deg(alpha2_fwd) + 180.0) % 360.0;

    Ok(VincentyResult {
        distance_m: distance,
        azimuth_fwd_deg: azi_to_deg(alpha1),
        azimuth_rev_deg: alpha2_back_deg,
        iterations: iter,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Vincenty direct
// ─────────────────────────────────────────────────────────────────────────────

/// Vincenty direct formula: destination point and reverse azimuth from a starting point,
/// forward azimuth, and distance.
///
/// # Parameters
/// - `lat1_deg`, `lon1_deg` — starting point in decimal degrees
/// - `azimuth_fwd_deg` — forward azimuth (bearing) from the starting point in degrees [0, 360)
/// - `distance_m` — geodesic distance to travel in metres
/// - `params` — ellipsoid parameters and convergence controls (see [`GeodesicParams`])
///
/// # Errors
/// Returns [`GeodesicError::InvalidInput`] if inputs are invalid.
/// Returns [`GeodesicError::AntipodalPoints`] if the direct calculation fails to converge.
pub fn vincenty_direct(
    lat1_deg: f64,
    lon1_deg: f64,
    azimuth_fwd_deg: f64,
    distance_m: f64,
    params: GeodesicParams,
) -> Result<VincentyDirectResult, GeodesicError> {
    validate_lat_lon(lat1_deg, lon1_deg, "starting point")?;
    let GeodesicParams {
        a,
        b,
        max_iter,
        tol,
    } = params;

    if !azimuth_fwd_deg.is_finite() {
        return Err(GeodesicError::InvalidInput(
            "azimuth must be a finite number".to_string(),
        ));
    }
    if !distance_m.is_finite() || distance_m < 0.0 {
        return Err(GeodesicError::InvalidInput(
            "distance must be a non-negative finite number".to_string(),
        ));
    }

    // Zero-distance short-circuit
    if distance_m < f64::EPSILON {
        return Ok(VincentyDirectResult {
            lat2_deg: lat1_deg,
            lon2_deg: lon1_deg,
            azimuth_rev_deg: (azimuth_fwd_deg + 180.0) % 360.0,
        });
    }

    let f = (a - b) / a;

    let phi1 = lat1_deg.to_radians();
    let alpha1 = azimuth_fwd_deg.to_radians();

    let sin_alpha1 = alpha1.sin();
    let cos_alpha1 = alpha1.cos();

    let one_minus_f = 1.0 - f;
    // Reduced latitude of point 1
    let tan_u1 = one_minus_f * phi1.tan();
    let cos_u1 = 1.0 / (1.0 + tan_u1 * tan_u1).sqrt();
    let sin_u1 = tan_u1 * cos_u1;

    // Angular distance on the sphere from equator to point 1
    let sigma1 = tan_u1.atan2(cos_alpha1);

    let sin_alpha = cos_u1 * sin_alpha1;
    let cos2_alpha = 1.0 - sin_alpha * sin_alpha;
    let u2_sq = cos2_alpha * (a * a - b * b) / (b * b);

    let big_a =
        1.0 + u2_sq / 16384.0 * (4096.0 + u2_sq * (-768.0 + u2_sq * (320.0 - 175.0 * u2_sq)));
    let big_b = u2_sq / 1024.0 * (256.0 + u2_sq * (-128.0 + u2_sq * (74.0 - 47.0 * u2_sq)));

    // Initial estimate for σ
    let mut sigma = distance_m / (b * big_a);
    let mut sigma_prev;
    let mut iter: u32 = 0;

    let mut cos2_sigma_m;
    let mut sin_sigma;
    let mut cos_sigma;

    loop {
        iter += 1;
        sigma_prev = sigma;

        cos2_sigma_m = (2.0 * sigma1 + sigma).cos();
        sin_sigma = sigma.sin();
        cos_sigma = sigma.cos();

        let cos2_sigma_m_sq = cos2_sigma_m * cos2_sigma_m;
        let sin_sigma_sq = sin_sigma * sin_sigma;

        let delta_sigma = big_b
            * sin_sigma
            * (cos2_sigma_m
                + big_b / 4.0
                    * (cos_sigma * (-1.0 + 2.0 * cos2_sigma_m_sq)
                        - big_b / 6.0
                            * cos2_sigma_m
                            * (-3.0 + 4.0 * sin_sigma_sq)
                            * (-3.0 + 4.0 * cos2_sigma_m_sq)));

        sigma = distance_m / (b * big_a) + delta_sigma;

        if (sigma - sigma_prev).abs() < tol {
            break;
        }

        if iter >= max_iter {
            return Err(GeodesicError::AntipodalPoints);
        }
    }

    // Recompute final values
    cos2_sigma_m = (2.0 * sigma1 + sigma).cos();
    sin_sigma = sigma.sin();
    cos_sigma = sigma.cos();

    // Destination latitude
    let num = sin_u1 * cos_sigma + cos_u1 * sin_sigma * cos_alpha1;
    let denom = one_minus_f
        * (sin_alpha * sin_alpha + (sin_u1 * sin_sigma - cos_u1 * cos_sigma * cos_alpha1).powi(2))
            .sqrt();
    let phi2 = num.atan2(denom);

    // Longitude difference on the ellipsoid
    let lambda_num = sin_sigma * sin_alpha1;
    let lambda_den = cos_u1 * cos_sigma - sin_u1 * sin_sigma * cos_alpha1;
    let lambda_on_sphere = lambda_num.atan2(lambda_den);

    let cos2_sigma_m_sq = cos2_sigma_m * cos2_sigma_m;
    let c = f / 16.0 * cos2_alpha * (4.0 + f * (4.0 - 3.0 * cos2_alpha));

    let l = lambda_on_sphere
        - (1.0 - c)
            * f
            * sin_alpha
            * (sigma
                + c * sin_sigma * (cos2_sigma_m + c * cos_sigma * (-1.0 + 2.0 * cos2_sigma_m_sq)));

    let lon2 = lon1_deg.to_radians() + l;

    // alpha2: Vincenty geodesic bearing at destination in the forward direction.
    // The "reverse azimuth" (bearing from destination back toward origin) is alpha2 + 180°.
    let alpha2_fwd = sin_alpha.atan2(-sin_u1 * sin_sigma + cos_u1 * cos_sigma * cos_alpha1);
    let azimuth_rev_deg = (azi_to_deg(alpha2_fwd) + 180.0) % 360.0;

    Ok(VincentyDirectResult {
        lat2_deg: phi2.to_degrees(),
        lon2_deg: lon2.to_degrees(),
        azimuth_rev_deg,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Haversine (spherical approximation)
// ─────────────────────────────────────────────────────────────────────────────

/// Haversine spherical distance between two geographic points.
///
/// Fast approximation that ignores ellipsoidal flattening. Suitable for rough
/// distance estimates; errors are up to ±0.5% compared to the geodesic distance.
///
/// # Parameters
/// - `lat1_deg`, `lon1_deg` — first point in decimal degrees
/// - `lat2_deg`, `lon2_deg` — second point in decimal degrees
/// - `radius_m` — sphere radius in metres (use `WGS84_MEAN_RADIUS` for Earth)
///
/// # Returns
/// Distance in metres.
pub fn haversine_distance_m(
    lat1_deg: f64,
    lon1_deg: f64,
    lat2_deg: f64,
    lon2_deg: f64,
    radius_m: f64,
) -> f64 {
    let phi1 = lat1_deg.to_radians();
    let phi2 = lat2_deg.to_radians();
    let delta_phi = (lat2_deg - lat1_deg).to_radians();
    let delta_lambda = (lon2_deg - lon1_deg).to_radians();

    let a = (delta_phi / 2.0).sin().powi(2)
        + phi1.cos() * phi2.cos() * (delta_lambda / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().asin();

    radius_m * c
}

// ─────────────────────────────────────────────────────────────────────────────
// WGS84 convenience wrappers
// ─────────────────────────────────────────────────────────────────────────────

/// Vincenty inverse on the WGS84 ellipsoid (a = 6 378 137 m, b = 6 356 752.314 245 179 m).
///
/// Uses default convergence parameters (100 iterations, tolerance 1e-12).
///
/// # Errors
/// See [`vincenty_inverse`].
pub fn wgs84_inverse(
    lat1_deg: f64,
    lon1_deg: f64,
    lat2_deg: f64,
    lon2_deg: f64,
) -> Result<VincentyResult, GeodesicError> {
    vincenty_inverse(
        lat1_deg,
        lon1_deg,
        lat2_deg,
        lon2_deg,
        GeodesicParams::wgs84(),
    )
}

/// Vincenty direct on the WGS84 ellipsoid.
///
/// Uses default convergence parameters (100 iterations, tolerance 1e-12).
///
/// # Errors
/// See [`vincenty_direct`].
pub fn wgs84_direct(
    lat1_deg: f64,
    lon1_deg: f64,
    azimuth_fwd_deg: f64,
    distance_m: f64,
) -> Result<VincentyDirectResult, GeodesicError> {
    vincenty_direct(
        lat1_deg,
        lon1_deg,
        azimuth_fwd_deg,
        distance_m,
        GeodesicParams::wgs84(),
    )
}

/// Haversine distance on the WGS84 mean radius (6 371 008.8 m).
///
/// # Returns
/// Distance in metres.
pub fn wgs84_haversine_m(lat1_deg: f64, lon1_deg: f64, lat2_deg: f64, lon2_deg: f64) -> f64 {
    haversine_distance_m(lat1_deg, lon1_deg, lat2_deg, lon2_deg, WGS84_MEAN_RADIUS)
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests (in-module)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_coincident_points_zero_distance() {
        let result = wgs84_inverse(51.5, -0.1, 51.5, -0.1).unwrap();
        assert_eq!(result.distance_m, 0.0);
        assert_eq!(result.iterations, 0);
    }

    #[test]
    fn test_azimuth_normalisation() {
        // Normalise a negative radian azimuth
        let azi = azi_to_deg(-core::f64::consts::PI / 4.0);
        assert!((azi - 315.0).abs() < 1e-9);
    }

    #[test]
    fn test_haversine_equatorial() {
        // 1° of longitude on equator using WGS84 mean radius ≈ 111 195 m
        // (Note: using equatorial radius 6 378 137 m gives 111 319 m — different radius)
        let d = haversine_distance_m(0.0, 0.0, 0.0, 1.0, WGS84_MEAN_RADIUS);
        assert!((d - 111_195.0).abs() < 10.0);
    }

    #[test]
    fn test_validate_lat_lon_rejects_bad_lat() {
        let err = validate_lat_lon(91.0, 0.0, "pt");
        assert!(err.is_err());
    }

    #[test]
    fn test_validate_lat_lon_rejects_bad_lon() {
        let err = validate_lat_lon(0.0, 181.0, "pt");
        assert!(err.is_err());
    }

    #[test]
    fn test_wgs84_constants_consistent() {
        // f = (a - b) / a must be close to 1/298.257223563
        let f = (WGS84_A - WGS84_B) / WGS84_A;
        let expected_f = 1.0 / 298.257_223_563;
        assert!((f - expected_f).abs() < 1e-12);
    }

    #[test]
    fn test_direct_zero_distance() {
        let result = wgs84_direct(48.0, 2.0, 90.0, 0.0).unwrap();
        assert!((result.lat2_deg - 48.0).abs() < 1e-10);
        assert!((result.lon2_deg - 2.0).abs() < 1e-10);
    }
}
