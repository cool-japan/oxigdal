//! Centroid calculation for geometric features
//!
//! This module provides functions for computing centroids (geometric centers)
//! of various geometry types using different methods.
//!
//! # Centroid Types
//!
//! - **Geometric Centroid**: Simple average of coordinates (for points and lines)
//! - **Area-Weighted Centroid**: Center of mass for polygons (accounts for area distribution)
//! - **3D Centroid**: Centroid calculation including Z coordinates
//!
//! # Examples
//!
//! ```
//! use oxigdal_algorithms::vector::{Coordinate, Polygon, LineString, centroid_polygon};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let coords = vec![
//!     Coordinate::new_2d(0.0, 0.0),
//!     Coordinate::new_2d(4.0, 0.0),
//!     Coordinate::new_2d(4.0, 4.0),
//!     Coordinate::new_2d(0.0, 4.0),
//!     Coordinate::new_2d(0.0, 0.0),
//! ];
//! let exterior = LineString::new(coords)?;
//! let polygon = Polygon::new(exterior, vec![])?;
//! let center = centroid_polygon(&polygon)?;
//! // Center should be at (2.0, 2.0)
//! # Ok(())
//! # }
//! ```

use crate::error::{AlgorithmError, Result};
use oxigdal_core::vector::{
    Coordinate, Geometry, GeometryCollection, LineString, MultiLineString, MultiPoint,
    MultiPolygon, Point, Polygon,
};

/// Computes the centroid of any geometry type
///
/// # Arguments
///
/// * `geometry` - Input geometry
///
/// # Returns
///
/// Point representing the centroid
///
/// # Errors
///
/// Returns error if geometry is empty or invalid
pub fn centroid(geometry: &Geometry) -> Result<Point> {
    match geometry {
        Geometry::Point(p) => Ok(p.clone()),
        Geometry::LineString(ls) => centroid_linestring(ls),
        Geometry::Polygon(p) => centroid_polygon(p),
        Geometry::MultiPoint(mp) => centroid_multipoint(mp),
        Geometry::MultiLineString(mls) => centroid_multilinestring(mls),
        Geometry::MultiPolygon(mp) => centroid_multipolygon(mp),
        Geometry::GeometryCollection(gc) => centroid_collection(gc),
    }
}

/// Computes the centroid of a point (returns the point itself)
///
/// # Arguments
///
/// * `point` - Input point
///
/// # Returns
///
/// The point itself as its own centroid
pub fn centroid_point(point: &Point) -> Point {
    point.clone()
}

/// Computes the geometric centroid of a linestring
///
/// The centroid is computed as the weighted average of coordinates,
/// where weights are the lengths of segments ending at each coordinate.
///
/// # Arguments
///
/// * `linestring` - Input linestring
///
/// # Returns
///
/// Point representing the centroid
///
/// # Errors
///
/// Returns error if linestring is empty
pub fn centroid_linestring(linestring: &LineString) -> Result<Point> {
    if linestring.coords.is_empty() {
        return Err(AlgorithmError::EmptyInput {
            operation: "centroid_linestring",
        });
    }

    if linestring.coords.len() == 1 {
        return Ok(Point::from_coord(linestring.coords[0]));
    }

    let mut total_length = 0.0;
    let mut weighted_x = 0.0;
    let mut weighted_y = 0.0;
    let mut weighted_z = 0.0;
    let has_z = linestring.coords[0].has_z();

    for i in 0..linestring.coords.len() - 1 {
        let p1 = &linestring.coords[i];
        let p2 = &linestring.coords[i + 1];

        let dx = p2.x - p1.x;
        let dy = p2.y - p1.y;
        let dz = if has_z {
            p2.z.unwrap_or(0.0) - p1.z.unwrap_or(0.0)
        } else {
            0.0
        };

        let length = (dx * dx + dy * dy + dz * dz).sqrt();

        if length > 0.0 {
            // Use midpoint of segment weighted by length
            let mid_x = (p1.x + p2.x) / 2.0;
            let mid_y = (p1.y + p2.y) / 2.0;

            weighted_x += mid_x * length;
            weighted_y += mid_y * length;

            if has_z {
                let mid_z = (p1.z.unwrap_or(0.0) + p2.z.unwrap_or(0.0)) / 2.0;
                weighted_z += mid_z * length;
            }

            total_length += length;
        }
    }

    if total_length == 0.0 {
        // All points are the same - return the first point
        return Ok(Point::from_coord(linestring.coords[0]));
    }

    let centroid_coord = if has_z {
        Coordinate::new_3d(
            weighted_x / total_length,
            weighted_y / total_length,
            weighted_z / total_length,
        )
    } else {
        Coordinate::new_2d(weighted_x / total_length, weighted_y / total_length)
    };

    Ok(Point::from_coord(centroid_coord))
}

/// Computes the area-weighted centroid of a polygon
///
/// Uses the signed area method to compute the true geometric centroid
/// accounting for the area distribution within the polygon.
///
/// # Arguments
///
/// * `polygon` - Input polygon
///
/// # Returns
///
/// Point representing the centroid
///
/// # Errors
///
/// Returns error if polygon is invalid or has zero area
pub fn centroid_polygon(polygon: &Polygon) -> Result<Point> {
    if polygon.exterior.coords.len() < 3 {
        return Err(AlgorithmError::InsufficientData {
            operation: "centroid_polygon",
            message: "polygon must have at least 3 coordinates".to_string(),
        });
    }

    // Compute centroid of exterior ring
    let (ext_area, ext_cx, ext_cy) = ring_centroid(&polygon.exterior.coords)?;

    if ext_area.abs() < f64::EPSILON {
        return Err(AlgorithmError::GeometryError {
            message: "polygon has zero area".to_string(),
        });
    }

    let mut total_area = ext_area;
    let mut weighted_x = ext_cx * ext_area;
    let mut weighted_y = ext_cy * ext_area;

    // Subtract hole contributions
    for hole in &polygon.interiors {
        let (hole_area, hole_cx, hole_cy) = ring_centroid(&hole.coords)?;

        total_area -= hole_area;
        weighted_x -= hole_cx * hole_area;
        weighted_y -= hole_cy * hole_area;
    }

    if total_area.abs() < f64::EPSILON {
        return Err(AlgorithmError::GeometryError {
            message: "polygon has zero effective area after holes".to_string(),
        });
    }

    let centroid_coord = Coordinate::new_2d(weighted_x / total_area, weighted_y / total_area);

    Ok(Point::from_coord(centroid_coord))
}

/// Computes the centroid of a multipoint
///
/// # Arguments
///
/// * `multipoint` - Input multipoint
///
/// # Returns
///
/// Point representing the average of all points
///
/// # Errors
///
/// Returns error if multipoint is empty
pub fn centroid_multipoint(multipoint: &MultiPoint) -> Result<Point> {
    if multipoint.points.is_empty() {
        return Err(AlgorithmError::EmptyInput {
            operation: "centroid_multipoint",
        });
    }

    let mut sum_x = 0.0;
    let mut sum_y = 0.0;
    let mut sum_z = 0.0;
    let has_z = multipoint.points[0].coord.has_z();

    for point in &multipoint.points {
        sum_x += point.coord.x;
        sum_y += point.coord.y;

        if has_z {
            sum_z += point.coord.z.unwrap_or(0.0);
        }
    }

    let n = multipoint.points.len() as f64;
    let centroid_coord = if has_z {
        Coordinate::new_3d(sum_x / n, sum_y / n, sum_z / n)
    } else {
        Coordinate::new_2d(sum_x / n, sum_y / n)
    };

    Ok(Point::from_coord(centroid_coord))
}

/// Computes the centroid of a multilinestring
///
/// # Arguments
///
/// * `multilinestring` - Input multilinestring
///
/// # Returns
///
/// Point representing the weighted centroid of all linestrings
///
/// # Errors
///
/// Returns error if multilinestring is empty
pub fn centroid_multilinestring(multilinestring: &MultiLineString) -> Result<Point> {
    if multilinestring.line_strings.is_empty() {
        return Err(AlgorithmError::EmptyInput {
            operation: "centroid_multilinestring",
        });
    }

    let mut total_length = 0.0;
    let mut weighted_x = 0.0;
    let mut weighted_y = 0.0;

    for linestring in &multilinestring.line_strings {
        let ls_centroid = centroid_linestring(linestring)?;
        let length = linestring_length(linestring);

        weighted_x += ls_centroid.coord.x * length;
        weighted_y += ls_centroid.coord.y * length;
        total_length += length;
    }

    if total_length == 0.0 {
        return Err(AlgorithmError::GeometryError {
            message: "multilinestring has zero total length".to_string(),
        });
    }

    let centroid_coord = Coordinate::new_2d(weighted_x / total_length, weighted_y / total_length);

    Ok(Point::from_coord(centroid_coord))
}

/// Computes the centroid of a multipolygon
///
/// # Arguments
///
/// * `multipolygon` - Input multipolygon
///
/// # Returns
///
/// Point representing the area-weighted centroid of all polygons
///
/// # Errors
///
/// Returns error if multipolygon is empty or has zero total area
pub fn centroid_multipolygon(multipolygon: &MultiPolygon) -> Result<Point> {
    if multipolygon.polygons.is_empty() {
        return Err(AlgorithmError::EmptyInput {
            operation: "centroid_multipolygon",
        });
    }

    let mut total_area = 0.0;
    let mut weighted_x = 0.0;
    let mut weighted_y = 0.0;

    for polygon in &multipolygon.polygons {
        let poly_centroid = centroid_polygon(polygon)?;
        let area = polygon_area(polygon);

        weighted_x += poly_centroid.coord.x * area;
        weighted_y += poly_centroid.coord.y * area;
        total_area += area;
    }

    if total_area.abs() < f64::EPSILON {
        return Err(AlgorithmError::GeometryError {
            message: "multipolygon has zero total area".to_string(),
        });
    }

    let centroid_coord = Coordinate::new_2d(weighted_x / total_area, weighted_y / total_area);

    Ok(Point::from_coord(centroid_coord))
}

/// Computes the centroid of a geometry collection
///
/// # Arguments
///
/// * `collection` - Input geometry collection
///
/// # Returns
///
/// Point representing the centroid of all geometries
///
/// # Errors
///
/// Returns error if collection is empty
pub fn centroid_collection(collection: &GeometryCollection) -> Result<Point> {
    if collection.geometries.is_empty() {
        return Err(AlgorithmError::EmptyInput {
            operation: "centroid_collection",
        });
    }

    // Compute weighted centroid based on geometry types
    let mut total_weight = 0.0;
    let mut weighted_x = 0.0;
    let mut weighted_y = 0.0;

    for geometry in &collection.geometries {
        let geom_centroid = centroid(geometry)?;
        let weight = geometry_weight(geometry);

        weighted_x += geom_centroid.coord.x * weight;
        weighted_y += geom_centroid.coord.y * weight;
        total_weight += weight;
    }

    if total_weight == 0.0 {
        return Err(AlgorithmError::GeometryError {
            message: "collection has zero total weight".to_string(),
        });
    }

    let centroid_coord = Coordinate::new_2d(weighted_x / total_weight, weighted_y / total_weight);

    Ok(Point::from_coord(centroid_coord))
}

/// Computes the centroid and area of a polygon ring
fn ring_centroid(coords: &[Coordinate]) -> Result<(f64, f64, f64)> {
    if coords.len() < 3 {
        return Err(AlgorithmError::InsufficientData {
            operation: "ring_centroid",
            message: "ring must have at least 3 coordinates".to_string(),
        });
    }

    let mut area = 0.0;
    let mut cx = 0.0;
    let mut cy = 0.0;

    let n = coords.len();
    for i in 0..n {
        let j = (i + 1) % n;
        let cross = coords[i].x * coords[j].y - coords[j].x * coords[i].y;

        area += cross;
        cx += (coords[i].x + coords[j].x) * cross;
        cy += (coords[i].y + coords[j].y) * cross;
    }

    area /= 2.0;

    if area.abs() < f64::EPSILON {
        // Degenerate polygon - use simple average
        let mut sum_x = 0.0;
        let mut sum_y = 0.0;
        for coord in coords {
            sum_x += coord.x;
            sum_y += coord.y;
        }
        let n = coords.len() as f64;
        return Ok((0.0, sum_x / n, sum_y / n));
    }

    cx /= 6.0 * area;
    cy /= 6.0 * area;

    Ok((area, cx, cy))
}

/// Computes the length of a linestring
fn linestring_length(linestring: &LineString) -> f64 {
    let mut length = 0.0;

    for i in 0..linestring.coords.len().saturating_sub(1) {
        let p1 = &linestring.coords[i];
        let p2 = &linestring.coords[i + 1];

        let dx = p2.x - p1.x;
        let dy = p2.y - p1.y;

        length += (dx * dx + dy * dy).sqrt();
    }

    length
}

/// Computes the area of a polygon
fn polygon_area(polygon: &Polygon) -> f64 {
    let mut area = ring_area(&polygon.exterior.coords);

    for hole in &polygon.interiors {
        area -= ring_area(&hole.coords);
    }

    area.abs()
}

/// Computes the signed area of a ring
fn ring_area(coords: &[Coordinate]) -> f64 {
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

/// Assigns a weight to a geometry for weighted centroid calculation
fn geometry_weight(geometry: &Geometry) -> f64 {
    match geometry {
        Geometry::Point(_) => 1.0,
        Geometry::LineString(ls) => linestring_length(ls).max(1.0),
        Geometry::Polygon(p) => polygon_area(p).max(1.0),
        Geometry::MultiPoint(mp) => mp.points.len() as f64,
        Geometry::MultiLineString(mls) => mls
            .line_strings
            .iter()
            .map(linestring_length)
            .sum::<f64>()
            .max(1.0),
        Geometry::MultiPolygon(mp) => mp.polygons.iter().map(polygon_area).sum::<f64>().max(1.0),
        Geometry::GeometryCollection(gc) => gc
            .geometries
            .iter()
            .map(geometry_weight)
            .sum::<f64>()
            .max(1.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_square() -> Result<Polygon> {
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(4.0, 0.0),
            Coordinate::new_2d(4.0, 4.0),
            Coordinate::new_2d(0.0, 4.0),
            Coordinate::new_2d(0.0, 0.0),
        ];
        let exterior = LineString::new(coords).map_err(|e| AlgorithmError::Core(e))?;
        Polygon::new(exterior, vec![]).map_err(|e| AlgorithmError::Core(e))
    }

    #[test]
    fn test_centroid_point() {
        let point = Point::new(3.0, 5.0);
        let result = centroid_point(&point);

        assert_eq!(result.coord.x, 3.0);
        assert_eq!(result.coord.y, 5.0);
    }

    #[test]
    fn test_centroid_linestring() {
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(4.0, 0.0),
            Coordinate::new_2d(4.0, 4.0),
        ];
        let line = LineString::new(coords);
        assert!(line.is_ok());

        if let Ok(l) = line {
            let result = centroid_linestring(&l);
            assert!(result.is_ok());

            if let Ok(centroid) = result {
                // Centroid should be somewhere along the path
                assert!(centroid.coord.x >= 0.0 && centroid.coord.x <= 4.0);
                assert!(centroid.coord.y >= 0.0 && centroid.coord.y <= 4.0);
            }
        }
    }

    #[test]
    fn test_centroid_polygon_square() {
        let poly = create_square();
        assert!(poly.is_ok());

        if let Ok(p) = poly {
            let result = centroid_polygon(&p);
            assert!(result.is_ok());

            if let Ok(centroid) = result {
                // Square centroid should be at (2, 2)
                assert!((centroid.coord.x - 2.0).abs() < 1e-10);
                assert!((centroid.coord.y - 2.0).abs() < 1e-10);
            }
        }
    }

    #[test]
    fn test_centroid_polygon_with_hole() {
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

        let exterior = LineString::new(exterior_coords);
        let hole = LineString::new(hole_coords);

        assert!(exterior.is_ok() && hole.is_ok());

        if let (Ok(ext), Ok(h)) = (exterior, hole) {
            let poly = Polygon::new(ext, vec![h]);
            assert!(poly.is_ok());

            if let Ok(p) = poly {
                let result = centroid_polygon(&p);
                assert!(result.is_ok());

                if let Ok(centroid) = result {
                    // Centroid should still be near (5, 5) but exact value depends on hole size
                    assert!(centroid.coord.x >= 0.0 && centroid.coord.x <= 10.0);
                    assert!(centroid.coord.y >= 0.0 && centroid.coord.y <= 10.0);
                }
            }
        }
    }

    #[test]
    fn test_centroid_multipoint() {
        let points = vec![
            Point::new(0.0, 0.0),
            Point::new(4.0, 0.0),
            Point::new(4.0, 4.0),
            Point::new(0.0, 4.0),
        ];
        let mp = MultiPoint::new(points);

        let result = centroid_multipoint(&mp);
        assert!(result.is_ok());

        if let Ok(centroid) = result {
            // Average of 4 corners of a square
            assert!((centroid.coord.x - 2.0).abs() < 1e-10);
            assert!((centroid.coord.y - 2.0).abs() < 1e-10);
        }
    }

    #[test]
    fn test_centroid_empty_multipoint() {
        let mp = MultiPoint::empty();
        let result = centroid_multipoint(&mp);
        assert!(result.is_err());
    }

    #[test]
    fn test_linestring_length() {
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(3.0, 0.0),
            Coordinate::new_2d(3.0, 4.0),
        ];
        let line = LineString::new(coords);
        assert!(line.is_ok());

        if let Ok(l) = line {
            let length = linestring_length(&l);
            assert!((length - 7.0).abs() < 1e-10); // 3 + 4 = 7
        }
    }

    #[test]
    fn test_polygon_area() {
        let poly = create_square();
        assert!(poly.is_ok());

        if let Ok(p) = poly {
            let area = polygon_area(&p);
            assert!((area - 16.0).abs() < 1e-10); // 4x4 = 16
        }
    }

    #[test]
    fn test_ring_centroid() {
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(4.0, 0.0),
            Coordinate::new_2d(4.0, 4.0),
            Coordinate::new_2d(0.0, 4.0),
            Coordinate::new_2d(0.0, 0.0),
        ];

        let result = ring_centroid(&coords);
        assert!(result.is_ok());

        if let Ok((area, cx, cy)) = result {
            assert!((area - 16.0).abs() < 1e-10);
            assert!((cx - 2.0).abs() < 1e-10);
            assert!((cy - 2.0).abs() < 1e-10);
        }
    }
}
