//! Envelope (bounding box) calculation for geometric features
//!
//! This module provides functions for computing envelopes (axis-aligned bounding boxes)
//! of various geometry types as polygon geometries.
//!
//! # Envelope vs Bounds
//!
//! - **Bounds**: Returns tuple (min_x, min_y, max_x, max_y) - available in core
//! - **Envelope**: Returns a rectangular Polygon geometry representing the bounding box
//!
//! # Examples
//!
//! ```
//! # use oxigdal_algorithms::error::Result;
//! use oxigdal_algorithms::vector::{Coordinate, LineString, envelope_linestring};
//!
//! # fn main() -> Result<()> {
//! let coords = vec![
//!     Coordinate::new_2d(0.0, 0.0),
//!     Coordinate::new_2d(4.0, 2.0),
//!     Coordinate::new_2d(2.0, 5.0),
//! ];
//! let line = LineString::new(coords)?;
//! let env = envelope_linestring(&line)?;
//! // Envelope will be a rectangle from (0,0) to (4,5)
//! # Ok(())
//! # }
//! ```

use crate::error::{AlgorithmError, Result};
use oxigdal_core::vector::{
    Coordinate, Geometry, GeometryCollection, LineString, MultiLineString, MultiPoint,
    MultiPolygon, Point, Polygon,
};

/// Computes the envelope of any geometry type
///
/// Returns a rectangular polygon that represents the axis-aligned bounding box
/// of the input geometry.
///
/// # Arguments
///
/// * `geometry` - Input geometry
///
/// # Returns
///
/// Polygon representing the envelope (bounding box)
///
/// # Errors
///
/// Returns error if geometry is empty or invalid
///
/// # Examples
///
/// ```
/// # use oxigdal_algorithms::error::Result;
/// use oxigdal_core::vector::Geometry;
/// use oxigdal_algorithms::vector::{Point, envelope};
///
/// # fn main() -> Result<()> {
/// let point = Point::new(3.0, 5.0);
/// let geom = Geometry::Point(point);
/// let env = envelope(&geom)?;
/// // For a point, envelope is a degenerate rectangle at that point
/// # Ok(())
/// # }
/// ```
pub fn envelope(geometry: &Geometry) -> Result<Polygon> {
    match geometry {
        Geometry::Point(p) => envelope_point(p),
        Geometry::LineString(ls) => envelope_linestring(ls),
        Geometry::Polygon(p) => envelope_polygon(p),
        Geometry::MultiPoint(mp) => envelope_multipoint(mp),
        Geometry::MultiLineString(mls) => envelope_multilinestring(mls),
        Geometry::MultiPolygon(mp) => envelope_multipolygon(mp),
        Geometry::GeometryCollection(gc) => envelope_collection(gc),
    }
}

/// Computes the envelope of a point
///
/// For a single point, the envelope is a degenerate rectangle (all four corners
/// at the same location).
///
/// # Arguments
///
/// * `point` - Input point
///
/// # Returns
///
/// Polygon representing the envelope
///
/// # Errors
///
/// Returns error if polygon creation fails
pub fn envelope_point(point: &Point) -> Result<Polygon> {
    let x = point.coord.x;
    let y = point.coord.y;

    // Create a degenerate rectangle (all corners at the same point)
    let coords = vec![
        Coordinate::new_2d(x, y),
        Coordinate::new_2d(x, y),
        Coordinate::new_2d(x, y),
        Coordinate::new_2d(x, y),
        Coordinate::new_2d(x, y),
    ];

    let exterior = LineString::new(coords).map_err(AlgorithmError::Core)?;
    Polygon::new(exterior, vec![]).map_err(AlgorithmError::Core)
}

/// Computes the envelope of a linestring
///
/// Returns a rectangular polygon that bounds all points in the linestring.
///
/// # Arguments
///
/// * `linestring` - Input linestring
///
/// # Returns
///
/// Polygon representing the envelope
///
/// # Errors
///
/// Returns error if linestring is empty or envelope creation fails
pub fn envelope_linestring(linestring: &LineString) -> Result<Polygon> {
    if linestring.coords.is_empty() {
        return Err(AlgorithmError::EmptyInput {
            operation: "envelope_linestring",
        });
    }

    let bounds = linestring
        .bounds()
        .ok_or_else(|| AlgorithmError::GeometryError {
            message: "failed to compute bounds for linestring".to_string(),
        })?;

    create_envelope_polygon(bounds)
}

/// Computes the envelope of a polygon
///
/// Returns a rectangular polygon that bounds the input polygon.
///
/// # Arguments
///
/// * `polygon` - Input polygon
///
/// # Returns
///
/// Polygon representing the envelope
///
/// # Errors
///
/// Returns error if polygon is invalid or envelope creation fails
pub fn envelope_polygon(polygon: &Polygon) -> Result<Polygon> {
    if polygon.exterior.coords.is_empty() {
        return Err(AlgorithmError::EmptyInput {
            operation: "envelope_polygon",
        });
    }

    let bounds = polygon
        .bounds()
        .ok_or_else(|| AlgorithmError::GeometryError {
            message: "failed to compute bounds for polygon".to_string(),
        })?;

    create_envelope_polygon(bounds)
}

/// Computes the envelope of a multipoint
///
/// Returns a rectangular polygon that bounds all points.
///
/// # Arguments
///
/// * `multipoint` - Input multipoint
///
/// # Returns
///
/// Polygon representing the envelope
///
/// # Errors
///
/// Returns error if multipoint is empty or envelope creation fails
pub fn envelope_multipoint(multipoint: &MultiPoint) -> Result<Polygon> {
    if multipoint.points.is_empty() {
        return Err(AlgorithmError::EmptyInput {
            operation: "envelope_multipoint",
        });
    }

    let bounds = multipoint
        .bounds()
        .ok_or_else(|| AlgorithmError::GeometryError {
            message: "failed to compute bounds for multipoint".to_string(),
        })?;

    create_envelope_polygon(bounds)
}

/// Computes the envelope of a multilinestring
///
/// Returns a rectangular polygon that bounds all linestrings.
///
/// # Arguments
///
/// * `multilinestring` - Input multilinestring
///
/// # Returns
///
/// Polygon representing the envelope
///
/// # Errors
///
/// Returns error if multilinestring is empty or envelope creation fails
pub fn envelope_multilinestring(multilinestring: &MultiLineString) -> Result<Polygon> {
    if multilinestring.line_strings.is_empty() {
        return Err(AlgorithmError::EmptyInput {
            operation: "envelope_multilinestring",
        });
    }

    let bounds = multilinestring
        .bounds()
        .ok_or_else(|| AlgorithmError::GeometryError {
            message: "failed to compute bounds for multilinestring".to_string(),
        })?;

    create_envelope_polygon(bounds)
}

/// Computes the envelope of a multipolygon
///
/// Returns a rectangular polygon that bounds all polygons.
///
/// # Arguments
///
/// * `multipolygon` - Input multipolygon
///
/// # Returns
///
/// Polygon representing the envelope
///
/// # Errors
///
/// Returns error if multipolygon is empty or envelope creation fails
pub fn envelope_multipolygon(multipolygon: &MultiPolygon) -> Result<Polygon> {
    if multipolygon.polygons.is_empty() {
        return Err(AlgorithmError::EmptyInput {
            operation: "envelope_multipolygon",
        });
    }

    let bounds = multipolygon
        .bounds()
        .ok_or_else(|| AlgorithmError::GeometryError {
            message: "failed to compute bounds for multipolygon".to_string(),
        })?;

    create_envelope_polygon(bounds)
}

/// Computes the envelope of a geometry collection
///
/// Returns a rectangular polygon that bounds all geometries in the collection.
///
/// # Arguments
///
/// * `collection` - Input geometry collection
///
/// # Returns
///
/// Polygon representing the envelope
///
/// # Errors
///
/// Returns error if collection is empty or envelope creation fails
pub fn envelope_collection(collection: &GeometryCollection) -> Result<Polygon> {
    if collection.geometries.is_empty() {
        return Err(AlgorithmError::EmptyInput {
            operation: "envelope_collection",
        });
    }

    let bounds = collection
        .bounds()
        .ok_or_else(|| AlgorithmError::GeometryError {
            message: "failed to compute bounds for geometry collection".to_string(),
        })?;

    create_envelope_polygon(bounds)
}

/// Creates a rectangular polygon from bounds
///
/// # Arguments
///
/// * `bounds` - Tuple of (min_x, min_y, max_x, max_y)
///
/// # Returns
///
/// Polygon representing the envelope
///
/// # Errors
///
/// Returns error if polygon creation fails
fn create_envelope_polygon(bounds: (f64, f64, f64, f64)) -> Result<Polygon> {
    let (min_x, min_y, max_x, max_y) = bounds;

    // Create rectangle corners (counter-clockwise)
    let coords = vec![
        Coordinate::new_2d(min_x, min_y),
        Coordinate::new_2d(max_x, min_y),
        Coordinate::new_2d(max_x, max_y),
        Coordinate::new_2d(min_x, max_y),
        Coordinate::new_2d(min_x, min_y), // Close the ring
    ];

    let exterior = LineString::new(coords).map_err(AlgorithmError::Core)?;
    Polygon::new(exterior, vec![]).map_err(AlgorithmError::Core)
}

/// Computes expanded envelope with buffer
///
/// Creates an envelope that is expanded by a specified distance in all directions.
///
/// # Arguments
///
/// * `geometry` - Input geometry
/// * `buffer` - Distance to expand envelope in all directions
///
/// # Returns
///
/// Expanded envelope polygon
///
/// # Errors
///
/// Returns error if geometry is invalid or buffer is negative
///
/// # Examples
///
/// ```
/// # use oxigdal_algorithms::error::Result;
/// use oxigdal_core::vector::Geometry;
/// use oxigdal_algorithms::vector::{Point, envelope_with_buffer};
///
/// # fn main() -> Result<()> {
/// let point = Point::new(5.0, 5.0);
/// let geom = Geometry::Point(point);
/// let env = envelope_with_buffer(&geom, 1.0)?;
/// // Envelope will be rectangle from (4,4) to (6,6)
/// # Ok(())
/// # }
/// ```
pub fn envelope_with_buffer(geometry: &Geometry, buffer: f64) -> Result<Polygon> {
    if buffer < 0.0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffer",
            message: "buffer must be non-negative".to_string(),
        });
    }

    let base_envelope = envelope(geometry)?;
    let bounds = base_envelope
        .bounds()
        .ok_or_else(|| AlgorithmError::GeometryError {
            message: "failed to compute bounds for envelope".to_string(),
        })?;

    let (min_x, min_y, max_x, max_y) = bounds;

    // Expand bounds by buffer
    let expanded_bounds = (
        min_x - buffer,
        min_y - buffer,
        max_x + buffer,
        max_y + buffer,
    );

    create_envelope_polygon(expanded_bounds)
}

/// Checks if an envelope contains a point
///
/// # Arguments
///
/// * `envelope` - The envelope polygon
/// * `point` - Point to test
///
/// # Returns
///
/// True if the point is within or on the boundary of the envelope
pub fn envelope_contains_point(envelope: &Polygon, point: &Point) -> bool {
    if let Some((min_x, min_y, max_x, max_y)) = envelope.bounds() {
        point.coord.x >= min_x
            && point.coord.x <= max_x
            && point.coord.y >= min_y
            && point.coord.y <= max_y
    } else {
        false
    }
}

/// Checks if two envelopes intersect
///
/// # Arguments
///
/// * `env1` - First envelope polygon
/// * `env2` - Second envelope polygon
///
/// # Returns
///
/// True if the envelopes intersect or touch
pub fn envelopes_intersect(env1: &Polygon, env2: &Polygon) -> bool {
    if let (Some(b1), Some(b2)) = (env1.bounds(), env2.bounds()) {
        let (min_x1, min_y1, max_x1, max_y1) = b1;
        let (min_x2, min_y2, max_x2, max_y2) = b2;

        // Check if bounding boxes overlap
        !(max_x1 < min_x2 || max_x2 < min_x1 || max_y1 < min_y2 || max_y2 < min_y1)
    } else {
        false
    }
}

/// Computes the union of two envelopes
///
/// Returns the smallest envelope that contains both input envelopes.
///
/// # Arguments
///
/// * `env1` - First envelope polygon
/// * `env2` - Second envelope polygon
///
/// # Returns
///
/// Envelope polygon containing both input envelopes
///
/// # Errors
///
/// Returns error if envelope creation fails
pub fn envelope_union(env1: &Polygon, env2: &Polygon) -> Result<Polygon> {
    let b1 = env1.bounds().ok_or_else(|| AlgorithmError::GeometryError {
        message: "failed to compute bounds for first envelope".to_string(),
    })?;

    let b2 = env2.bounds().ok_or_else(|| AlgorithmError::GeometryError {
        message: "failed to compute bounds for second envelope".to_string(),
    })?;

    let (min_x1, min_y1, max_x1, max_y1) = b1;
    let (min_x2, min_y2, max_x2, max_y2) = b2;

    let union_bounds = (
        min_x1.min(min_x2),
        min_y1.min(min_y2),
        max_x1.max(max_x2),
        max_y1.max(max_y2),
    );

    create_envelope_polygon(union_bounds)
}

/// Computes the intersection of two envelopes
///
/// Returns the envelope representing the overlapping region, or None if they don't intersect.
///
/// # Arguments
///
/// * `env1` - First envelope polygon
/// * `env2` - Second envelope polygon
///
/// # Returns
///
/// Some(envelope) if they intersect, None otherwise
///
/// # Errors
///
/// Returns error if envelope creation fails
pub fn envelope_intersection(env1: &Polygon, env2: &Polygon) -> Result<Option<Polygon>> {
    if !envelopes_intersect(env1, env2) {
        return Ok(None);
    }

    let b1 = env1.bounds().ok_or_else(|| AlgorithmError::GeometryError {
        message: "failed to compute bounds for first envelope".to_string(),
    })?;

    let b2 = env2.bounds().ok_or_else(|| AlgorithmError::GeometryError {
        message: "failed to compute bounds for second envelope".to_string(),
    })?;

    let (min_x1, min_y1, max_x1, max_y1) = b1;
    let (min_x2, min_y2, max_x2, max_y2) = b2;

    let intersection_bounds = (
        min_x1.max(min_x2),
        min_y1.max(min_y2),
        max_x1.min(max_x2),
        max_y1.min(max_y2),
    );

    // Verify the intersection is valid
    if intersection_bounds.0 <= intersection_bounds.2
        && intersection_bounds.1 <= intersection_bounds.3
    {
        Ok(Some(create_envelope_polygon(intersection_bounds)?))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_linestring() -> Result<LineString> {
        let coords = vec![
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(5.0, 3.0),
            Coordinate::new_2d(3.0, 7.0),
        ];
        LineString::new(coords).map_err(AlgorithmError::Core)
    }

    fn create_test_polygon() -> Result<Polygon> {
        let coords = vec![
            Coordinate::new_2d(2.0, 2.0),
            Coordinate::new_2d(8.0, 2.0),
            Coordinate::new_2d(8.0, 6.0),
            Coordinate::new_2d(2.0, 6.0),
            Coordinate::new_2d(2.0, 2.0),
        ];
        let exterior = LineString::new(coords).map_err(AlgorithmError::Core)?;
        Polygon::new(exterior, vec![]).map_err(AlgorithmError::Core)
    }

    #[test]
    fn test_envelope_point() {
        let point = Point::new(3.0, 5.0);
        let env = envelope_point(&point);

        assert!(env.is_ok());
        if let Ok(e) = env {
            let bounds = e.bounds();
            assert!(bounds.is_some());
            if let Some((min_x, min_y, max_x, max_y)) = bounds {
                assert_eq!(min_x, 3.0);
                assert_eq!(min_y, 5.0);
                assert_eq!(max_x, 3.0);
                assert_eq!(max_y, 5.0);
            }
        }
    }

    #[test]
    fn test_envelope_linestring() {
        let line = create_test_linestring();
        assert!(line.is_ok());

        if let Ok(l) = line {
            let env = envelope_linestring(&l);
            assert!(env.is_ok());

            if let Ok(e) = env {
                let bounds = e.bounds();
                assert!(bounds.is_some());
                if let Some((min_x, min_y, max_x, max_y)) = bounds {
                    assert_eq!(min_x, 1.0);
                    assert_eq!(min_y, 1.0);
                    assert_eq!(max_x, 5.0);
                    assert_eq!(max_y, 7.0);
                }
            }
        }
    }

    #[test]
    fn test_envelope_polygon() {
        let poly = create_test_polygon();
        assert!(poly.is_ok());

        if let Ok(p) = poly {
            let env = envelope_polygon(&p);
            assert!(env.is_ok());

            if let Ok(e) = env {
                let bounds = e.bounds();
                assert!(bounds.is_some());
                if let Some((min_x, min_y, max_x, max_y)) = bounds {
                    assert_eq!(min_x, 2.0);
                    assert_eq!(min_y, 2.0);
                    assert_eq!(max_x, 8.0);
                    assert_eq!(max_y, 6.0);
                }
            }
        }
    }

    #[test]
    fn test_envelope_multipoint() {
        let points = vec![
            Point::new(1.0, 1.0),
            Point::new(5.0, 3.0),
            Point::new(3.0, 7.0),
        ];
        let mp = MultiPoint::new(points);

        let env = envelope_multipoint(&mp);
        assert!(env.is_ok());

        if let Ok(e) = env {
            let bounds = e.bounds();
            assert!(bounds.is_some());
            if let Some((min_x, min_y, max_x, max_y)) = bounds {
                assert_eq!(min_x, 1.0);
                assert_eq!(min_y, 1.0);
                assert_eq!(max_x, 5.0);
                assert_eq!(max_y, 7.0);
            }
        }
    }

    #[test]
    fn test_envelope_with_buffer() {
        let point = Point::new(5.0, 5.0);
        let geom = Geometry::Point(point);

        let env = envelope_with_buffer(&geom, 2.0);
        assert!(env.is_ok());

        if let Ok(e) = env {
            let bounds = e.bounds();
            assert!(bounds.is_some());
            if let Some((min_x, min_y, max_x, max_y)) = bounds {
                assert_eq!(min_x, 3.0);
                assert_eq!(min_y, 3.0);
                assert_eq!(max_x, 7.0);
                assert_eq!(max_y, 7.0);
            }
        }
    }

    #[test]
    fn test_envelope_with_negative_buffer() {
        let point = Point::new(5.0, 5.0);
        let geom = Geometry::Point(point);

        let env = envelope_with_buffer(&geom, -1.0);
        assert!(env.is_err());
    }

    #[test]
    fn test_envelope_contains_point() {
        let poly = create_test_polygon();
        assert!(poly.is_ok());

        if let Ok(p) = poly {
            let env = envelope_polygon(&p);
            assert!(env.is_ok());

            if let Ok(e) = env {
                // Point inside
                let inside = Point::new(5.0, 4.0);
                assert!(envelope_contains_point(&e, &inside));

                // Point on boundary
                let boundary = Point::new(2.0, 4.0);
                assert!(envelope_contains_point(&e, &boundary));

                // Point outside
                let outside = Point::new(10.0, 10.0);
                assert!(!envelope_contains_point(&e, &outside));
            }
        }
    }

    #[test]
    fn test_envelopes_intersect() {
        let poly1 = create_test_polygon();
        let coords2 = vec![
            Coordinate::new_2d(5.0, 4.0),
            Coordinate::new_2d(10.0, 4.0),
            Coordinate::new_2d(10.0, 8.0),
            Coordinate::new_2d(5.0, 8.0),
            Coordinate::new_2d(5.0, 4.0),
        ];
        let ext2 = LineString::new(coords2);

        assert!(poly1.is_ok() && ext2.is_ok());
        if let (Ok(p1), Ok(e2)) = (poly1, ext2) {
            let poly2 = Polygon::new(e2, vec![]);
            assert!(poly2.is_ok());

            if let Ok(p2) = poly2 {
                let env1 = envelope_polygon(&p1);
                let env2 = envelope_polygon(&p2);

                assert!(env1.is_ok() && env2.is_ok());
                if let (Ok(e1), Ok(e2)) = (env1, env2) {
                    assert!(envelopes_intersect(&e1, &e2));
                }
            }
        }
    }

    #[test]
    fn test_envelopes_no_intersect() {
        let poly1 = create_test_polygon();
        let coords2 = vec![
            Coordinate::new_2d(20.0, 20.0),
            Coordinate::new_2d(25.0, 20.0),
            Coordinate::new_2d(25.0, 25.0),
            Coordinate::new_2d(20.0, 25.0),
            Coordinate::new_2d(20.0, 20.0),
        ];
        let ext2 = LineString::new(coords2);

        assert!(poly1.is_ok() && ext2.is_ok());
        if let (Ok(p1), Ok(e2)) = (poly1, ext2) {
            let poly2 = Polygon::new(e2, vec![]);
            assert!(poly2.is_ok());

            if let Ok(p2) = poly2 {
                let env1 = envelope_polygon(&p1);
                let env2 = envelope_polygon(&p2);

                assert!(env1.is_ok() && env2.is_ok());
                if let (Ok(e1), Ok(e2)) = (env1, env2) {
                    assert!(!envelopes_intersect(&e1, &e2));
                }
            }
        }
    }

    #[test]
    fn test_envelope_union() {
        let coords1 = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(5.0, 0.0),
            Coordinate::new_2d(5.0, 5.0),
            Coordinate::new_2d(0.0, 5.0),
            Coordinate::new_2d(0.0, 0.0),
        ];
        let coords2 = vec![
            Coordinate::new_2d(3.0, 3.0),
            Coordinate::new_2d(8.0, 3.0),
            Coordinate::new_2d(8.0, 8.0),
            Coordinate::new_2d(3.0, 8.0),
            Coordinate::new_2d(3.0, 3.0),
        ];

        let ext1 = LineString::new(coords1);
        let ext2 = LineString::new(coords2);

        assert!(ext1.is_ok() && ext2.is_ok());
        if let (Ok(e1), Ok(e2)) = (ext1, ext2) {
            let poly1 = Polygon::new(e1, vec![]);
            let poly2 = Polygon::new(e2, vec![]);

            assert!(poly1.is_ok() && poly2.is_ok());
            if let (Ok(p1), Ok(p2)) = (poly1, poly2) {
                let env1 = envelope_polygon(&p1);
                let env2 = envelope_polygon(&p2);

                assert!(env1.is_ok() && env2.is_ok());
                if let (Ok(e1), Ok(e2)) = (env1, env2) {
                    let union = envelope_union(&e1, &e2);
                    assert!(union.is_ok());

                    if let Ok(u) = union {
                        let bounds = u.bounds();
                        assert!(bounds.is_some());
                        if let Some((min_x, min_y, max_x, max_y)) = bounds {
                            assert_eq!(min_x, 0.0);
                            assert_eq!(min_y, 0.0);
                            assert_eq!(max_x, 8.0);
                            assert_eq!(max_y, 8.0);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn test_envelope_intersection() {
        let coords1 = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(6.0, 0.0),
            Coordinate::new_2d(6.0, 6.0),
            Coordinate::new_2d(0.0, 6.0),
            Coordinate::new_2d(0.0, 0.0),
        ];
        let coords2 = vec![
            Coordinate::new_2d(3.0, 3.0),
            Coordinate::new_2d(9.0, 3.0),
            Coordinate::new_2d(9.0, 9.0),
            Coordinate::new_2d(3.0, 9.0),
            Coordinate::new_2d(3.0, 3.0),
        ];

        let ext1 = LineString::new(coords1);
        let ext2 = LineString::new(coords2);

        assert!(ext1.is_ok() && ext2.is_ok());
        if let (Ok(e1), Ok(e2)) = (ext1, ext2) {
            let poly1 = Polygon::new(e1, vec![]);
            let poly2 = Polygon::new(e2, vec![]);

            assert!(poly1.is_ok() && poly2.is_ok());
            if let (Ok(p1), Ok(p2)) = (poly1, poly2) {
                let env1 = envelope_polygon(&p1);
                let env2 = envelope_polygon(&p2);

                assert!(env1.is_ok() && env2.is_ok());
                if let (Ok(e1), Ok(e2)) = (env1, env2) {
                    let intersection = envelope_intersection(&e1, &e2);
                    assert!(intersection.is_ok());

                    if let Ok(Some(i)) = intersection {
                        let bounds = i.bounds();
                        assert!(bounds.is_some());
                        if let Some((min_x, min_y, max_x, max_y)) = bounds {
                            assert_eq!(min_x, 3.0);
                            assert_eq!(min_y, 3.0);
                            assert_eq!(max_x, 6.0);
                            assert_eq!(max_y, 6.0);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn test_envelope_intersection_no_overlap() {
        let coords1 = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(5.0, 0.0),
            Coordinate::new_2d(5.0, 5.0),
            Coordinate::new_2d(0.0, 5.0),
            Coordinate::new_2d(0.0, 0.0),
        ];
        let coords2 = vec![
            Coordinate::new_2d(10.0, 10.0),
            Coordinate::new_2d(15.0, 10.0),
            Coordinate::new_2d(15.0, 15.0),
            Coordinate::new_2d(10.0, 15.0),
            Coordinate::new_2d(10.0, 10.0),
        ];

        let ext1 = LineString::new(coords1);
        let ext2 = LineString::new(coords2);

        assert!(ext1.is_ok() && ext2.is_ok());
        if let (Ok(e1), Ok(e2)) = (ext1, ext2) {
            let poly1 = Polygon::new(e1, vec![]);
            let poly2 = Polygon::new(e2, vec![]);

            assert!(poly1.is_ok() && poly2.is_ok());
            if let (Ok(p1), Ok(p2)) = (poly1, poly2) {
                let env1 = envelope_polygon(&p1);
                let env2 = envelope_polygon(&p2);

                assert!(env1.is_ok() && env2.is_ok());
                if let (Ok(e1), Ok(e2)) = (env1, env2) {
                    let intersection = envelope_intersection(&e1, &e2);
                    assert!(intersection.is_ok());

                    if let Ok(result) = intersection {
                        assert!(result.is_none());
                    }
                }
            }
        }
    }

    #[test]
    fn test_envelope_empty_linestring() {
        let coords: Vec<Coordinate> = vec![];
        let line = LineString::new(coords);

        // Empty linestring cannot be created
        assert!(line.is_err());
    }

    #[test]
    fn test_envelope_multipoint_empty() {
        let mp = MultiPoint::empty();
        let env = envelope_multipoint(&mp);
        assert!(env.is_err());
    }

    #[test]
    fn test_envelope_geometry_dispatch() {
        let point = Point::new(3.0, 5.0);
        let geom = Geometry::Point(point);

        let env = envelope(&geom);
        assert!(env.is_ok());

        if let Ok(e) = env {
            let bounds = e.bounds();
            assert!(bounds.is_some());
        }
    }
}
