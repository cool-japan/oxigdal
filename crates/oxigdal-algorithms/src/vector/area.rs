//! Area calculation for polygonal geometries
//!
//! This module provides functions for computing areas of polygons using
//! different methods appropriate for planar and geodetic coordinates.
//!
//! # Area Methods
//!
//! - **Planar**: Fast area calculation assuming Cartesian coordinates (projected data)
//! - **Geodetic**: Accurate area on Earth's surface using WGS84 ellipsoid
//! - **Signed**: Preserves orientation information (counter-clockwise = positive)
//!
//! # Examples
//!
//! ```
//! use oxigdal_algorithms::vector::{Coordinate, Polygon, LineString, area_polygon, AreaMethod};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let coords = vec![
//!     Coordinate::new_2d(0.0, 0.0),
//!     Coordinate::new_2d(10.0, 0.0),
//!     Coordinate::new_2d(10.0, 10.0),
//!     Coordinate::new_2d(0.0, 10.0),
//!     Coordinate::new_2d(0.0, 0.0),
//! ];
//! let exterior = LineString::new(coords)?;
//! let polygon = Polygon::new(exterior, vec![])?;
//! let area_planar = area_polygon(&polygon, AreaMethod::Planar)?;
//! // Planar area = 100.0 square units
//! # Ok(())
//! # }
//! ```

use crate::error::{AlgorithmError, Result};
use oxigdal_core::vector::{Coordinate, Geometry, MultiPolygon, Polygon};

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

/// Area calculation method
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AreaMethod {
    /// Planar area using Cartesian coordinates (fast, for projected data)
    Planar,
    /// Geodetic area on WGS84 ellipsoid (accurate for lat/lon coordinates)
    Geodetic,
    /// Signed planar area (preserves orientation)
    SignedPlanar,
}

/// Computes the area of a geometry
///
/// # Arguments
///
/// * `geometry` - Input geometry
/// * `method` - Area calculation method
///
/// # Returns
///
/// Area in appropriate units (square meters for geodetic, coordinate units for planar)
///
/// # Errors
///
/// Returns error if geometry is not polygonal or invalid
pub fn area(geometry: &Geometry, method: AreaMethod) -> Result<f64> {
    match geometry {
        Geometry::Polygon(p) => area_polygon(p, method),
        Geometry::MultiPolygon(mp) => area_multipolygon(mp, method),
        _ => Err(AlgorithmError::GeometryError {
            message: "area calculation requires Polygon or MultiPolygon geometry".to_string(),
        }),
    }
}

/// Computes the area of a polygon
///
/// # Arguments
///
/// * `polygon` - Input polygon
/// * `method` - Area calculation method
///
/// # Returns
///
/// Area value (always non-negative unless using SignedPlanar method)
///
/// # Errors
///
/// Returns error if polygon is invalid
pub fn area_polygon(polygon: &Polygon, method: AreaMethod) -> Result<f64> {
    if polygon.exterior.coords.len() < 3 {
        return Err(AlgorithmError::InsufficientData {
            operation: "area_polygon",
            message: "polygon must have at least 3 coordinates".to_string(),
        });
    }

    match method {
        AreaMethod::Planar => Ok(area_polygon_planar(polygon)),
        AreaMethod::Geodetic => area_polygon_geodetic(polygon),
        AreaMethod::SignedPlanar => Ok(area_polygon_signed(polygon)),
    }
}

/// Computes the area of a multipolygon
///
/// # Arguments
///
/// * `multipolygon` - Input multipolygon
/// * `method` - Area calculation method
///
/// # Returns
///
/// Total area of all polygons
///
/// # Errors
///
/// Returns error if any polygon is invalid
pub fn area_multipolygon(multipolygon: &MultiPolygon, method: AreaMethod) -> Result<f64> {
    if multipolygon.polygons.is_empty() {
        return Ok(0.0);
    }

    let mut total = 0.0;
    for polygon in &multipolygon.polygons {
        total += area_polygon(polygon, method)?;
    }

    Ok(total)
}

/// Computes planar area of a polygon (fast, Cartesian)
///
/// Uses the shoelace formula (Gauss's area formula).
fn area_polygon_planar(polygon: &Polygon) -> f64 {
    let mut area = ring_area_planar(&polygon.exterior.coords).abs();

    // Subtract holes
    for hole in &polygon.interiors {
        area -= ring_area_planar(&hole.coords).abs();
    }

    area
}

/// Computes signed planar area of a polygon
///
/// Positive for counter-clockwise orientation, negative for clockwise.
fn area_polygon_signed(polygon: &Polygon) -> f64 {
    let mut area = ring_area_planar(&polygon.exterior.coords);

    // Subtract holes (with their signs)
    for hole in &polygon.interiors {
        area -= ring_area_planar(&hole.coords);
    }

    area
}

/// Computes planar area of a ring using shoelace formula
fn ring_area_planar(coords: &[Coordinate]) -> f64 {
    if coords.len() < 3 {
        return 0.0;
    }

    let mut area = 0.0;
    let n = coords.len();

    for i in 0..n {
        let j = (i + 1) % n;
        area += coords[i].x * coords[j].y;
        area -= coords[j].x * coords[i].y;
    }

    area / 2.0
}

/// Computes geodetic area of a polygon on WGS84 ellipsoid
///
/// Uses the method from Bessel (1825) for geodetic polygons.
/// Coordinates must be in longitude/latitude (degrees).
fn area_polygon_geodetic(polygon: &Polygon) -> Result<f64> {
    let mut area = ring_area_geodetic(&polygon.exterior.coords)?;

    // Subtract holes
    for hole in &polygon.interiors {
        area -= ring_area_geodetic(&hole.coords)?;
    }

    Ok(area.abs())
}

/// Computes geodetic area of a ring
///
/// Implementation based on:
/// Chamberlain, R. G., and W. H. Duquette. "Some algorithms for polygons on a sphere."
/// JPL Publication 07-03 (2007).
fn ring_area_geodetic(coords: &[Coordinate]) -> Result<f64> {
    if coords.len() < 3 {
        return Ok(0.0);
    }

    let n = coords.len();
    let mut area = 0.0;

    for i in 0..n - 1 {
        let p1 = &coords[i];
        let p2 = &coords[i + 1];

        // Convert to radians
        let lat1 = p1.y * PI / 180.0;
        let lon1 = p1.x * PI / 180.0;
        let lat2 = p2.y * PI / 180.0;
        let lon2 = p2.x * PI / 180.0;

        // Validate latitude range
        if lat1.abs() > PI / 2.0 || lat2.abs() > PI / 2.0 {
            return Err(AlgorithmError::InvalidParameter {
                parameter: "latitude",
                message: "latitude must be between -90 and 90 degrees".to_string(),
            });
        }

        // Calculate area contribution of this edge
        let dlon = lon2 - lon1;

        // Handle antimeridian crossing
        let dlon = if dlon > PI {
            dlon - 2.0 * PI
        } else if dlon < -PI {
            dlon + 2.0 * PI
        } else {
            dlon
        };

        // Accumulate area using trapezoid rule on the sphere
        area += dlon * (lat1.sin() + lat2.sin());
    }

    // Convert to square meters using WGS84 parameters
    let authalic_radius =
        ((WGS84_A * WGS84_A + WGS84_B * WGS84_B / (1.0 - WGS84_F).sqrt()) / 2.0).sqrt();
    let area_m2 = area.abs() * authalic_radius * authalic_radius / 2.0;

    Ok(area_m2)
}

/// Checks if a polygon is oriented counter-clockwise
///
/// # Arguments
///
/// * `polygon` - Input polygon
///
/// # Returns
///
/// True if exterior ring is counter-clockwise, false otherwise
pub fn is_counter_clockwise(polygon: &Polygon) -> bool {
    ring_area_planar(&polygon.exterior.coords) > 0.0
}

/// Checks if a polygon is oriented clockwise
///
/// # Arguments
///
/// * `polygon` - Input polygon
///
/// # Returns
///
/// True if exterior ring is clockwise, false otherwise
pub fn is_clockwise(polygon: &Polygon) -> bool {
    ring_area_planar(&polygon.exterior.coords) < 0.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigdal_core::vector::LineString;

    fn create_square(size: f64) -> Result<Polygon> {
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(size, 0.0),
            Coordinate::new_2d(size, size),
            Coordinate::new_2d(0.0, size),
            Coordinate::new_2d(0.0, 0.0),
        ];
        let exterior = LineString::new(coords).map_err(|e| AlgorithmError::Core(e))?;
        Polygon::new(exterior, vec![]).map_err(|e| AlgorithmError::Core(e))
    }

    fn create_square_with_hole() -> Result<Polygon> {
        let exterior_coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(10.0, 0.0),
            Coordinate::new_2d(10.0, 10.0),
            Coordinate::new_2d(0.0, 10.0),
            Coordinate::new_2d(0.0, 0.0),
        ];
        let hole_coords = vec![
            Coordinate::new_2d(2.0, 2.0),
            Coordinate::new_2d(8.0, 2.0),
            Coordinate::new_2d(8.0, 8.0),
            Coordinate::new_2d(2.0, 8.0),
            Coordinate::new_2d(2.0, 2.0),
        ];

        let exterior = LineString::new(exterior_coords).map_err(|e| AlgorithmError::Core(e))?;
        let hole = LineString::new(hole_coords).map_err(|e| AlgorithmError::Core(e))?;

        Polygon::new(exterior, vec![hole]).map_err(|e| AlgorithmError::Core(e))
    }

    #[test]
    fn test_area_polygon_planar() {
        let poly = create_square(10.0);
        assert!(poly.is_ok());

        if let Ok(p) = poly {
            let result = area_polygon(&p, AreaMethod::Planar);
            assert!(result.is_ok());

            if let Ok(area) = result {
                assert!((area - 100.0).abs() < 1e-10);
            }
        }
    }

    #[test]
    fn test_area_polygon_with_hole() {
        let poly = create_square_with_hole();
        assert!(poly.is_ok());

        if let Ok(p) = poly {
            let result = area_polygon(&p, AreaMethod::Planar);
            assert!(result.is_ok());

            if let Ok(area) = result {
                // Outer area = 100, hole area = 36, effective = 64
                assert!((area - 64.0).abs() < 1e-10);
            }
        }
    }

    #[test]
    fn test_area_polygon_signed() {
        let poly = create_square(10.0);
        assert!(poly.is_ok());

        if let Ok(p) = poly {
            let result = area_polygon(&p, AreaMethod::SignedPlanar);
            assert!(result.is_ok());

            if let Ok(area) = result {
                // CCW orientation = positive area
                assert!(area > 0.0);
                assert!((area.abs() - 100.0).abs() < 1e-10);
            }
        }
    }

    #[test]
    fn test_area_multipolygon() {
        let poly1 = create_square(5.0);
        let poly2 = create_square(3.0);

        assert!(poly1.is_ok() && poly2.is_ok());

        if let (Ok(p1), Ok(p2)) = (poly1, poly2) {
            let mp = MultiPolygon::new(vec![p1, p2]);
            let result = area_multipolygon(&mp, AreaMethod::Planar);
            assert!(result.is_ok());

            if let Ok(area) = result {
                // 5*5 + 3*3 = 25 + 9 = 34
                assert!((area - 34.0).abs() < 1e-10);
            }
        }
    }

    #[test]
    fn test_area_geodetic_small_polygon() {
        // Small polygon in degrees (approximately 1 degree square near equator)
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 0.0),
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(0.0, 1.0),
            Coordinate::new_2d(0.0, 0.0),
        ];
        let exterior = LineString::new(coords);
        assert!(exterior.is_ok());

        if let Ok(ext) = exterior {
            let poly = Polygon::new(ext, vec![]);
            assert!(poly.is_ok());

            if let Ok(p) = poly {
                let result = area_polygon(&p, AreaMethod::Geodetic);
                assert!(result.is_ok());

                if let Ok(area) = result {
                    // 1 degree at equator is approximately 111 km
                    // So 1 degree square is approximately 111 * 111 = 12,321 km^2
                    // = 12,321,000,000 m^2
                    assert!(area > 1e10); // Should be in billions of square meters
                    assert!(area < 2e10); // Rough upper bound
                }
            }
        }
    }

    #[test]
    fn test_is_counter_clockwise() {
        let poly = create_square(10.0);
        assert!(poly.is_ok());

        if let Ok(p) = poly {
            assert!(is_counter_clockwise(&p));
            assert!(!is_clockwise(&p));
        }
    }

    #[test]
    fn test_ring_area_planar() {
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(10.0, 0.0),
            Coordinate::new_2d(10.0, 10.0),
            Coordinate::new_2d(0.0, 10.0),
            Coordinate::new_2d(0.0, 0.0),
        ];

        let area = ring_area_planar(&coords);
        assert!((area.abs() - 100.0).abs() < 1e-10);
    }

    #[test]
    fn test_area_empty_multipolygon() {
        let mp = MultiPolygon::empty();
        let result = area_multipolygon(&mp, AreaMethod::Planar);
        assert!(result.is_ok());

        if let Ok(area) = result {
            assert_eq!(area, 0.0);
        }
    }

    #[test]
    fn test_area_invalid_geometry() {
        let point = Geometry::Point(oxigdal_core::vector::Point::new(0.0, 0.0));
        let result = area(&point, AreaMethod::Planar);
        assert!(result.is_err());
    }

    #[test]
    fn test_geodetic_area_invalid_latitude() {
        // Latitude > 90 degrees
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 0.0),
            Coordinate::new_2d(1.0, 100.0), // Invalid
            Coordinate::new_2d(0.0, 1.0),
            Coordinate::new_2d(0.0, 0.0),
        ];
        let exterior = LineString::new(coords);
        assert!(exterior.is_ok());

        if let Ok(ext) = exterior {
            let poly = Polygon::new(ext, vec![]);
            assert!(poly.is_ok());

            if let Ok(p) = poly {
                let result = area_polygon(&p, AreaMethod::Geodetic);
                assert!(result.is_err());
            }
        }
    }
}
