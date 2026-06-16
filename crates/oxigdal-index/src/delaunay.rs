//! Delaunay triangulation via the Bowyer–Watson algorithm.
//!
//! # Algorithm overview
//!
//! The [Bowyer–Watson algorithm] incrementally builds a Delaunay triangulation
//! by inserting one point at a time.  For each new point `p`:
//!
//! 1. Find all *bad* triangles whose circumcircle strictly contains `p`.
//! 2. Identify the *boundary polygon* — the set of edges that are on the
//!    boundary of the cavity formed by the bad triangles (i.e. edges shared by
//!    exactly one bad triangle).
//! 3. Delete the bad triangles.
//! 4. Re-triangulate the cavity by connecting each boundary edge to `p`.
//!
//! A *super-triangle* that encloses all input points is used to bootstrap the
//! algorithm.  After all points are inserted, any triangle that shares a vertex
//! with the super-triangle is removed.
//!
//! [Bowyer–Watson algorithm]: https://en.wikipedia.org/wiki/Bowyer%E2%80%93Watson_algorithm
//!
//! # Example
//!
//! ```rust
//! use oxigdal_index::{triangulate, Coord};
//!
//! let pts = vec![
//!     Coord::new(0.0, 0.0),
//!     Coord::new(1.0, 0.0),
//!     Coord::new(0.5, 1.0),
//! ];
//! let tri = triangulate(&pts).unwrap();
//! assert_eq!(tri.triangles.len(), 1);
//! ```

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use crate::error::IndexError;
use crate::validation::Coord;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A Delaunay triangulation of a planar point set.
#[derive(Debug, Clone)]
pub struct Triangulation {
    /// The input points in original order, with duplicates removed.
    pub points: Vec<Coord>,
    /// Triangles as index triples into [`points`][Triangulation::points],
    /// ordered counter-clockwise.
    pub triangles: Vec<[usize; 3]>,
    /// For each triangle `t`, `adjacency[t][e]` is the index of the triangle
    /// that shares the edge *opposite* vertex `triangles[t][e]`, or `None` if
    /// that edge is on the convex-hull boundary.
    pub adjacency: Vec<[Option<usize>; 3]>,
}

impl Triangulation {
    /// Return the convex hull as a list of point indices in counter-clockwise
    /// order.
    ///
    /// The convex hull consists of the boundary edges of the triangulation
    /// (edges with no adjacent triangle on the other side).
    pub fn convex_hull(&self) -> Vec<usize> {
        // Collect all boundary edges (where adjacency is None).
        let mut boundary_edges: Vec<(usize, usize)> = Vec::new();
        for (t, tri) in self.triangles.iter().enumerate() {
            for e in 0..3 {
                if self.adjacency[t][e].is_none() {
                    // Edge opposite vertex e goes from tri[(e+1)%3] to tri[(e+2)%3].
                    let v0 = tri[(e + 1) % 3];
                    let v1 = tri[(e + 2) % 3];
                    boundary_edges.push((v0, v1));
                }
            }
        }

        if boundary_edges.is_empty() {
            return Vec::new();
        }

        // Chain the boundary edges into a polygon (order them).
        let mut hull: Vec<usize> = Vec::with_capacity(boundary_edges.len());
        hull.push(boundary_edges[0].0);
        let mut current = boundary_edges[0].1;

        for _ in 1..boundary_edges.len() {
            hull.push(current);
            // Find the next edge that starts at `current`.
            let next = boundary_edges.iter().find(|&&(s, _)| s == current);
            match next {
                Some(&(_, end)) => current = end,
                None => break,
            }
        }

        // Ensure CCW orientation.
        if !is_polygon_ccw(&self.points, &hull) {
            hull.reverse();
        }
        hull
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Compute the Delaunay triangulation of `points` using the Bowyer–Watson
/// algorithm.
///
/// Duplicate points (within `1e-10`) are silently removed, keeping the first
/// occurrence.
///
/// # Errors
///
/// * [`IndexError::InvalidInput`] if there are fewer than 3 distinct points.
/// * [`IndexError::InvalidInput`] if all distinct points are collinear.
pub fn triangulate(points: &[Coord]) -> Result<Triangulation, IndexError> {
    // ------------------------------------------------------------------
    // Step 1: deduplicate
    // ------------------------------------------------------------------
    let deduped = deduplicate(points);

    if deduped.len() < 3 {
        return Err(IndexError::InvalidInput(
            "at least 3 distinct input points are required for triangulation".into(),
        ));
    }

    // ------------------------------------------------------------------
    // Step 2: collinearity check
    // ------------------------------------------------------------------
    if all_collinear(&deduped) {
        return Err(IndexError::InvalidInput(
            "All input points are collinear".into(),
        ));
    }

    // ------------------------------------------------------------------
    // Step 3: build working point list (input + super-triangle)
    // ------------------------------------------------------------------
    let n = deduped.len();
    let (min_x, min_y, max_x, max_y) = bounding_box(&deduped);
    let cx = (min_x + max_x) * 0.5;
    let cy = (min_y + max_y) * 0.5;
    let span = (max_x - min_x).max(max_y - min_y).max(1.0);

    // Super-triangle vertices — far outside all input points.
    let sa = Coord::new(cx - 20.0 * span, cy - span);
    let sb = Coord::new(cx, cy + 20.0 * span);
    let sc = Coord::new(cx + 20.0 * span, cy - span);

    // Working point list: [deduped..., sa, sb, sc]
    let mut all_pts: Vec<Coord> = Vec::with_capacity(n + 3);
    all_pts.extend_from_slice(&deduped);
    all_pts.push(sa);
    all_pts.push(sb);
    all_pts.push(sc);

    let si_a = n;
    let si_b = n + 1;
    let si_c = n + 2;

    // ------------------------------------------------------------------
    // Step 4: initialise triangulation with the super-triangle
    // ------------------------------------------------------------------
    // A triangle is stored as [v0, v1, v2] with CCW winding.
    // We also store whether it is "alive" (not deleted).
    let mut tri_verts: Vec<[usize; 3]> = Vec::new();
    let mut tri_alive: Vec<bool> = Vec::new();

    let super_tri = ccw_order(&all_pts, [si_a, si_b, si_c]);
    tri_verts.push(super_tri);
    tri_alive.push(true);

    // ------------------------------------------------------------------
    // Step 5: insert points one at a time
    // ------------------------------------------------------------------
    for pt_idx in 0..n {
        let p = &all_pts[pt_idx];

        // 5a. Find bad triangles.
        let bad: Vec<usize> = tri_verts
            .iter()
            .enumerate()
            .filter(|(i, _)| tri_alive[*i])
            .filter(|(_, tv)| {
                let &[a, b, c] = *tv;
                in_circumcircle(&all_pts[a], &all_pts[b], &all_pts[c], p)
            })
            .map(|(i, _)| i)
            .collect();

        // 5b. Find the boundary polygon of the cavity.
        // An edge (v0, v1) is on the boundary iff it appears in exactly one
        // bad triangle (i.e. NOT shared with another bad triangle).
        let boundary = cavity_boundary(&tri_verts, &bad);

        // 5c. Delete bad triangles.
        for &bi in &bad {
            tri_alive[bi] = false;
        }

        // 5d. Re-triangulate cavity: connect each boundary edge to pt_idx.
        for (e0, e1) in boundary {
            let new_tri = ccw_order(&all_pts, [e0, e1, pt_idx]);
            tri_verts.push(new_tri);
            tri_alive.push(true);
        }
    }

    // ------------------------------------------------------------------
    // Step 6: strip super-triangle triangles
    // ------------------------------------------------------------------
    let final_tris: Vec<[usize; 3]> = tri_verts
        .into_iter()
        .zip(tri_alive.iter())
        .filter(|&(_, alive)| *alive)
        .map(|(tv, _)| tv)
        .filter(|tv| tv[0] < n && tv[1] < n && tv[2] < n)
        .collect();

    // ------------------------------------------------------------------
    // Step 7: build adjacency
    // ------------------------------------------------------------------
    let adjacency = build_adjacency(&final_tris);

    Ok(Triangulation {
        points: deduped,
        triangles: final_tris,
        adjacency,
    })
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Remove duplicate points (within epsilon = 1e-10), keeping first occurrence.
fn deduplicate(pts: &[Coord]) -> Vec<Coord> {
    const EPS: f64 = 1e-10;
    let mut out: Vec<Coord> = Vec::with_capacity(pts.len());
    'outer: for &p in pts {
        for &q in &out {
            if (p.x - q.x).abs() < EPS && (p.y - q.y).abs() < EPS {
                continue 'outer;
            }
        }
        out.push(p);
    }
    out
}

/// Compute the axis-aligned bounding box of a non-empty slice of points.
fn bounding_box(pts: &[Coord]) -> (f64, f64, f64, f64) {
    let mut min_x = pts[0].x;
    let mut min_y = pts[0].y;
    let mut max_x = pts[0].x;
    let mut max_y = pts[0].y;
    for p in pts.iter().skip(1) {
        if p.x < min_x {
            min_x = p.x;
        }
        if p.y < min_y {
            min_y = p.y;
        }
        if p.x > max_x {
            max_x = p.x;
        }
        if p.y > max_y {
            max_y = p.y;
        }
    }
    (min_x, min_y, max_x, max_y)
}

/// Test whether all points lie on a single line.
///
/// Uses the cross-product of consecutive edge vectors.  Any point far enough
/// off the line will produce a non-zero cross product.
fn all_collinear(pts: &[Coord]) -> bool {
    if pts.len() < 3 {
        return true;
    }
    const EPS: f64 = 1e-10;
    let ax = pts[1].x - pts[0].x;
    let ay = pts[1].y - pts[0].y;
    for p in pts.iter().skip(2) {
        let bx = p.x - pts[0].x;
        let by = p.y - pts[0].y;
        if (ax * by - ay * bx).abs() > EPS {
            return false;
        }
    }
    true
}

/// Signed 2-D cross product of vectors `O→A` and `O→B`.
///
/// Positive ⇒ `A` is to the left of `O→B` (CCW turn).
#[inline]
fn cross(o: &Coord, a: &Coord, b: &Coord) -> f64 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

/// Return `true` if the triangle `(a, b, c)` is wound counter-clockwise.
#[inline]
fn is_ccw_tri(pts: &[Coord], a: usize, b: usize, c: usize) -> bool {
    cross(&pts[a], &pts[b], &pts[c]) > 0.0
}

/// Return the triangle with vertices re-ordered to be CCW.
fn ccw_order(pts: &[Coord], [a, b, c]: [usize; 3]) -> [usize; 3] {
    if is_ccw_tri(pts, a, b, c) {
        [a, b, c]
    } else {
        [a, c, b]
    }
}

/// Test whether `p` lies strictly inside the circumcircle of the CCW triangle
/// `(a, b, c)`.
///
/// Uses the exact 3×3 determinant predicate (see Shewchuk 1996).
/// For a CCW triangle the determinant is positive iff `p` is inside.
/// A small positive threshold avoids false positives for co-circular points
/// (where `det ≈ 0` due to floating-point noise).
fn in_circumcircle(a: &Coord, b: &Coord, c: &Coord, p: &Coord) -> bool {
    let ax_ = a.x - p.x;
    let ay_ = a.y - p.y;
    let bx_ = b.x - p.x;
    let by_ = b.y - p.y;
    let cx_ = c.x - p.x;
    let cy_ = c.y - p.y;

    let det = ax_ * (by_ * (cx_ * cx_ + cy_ * cy_) - cy_ * (bx_ * bx_ + by_ * by_))
        - ay_ * (bx_ * (cx_ * cx_ + cy_ * cy_) - cx_ * (bx_ * bx_ + by_ * by_))
        + (ax_ * ax_ + ay_ * ay_) * (bx_ * cy_ - by_ * cx_);
    det > 1e-10
}

/// Collect the *boundary* edges of the cavity formed by the bad triangles.
///
/// An edge `(u, v)` is on the boundary iff it is in exactly one of the bad
/// triangles (direction-independent).  The returned edges are in the winding
/// order of the bad triangle that contains them.
fn cavity_boundary(tri_verts: &[[usize; 3]], bad: &[usize]) -> Vec<(usize, usize)> {
    // Collect every directed edge from every bad triangle.
    let mut directed: Vec<(usize, usize)> = Vec::with_capacity(bad.len() * 3);
    for &bi in bad {
        let [a, b, c] = tri_verts[bi];
        directed.push((a, b));
        directed.push((b, c));
        directed.push((c, a));
    }

    // An edge (u, v) is on the boundary iff its reverse (v, u) does NOT appear
    // among the directed edges.
    directed
        .iter()
        .filter(|&&(u, v)| !directed.contains(&(v, u)))
        .copied()
        .collect()
}

/// Build the adjacency table for a list of triangles.
///
/// `adjacency[t][e]` = index of the triangle sharing edge `e`, or `None`.
///
/// Edge `e` of triangle `t` is the edge *opposite* vertex `triangles[t][e]`,
/// i.e. the edge between vertices `triangles[t][(e+1)%3]` and
/// `triangles[t][(e+2)%3]`.
fn build_adjacency(tris: &[[usize; 3]]) -> Vec<[Option<usize>; 3]> {
    let nt = tris.len();
    let mut adj: Vec<[Option<usize>; 3]> = vec![[None; 3]; nt];

    for t in 0..nt {
        for e in 0..3 {
            if adj[t][e].is_some() {
                continue; // already filled
            }
            let v0 = tris[t][(e + 1) % 3];
            let v1 = tris[t][(e + 2) % 3];

            // Search for a triangle that has the reversed edge (v1, v0).
            for t2 in 0..nt {
                if t2 == t {
                    continue;
                }
                for e2 in 0..3 {
                    let u0 = tris[t2][(e2 + 1) % 3];
                    let u1 = tris[t2][(e2 + 2) % 3];
                    if u0 == v1 && u1 == v0 {
                        adj[t][e] = Some(t2);
                        adj[t2][e2] = Some(t);
                    }
                }
            }
        }
    }
    adj
}

/// Test whether a polygon (given as indices into `pts`) has CCW winding using
/// the shoelace formula.
fn is_polygon_ccw(pts: &[Coord], indices: &[usize]) -> bool {
    let n = indices.len();
    if n < 3 {
        return true;
    }
    let mut area = 0.0_f64;
    for i in 0..n {
        let j = (i + 1) % n;
        let pi = &pts[indices[i]];
        let pj = &pts[indices[j]];
        area += pi.x * pj.y;
        area -= pj.x * pi.y;
    }
    area > 0.0
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn coord(x: f64, y: f64) -> Coord {
        Coord::new(x, y)
    }

    // 1. Three non-collinear points → exactly one triangle.
    #[test]
    fn test_triangle() {
        let pts = vec![coord(0.0, 0.0), coord(1.0, 0.0), coord(0.5, 1.0)];
        let tri = triangulate(&pts).expect("triangulate should succeed");
        assert_eq!(tri.points.len(), 3);
        assert_eq!(tri.triangles.len(), 1);
        // The single triangle must contain all three original indices.
        let t = tri.triangles[0];
        let mut sorted = [t[0], t[1], t[2]];
        sorted.sort_unstable();
        assert_eq!(sorted, [0, 1, 2]);
    }

    // 2. Square (4 corners) → exactly two triangles.
    #[test]
    fn test_square() {
        let pts = vec![
            coord(0.0, 0.0),
            coord(1.0, 0.0),
            coord(1.0, 1.0),
            coord(0.0, 1.0),
        ];
        let tri = triangulate(&pts).expect("triangulate should succeed");
        assert_eq!(tri.points.len(), 4);
        assert_eq!(tri.triangles.len(), 2);

        // Verify the empty circumcircle property for all triangles.
        assert!(empty_circumcircle_property(&tri));
    }

    // 3. Five fixed points → verify empty circumcircle property.
    #[test]
    fn test_delaunay_property() {
        let pts = vec![
            coord(0.0, 0.0),
            coord(4.0, 0.0),
            coord(2.0, 3.0),
            coord(1.0, 1.5),
            coord(3.0, 1.5),
        ];
        let tri = triangulate(&pts).expect("triangulate should succeed");
        assert!(empty_circumcircle_property(&tri));
    }

    // 4. Output triangles must be counter-clockwise.
    #[test]
    fn test_triangle_is_ccw() {
        let pts = vec![
            coord(0.0, 0.0),
            coord(3.0, 0.0),
            coord(1.5, 2.0),
            coord(1.0, 0.5),
            coord(2.0, 0.5),
        ];
        let tri = triangulate(&pts).expect("triangulate should succeed");
        for t in &tri.triangles {
            let [a, b, c] = *t;
            assert!(
                is_ccw_tri(&tri.points, a, b, c),
                "triangle ({a}, {b}, {c}) is not CCW"
            );
        }
    }

    // 5. Collinear input → error.
    #[test]
    fn test_collinear() {
        let pts = vec![coord(0.0, 0.0), coord(1.0, 1.0), coord(2.0, 2.0)];
        let result = triangulate(&pts);
        assert!(
            matches!(result, Err(IndexError::InvalidInput(_))),
            "expected InvalidInput for collinear points, got {:?}",
            result
        );
    }

    // 6. Duplicate points → same result as deduplicated input.
    #[test]
    fn test_duplicate_points() {
        let unique = vec![
            coord(0.0, 0.0),
            coord(1.0, 0.0),
            coord(0.5, 1.0),
            coord(0.25, 0.5),
        ];
        let with_dups = {
            let mut v = unique.clone();
            v.push(coord(0.0, 0.0)); // duplicate of index 0
            v.push(coord(0.5, 1.0)); // duplicate of index 2
            v
        };
        let t1 = triangulate(&unique).expect("unique triangulate");
        let t2 = triangulate(&with_dups).expect("dup triangulate");
        assert_eq!(t1.points.len(), t2.points.len());
        assert_eq!(t1.triangles.len(), t2.triangles.len());
    }

    // 7. Convex hull of rectangle + interior point → 4 hull points.
    #[test]
    fn test_convex_hull() {
        let pts = vec![
            coord(0.0, 0.0),
            coord(4.0, 0.0),
            coord(4.0, 4.0),
            coord(0.0, 4.0),
            coord(2.0, 2.0), // interior
        ];
        let tri = triangulate(&pts).expect("triangulate should succeed");
        let hull = tri.convex_hull();
        // The interior point (index 4) must NOT be on the hull.
        assert_eq!(hull.len(), 4, "hull should have 4 points, got {:?}", hull);
        assert!(!hull.contains(&4), "interior point must not be on hull");
    }

    // 8. Adjacency consistency check for a square.
    #[test]
    fn test_adjacency() {
        let pts = vec![
            coord(0.0, 0.0),
            coord(1.0, 0.0),
            coord(1.0, 1.0),
            coord(0.0, 1.0),
        ];
        let tri = triangulate(&pts).expect("triangulate should succeed");
        // Two triangles; they must share exactly one edge.
        assert_eq!(tri.triangles.len(), 2);
        let adj0 = tri.adjacency[0];
        let adj1 = tri.adjacency[1];

        // Each triangle must see the other as a neighbor across their shared edge.
        let t0_sees_t1 = adj0.contains(&Some(1));
        let t1_sees_t0 = adj1.contains(&Some(0));
        assert!(t0_sees_t1, "triangle 0 should see triangle 1 as neighbor");
        assert!(t1_sees_t0, "triangle 1 should see triangle 0 as neighbor");

        // Both triangles must have exactly two boundary edges (None adjacency).
        let boundary_edges_t0 = adj0.iter().filter(|x| x.is_none()).count();
        let boundary_edges_t1 = adj1.iter().filter(|x| x.is_none()).count();
        assert_eq!(boundary_edges_t0, 2);
        assert_eq!(boundary_edges_t1, 2);
    }

    // 9. Fewer than 3 points → error.
    #[test]
    fn test_too_few_points() {
        let result = triangulate(&[coord(0.0, 0.0), coord(1.0, 1.0)]);
        assert!(
            matches!(result, Err(IndexError::InvalidInput(_))),
            "expected InvalidInput for 2 points"
        );
    }

    // 10. Large regular polygon — check triangle count = n-2 and property.
    #[test]
    fn test_regular_polygon() {
        use core::f64::consts::PI;
        let n = 8usize;
        let pts: Vec<Coord> = (0..n)
            .map(|i| {
                let theta = 2.0 * PI * (i as f64) / (n as f64);
                coord(theta.cos(), theta.sin())
            })
            .collect();
        let tri = triangulate(&pts).expect("regular polygon triangulate");
        // A simple polygon with n vertices has n-2 triangles.
        assert_eq!(tri.triangles.len(), n - 2);
        assert!(empty_circumcircle_property(&tri));
    }

    // Helper: verify the empty circumcircle property (Delaunay criterion).
    fn empty_circumcircle_property(tri: &Triangulation) -> bool {
        let pts = &tri.points;
        for t in &tri.triangles {
            let [a, b, c] = *t;
            for (k, p) in pts.iter().enumerate() {
                if k == a || k == b || k == c {
                    continue;
                }
                if in_circumcircle(&pts[a], &pts[b], &pts[c], p) {
                    return false;
                }
            }
        }
        true
    }
}
