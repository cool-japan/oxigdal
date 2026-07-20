//! Minimum bounding circle (smallest enclosing circle) using Welzl's algorithm.
//!
//! # Overview
//!
//! This module provides [`BoundingCircle`], a 2-D bounding circle type, together
//! with [`smallest_enclosing_circle`], which computes the unique smallest circle
//! that contains a given set of 2-D points.
//!
//! # Algorithm
//!
//! [`smallest_enclosing_circle`] uses the iterative variant of Welzl's randomized
//! algorithm.  The input is shuffled with a deterministic Knuth MMIX LCG so the
//! expected-O(n) average-case property holds while tests remain reproducible.
//!
//! [`smallest_enclosing_circle_from_bboxes`] is a convenience wrapper that flattens
//! bounding boxes into their four corner points before calling the core function.

use std::f64::consts::PI;

use crate::bbox::Bbox2D;

// ---------------------------------------------------------------------------
// BoundingCircle
// ---------------------------------------------------------------------------

/// A 2-D bounding circle.
///
/// The circle is parameterised by its centre `(center_x, center_y)` and a
/// non-negative `radius`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundingCircle {
    /// X-coordinate of the circle centre.
    pub center_x: f64,
    /// Y-coordinate of the circle centre.
    pub center_y: f64,
    /// Radius of the circle.  Always `>= 0.0`.
    pub radius: f64,
}

impl BoundingCircle {
    // -----------------------------------------------------------------------
    // Constructors
    // -----------------------------------------------------------------------

    /// A degenerate circle at the origin with radius 0.
    #[inline]
    pub fn empty() -> Self {
        Self {
            center_x: 0.0,
            center_y: 0.0,
            radius: 0.0,
        }
    }

    /// A circle of radius 0 centred on the point `p`.
    #[inline]
    pub fn from_point(p: (f64, f64)) -> Self {
        Self {
            center_x: p.0,
            center_y: p.1,
            radius: 0.0,
        }
    }

    /// The unique circle whose diameter is the segment `[a, b]`.
    ///
    /// The centre is the midpoint of `a` and `b`, and the radius is half the
    /// Euclidean distance between them.
    #[inline]
    pub fn from_two(a: (f64, f64), b: (f64, f64)) -> Self {
        let cx = (a.0 + b.0) * 0.5;
        let cy = (a.1 + b.1) * 0.5;
        let dx = b.0 - a.0;
        let dy = b.1 - a.1;
        let r = (dx * dx + dy * dy).sqrt() * 0.5;
        Self {
            center_x: cx,
            center_y: cy,
            radius: r,
        }
    }

    /// The unique circumscribed circle through three non-collinear points.
    ///
    /// Returns `None` when the three points are collinear, detected by
    /// `|2D signed area| < 1e-12`.
    pub fn from_three(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> Option<Self> {
        let (ax, ay) = a;
        let (bx, by) = b;
        let (cx, cy) = c;

        // D = 2 * (signed area of the triangle).  Collinear if |D| < epsilon.
        let d = 2.0 * (ax * (by - cy) + bx * (cy - ay) + cx * (ay - by));
        if d.abs() < 1e-12 {
            return None;
        }

        // Squared magnitudes of position vectors.
        let a_sq = ax * ax + ay * ay;
        let b_sq = bx * bx + by * by;
        let c_sq = cx * cx + cy * cy;

        // Circumcenter coordinates via perpendicular-bisector formula.
        let ux = (a_sq * (by - cy) + b_sq * (cy - ay) + c_sq * (ay - by)) / d;
        let uy = (a_sq * (cx - bx) + b_sq * (ax - cx) + c_sq * (bx - ax)) / d;

        // Circumradius = distance from circumcenter to any vertex.
        let dx = ux - ax;
        let dy = uy - ay;
        let r = (dx * dx + dy * dy).sqrt();

        Some(Self {
            center_x: ux,
            center_y: uy,
            radius: r,
        })
    }

    // -----------------------------------------------------------------------
    // Containment / intersection queries
    // -----------------------------------------------------------------------

    /// Returns `true` if point `p` is inside or on the circle boundary.
    ///
    /// An epsilon of `1e-10` (added to the radius) is used to absorb
    /// floating-point rounding accumulated during circumcenter computation.
    #[inline]
    pub fn contains_point(&self, p: (f64, f64)) -> bool {
        let dx = p.0 - self.center_x;
        let dy = p.1 - self.center_y;
        let dist_sq = dx * dx + dy * dy;
        let r_eps = (self.radius + 1e-10).max(0.0);
        dist_sq <= r_eps * r_eps
    }

    /// Returns `true` if point `p` is **strictly** inside the circle (no epsilon).
    #[inline]
    pub fn contains_point_strict(&self, p: (f64, f64)) -> bool {
        let dx = p.0 - self.center_x;
        let dy = p.1 - self.center_y;
        let dist_sq = dx * dx + dy * dy;
        dist_sq < self.radius * self.radius
    }

    /// Returns `true` if the circle intersects (or touches) the axis-aligned bbox.
    ///
    /// Uses the closest-point-on-rectangle algorithm: clamp the circle centre
    /// to the rectangle, then test whether the squared distance to that nearest
    /// point is within the squared radius (plus a small epsilon for touching).
    #[inline]
    pub fn intersects_bbox(&self, bbox: &Bbox2D) -> bool {
        let cx_clamp = self.center_x.clamp(bbox.min_x, bbox.max_x);
        let cy_clamp = self.center_y.clamp(bbox.min_y, bbox.max_y);
        let dx = self.center_x - cx_clamp;
        let dy = self.center_y - cy_clamp;
        let dist_sq = dx * dx + dy * dy;
        dist_sq <= self.radius * self.radius + 1e-10
    }

    // -----------------------------------------------------------------------
    // Derived quantities
    // -----------------------------------------------------------------------

    /// Area of the circle: `π · r²`.
    #[inline]
    pub fn area(&self) -> f64 {
        PI * self.radius * self.radius
    }

    /// Diameter of the circle: `2 · r`.
    #[inline]
    pub fn diameter(&self) -> f64 {
        2.0 * self.radius
    }
}

// ---------------------------------------------------------------------------
// LCG shuffle (Knuth MMIX)
// ---------------------------------------------------------------------------

/// Fisher-Yates in-place shuffle using the Knuth MMIX linear congruential
/// generator.
///
/// LCG recurrence:
/// ```text
/// state = state.wrapping_mul(6364136223846793005)
///              .wrapping_add(1442695040888963407)
/// ```
/// Index is derived from the upper 31 bits for better statistical quality:
/// `index = (state >> 33) as usize % (i + 1)`.
fn lcg_shuffle<T>(arr: &mut [T], seed: u64) {
    let n = arr.len();
    if n <= 1 {
        return;
    }
    let mut state: u64 = seed;
    for i in (1..n).rev() {
        // Advance state.
        state = state
            .wrapping_mul(6_364_136_223_846_793_005_u64)
            .wrapping_add(1_442_695_040_888_963_407_u64);
        // Use upper 31 bits for the index.
        let j = (state >> 33) as usize % (i + 1);
        arr.swap(i, j);
    }
}

// ---------------------------------------------------------------------------
// Welzl helpers
// ---------------------------------------------------------------------------

/// Build the minimum enclosing circle from 0, 1, 2, or 3 boundary support
/// points.
///
/// - 0 points → degenerate circle at origin (should not occur in practice)
/// - 1 point  → circle of radius 0
/// - 2 points → diameter circle
/// - 3 points → circumscribed circle; falls back to the diameter of the
///   longest pair when the three are collinear
fn circle_from_boundary(boundary: &[(f64, f64)]) -> BoundingCircle {
    match boundary.len() {
        0 => BoundingCircle::empty(),
        1 => BoundingCircle::from_point(boundary[0]),
        2 => BoundingCircle::from_two(boundary[0], boundary[1]),
        3 => {
            let a = boundary[0];
            let b = boundary[1];
            let c = boundary[2];
            BoundingCircle::from_three(a, b, c).unwrap_or_else(|| {
                // Collinear: take the diameter of the longest pair.
                let ab = BoundingCircle::from_two(a, b);
                let ac = BoundingCircle::from_two(a, c);
                let bc = BoundingCircle::from_two(b, c);
                // Return whichever diameter circle is largest.
                if ab.radius >= ac.radius && ab.radius >= bc.radius {
                    ab
                } else if ac.radius >= bc.radius {
                    ac
                } else {
                    bc
                }
            })
        }
        _ => unreachable!("boundary has at most 3 points"),
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Compute the smallest enclosing circle of a set of 2-D points.
///
/// # Algorithm
///
/// Uses the iterative variant of Welzl's randomized algorithm (expected O(n)).
/// A local copy of the points is shuffled using the deterministic Knuth MMIX
/// LCG (seed `0x12345678`) before processing, so the expected-O(n) property
/// holds while results are deterministic.
///
/// # Edge cases
///
/// * Empty input → [`BoundingCircle::empty()`].
/// * Single point → circle of radius 0 at that point.
/// * Two points → [`BoundingCircle::from_two`].
pub fn smallest_enclosing_circle(points: &[(f64, f64)]) -> BoundingCircle {
    if points.is_empty() {
        return BoundingCircle::empty();
    }

    // Work on a shuffled copy so the caller's slice is not mutated.
    let mut pts: Vec<(f64, f64)> = points.to_vec();
    lcg_shuffle(&mut pts, 0x1234_5678);

    // Iterative Welzl (Shamos-Hoey / Welzl incremental variant):
    //
    // Process points one at a time.  When a new point falls outside the
    // current circle it must lie on the boundary of the optimal circle, so we
    // restart with that point fixed on the boundary and repeat for all prior
    // points.
    let mut circle = circle_from_boundary(&[pts[0]]);

    for i in 1..pts.len() {
        if !circle.contains_point(pts[i]) {
            // pts[i] must lie on the boundary of the optimal circle for pts[0..=i].
            circle = circle_from_boundary(&[pts[i]]);
            for j in 0..i {
                if !circle.contains_point(pts[j]) {
                    // pts[j] is also on the boundary; now determined by {pts[i], pts[j]}.
                    circle = circle_from_boundary(&[pts[i], pts[j]]);
                    for k in 0..j {
                        if !circle.contains_point(pts[k]) {
                            // All three points are on the boundary.
                            circle = circle_from_boundary(&[pts[i], pts[j], pts[k]]);
                        }
                    }
                }
            }
        }
    }

    circle
}

/// Compute the smallest enclosing circle of a set of [`Bbox2D`] bounding boxes.
///
/// Flattens each bounding box into its four corner points, then delegates to
/// [`smallest_enclosing_circle`].
pub fn smallest_enclosing_circle_from_bboxes(bboxes: &[Bbox2D]) -> BoundingCircle {
    let mut pts: Vec<(f64, f64)> = Vec::with_capacity(bboxes.len() * 4);
    for bb in bboxes {
        pts.push((bb.min_x, bb.min_y));
        pts.push((bb.max_x, bb.min_y));
        pts.push((bb.max_x, bb.max_y));
        pts.push((bb.min_x, bb.max_y));
    }
    smallest_enclosing_circle(&pts)
}

// ---------------------------------------------------------------------------
// Module-level unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_at_origin() {
        let c = BoundingCircle::empty();
        assert_eq!(c.center_x, 0.0);
        assert_eq!(c.center_y, 0.0);
        assert_eq!(c.radius, 0.0);
    }

    #[test]
    fn from_two_midpoint_and_half_dist() {
        let c = BoundingCircle::from_two((0.0, 0.0), (4.0, 0.0));
        assert!((c.center_x - 2.0).abs() < 1e-12);
        assert!((c.center_y).abs() < 1e-12);
        assert!((c.radius - 2.0).abs() < 1e-12);
    }

    #[test]
    fn from_three_circumcircle_equilateral() {
        // Equilateral triangle with vertices at known positions.
        let a = (0.0_f64, 0.0_f64);
        let b = (2.0, 0.0);
        let c = (1.0, 3.0_f64.sqrt());
        let circle = BoundingCircle::from_three(a, b, c).expect("non-collinear");
        // Circumradius of equilateral triangle side s = s / sqrt(3).
        let expected_r = 2.0_f64 / 3.0_f64.sqrt();
        assert!((circle.radius - expected_r).abs() < 1e-10);
        // Circumcenter at (1, 1/sqrt(3)).
        assert!((circle.center_x - 1.0).abs() < 1e-10);
        assert!((circle.center_y - 1.0 / 3.0_f64.sqrt()).abs() < 1e-10);
    }

    #[test]
    fn from_three_collinear_returns_none() {
        let result = BoundingCircle::from_three((0.0, 0.0), (1.0, 0.0), (2.0, 0.0));
        assert!(result.is_none());
    }

    #[test]
    fn lcg_shuffle_changes_order_deterministically() {
        let mut v = vec![1, 2, 3, 4, 5, 6, 7, 8];
        lcg_shuffle(&mut v, 42);
        // Must be a permutation.
        let mut sorted = v.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, vec![1, 2, 3, 4, 5, 6, 7, 8]);
        // Must be deterministic.
        let mut v2 = vec![1, 2, 3, 4, 5, 6, 7, 8];
        lcg_shuffle(&mut v2, 42);
        assert_eq!(v, v2);
    }

    #[test]
    fn area_and_diameter() {
        let c = BoundingCircle {
            center_x: 0.0,
            center_y: 0.0,
            radius: 3.0,
        };
        assert!((c.area() - PI * 9.0).abs() < 1e-12);
        assert!((c.diameter() - 6.0).abs() < 1e-12);
    }

    #[test]
    fn contains_point_and_strict() {
        let c = BoundingCircle {
            center_x: 1.0,
            center_y: 1.0,
            radius: 1.0,
        };
        // Centre is strictly inside.
        assert!(c.contains_point_strict((1.0, 1.0)));
        // Point exactly on the boundary is inside (with epsilon).
        assert!(c.contains_point((2.0, 1.0)));
        // Point just outside.
        assert!(!c.contains_point((2.0 + 1e-9, 1.0)));
    }

    #[test]
    fn intersects_bbox_basic() {
        let c = BoundingCircle {
            center_x: 0.0,
            center_y: 0.0,
            radius: 1.0,
        };
        let overlapping = Bbox2D::new(0.5, 0.5, 2.0, 2.0).unwrap();
        let disjoint = Bbox2D::new(5.0, 5.0, 6.0, 6.0).unwrap();
        assert!(c.intersects_bbox(&overlapping));
        assert!(!c.intersects_bbox(&disjoint));
    }

    #[test]
    fn circle_from_boundary_0() {
        let c = circle_from_boundary(&[]);
        assert_eq!(c.radius, 0.0);
    }

    #[test]
    fn circle_from_boundary_collinear_3() {
        let c = circle_from_boundary(&[(0.0, 0.0), (1.0, 0.0), (3.0, 0.0)]);
        // Should be diameter of longest pair (0,0)-(3,0), radius 1.5.
        assert!((c.radius - 1.5).abs() < 1e-12);
    }
}
