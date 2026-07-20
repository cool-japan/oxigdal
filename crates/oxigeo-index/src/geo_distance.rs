//! Geographic distance computation: haversine (spherical) and Vincenty (geodetic WGS84).
//!
//! All coordinate inputs are in **degrees** (latitude, longitude).
//! All distances are in **metres**.
//!
//! # Overview
//!
//! * [`haversine_m`] — fast spherical distance using the mean WGS84 radius.
//! * [`haversine_m_with_radius`] — same formula with a configurable sphere radius.
//! * [`vincenty_inverse_wgs84`] — accurate geodesic distance on the WGS84 ellipsoid,
//!   including forward and reverse azimuths.
//! * [`geo_nearest_k`] — linear-scan k-NN over a `(GeoPoint, T)` slice.
//! * [`geo_within_radius`] — filter a slice to points within a haversine radius.
//! * [`geo_bbox_extent_m`] — convert a geographic bounding box to approximate
//!   metric extents `(width_m, height_m)`.
//!
//! # Algorithm references
//!
//! * Haversine: standard spherical-trigonometry formula; see
//!   R. W. Sinnott, "Virtues of the Haversine", Sky and Telescope 68(2):158 (1984).
//! * Vincenty inverse: T. Vincenty, "Direct and Inverse Solutions of Geodesics
//!   on the Ellipsoid with Application of Nested Equations", Survey Review 23(176):
//!   88-93 (1975).

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use core::f64::consts::PI;

// ---------------------------------------------------------------------------
// WGS84 ellipsoid constants
// ---------------------------------------------------------------------------

/// WGS84 semi-major axis (equatorial radius) in metres.
pub const WGS84_A: f64 = 6_378_137.0;

/// WGS84 semi-minor axis (polar radius) in metres.
///
/// Derived from `a` and inverse flattening: `b = a * (1 - 1/f)`.
pub const WGS84_B: f64 = 6_356_752.314_245_179;

/// WGS84 inverse flattening `1/f`.
pub const WGS84_INV_F: f64 = 298.257_223_563;

/// WGS84 mean radius in metres, used by the haversine formula.
///
/// Computed as `(2a + b) / 3` per the IUGG definition.
pub const WGS84_MEAN_RADIUS_M: f64 = 6_371_008.8;

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Convert degrees to radians.
#[inline]
fn deg_to_rad(deg: f64) -> f64 {
    deg * (PI / 180.0)
}

/// Convert radians to degrees.
#[inline]
fn rad_to_deg(rad: f64) -> f64 {
    rad * (180.0 / PI)
}

/// Normalise an azimuth in radians to `[0, 2π)`.
#[inline]
fn normalise_azimuth_rad(az: f64) -> f64 {
    let two_pi = 2.0 * PI;
    let az = az % two_pi;
    if az < 0.0 { az + two_pi } else { az }
}

// ---------------------------------------------------------------------------
// Haversine distance
// ---------------------------------------------------------------------------

/// Compute spherical haversine distance between two geographic points.
///
/// Inputs are latitude and longitude in **degrees**.
/// Returns the great-circle distance in **metres** using `radius_m` as the
/// sphere radius.
///
/// The haversine formula is numerically well-conditioned for all distances,
/// including near-zero and near-antipodal pairs.
///
/// # Formula
///
/// ```text
/// Δlat = (lat2 - lat1) in radians
/// Δlon = (lon2 - lon1) in radians
/// a = sin²(Δlat/2) + cos(lat1) * cos(lat2) * sin²(Δlon/2)
/// c = 2 * atan2(√a, √(1−a))
/// d = radius_m * c
/// ```
pub fn haversine_m_with_radius(
    lat1_deg: f64,
    lon1_deg: f64,
    lat2_deg: f64,
    lon2_deg: f64,
    radius_m: f64,
) -> f64 {
    let lat1 = deg_to_rad(lat1_deg);
    let lat2 = deg_to_rad(lat2_deg);
    let d_lat = deg_to_rad(lat2_deg - lat1_deg);
    let d_lon = deg_to_rad(lon2_deg - lon1_deg);

    let sin_d_lat_half = (d_lat * 0.5).sin();
    let sin_d_lon_half = (d_lon * 0.5).sin();

    let a =
        sin_d_lat_half * sin_d_lat_half + lat1.cos() * lat2.cos() * sin_d_lon_half * sin_d_lon_half;

    // Clamp to [0, 1] to guard against floating-point drift slightly outside [0,1].
    let a = a.clamp(0.0, 1.0);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());

    radius_m * c
}

/// Compute spherical haversine distance between two geographic points.
///
/// Inputs are latitude and longitude in **degrees**.
/// Returns the great-circle distance in **metres**, using [`WGS84_MEAN_RADIUS_M`].
///
/// For most geographic applications this is accurate to within ±0.5% of the
/// true geodesic.  For centimetre-level accuracy use [`vincenty_inverse_wgs84`].
#[inline]
pub fn haversine_m(lat1_deg: f64, lon1_deg: f64, lat2_deg: f64, lon2_deg: f64) -> f64 {
    haversine_m_with_radius(lat1_deg, lon1_deg, lat2_deg, lon2_deg, WGS84_MEAN_RADIUS_M)
}

// ---------------------------------------------------------------------------
// Vincenty inverse solution (WGS84)
// ---------------------------------------------------------------------------

/// Result of a Vincenty inverse geodesic computation.
///
/// All angular values are in **degrees**, the distance in **metres**.
#[derive(Debug, Clone, PartialEq)]
pub struct VincentyGeoResult {
    /// Geodesic distance between the two points in metres.
    pub distance_m: f64,
    /// Forward azimuth at point 1 → point 2, in degrees `[0°, 360°)`.
    pub azimuth_fwd_deg: f64,
    /// Reverse azimuth at point 2 → point 1, in degrees `[0°, 360°)`.
    pub azimuth_rev_deg: f64,
}

/// Maximum iteration count for the Vincenty convergence loop.
const VINCENTY_MAX_ITER: usize = 100;

/// Convergence threshold for λ (longitude on the auxiliary sphere).
const VINCENTY_TOL: f64 = 1e-12;

/// Vincenty inverse solution on the WGS84 ellipsoid.
///
/// Computes the geodesic (shortest surface path) distance between two points
/// given in geographic coordinates (latitude, longitude, degrees).  Also
/// returns the forward azimuth (at point 1) and reverse azimuth (at point 2).
///
/// # Returns
///
/// `Some(VincentyGeoResult)` on convergence, or `None` if the two points are
/// nearly antipodal (the iterative loop fails to converge within
/// `VINCENTY_MAX_ITER` iterations).
///
/// # Special cases
///
/// * Coincident points (distance ≈ 0): the loop converges immediately;
///   azimuths are 0°.
/// * Points along the same meridian or equator: handled correctly.
///
/// # Algorithm reference
///
/// T. Vincenty (1975). "Direct and Inverse Solutions of Geodesics on the
/// Ellipsoid with Application of Nested Equations."  Survey Review 23(176):
/// 88–93.
pub fn vincenty_inverse_wgs84(
    lat1_deg: f64,
    lon1_deg: f64,
    lat2_deg: f64,
    lon2_deg: f64,
) -> Option<VincentyGeoResult> {
    let a = WGS84_A;
    let b = WGS84_B;
    let f = 1.0 / WGS84_INV_F;

    let lat1 = deg_to_rad(lat1_deg);
    let lat2 = deg_to_rad(lat2_deg);
    let lon1 = deg_to_rad(lon1_deg);
    let lon2 = deg_to_rad(lon2_deg);

    // Reduced latitudes on the auxiliary sphere.
    let tan_u1 = (1.0 - f) * lat1.tan();
    let tan_u2 = (1.0 - f) * lat2.tan();

    let cos_u1 = 1.0 / (1.0 + tan_u1 * tan_u1).sqrt();
    let sin_u1 = tan_u1 * cos_u1;
    let cos_u2 = 1.0 / (1.0 + tan_u2 * tan_u2).sqrt();
    let sin_u2 = tan_u2 * cos_u2;

    // Difference in longitude.
    let l_val = lon2 - lon1;

    // Initialise λ to L.
    let mut lambda = l_val;

    // Variables updated each iteration and used after the loop.
    let mut sin_sigma = 0.0_f64;
    let mut cos_sigma = 0.0_f64;
    let mut sigma = 0.0_f64;
    let mut cos_sq_alpha = 0.0_f64;
    let mut cos_2sigma_m = 0.0_f64;

    let mut converged = false;

    for _ in 0..VINCENTY_MAX_ITER {
        let sin_lambda = lambda.sin();
        let cos_lambda = lambda.cos();

        // sin σ = sqrt((cosU2·sinλ)² + (cosU1·sinU2 − sinU1·cosU2·cosλ)²)
        let term1 = cos_u2 * sin_lambda;
        let term2 = cos_u1 * sin_u2 - sin_u1 * cos_u2 * cos_lambda;
        sin_sigma = (term1 * term1 + term2 * term2).sqrt();

        // cos σ = sinU1·sinU2 + cosU1·cosU2·cosλ
        cos_sigma = sin_u1 * sin_u2 + cos_u1 * cos_u2 * cos_lambda;

        // σ = atan2(sin σ, cos σ)
        sigma = sin_sigma.atan2(cos_sigma);

        if sin_sigma.abs() < 1e-15 {
            // Coincident points (distance ≈ 0): λ has converged to L already;
            // the outer state variables have been initialised to zeros, which
            // produce distance ≈ 0 after the series expansion below.
            cos_sq_alpha = 1.0;
            cos_2sigma_m = 1.0;
            let lambda_prev = lambda;
            lambda = l_val;
            if (lambda - lambda_prev).abs() < VINCENTY_TOL {
                converged = true;
                break;
            }
            continue;
        }

        // sin α = cosU1·cosU2·sinλ / sin σ
        let sin_alpha = cos_u1 * cos_u2 * lambda.sin() / sin_sigma;

        // cos² α = 1 − sin² α
        cos_sq_alpha = 1.0 - sin_alpha * sin_alpha;

        // cos 2σ_m = cos σ − 2·sinU1·sinU2 / cos²α
        // For equatorial lines (cos²α ≈ 0), set cos 2σ_m = 0.
        cos_2sigma_m = if cos_sq_alpha.abs() > 1e-15 {
            cos_sigma - 2.0 * sin_u1 * sin_u2 / cos_sq_alpha
        } else {
            0.0
        };

        // C = f/16 · cos²α · (4 + f·(4 − 3·cos²α))
        let cap_c = f / 16.0 * cos_sq_alpha * (4.0 + f * (4.0 - 3.0 * cos_sq_alpha));

        let lambda_prev = lambda;

        // λ′ = L + (1−C)·f·sinα·(σ + C·sinσ·(cos2σm + C·cosσ·(−1 + 2·cos²2σm)))
        let cos_2sigma_m_sq = cos_2sigma_m * cos_2sigma_m;
        lambda = l_val
            + (1.0 - cap_c)
                * f
                * sin_alpha
                * (sigma
                    + cap_c
                        * sin_sigma
                        * (cos_2sigma_m + cap_c * cos_sigma * (-1.0 + 2.0 * cos_2sigma_m_sq)));

        if (lambda - lambda_prev).abs() < VINCENTY_TOL {
            converged = true;
            break;
        }
    }

    if !converged {
        // Antipodal case: iteration did not converge.
        return None;
    }

    // u² = cos²α · (a² − b²) / b²
    let u_sq = cos_sq_alpha * (a * a - b * b) / (b * b);

    // A_v = 1 + u²/16384 · (4096 + u²·(−768 + u²·(320 − 175·u²)))
    let cap_a_v = 1.0 + u_sq / 16384.0 * (4096.0 + u_sq * (-768.0 + u_sq * (320.0 - 175.0 * u_sq)));

    // B_v = u²/1024 · (256 + u²·(−128 + u²·(74 − 47·u²)))
    let cap_b_v = u_sq / 1024.0 * (256.0 + u_sq * (-128.0 + u_sq * (74.0 - 47.0 * u_sq)));

    // Δσ = B_v·sinσ·(cos2σm + B_v/4·(cosσ·(−1+2·cos²2σm) − B_v/6·cos2σm·(−3+4·sin²σ)·(−3+4·cos²2σm)))
    let cos_2sigma_m_sq = cos_2sigma_m * cos_2sigma_m;
    let sin_sigma_sq = sin_sigma * sin_sigma;
    let delta_sigma = cap_b_v
        * sin_sigma
        * (cos_2sigma_m
            + cap_b_v / 4.0
                * (cos_sigma * (-1.0 + 2.0 * cos_2sigma_m_sq)
                    - cap_b_v / 6.0
                        * cos_2sigma_m
                        * (-3.0 + 4.0 * sin_sigma_sq)
                        * (-3.0 + 4.0 * cos_2sigma_m_sq)));

    // Geodesic distance s = b · A_v · (σ − Δσ)
    let s = b * cap_a_v * (sigma - delta_sigma);

    // Forward azimuth α₁ = atan2(cosU2·sinλ, cosU1·sinU2 − sinU1·cosU2·cosλ)
    let sin_lambda = lambda.sin();
    let cos_lambda = lambda.cos();
    let alpha1_rad = (cos_u2 * sin_lambda).atan2(cos_u1 * sin_u2 - sin_u1 * cos_u2 * cos_lambda);

    // Reverse azimuth α₂ = atan2(cosU1·sinλ, −sinU1·cosU2 + cosU1·sinU2·cosλ)
    let alpha2_rad = (cos_u1 * sin_lambda).atan2(-sin_u1 * cos_u2 + cos_u1 * sin_u2 * cos_lambda);

    Some(VincentyGeoResult {
        distance_m: s.abs(),
        azimuth_fwd_deg: rad_to_deg(normalise_azimuth_rad(alpha1_rad)),
        azimuth_rev_deg: rad_to_deg(normalise_azimuth_rad(alpha2_rad)),
    })
}

// ---------------------------------------------------------------------------
// Geographic point and nearest-neighbour types
// ---------------------------------------------------------------------------

/// A point defined by geographic coordinates in degrees.
#[derive(Debug, Clone, PartialEq)]
pub struct GeoPoint {
    /// Latitude in degrees (−90 to +90).
    pub lat_deg: f64,
    /// Longitude in degrees (−180 to +180).
    pub lon_deg: f64,
}

impl GeoPoint {
    /// Construct a [`GeoPoint`] from latitude and longitude in degrees.
    #[inline]
    pub fn new(lat_deg: f64, lon_deg: f64) -> Self {
        GeoPoint { lat_deg, lon_deg }
    }
}

/// One entry in the result list returned by geographic k-NN queries.
#[derive(Debug)]
pub struct GeoNearestResult<T> {
    /// The user value associated with this point.
    pub value: T,
    /// Haversine distance from the query point in metres.
    pub distance_m: f64,
    /// Latitude of the found point in degrees.
    pub lat_deg: f64,
    /// Longitude of the found point in degrees.
    pub lon_deg: f64,
}

// ---------------------------------------------------------------------------
// Geographic k-NN (linear scan)
// ---------------------------------------------------------------------------

/// Find the *k* nearest geographic points to a query location using haversine distance.
///
/// Performs a **linear scan** over `points` — O(n) in the number of input points.
/// This is sufficient for small to medium datasets (up to tens of thousands of
/// points); for larger datasets consider building a projected spatial index after
/// reprojection.
///
/// # Parameters
///
/// * `points` — slice of `(GeoPoint, T)` pairs to search.
/// * `query_lat_deg`, `query_lon_deg` — query location in degrees.
/// * `k` — maximum number of results to return.
///
/// # Returns
///
/// A `Vec` of up to `k` [`GeoNearestResult`] entries, sorted by ascending
/// `distance_m`.  If `points` has fewer than `k` elements, all points are
/// returned.
pub fn geo_nearest_k<T: Clone>(
    points: &[(GeoPoint, T)],
    query_lat_deg: f64,
    query_lon_deg: f64,
    k: usize,
) -> Vec<GeoNearestResult<T>> {
    if k == 0 || points.is_empty() {
        return Vec::new();
    }

    // Compute haversine distance to every point.
    let mut scored: Vec<(f64, usize)> = points
        .iter()
        .enumerate()
        .map(|(i, (pt, _))| {
            let d = haversine_m(pt.lat_deg, pt.lon_deg, query_lat_deg, query_lon_deg);
            (d, i)
        })
        .collect();

    // Sort ascending by distance; use index as tie-breaker for determinism.
    scored.sort_unstable_by(|(da, ia), (db, ib)| {
        da.partial_cmp(db)
            .unwrap_or(core::cmp::Ordering::Equal)
            .then(ia.cmp(ib))
    });

    // Collect up to k results.
    scored
        .into_iter()
        .take(k)
        .map(|(d, i)| {
            let (pt, val) = &points[i];
            GeoNearestResult {
                value: val.clone(),
                distance_m: d,
                lat_deg: pt.lat_deg,
                lon_deg: pt.lon_deg,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Geographic radius filter
// ---------------------------------------------------------------------------

/// Return all points in `points` whose haversine distance to the query is ≤
/// `radius_m` metres, sorted ascending by distance.
///
/// # Parameters
///
/// * `points` — slice of `(GeoPoint, T)` pairs to search.
/// * `query_lat_deg`, `query_lon_deg` — query location in degrees.
/// * `radius_m` — search radius in metres.
///
/// # Returns
///
/// A `Vec` of [`GeoNearestResult`] for every point within the radius, sorted by
/// ascending `distance_m`.
pub fn geo_within_radius<T: Clone>(
    points: &[(GeoPoint, T)],
    query_lat_deg: f64,
    query_lon_deg: f64,
    radius_m: f64,
) -> Vec<GeoNearestResult<T>> {
    if points.is_empty() {
        return Vec::new();
    }

    let mut results: Vec<GeoNearestResult<T>> = points
        .iter()
        .filter_map(|(pt, val)| {
            let d = haversine_m(pt.lat_deg, pt.lon_deg, query_lat_deg, query_lon_deg);
            if d <= radius_m {
                Some(GeoNearestResult {
                    value: val.clone(),
                    distance_m: d,
                    lat_deg: pt.lat_deg,
                    lon_deg: pt.lon_deg,
                })
            } else {
                None
            }
        })
        .collect();

    results.sort_unstable_by(|a, b| {
        a.distance_m
            .partial_cmp(&b.distance_m)
            .unwrap_or(core::cmp::Ordering::Equal)
    });

    results
}

// ---------------------------------------------------------------------------
// Geographic bounding-box extent estimator
// ---------------------------------------------------------------------------

/// Estimate the metric width and height of a geographic bounding box.
///
/// Given a bounding box in degrees, compute approximate horizontal and vertical
/// extents in **metres** using haversine distances along the bbox edges.
///
/// * `width_m`  — distance from `(centre_lat, min_lon)` to `(centre_lat, max_lon)`
/// * `height_m` — distance from `(min_lat, centre_lon)` to `(max_lat, centre_lon)`
///
/// This is an approximation: the true metric extent varies with latitude and
/// the particular geodetic model.  For small bboxes (< a few degrees) it is
/// accurate to well within 1%.
///
/// # Returns
///
/// `(width_m, height_m)` in metres.
pub fn geo_bbox_extent_m(min_lat: f64, min_lon: f64, max_lat: f64, max_lon: f64) -> (f64, f64) {
    let centre_lat = (min_lat + max_lat) * 0.5;
    let centre_lon = (min_lon + max_lon) * 0.5;

    // Horizontal extent: along the latitude bisector.
    let width_m = haversine_m(centre_lat, min_lon, centre_lat, max_lon);

    // Vertical extent: along the longitude bisector.
    let height_m = haversine_m(min_lat, centre_lon, max_lat, centre_lon);

    (width_m, height_m)
}

// ---------------------------------------------------------------------------
// Unit tests (inline)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn haversine_coincident() {
        let d = haversine_m(48.8566, 2.3522, 48.8566, 2.3522);
        assert!(d.abs() < 1e-6, "coincident points should give 0 m, got {d}");
    }

    #[test]
    fn haversine_with_radius_matches_default_at_mean_radius() {
        let d1 = haversine_m(0.0, 0.0, 1.0, 0.0);
        let d2 = haversine_m_with_radius(0.0, 0.0, 1.0, 0.0, WGS84_MEAN_RADIUS_M);
        assert!(
            (d1 - d2).abs() < 1e-9,
            "haversine_m and haversine_m_with_radius(..mean_radius) must agree"
        );
    }

    #[test]
    fn vincenty_coincident_near_zero() {
        let result = vincenty_inverse_wgs84(0.0, 0.0, 0.0, 0.0);
        let r = result.expect("coincident should converge");
        assert!(
            r.distance_m < 1e-3,
            "coincident distance should be ~0 m, got {}",
            r.distance_m
        );
    }

    #[test]
    fn geopoint_new_roundtrip() {
        let p = GeoPoint::new(35.6762, 139.6503);
        assert_eq!(p.lat_deg, 35.6762);
        assert_eq!(p.lon_deg, 139.6503);
    }

    #[test]
    fn geo_nearest_k_empty_returns_empty() {
        let empty: Vec<(GeoPoint, &str)> = Vec::new();
        let res = geo_nearest_k(&empty, 0.0, 0.0, 5);
        assert!(res.is_empty());
    }

    #[test]
    fn geo_within_radius_empty_returns_empty() {
        let empty: Vec<(GeoPoint, i32)> = Vec::new();
        let res = geo_within_radius(&empty, 0.0, 0.0, 1_000.0);
        assert!(res.is_empty());
    }

    #[test]
    fn geo_bbox_extent_equatorial_one_degree() {
        // At the equator, 1° of lat or lon ≈ 111,195 m.
        let (w, h) = geo_bbox_extent_m(0.0, 0.0, 1.0, 1.0);
        let expected = 111_195.0;
        assert!(
            (w - expected).abs() < 1000.0,
            "width at equator should be ~111,195 m, got {w}"
        );
        assert!(
            (h - expected).abs() < 1000.0,
            "height at equator should be ~111,195 m, got {h}"
        );
    }
}
