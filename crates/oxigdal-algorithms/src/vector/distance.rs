//! Distance calculation between geometric features
//!
//! This module provides functions for computing distances between geometries
//! using different metrics appropriate for planar and geodetic coordinates.
//!
//! # Distance Methods
//!
//! - **Euclidean**: Simple Cartesian distance (fast, for projected data)
//! - **Haversine**: Great-circle distance on sphere (good approximation for lat/lon)
//! - **Vincenty**: Precise ellipsoidal distance on WGS84 (most accurate for lat/lon)
//!
//! # Examples
//!
//! ```
//! # use oxigdal_algorithms::error::Result;
//! use oxigdal_algorithms::vector::{Point, distance_point_to_point, DistanceMethod};
//!
//! # fn main() -> Result<()> {
//! let p1 = Point::new(0.0, 0.0);
//! let p2 = Point::new(3.0, 4.0);
//! let dist = distance_point_to_point(&p1, &p2, DistanceMethod::Euclidean)?;
//! // Distance = 5.0
//! # Ok(())
//! # }
//! ```

use crate::error::{AlgorithmError, Result};
use oxigdal_core::vector::{Coordinate, LineString, Point, Polygon};

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

/// Distance calculation method
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistanceMethod {
    /// Euclidean (Cartesian) distance
    Euclidean,
    /// Haversine formula (spherical Earth approximation)
    Haversine,
    /// Vincenty's formula (accurate ellipsoidal distance)
    Vincenty,
}

/// Computes distance between two points
///
/// # Arguments
///
/// * `p1` - First point
/// * `p2` - Second point
/// * `method` - Distance calculation method
///
/// # Returns
///
/// Distance in appropriate units (meters for geodetic methods, coordinate units for Euclidean)
///
/// # Errors
///
/// Returns error if coordinates are invalid or computation fails
pub fn distance_point_to_point(p1: &Point, p2: &Point, method: DistanceMethod) -> Result<f64> {
    match method {
        DistanceMethod::Euclidean => Ok(euclidean_distance(&p1.coord, &p2.coord)),
        DistanceMethod::Haversine => haversine_distance(&p1.coord, &p2.coord),
        DistanceMethod::Vincenty => vincenty_distance(&p1.coord, &p2.coord),
    }
}

/// Computes minimum distance from a point to a linestring
///
/// # Arguments
///
/// * `point` - Input point
/// * `linestring` - Input linestring
/// * `method` - Distance calculation method
///
/// # Returns
///
/// Minimum distance from point to any point on the linestring
///
/// # Errors
///
/// Returns error if linestring is empty or computation fails
pub fn distance_point_to_linestring(
    point: &Point,
    linestring: &LineString,
    method: DistanceMethod,
) -> Result<f64> {
    if linestring.coords.is_empty() {
        return Err(AlgorithmError::EmptyInput {
            operation: "distance_point_to_linestring",
        });
    }

    if linestring.coords.len() == 1 {
        return distance_point_to_point(point, &Point::from_coord(linestring.coords[0]), method);
    }

    let mut min_dist = f64::INFINITY;

    for i in 0..linestring.coords.len() - 1 {
        let seg_start = &linestring.coords[i];
        let seg_end = &linestring.coords[i + 1];

        let dist = distance_point_to_segment(&point.coord, seg_start, seg_end, method)?;
        min_dist = min_dist.min(dist);
    }

    Ok(min_dist)
}

/// Computes minimum distance from a point to a polygon boundary
///
/// # Arguments
///
/// * `point` - Input point
/// * `polygon` - Input polygon
/// * `method` - Distance calculation method
///
/// # Returns
///
/// Minimum distance from point to polygon boundary (0 if point is inside)
///
/// # Errors
///
/// Returns error if polygon is invalid or computation fails
pub fn distance_point_to_polygon(
    point: &Point,
    polygon: &Polygon,
    method: DistanceMethod,
) -> Result<f64> {
    if polygon.exterior.coords.len() < 3 {
        return Err(AlgorithmError::InsufficientData {
            operation: "distance_point_to_polygon",
            message: "polygon must have at least 3 coordinates".to_string(),
        });
    }

    // Check if point is inside polygon (using ray casting)
    if point_in_polygon_boundary(&point.coord, polygon) {
        return Ok(0.0);
    }

    // Compute distance to exterior ring
    let mut min_dist = distance_point_to_linestring(point, &polygon.exterior, method)?;

    // Check distance to holes
    for hole in &polygon.interiors {
        let hole_dist = distance_point_to_linestring(point, hole, method)?;
        min_dist = min_dist.min(hole_dist);
    }

    Ok(min_dist)
}

/// Computes Euclidean distance between two coordinates
fn euclidean_distance(c1: &Coordinate, c2: &Coordinate) -> f64 {
    let dx = c2.x - c1.x;
    let dy = c2.y - c1.y;

    if let (Some(z1), Some(z2)) = (c1.z, c2.z) {
        let dz = z2 - z1;
        (dx * dx + dy * dy + dz * dz).sqrt()
    } else {
        (dx * dx + dy * dy).sqrt()
    }
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

/// Computes distance from a point to a line segment
fn distance_point_to_segment(
    point: &Coordinate,
    seg_start: &Coordinate,
    seg_end: &Coordinate,
    method: DistanceMethod,
) -> Result<f64> {
    match method {
        DistanceMethod::Euclidean => Ok(distance_point_to_segment_euclidean(
            point, seg_start, seg_end,
        )),
        DistanceMethod::Haversine | DistanceMethod::Vincenty => {
            // For geodetic methods, approximate by sampling the segment
            distance_point_to_segment_geodetic(point, seg_start, seg_end, method)
        }
    }
}

/// Euclidean distance from point to segment
fn distance_point_to_segment_euclidean(
    point: &Coordinate,
    seg_start: &Coordinate,
    seg_end: &Coordinate,
) -> f64 {
    let dx = seg_end.x - seg_start.x;
    let dy = seg_end.y - seg_start.y;

    let len_sq = dx * dx + dy * dy;

    if len_sq < f64::EPSILON {
        // Segment is actually a point
        return euclidean_distance(point, seg_start);
    }

    // Compute projection parameter t
    let t = ((point.x - seg_start.x) * dx + (point.y - seg_start.y) * dy) / len_sq;

    let t_clamped = t.clamp(0.0, 1.0);

    // Compute closest point on segment
    let closest_x = seg_start.x + t_clamped * dx;
    let closest_y = seg_start.y + t_clamped * dy;

    let closest = Coordinate::new_2d(closest_x, closest_y);

    euclidean_distance(point, &closest)
}

/// Geodetic distance from point to segment (approximated by sampling)
fn distance_point_to_segment_geodetic(
    point: &Coordinate,
    seg_start: &Coordinate,
    seg_end: &Coordinate,
    method: DistanceMethod,
) -> Result<f64> {
    // Sample the segment at regular intervals
    const NUM_SAMPLES: usize = 10;

    let mut min_dist = f64::INFINITY;

    for i in 0..=NUM_SAMPLES {
        let t = i as f64 / NUM_SAMPLES as f64;

        let sample_x = seg_start.x + t * (seg_end.x - seg_start.x);
        let sample_y = seg_start.y + t * (seg_end.y - seg_start.y);
        let sample = Coordinate::new_2d(sample_x, sample_y);

        let dist = match method {
            DistanceMethod::Haversine => haversine_distance(point, &sample)?,
            DistanceMethod::Vincenty => vincenty_distance(point, &sample)?,
            _ => euclidean_distance(point, &sample),
        };

        min_dist = min_dist.min(dist);
    }

    Ok(min_dist)
}

/// Simple point-in-polygon test using ray casting
fn point_in_polygon_boundary(point: &Coordinate, polygon: &Polygon) -> bool {
    ray_casting_test(point, &polygon.exterior.coords)
}

/// Ray casting algorithm for point-in-polygon test
fn ray_casting_test(point: &Coordinate, ring: &[Coordinate]) -> bool {
    let mut inside = false;
    let n = ring.len();

    let mut j = n - 1;
    for i in 0..n {
        let xi = ring[i].x;
        let yi = ring[i].y;
        let xj = ring[j].x;
        let yj = ring[j].y;

        let intersect = ((yi > point.y) != (yj > point.y))
            && (point.x < (xj - xi) * (point.y - yi) / (yj - yi) + xi);

        if intersect {
            inside = !inside;
        }

        j = i;
    }

    inside
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_distance_point_to_point_euclidean() {
        let p1 = Point::new(0.0, 0.0);
        let p2 = Point::new(3.0, 4.0);

        let result = distance_point_to_point(&p1, &p2, DistanceMethod::Euclidean);
        assert!(result.is_ok());

        if let Ok(dist) = result {
            assert!((dist - 5.0).abs() < 1e-10);
        }
    }

    #[test]
    fn test_distance_point_to_point_3d() {
        let p1 = Point::new_3d(0.0, 0.0, 0.0);
        let p2 = Point::new_3d(1.0, 1.0, 1.0);

        let result = distance_point_to_point(&p1, &p2, DistanceMethod::Euclidean);
        assert!(result.is_ok());

        if let Ok(dist) = result {
            assert!((dist - 3.0_f64.sqrt()).abs() < 1e-10);
        }
    }

    #[test]
    fn test_distance_point_to_point_haversine() {
        // Distance from New York to London (approximately)
        let nyc = Point::new(-74.0, 40.7); // NYC: 74°W, 40.7°N
        let lon = Point::new(-0.1, 51.5); // London: 0.1°W, 51.5°N

        let result = distance_point_to_point(&nyc, &lon, DistanceMethod::Haversine);
        assert!(result.is_ok());

        if let Ok(dist) = result {
            // Approximate distance is about 5,570 km
            assert!(dist > 5_000_000.0);
            assert!(dist < 6_000_000.0);
        }
    }

    #[test]
    fn test_distance_point_to_linestring() {
        let point = Point::new(1.0, 1.0);

        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(2.0, 0.0),
            Coordinate::new_2d(2.0, 2.0),
        ];
        let linestring = LineString::new(coords);
        assert!(linestring.is_ok());

        if let Ok(ls) = linestring {
            let result = distance_point_to_linestring(&point, &ls, DistanceMethod::Euclidean);
            assert!(result.is_ok());

            if let Ok(dist) = result {
                // Point (1,1) is 1 unit from segment (0,0)-(2,0)
                assert!((dist - 1.0).abs() < 1e-10);
            }
        }
    }

    #[test]
    fn test_distance_point_to_polygon() {
        // Square polygon
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(4.0, 0.0),
            Coordinate::new_2d(4.0, 4.0),
            Coordinate::new_2d(0.0, 4.0),
            Coordinate::new_2d(0.0, 0.0),
        ];
        let exterior = LineString::new(coords);
        assert!(exterior.is_ok());

        if let Ok(ext) = exterior {
            let polygon = Polygon::new(ext, vec![]);
            assert!(polygon.is_ok());

            if let Ok(poly) = polygon {
                // Point inside polygon
                let inside = Point::new(2.0, 2.0);
                let result1 = distance_point_to_polygon(&inside, &poly, DistanceMethod::Euclidean);
                assert!(result1.is_ok());

                if let Ok(dist) = result1 {
                    assert_eq!(dist, 0.0);
                }

                // Point outside polygon
                let outside = Point::new(5.0, 5.0);
                let result2 = distance_point_to_polygon(&outside, &poly, DistanceMethod::Euclidean);
                assert!(result2.is_ok());

                if let Ok(dist) = result2 {
                    // Distance from (5,5) to corner (4,4)
                    assert!((dist - 2.0_f64.sqrt()).abs() < 1e-10);
                }
            }
        }
    }

    #[test]
    fn test_euclidean_distance() {
        let c1 = Coordinate::new_2d(0.0, 0.0);
        let c2 = Coordinate::new_2d(3.0, 4.0);

        let dist = euclidean_distance(&c1, &c2);
        assert!((dist - 5.0).abs() < 1e-10);
    }

    #[test]
    fn test_haversine_same_point() {
        let c1 = Coordinate::new_2d(0.0, 0.0);
        let c2 = Coordinate::new_2d(0.0, 0.0);

        let result = haversine_distance(&c1, &c2);
        assert!(result.is_ok());

        if let Ok(dist) = result {
            assert!(dist.abs() < 1e-10);
        }
    }

    #[test]
    fn test_vincenty_same_point() {
        let c1 = Coordinate::new_2d(0.0, 0.0);
        let c2 = Coordinate::new_2d(0.0, 0.0);

        let result = vincenty_distance(&c1, &c2);
        assert!(result.is_ok());

        if let Ok(dist) = result {
            assert!(dist.abs() < 1e-10);
        }
    }

    #[test]
    fn test_distance_point_to_segment_euclidean() {
        let point = Coordinate::new_2d(1.0, 1.0);
        let seg_start = Coordinate::new_2d(0.0, 0.0);
        let seg_end = Coordinate::new_2d(2.0, 0.0);

        let dist = distance_point_to_segment_euclidean(&point, &seg_start, &seg_end);
        assert!((dist - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_ray_casting() {
        let ring = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(4.0, 0.0),
            Coordinate::new_2d(4.0, 4.0),
            Coordinate::new_2d(0.0, 4.0),
            Coordinate::new_2d(0.0, 0.0),
        ];

        // Point inside
        let inside = Coordinate::new_2d(2.0, 2.0);
        assert!(ray_casting_test(&inside, &ring));

        // Point outside
        let outside = Coordinate::new_2d(5.0, 5.0);
        assert!(!ray_casting_test(&outside, &ring));
    }

    #[test]
    fn test_invalid_latitude() {
        let c1 = Coordinate::new_2d(0.0, 95.0); // Invalid latitude
        let c2 = Coordinate::new_2d(0.0, 0.0);

        let result = haversine_distance(&c1, &c2);
        assert!(result.is_err());
    }
}
