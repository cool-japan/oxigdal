//! Geometry operations on polygons and coordinate sequences.
//!
//! Provides area (shoelace), perimeter, centroid, point-in-polygon (ray
//! casting), Douglas–Peucker simplification, Graham-scan convex hull, and
//! bounding-box utilities.

use crate::validation::{Coord, Polygon};

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// Distance
// ---------------------------------------------------------------------------

/// Euclidean distance between two coordinates.
#[inline]
pub fn distance(a: &Coord, b: &Coord) -> f64 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    (dx * dx + dy * dy).sqrt()
}

// ---------------------------------------------------------------------------
// Signed area (internal)
// ---------------------------------------------------------------------------

/// Signed area of a coordinate slice (shoelace formula).
///
/// Positive ⇒ counter-clockwise; negative ⇒ clockwise.
fn signed_area_slice(coords: &[Coord]) -> f64 {
    if coords.len() < 3 {
        return 0.0;
    }
    let n = coords.len();
    let mut sum = 0.0_f64;
    for i in 0..n {
        let j = (i + 1) % n;
        sum += coords[i].x * coords[j].y;
        sum -= coords[j].x * coords[i].y;
    }
    sum * 0.5
}

// ---------------------------------------------------------------------------
// Area
// ---------------------------------------------------------------------------

/// Unsigned area of a polygon using the shoelace formula.
///
/// Hole areas are subtracted from the exterior area.
pub fn area(polygon: &Polygon) -> f64 {
    let ext = signed_area_slice(polygon.exterior.coords()).abs();
    let holes: f64 = polygon
        .holes
        .iter()
        .map(|h| signed_area_slice(h.coords()).abs())
        .sum();
    (ext - holes).abs()
}

// ---------------------------------------------------------------------------
// Perimeter
// ---------------------------------------------------------------------------

/// Perimeter of the exterior ring of a polygon.
pub fn perimeter(polygon: &Polygon) -> f64 {
    ring_perimeter(polygon.exterior.coords())
}

/// Perimeter of a coordinate sequence (sum of consecutive edge lengths).
fn ring_perimeter(coords: &[Coord]) -> f64 {
    if coords.len() < 2 {
        return 0.0;
    }
    let mut sum = 0.0_f64;
    for i in 0..coords.len() - 1 {
        sum += distance(&coords[i], &coords[i + 1]);
    }
    sum
}

// ---------------------------------------------------------------------------
// Centroid
// ---------------------------------------------------------------------------

/// Area-weighted centroid of a polygon (exterior only; holes are not
/// considered in the weighting).
///
/// Falls back to the arithmetic mean of coordinates when area is zero.
pub fn centroid(polygon: &Polygon) -> Coord {
    let coords = polygon.exterior.coords();
    if coords.is_empty() {
        return Coord::new(0.0, 0.0);
    }

    let a = signed_area_slice(coords);
    if a.abs() < 1e-15 {
        // Degenerate — use arithmetic mean.
        let n = coords.len() as f64;
        let cx = coords.iter().map(|c| c.x).sum::<f64>() / n;
        let cy = coords.iter().map(|c| c.y).sum::<f64>() / n;
        return Coord::new(cx, cy);
    }

    let mut cx = 0.0_f64;
    let mut cy = 0.0_f64;
    let n = coords.len();
    for i in 0..n {
        let j = (i + 1) % n;
        let cross = coords[i].x * coords[j].y - coords[j].x * coords[i].y;
        cx += (coords[i].x + coords[j].x) * cross;
        cy += (coords[i].y + coords[j].y) * cross;
    }
    let factor = 1.0 / (6.0 * a);
    Coord::new(cx * factor, cy * factor)
}

// ---------------------------------------------------------------------------
// Point-in-polygon (ray casting)
// ---------------------------------------------------------------------------

/// Determine whether `point` lies inside `polygon` using the ray-casting
/// algorithm.
///
/// A point that falls inside a hole is considered **outside** the polygon.
pub fn point_in_polygon(point: &Coord, polygon: &Polygon) -> bool {
    if !point_in_ring(point, polygon.exterior.coords()) {
        return false;
    }
    // If inside any hole, the point is outside the polygon.
    for hole in &polygon.holes {
        if point_in_ring(point, hole.coords()) {
            return false;
        }
    }
    true
}

/// Ray-casting test against a single ring.
fn point_in_ring(point: &Coord, coords: &[Coord]) -> bool {
    let n = coords.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let ci = &coords[i];
        let cj = &coords[j];
        if ((ci.y > point.y) != (cj.y > point.y))
            && (point.x < (cj.x - ci.x) * (point.y - ci.y) / (cj.y - ci.y) + ci.x)
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

// ---------------------------------------------------------------------------
// Douglas–Peucker simplification
// ---------------------------------------------------------------------------

/// Simplify a polyline using the Douglas–Peucker algorithm.
///
/// `epsilon` is the maximum perpendicular distance tolerance.  The first and
/// last points are always retained.
pub fn simplify(coords: &[Coord], epsilon: f64) -> Vec<Coord> {
    if coords.len() < 3 {
        return coords.to_vec();
    }
    let mut keep = vec![false; coords.len()];
    keep[0] = true;
    keep[coords.len() - 1] = true;
    dp_recurse(coords, 0, coords.len() - 1, epsilon, &mut keep);
    coords
        .iter()
        .zip(keep.iter())
        .filter(|(_, k)| **k)
        .map(|(c, _)| *c)
        .collect()
}

fn dp_recurse(coords: &[Coord], start: usize, end: usize, epsilon: f64, keep: &mut [bool]) {
    if end <= start + 1 {
        return;
    }
    let mut max_dist = 0.0_f64;
    let mut max_idx = start;
    let a = &coords[start];
    let b = &coords[end];
    for (i, c) in coords.iter().enumerate().take(end).skip(start + 1) {
        let d = perpendicular_distance(c, a, b);
        if d > max_dist {
            max_dist = d;
            max_idx = i;
        }
    }
    if max_dist > epsilon {
        keep[max_idx] = true;
        dp_recurse(coords, start, max_idx, epsilon, keep);
        dp_recurse(coords, max_idx, end, epsilon, keep);
    }
}

/// Perpendicular distance from `p` to the line segment `a→b`.
fn perpendicular_distance(p: &Coord, a: &Coord, b: &Coord) -> f64 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let len_sq = dx * dx + dy * dy;
    if len_sq < 1e-20 {
        return distance(p, a);
    }
    let numerator = ((dy * p.x) - (dx * p.y) + (b.x * a.y) - (b.y * a.x)).abs();
    numerator / len_sq.sqrt()
}

// ---------------------------------------------------------------------------
// Convex hull (Graham scan)
// ---------------------------------------------------------------------------

/// Compute the convex hull of a set of points using Graham scan.
///
/// Returns the hull vertices in counter-clockwise order.  If all points are
/// collinear or there are fewer than 3 unique points, the result contains
/// just those points without forming a proper polygon.
pub fn convex_hull(points: &[Coord]) -> Vec<Coord> {
    if points.len() < 2 {
        return points.to_vec();
    }

    // Find the point with the lowest y (then lowest x) as the pivot.
    let mut pivot_idx = 0;
    for (i, p) in points.iter().enumerate() {
        if p.y < points[pivot_idx].y || (p.y == points[pivot_idx].y && p.x < points[pivot_idx].x) {
            pivot_idx = i;
        }
    }
    let pivot = points[pivot_idx];

    // Sort by polar angle from pivot.
    let mut sorted: Vec<Coord> = points.to_vec();
    sorted.swap(0, pivot_idx);
    let rest = &mut sorted[1..];
    rest.sort_by(|a, b| {
        let cross = cross_2d(&pivot, a, b);
        if cross.abs() < 1e-10 {
            // Collinear — nearer point first.
            let da = (a.x - pivot.x).powi(2) + (a.y - pivot.y).powi(2);
            let db = (b.x - pivot.x).powi(2) + (b.y - pivot.y).powi(2);
            da.partial_cmp(&db).unwrap_or(core::cmp::Ordering::Equal)
        } else if cross > 0.0 {
            core::cmp::Ordering::Less
        } else {
            core::cmp::Ordering::Greater
        }
    });

    let mut hull: Vec<Coord> = Vec::with_capacity(points.len());
    for p in &sorted {
        while hull.len() >= 2 {
            let a = hull[hull.len() - 2];
            let b = hull[hull.len() - 1];
            if cross_2d(&a, &b, p) <= 0.0 {
                hull.pop();
            } else {
                break;
            }
        }
        hull.push(*p);
    }
    hull
}

/// 2D cross product of vectors `(b - a)` and `(c - a)`.
#[inline]
fn cross_2d(a: &Coord, b: &Coord, c: &Coord) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

// ---------------------------------------------------------------------------
// Convexity test
// ---------------------------------------------------------------------------

/// Check whether a ring (coordinate slice) is convex.
///
/// A ring with fewer than 3 distinct points is considered not convex.
pub fn is_convex(ring: &[Coord]) -> bool {
    let n = ring.len();
    if n < 3 {
        return false;
    }
    // If the ring is closed (first == last), skip the duplicate closing point.
    let effective_n = if n >= 2
        && (ring[0].x - ring[n - 1].x).abs() < 1e-10
        && (ring[0].y - ring[n - 1].y).abs() < 1e-10
    {
        n - 1
    } else {
        n
    };
    if effective_n < 3 {
        return false;
    }

    let mut sign: Option<bool> = None;
    for i in 0..effective_n {
        let a = &ring[i];
        let b = &ring[(i + 1) % effective_n];
        let c = &ring[(i + 2) % effective_n];
        let cross = cross_2d(a, b, c);
        if cross.abs() < 1e-10 {
            continue; // collinear triple — skip
        }
        let positive = cross > 0.0;
        match sign {
            None => sign = Some(positive),
            Some(s) if s != positive => return false,
            _ => {}
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Bounding-box utilities
// ---------------------------------------------------------------------------

/// Compute the axis-aligned bounding box of a ring.
///
/// Returns `None` if the slice is empty.
pub fn ring_bbox(ring: &[Coord]) -> Option<(Coord, Coord)> {
    let first = ring.first()?;
    let mut min_x = first.x;
    let mut min_y = first.y;
    let mut max_x = first.x;
    let mut max_y = first.y;
    for c in ring.iter().skip(1) {
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
    Some((Coord::new(min_x, min_y), Coord::new(max_x, max_y)))
}

/// Expand a bounding box by `amount` on all sides.
pub fn buffer_bbox(min: &Coord, max: &Coord, amount: f64) -> (Coord, Coord) {
    (
        Coord::new(min.x - amount, min.y - amount),
        Coord::new(max.x + amount, max.y + amount),
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn unit_square() -> Polygon {
        Polygon::simple(crate::Ring::new(vec![
            Coord::new(0.0, 0.0),
            Coord::new(1.0, 0.0),
            Coord::new(1.0, 1.0),
            Coord::new(0.0, 1.0),
            Coord::new(0.0, 0.0),
        ]))
    }

    #[test]
    fn area_unit_square() {
        assert!((area(&unit_square()) - 1.0).abs() < 1e-10);
    }

    #[test]
    fn perimeter_unit_square() {
        assert!((perimeter(&unit_square()) - 4.0).abs() < 1e-10);
    }

    #[test]
    fn centroid_unit_square() {
        let c = centroid(&unit_square());
        assert!((c.x - 0.5).abs() < 1e-10);
        assert!((c.y - 0.5).abs() < 1e-10);
    }

    #[test]
    fn point_inside_square() {
        assert!(point_in_polygon(&Coord::new(0.5, 0.5), &unit_square()));
    }

    #[test]
    fn point_outside_square() {
        assert!(!point_in_polygon(&Coord::new(2.0, 2.0), &unit_square()));
    }

    #[test]
    fn distance_basic() {
        let d = distance(&Coord::new(0.0, 0.0), &Coord::new(3.0, 4.0));
        assert!((d - 5.0).abs() < 1e-10);
    }

    #[test]
    fn convex_hull_basic() {
        let points = vec![
            Coord::new(0.0, 0.0),
            Coord::new(1.0, 0.0),
            Coord::new(0.5, 0.5),
            Coord::new(1.0, 1.0),
            Coord::new(0.0, 1.0),
        ];
        let hull = convex_hull(&points);
        assert_eq!(hull.len(), 4); // interior point excluded
    }

    #[test]
    fn is_convex_square() {
        let ring = [
            Coord::new(0.0, 0.0),
            Coord::new(1.0, 0.0),
            Coord::new(1.0, 1.0),
            Coord::new(0.0, 1.0),
            Coord::new(0.0, 0.0),
        ];
        assert!(is_convex(&ring));
    }

    #[test]
    fn is_not_convex_l_shape() {
        let ring = [
            Coord::new(0.0, 0.0),
            Coord::new(2.0, 0.0),
            Coord::new(2.0, 1.0),
            Coord::new(1.0, 1.0),
            Coord::new(1.0, 2.0),
            Coord::new(0.0, 2.0),
            Coord::new(0.0, 0.0),
        ];
        assert!(!is_convex(&ring));
    }

    #[test]
    fn ring_bbox_basic() {
        let ring = [
            Coord::new(1.0, 2.0),
            Coord::new(3.0, 5.0),
            Coord::new(-1.0, 0.0),
        ];
        let (min, max) = ring_bbox(&ring).expect("non-empty");
        assert!((min.x - (-1.0)).abs() < 1e-10);
        assert!((min.y - 0.0).abs() < 1e-10);
        assert!((max.x - 3.0).abs() < 1e-10);
        assert!((max.y - 5.0).abs() < 1e-10);
    }

    #[test]
    fn buffer_bbox_basic() {
        let (min, max) = buffer_bbox(&Coord::new(0.0, 0.0), &Coord::new(1.0, 1.0), 0.5);
        assert!((min.x - (-0.5)).abs() < 1e-10);
        assert!((max.x - 1.5).abs() < 1e-10);
    }
}
