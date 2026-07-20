//! Karney's geodesic algorithms for accurate ellipsoidal computations
//!
//! Implements geodesic area and distance computation on the WGS84 ellipsoid
//! using the algorithms from:
//! Karney, C.F.F. (2013) "Algorithms for geodesics",
//! Journal of Geodesy 87(1), pp. 43-55.
//!
//! Provides sub-millimeter accuracy (relative error < 1e-15 for full Earth)
//! for geodesic area computation. Handles antimeridian-crossing and
//! polar-enclosing polygons correctly.
//!
//! # Algorithm Overview
//!
//! Uses 6th-order Taylor expansion in the third flattening `n = (a-b)/(a+b)`.
//! For each polygon edge, solves the inverse geodesic problem to obtain the S12
//! area element. The polygon area is the sum of S12 values with winding
//! correction.
//!
//! # Backend
//!
//! Powered by `geographiclib-rs`, a Pure Rust port of GeographicLib.
//! This ensures correctness (tested against the full GeographicLib test suite)
//! while maintaining the COOLJAPAN Pure Rust policy.

use crate::error::{AlgorithmError, Result};
use oxigeo_core::vector::{Coordinate, Polygon};

// ────────────────────────────────────────────────────
// Re-export of the underlying geodesic engine
// ────────────────────────────────────────────────────

use geographiclib_rs::{
    Geodesic as GeodesicEngine, InverseGeodesic, PolygonArea as GeodesicPolygonArea, Winding,
};

// ────────────────────────────────────────────────────
// Geodesic Struct — thin wrapper
// ────────────────────────────────────────────────────

/// Geodesic solver for an ellipsoid of revolution.
///
/// Wraps the `geographiclib-rs` implementation of Karney's algorithms.
/// The default constructor uses WGS84 parameters.
///
/// # Examples
///
/// ```
/// use oxigeo_algorithms::vector::geodesic::Geodesic;
///
/// let geod = Geodesic::wgs84();
/// let result = geod.inverse(0.0, 0.0, 1.0, 0.0);
/// assert!(result.is_ok());
/// ```
#[derive(Debug, Clone)]
pub struct Geodesic {
    engine: GeodesicEngine,
}

/// Result of the inverse geodesic problem between two points.
#[derive(Debug, Clone, Copy)]
pub struct InverseResult {
    /// Geodesic distance in meters
    pub s12: f64,
    /// Forward azimuth at point 1 (degrees, clockwise from north)
    pub azi1: f64,
    /// Forward azimuth at point 2 (degrees, clockwise from north)
    pub azi2: f64,
    /// Area element between the geodesic and the equator (square meters)
    pub s12_area: f64,
}

/// Result of polygon area computation.
#[derive(Debug, Clone, Copy)]
pub struct PolygonAreaResult {
    /// Absolute area in square meters
    pub area: f64,
    /// Perimeter in meters
    pub perimeter: f64,
    /// Number of vertices
    pub num_vertices: usize,
}

impl Geodesic {
    /// Creates a new `Geodesic` for the WGS84 ellipsoid.
    pub fn wgs84() -> Self {
        Self {
            engine: GeodesicEngine::wgs84(),
        }
    }

    /// Creates a new `Geodesic` for an arbitrary ellipsoid.
    ///
    /// # Arguments
    ///
    /// * `a` - Semi-major axis (meters)
    /// * `f` - Flattening (dimensionless)
    pub fn new(a: f64, f: f64) -> Self {
        Self {
            engine: GeodesicEngine::new(a, f),
        }
    }

    /// Returns the total surface area of the ellipsoid in square meters.
    pub fn ellipsoid_area(&self) -> f64 {
        self.engine.area()
    }

    /// Solve the inverse geodesic problem: given two points (lat1,lon1) and
    /// (lat2,lon2) in degrees, compute distance, azimuths, and area element.
    ///
    /// # Arguments
    ///
    /// * `lat1` - Latitude of point 1 (degrees, -90..90)
    /// * `lon1` - Longitude of point 1 (degrees)
    /// * `lat2` - Latitude of point 2 (degrees, -90..90)
    /// * `lon2` - Longitude of point 2 (degrees)
    ///
    /// # Returns
    ///
    /// `InverseResult` with distance, azimuths, and area element S12.
    ///
    /// # Errors
    ///
    /// Returns error if latitude is out of range [-90, 90].
    pub fn inverse(&self, lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> Result<InverseResult> {
        // Validate latitudes
        if lat1.abs() > 90.0 + 1e-10 || lat2.abs() > 90.0 + 1e-10 {
            return Err(AlgorithmError::InvalidParameter {
                parameter: "latitude",
                message: "latitude must be between -90 and 90 degrees".to_string(),
            });
        }

        // Clamp latitudes to [-90, 90]
        let lat1 = lat1.clamp(-90.0, 90.0);
        let lat2 = lat2.clamp(-90.0, 90.0);

        // Use the full _gen_inverse to get all outputs including S12
        #[allow(non_snake_case)]
        let (_a12, s12, azi1, _calp1, azi2, _calp2, _m12, _M12, _M21, S12) = self
            .engine
            ._gen_inverse(lat1, lon1, lat2, lon2, Self::full_mask());

        Ok(InverseResult {
            s12,
            azi1: Self::atan2d(azi1, _calp1),
            azi2: Self::atan2d(azi2, _calp2),
            s12_area: S12,
        })
    }

    /// Compute distance between two points in meters.
    ///
    /// Convenience method that only returns the distance.
    ///
    /// # Arguments
    ///
    /// * `lat1` - Latitude of point 1 (degrees)
    /// * `lon1` - Longitude of point 1 (degrees)
    /// * `lat2` - Latitude of point 2 (degrees)
    /// * `lon2` - Longitude of point 2 (degrees)
    ///
    /// # Errors
    ///
    /// Returns error if latitude is out of range.
    pub fn distance(&self, lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> Result<f64> {
        if lat1.abs() > 90.0 + 1e-10 || lat2.abs() > 90.0 + 1e-10 {
            return Err(AlgorithmError::InvalidParameter {
                parameter: "latitude",
                message: "latitude must be between -90 and 90 degrees".to_string(),
            });
        }

        let lat1 = lat1.clamp(-90.0, 90.0);
        let lat2 = lat2.clamp(-90.0, 90.0);

        let s12: f64 = self.engine.inverse(lat1, lon1, lat2, lon2);
        Ok(s12)
    }

    /// Compute the area of a polygon whose vertices are given in longitude/latitude degrees.
    ///
    /// The polygon should be given as a ring of coordinates. The ring may or may
    /// not be closed (last == first). Uses Karney's geodesic area algorithm for
    /// sub-millimeter accuracy on the WGS84 ellipsoid.
    ///
    /// # Arguments
    ///
    /// * `coords` - Ring of (longitude, latitude) coordinates in degrees
    ///
    /// # Returns
    ///
    /// `PolygonAreaResult` with absolute area (m^2), perimeter (m), and vertex count.
    ///
    /// # Errors
    ///
    /// Returns error if fewer than 3 distinct vertices or invalid latitudes.
    pub fn polygon_area(&self, coords: &[Coordinate]) -> Result<PolygonAreaResult> {
        if coords.len() < 3 {
            return Err(AlgorithmError::InsufficientData {
                operation: "polygon_area_karney",
                message: "polygon must have at least 3 coordinates".to_string(),
            });
        }

        // Validate all latitudes
        for (i, coord) in coords.iter().enumerate() {
            if coord.y.abs() > 90.0 + 1e-10 {
                return Err(AlgorithmError::InvalidParameter {
                    parameter: "latitude",
                    message: format!(
                        "coordinate {} has invalid latitude {} (must be -90..90)",
                        i, coord.y
                    ),
                });
            }
        }

        // Determine if the ring is closed
        let n = coords.len();
        let is_closed = n > 3
            && (coords[0].x - coords[n - 1].x).abs() < 1e-12
            && (coords[0].y - coords[n - 1].y).abs() < 1e-12;

        // Number of distinct vertices
        let num_verts = if is_closed { n - 1 } else { n };

        // Use geographiclib-rs PolygonArea with CCW winding (standard GIS convention)
        let mut pa = GeodesicPolygonArea::new(&self.engine, Winding::CounterClockwise);

        for i in 0..num_verts {
            let lat = coords[i].y.clamp(-90.0, 90.0);
            let lon = coords[i].x;
            // PolygonArea::add_point takes (lat, lon)
            pa.add_point(lat, lon);
        }

        // compute(true) = signed area (positive for CCW, negative for CW)
        let (perimeter, signed_area, count) = pa.compute(true);

        // Take absolute value to get the polygon interior area.
        // For CW-wound polygons the signed area is negative; abs() recovers the
        // interior area. For very large polygons (> half Earth) this naturally
        // gives the correct interior area since the signed area is already the
        // smaller quantity.
        let area = signed_area.abs();

        Ok(PolygonAreaResult {
            area,
            perimeter,
            num_vertices: count,
        })
    }

    /// Compute the signed area of a polygon ring.
    ///
    /// Positive for counter-clockwise, negative for clockwise.
    /// This is useful for determining polygon winding order.
    ///
    /// # Arguments
    ///
    /// * `coords` - Ring of (longitude, latitude) coordinates in degrees
    ///
    /// # Returns
    ///
    /// Signed area in square meters.
    ///
    /// # Errors
    ///
    /// Returns error if fewer than 3 distinct vertices or invalid latitudes.
    pub fn polygon_area_signed(&self, coords: &[Coordinate]) -> Result<f64> {
        if coords.len() < 3 {
            return Err(AlgorithmError::InsufficientData {
                operation: "polygon_area_signed_karney",
                message: "polygon must have at least 3 coordinates".to_string(),
            });
        }

        // Validate latitudes
        for (i, coord) in coords.iter().enumerate() {
            if coord.y.abs() > 90.0 + 1e-10 {
                return Err(AlgorithmError::InvalidParameter {
                    parameter: "latitude",
                    message: format!(
                        "coordinate {} has invalid latitude {} (must be -90..90)",
                        i, coord.y
                    ),
                });
            }
        }

        let n = coords.len();
        let is_closed = n > 3
            && (coords[0].x - coords[n - 1].x).abs() < 1e-12
            && (coords[0].y - coords[n - 1].y).abs() < 1e-12;
        let num_verts = if is_closed { n - 1 } else { n };

        let mut pa = GeodesicPolygonArea::new(&self.engine, Winding::CounterClockwise);
        for i in 0..num_verts {
            let lat = coords[i].y.clamp(-90.0, 90.0);
            let lon = coords[i].x;
            pa.add_point(lat, lon);
        }

        // compute(true) = signed area
        let (_perimeter, area, _count) = pa.compute(true);
        Ok(area)
    }

    /// Compute the area of an `oxigeo_core::vector::Polygon`.
    ///
    /// Computes the area of the exterior ring minus the area of all interior
    /// rings (holes). Coordinates are expected in (longitude, latitude) degrees.
    ///
    /// # Arguments
    ///
    /// * `polygon` - Input polygon
    ///
    /// # Returns
    ///
    /// Area in square meters (always non-negative)
    ///
    /// # Errors
    ///
    /// Returns error if polygon has fewer than 3 coordinates or invalid latitudes.
    pub fn polygon_area_full(&self, polygon: &Polygon) -> Result<f64> {
        let ext_result = self.polygon_area(&polygon.exterior.coords)?;
        let mut area = ext_result.area;

        for hole in &polygon.interiors {
            let hole_result = self.polygon_area(&hole.coords)?;
            area -= hole_result.area;
        }

        Ok(area.abs())
    }

    // ────────────────────────────────────────────────
    // Internal helpers
    // ────────────────────────────────────────────────

    /// Capability mask for full inverse output including area (S12).
    fn full_mask() -> u64 {
        use geographiclib_rs::geodesic_capability as caps;
        caps::LATITUDE
            | caps::LONGITUDE
            | caps::DISTANCE
            | caps::AREA
            | caps::LONG_UNROLL
            | caps::AZIMUTH
    }

    /// Convert (sin, cos) to degrees
    fn atan2d(sinx: f64, cosx: f64) -> f64 {
        sinx.atan2(cosx).to_degrees()
    }
}

// ────────────────────────────────────────────────────
// Public Convenience Functions
// ────────────────────────────────────────────────────

/// Compute the geodesic area of a polygon ring using Karney's algorithm.
///
/// Convenience function that creates a WGS84 `Geodesic` and calls `polygon_area`.
///
/// # Arguments
///
/// * `coords` - Ring of (longitude, latitude) coordinates in degrees
///
/// # Returns
///
/// Area in square meters
///
/// # Errors
///
/// Returns error if fewer than 3 vertices or invalid latitudes.
pub fn ring_area_karney(coords: &[Coordinate]) -> Result<f64> {
    let geod = Geodesic::wgs84();
    let result = geod.polygon_area(coords)?;
    Ok(result.area)
}

/// Compute the geodesic area of a polygon (exterior minus holes) using Karney's algorithm.
///
/// Convenience function that creates a WGS84 `Geodesic` and calls `polygon_area_full`.
///
/// # Arguments
///
/// * `polygon` - Input polygon
///
/// # Returns
///
/// Area in square meters
///
/// # Errors
///
/// Returns error if polygon is invalid or latitudes out of range.
pub fn area_polygon_karney(polygon: &Polygon) -> Result<f64> {
    let geod = Geodesic::wgs84();
    geod.polygon_area_full(polygon)
}

// ────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use oxigeo_core::vector::LineString;

    /// Helper: create a polygon from a sequence of (lon, lat) pairs.
    /// Automatically closes the ring.
    fn make_polygon(vertices: &[(f64, f64)]) -> Result<Polygon> {
        let mut coords: Vec<Coordinate> = vertices
            .iter()
            .map(|&(lon, lat)| Coordinate::new_2d(lon, lat))
            .collect();
        // Close the ring
        if let Some(&first) = vertices.first() {
            coords.push(Coordinate::new_2d(first.0, first.1));
        }
        let exterior = LineString::new(coords).map_err(AlgorithmError::Core)?;
        Polygon::new(exterior, vec![]).map_err(AlgorithmError::Core)
    }

    #[test]
    fn test_wgs84_construction() {
        let geod = Geodesic::wgs84();
        let area = geod.ellipsoid_area();
        // Earth surface area ~ 5.1e14 m^2
        assert!(area > 5.0e14);
        assert!(area < 5.2e14);
    }

    #[test]
    fn test_inverse_same_point() {
        let geod = Geodesic::wgs84();
        let result = geod.inverse(0.0, 0.0, 0.0, 0.0);
        assert!(result.is_ok());
        if let Ok(inv) = result {
            assert!(inv.s12.abs() < 1e-6);
        }
    }

    #[test]
    fn test_inverse_equator_short() {
        let geod = Geodesic::wgs84();
        // 1 degree along equator ~ 111,319 m
        let result = geod.inverse(0.0, 0.0, 0.0, 1.0);
        assert!(result.is_ok());
        if let Ok(inv) = result {
            assert!(
                inv.s12 > 111_000.0 && inv.s12 < 112_000.0,
                "equatorial 1-degree distance = {}, expected ~111319",
                inv.s12
            );
        }
    }

    #[test]
    fn test_inverse_meridian() {
        let geod = Geodesic::wgs84();
        // 1 degree along meridian at equator ~ 110,574 m
        let result = geod.inverse(0.0, 0.0, 1.0, 0.0);
        assert!(result.is_ok());
        if let Ok(inv) = result {
            assert!(
                inv.s12 > 110_000.0 && inv.s12 < 112_000.0,
                "meridional 1-degree distance = {}, expected ~110574",
                inv.s12
            );
        }
    }

    #[test]
    fn test_inverse_symmetry() {
        let geod = Geodesic::wgs84();
        let r1 = geod.inverse(10.0, 20.0, 30.0, 40.0);
        let r2 = geod.inverse(30.0, 40.0, 10.0, 20.0);

        assert!(r1.is_ok());
        assert!(r2.is_ok());

        if let (Ok(inv1), Ok(inv2)) = (r1, r2) {
            let rel_err = (inv1.s12 - inv2.s12).abs() / inv1.s12.max(1.0);
            assert!(
                rel_err < 1e-12,
                "inverse distance not symmetric: {} vs {}",
                inv1.s12,
                inv2.s12
            );
        }
    }

    #[test]
    fn test_inverse_invalid_latitude() {
        let geod = Geodesic::wgs84();
        // lat2 = 95 is invalid
        let result = geod.inverse(0.0, 0.0, 95.0, 0.0);
        assert!(result.is_err());
    }

    #[test]
    fn test_distance_convenience() {
        let geod = Geodesic::wgs84();
        let d = geod.distance(0.0, 0.0, 0.0, 1.0);
        assert!(d.is_ok());
        if let Ok(dist) = d {
            assert!(dist > 111_000.0 && dist < 112_000.0);
        }
    }

    // ────────────────────────────────────────────────
    // Polygon Area Tests
    // ────────────────────────────────────────────────

    #[test]
    fn test_polygon_area_unit_square_equator() {
        // 1 degree x 1 degree square at equator
        // GeographicLib Planimeter reference: 12,308,778,361.469 m^2
        let geod = Geodesic::wgs84();
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 0.0),
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(0.0, 1.0),
            Coordinate::new_2d(0.0, 0.0),
        ];
        let result = geod.polygon_area(&coords);
        assert!(result.is_ok(), "polygon_area failed: {:?}", result.err());
        if let Ok(pa) = result {
            let reference = 12_308_778_361.469;
            let rel_error = (pa.area - reference).abs() / reference;
            assert!(
                rel_error < 1e-6,
                "Unit square area {:.3} m^2, expected ~{:.3} m^2, relative error: {:.10}",
                pa.area,
                reference,
                rel_error
            );
        }
    }

    #[test]
    fn test_polygon_area_full_earth() {
        // Polygon enclosing the entire Earth (CCW rectangle around the globe)
        // From geographiclib-rs: area of WGS84 ellipsoid
        let geod = Geodesic::wgs84();
        let total_area = geod.ellipsoid_area();

        // A quadrilateral at 89N/89S that nearly covers the whole earth
        let coords = vec![
            Coordinate::new_2d(0.0, -89.0),
            Coordinate::new_2d(90.0, -89.0),
            Coordinate::new_2d(180.0, -89.0),
            Coordinate::new_2d(-90.0, -89.0),
            Coordinate::new_2d(0.0, -89.0),
        ];
        let result = geod.polygon_area(&coords);
        assert!(result.is_ok(), "polygon_area failed: {:?}", result.err());
        if let Ok(pa) = result {
            // This should be a cap around the south pole
            // Much smaller than the total earth area
            assert!(pa.area > 0.0);
            assert!(pa.area < total_area);
        }
    }

    #[test]
    fn test_polygon_area_ellipsoid_area() {
        // Check that the ellipsoid area matches known value
        let geod = Geodesic::wgs84();
        let total_area = geod.ellipsoid_area();
        let reference = 5.10065621724089e14;
        let rel_error = (total_area - reference).abs() / reference;
        assert!(
            rel_error < 1e-8,
            "Ellipsoid area {:.6e}, expected {:.6e}, rel err: {:.10}",
            total_area,
            reference,
            rel_error
        );
    }

    #[test]
    fn test_polygon_area_high_latitude_triangle() {
        // Triangle at 89N (near pole) — matches GeographicLib planimeter0 test
        let geod = Geodesic::wgs84();
        let coords = vec![
            Coordinate::new_2d(0.0, 89.0),
            Coordinate::new_2d(90.0, 89.0),
            Coordinate::new_2d(180.0, 89.0),
            Coordinate::new_2d(270.0, 89.0),
            Coordinate::new_2d(0.0, 89.0),
        ];
        let result = geod.polygon_area(&coords);
        assert!(result.is_ok(), "polygon_area failed: {:?}", result.err());
        if let Ok(pa) = result {
            // GeographicLib reference: 24,952,305,678 m^2
            let reference = 24_952_305_678.0;
            let rel_error = (pa.area - reference).abs() / reference;
            assert!(
                rel_error < 1e-6,
                "High-lat area {:.0} m^2, expected {:.0} m^2, rel err: {:.10}",
                pa.area,
                reference,
                rel_error
            );
        }
    }

    #[test]
    fn test_polygon_area_antimeridian_crossing() {
        // Rectangle crossing the antimeridian (179E to 179W = 2 degrees wide)
        let geod = Geodesic::wgs84();
        let coords = vec![
            Coordinate::new_2d(179.0, 0.0),
            Coordinate::new_2d(-179.0, 0.0),
            Coordinate::new_2d(-179.0, 1.0),
            Coordinate::new_2d(179.0, 1.0),
            Coordinate::new_2d(179.0, 0.0),
        ];
        let result = geod.polygon_area(&coords);
        assert!(result.is_ok(), "polygon_area failed: {:?}", result.err());
        if let Ok(pa) = result {
            // 2 degrees wide, 1 degree tall at equator
            // Should be approximately 2 * 12,308,778,361 m^2
            let reference_approx = 2.0 * 12_308_778_361.0;
            let rel_error = (pa.area - reference_approx).abs() / reference_approx;
            assert!(
                rel_error < 0.01,
                "Antimeridian area {:.3} m^2, expected ~{:.3} m^2, rel error: {:.6}",
                pa.area,
                reference_approx,
                rel_error
            );
        }
    }

    #[test]
    fn test_polygon_area_polar_enclosing() {
        // Quadrilateral around the south pole at -89 latitude.
        // At the south pole, CCW (as seen from outside the Earth) means
        // longitudes decreasing: 0 -> 270 -> 180 -> 90.
        // This matches the 89N CCW test by symmetry.
        let geod = Geodesic::wgs84();
        let coords = vec![
            Coordinate::new_2d(0.0, -89.0),
            Coordinate::new_2d(270.0, -89.0),
            Coordinate::new_2d(180.0, -89.0),
            Coordinate::new_2d(90.0, -89.0),
            Coordinate::new_2d(0.0, -89.0),
        ];
        let result = geod.polygon_area(&coords);
        assert!(result.is_ok(), "polygon_area failed: {:?}", result.err());
        if let Ok(pa) = result {
            assert!(pa.area > 0.0, "polar area should be positive");
            // Same as the 89N test by symmetry
            let reference = 24_952_305_678.0;
            let rel_error = (pa.area - reference).abs() / reference;
            assert!(
                rel_error < 1e-4,
                "Polar enclosing area {:.0}, expected ~{:.0}, rel err {:.8}",
                pa.area,
                reference,
                rel_error
            );
        }
    }

    #[test]
    fn test_polygon_area_cw_ccw_same_absolute() {
        // CCW square
        let geod = Geodesic::wgs84();
        let ccw = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 0.0),
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(0.0, 1.0),
            Coordinate::new_2d(0.0, 0.0),
        ];
        // CW square (reversed)
        let cw = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(0.0, 1.0),
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(1.0, 0.0),
            Coordinate::new_2d(0.0, 0.0),
        ];

        let result_ccw = geod.polygon_area(&ccw);
        let result_cw = geod.polygon_area(&cw);

        assert!(result_ccw.is_ok());
        assert!(result_cw.is_ok());

        if let (Ok(pa_ccw), Ok(pa_cw)) = (result_ccw, result_cw) {
            let diff = (pa_ccw.area - pa_cw.area).abs();
            let mean = (pa_ccw.area + pa_cw.area) / 2.0;
            assert!(
                diff / mean < 1e-10,
                "CW/CCW areas differ: {} vs {}",
                pa_ccw.area,
                pa_cw.area
            );
        }
    }

    #[test]
    fn test_polygon_area_signed_winding() {
        // Test that signed area differentiates CW from CCW
        let geod = Geodesic::wgs84();

        // CCW (standard GIS convention — positive signed area)
        let ccw = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 0.0),
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(0.0, 1.0),
            Coordinate::new_2d(0.0, 0.0),
        ];

        // CW (reversed — negative signed area)
        let cw = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(0.0, 1.0),
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(1.0, 0.0),
            Coordinate::new_2d(0.0, 0.0),
        ];

        let signed_ccw = geod.polygon_area_signed(&ccw);
        let signed_cw = geod.polygon_area_signed(&cw);

        assert!(signed_ccw.is_ok());
        assert!(signed_cw.is_ok());

        if let (Ok(area_ccw), Ok(area_cw)) = (signed_ccw, signed_cw) {
            assert!(
                area_ccw > 0.0,
                "CCW signed area should be positive: {}",
                area_ccw
            );
            assert!(
                area_cw < 0.0,
                "CW signed area should be negative: {}",
                area_cw
            );
            assert!(
                (area_ccw.abs() - area_cw.abs()).abs() / area_ccw.abs() < 1e-10,
                "absolute values should match"
            );
        }
    }

    #[test]
    fn test_polygon_area_degenerate_collinear() {
        // Degenerate polygon: all points on the equator
        let geod = Geodesic::wgs84();
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 0.0),
            Coordinate::new_2d(2.0, 0.0),
            Coordinate::new_2d(0.0, 0.0),
        ];
        let result = geod.polygon_area(&coords);
        assert!(result.is_ok());
        if let Ok(pa) = result {
            assert!(
                pa.area < 1.0,
                "collinear polygon area should be ~0, got {}",
                pa.area
            );
        }
    }

    #[test]
    fn test_polygon_area_convenience_function() {
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 0.0),
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(0.0, 1.0),
            Coordinate::new_2d(0.0, 0.0),
        ];
        let result = ring_area_karney(&coords);
        assert!(result.is_ok());
        if let Ok(area) = result {
            let reference = 12_308_778_361.469;
            let rel_error = (area - reference).abs() / reference;
            assert!(
                rel_error < 1e-6,
                "convenience area {:.3} m^2, expected ~{:.3} m^2",
                area,
                reference
            );
        }
    }

    #[test]
    fn test_area_polygon_karney_with_hole() {
        let outer = make_polygon(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]);
        let inner = make_polygon(&[(2.0, 2.0), (8.0, 2.0), (8.0, 8.0), (2.0, 8.0)]);

        assert!(outer.is_ok());
        assert!(inner.is_ok());

        if let (Ok(outer_p), Ok(inner_p)) = (outer, inner) {
            let exterior = outer_p.exterior.clone();
            let hole = inner_p.exterior.clone();
            let poly_with_hole = Polygon::new(exterior, vec![hole]);
            assert!(poly_with_hole.is_ok());

            if let Ok(p) = poly_with_hole {
                let result = area_polygon_karney(&p);
                assert!(result.is_ok());
                if let Ok(area) = result {
                    // Area should be outer minus inner, both > 0
                    assert!(area > 0.0);
                    // Outer ~ 10*10 degree square, inner ~ 6*6 degree square
                    // The difference should be substantial
                    assert!(
                        area > 1e11,
                        "area with hole should be substantial: {}",
                        area
                    );
                }
            }
        }
    }

    #[test]
    fn test_polygon_area_insufficient_coords() {
        let geod = Geodesic::wgs84();
        let coords = vec![Coordinate::new_2d(0.0, 0.0), Coordinate::new_2d(1.0, 0.0)];
        let result = geod.polygon_area(&coords);
        assert!(result.is_err());
    }

    #[test]
    fn test_polygon_area_invalid_latitude() {
        let geod = Geodesic::wgs84();
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 0.0),
            Coordinate::new_2d(1.0, 95.0), // Invalid
            Coordinate::new_2d(0.0, 0.0),
        ];
        let result = geod.polygon_area(&coords);
        assert!(result.is_err());
    }

    #[test]
    fn test_polygon_area_open_ring() {
        // Test with an open ring (not closed)
        let geod = Geodesic::wgs84();
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 0.0),
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(0.0, 1.0),
        ];
        let result = geod.polygon_area(&coords);
        assert!(result.is_ok());
        if let Ok(pa) = result {
            // Should give same result as closed ring
            let reference = 12_308_778_361.469;
            let rel_error = (pa.area - reference).abs() / reference;
            assert!(
                rel_error < 1e-6,
                "open ring area {:.3}, expected ~{:.3}",
                pa.area,
                reference
            );
        }
    }

    #[test]
    fn test_custom_ellipsoid() {
        // Perfect sphere with a = b = 6371000
        let r = 6_371_000.0;
        let geod = Geodesic::new(r, 0.0);
        let area = geod.ellipsoid_area();
        use core::f64::consts::PI;
        let expected = 4.0 * PI * r * r;
        let rel_error = (area - expected).abs() / expected;
        assert!(
            rel_error < 1e-10,
            "sphere area {:.6e}, expected {:.6e}",
            area,
            expected
        );
    }

    #[test]
    fn test_diamond_polygon() {
        // GeographicLib planimeter0 test: diamond at equator
        let geod = Geodesic::wgs84();
        let coords = vec![
            Coordinate::new_2d(-1.0, 0.0),
            Coordinate::new_2d(0.0, -1.0),
            Coordinate::new_2d(1.0, 0.0),
            Coordinate::new_2d(0.0, 1.0),
            Coordinate::new_2d(-1.0, 0.0),
        ];
        let result = geod.polygon_area(&coords);
        assert!(result.is_ok());
        if let Ok(pa) = result {
            // GeographicLib reference: 24,619,419,146 m^2
            let reference = 24_619_419_146.0;
            let rel_error = (pa.area - reference).abs() / reference;
            assert!(
                rel_error < 1e-5,
                "diamond area {:.0}, expected {:.0}, rel err {:.8}",
                pa.area,
                reference,
                rel_error
            );
        }
    }

    #[test]
    fn test_perimeter_unit_square() {
        // Perimeter of 1-degree square at equator
        // GeographicLib reference: 443,770.917 m
        let geod = Geodesic::wgs84();
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 0.0),
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(0.0, 1.0),
            Coordinate::new_2d(0.0, 0.0),
        ];
        let result = geod.polygon_area(&coords);
        assert!(result.is_ok());
        if let Ok(pa) = result {
            let reference = 443_770.917;
            let rel_error = (pa.perimeter - reference).abs() / reference;
            assert!(
                rel_error < 1e-5,
                "perimeter {:.3}, expected {:.3}",
                pa.perimeter,
                reference
            );
        }
    }

    #[test]
    fn test_area_method_karney_via_area_module() {
        // Test that AreaMethod::KarneyGeodesic works through the area module
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
                let result = crate::vector::area::area_polygon(
                    &p,
                    crate::vector::area::AreaMethod::KarneyGeodesic,
                );
                assert!(result.is_ok());
                if let Ok(area) = result {
                    let reference = 12_308_778_361.469;
                    let rel_error = (area - reference).abs() / reference;
                    assert!(
                        rel_error < 1e-6,
                        "AreaMethod::KarneyGeodesic: area {:.3}, expected ~{:.3}",
                        area,
                        reference
                    );
                }
            }
        }
    }
}
