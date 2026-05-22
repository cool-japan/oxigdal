//! Minimum bounding geometry algorithms.
//!
//! This module provides two fundamental bounding-geometry computations:
//!
//! 1. **Minimum-area rotated rectangle** via the rotating-calipers method on the
//!    convex hull.  Expected O(n log n) dominated by the hull computation.
//!
//! 2. **Smallest enclosing circle** via Welzl's randomised incremental algorithm.
//!    Expected O(n) after a one-time LCG shuffle (no `rand` dependency).
//!
//! # Stack depth note
//!
//! The recursive Welzl implementation has worst-case stack depth O(n).  For
//! inputs larger than ~100 000 points the recursion may overflow the default
//! thread stack.  If you need to handle very large point clouds, either run the
//! call in a thread spawned with a larger stack size, or switch to the iterative
//! Ritter / Shamos–Hoey version.

use crate::error::Result;
use crate::vector::convex_hull;
use oxigdal_core::vector::Coordinate;

// ─────────────────────────────────────────────────────────────────────────────
// Public types
// ─────────────────────────────────────────────────────────────────────────────

/// A minimum-area bounding rectangle (possibly rotated).
#[derive(Debug, Clone, PartialEq)]
pub struct RotatedRect {
    /// Centre of the rectangle in world coordinates.
    pub center: Coordinate,
    /// Extent along the primary axis (the edge direction, `angle_rad`).
    pub width: f64,
    /// Extent perpendicular to the primary axis.
    pub height: f64,
    /// Orientation in radians, counter-clockwise from the +X axis.
    pub angle_rad: f64,
}

impl RotatedRect {
    /// Returns the four corner points in CCW order.
    pub fn corners(&self) -> [Coordinate; 4] {
        let (cos_a, sin_a) = (self.angle_rad.cos(), self.angle_rad.sin());
        let hw = self.width * 0.5;
        let hh = self.height * 0.5;
        // Local offsets: (±hw, ±hh)
        let local: [(f64, f64); 4] = [(hw, hh), (-hw, hh), (-hw, -hh), (hw, -hh)];
        local.map(|(lx, ly)| {
            Coordinate::new_2d(
                self.center.x + lx * cos_a - ly * sin_a,
                self.center.y + lx * sin_a + ly * cos_a,
            )
        })
    }

    /// Area of the rectangle.
    #[inline]
    pub fn area(&self) -> f64 {
        self.width * self.height
    }
}

/// A circle defined by its centre and radius.
#[derive(Debug, Clone, PartialEq)]
pub struct Circle {
    /// Centre of the circle.
    pub center: Coordinate,
    /// Radius of the circle (always ≥ 0).
    pub radius: f64,
}

impl Circle {
    /// Returns `true` when `p` is inside or on the boundary of the circle.
    ///
    /// A small epsilon (1e-10) is added to the radius to absorb floating-point
    /// rounding that arises during the Welzl recursion.
    #[inline]
    pub fn contains(&self, p: Coordinate) -> bool {
        let dx = p.x - self.center.x;
        let dy = p.y - self.center.y;
        dx * dx + dy * dy <= (self.radius + 1e-10) * (self.radius + 1e-10)
    }

    /// The degenerate circle at the origin with zero radius.
    #[inline]
    pub fn zero() -> Self {
        Self {
            center: Coordinate::new_2d(0.0, 0.0),
            radius: 0.0,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Axis-aligned bounding box
// ─────────────────────────────────────────────────────────────────────────────

/// Computes the axis-aligned bounding box of a point set.
///
/// Returns `(min_x, min_y, max_x, max_y)`, or `None` for empty input.
pub fn aabb(points: &[Coordinate]) -> Option<(f64, f64, f64, f64)> {
    if points.is_empty() {
        return None;
    }
    let mut min_x = points[0].x;
    let mut min_y = points[0].y;
    let mut max_x = min_x;
    let mut max_y = min_y;
    for c in &points[1..] {
        if c.x < min_x {
            min_x = c.x;
        }
        if c.y < min_y {
            min_y = c.y;
        }
        if c.x > max_x {
            max_x = c.x;
        }
        if c.y > max_y {
            max_y = c.y;
        }
    }
    Some((min_x, min_y, max_x, max_y))
}

// ─────────────────────────────────────────────────────────────────────────────
// Minimum-area rotated rectangle (rotating calipers)
// ─────────────────────────────────────────────────────────────────────────────

/// Computes the minimum-area bounding rectangle for a point set.
///
/// Uses the *rotating calipers* technique on the convex hull.  The algorithm
/// iterates over each hull edge, projects all hull vertices onto the edge's
/// local frame, and records the extent.  The orientation that minimises the
/// product of the two extents is returned.
///
/// Returns `None` if the input has fewer than 2 points.
pub fn min_area_rotated_rect(points: &[Coordinate]) -> Option<RotatedRect> {
    if points.is_empty() {
        return None;
    }
    if points.len() == 1 {
        return Some(RotatedRect {
            center: points[0],
            width: 0.0,
            height: 0.0,
            angle_rad: 0.0,
        });
    }
    if points.len() == 2 {
        let (p, q) = (points[0], points[1]);
        let dx = q.x - p.x;
        let dy = q.y - p.y;
        let length = (dx * dx + dy * dy).sqrt();
        let angle = dy.atan2(dx);
        return Some(RotatedRect {
            center: Coordinate::new_2d((p.x + q.x) * 0.5, (p.y + q.y) * 0.5),
            width: length,
            height: 0.0,
            angle_rad: angle,
        });
    }

    // --- convex hull --------------------------------------------------------
    // convex_hull returns Err for < 3 points; we already handled that above.
    let hull: Vec<Coordinate> = match convex_hull(points) {
        Ok(h) => h,
        Err(_) => return None,
    };
    let n = hull.len();
    if n < 2 {
        return None;
    }
    // For a degenerate two-point hull (collinear case), fall through to the
    // edge-iteration loop which will handle it correctly.

    // --- rotating calipers --------------------------------------------------
    let mut best_area = f64::INFINITY;
    let mut best_rect: Option<RotatedRect> = None;

    for i in 0..n {
        let j = (i + 1) % n;
        let edge_x = hull[j].x - hull[i].x;
        let edge_y = hull[j].y - hull[i].y;
        let edge_len = (edge_x * edge_x + edge_y * edge_y).sqrt();
        if edge_len < 1e-12 {
            continue;
        }
        // Unit vector along the edge (u-axis)
        let ux = edge_x / edge_len;
        let uy = edge_y / edge_len;
        // Perpendicular unit vector (v-axis, 90° CCW)
        let vx = -uy;
        let vy = ux;

        // Project all hull points onto (u, v)
        let mut min_u = f64::INFINITY;
        let mut max_u = f64::NEG_INFINITY;
        let mut min_v = f64::INFINITY;
        let mut max_v = f64::NEG_INFINITY;
        for c in &hull {
            let u = c.x * ux + c.y * uy;
            let v = c.x * vx + c.y * vy;
            if u < min_u {
                min_u = u;
            }
            if u > max_u {
                max_u = u;
            }
            if v < min_v {
                min_v = v;
            }
            if v > max_v {
                max_v = v;
            }
        }

        let w = max_u - min_u;
        let h = max_v - min_v;
        let area = w * h;

        if area < best_area {
            best_area = area;

            // Centre in world coordinates:
            //   c_world = c_u * u_hat + c_v * v_hat
            let cu = (min_u + max_u) * 0.5;
            let cv = (min_v + max_v) * 0.5;
            let cx = cu * ux + cv * vx;
            let cy = cu * uy + cv * vy;

            best_rect = Some(RotatedRect {
                center: Coordinate::new_2d(cx, cy),
                width: w,
                height: h,
                // angle: direction of the u-axis in world frame
                angle_rad: uy.atan2(ux),
            });
        }
    }

    best_rect
}

// ─────────────────────────────────────────────────────────────────────────────
// Smallest enclosing circle (Welzl's algorithm)
// ─────────────────────────────────────────────────────────────────────────────

/// Circle through exactly one point (radius 0).
#[inline]
fn circle_from_1(a: Coordinate) -> Circle {
    Circle {
        center: a,
        radius: 0.0,
    }
}

/// Smallest circle with diameter defined by two points.
#[inline]
fn circle_from_2(a: Coordinate, b: Coordinate) -> Circle {
    let cx = (a.x + b.x) * 0.5;
    let cy = (a.y + b.y) * 0.5;
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    Circle {
        center: Coordinate::new_2d(cx, cy),
        radius: (dx * dx + dy * dy).sqrt() * 0.5,
    }
}

/// Circumscribed circle of three points, or `None` if they are collinear.
fn circumcircle(a: Coordinate, b: Coordinate, c: Coordinate) -> Option<Circle> {
    let ax = b.x - a.x;
    let ay = b.y - a.y;
    let bx = c.x - a.x;
    let by = c.y - a.y;
    let d = 2.0 * (ax * by - ay * bx);
    if d.abs() < 1e-12 {
        return None;
    }
    let a_sq = ax * ax + ay * ay;
    let b_sq = bx * bx + by * by;
    let ux = (by * a_sq - ay * b_sq) / d;
    let uy = (ax * b_sq - bx * a_sq) / d;
    let r = (ux * ux + uy * uy).sqrt();
    Some(Circle {
        center: Coordinate::new_2d(a.x + ux, a.y + uy),
        radius: r,
    })
}

/// Smallest circle determined solely by the `boundary` set (0–3 points).
#[inline]
fn trivial_circle(boundary: &[Coordinate]) -> Circle {
    match boundary.len() {
        0 => Circle::zero(),
        1 => circle_from_1(boundary[0]),
        2 => circle_from_2(boundary[0], boundary[1]),
        3 => circumcircle(boundary[0], boundary[1], boundary[2]).unwrap_or_else(|| {
            // Collinear: the enclosing circle is the diameter of the two
            // outermost points.
            let mut best = circle_from_2(boundary[0], boundary[1]);
            let c02 = circle_from_2(boundary[0], boundary[2]);
            let c12 = circle_from_2(boundary[1], boundary[2]);
            if c02.radius > best.radius {
                best = c02;
            }
            if c12.radius > best.radius {
                best = c12;
            }
            best
        }),
        _ => unreachable!("boundary set may not exceed 3 points"),
    }
}

/// Recursive Welzl helper.
///
/// `pts` is the (shuffled) full point slice; `n` is the count of points still
/// under consideration; `boundary` is the set of points on the current
/// boundary (at most 3).
fn welzl(pts: &[Coordinate], boundary: &mut Vec<Coordinate>, n: usize) -> Circle {
    if n == 0 || boundary.len() == 3 {
        return trivial_circle(boundary);
    }
    let p = pts[n - 1];
    let d = welzl(pts, boundary, n - 1);
    if d.contains(p) {
        return d;
    }
    boundary.push(p);
    let result = welzl(pts, boundary, n - 1);
    boundary.pop();
    result
}

/// Simple LCG (linear congruential generator) shuffle — no `rand` dependency.
///
/// Uses the Knuth/MMIX multiplicative constants with a Fisher–Yates sweep.
fn lcg_shuffle(v: &mut [Coordinate], seed: u64) {
    let n = v.len();
    if n <= 1 {
        return;
    }
    let mut state = seed;
    for i in (1..n).rev() {
        // Advance LCG state
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        // Map to index in [0, i]
        let j = (state >> 33) as usize % (i + 1);
        v.swap(i, j);
    }
}

/// Computes the smallest enclosing circle for a set of points.
///
/// Uses Welzl's randomised incremental algorithm (expected O(n)).  The shuffle
/// is driven by a deterministic LCG seeded from the coordinates, so results
/// are reproducible.
///
/// Returns a zero-radius circle at the origin for empty input.
///
/// # Stack depth
///
/// The underlying recursion has O(n) worst-case depth.  For inputs ≥ 100 000
/// points consider running in a thread with a larger stack.
pub fn smallest_enclosing_circle(points: &[Coordinate]) -> Circle {
    match points.len() {
        0 => Circle::zero(),
        1 => circle_from_1(points[0]),
        2 => circle_from_2(points[0], points[1]),
        _ => {
            let mut pts: Vec<Coordinate> = points.to_vec();
            // Seed from XOR of all coordinate bits — deterministic, avoids
            // clustering on regular grids by mixing both fields.
            let seed: u64 = pts.iter().fold(0u64, |acc, c| {
                acc.wrapping_add(c.x.to_bits() ^ c.y.to_bits())
            });
            lcg_shuffle(&mut pts, seed.wrapping_add(42));
            let mut boundary: Vec<Coordinate> = Vec::with_capacity(3);
            welzl(&pts, &mut boundary, pts.len())
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests (inline)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aabb_basic() {
        let pts = vec![
            Coordinate::new_2d(1.0, 2.0),
            Coordinate::new_2d(3.0, 0.0),
            Coordinate::new_2d(0.0, 4.0),
        ];
        let (min_x, min_y, max_x, max_y) = aabb(&pts).expect("non-empty");
        assert!((min_x - 0.0).abs() < 1e-12);
        assert!((min_y - 0.0).abs() < 1e-12);
        assert!((max_x - 3.0).abs() < 1e-12);
        assert!((max_y - 4.0).abs() < 1e-12);
    }

    #[test]
    fn test_rotated_rect_corners_roundtrip() {
        let rect = RotatedRect {
            center: Coordinate::new_2d(1.0, 1.0),
            width: 4.0,
            height: 2.0,
            angle_rad: 0.0,
        };
        let corners = rect.corners();
        // At angle 0 the corners should be axis-aligned
        assert!((corners[0].x - 3.0).abs() < 1e-10); // center.x + hw
        assert!((corners[0].y - 2.0).abs() < 1e-10); // center.y + hh
    }

    #[test]
    fn test_circle_contains_boundary() {
        let c = Circle {
            center: Coordinate::new_2d(0.0, 0.0),
            radius: 1.0,
        };
        // Exact boundary point — epsilon in `contains` should accept it
        assert!(c.contains(Coordinate::new_2d(1.0, 0.0)));
        assert!(!c.contains(Coordinate::new_2d(1.1, 0.0)));
    }
}
