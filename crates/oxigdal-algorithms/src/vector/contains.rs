//! Spatial predicates for geometric relationships
//!
//! This module provides binary spatial predicates that test topological
//! relationships between geometries following the DE-9IM model.
//!
//! # Predicates
//!
//! - **Contains**: Tests if geometry A completely contains geometry B
//! - **Within**: Tests if geometry A is completely within geometry B
//! - **Intersects**: Tests if geometries share any points
//! - **Touches**: Tests if geometries share boundary points but not interior points
//! - **Disjoint**: Tests if geometries share no points
//! - **Overlaps**: Tests if geometries share some but not all points
//! - **Covers**: Tests if every point of B is a point of A
//! - **CoveredBy**: Tests if every point of A is a point of B
//!
//! # Examples
//!
//! ```
//! use oxigdal_algorithms::vector::{Polygon, LineString, Coordinate, point_in_polygon_or_boundary};
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
//! let point = Coordinate::new_2d(2.0, 2.0);
//! let result = point_in_polygon_or_boundary(&point, &polygon);
//! // result should be true
//! # Ok(())
//! # }
//! ```

use crate::error::Result;
use oxigdal_core::vector::{Coordinate, Point, Polygon};

/// Tests if geometry A contains geometry B
///
/// Geometry A contains B if:
/// - No points of B lie in the exterior of A
/// - At least one point of the interior of B lies in the interior of A
///
/// # Arguments
///
/// * `a` - Container geometry
/// * `b` - Contained geometry
///
/// # Returns
///
/// True if A contains B
///
/// # Errors
///
/// Returns error if geometries are invalid
pub fn contains<T: ContainsPredicate>(a: &T, b: &T) -> Result<bool> {
    a.contains(b)
}

/// Tests if geometry A is within geometry B (inverse of contains)
///
/// # Arguments
///
/// * `a` - Inner geometry
/// * `b` - Outer geometry
///
/// # Returns
///
/// True if A is within B
///
/// # Errors
///
/// Returns error if geometries are invalid
pub fn within<T: ContainsPredicate>(a: &T, b: &T) -> Result<bool> {
    b.contains(a)
}

/// Tests if geometries intersect (share any points)
///
/// # Arguments
///
/// * `a` - First geometry
/// * `b` - Second geometry
///
/// # Returns
///
/// True if geometries intersect
///
/// # Errors
///
/// Returns error if geometries are invalid
pub fn intersects<T: IntersectsPredicate>(a: &T, b: &T) -> Result<bool> {
    a.intersects(b)
}

/// Tests if geometries are disjoint (share no points)
///
/// # Arguments
///
/// * `a` - First geometry
/// * `b` - Second geometry
///
/// # Returns
///
/// True if geometries are disjoint
///
/// # Errors
///
/// Returns error if geometries are invalid
pub fn disjoint<T: IntersectsPredicate>(a: &T, b: &T) -> Result<bool> {
    Ok(!a.intersects(b)?)
}

/// Tests if geometries touch (share boundary but not interior)
///
/// # Arguments
///
/// * `a` - First geometry
/// * `b` - Second geometry
///
/// # Returns
///
/// True if geometries touch
///
/// # Errors
///
/// Returns error if geometries are invalid
pub fn touches<T: TouchesPredicate>(a: &T, b: &T) -> Result<bool> {
    a.touches(b)
}

/// Tests if geometries overlap (share some but not all points)
///
/// Two geometries overlap if:
/// - They have the same dimension
/// - Their interiors intersect
/// - Neither geometry completely contains the other
///
/// # Arguments
///
/// * `a` - First geometry
/// * `b` - Second geometry
///
/// # Returns
///
/// True if geometries overlap
///
/// # Errors
///
/// Returns error if geometries are invalid
pub fn overlaps<T: OverlapsPredicate>(a: &T, b: &T) -> Result<bool> {
    a.overlaps(b)
}

/// Tests if one geometry crosses another
///
/// Two geometries cross if:
/// - They have some but not all interior points in common
/// - The dimension of the intersection is less than the maximum dimension of the two geometries
///
/// # Arguments
///
/// * `a` - First geometry
/// * `b` - Second geometry
///
/// # Returns
///
/// True if geometries cross
///
/// # Errors
///
/// Returns error if geometries are invalid
pub fn crosses<T: CrossesPredicate>(a: &T, b: &T) -> Result<bool> {
    a.crosses(b)
}

/// Trait for geometries that support contains predicate
pub trait ContainsPredicate {
    /// Tests if this geometry contains another
    fn contains(&self, other: &Self) -> Result<bool>;
}

/// Trait for geometries that support intersects predicate
pub trait IntersectsPredicate {
    /// Tests if this geometry intersects another
    fn intersects(&self, other: &Self) -> Result<bool>;
}

/// Trait for geometries that support touches predicate
pub trait TouchesPredicate {
    /// Tests if this geometry touches another
    fn touches(&self, other: &Self) -> Result<bool>;
}

/// Trait for geometries that support overlaps predicate
pub trait OverlapsPredicate {
    /// Tests if this geometry overlaps another
    fn overlaps(&self, other: &Self) -> Result<bool>;
}

/// Trait for geometries that support crosses predicate
pub trait CrossesPredicate {
    /// Tests if this geometry crosses another
    fn crosses(&self, other: &Self) -> Result<bool>;
}

// Implement ContainsPredicate for Point
impl ContainsPredicate for Point {
    fn contains(&self, other: &Self) -> Result<bool> {
        // A point contains another point only if they're the same
        Ok((self.coord.x - other.coord.x).abs() < f64::EPSILON
            && (self.coord.y - other.coord.y).abs() < f64::EPSILON)
    }
}

// Implement ContainsPredicate for Polygon
impl ContainsPredicate for Polygon {
    fn contains(&self, other: &Self) -> Result<bool> {
        // Check if all vertices of other are inside or on boundary of self
        for coord in &other.exterior.coords {
            if !point_in_polygon_or_boundary(coord, self) {
                return Ok(false);
            }
        }

        // Check if any vertex is strictly inside (not just on boundary)
        let mut has_interior_point = false;
        for coord in &other.exterior.coords {
            if point_strictly_inside_polygon(coord, self) {
                has_interior_point = true;
                break;
            }
        }

        Ok(has_interior_point)
    }
}

// Implement IntersectsPredicate for Point
impl IntersectsPredicate for Point {
    fn intersects(&self, other: &Self) -> Result<bool> {
        self.contains(other)
    }
}

// Implement IntersectsPredicate for Polygon
impl IntersectsPredicate for Polygon {
    fn intersects(&self, other: &Self) -> Result<bool> {
        // Check if any vertices are inside
        for coord in &other.exterior.coords {
            if point_in_polygon_or_boundary(coord, self) {
                return Ok(true);
            }
        }

        for coord in &self.exterior.coords {
            if point_in_polygon_or_boundary(coord, other) {
                return Ok(true);
            }
        }

        // Check if any edges intersect
        Ok(rings_intersect(
            &self.exterior.coords,
            &other.exterior.coords,
        ))
    }
}

// Implement TouchesPredicate for Polygon
impl TouchesPredicate for Polygon {
    fn touches(&self, other: &Self) -> Result<bool> {
        let mut has_boundary_contact = false;
        let mut has_interior_contact = false;

        // Check vertices of other against self
        for coord in &other.exterior.coords {
            if point_on_polygon_boundary(coord, self) {
                has_boundary_contact = true;
            } else if point_strictly_inside_polygon(coord, self) {
                has_interior_contact = true;
            }
        }

        // Check vertices of self against other
        for coord in &self.exterior.coords {
            if point_strictly_inside_polygon(coord, other) {
                has_interior_contact = true;
            }
        }

        Ok(has_boundary_contact && !has_interior_contact)
    }
}

// Implement OverlapsPredicate for Point
impl OverlapsPredicate for Point {
    fn overlaps(&self, _other: &Self) -> Result<bool> {
        // Points cannot overlap - they are either the same (equal) or disjoint
        Ok(false)
    }
}

// Implement OverlapsPredicate for Polygon
impl OverlapsPredicate for Polygon {
    fn overlaps(&self, other: &Self) -> Result<bool> {
        // Two polygons overlap if:
        // 1. They intersect
        // 2. Neither completely contains the other
        // 3. They have interior points in common

        // First check if they intersect at all
        if !self.intersects(other)? {
            return Ok(false);
        }

        // Check if either polygon completely contains the other
        if self.contains(other)? || other.contains(self)? {
            return Ok(false);
        }

        // Check if they have interior points in common
        // If they intersect and neither contains the other, they must overlap
        Ok(true)
    }
}

// Implement CrossesPredicate for Point
impl CrossesPredicate for Point {
    fn crosses(&self, _other: &Self) -> Result<bool> {
        // Points cannot cross each other
        Ok(false)
    }
}

// Implement CrossesPredicate for Polygon
impl CrossesPredicate for Polygon {
    fn crosses(&self, other: &Self) -> Result<bool> {
        // For polygons to cross, they must have interior points in common
        // and exterior points in common. This is similar to overlaps but
        // typically applies more to lower dimensional geometries crossing
        // higher dimensional ones (e.g., line crossing polygon).
        // For polygon-polygon, we define crossing as:
        // - They intersect
        // - Neither completely contains the other
        // - They share boundary segments (edges cross)

        if !self.intersects(other)? {
            return Ok(false);
        }

        // If one completely contains the other, they don't cross
        if self.contains(other)? || other.contains(self)? {
            return Ok(false);
        }

        // Check if some but not all vertices of other are inside self
        let mut other_some_inside = false;
        let mut other_some_outside = false;

        for coord in &other.exterior.coords {
            if point_in_polygon_or_boundary(coord, self) {
                other_some_inside = true;
            } else {
                other_some_outside = true;
            }
        }

        // Also check if some but not all vertices of self are inside other
        let mut self_some_inside = false;
        let mut self_some_outside = false;

        for coord in &self.exterior.coords {
            if point_in_polygon_or_boundary(coord, other) {
                self_some_inside = true;
            } else {
                self_some_outside = true;
            }
        }

        // Crosses if:
        // 1. Either direction shows partial containment, OR
        // 2. They intersect but no vertices are contained (edge-only intersection)
        if (other_some_inside && other_some_outside) || (self_some_inside && self_some_outside) {
            Ok(true)
        } else if !other_some_inside && !self_some_inside {
            // No vertices inside either polygon, but they intersect
            // This means edges must be crossing
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

/// Tests if a point is inside or on the boundary of a polygon
pub fn point_in_polygon_or_boundary(point: &Coordinate, polygon: &Polygon) -> bool {
    point_in_polygon_boundary(point, polygon) || point_on_polygon_boundary(point, polygon)
}

/// Tests if a point is strictly inside a polygon (not on boundary)
pub fn point_strictly_inside_polygon(point: &Coordinate, polygon: &Polygon) -> bool {
    point_in_polygon_boundary(point, polygon) && !point_on_polygon_boundary(point, polygon)
}

/// Tests if a point is on the boundary of a polygon
pub fn point_on_polygon_boundary(point: &Coordinate, polygon: &Polygon) -> bool {
    point_on_ring(&polygon.exterior.coords, point)
        || polygon
            .interiors
            .iter()
            .any(|hole| point_on_ring(&hole.coords, point))
}

/// Tests if a point is on a ring (linestring)
fn point_on_ring(ring: &[Coordinate], point: &Coordinate) -> bool {
    for i in 0..ring.len().saturating_sub(1) {
        if point_on_segment(point, &ring[i], &ring[i + 1]) {
            return true;
        }
    }
    false
}

/// Tests if a point lies on a line segment
fn point_on_segment(point: &Coordinate, seg_start: &Coordinate, seg_end: &Coordinate) -> bool {
    // Check if point is collinear with segment
    let cross = (seg_end.y - seg_start.y) * (point.x - seg_start.x)
        - (seg_end.x - seg_start.x) * (point.y - seg_start.y);

    if cross.abs() > f64::EPSILON {
        return false;
    }

    // Check if point is within segment bounds
    let dot = (point.x - seg_start.x) * (seg_end.x - seg_start.x)
        + (point.y - seg_start.y) * (seg_end.y - seg_start.y);

    let len_sq = (seg_end.x - seg_start.x).powi(2) + (seg_end.y - seg_start.y).powi(2);

    if dot < -f64::EPSILON || dot > len_sq + f64::EPSILON {
        return false;
    }

    true
}

/// Ray casting algorithm for point-in-polygon test
fn point_in_polygon_boundary(point: &Coordinate, polygon: &Polygon) -> bool {
    let mut inside = ray_casting_test(point, &polygon.exterior.coords);

    // Subtract holes using XOR
    for hole in &polygon.interiors {
        if ray_casting_test(point, &hole.coords) {
            inside = !inside;
        }
    }

    inside
}

/// Ray casting algorithm implementation
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

/// Winding number algorithm for point-in-polygon test (alternative to ray casting)
///
/// More robust than ray casting for edge cases.
#[allow(dead_code)]
fn winding_number_test(point: &Coordinate, ring: &[Coordinate]) -> bool {
    let mut winding_number = 0;
    let n = ring.len();

    for i in 0..n - 1 {
        let p1 = &ring[i];
        let p2 = &ring[i + 1];

        if p1.y <= point.y {
            if p2.y > point.y {
                // Upward crossing
                if is_left(p1, p2, point) > 0.0 {
                    winding_number += 1;
                }
            }
        } else if p2.y <= point.y {
            // Downward crossing
            if is_left(p1, p2, point) < 0.0 {
                winding_number -= 1;
            }
        }
    }

    winding_number != 0
}

/// Tests if a point is to the left of a line
fn is_left(p1: &Coordinate, p2: &Coordinate, point: &Coordinate) -> f64 {
    (p2.x - p1.x) * (point.y - p1.y) - (point.x - p1.x) * (p2.y - p1.y)
}

/// Tests if two rings intersect
fn rings_intersect(ring1: &[Coordinate], ring2: &[Coordinate]) -> bool {
    for i in 0..ring1.len().saturating_sub(1) {
        for j in 0..ring2.len().saturating_sub(1) {
            if segments_intersect(&ring1[i], &ring1[i + 1], &ring2[j], &ring2[j + 1]) {
                return true;
            }
        }
    }
    false
}

/// Tests if two line segments intersect
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

/// Computes the direction/orientation
fn direction(a: &Coordinate, b: &Coordinate, p: &Coordinate) -> f64 {
    (b.x - a.x) * (p.y - a.y) - (p.x - a.x) * (b.y - a.y)
}

/// Tests if point q lies on segment pr
fn on_segment(p: &Coordinate, q: &Coordinate, r: &Coordinate) -> bool {
    q.x <= p.x.max(r.x) && q.x >= p.x.min(r.x) && q.y <= p.y.max(r.y) && q.y >= p.y.min(r.y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AlgorithmError;
    use oxigdal_core::vector::LineString;

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
    fn test_point_contains_point() {
        let p1 = Point::new(1.0, 2.0);
        let p2 = Point::new(1.0, 2.0);
        let p3 = Point::new(3.0, 4.0);

        let result1 = p1.contains(&p2);
        assert!(result1.is_ok());
        if let Ok(contains) = result1 {
            assert!(contains);
        }

        let result2 = p1.contains(&p3);
        assert!(result2.is_ok());
        if let Ok(contains) = result2 {
            assert!(!contains);
        }
    }

    #[test]
    fn test_polygon_contains_point() {
        let poly = create_square();
        assert!(poly.is_ok());

        if let Ok(p) = poly {
            // Point inside
            let inside = Coordinate::new_2d(2.0, 2.0);
            assert!(point_strictly_inside_polygon(&inside, &p));

            // Point outside
            let outside = Coordinate::new_2d(5.0, 5.0);
            assert!(!point_in_polygon_or_boundary(&outside, &p));

            // Point on boundary
            let boundary = Coordinate::new_2d(0.0, 2.0);
            assert!(point_on_polygon_boundary(&boundary, &p));
        }
    }

    #[test]
    fn test_point_in_polygon_boundary() {
        let poly = create_square();
        assert!(poly.is_ok());

        if let Ok(p) = poly {
            let inside = Coordinate::new_2d(2.0, 2.0);
            assert!(point_in_polygon_boundary(&inside, &p));

            let outside = Coordinate::new_2d(5.0, 5.0);
            assert!(!point_in_polygon_boundary(&outside, &p));
        }
    }

    #[test]
    fn test_point_on_segment() {
        let seg_start = Coordinate::new_2d(0.0, 0.0);
        let seg_end = Coordinate::new_2d(4.0, 0.0);

        // Point on segment
        let on = Coordinate::new_2d(2.0, 0.0);
        assert!(point_on_segment(&on, &seg_start, &seg_end));

        // Point off segment
        let off = Coordinate::new_2d(2.0, 1.0);
        assert!(!point_on_segment(&off, &seg_start, &seg_end));
    }

    #[test]
    fn test_polygon_intersects_polygon() {
        let poly1 = create_square();
        assert!(poly1.is_ok());

        // Overlapping polygon
        let coords2 = vec![
            Coordinate::new_2d(2.0, 2.0),
            Coordinate::new_2d(6.0, 2.0),
            Coordinate::new_2d(6.0, 6.0),
            Coordinate::new_2d(2.0, 6.0),
            Coordinate::new_2d(2.0, 2.0),
        ];
        let exterior2 = LineString::new(coords2);
        assert!(exterior2.is_ok());

        if let (Ok(p1), Ok(ext2)) = (poly1, exterior2) {
            let poly2 = Polygon::new(ext2, vec![]);
            assert!(poly2.is_ok());

            if let Ok(p2) = poly2 {
                let result: crate::error::Result<bool> = intersects(&p1, &p2);
                assert!(result.is_ok());

                if let Ok(do_intersect) = result {
                    assert!(do_intersect);
                }
            }
        }
    }

    #[test]
    fn test_disjoint_polygons() {
        let poly1 = create_square();

        // Disjoint polygon
        let coords2 = vec![
            Coordinate::new_2d(10.0, 10.0),
            Coordinate::new_2d(14.0, 10.0),
            Coordinate::new_2d(14.0, 14.0),
            Coordinate::new_2d(10.0, 14.0),
            Coordinate::new_2d(10.0, 10.0),
        ];
        let exterior2 = LineString::new(coords2);

        assert!(poly1.is_ok() && exterior2.is_ok());

        if let (Ok(p1), Ok(ext2)) = (poly1, exterior2) {
            let poly2 = Polygon::new(ext2, vec![]);
            assert!(poly2.is_ok());

            if let Ok(p2) = poly2 {
                let result: crate::error::Result<bool> = intersects(&p1, &p2);
                assert!(result.is_ok());

                if let Ok(do_intersect) = result {
                    assert!(!do_intersect);
                }
            }
        }
    }

    #[test]
    fn test_segments_intersect() {
        // Crossing segments
        let p1 = Coordinate::new_2d(0.0, 0.0);
        let p2 = Coordinate::new_2d(2.0, 2.0);
        let p3 = Coordinate::new_2d(0.0, 2.0);
        let p4 = Coordinate::new_2d(2.0, 0.0);

        assert!(segments_intersect(&p1, &p2, &p3, &p4));
    }

    #[test]
    fn test_segments_no_intersect() {
        // Parallel segments
        let p1 = Coordinate::new_2d(0.0, 0.0);
        let p2 = Coordinate::new_2d(2.0, 0.0);
        let p3 = Coordinate::new_2d(0.0, 1.0);
        let p4 = Coordinate::new_2d(2.0, 1.0);

        assert!(!segments_intersect(&p1, &p2, &p3, &p4));
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

        let inside = Coordinate::new_2d(2.0, 2.0);
        assert!(ray_casting_test(&inside, &ring));

        let outside = Coordinate::new_2d(5.0, 5.0);
        assert!(!ray_casting_test(&outside, &ring));
    }

    #[test]
    fn test_winding_number() {
        let ring = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(4.0, 0.0),
            Coordinate::new_2d(4.0, 4.0),
            Coordinate::new_2d(0.0, 4.0),
            Coordinate::new_2d(0.0, 0.0),
        ];

        let inside = Coordinate::new_2d(2.0, 2.0);
        assert!(winding_number_test(&inside, &ring));

        let outside = Coordinate::new_2d(5.0, 5.0);
        assert!(!winding_number_test(&outside, &ring));
    }

    #[test]
    fn test_overlaps_polygons_partial() {
        // Two polygons that partially overlap
        let poly1 = create_square();
        assert!(poly1.is_ok());

        let coords2 = vec![
            Coordinate::new_2d(2.0, 2.0),
            Coordinate::new_2d(6.0, 2.0),
            Coordinate::new_2d(6.0, 6.0),
            Coordinate::new_2d(2.0, 6.0),
            Coordinate::new_2d(2.0, 2.0),
        ];
        let exterior2 = LineString::new(coords2);
        assert!(exterior2.is_ok());

        if let (Ok(p1), Ok(ext2)) = (poly1, exterior2) {
            let poly2 = Polygon::new(ext2, vec![]);
            assert!(poly2.is_ok());

            if let Ok(p2) = poly2 {
                let result = overlaps(&p1, &p2);
                assert!(result.is_ok());
                if let Ok(do_overlap) = result {
                    assert!(do_overlap, "Partially overlapping polygons should overlap");
                }
            }
        }
    }

    #[test]
    fn test_overlaps_polygons_disjoint() {
        // Two polygons that don't overlap (disjoint)
        let poly1 = create_square();
        assert!(poly1.is_ok());

        let coords2 = vec![
            Coordinate::new_2d(10.0, 10.0),
            Coordinate::new_2d(14.0, 10.0),
            Coordinate::new_2d(14.0, 14.0),
            Coordinate::new_2d(10.0, 14.0),
            Coordinate::new_2d(10.0, 10.0),
        ];
        let exterior2 = LineString::new(coords2);
        assert!(exterior2.is_ok());

        if let (Ok(p1), Ok(ext2)) = (poly1, exterior2) {
            let poly2 = Polygon::new(ext2, vec![]);
            assert!(poly2.is_ok());

            if let Ok(p2) = poly2 {
                let result = overlaps(&p1, &p2);
                assert!(result.is_ok());
                if let Ok(do_overlap) = result {
                    assert!(!do_overlap, "Disjoint polygons should not overlap");
                }
            }
        }
    }

    #[test]
    fn test_overlaps_polygons_contained() {
        // One polygon completely contains another
        let poly1 = create_square();
        assert!(poly1.is_ok());

        let coords2 = vec![
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(3.0, 1.0),
            Coordinate::new_2d(3.0, 3.0),
            Coordinate::new_2d(1.0, 3.0),
            Coordinate::new_2d(1.0, 1.0),
        ];
        let exterior2 = LineString::new(coords2);
        assert!(exterior2.is_ok());

        if let (Ok(p1), Ok(ext2)) = (poly1, exterior2) {
            let poly2 = Polygon::new(ext2, vec![]);
            assert!(poly2.is_ok());

            if let Ok(p2) = poly2 {
                let result = overlaps(&p1, &p2);
                assert!(result.is_ok());
                if let Ok(do_overlap) = result {
                    assert!(!do_overlap, "Contained polygons should not overlap");
                }
            }
        }
    }

    #[test]
    fn test_overlaps_points() {
        let p1 = Point::new(1.0, 2.0);
        let p2 = Point::new(1.0, 2.0);

        let result = overlaps(&p1, &p2);
        assert!(result.is_ok());
        if let Ok(do_overlap) = result {
            assert!(!do_overlap, "Points should not overlap");
        }
    }

    #[test]
    fn test_crosses_polygons() {
        // Two polygons where one crosses the other
        let poly1 = create_square();
        assert!(poly1.is_ok());

        let coords2 = vec![
            Coordinate::new_2d(-1.0, 2.0),
            Coordinate::new_2d(5.0, 2.0),
            Coordinate::new_2d(5.0, 3.0),
            Coordinate::new_2d(-1.0, 3.0),
            Coordinate::new_2d(-1.0, 2.0),
        ];
        let exterior2 = LineString::new(coords2);
        assert!(exterior2.is_ok());

        if let (Ok(p1), Ok(ext2)) = (poly1, exterior2) {
            let poly2 = Polygon::new(ext2, vec![]);
            assert!(poly2.is_ok());

            if let Ok(p2) = poly2 {
                let result = crosses(&p1, &p2);
                assert!(result.is_ok());
                if let Ok(do_cross) = result {
                    assert!(do_cross, "Polygon crossing through another should cross");
                }
            }
        }
    }

    #[test]
    fn test_crosses_polygons_disjoint() {
        // Two polygons that don't cross (disjoint)
        let poly1 = create_square();
        assert!(poly1.is_ok());

        let coords2 = vec![
            Coordinate::new_2d(10.0, 10.0),
            Coordinate::new_2d(14.0, 10.0),
            Coordinate::new_2d(14.0, 14.0),
            Coordinate::new_2d(10.0, 14.0),
            Coordinate::new_2d(10.0, 10.0),
        ];
        let exterior2 = LineString::new(coords2);
        assert!(exterior2.is_ok());

        if let (Ok(p1), Ok(ext2)) = (poly1, exterior2) {
            let poly2 = Polygon::new(ext2, vec![]);
            assert!(poly2.is_ok());

            if let Ok(p2) = poly2 {
                let result = crosses(&p1, &p2);
                assert!(result.is_ok());
                if let Ok(do_cross) = result {
                    assert!(!do_cross, "Disjoint polygons should not cross");
                }
            }
        }
    }

    #[test]
    fn test_crosses_points() {
        let p1 = Point::new(1.0, 2.0);
        let p2 = Point::new(3.0, 4.0);

        let result = crosses(&p1, &p2);
        assert!(result.is_ok());
        if let Ok(do_cross) = result {
            assert!(!do_cross, "Points should not cross");
        }
    }

    #[test]
    fn test_touches_adjacent_polygons() {
        // Two polygons that share a boundary
        let poly1 = create_square();
        assert!(poly1.is_ok());

        let coords2 = vec![
            Coordinate::new_2d(4.0, 0.0),
            Coordinate::new_2d(8.0, 0.0),
            Coordinate::new_2d(8.0, 4.0),
            Coordinate::new_2d(4.0, 4.0),
            Coordinate::new_2d(4.0, 0.0),
        ];
        let exterior2 = LineString::new(coords2);
        assert!(exterior2.is_ok());

        if let (Ok(p1), Ok(ext2)) = (poly1, exterior2) {
            let poly2 = Polygon::new(ext2, vec![]);
            assert!(poly2.is_ok());

            if let Ok(p2) = poly2 {
                let result = touches(&p1, &p2);
                assert!(result.is_ok());
                if let Ok(do_touch) = result {
                    assert!(do_touch, "Adjacent polygons should touch");
                }
            }
        }
    }

    #[test]
    fn test_within_polygon() {
        // Small polygon within larger polygon
        let poly1 = create_square();
        assert!(poly1.is_ok());

        let coords2 = vec![
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(3.0, 1.0),
            Coordinate::new_2d(3.0, 3.0),
            Coordinate::new_2d(1.0, 3.0),
            Coordinate::new_2d(1.0, 1.0),
        ];
        let exterior2 = LineString::new(coords2);
        assert!(exterior2.is_ok());

        if let (Ok(p1), Ok(ext2)) = (poly1, exterior2) {
            let poly2 = Polygon::new(ext2, vec![]);
            assert!(poly2.is_ok());

            if let Ok(p2) = poly2 {
                let result = within(&p2, &p1);
                assert!(result.is_ok());
                if let Ok(is_within) = result {
                    assert!(is_within, "Small polygon should be within larger polygon");
                }
            }
        }
    }

    #[test]
    fn test_contains_polygon() {
        // Large polygon contains smaller polygon
        let poly1 = create_square();
        assert!(poly1.is_ok());

        let coords2 = vec![
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(3.0, 1.0),
            Coordinate::new_2d(3.0, 3.0),
            Coordinate::new_2d(1.0, 3.0),
            Coordinate::new_2d(1.0, 1.0),
        ];
        let exterior2 = LineString::new(coords2);
        assert!(exterior2.is_ok());

        if let (Ok(p1), Ok(ext2)) = (poly1, exterior2) {
            let poly2 = Polygon::new(ext2, vec![]);
            assert!(poly2.is_ok());

            if let Ok(p2) = poly2 {
                let result = contains(&p1, &p2);
                assert!(result.is_ok());
                if let Ok(does_contain) = result {
                    assert!(does_contain, "Large polygon should contain smaller polygon");
                }
            }
        }
    }

    #[test]
    fn test_disjoint_polygons_separated() {
        let poly1 = create_square();
        assert!(poly1.is_ok());

        let coords2 = vec![
            Coordinate::new_2d(10.0, 10.0),
            Coordinate::new_2d(14.0, 10.0),
            Coordinate::new_2d(14.0, 14.0),
            Coordinate::new_2d(10.0, 14.0),
            Coordinate::new_2d(10.0, 10.0),
        ];
        let exterior2 = LineString::new(coords2);
        assert!(exterior2.is_ok());

        if let (Ok(p1), Ok(ext2)) = (poly1, exterior2) {
            let poly2 = Polygon::new(ext2, vec![]);
            assert!(poly2.is_ok());

            if let Ok(p2) = poly2 {
                let result = disjoint(&p1, &p2);
                assert!(result.is_ok());
                if let Ok(are_disjoint) = result {
                    assert!(are_disjoint, "Separated polygons should be disjoint");
                }
            }
        }
    }
}
