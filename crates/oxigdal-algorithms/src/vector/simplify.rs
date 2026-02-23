//! Line and polygon simplification algorithms
//!
//! This module provides multiple algorithms for simplifying linear geometries
//! while preserving their essential shape characteristics.
//!
//! # Algorithms
//!
//! - **Douglas-Peucker**: Classic recursive simplification based on perpendicular distance
//! - **Visvalingam-Whyatt**: Progressive simplification based on triangle area
//! - **Topology-Preserving**: Ensures simplified geometry maintains topological relationships
//!
//! # Examples
//!
//! ```
//! # use oxigdal_algorithms::error::Result;
//! use oxigdal_algorithms::vector::{Coordinate, LineString, simplify_linestring, SimplifyMethod};
//!
//! # fn main() -> Result<()> {
//! let coords = vec![
//!     Coordinate::new_2d(0.0, 0.0),
//!     Coordinate::new_2d(1.0, 0.1),
//!     Coordinate::new_2d(2.0, 0.0),
//!     Coordinate::new_2d(3.0, 0.0),
//! ];
//! let line = LineString::new(coords)?;
//! let simplified = simplify_linestring(&line, 0.2, SimplifyMethod::DouglasPeucker)?;
//! # Ok(())
//! # }
//! ```

use crate::error::{AlgorithmError, Result};
use crate::vector::douglas_peucker::simplify_linestring as dp_simplify;
use oxigdal_core::vector::{Coordinate, LineString, Polygon};

#[cfg(feature = "std")]
use std::vec::Vec;

/// Simplification algorithm selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimplifyMethod {
    /// Douglas-Peucker algorithm (perpendicular distance)
    DouglasPeucker,
    /// Visvalingam-Whyatt algorithm (triangle area)
    VisvalingamWhyatt,
    /// Topology-preserving simplification
    TopologyPreserving,
}

/// Simplifies a linestring using the specified algorithm
///
/// # Arguments
///
/// * `line` - Input line string to simplify
/// * `tolerance` - Simplification tolerance (meaning depends on algorithm)
/// * `method` - Simplification algorithm to use
///
/// # Returns
///
/// Simplified line string with fewer points while maintaining shape
///
/// # Errors
///
/// Returns error if:
/// - Line is empty or has fewer than 2 points
/// - Tolerance is negative
/// - Algorithm encounters numerical issues
///
/// # Examples
///
/// ```
/// # use oxigdal_algorithms::error::Result;
/// use oxigdal_algorithms::vector::{Coordinate, LineString, simplify_linestring, SimplifyMethod};
///
/// # fn main() -> Result<()> {
/// let coords = vec![
///     Coordinate::new_2d(0.0, 0.0),
///     Coordinate::new_2d(1.0, 0.01),
///     Coordinate::new_2d(2.0, 0.0),
/// ];
/// let line = LineString::new(coords)?;
/// let simplified = simplify_linestring(&line, 0.05, SimplifyMethod::DouglasPeucker)?;
/// assert_eq!(simplified.len(), 2); // Middle point removed
/// # Ok(())
/// # }
/// ```
pub fn simplify_linestring(
    line: &LineString,
    tolerance: f64,
    method: SimplifyMethod,
) -> Result<LineString> {
    if line.coords.is_empty() {
        return Err(AlgorithmError::EmptyInput {
            operation: "simplify_linestring",
        });
    }

    if tolerance < 0.0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "tolerance",
            message: "tolerance must be non-negative".to_string(),
        });
    }

    match method {
        SimplifyMethod::DouglasPeucker => dp_simplify(line, tolerance),
        SimplifyMethod::VisvalingamWhyatt => simplify_visvalingam_whyatt(line, tolerance),
        SimplifyMethod::TopologyPreserving => simplify_topology_preserving(line, tolerance),
    }
}

/// Simplifies a polygon using the specified algorithm
///
/// # Arguments
///
/// * `polygon` - Input polygon to simplify
/// * `tolerance` - Simplification tolerance
/// * `method` - Simplification algorithm to use
///
/// # Returns
///
/// Simplified polygon with exterior and interior rings simplified
///
/// # Errors
///
/// Returns error if:
/// - Polygon is invalid
/// - Tolerance is negative
/// - Simplified polygon would have fewer than 4 points
pub fn simplify_polygon(
    polygon: &Polygon,
    tolerance: f64,
    method: SimplifyMethod,
) -> Result<Polygon> {
    if polygon.exterior.coords.len() < 4 {
        return Err(AlgorithmError::InsufficientData {
            operation: "simplify_polygon",
            message: "polygon exterior must have at least 4 coordinates".to_string(),
        });
    }

    // Simplify exterior ring
    let simplified_exterior = simplify_linestring(&polygon.exterior, tolerance, method)?;

    // Ensure exterior still has at least 4 points
    if simplified_exterior.coords.len() < 4 {
        return Err(AlgorithmError::GeometryError {
            message: "simplified exterior would have fewer than 4 points".to_string(),
        });
    }

    // Simplify interior rings
    let mut simplified_interiors = Vec::with_capacity(polygon.interiors.len());
    for interior in &polygon.interiors {
        let simplified_interior = simplify_linestring(interior, tolerance, method)?;

        // Only keep interior rings that still have at least 4 points
        if simplified_interior.coords.len() >= 4 {
            simplified_interiors.push(simplified_interior);
        }
    }

    Polygon::new(simplified_exterior, simplified_interiors).map_err(AlgorithmError::Core)
}

/// Visvalingam-Whyatt simplification algorithm
///
/// This algorithm progressively removes vertices that form triangles with
/// the smallest effective area, continuing until all remaining triangles
/// exceed the tolerance threshold.
fn simplify_visvalingam_whyatt(line: &LineString, tolerance: f64) -> Result<LineString> {
    if line.coords.len() <= 2 {
        return Ok(line.clone());
    }

    let n = line.coords.len();
    let mut keep = vec![true; n];
    let mut areas = vec![f64::INFINITY; n];

    // Keep endpoints
    keep[0] = true;
    keep[n - 1] = true;

    // Calculate initial effective areas
    for i in 1..n - 1 {
        areas[i] = triangle_area(&line.coords[i - 1], &line.coords[i], &line.coords[i + 1]);
    }

    // Progressively remove points with smallest area
    let mut removed_count = 0;
    let target_count = n - 2; // Maximum number we can remove

    while removed_count < target_count {
        // Find point with minimum area that hasn't been removed
        let mut min_area = f64::INFINITY;
        let mut min_idx = 0;

        for i in 1..n - 1 {
            if keep[i] && areas[i] < min_area {
                min_area = areas[i];
                min_idx = i;
            }
        }

        // If minimum area exceeds tolerance, stop
        if min_area > tolerance {
            break;
        }

        // Remove the point
        keep[min_idx] = false;
        removed_count += 1;

        // Recalculate areas for neighboring points
        let prev = find_prev_kept(&keep, min_idx);
        let next = find_next_kept(&keep, min_idx);

        if let (Some(p), Some(n)) = (prev, next) {
            // Update previous point's area
            if p > 0 {
                if let Some(pp) = find_prev_kept(&keep, p) {
                    areas[p] = triangle_area(&line.coords[pp], &line.coords[p], &line.coords[n]);
                }
            }

            // Update next point's area
            if n < line.coords.len() - 1 {
                if let Some(nn) = find_next_kept(&keep, n) {
                    areas[n] = triangle_area(&line.coords[p], &line.coords[n], &line.coords[nn]);
                }
            }
        }
    }

    // Build simplified line
    let simplified_coords: Vec<Coordinate> = line
        .coords
        .iter()
        .zip(keep.iter())
        .filter(|&(_, &k)| k)
        .map(|(c, _)| *c)
        .collect();

    LineString::new(simplified_coords).map_err(AlgorithmError::Core)
}

/// Topology-preserving simplification
///
/// Ensures that simplification doesn't create self-intersections or
/// change the topology of the geometry.
fn simplify_topology_preserving(line: &LineString, tolerance: f64) -> Result<LineString> {
    // Use Douglas-Peucker as base
    let simplified = dp_simplify(line, tolerance)?;

    // Verify no self-intersections were created
    if has_self_intersection(&simplified) {
        // Fall back to more conservative tolerance
        let conservative_tolerance = tolerance * 0.5;
        if conservative_tolerance < 1e-10 {
            // Can't simplify further without creating issues
            return Ok(line.clone());
        }
        return simplify_topology_preserving(line, conservative_tolerance);
    }

    Ok(simplified)
}

/// Calculates the area of a triangle formed by three points
fn triangle_area(p1: &Coordinate, p2: &Coordinate, p3: &Coordinate) -> f64 {
    // Use cross product formula: |AB × AC| / 2
    let ab_x = p2.x - p1.x;
    let ab_y = p2.y - p1.y;
    let ac_x = p3.x - p1.x;
    let ac_y = p3.y - p1.y;

    ((ab_x * ac_y - ab_y * ac_x).abs()) / 2.0
}

/// Finds the previous kept point index
fn find_prev_kept(keep: &[bool], from: usize) -> Option<usize> {
    if from == 0 {
        return None;
    }

    (0..from).rev().find(|&i| keep[i])
}

/// Finds the next kept point index
fn find_next_kept(keep: &[bool], from: usize) -> Option<usize> {
    for i in from + 1..keep.len() {
        if keep[i] {
            return Some(i);
        }
    }

    None
}

/// Checks if a linestring has self-intersections
fn has_self_intersection(line: &LineString) -> bool {
    let n = line.coords.len();
    if n < 4 {
        return false; // Can't self-intersect with fewer than 4 points
    }

    // Check all pairs of non-adjacent segments
    for i in 0..n - 1 {
        for j in i + 2..n - 1 {
            // Skip adjacent segments
            if j == i + 1 || (i == 0 && j == n - 2) {
                continue;
            }

            if segments_intersect(
                &line.coords[i],
                &line.coords[i + 1],
                &line.coords[j],
                &line.coords[j + 1],
            ) {
                return true;
            }
        }
    }

    false
}

/// Checks if two line segments intersect
fn segments_intersect(p1: &Coordinate, p2: &Coordinate, p3: &Coordinate, p4: &Coordinate) -> bool {
    let d1 = direction(p3, p4, p1);
    let d2 = direction(p3, p4, p2);
    let d3 = direction(p1, p2, p3);
    let d4 = direction(p1, p2, p4);

    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
    {
        return true;
    }

    // Check collinear cases
    if d1.abs() < f64::EPSILON && on_segment(p3, p1, p4) {
        return true;
    }
    if d2.abs() < f64::EPSILON && on_segment(p3, p2, p4) {
        return true;
    }
    if d3.abs() < f64::EPSILON && on_segment(p1, p3, p2) {
        return true;
    }
    if d4.abs() < f64::EPSILON && on_segment(p1, p4, p2) {
        return true;
    }

    false
}

/// Computes the direction/orientation of point p relative to line from a to b
fn direction(a: &Coordinate, b: &Coordinate, p: &Coordinate) -> f64 {
    (b.x - a.x) * (p.y - a.y) - (p.x - a.x) * (b.y - a.y)
}

/// Checks if point q lies on segment pr
fn on_segment(p: &Coordinate, q: &Coordinate, r: &Coordinate) -> bool {
    q.x <= p.x.max(r.x) && q.x >= p.x.min(r.x) && q.y <= p.y.max(r.y) && q.y >= p.y.min(r.y)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_zigzag_line() -> Result<LineString> {
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(2.0, 0.0),
            Coordinate::new_2d(3.0, 1.0),
            Coordinate::new_2d(4.0, 0.0),
        ];
        LineString::new(coords).map_err(|e| AlgorithmError::Core(e))
    }

    fn create_square_polygon() -> Result<Polygon> {
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(10.0, 0.0),
            Coordinate::new_2d(10.0, 10.0),
            Coordinate::new_2d(0.0, 10.0),
            Coordinate::new_2d(0.0, 0.0),
        ];
        let exterior = LineString::new(coords).map_err(|e| AlgorithmError::Core(e))?;
        Polygon::new(exterior, vec![]).map_err(|e| AlgorithmError::Core(e))
    }

    #[test]
    fn test_simplify_linestring_douglas_peucker() {
        let line = create_zigzag_line();
        assert!(line.is_ok());

        if let Ok(l) = line {
            let result = simplify_linestring(&l, 0.5, SimplifyMethod::DouglasPeucker);
            assert!(result.is_ok());

            if let Ok(simplified) = result {
                // Should keep endpoints and some intermediate points
                assert!(simplified.len() >= 2);
                assert!(simplified.len() <= l.len());
            }
        }
    }

    #[test]
    fn test_simplify_linestring_visvalingam() {
        let line = create_zigzag_line();
        assert!(line.is_ok());

        if let Ok(l) = line {
            let result = simplify_linestring(&l, 0.5, SimplifyMethod::VisvalingamWhyatt);
            assert!(result.is_ok());

            if let Ok(simplified) = result {
                assert!(simplified.len() >= 2);
                assert!(simplified.len() <= l.len());
            }
        }
    }

    #[test]
    fn test_simplify_linestring_topology_preserving() {
        let line = create_zigzag_line();
        assert!(line.is_ok());

        if let Ok(l) = line {
            let result = simplify_linestring(&l, 0.5, SimplifyMethod::TopologyPreserving);
            assert!(result.is_ok());

            if let Ok(simplified) = result {
                // Should not have self-intersections
                assert!(!has_self_intersection(&simplified));
            }
        }
    }

    #[test]
    fn test_simplify_polygon() {
        let poly = create_square_polygon();
        assert!(poly.is_ok());

        if let Ok(p) = poly {
            let result = simplify_polygon(&p, 0.1, SimplifyMethod::DouglasPeucker);
            assert!(result.is_ok());

            if let Ok(simplified) = result {
                // Square should remain a square (4 corners + closing point)
                assert_eq!(simplified.exterior.len(), 5);
            }
        }
    }

    #[test]
    fn test_triangle_area() {
        let p1 = Coordinate::new_2d(0.0, 0.0);
        let p2 = Coordinate::new_2d(2.0, 0.0);
        let p3 = Coordinate::new_2d(1.0, 2.0);

        let area = triangle_area(&p1, &p2, &p3);
        assert!((area - 2.0).abs() < 1e-10); // Area = base * height / 2 = 2 * 2 / 2 = 2
    }

    #[test]
    fn test_segments_intersect() {
        // Intersecting segments
        let p1 = Coordinate::new_2d(0.0, 0.0);
        let p2 = Coordinate::new_2d(2.0, 2.0);
        let p3 = Coordinate::new_2d(0.0, 2.0);
        let p4 = Coordinate::new_2d(2.0, 0.0);

        assert!(segments_intersect(&p1, &p2, &p3, &p4));
    }

    #[test]
    fn test_segments_no_intersect() {
        // Non-intersecting segments
        let p1 = Coordinate::new_2d(0.0, 0.0);
        let p2 = Coordinate::new_2d(1.0, 0.0);
        let p3 = Coordinate::new_2d(0.0, 1.0);
        let p4 = Coordinate::new_2d(1.0, 1.0);

        assert!(!segments_intersect(&p1, &p2, &p3, &p4));
    }

    #[test]
    fn test_has_self_intersection_false() {
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 0.0),
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(0.0, 1.0),
        ];
        let line = LineString::new(coords);
        assert!(line.is_ok());

        if let Ok(l) = line {
            assert!(!has_self_intersection(&l));
        }
    }

    #[test]
    fn test_simplify_empty_line() {
        let coords: Vec<Coordinate> = vec![];
        let line = LineString::new(coords);

        // Empty line can't be created
        assert!(line.is_err());
    }

    #[test]
    fn test_simplify_negative_tolerance() {
        let line = create_zigzag_line();
        assert!(line.is_ok());

        if let Ok(l) = line {
            let result = simplify_linestring(&l, -1.0, SimplifyMethod::DouglasPeucker);
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_find_prev_next_kept() {
        let keep = vec![true, false, true, false, true];

        assert_eq!(find_prev_kept(&keep, 2), Some(0));
        assert_eq!(find_next_kept(&keep, 2), Some(4));
        assert_eq!(find_prev_kept(&keep, 0), None);
        assert_eq!(find_next_kept(&keep, 4), None);
    }
}
