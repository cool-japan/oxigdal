//! Fixed-capacity closed ring type for `no_std`, `no_alloc` environments.
//!
//! [`FixedRing`] stores up to `N` vertices inline, representing a closed ring
//! (the last vertex is implicitly connected back to the first). Provides
//! signed area (shoelace), winding direction, and point-in-ring (ray casting).

use crate::{NoAllocError, Point2D};

/// A fixed-capacity closed ring backed by an inline array.
///
/// The ring is implicitly closed: the edge from `vertices[len-1]` back to
/// `vertices[0]` is always present when the ring has 3+ vertices.
pub struct FixedRing<const N: usize> {
    vertices: [Point2D; N],
    len: usize,
}

impl<const N: usize> FixedRing<N> {
    /// Creates an empty `FixedRing`.
    #[must_use]
    #[inline]
    pub fn new() -> Self {
        Self {
            vertices: [Point2D::new(0.0, 0.0); N],
            len: 0,
        }
    }

    /// Attempts to append a vertex. Returns `Ok(())` on success or
    /// `Err(NoAllocError::CapacityExceeded)` if the ring is full.
    #[inline]
    pub fn push(&mut self, p: Point2D) -> Result<(), NoAllocError> {
        if self.len >= N {
            return Err(NoAllocError::CapacityExceeded);
        }
        self.vertices[self.len] = p;
        self.len += 1;
        Ok(())
    }

    /// Returns the number of vertices.
    #[must_use]
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if the ring has no vertices.
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns a slice of the current vertices.
    #[must_use]
    #[inline]
    pub fn vertices(&self) -> &[Point2D] {
        &self.vertices[..self.len]
    }

    /// Returns the vertex at the given index, or `None` if out of bounds.
    #[must_use]
    #[inline]
    pub fn get(&self, index: usize) -> Option<&Point2D> {
        if index < self.len {
            Some(&self.vertices[index])
        } else {
            None
        }
    }

    /// Computes the signed area using the shoelace formula.
    ///
    /// Positive for counter-clockwise winding, negative for clockwise.
    /// Returns `0.0` if the ring has fewer than 3 vertices.
    #[must_use]
    pub fn signed_area(&self) -> f64 {
        if self.len < 3 {
            return 0.0;
        }
        let verts = self.vertices();
        let mut sum = 0.0_f64;
        let mut i = 0;
        while i < self.len {
            let j = if i + 1 < self.len { i + 1 } else { 0 };
            sum += verts[i].x * verts[j].y;
            sum -= verts[j].x * verts[i].y;
            i += 1;
        }
        sum * 0.5
    }

    /// Computes the absolute area using the shoelace formula.
    #[must_use]
    #[inline]
    pub fn area(&self) -> f64 {
        let sa = self.signed_area();
        if sa < 0.0 { -sa } else { sa }
    }

    /// Returns `true` if the ring's vertices are ordered counter-clockwise.
    ///
    /// A ring with fewer than 3 vertices is considered neither CW nor CCW
    /// and returns `false`.
    #[must_use]
    #[inline]
    pub fn is_counter_clockwise(&self) -> bool {
        self.signed_area() > 0.0
    }

    /// Returns `true` if the ring's vertices are ordered clockwise.
    #[must_use]
    #[inline]
    pub fn is_clockwise(&self) -> bool {
        self.signed_area() < 0.0
    }

    /// Tests whether a point lies inside or on the boundary of the ring
    /// using the ray-casting algorithm.
    ///
    /// A horizontal ray from the point toward +X is tested against each edge
    /// of the ring. An odd number of crossings means the point is inside.
    ///
    /// Returns `false` for rings with fewer than 3 vertices.
    #[must_use]
    pub fn contains_point(&self, p: Point2D) -> bool {
        if self.len < 3 {
            return false;
        }
        let verts = self.vertices();
        let mut inside = false;
        let mut j = self.len - 1;
        let mut i = 0;
        while i < self.len {
            let vi = &verts[i];
            let vj = &verts[j];

            // Check if the edge (vj -> vi) crosses the horizontal ray from p
            let yi_above = vi.y > p.y;
            let yj_above = vj.y > p.y;
            if yi_above != yj_above {
                // Compute the X coordinate of the edge at y = p.y
                let slope_inv = (vj.x - vi.x) / (vj.y - vi.y);
                let x_intersect = vi.x + (p.y - vi.y) * slope_inv;
                if p.x < x_intersect {
                    inside = !inside;
                }
            }
            j = i;
            i += 1;
        }
        inside
    }

    /// Computes the perimeter (sum of edge lengths including the closing edge).
    ///
    /// Returns `0.0` for rings with fewer than 2 vertices.
    #[must_use]
    pub fn perimeter(&self) -> f64 {
        if self.len < 2 {
            return 0.0;
        }
        let verts = self.vertices();
        let mut total = 0.0_f64;
        let mut i = 0;
        while i < self.len {
            let j = if i + 1 < self.len { i + 1 } else { 0 };
            total += verts[i].distance_to(&verts[j]);
            i += 1;
        }
        total
    }

    /// Returns the capacity of this ring.
    #[must_use]
    #[inline]
    pub const fn capacity(&self) -> usize {
        N
    }

    /// Clears all vertices, resetting the length to zero.
    #[inline]
    pub fn clear(&mut self) {
        self.len = 0;
    }
}

impl<const N: usize> Default for FixedRing<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ccw_square() -> FixedRing<8> {
        let mut ring: FixedRing<8> = FixedRing::new();
        // CCW square: (0,0) -> (1,0) -> (1,1) -> (0,1)
        let _ = ring.push(Point2D::new(0.0, 0.0));
        let _ = ring.push(Point2D::new(1.0, 0.0));
        let _ = ring.push(Point2D::new(1.0, 1.0));
        let _ = ring.push(Point2D::new(0.0, 1.0));
        ring
    }

    fn make_cw_square() -> FixedRing<8> {
        let mut ring: FixedRing<8> = FixedRing::new();
        // CW square: (0,0) -> (0,1) -> (1,1) -> (1,0)
        let _ = ring.push(Point2D::new(0.0, 0.0));
        let _ = ring.push(Point2D::new(0.0, 1.0));
        let _ = ring.push(Point2D::new(1.0, 1.0));
        let _ = ring.push(Point2D::new(1.0, 0.0));
        ring
    }

    #[test]
    fn test_new_empty() {
        let ring: FixedRing<10> = FixedRing::new();
        assert!(ring.is_empty());
        assert_eq!(ring.len(), 0);
        assert_eq!(ring.capacity(), 10);
    }

    #[test]
    fn test_push_and_get() {
        let mut ring: FixedRing<4> = FixedRing::new();
        assert!(ring.push(Point2D::new(1.0, 2.0)).is_ok());
        assert!(ring.push(Point2D::new(3.0, 4.0)).is_ok());
        assert_eq!(ring.len(), 2);

        let p = ring.get(0).expect("index 0 exists");
        assert!((p.x - 1.0).abs() < f64::EPSILON);
        assert!(ring.get(5).is_none());
    }

    #[test]
    fn test_capacity_exceeded() {
        let mut ring: FixedRing<2> = FixedRing::new();
        assert!(ring.push(Point2D::new(0.0, 0.0)).is_ok());
        assert!(ring.push(Point2D::new(1.0, 1.0)).is_ok());
        assert_eq!(
            ring.push(Point2D::new(2.0, 2.0)),
            Err(NoAllocError::CapacityExceeded)
        );
    }

    #[test]
    fn test_signed_area_ccw() {
        let ring = make_ccw_square();
        let area = ring.signed_area();
        assert!(
            (area - 1.0).abs() < 1e-10,
            "CCW square should have positive area ~1.0, got {area}"
        );
    }

    #[test]
    fn test_signed_area_cw() {
        let ring = make_cw_square();
        let area = ring.signed_area();
        assert!(
            (area - (-1.0)).abs() < 1e-10,
            "CW square should have negative area ~-1.0, got {area}"
        );
    }

    #[test]
    fn test_area_absolute() {
        let ccw = make_ccw_square();
        let cw = make_cw_square();
        assert!((ccw.area() - 1.0).abs() < 1e-10);
        assert!((cw.area() - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_area_degenerate() {
        let ring: FixedRing<10> = FixedRing::new();
        assert!((ring.area() - 0.0).abs() < f64::EPSILON);

        let mut ring2: FixedRing<10> = FixedRing::new();
        let _ = ring2.push(Point2D::new(0.0, 0.0));
        let _ = ring2.push(Point2D::new(1.0, 1.0));
        assert!((ring2.area() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_is_ccw_cw() {
        let ccw = make_ccw_square();
        let cw = make_cw_square();
        assert!(ccw.is_counter_clockwise());
        assert!(!ccw.is_clockwise());
        assert!(cw.is_clockwise());
        assert!(!cw.is_counter_clockwise());
    }

    #[test]
    fn test_contains_point_inside() {
        let ring = make_ccw_square();
        assert!(ring.contains_point(Point2D::new(0.5, 0.5)));
        assert!(ring.contains_point(Point2D::new(0.1, 0.9)));
    }

    #[test]
    fn test_contains_point_outside() {
        let ring = make_ccw_square();
        assert!(!ring.contains_point(Point2D::new(2.0, 0.5)));
        assert!(!ring.contains_point(Point2D::new(-0.1, 0.5)));
        assert!(!ring.contains_point(Point2D::new(0.5, -0.1)));
        assert!(!ring.contains_point(Point2D::new(0.5, 1.1)));
    }

    #[test]
    fn test_contains_point_degenerate() {
        let ring: FixedRing<10> = FixedRing::new();
        assert!(!ring.contains_point(Point2D::new(0.0, 0.0)));
    }

    #[test]
    fn test_contains_point_triangle() {
        let mut ring: FixedRing<8> = FixedRing::new();
        let _ = ring.push(Point2D::new(0.0, 0.0));
        let _ = ring.push(Point2D::new(4.0, 0.0));
        let _ = ring.push(Point2D::new(2.0, 3.0));
        assert!(ring.contains_point(Point2D::new(2.0, 1.0)));
        assert!(!ring.contains_point(Point2D::new(0.0, 3.0)));
    }

    #[test]
    fn test_perimeter() {
        let ring = make_ccw_square();
        assert!((ring.perimeter() - 4.0).abs() < 1e-10);
    }

    #[test]
    fn test_perimeter_degenerate() {
        let ring: FixedRing<10> = FixedRing::new();
        assert!((ring.perimeter() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_clear() {
        let mut ring = make_ccw_square();
        ring.clear();
        assert!(ring.is_empty());
    }

    #[test]
    fn test_default() {
        let ring: FixedRing<5> = FixedRing::default();
        assert!(ring.is_empty());
    }

    #[test]
    fn test_vertices_slice() {
        let ring = make_ccw_square();
        let verts = ring.vertices();
        assert_eq!(verts.len(), 4);
    }

    #[test]
    fn test_contains_point_cw_ring() {
        // Ray casting works regardless of winding direction
        let ring = make_cw_square();
        assert!(ring.contains_point(Point2D::new(0.5, 0.5)));
        assert!(!ring.contains_point(Point2D::new(2.0, 2.0)));
    }
}
