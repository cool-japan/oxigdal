//! Length calculation for linear geometries
//!
//! This module provides functions for computing lengths of linestrings using
//! different methods appropriate for planar and geodetic coordinates.
//!
//! # Length Methods
//!
//! - **Planar**: Fast length calculation assuming Cartesian coordinates (projected data)
//! - **Geodetic (Haversine)**: Great-circle length on sphere (good approximation for lat/lon)
//! - **Geodetic (Vincenty)**: Precise ellipsoidal length on WGS84 (most accurate for lat/lon)
//!
//! # Examples
//!
//! ```
//! # use oxigdal_algorithms::error::Result;
//! use oxigdal_algorithms::vector::{Coordinate, LineString, length_linestring, LengthMethod};
//!
//! # fn main() -> Result<()> {
//! let coords = vec![
//!     Coordinate::new_2d(0.0, 0.0),
//!     Coordinate::new_2d(3.0, 0.0),
//!     Coordinate::new_2d(3.0, 4.0),
//! ];
//! let linestring = LineString::new(coords)?;
//! let len = length_linestring(&linestring, LengthMethod::Planar)?;
//! // Planar length = 3.0 + 4.0 = 7.0 units
//! # Ok(())
//! # }
//! ```

use crate::error::{AlgorithmError, Result};
use oxigdal_core::vector::{Coordinate, Geometry, LineString, MultiLineString};

#[cfg(feature = "std")]
use std::f64::consts::PI;

#[cfg(not(feature = "std"))]
use core::f64::consts::PI;

/// WGS84 ellipsoid semi-major axis (meters)
const WGS84_A: f64 = 6_378_137.0;

/// WGS84 ellipsoid semi-minor axis (meters)
const WGS84_B: f64 = 6_356_752.314_245;

/// WGS84 ellipsoid flattening
const WGS84_F: f64 = (WGS84_A - WGS84_B) / WGS84_A;

/// Maximum iterations for Vincenty's formula
const VINCENTY_MAX_ITER: usize = 200;

/// Convergence threshold for Vincenty's formula
const VINCENTY_THRESHOLD: f64 = 1e-12;

/// Length calculation method
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LengthMethod {
    /// Planar length using Cartesian coordinates (fast, for projected data)
    Planar,
    /// Geodetic length using Haversine formula (spherical Earth approximation)
    Haversine,
    /// Geodetic length using Vincenty's formula (accurate ellipsoidal distance)
    Vincenty,
}

/// Computes the length of a geometry
///
/// # Arguments
///
/// * `geometry` - Input geometry
/// * `method` - Length calculation method
///
/// # Returns
///
/// Length in appropriate units (meters for geodetic, coordinate units for planar)
///
/// # Errors
///
/// Returns error if geometry is not linear or invalid
pub fn length(geometry: &Geometry, method: LengthMethod) -> Result<f64> {
    match geometry {
        Geometry::LineString(ls) => length_linestring(ls, method),
        Geometry::MultiLineString(mls) => length_multilinestring(mls, method),
        _ => Err(AlgorithmError::GeometryError {
            message: "length calculation requires LineString or MultiLineString geometry"
                .to_string(),
        }),
    }
}

/// Computes the length of a linestring
///
/// # Arguments
///
/// * `linestring` - Input linestring
/// * `method` - Length calculation method
///
/// # Returns
///
/// Total length of all segments
///
/// # Errors
///
/// Returns error if linestring is invalid or has fewer than 2 points
pub fn length_linestring(linestring: &LineString, method: LengthMethod) -> Result<f64> {
    if linestring.coords.len() < 2 {
        return Err(AlgorithmError::InsufficientData {
            operation: "length_linestring",
            message: "linestring must have at least 2 coordinates".to_string(),
        });
    }

    match method {
        LengthMethod::Planar => Ok(length_linestring_planar(linestring)),
        LengthMethod::Haversine => length_linestring_haversine(linestring),
        LengthMethod::Vincenty => length_linestring_vincenty(linestring),
    }
}

/// Computes the length of a multilinestring
///
/// # Arguments
///
/// * `multilinestring` - Input multilinestring
/// * `method` - Length calculation method
///
/// # Returns
///
/// Total length of all linestrings
///
/// # Errors
///
/// Returns error if any linestring is invalid
pub fn length_multilinestring(
    multilinestring: &MultiLineString,
    method: LengthMethod,
) -> Result<f64> {
    if multilinestring.line_strings.is_empty() {
        return Ok(0.0);
    }

    let mut total = 0.0;
    for linestring in &multilinestring.line_strings {
        total += length_linestring(linestring, method)?;
    }

    Ok(total)
}

/// Computes 3D length of a linestring (Euclidean distance including Z coordinate)
///
/// # Arguments
///
/// * `linestring` - Input linestring with Z coordinates
///
/// # Returns
///
/// Total 3D length
///
/// # Errors
///
/// Returns error if linestring is invalid or has fewer than 2 points
pub fn length_linestring_3d(linestring: &LineString) -> Result<f64> {
    if linestring.coords.len() < 2 {
        return Err(AlgorithmError::InsufficientData {
            operation: "length_linestring_3d",
            message: "linestring must have at least 2 coordinates".to_string(),
        });
    }

    let mut total_length = 0.0;

    for i in 0..(linestring.coords.len() - 1) {
        let p1 = &linestring.coords[i];
        let p2 = &linestring.coords[i + 1];

        let dx = p2.x - p1.x;
        let dy = p2.y - p1.y;

        let segment_length = if let (Some(z1), Some(z2)) = (p1.z, p2.z) {
            let dz = z2 - z1;
            (dx * dx + dy * dy + dz * dz).sqrt()
        } else {
            (dx * dx + dy * dy).sqrt()
        };

        total_length += segment_length;
    }

    Ok(total_length)
}

/// Computes planar length of a linestring (fast, Cartesian)
fn length_linestring_planar(linestring: &LineString) -> f64 {
    let mut total_length = 0.0;

    for i in 0..(linestring.coords.len() - 1) {
        let p1 = &linestring.coords[i];
        let p2 = &linestring.coords[i + 1];

        let dx = p2.x - p1.x;
        let dy = p2.y - p1.y;
        let segment_length = (dx * dx + dy * dy).sqrt();

        total_length += segment_length;
    }

    total_length
}

/// Computes geodetic length using Haversine formula
fn length_linestring_haversine(linestring: &LineString) -> Result<f64> {
    let mut total_length = 0.0;

    for i in 0..(linestring.coords.len() - 1) {
        let p1 = &linestring.coords[i];
        let p2 = &linestring.coords[i + 1];

        let segment_length = haversine_distance(p1, p2)?;
        total_length += segment_length;
    }

    Ok(total_length)
}

/// Computes geodetic length using Vincenty's formula
fn length_linestring_vincenty(linestring: &LineString) -> Result<f64> {
    let mut total_length = 0.0;

    for i in 0..(linestring.coords.len() - 1) {
        let p1 = &linestring.coords[i];
        let p2 = &linestring.coords[i + 1];

        let segment_length = vincenty_distance(p1, p2)?;
        total_length += segment_length;
    }

    Ok(total_length)
}

/// Computes Haversine distance between two coordinates (assumes lat/lon in degrees)
fn haversine_distance(c1: &Coordinate, c2: &Coordinate) -> Result<f64> {
    let lat1 = c1.y * PI / 180.0;
    let lon1 = c1.x * PI / 180.0;
    let lat2 = c2.y * PI / 180.0;
    let lon2 = c2.x * PI / 180.0;

    // Validate latitude range
    if lat1.abs() > PI / 2.0 || lat2.abs() > PI / 2.0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "latitude",
            message: "latitude must be between -90 and 90 degrees".to_string(),
        });
    }

    let dlat = lat2 - lat1;
    let dlon = lon2 - lon1;

    let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);

    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());

    // Use mean Earth radius
    let radius = (WGS84_A + WGS84_B) / 2.0;
    Ok(radius * c)
}

/// Computes Vincenty distance between two coordinates (assumes lat/lon in degrees)
///
/// Implementation of Vincenty's inverse formula for WGS84 ellipsoid.
fn vincenty_distance(c1: &Coordinate, c2: &Coordinate) -> Result<f64> {
    let lat1 = c1.y * PI / 180.0;
    let lon1 = c1.x * PI / 180.0;
    let lat2 = c2.y * PI / 180.0;
    let lon2 = c2.x * PI / 180.0;

    // Validate latitude range
    if lat1.abs() > PI / 2.0 || lat2.abs() > PI / 2.0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "latitude",
            message: "latitude must be between -90 and 90 degrees".to_string(),
        });
    }

    let l = lon2 - lon1;

    let u1 = ((1.0 - WGS84_F) * lat1.tan()).atan();
    let u2 = ((1.0 - WGS84_F) * lat2.tan()).atan();

    let sin_u1 = u1.sin();
    let cos_u1 = u1.cos();
    let sin_u2 = u2.sin();
    let cos_u2 = u2.cos();

    let mut lambda = l;
    let mut lambda_prev;
    let mut iter_count = 0;

    let (sin_sigma, cos_sigma, sigma, cos_sq_alpha, cos_2sigma_m);

    loop {
        let sin_lambda = lambda.sin();
        let cos_lambda = lambda.cos();

        let sin_sigma_temp = ((cos_u2 * sin_lambda).powi(2)
            + (cos_u1 * sin_u2 - sin_u1 * cos_u2 * cos_lambda).powi(2))
        .sqrt();

        if sin_sigma_temp.abs() < f64::EPSILON {
            // Coincident points
            return Ok(0.0);
        }

        let cos_sigma_temp = sin_u1 * sin_u2 + cos_u1 * cos_u2 * cos_lambda;
        let sigma_temp = sin_sigma_temp.atan2(cos_sigma_temp);

        let sin_alpha_temp = cos_u1 * cos_u2 * sin_lambda / sin_sigma_temp;
        let cos_sq_alpha_temp = 1.0 - sin_alpha_temp * sin_alpha_temp;

        let cos_2sigma_m_temp = if cos_sq_alpha_temp.abs() < f64::EPSILON {
            0.0
        } else {
            cos_sigma_temp - 2.0 * sin_u1 * sin_u2 / cos_sq_alpha_temp
        };

        let c =
            WGS84_F / 16.0 * cos_sq_alpha_temp * (4.0 + WGS84_F * (4.0 - 3.0 * cos_sq_alpha_temp));

        lambda_prev = lambda;
        lambda = l
            + (1.0 - c)
                * WGS84_F
                * sin_alpha_temp
                * (sigma_temp
                    + c * sin_sigma_temp
                        * (cos_2sigma_m_temp
                            + c * cos_sigma_temp
                                * (-1.0 + 2.0 * cos_2sigma_m_temp * cos_2sigma_m_temp)));

        iter_count += 1;
        if (lambda - lambda_prev).abs() < VINCENTY_THRESHOLD || iter_count >= VINCENTY_MAX_ITER {
            sin_sigma = sin_sigma_temp;
            cos_sigma = cos_sigma_temp;
            sigma = sigma_temp;
            cos_sq_alpha = cos_sq_alpha_temp;
            cos_2sigma_m = cos_2sigma_m_temp;
            break;
        }
    }

    if iter_count >= VINCENTY_MAX_ITER {
        return Err(AlgorithmError::NumericalError {
            operation: "vincenty_distance",
            message: "failed to converge".to_string(),
        });
    }

    let u_sq = cos_sq_alpha * (WGS84_A * WGS84_A - WGS84_B * WGS84_B) / (WGS84_B * WGS84_B);
    let a = 1.0 + u_sq / 16384.0 * (4096.0 + u_sq * (-768.0 + u_sq * (320.0 - 175.0 * u_sq)));
    let b = u_sq / 1024.0 * (256.0 + u_sq * (-128.0 + u_sq * (74.0 - 47.0 * u_sq)));

    let delta_sigma = b
        * sin_sigma
        * (cos_2sigma_m
            + b / 4.0
                * (cos_sigma * (-1.0 + 2.0 * cos_2sigma_m * cos_2sigma_m)
                    - b / 6.0
                        * cos_2sigma_m
                        * (-3.0 + 4.0 * sin_sigma * sin_sigma)
                        * (-3.0 + 4.0 * cos_2sigma_m * cos_2sigma_m)));

    let distance = WGS84_B * a * (sigma - delta_sigma);

    Ok(distance)
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    fn create_linestring_2d() -> Result<LineString> {
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(3.0, 0.0),
            Coordinate::new_2d(3.0, 4.0),
        ];
        LineString::new(coords).map_err(AlgorithmError::Core)
    }

    fn create_linestring_3d() -> Result<LineString> {
        let coords = vec![
            Coordinate::new_3d(0.0, 0.0, 0.0),
            Coordinate::new_3d(3.0, 0.0, 0.0),
            Coordinate::new_3d(3.0, 4.0, 0.0),
            Coordinate::new_3d(3.0, 4.0, 5.0),
        ];
        LineString::new(coords).map_err(AlgorithmError::Core)
    }

    #[test]
    fn test_length_linestring_planar() {
        let ls = create_linestring_2d();
        assert!(ls.is_ok());

        if let Ok(linestring) = ls {
            let result = length_linestring(&linestring, LengthMethod::Planar);
            assert!(result.is_ok());

            if let Ok(len) = result {
                // Length = 3.0 (first segment) + 4.0 (second segment) = 7.0
                assert_relative_eq!(len, 7.0, epsilon = 1e-10);
            }
        }
    }

    #[test]
    fn test_length_linestring_3d() {
        let ls = create_linestring_3d();
        assert!(ls.is_ok());

        if let Ok(linestring) = ls {
            let result = length_linestring_3d(&linestring);
            assert!(result.is_ok());

            if let Ok(len) = result {
                // Length = 3.0 + 4.0 + 5.0 = 12.0
                assert_relative_eq!(len, 12.0, epsilon = 1e-10);
            }
        }
    }

    #[test]
    fn test_length_linestring_single_point() {
        let coords = vec![Coordinate::new_2d(0.0, 0.0)];
        let ls = LineString::new(coords);

        // LineString::new should fail with a single point
        assert!(ls.is_err());
    }

    #[test]
    fn test_length_linestring_two_points() {
        let coords = vec![Coordinate::new_2d(0.0, 0.0), Coordinate::new_2d(5.0, 0.0)];
        let ls = LineString::new(coords);
        assert!(ls.is_ok());

        if let Ok(linestring) = ls {
            let result = length_linestring(&linestring, LengthMethod::Planar);
            assert!(result.is_ok());

            if let Ok(len) = result {
                assert_relative_eq!(len, 5.0, epsilon = 1e-10);
            }
        }
    }

    #[test]
    fn test_length_multilinestring() {
        let ls1 = create_linestring_2d();
        let coords2 = vec![Coordinate::new_2d(0.0, 0.0), Coordinate::new_2d(10.0, 0.0)];
        let ls2 = LineString::new(coords2);

        assert!(ls1.is_ok() && ls2.is_ok());

        if let (Ok(l1), Ok(l2)) = (ls1, ls2) {
            let mls = MultiLineString::new(vec![l1, l2]);
            let result = length_multilinestring(&mls, LengthMethod::Planar);
            assert!(result.is_ok());

            if let Ok(len) = result {
                // Total = 7.0 + 10.0 = 17.0
                assert_relative_eq!(len, 17.0, epsilon = 1e-10);
            }
        }
    }

    #[test]
    fn test_length_multilinestring_empty() {
        let mls = MultiLineString::empty();
        let result = length_multilinestring(&mls, LengthMethod::Planar);
        assert!(result.is_ok());

        if let Ok(len) = result {
            assert_eq!(len, 0.0);
        }
    }

    #[test]
    fn test_length_haversine() {
        // Line from equator at 0° to 1° longitude
        let coords = vec![Coordinate::new_2d(0.0, 0.0), Coordinate::new_2d(1.0, 0.0)];
        let ls = LineString::new(coords);
        assert!(ls.is_ok());

        if let Ok(linestring) = ls {
            let result = length_linestring(&linestring, LengthMethod::Haversine);
            assert!(result.is_ok());

            if let Ok(len) = result {
                // 1 degree at equator is approximately 111.32 km
                assert!(len > 110_000.0);
                assert!(len < 112_000.0);
            }
        }
    }

    #[test]
    fn test_length_vincenty() {
        // Line from equator at 0° to 1° longitude
        let coords = vec![Coordinate::new_2d(0.0, 0.0), Coordinate::new_2d(1.0, 0.0)];
        let ls = LineString::new(coords);
        assert!(ls.is_ok());

        if let Ok(linestring) = ls {
            let result = length_linestring(&linestring, LengthMethod::Vincenty);
            assert!(result.is_ok());

            if let Ok(len) = result {
                // 1 degree at equator is approximately 111.32 km
                assert!(len > 110_000.0);
                assert!(len < 112_000.0);
            }
        }
    }

    #[test]
    fn test_length_geometry_linestring() {
        let ls = create_linestring_2d();
        assert!(ls.is_ok());

        if let Ok(linestring) = ls {
            let geom = Geometry::LineString(linestring);
            let result = length(&geom, LengthMethod::Planar);
            assert!(result.is_ok());

            if let Ok(len) = result {
                assert_relative_eq!(len, 7.0, epsilon = 1e-10);
            }
        }
    }

    #[test]
    fn test_length_geometry_multilinestring() {
        let ls = create_linestring_2d();
        assert!(ls.is_ok());

        if let Ok(linestring) = ls {
            let mls = MultiLineString::new(vec![linestring]);
            let geom = Geometry::MultiLineString(mls);
            let result = length(&geom, LengthMethod::Planar);
            assert!(result.is_ok());

            if let Ok(len) = result {
                assert_relative_eq!(len, 7.0, epsilon = 1e-10);
            }
        }
    }

    #[test]
    fn test_length_invalid_geometry() {
        use oxigdal_core::vector::Point;

        let point = Geometry::Point(Point::new(0.0, 0.0));
        let result = length(&point, LengthMethod::Planar);
        assert!(result.is_err());
    }

    #[test]
    fn test_haversine_distance_basic() {
        let c1 = Coordinate::new_2d(0.0, 0.0);
        let c2 = Coordinate::new_2d(1.0, 1.0);

        let result = haversine_distance(&c1, &c2);
        assert!(result.is_ok());

        if let Ok(dist) = result {
            // Should be positive and reasonable
            assert!(dist > 0.0);
            assert!(dist < 200_000.0); // Less than 200 km
        }
    }

    #[test]
    fn test_haversine_distance_same_point() {
        let c1 = Coordinate::new_2d(0.0, 0.0);
        let c2 = Coordinate::new_2d(0.0, 0.0);

        let result = haversine_distance(&c1, &c2);
        assert!(result.is_ok());

        if let Ok(dist) = result {
            assert!(dist.abs() < 1e-10);
        }
    }

    #[test]
    fn test_vincenty_distance_basic() {
        let c1 = Coordinate::new_2d(0.0, 0.0);
        let c2 = Coordinate::new_2d(1.0, 1.0);

        let result = vincenty_distance(&c1, &c2);
        assert!(result.is_ok());

        if let Ok(dist) = result {
            // Should be positive and reasonable
            assert!(dist > 0.0);
            assert!(dist < 200_000.0); // Less than 200 km
        }
    }

    #[test]
    fn test_vincenty_distance_same_point() {
        let c1 = Coordinate::new_2d(0.0, 0.0);
        let c2 = Coordinate::new_2d(0.0, 0.0);

        let result = vincenty_distance(&c1, &c2);
        assert!(result.is_ok());

        if let Ok(dist) = result {
            assert!(dist.abs() < 1e-10);
        }
    }

    #[test]
    fn test_invalid_latitude() {
        let c1 = Coordinate::new_2d(0.0, 95.0); // Invalid latitude
        let c2 = Coordinate::new_2d(0.0, 0.0);

        let result = haversine_distance(&c1, &c2);
        assert!(result.is_err());

        let result2 = vincenty_distance(&c1, &c2);
        assert!(result2.is_err());
    }

    #[test]
    fn test_length_linestring_closed_ring() {
        // Square ring
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(10.0, 0.0),
            Coordinate::new_2d(10.0, 10.0),
            Coordinate::new_2d(0.0, 10.0),
            Coordinate::new_2d(0.0, 0.0),
        ];
        let ls = LineString::new(coords);
        assert!(ls.is_ok());

        if let Ok(linestring) = ls {
            let result = length_linestring(&linestring, LengthMethod::Planar);
            assert!(result.is_ok());

            if let Ok(len) = result {
                // Perimeter = 4 * 10 = 40
                assert_relative_eq!(len, 40.0, epsilon = 1e-10);
            }
        }
    }

    #[test]
    fn test_length_planar_equals_3d_for_2d() {
        let ls = create_linestring_2d();
        assert!(ls.is_ok());

        if let Ok(linestring) = ls {
            let planar = length_linestring(&linestring, LengthMethod::Planar);
            let three_d = length_linestring_3d(&linestring);

            assert!(planar.is_ok() && three_d.is_ok());

            if let (Ok(p), Ok(td)) = (planar, three_d) {
                // For 2D coordinates, planar and 3D should be equal
                assert_relative_eq!(p, td, epsilon = 1e-10);
            }
        }
    }
}
