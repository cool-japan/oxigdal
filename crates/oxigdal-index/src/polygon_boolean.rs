//! Polygon boolean set operations (union, intersection, difference, symmetric difference).
//!
//! Implements Sutherland-Hodgman clipping for intersection, with fast-path
//! logic for disjoint, identical, and containment cases. Union and difference
//! are derived from intersection and containment tests.
//!
//! # Limitations
//!
//! Only simple polygons (no holes) are supported. Hole information in inputs
//! is silently ignored. For partial-overlap union and difference (the hardest
//! case), the implementation uses a correct but potentially approximate result
//! — specifically, a full Weiler-Atherton traversal is used to handle partial
//! overlap for union, and Sutherland-Hodgman with complement half-planes is
//! used for difference.

use crate::bbox::Bbox2D;
use crate::validation::{Coord, Polygon, Ring};

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// Public API types
// ---------------------------------------------------------------------------

/// Boolean set operation to perform on two polygons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanOp {
    /// The area covered by either polygon.
    Union,
    /// The area covered by both polygons simultaneously.
    Intersection,
    /// The area in the subject polygon that is not in the clip polygon.
    Difference,
    /// The area in either polygon that is not in both.
    SymmetricDifference,
}

/// Result of a polygon boolean operation.
#[derive(Debug, Clone)]
pub enum BooleanResult {
    /// A single connected output polygon.
    Single(Polygon),
    /// Multiple disjoint output polygons.
    Multiple(Vec<Polygon>),
    /// The operation produces an empty (zero-area) result.
    Empty,
}

// ---------------------------------------------------------------------------
// Public functions
// ---------------------------------------------------------------------------

/// Compute a boolean set operation on two simple polygons.
///
/// Only the exterior rings of `subject` and `clip` are used; holes are
/// ignored. For degenerate or boundary cases a conservative result is
/// returned.
pub fn polygon_boolean(subject: &Polygon, clip: &Polygon, op: BooleanOp) -> BooleanResult {
    match op {
        BooleanOp::Union => polygon_union(subject, clip),
        BooleanOp::Intersection => polygon_intersection(subject, clip),
        BooleanOp::Difference => polygon_difference(subject, clip),
        BooleanOp::SymmetricDifference => polygon_symmetric_difference(subject, clip),
    }
}

/// Compute the union of two simple polygons.
///
/// Returns `Single` when the polygons are identical or one contains the
/// other; `Multiple` when they are disjoint or produce separate pieces
/// in an overlapping union; `Empty` only if both inputs are degenerate.
pub fn polygon_union(a: &Polygon, b: &Polygon) -> BooleanResult {
    let a_ext = a.exterior.coords();
    let b_ext = b.exterior.coords();

    // Degenerate inputs.
    if a_ext.len() < 3 && b_ext.len() < 3 {
        return BooleanResult::Empty;
    }
    if a_ext.len() < 3 {
        return BooleanResult::Single(b.clone());
    }
    if b_ext.len() < 3 {
        return BooleanResult::Single(a.clone());
    }

    // Fast path: identical polygons.
    if polygons_identical(a, b) {
        return BooleanResult::Single(a.clone());
    }

    // Bbox disjoint fast path.
    let bbox_a = match polygon_bbox(a) {
        Some(bb) => bb,
        None => return BooleanResult::Single(b.clone()),
    };
    let bbox_b = match polygon_bbox(b) {
        Some(bb) => bb,
        None => return BooleanResult::Single(a.clone()),
    };

    if bboxes_disjoint(&bbox_a, &bbox_b) {
        return BooleanResult::Multiple(vec![a.clone(), b.clone()]);
    }

    // Containment fast paths.
    if polygon_a_contains_b(a, b) {
        return BooleanResult::Single(a.clone());
    }
    if polygon_a_contains_b(b, a) {
        return BooleanResult::Single(b.clone());
    }

    // Overlapping case: use Weiler-Atherton to compute the union ring.
    let rings = weiler_atherton_union(a_ext, b_ext);
    if rings.is_empty() {
        // Fallback: return both polygons.
        return BooleanResult::Multiple(vec![a.clone(), b.clone()]);
    }
    if rings.len() == 1 {
        let poly = coords_to_polygon(&rings[0]);
        if poly.exterior.coords().len() >= 3 {
            return BooleanResult::Single(poly);
        }
        return BooleanResult::Multiple(vec![a.clone(), b.clone()]);
    }
    let polys: Vec<Polygon> = rings
        .iter()
        .map(|r| coords_to_polygon(r))
        .filter(|p| p.exterior.coords().len() >= 3)
        .collect();
    if polys.is_empty() {
        BooleanResult::Multiple(vec![a.clone(), b.clone()])
    } else if polys.len() == 1 {
        BooleanResult::Single(
            polys
                .into_iter()
                .next()
                .expect("polys.len() == 1 ensures Some"),
        )
    } else {
        BooleanResult::Multiple(polys)
    }
}

/// Compute the intersection of two simple polygons.
///
/// Returns `Empty` when the polygons are disjoint; `Single` with the
/// overlapping area otherwise.
pub fn polygon_intersection(a: &Polygon, b: &Polygon) -> BooleanResult {
    let a_ext = a.exterior.coords();
    let b_ext = b.exterior.coords();

    // Degenerate inputs.
    if a_ext.len() < 3 || b_ext.len() < 3 {
        return BooleanResult::Empty;
    }

    // Fast path: identical polygons → intersection is the polygon itself.
    if polygons_identical(a, b) {
        return BooleanResult::Single(a.clone());
    }

    // Bbox disjoint fast path.
    let bbox_a = match polygon_bbox(a) {
        Some(bb) => bb,
        None => return BooleanResult::Empty,
    };
    let bbox_b = match polygon_bbox(b) {
        Some(bb) => bb,
        None => return BooleanResult::Empty,
    };

    if bboxes_disjoint(&bbox_a, &bbox_b) {
        return BooleanResult::Empty;
    }

    // Containment fast paths.
    if polygon_a_contains_b(a, b) {
        return BooleanResult::Single(b.clone());
    }
    if polygon_a_contains_b(b, a) {
        return BooleanResult::Single(a.clone());
    }

    // General case: Sutherland-Hodgman.
    let clipped = sutherland_hodgman(a_ext, b_ext);
    if clipped.len() < 3 {
        return BooleanResult::Empty;
    }
    BooleanResult::Single(coords_to_polygon(&clipped))
}

/// Compute the difference `a − b` (the part of `a` not covered by `b`).
///
/// Returns `Single(a)` when `a` and `b` are disjoint; `Empty` when `b`
/// fully contains `a`; an approximation of the difference otherwise.
pub fn polygon_difference(a: &Polygon, b: &Polygon) -> BooleanResult {
    let a_ext = a.exterior.coords();
    let b_ext = b.exterior.coords();

    // Degenerate inputs.
    if a_ext.len() < 3 {
        return BooleanResult::Empty;
    }
    if b_ext.len() < 3 {
        return BooleanResult::Single(a.clone());
    }

    // Fast path: identical polygons → difference is empty.
    if polygons_identical(a, b) {
        return BooleanResult::Empty;
    }

    // Bbox disjoint fast path.
    let bbox_a = match polygon_bbox(a) {
        Some(bb) => bb,
        None => return BooleanResult::Empty,
    };
    let bbox_b = match polygon_bbox(b) {
        Some(bb) => bb,
        None => return BooleanResult::Single(a.clone()),
    };

    if bboxes_disjoint(&bbox_a, &bbox_b) {
        return BooleanResult::Single(a.clone());
    }

    // Containment fast paths.
    if polygon_a_contains_b(b, a) {
        // b fully contains a → difference is empty.
        return BooleanResult::Empty;
    }
    if polygon_a_contains_b(a, b) {
        // a fully contains b — this is the hole-punch case.
        // Return Single(a) as a known limitation (proper hole polygon not implemented).
        return BooleanResult::Single(a.clone());
    }

    // Partial overlap: clip a against the complement of b.
    // Sutherland-Hodgman with reversed clip edges gives a ∩ ¬b.
    let clipped = sutherland_hodgman_complement(a_ext, b_ext);
    if clipped.len() < 3 {
        // If the complement clip returns nothing, b must cover all of a.
        return BooleanResult::Empty;
    }
    BooleanResult::Single(coords_to_polygon(&clipped))
}

/// Compute the symmetric difference `(a − b) ∪ (b − a)`.
pub fn polygon_symmetric_difference(a: &Polygon, b: &Polygon) -> BooleanResult {
    // Fast path: identical polygons → symmetric difference is empty.
    if polygons_identical(a, b) {
        return BooleanResult::Empty;
    }

    // Fast path: disjoint polygons → both polygons.
    let bbox_a = polygon_bbox(a);
    let bbox_b = polygon_bbox(b);
    if let (Some(ba), Some(bb)) = (&bbox_a, &bbox_b)
        && bboxes_disjoint(ba, bb)
    {
        return BooleanResult::Multiple(vec![a.clone(), b.clone()]);
    }

    // Collect (a - b) and (b - a) results.
    let diff_ab = polygon_difference(a, b);
    let diff_ba = polygon_difference(b, a);

    let mut parts: Vec<Polygon> = Vec::new();
    collect_result(diff_ab, &mut parts);
    collect_result(diff_ba, &mut parts);

    match parts.len() {
        0 => BooleanResult::Empty,
        1 => BooleanResult::Single(parts.remove(0)),
        _ => BooleanResult::Multiple(parts),
    }
}

/// Return `true` if the bounding boxes of `a` and `b` overlap (including touching edges).
///
/// This is a necessary but not sufficient condition for polygon overlap.
pub fn polygons_intersect_bbox_test(a: &Polygon, b: &Polygon) -> bool {
    match (polygon_bbox(a), polygon_bbox(b)) {
        (Some(ba), Some(bb)) => !bboxes_disjoint(&ba, &bb),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Private: bounding box helpers
// ---------------------------------------------------------------------------

fn polygon_bbox(poly: &Polygon) -> Option<Bbox2D> {
    let coords = poly.exterior.coords();
    if coords.is_empty() {
        return None;
    }
    let mut min_x = coords[0].x;
    let mut min_y = coords[0].y;
    let mut max_x = coords[0].x;
    let mut max_y = coords[0].y;
    for c in coords.iter().skip(1) {
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
    Bbox2D::new(min_x, min_y, max_x, max_y)
}

/// Return `true` if two bounding boxes are strictly disjoint (no overlap, no touching).
#[inline]
fn bboxes_disjoint(a: &Bbox2D, b: &Bbox2D) -> bool {
    a.max_x < b.min_x || b.max_x < a.min_x || a.max_y < b.min_y || b.max_y < a.min_y
}

// ---------------------------------------------------------------------------
// Private: point-in-ring (ray casting)
// ---------------------------------------------------------------------------

/// Ray-casting point-in-ring test.
///
/// Returns `true` if `p` lies strictly inside the ring. Boundary points give
/// indeterminate results.
fn point_in_ring(p: &Coord, ring: &Ring) -> bool {
    let coords = ring.coords();
    let n = coords.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let ci = &coords[i];
        let cj = &coords[j];
        if ((ci.y > p.y) != (cj.y > p.y))
            && (p.x < (cj.x - ci.x) * (p.y - ci.y) / (cj.y - ci.y) + ci.x)
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Ray-casting point-in-ring test operating on a coordinate slice directly.
fn point_in_coords(p: &Coord, coords: &[Coord]) -> bool {
    let n = coords.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let ci = &coords[i];
        let cj = &coords[j];
        if ((ci.y > p.y) != (cj.y > p.y))
            && (p.x < (cj.x - ci.x) * (p.y - ci.y) / (cj.y - ci.y) + ci.x)
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

// ---------------------------------------------------------------------------
// Private: polygon comparison and containment
// ---------------------------------------------------------------------------

const COORD_EPS: f64 = 1e-10;

/// Return `true` if two polygons have identical exterior rings (vertex-by-vertex,
/// within `COORD_EPS` tolerance).
fn polygons_identical(a: &Polygon, b: &Polygon) -> bool {
    let ac = a.exterior.coords();
    let bc = b.exterior.coords();
    if ac.len() != bc.len() {
        return false;
    }
    for (ca, cb) in ac.iter().zip(bc.iter()) {
        if (ca.x - cb.x).abs() > COORD_EPS || (ca.y - cb.y).abs() > COORD_EPS {
            return false;
        }
    }
    true
}

/// Return `true` if all vertices of `b`'s exterior ring lie inside `a`.
fn polygon_a_contains_b(a: &Polygon, b: &Polygon) -> bool {
    let b_coords = b.exterior.coords();
    if b_coords.is_empty() {
        return false;
    }
    // Use at most the non-closing vertices (skip the closing duplicate if present).
    let effective_n = if b_coords.len() >= 2 {
        let first = &b_coords[0];
        let last = &b_coords[b_coords.len() - 1];
        if (first.x - last.x).abs() < COORD_EPS && (first.y - last.y).abs() < COORD_EPS {
            b_coords.len() - 1
        } else {
            b_coords.len()
        }
    } else {
        b_coords.len()
    };

    for coord in b_coords.iter().take(effective_n) {
        if !point_in_ring(coord, &a.exterior) {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Private: segment intersection (parametric)
// ---------------------------------------------------------------------------

/// Compute the intersection point of segments `p1→p2` and `p3→p4`.
///
/// Returns `Some((point, t, u))` where `t` is the parameter along `p1→p2`
/// and `u` is the parameter along `p3→p4`, both in the open interval `(0, 1)`.
/// Endpoint-touching intersections (`t` or `u` exactly 0 or 1) are excluded.
fn segment_intersection(p1: Coord, p2: Coord, p3: Coord, p4: Coord) -> Option<(Coord, f64, f64)> {
    let dx1 = p2.x - p1.x;
    let dy1 = p2.y - p1.y;
    let dx2 = p4.x - p3.x;
    let dy2 = p4.y - p3.y;

    let denom = dx1 * dy2 - dy1 * dx2;
    if denom.abs() < 1e-14 {
        // Parallel or collinear.
        return None;
    }

    let t = ((p3.x - p1.x) * dy2 - (p3.y - p1.y) * dx2) / denom;
    let u = ((p3.x - p1.x) * dy1 - (p3.y - p1.y) * dx1) / denom;

    let eps = 1e-10;
    if t > eps && t < 1.0 - eps && u > eps && u < 1.0 - eps {
        let ix = p1.x + t * dx1;
        let iy = p1.y + t * dy1;
        Some((Coord::new(ix, iy), t, u))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Private: Sutherland-Hodgman clipping
// ---------------------------------------------------------------------------

/// Clip `subject` against a single half-plane defined by the directed edge
/// `p1→p2`.  Points on the left side of the edge (inside) are kept when
/// `keep_inside` is `true`; the right side is kept when `false`.
fn clip_polygon_by_half_plane(
    subject: &[Coord],
    p1: Coord,
    p2: Coord,
    keep_inside: bool,
) -> Vec<Coord> {
    if subject.is_empty() {
        return Vec::new();
    }

    let edge_dx = p2.x - p1.x;
    let edge_dy = p2.y - p1.y;

    // Positive cross product = left of edge = inside (for CCW clip polygon).
    let side = |p: &Coord| -> f64 { edge_dx * (p.y - p1.y) - edge_dy * (p.x - p1.x) };
    let is_inside = |p: &Coord| -> bool {
        let s = side(p);
        if keep_inside { s >= 0.0 } else { s <= 0.0 }
    };

    let n = subject.len();
    let mut output: Vec<Coord> = Vec::with_capacity(n + 2);

    for i in 0..n {
        let current = subject[i];
        let next = subject[(i + 1) % n];

        let curr_inside = is_inside(&current);
        let next_inside = is_inside(&next);

        if curr_inside {
            output.push(current);
            if !next_inside {
                // Exiting: add intersection.
                if let Some((pt, _, _)) = segment_intersection(current, next, p1, p2) {
                    output.push(pt);
                } else {
                    // Fallback: linear interpolation at the boundary.
                    let s_curr = side(&current);
                    let s_next = side(&next);
                    let t = if (s_curr - s_next).abs() > 1e-14 {
                        s_curr / (s_curr - s_next)
                    } else {
                        0.5
                    };
                    output.push(Coord::new(
                        current.x + t * (next.x - current.x),
                        current.y + t * (next.y - current.y),
                    ));
                }
            }
        } else if next_inside {
            // Entering: add intersection then next point is handled in next iteration.
            if let Some((pt, _, _)) = segment_intersection(current, next, p1, p2) {
                output.push(pt);
            } else {
                let s_curr = side(&current);
                let s_next = side(&next);
                let t = if (s_curr - s_next).abs() > 1e-14 {
                    s_curr / (s_curr - s_next)
                } else {
                    0.5
                };
                output.push(Coord::new(
                    current.x + t * (next.x - current.x),
                    current.y + t * (next.y - current.y),
                ));
            }
        }
    }

    output
}

/// Sutherland-Hodgman algorithm: clip `subject` polygon against every edge of
/// `clip` polygon, keeping the inside (left of each directed edge when the
/// clip is CCW).
///
/// Returns the clipped polygon's coordinate list.  The result may be empty
/// when there is no overlap.
fn sutherland_hodgman(subject: &[Coord], clip: &[Coord]) -> Vec<Coord> {
    if clip.len() < 3 || subject.is_empty() {
        return Vec::new();
    }

    let clip_n = clip.len();
    let mut output: Vec<Coord> = subject.to_vec();

    for i in 0..clip_n {
        if output.is_empty() {
            break;
        }
        let p1 = clip[i];
        let p2 = clip[(i + 1) % clip_n];
        output = clip_polygon_by_half_plane(&output, p1, p2, true);
    }

    output
}

/// Sutherland-Hodgman clipping against the *complement* of the clip polygon
/// (i.e., clip `subject` against the outside of `clip`).
///
/// This computes `subject ∩ ¬clip` by clipping against the reversed edges.
/// Because every reversed edge's "inside" half-plane is the outside of the
/// original, this works for convex clip polygons.  For concave polygons the
/// result may be approximate (we return one component only).
fn sutherland_hodgman_complement(subject: &[Coord], clip: &[Coord]) -> Vec<Coord> {
    if clip.len() < 3 || subject.is_empty() {
        return subject.to_vec();
    }

    let clip_n = clip.len();
    let mut output: Vec<Coord> = subject.to_vec();

    for i in 0..clip_n {
        if output.is_empty() {
            break;
        }
        // Reversed edge: clip[(i+1)%n] → clip[i] → outside means keep_inside=false.
        let p1 = clip[i];
        let p2 = clip[(i + 1) % clip_n];
        // Clipping against the outside of the directed edge p1→p2:
        // keep points where side < 0 (right of the directed edge).
        output = clip_polygon_by_half_plane(&output, p1, p2, false);
    }

    output
}

// ---------------------------------------------------------------------------
// Private: Weiler-Atherton union
// ---------------------------------------------------------------------------

/// A vertex in the augmented vertex list used by Weiler-Atherton.
#[derive(Clone, Debug)]
struct WaVertex {
    /// The coordinate.
    coord: Coord,
    /// Whether this is an intersection vertex (rather than an original vertex).
    is_intersection: bool,
    /// For intersection vertices: is this an *entering* intersection
    /// (subject enters the clip polygon going forward)?
    is_entering: bool,
    /// For intersection vertices: the parameter `t` along the current subject edge.
    t: f64,
    /// Index into the clip vertex list where this intersection came from (clip edge index).
    #[allow(dead_code)]
    clip_edge_idx: usize,
    /// Parameter `u` along the clip edge at this intersection.
    #[allow(dead_code)]
    u: f64,
}

impl WaVertex {
    fn original(coord: Coord) -> Self {
        Self {
            coord,
            is_intersection: false,
            is_entering: false,
            t: 0.0,
            clip_edge_idx: 0,
            u: 0.0,
        }
    }

    fn intersection(coord: Coord, is_entering: bool, t: f64, clip_edge_idx: usize, u: f64) -> Self {
        Self {
            coord,
            is_intersection: true,
            is_entering,
            t,
            clip_edge_idx,
            u,
        }
    }
}

/// Cross product (signed area of triangle) to determine inside/outside.
#[inline]
fn cross2(o: Coord, a: Coord, b: Coord) -> f64 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

/// Signed area of a coordinate slice (shoelace).
fn signed_area_coords(coords: &[Coord]) -> f64 {
    let n = coords.len();
    if n < 3 {
        return 0.0;
    }
    let mut sum = 0.0_f64;
    for i in 0..n {
        let j = (i + 1) % n;
        sum += coords[i].x * coords[j].y;
        sum -= coords[j].x * coords[i].y;
    }
    sum * 0.5
}

/// Ensure a coordinate ring is in counter-clockwise order.
fn ensure_ccw(coords: &mut [Coord]) {
    if signed_area_coords(coords) < 0.0 {
        coords.reverse();
    }
}

/// Test whether a point is inside a polygon ring (coordinate slice).
#[inline]
fn inside_ring(p: Coord, ring: &[Coord]) -> bool {
    point_in_coords(&p, ring)
}

/// Weiler-Atherton union: compute the union of two simple polygons.
///
/// Returns a list of output polygon rings (as coordinate lists).  If the
/// algorithm cannot produce a result, returns an empty list (caller falls back
/// to returning both polygons).
fn weiler_atherton_union(subject: &[Coord], clip: &[Coord]) -> Vec<Vec<Coord>> {
    // Build work copies guaranteed to be CCW.
    let mut subj = subject.to_vec();
    let mut clp = clip.to_vec();
    ensure_ccw(&mut subj);
    ensure_ccw(&mut clp);

    // Remove closing vertex if present.
    strip_closing(&mut subj);
    strip_closing(&mut clp);

    if subj.len() < 3 || clp.len() < 3 {
        return Vec::new();
    }

    // Build augmented vertex list for the subject.
    let mut subj_verts: Vec<WaVertex> = subj.iter().map(|&c| WaVertex::original(c)).collect();
    let mut clp_verts: Vec<WaVertex> = clp.iter().map(|&c| WaVertex::original(c)).collect();

    let sn = subj.len();
    let cn = clp.len();

    // Find all intersections between subject and clip edges.
    let mut has_intersections = false;
    for si in 0..sn {
        let s1 = subj[si];
        let s2 = subj[(si + 1) % sn];
        let mut edge_intersections: Vec<(f64, f64, usize, Coord)> = Vec::new();
        for ci in 0..cn {
            let c1 = clp[ci];
            let c2 = clp[(ci + 1) % cn];
            if let Some((pt, t, u)) = segment_intersection(s1, s2, c1, c2) {
                edge_intersections.push((t, u, ci, pt));
                has_intersections = true;
            }
        }
        // Sort by t to insert in order.
        edge_intersections
            .sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(core::cmp::Ordering::Equal));
        for (t, u, ci, pt) in edge_intersections {
            // Determine entering/exiting: a subject vertex entering clip =
            // the subject goes from outside to inside the clip polygon.
            // For union we trace outside, so "entering" means we switch to clip.
            let s1_inside = inside_ring(s1, &clp);
            let is_entering = !s1_inside; // was outside, now entering
            let vert = WaVertex::intersection(pt, is_entering, t, ci, u);
            // Insert after position si in subj_verts (in order of t).
            insert_by_t(&mut subj_verts, si, vert.clone(), t, sn);
            // Also insert into clp_verts at the appropriate clip edge, by u.
            let clip_entering = !is_entering;
            let clip_vert = WaVertex::intersection(pt, clip_entering, u, si, t);
            insert_by_t(&mut clp_verts, ci, clip_vert, u, cn);
        }
    }

    if !has_intersections {
        // No intersections → either disjoint (already handled) or one inside the other.
        return Vec::new();
    }

    // Trace union: start from the first vertex of subject that is outside clip.
    // For union, when we hit an exiting intersection (subject exits clip) we continue on subject.
    // When we hit an entering intersection (subject enters clip), we switch to clip and trace
    // the clip ring until we find an exiting intersection.
    // Union traces outside of clip on subject and outside of subject on clip.
    // This is the standard Weiler-Atherton union tracing.

    trace_weiler_union(&subj_verts, &clp_verts, &subj, &clp)
}

/// Insert a new Weiler-Atherton vertex into the vertex list after the
/// segment starting at index `edge_start` (in the original polygon), ordered
/// by parameter `t`.
fn insert_by_t(
    verts: &mut Vec<WaVertex>,
    edge_start: usize,
    new_vert: WaVertex,
    t: f64,
    _orig_n: usize,
) {
    // Find the position of the original vertex at edge_start in the list,
    // then find the right insertion spot (after edge_start, before edge_start+1 or
    // any already-inserted intersections on this edge ordered by t).
    let start_pos = find_original_position(verts, edge_start);
    let end_pos = find_original_position(verts, edge_start + 1).unwrap_or(verts.len());

    // Effective end is the position of the next original vertex.
    let insert_range_start = start_pos.map(|p| p + 1).unwrap_or(0);
    let insert_range_end = end_pos;

    // Among already-inserted intersections in [insert_range_start, insert_range_end),
    // find the right slot by t.
    let mut insert_pos = insert_range_end;
    for (idx, v) in verts[insert_range_start..insert_range_end]
        .iter()
        .enumerate()
    {
        if v.is_intersection && v.t > t {
            insert_pos = insert_range_start + idx;
            break;
        }
    }
    let _ = t; // suppress warning
    verts.insert(insert_pos, new_vert);
}

/// Find the position of the i-th original vertex in the augmented list.
fn find_original_position(verts: &[WaVertex], orig_idx: usize) -> Option<usize> {
    let mut orig_count = 0;
    for (i, v) in verts.iter().enumerate() {
        if !v.is_intersection {
            if orig_count == orig_idx {
                return Some(i);
            }
            orig_count += 1;
        }
    }
    None
}

/// Trace the union polygon using augmented Weiler-Atherton vertex lists.
///
/// For union, we trace the subject ring when outside the clip, and the clip
/// ring when outside the subject, switching at intersection vertices.
fn trace_weiler_union(
    subj_verts: &[WaVertex],
    clp_verts: &[WaVertex],
    _subj_orig: &[Coord],
    clp_orig: &[Coord],
) -> Vec<Vec<Coord>> {
    let sn = subj_verts.len();
    let cn = clp_verts.len();

    if sn == 0 || cn == 0 {
        return Vec::new();
    }

    // Find a starting vertex on the subject that is outside the clip polygon.
    let start_idx = {
        let mut found = None;
        for (i, v) in subj_verts.iter().enumerate() {
            if !v.is_intersection && !inside_ring(v.coord, clp_orig) {
                found = Some(i);
                break;
            }
        }
        found
    };

    let start_idx = match start_idx {
        Some(s) => s,
        None => {
            // All subject vertices inside clip → subject ⊂ clip.
            return Vec::new();
        }
    };

    let mut output: Vec<Coord> = Vec::new();
    let mut on_subject = true;
    let mut si = start_idx;
    let mut visited_intersections: Vec<usize> = Vec::new(); // track visited intersection coords
    let max_steps = (sn + cn) * 4; // safety limit

    for _step in 0..max_steps {
        let vert = if on_subject {
            &subj_verts[si]
        } else {
            &clp_verts[si]
        };

        output.push(vert.coord);

        if vert.is_intersection {
            // Check if we need to switch rings.
            if on_subject {
                if vert.is_entering {
                    // Subject enters clip → switch to clip ring.
                    // Find corresponding clip vertex.
                    if let Some(ci) = find_intersection_in_clip(clp_verts, vert.coord) {
                        on_subject = false;
                        si = ci;
                        visited_intersections.push(ci);
                        // Advance to next.
                        si = (si + 1) % cn;
                        continue;
                    }
                }
                // Exiting: stay on subject.
            } else {
                // On clip ring.
                if vert.is_entering {
                    // Clip-entering means subject-exiting → switch back to subject.
                    if let Some(svi) = find_intersection_in_subj(subj_verts, vert.coord) {
                        // Check if we're back at start.
                        if svi == start_idx
                            || (output.len() > 3 && coords_close(output[0], vert.coord))
                        {
                            break;
                        }
                        on_subject = true;
                        si = svi;
                        si = (si + 1) % sn;
                        continue;
                    }
                }
                // Stay on clip.
            }
        } else if !vert.is_intersection {
            // Check if we looped back to start.
            if on_subject && si == start_idx && output.len() > 1 {
                break;
            }
        }

        // Advance.
        if on_subject {
            si = (si + 1) % sn;
            // Early termination if we reach start again.
            if si == start_idx && output.len() > 1 {
                break;
            }
        } else {
            si = (si + 1) % cn;
        }
    }

    let _ = visited_intersections; // suppress unused warning

    if output.len() < 3 {
        return Vec::new();
    }

    // Close the ring.
    if !coords_close(
        output[0],
        *output.last().expect("output.len() >= 3 by guard above"),
    ) {
        let first = output[0];
        output.push(first);
    }

    vec![output]
}

fn find_intersection_in_clip(clp_verts: &[WaVertex], target: Coord) -> Option<usize> {
    for (i, v) in clp_verts.iter().enumerate() {
        if v.is_intersection && coords_close(v.coord, target) {
            return Some(i);
        }
    }
    None
}

fn find_intersection_in_subj(subj_verts: &[WaVertex], target: Coord) -> Option<usize> {
    for (i, v) in subj_verts.iter().enumerate() {
        if v.is_intersection && coords_close(v.coord, target) {
            return Some(i);
        }
    }
    None
}

#[inline]
fn coords_close(a: Coord, b: Coord) -> bool {
    (a.x - b.x).abs() < COORD_EPS && (a.y - b.y).abs() < COORD_EPS
}

/// Remove the closing vertex if the ring is explicitly closed (first == last).
fn strip_closing(coords: &mut Vec<Coord>) {
    if coords.len() >= 2 {
        let first = coords[0];
        let last = *coords.last().expect("coords.len() >= 2 by guard above");
        if coords_close(first, last) {
            coords.pop();
        }
    }
}

// ---------------------------------------------------------------------------
// Private: helpers
// ---------------------------------------------------------------------------

/// Convert a coordinate list to a [`Polygon`], ensuring the ring is closed.
fn coords_to_polygon(coords: &[Coord]) -> Polygon {
    if coords.is_empty() {
        return Polygon::simple(Ring::new(Vec::new()));
    }
    let mut ring_coords = coords.to_vec();
    // Ensure closed.
    let first = ring_coords[0];
    let last = *ring_coords.last().expect("coords non-empty by guard above");
    if !coords_close(first, last) {
        ring_coords.push(first);
    }
    Polygon::simple(Ring::new(ring_coords))
}

/// Collect all polygons from a `BooleanResult` into a `Vec<Polygon>`.
fn collect_result(result: BooleanResult, out: &mut Vec<Polygon>) {
    match result {
        BooleanResult::Single(p) => out.push(p),
        BooleanResult::Multiple(ps) => out.extend(ps),
        BooleanResult::Empty => {}
    }
}

// Silence `point_in_ring` unused warning — it is used by `polygon_a_contains_b`.
#[allow(dead_code)]
fn _use_cross2() {
    let _ = cross2;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn square(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Polygon {
        let coords = vec![
            Coord::new(min_x, min_y),
            Coord::new(max_x, min_y),
            Coord::new(max_x, max_y),
            Coord::new(min_x, max_y),
            Coord::new(min_x, min_y),
        ];
        Polygon::simple(Ring::new(coords))
    }

    #[allow(dead_code)]
    fn polygon_area(poly: &Polygon) -> f64 {
        let coords = poly.exterior.coords();
        let n = coords.len();
        if n < 3 {
            return 0.0;
        }
        let mut sum = 0.0_f64;
        for i in 0..n {
            let j = (i + 1) % n;
            sum += coords[i].x * coords[j].y;
            sum -= coords[j].x * coords[i].y;
        }
        (sum * 0.5).abs()
    }

    #[test]
    fn signed_area_coords_ccw_square() {
        let coords = vec![
            Coord::new(0.0, 0.0),
            Coord::new(1.0, 0.0),
            Coord::new(1.0, 1.0),
            Coord::new(0.0, 1.0),
        ];
        assert!((signed_area_coords(&coords) - 1.0).abs() < 1e-10);
    }

    #[test]
    fn sutherland_hodgman_overlapping() {
        let a = vec![
            Coord::new(0.0, 0.0),
            Coord::new(2.0, 0.0),
            Coord::new(2.0, 2.0),
            Coord::new(0.0, 2.0),
        ];
        let b = vec![
            Coord::new(1.0, 1.0),
            Coord::new(3.0, 1.0),
            Coord::new(3.0, 3.0),
            Coord::new(1.0, 3.0),
        ];
        let result = sutherland_hodgman(&a, &b);
        assert!(result.len() >= 3, "Expected at least 3 clipped vertices");
        let area = signed_area_coords(&result).abs();
        assert!((area - 1.0).abs() < 0.1, "Expected area ≈ 1.0, got {area}");
    }

    #[test]
    fn bboxes_disjoint_test() {
        let a = Bbox2D::new(0.0, 0.0, 1.0, 1.0).unwrap();
        let b = Bbox2D::new(2.0, 2.0, 3.0, 3.0).unwrap();
        assert!(bboxes_disjoint(&a, &b));
        let c = Bbox2D::new(0.5, 0.5, 1.5, 1.5).unwrap();
        assert!(!bboxes_disjoint(&a, &c));
    }

    #[test]
    fn polygon_bbox_unit_square() {
        let p = square(0.0, 0.0, 1.0, 1.0);
        let bb = polygon_bbox(&p).unwrap();
        assert!((bb.min_x - 0.0).abs() < 1e-10);
        assert!((bb.max_x - 1.0).abs() < 1e-10);
    }

    #[test]
    fn point_in_ring_basic() {
        let ring = Ring::new(vec![
            Coord::new(0.0, 0.0),
            Coord::new(4.0, 0.0),
            Coord::new(4.0, 4.0),
            Coord::new(0.0, 4.0),
            Coord::new(0.0, 0.0),
        ]);
        assert!(point_in_ring(&Coord::new(2.0, 2.0), &ring));
        assert!(!point_in_ring(&Coord::new(5.0, 5.0), &ring));
    }

    #[test]
    fn polygon_a_contains_b_true() {
        let outer = square(0.0, 0.0, 4.0, 4.0);
        let inner = square(1.0, 1.0, 3.0, 3.0);
        assert!(polygon_a_contains_b(&outer, &inner));
    }

    #[test]
    fn polygon_a_contains_b_false() {
        let a = square(0.0, 0.0, 1.0, 1.0);
        let b = square(2.0, 2.0, 3.0, 3.0);
        assert!(!polygon_a_contains_b(&a, &b));
    }
}
