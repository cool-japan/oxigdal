//! Fixed-capacity polyline type for `no_std`, `no_alloc` environments.
//!
//! [`FixedLineString`] stores up to `N` points inline, representing an
//! open polyline (sequence of connected line segments).

use crate::{BBox2D, NoAllocError, Point2D, f64_max, f64_min};

/// A fixed-capacity polyline (open line string) backed by an inline array.
///
/// Stores up to `N` [`Point2D`] vertices with no heap allocation.
pub struct FixedLineString<const N: usize> {
    points: [Point2D; N],
    len: usize,
}

impl<const N: usize> FixedLineString<N> {
    /// Creates an empty `FixedLineString`.
    #[must_use]
    #[inline]
    pub fn new() -> Self {
        Self {
            points: [Point2D::new(0.0, 0.0); N],
            len: 0,
        }
    }

    /// Attempts to append a point. Returns `Ok(())` on success or
    /// `Err(NoAllocError::CapacityExceeded)` if full.
    #[inline]
    pub fn push(&mut self, p: Point2D) -> Result<(), NoAllocError> {
        if self.len >= N {
            return Err(NoAllocError::CapacityExceeded);
        }
        self.points[self.len] = p;
        self.len += 1;
        Ok(())
    }

    /// Returns the number of points in the line string.
    #[must_use]
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if the line string contains no points.
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns the point at the given index, or `None` if out of bounds.
    #[must_use]
    #[inline]
    pub fn get(&self, index: usize) -> Option<&Point2D> {
        if index < self.len {
            Some(&self.points[index])
        } else {
            None
        }
    }

    /// Returns a slice of the current points.
    #[must_use]
    #[inline]
    pub fn as_slice(&self) -> &[Point2D] {
        &self.points[..self.len]
    }

    /// Computes the total length of the polyline (sum of segment lengths).
    ///
    /// Returns `0.0` for line strings with fewer than 2 points.
    #[must_use]
    pub fn total_length(&self) -> f64 {
        if self.len < 2 {
            return 0.0;
        }
        let pts = self.as_slice();
        let mut total = 0.0_f64;
        let mut i = 0;
        while i + 1 < self.len {
            total += pts[i].distance_to(&pts[i + 1]);
            i += 1;
        }
        total
    }

    /// Returns the axis-aligned bounding box, or `None` if empty.
    #[must_use]
    pub fn bbox(&self) -> Option<BBox2D> {
        if self.is_empty() {
            return None;
        }
        let pts = self.as_slice();
        let mut min_x = pts[0].x;
        let mut min_y = pts[0].y;
        let mut max_x = pts[0].x;
        let mut max_y = pts[0].y;
        let mut i = 1;
        while i < self.len {
            min_x = f64_min(min_x, pts[i].x);
            min_y = f64_min(min_y, pts[i].y);
            max_x = f64_max(max_x, pts[i].x);
            max_y = f64_max(max_y, pts[i].y);
            i += 1;
        }
        Some(BBox2D::new(min_x, min_y, max_x, max_y))
    }

    /// Returns an iterator over the points.
    #[inline]
    pub fn iter(&self) -> core::slice::Iter<'_, Point2D> {
        self.as_slice().iter()
    }

    /// Returns the capacity of this line string.
    #[must_use]
    #[inline]
    pub const fn capacity(&self) -> usize {
        N
    }

    /// Clears all points, resetting the length to zero.
    #[inline]
    pub fn clear(&mut self) {
        self.len = 0;
    }

    /// Returns the first point, or `None` if empty.
    #[must_use]
    #[inline]
    pub fn first(&self) -> Option<&Point2D> {
        if self.is_empty() {
            None
        } else {
            Some(&self.points[0])
        }
    }

    /// Returns the last point, or `None` if empty.
    #[must_use]
    #[inline]
    pub fn last(&self) -> Option<&Point2D> {
        if self.is_empty() {
            None
        } else {
            Some(&self.points[self.len - 1])
        }
    }
}

impl<const N: usize> Default for FixedLineString<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_empty() {
        let ls: FixedLineString<10> = FixedLineString::new();
        assert!(ls.is_empty());
        assert_eq!(ls.len(), 0);
        assert_eq!(ls.capacity(), 10);
    }

    #[test]
    fn test_push_and_get() {
        let mut ls: FixedLineString<4> = FixedLineString::new();
        assert!(ls.push(Point2D::new(1.0, 2.0)).is_ok());
        assert!(ls.push(Point2D::new(3.0, 4.0)).is_ok());
        assert_eq!(ls.len(), 2);
        assert!(!ls.is_empty());

        let p = ls.get(0);
        assert!(p.is_some());
        let p = p.expect("tested above");
        assert!((p.x - 1.0).abs() < f64::EPSILON);
        assert!((p.y - 2.0).abs() < f64::EPSILON);

        assert!(ls.get(2).is_none());
    }

    #[test]
    fn test_capacity_exceeded() {
        let mut ls: FixedLineString<2> = FixedLineString::new();
        assert!(ls.push(Point2D::new(0.0, 0.0)).is_ok());
        assert!(ls.push(Point2D::new(1.0, 1.0)).is_ok());
        let result = ls.push(Point2D::new(2.0, 2.0));
        assert_eq!(result, Err(NoAllocError::CapacityExceeded));
    }

    #[test]
    fn test_total_length_empty() {
        let ls: FixedLineString<10> = FixedLineString::new();
        assert!((ls.total_length() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_total_length_single_point() {
        let mut ls: FixedLineString<10> = FixedLineString::new();
        let _ = ls.push(Point2D::new(5.0, 5.0));
        assert!((ls.total_length() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_total_length_two_points() {
        let mut ls: FixedLineString<10> = FixedLineString::new();
        let _ = ls.push(Point2D::new(0.0, 0.0));
        let _ = ls.push(Point2D::new(3.0, 4.0));
        assert!((ls.total_length() - 5.0).abs() < 1e-10);
    }

    #[test]
    fn test_total_length_three_points() {
        let mut ls: FixedLineString<10> = FixedLineString::new();
        let _ = ls.push(Point2D::new(0.0, 0.0));
        let _ = ls.push(Point2D::new(3.0, 0.0));
        let _ = ls.push(Point2D::new(3.0, 4.0));
        assert!((ls.total_length() - 7.0).abs() < 1e-10);
    }

    #[test]
    fn test_bbox_empty() {
        let ls: FixedLineString<10> = FixedLineString::new();
        assert!(ls.bbox().is_none());
    }

    #[test]
    fn test_bbox_single_point() {
        let mut ls: FixedLineString<10> = FixedLineString::new();
        let _ = ls.push(Point2D::new(5.0, 10.0));
        let bb = ls.bbox();
        assert!(bb.is_some());
        let bb = bb.expect("tested above");
        assert!((bb.min_x - 5.0).abs() < f64::EPSILON);
        assert!((bb.max_x - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_bbox_multiple_points() {
        let mut ls: FixedLineString<10> = FixedLineString::new();
        let _ = ls.push(Point2D::new(1.0, 5.0));
        let _ = ls.push(Point2D::new(3.0, 2.0));
        let _ = ls.push(Point2D::new(-1.0, 8.0));
        let bb = ls.bbox().expect("non-empty line string");
        assert!((bb.min_x - (-1.0)).abs() < f64::EPSILON);
        assert!((bb.min_y - 2.0).abs() < f64::EPSILON);
        assert!((bb.max_x - 3.0).abs() < f64::EPSILON);
        assert!((bb.max_y - 8.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_iter() {
        let mut ls: FixedLineString<10> = FixedLineString::new();
        let _ = ls.push(Point2D::new(1.0, 2.0));
        let _ = ls.push(Point2D::new(3.0, 4.0));
        let collected: &[Point2D] = ls.as_slice();
        assert_eq!(collected.len(), 2);
    }

    #[test]
    fn test_first_last() {
        let mut ls: FixedLineString<10> = FixedLineString::new();
        assert!(ls.first().is_none());
        assert!(ls.last().is_none());

        let _ = ls.push(Point2D::new(1.0, 2.0));
        let _ = ls.push(Point2D::new(5.0, 6.0));
        let first = ls.first().expect("non-empty");
        let last = ls.last().expect("non-empty");
        assert!((first.x - 1.0).abs() < f64::EPSILON);
        assert!((last.x - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_clear() {
        let mut ls: FixedLineString<10> = FixedLineString::new();
        let _ = ls.push(Point2D::new(1.0, 2.0));
        ls.clear();
        assert!(ls.is_empty());
        assert_eq!(ls.len(), 0);
    }

    #[test]
    fn test_default() {
        let ls: FixedLineString<5> = FixedLineString::default();
        assert!(ls.is_empty());
    }
}
