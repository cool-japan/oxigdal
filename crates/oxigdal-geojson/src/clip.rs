//! Bounding-box clipping algorithms for GeoJSON geometries.
//!
//! # Algorithms
//!
//! - **Cohen-Sutherland** for line-string clipping (segment-by-segment, with
//!   output stitching into continuous sub-strings).
//! - **Sutherland-Hodgman** for polygon ring clipping (four successive
//!   half-plane clips).
//!
//! All coordinates are treated as 2-D (x, y).  The Z-aware entry points
//! (`LineStringZ`, `PolygonZ`, etc.) project to 2-D for the clip test and
//! re-interpolate Z at intersection points.

use crate::types::GeoJsonGeometry;

// ─── ClipBox ────────────────────────────────────────────────────────────────

/// Axis-aligned clipping box `[min_x, min_y, max_x, max_y]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipBox {
    /// Left boundary (minimum x).
    pub min_x: f64,
    /// Bottom boundary (minimum y).
    pub min_y: f64,
    /// Right boundary (maximum x).
    pub max_x: f64,
    /// Top boundary (maximum y).
    pub max_y: f64,
}

impl ClipBox {
    /// Construct a new clipping box.
    ///
    /// # Panics (debug only)
    /// Panics in debug mode when `min_x > max_x` or `min_y > max_y`.
    #[inline]
    #[must_use]
    pub fn new(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        debug_assert!(min_x <= max_x, "min_x must not exceed max_x");
        debug_assert!(min_y <= max_y, "min_y must not exceed max_y");
        Self {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    }

    /// Returns `true` when the point `(x, y)` lies inside or on the boundary.
    #[inline]
    #[must_use]
    pub fn contains_point(&self, x: f64, y: f64) -> bool {
        x >= self.min_x && x <= self.max_x && y >= self.min_y && y <= self.max_y
    }

    /// Construct from a 4-element bbox array `[minx, miny, maxx, maxy]`.
    #[inline]
    #[must_use]
    pub fn from_bbox(bbox: [f64; 4]) -> Self {
        Self::new(bbox[0], bbox[1], bbox[2], bbox[3])
    }
}

// ─── Cohen-Sutherland outcode bits ──────────────────────────────────────────

const CS_INSIDE: u8 = 0b0000;
const CS_LEFT: u8 = 0b0001;
const CS_RIGHT: u8 = 0b0010;
const CS_BOTTOM: u8 = 0b0100;
const CS_TOP: u8 = 0b1000;

/// Compute the Cohen-Sutherland region outcode for point `(x, y)`.
#[inline]
fn outcode(x: f64, y: f64, clip: &ClipBox) -> u8 {
    let mut code = CS_INSIDE;
    if x < clip.min_x {
        code |= CS_LEFT;
    } else if x > clip.max_x {
        code |= CS_RIGHT;
    }
    if y < clip.min_y {
        code |= CS_BOTTOM;
    } else if y > clip.max_y {
        code |= CS_TOP;
    }
    code
}

// ─── Cohen-Sutherland segment clip ──────────────────────────────────────────

/// Clip a single line segment from `p0` to `p1` against `clip`.
///
/// Returns `Some((clipped_p0, clipped_p1))` when the segment (or a portion of
/// it) lies inside the box, or `None` when it is completely outside.
fn clip_segment_cs(
    mut x0: f64,
    mut y0: f64,
    mut x1: f64,
    mut y1: f64,
    clip: &ClipBox,
) -> Option<([f64; 2], [f64; 2])> {
    let mut out0 = outcode(x0, y0, clip);
    let mut out1 = outcode(x1, y1, clip);

    loop {
        if out0 | out1 == CS_INSIDE {
            // Both inside — trivially accept.
            return Some(([x0, y0], [x1, y1]));
        }
        if out0 & out1 != CS_INSIDE {
            // Both on the same outside side — trivially reject.
            return None;
        }

        // Pick the endpoint that is outside.
        let out_current = if out0 != CS_INSIDE { out0 } else { out1 };

        // Find intersection with the relevant clip edge.
        let (xi, yi) = if out_current & CS_TOP != CS_INSIDE {
            // Clip to top edge (y = max_y)
            let t = (clip.max_y - y0) / (y1 - y0);
            (x0 + t * (x1 - x0), clip.max_y)
        } else if out_current & CS_BOTTOM != CS_INSIDE {
            // Clip to bottom edge (y = min_y)
            let t = (clip.min_y - y0) / (y1 - y0);
            (x0 + t * (x1 - x0), clip.min_y)
        } else if out_current & CS_RIGHT != CS_INSIDE {
            // Clip to right edge (x = max_x)
            let t = (clip.max_x - x0) / (x1 - x0);
            (clip.max_x, y0 + t * (y1 - y0))
        } else {
            // Clip to left edge (x = min_x)
            let t = (clip.min_x - x0) / (x1 - x0);
            (clip.min_x, y0 + t * (y1 - y0))
        };

        // Move the outside endpoint to the intersection point and update
        // its outcode.
        if out_current == out0 {
            x0 = xi;
            y0 = yi;
            out0 = outcode(x0, y0, clip);
        } else {
            x1 = xi;
            y1 = yi;
            out1 = outcode(x1, y1, clip);
        }
    }
}

// ─── Linestring clipping (Cohen-Sutherland, with output stitching) ───────────

/// Clip a 2-D polyline against `clip` using the Cohen-Sutherland algorithm.
///
/// Each segment `(coords[i], coords[i+1])` is clipped independently.
/// Adjacent clipped segments whose shared endpoint agrees (within a tolerance
/// of 1 × 10⁻¹⁰) are stitched into a single continuous output linestring.
///
/// Returns a `Vec` of sub-linestrings (may be empty if the entire polyline lies
/// outside the box).
#[must_use]
pub fn clip_linestring(coords: &[[f64; 2]], clip: &ClipBox) -> Vec<Vec<[f64; 2]>> {
    if coords.len() < 2 {
        // A single point or empty input is not a linestring — return nothing.
        return Vec::new();
    }

    const TOL: f64 = 1e-10;

    let mut result: Vec<Vec<[f64; 2]>> = Vec::new();
    // The linestring currently being accumulated.
    let mut current: Vec<[f64; 2]> = Vec::new();
    // The last clipped endpoint from the previous accepted segment (if any).
    let mut prev_end: Option<[f64; 2]> = None;

    for i in 0..coords.len() - 1 {
        let [x0, y0] = coords[i];
        let [x1, y1] = coords[i + 1];

        match clip_segment_cs(x0, y0, x1, y1, clip) {
            None => {
                // Segment is outside — flush any current sub-string.
                if !current.is_empty() {
                    result.push(current.clone());
                    current.clear();
                }
                prev_end = None;
            }
            Some((p0, p1)) => {
                // Decide whether to extend the current sub-string or start a
                // new one.
                let can_stitch = match prev_end {
                    Some(pe) => (pe[0] - p0[0]).abs() < TOL && (pe[1] - p0[1]).abs() < TOL,
                    None => false,
                };

                if can_stitch {
                    // Extend: p0 is already the last point of `current`.
                    current.push(p1);
                } else {
                    // Start a new sub-string.
                    if !current.is_empty() {
                        result.push(current.clone());
                        current.clear();
                    }
                    current.push(p0);
                    current.push(p1);
                }
                prev_end = Some(p1);
            }
        }
    }

    // Flush the last sub-string.
    if !current.is_empty() {
        result.push(current);
    }

    result
}

// ─── Sutherland-Hodgman half-plane clip helpers ──────────────────────────────

/// Compute the intersection of the line segment `(s, e)` with the clip
/// edge defined by the four clipping half-planes used in Sutherland-Hodgman.
///
/// The clip edge is described by its normal direction and limit:
/// - `axis`  — `0` = x axis, `1` = y axis
/// - `limit` — the boundary value along `axis`
/// - `min_side` — `true` means "inside ≥ limit", `false` means "inside ≤ limit"
#[inline]
fn sh_intersect(sx: f64, sy: f64, ex: f64, ey: f64, axis: usize, limit: f64) -> [f64; 2] {
    // Parametric t for the intersection.
    // For axis == 0 (x clip): t = (limit - sx) / (ex - sx)
    // For axis == 1 (y clip): t = (limit - sy) / (ey - sy)
    let (s_val, e_val, s_other, e_other) = if axis == 0 {
        (sx, ex, sy, ey)
    } else {
        (sy, ey, sx, ex)
    };

    let dv = e_val - s_val;
    let t = if dv.abs() < f64::EPSILON {
        0.0
    } else {
        (limit - s_val) / dv
    };
    let other = s_other + t * (e_other - s_other);

    if axis == 0 {
        [limit, other]
    } else {
        [other, limit]
    }
}

/// Test whether `(x, y)` is on the *inside* of a half-plane.
///
/// - axis 0, min_side true  → x ≥ limit  (left clip)
/// - axis 0, min_side false → x ≤ limit  (right clip)
/// - axis 1, min_side true  → y ≥ limit  (bottom clip)
/// - axis 1, min_side false → y ≤ limit  (top clip)
#[inline]
fn sh_inside(x: f64, y: f64, axis: usize, limit: f64, min_side: bool) -> bool {
    let val = if axis == 0 { x } else { y };
    if min_side { val >= limit } else { val <= limit }
}

/// Apply a single Sutherland-Hodgman clip pass against one half-plane edge.
fn sh_clip_one_edge(
    polygon: &[[f64; 2]],
    axis: usize,
    limit: f64,
    min_side: bool,
) -> Vec<[f64; 2]> {
    if polygon.is_empty() {
        return Vec::new();
    }

    let n = polygon.len();
    let mut output = Vec::with_capacity(n + 4);

    for i in 0..n {
        let [sx, sy] = polygon[i];
        let [ex, ey] = polygon[(i + 1) % n];

        let s_inside = sh_inside(sx, sy, axis, limit, min_side);
        let e_inside = sh_inside(ex, ey, axis, limit, min_side);

        match (s_inside, e_inside) {
            (true, true) => {
                // Both inside — emit e.
                output.push([ex, ey]);
            }
            (true, false) => {
                // s inside, e outside — emit intersection.
                output.push(sh_intersect(sx, sy, ex, ey, axis, limit));
            }
            (false, true) => {
                // s outside, e inside — emit intersection then e.
                output.push(sh_intersect(sx, sy, ex, ey, axis, limit));
                output.push([ex, ey]);
            }
            (false, false) => {
                // Both outside — emit nothing.
            }
        }
    }

    output
}

// ─── Polygon ring clipping (Sutherland-Hodgman) ──────────────────────────────

/// Clip a single 2-D polygon ring against `clip` using the Sutherland-Hodgman
/// algorithm.
///
/// The four clip edges are applied in sequence:
/// 1. Left   (x ≥ min_x)
/// 2. Right  (x ≤ max_x)
/// 3. Bottom (y ≥ min_y)
/// 4. Top    (y ≤ max_y)
///
/// Returns the clipped ring.  An empty `Vec` is returned when the ring lies
/// entirely outside the box or the result is degenerate (fewer than 3 vertices).
/// Ring closure (first == last) is preserved when the input was closed.
#[must_use]
pub fn clip_polygon_ring(ring: &[[f64; 2]], clip: &ClipBox) -> Vec<[f64; 2]> {
    if ring.is_empty() {
        return Vec::new();
    }

    // Detect whether the input ring is closed (first == last within tolerance).
    const TOL: f64 = 1e-10;
    let first = ring[0];
    let last = *ring.last().unwrap_or(&ring[0]); // ring is non-empty — this is always Some
    let was_closed = (first[0] - last[0]).abs() < TOL && (first[1] - last[1]).abs() < TOL;

    // Work on an open representation for Sutherland-Hodgman.
    let open: &[[f64; 2]] = if was_closed && ring.len() > 1 {
        &ring[..ring.len() - 1]
    } else {
        ring
    };

    // Apply the four half-plane clips.
    let poly = sh_clip_one_edge(open, 0, clip.min_x, true); // x ≥ min_x
    let poly = sh_clip_one_edge(&poly, 0, clip.max_x, false); // x ≤ max_x
    let poly = sh_clip_one_edge(&poly, 1, clip.min_y, true); // y ≥ min_y
    let poly = sh_clip_one_edge(&poly, 1, clip.max_y, false); // y ≤ max_y

    if poly.len() < 3 {
        // Degenerate — discard.
        return Vec::new();
    }

    // Re-close the ring if the input was closed.
    if was_closed {
        let mut closed = poly;
        let first_pt = closed[0];
        closed.push(first_pt);
        closed
    } else {
        poly
    }
}

// ─── Full polygon clipping ───────────────────────────────────────────────────

/// Clip a 2-D polygon (exterior ring + optional holes) against `clip`.
///
/// Returns `None` when the exterior ring is completely outside the clip box,
/// otherwise `Some(rings)` where `rings[0]` is the clipped exterior and any
/// remaining elements are the clipped holes (holes that became empty are
/// dropped).
#[must_use]
pub fn clip_polygon(rings: &[Vec<[f64; 2]>], clip: &ClipBox) -> Option<Vec<Vec<[f64; 2]>>> {
    if rings.is_empty() {
        return None;
    }

    // Clip the exterior ring.
    let exterior = clip_polygon_ring(&rings[0], clip);
    if exterior.is_empty() {
        return None;
    }

    let mut clipped = vec![exterior];

    // Clip each hole.
    for hole in &rings[1..] {
        let clipped_hole = clip_polygon_ring(hole, clip);
        if !clipped_hole.is_empty() {
            clipped.push(clipped_hole);
        }
    }

    Some(clipped)
}

// ─── Z-interpolation helpers ─────────────────────────────────────────────────

/// Given a 3-D segment `(p0, p1)` and a 2-D clipped segment with the same
/// parametric span, compute the Z values at the clipped endpoints by linear
/// interpolation.
///
/// `seg_p0` and `seg_p1` are the original 3-D endpoints of the segment.
/// `clip_p0` and `clip_p1` are the clipped 2-D endpoints.
fn interpolate_z_for_segment(
    seg_p0: [f64; 3],
    seg_p1: [f64; 3],
    clip_p0: [f64; 2],
    clip_p1: [f64; 2],
) -> ([f64; 3], [f64; 3]) {
    // Compute parametric t for each clipped endpoint relative to the full
    // 2-D segment length.
    let dx = seg_p1[0] - seg_p0[0];
    let dy = seg_p1[1] - seg_p0[1];
    let seg_len_sq = dx * dx + dy * dy;

    let t0 = if seg_len_sq < f64::EPSILON {
        0.0
    } else {
        let dpx = clip_p0[0] - seg_p0[0];
        let dpy = clip_p0[1] - seg_p0[1];
        (dpx * dx + dpy * dy) / seg_len_sq
    };

    let t1 = if seg_len_sq < f64::EPSILON {
        1.0
    } else {
        let dpx = clip_p1[0] - seg_p0[0];
        let dpy = clip_p1[1] - seg_p0[1];
        (dpx * dx + dpy * dy) / seg_len_sq
    };

    let z0 = seg_p0[2] + t0 * (seg_p1[2] - seg_p0[2]);
    let z1 = seg_p0[2] + t1 * (seg_p1[2] - seg_p0[2]);

    ([clip_p0[0], clip_p0[1], z0], [clip_p1[0], clip_p1[1], z1])
}

/// Clip a 3-D linestring using Cohen-Sutherland (operating on the XY
/// projection) and re-attach interpolated Z values at intersection points.
///
/// Returns a `Vec` of 3-D sub-linestrings.
fn clip_linestring_3d(coords: &[[f64; 3]], clip: &ClipBox) -> Vec<Vec<[f64; 3]>> {
    if coords.len() < 2 {
        return Vec::new();
    }

    const TOL: f64 = 1e-10;

    let mut result: Vec<Vec<[f64; 3]>> = Vec::new();
    let mut current: Vec<[f64; 3]> = Vec::new();
    let mut prev_end_3d: Option<[f64; 3]> = None;

    for i in 0..coords.len() - 1 {
        let p0_3d = coords[i];
        let p1_3d = coords[i + 1];
        let [x0, y0, _] = p0_3d;
        let [x1, y1, _] = p1_3d;

        match clip_segment_cs(x0, y0, x1, y1, clip) {
            None => {
                if !current.is_empty() {
                    result.push(current.clone());
                    current.clear();
                }
                prev_end_3d = None;
            }
            Some((clip_p0, clip_p1)) => {
                let (cp0_3d, cp1_3d) = interpolate_z_for_segment(p0_3d, p1_3d, clip_p0, clip_p1);

                let can_stitch = match prev_end_3d {
                    Some(pe) => (pe[0] - cp0_3d[0]).abs() < TOL && (pe[1] - cp0_3d[1]).abs() < TOL,
                    None => false,
                };

                if can_stitch {
                    current.push(cp1_3d);
                } else {
                    if !current.is_empty() {
                        result.push(current.clone());
                        current.clear();
                    }
                    current.push(cp0_3d);
                    current.push(cp1_3d);
                }
                prev_end_3d = Some(cp1_3d);
            }
        }
    }

    if !current.is_empty() {
        result.push(current);
    }

    result
}

/// Clip a single 3-D polygon ring using Sutherland-Hodgman in 2-D and
/// re-interpolate Z values for intersection vertices.
fn clip_polygon_ring_3d(ring: &[[f64; 3]], clip: &ClipBox) -> Vec<[f64; 3]> {
    if ring.is_empty() {
        return Vec::new();
    }

    // Project to 2-D.
    let ring_2d: Vec<[f64; 2]> = ring.iter().map(|[x, y, _]| [*x, *y]).collect();

    // Detect ring closure.
    const TOL: f64 = 1e-10;
    let first = ring[0];
    let last = ring[ring.len() - 1];
    let was_closed = (first[0] - last[0]).abs() < TOL && (first[1] - last[1]).abs() < TOL;

    // Clip in 2-D.
    let clipped_2d = clip_polygon_ring(&ring_2d, clip);
    if clipped_2d.is_empty() {
        return Vec::new();
    }

    // Reconstruct Z for each 2-D clipped vertex by projecting it back onto the
    // nearest original segment.
    //
    // We use a simple nearest-segment scan.  This is O(n_clipped × n_original)
    // which is acceptable for typical polygon ring sizes.
    let open_ring: &[[f64; 3]] = if was_closed && ring.len() > 1 {
        &ring[..ring.len() - 1]
    } else {
        ring
    };

    let mut result_3d: Vec<[f64; 3]> = clipped_2d
        .iter()
        .map(|&[cx, cy]| {
            let z = z_at_point_on_ring(open_ring, cx, cy);
            [cx, cy, z]
        })
        .collect();

    // Fix closure: make last == first in Z too if the output was closed.
    if was_closed && result_3d.len() >= 2 {
        let n = result_3d.len();
        let first_z = result_3d[0][2];
        result_3d[n - 1][2] = first_z;
    }

    result_3d
}

/// Interpolate Z at 2-D point `(px, py)` by finding which segment of the open
/// ring `pts3d` it is closest to and computing the along-segment parameter.
fn z_at_point_on_ring(pts3d: &[[f64; 3]], px: f64, py: f64) -> f64 {
    let n = pts3d.len();
    if n == 0 {
        return 0.0;
    }
    if n == 1 {
        return pts3d[0][2];
    }

    let mut best_t = 0.0_f64;
    let mut best_dist_sq = f64::MAX;
    let mut best_z0 = pts3d[0][2];
    let mut best_z1 = pts3d[1 % n][2];

    for i in 0..n {
        let [ax, ay, az] = pts3d[i];
        let [bx, by, bz] = pts3d[(i + 1) % n];

        let dx = bx - ax;
        let dy = by - ay;
        let len_sq = dx * dx + dy * dy;

        let t = if len_sq < f64::EPSILON {
            0.0
        } else {
            ((px - ax) * dx + (py - ay) * dy) / len_sq
        };
        let t_clamped = t.clamp(0.0, 1.0);

        let qx = ax + t_clamped * dx;
        let qy = ay + t_clamped * dy;
        let dist_sq = (px - qx) * (px - qx) + (py - qy) * (py - qy);

        if dist_sq < best_dist_sq {
            best_dist_sq = dist_sq;
            best_t = t_clamped;
            best_z0 = az;
            best_z1 = bz;
        }
    }

    best_z0 + best_t * (best_z1 - best_z0)
}

// ─── Main dispatch: clip_geometry ────────────────────────────────────────────

/// Clip a GeoJSON geometry against `clip`.
///
/// Returns `Some(clipped_geometry)` when any part of the geometry overlaps the
/// clip box, or `None` when the geometry is entirely outside.
///
/// Geometry semantics:
/// - **Point / PointZ** — inside/outside test.
/// - **LineString / LineStringZ** — Cohen-Sutherland per-segment clipping.
///   A single output sub-string is returned as `LineString`; multiple
///   sub-strings as `MultiLineString`.
/// - **Polygon / PolygonZ** — Sutherland-Hodgman ring clipping.
/// - **Multi*** — each sub-geometry is clipped individually; empty results are
///   dropped.
/// - **GeometryCollection** — each child is clipped; empty results are dropped.
/// - **Null** — always returns `None`.
#[must_use]
pub fn clip_geometry(geom: &GeoJsonGeometry, clip: &ClipBox) -> Option<GeoJsonGeometry> {
    match geom {
        // ── Points ───────────────────────────────────────────────────────────
        GeoJsonGeometry::Point([x, y]) => {
            if clip.contains_point(*x, *y) {
                Some(GeoJsonGeometry::Point([*x, *y]))
            } else {
                None
            }
        }
        GeoJsonGeometry::PointZ([x, y, z]) => {
            if clip.contains_point(*x, *y) {
                Some(GeoJsonGeometry::PointZ([*x, *y, *z]))
            } else {
                None
            }
        }

        // ── LineStrings ──────────────────────────────────────────────────────
        GeoJsonGeometry::LineString(coords) => {
            let parts = clip_linestring(coords, clip);
            linestring_parts_to_geom(parts)
        }
        GeoJsonGeometry::LineStringZ(coords) => {
            let parts = clip_linestring_3d(coords, clip);
            linestring_z_parts_to_geom(parts)
        }

        // ── Polygons ─────────────────────────────────────────────────────────
        GeoJsonGeometry::Polygon(rings) => clip_polygon(rings, clip).map(GeoJsonGeometry::Polygon),
        GeoJsonGeometry::PolygonZ(rings) => {
            clip_polygon_z(rings, clip).map(GeoJsonGeometry::PolygonZ)
        }

        // ── MultiPoints ──────────────────────────────────────────────────────
        GeoJsonGeometry::MultiPoint(pts) => {
            let clipped: Vec<[f64; 2]> = pts
                .iter()
                .filter(|[x, y]| clip.contains_point(*x, *y))
                .copied()
                .collect();
            if clipped.is_empty() {
                None
            } else {
                Some(GeoJsonGeometry::MultiPoint(clipped))
            }
        }
        GeoJsonGeometry::MultiPointZ(pts) => {
            let clipped: Vec<[f64; 3]> = pts
                .iter()
                .filter(|[x, y, _]| clip.contains_point(*x, *y))
                .copied()
                .collect();
            if clipped.is_empty() {
                None
            } else {
                Some(GeoJsonGeometry::MultiPointZ(clipped))
            }
        }

        // ── MultiLineStrings ─────────────────────────────────────────────────
        GeoJsonGeometry::MultiLineString(lines) => {
            let mut all_parts: Vec<Vec<[f64; 2]>> = Vec::new();
            for line in lines {
                let parts = clip_linestring(line, clip);
                all_parts.extend(parts);
            }
            if all_parts.is_empty() {
                None
            } else {
                Some(GeoJsonGeometry::MultiLineString(all_parts))
            }
        }
        GeoJsonGeometry::MultiLineStringZ(lines) => {
            let mut all_parts: Vec<Vec<[f64; 3]>> = Vec::new();
            for line in lines {
                let parts = clip_linestring_3d(line, clip);
                all_parts.extend(parts);
            }
            if all_parts.is_empty() {
                None
            } else {
                Some(GeoJsonGeometry::MultiLineStringZ(all_parts))
            }
        }

        // ── MultiPolygons ────────────────────────────────────────────────────
        GeoJsonGeometry::MultiPolygon(polys) => {
            let clipped: Vec<Vec<Vec<[f64; 2]>>> = polys
                .iter()
                .filter_map(|rings| clip_polygon(rings, clip))
                .collect();
            if clipped.is_empty() {
                None
            } else {
                Some(GeoJsonGeometry::MultiPolygon(clipped))
            }
        }
        GeoJsonGeometry::MultiPolygonZ(polys) => {
            let clipped: Vec<Vec<Vec<[f64; 3]>>> = polys
                .iter()
                .filter_map(|rings| clip_polygon_z(rings, clip))
                .collect();
            if clipped.is_empty() {
                None
            } else {
                Some(GeoJsonGeometry::MultiPolygonZ(clipped))
            }
        }

        // ── GeometryCollection ───────────────────────────────────────────────
        GeoJsonGeometry::GeometryCollection(children) => {
            let clipped: Vec<GeoJsonGeometry> = children
                .iter()
                .filter_map(|g| clip_geometry(g, clip))
                .collect();
            if clipped.is_empty() {
                None
            } else {
                Some(GeoJsonGeometry::GeometryCollection(clipped))
            }
        }

        // ── Null ─────────────────────────────────────────────────────────────
        GeoJsonGeometry::Null => None,
    }
}

// ─── Internal helpers for geometry construction ───────────────────────────────

/// Convert clipped 2-D linestring parts into a `GeoJsonGeometry`.
fn linestring_parts_to_geom(parts: Vec<Vec<[f64; 2]>>) -> Option<GeoJsonGeometry> {
    match parts.len() {
        0 => None,
        1 => {
            // SAFETY: parts.len() == 1, so index is valid.
            parts.into_iter().next().map(GeoJsonGeometry::LineString)
        }
        _ => Some(GeoJsonGeometry::MultiLineString(parts)),
    }
}

/// Convert clipped 3-D linestring parts into a `GeoJsonGeometry`.
fn linestring_z_parts_to_geom(parts: Vec<Vec<[f64; 3]>>) -> Option<GeoJsonGeometry> {
    match parts.len() {
        0 => None,
        1 => parts.into_iter().next().map(GeoJsonGeometry::LineStringZ),
        _ => Some(GeoJsonGeometry::MultiLineStringZ(parts)),
    }
}

/// Clip a 3-D polygon (rings with Z) using Sutherland-Hodgman (via 2-D
/// projection).
fn clip_polygon_z(rings: &[Vec<[f64; 3]>], clip: &ClipBox) -> Option<Vec<Vec<[f64; 3]>>> {
    if rings.is_empty() {
        return None;
    }

    let exterior = clip_polygon_ring_3d(&rings[0], clip);
    if exterior.is_empty() {
        return None;
    }

    let mut clipped = vec![exterior];
    for hole in &rings[1..] {
        let clipped_hole = clip_polygon_ring_3d(hole, clip);
        if !clipped_hole.is_empty() {
            clipped.push(clipped_hole);
        }
    }

    Some(clipped)
}

// ─── Unit tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn clip_box() -> ClipBox {
        ClipBox::new(0.0, 0.0, 10.0, 10.0)
    }

    // ── ClipBox ───────────────────────────────────────────────────────────────

    #[test]
    fn test_clip_box_contains_interior() {
        let cb = clip_box();
        assert!(cb.contains_point(5.0, 5.0));
    }

    #[test]
    fn test_clip_box_contains_boundary() {
        let cb = clip_box();
        assert!(cb.contains_point(0.0, 0.0));
        assert!(cb.contains_point(10.0, 10.0));
    }

    #[test]
    fn test_clip_box_excludes_outside() {
        let cb = clip_box();
        assert!(!cb.contains_point(-1.0, 5.0));
        assert!(!cb.contains_point(5.0, 11.0));
    }

    #[test]
    fn test_clip_box_from_bbox() {
        let cb = ClipBox::from_bbox([1.0, 2.0, 3.0, 4.0]);
        assert_eq!(cb.min_x, 1.0);
        assert_eq!(cb.max_y, 4.0);
    }

    // ── Cohen-Sutherland segment ──────────────────────────────────────────────

    #[test]
    fn test_cs_segment_fully_inside() {
        let cb = clip_box();
        let result = clip_segment_cs(2.0, 2.0, 8.0, 8.0, &cb);
        assert!(result.is_some());
        let (p0, p1) = result.expect("should be inside");
        assert!((p0[0] - 2.0).abs() < 1e-10);
        assert!((p1[0] - 8.0).abs() < 1e-10);
    }

    #[test]
    fn test_cs_segment_fully_outside_same_side() {
        let cb = clip_box();
        // Both points to the left.
        let result = clip_segment_cs(-5.0, 3.0, -2.0, 7.0, &cb);
        assert!(result.is_none());
    }

    #[test]
    fn test_cs_segment_crosses_left_boundary() {
        let cb = clip_box();
        // From (-5, 5) to (5, 5) — horizontal, crosses x=0.
        let result = clip_segment_cs(-5.0, 5.0, 5.0, 5.0, &cb);
        assert!(result.is_some());
        let (p0, p1) = result.expect("should clip");
        assert!((p0[0] - 0.0).abs() < 1e-10);
        assert!((p0[1] - 5.0).abs() < 1e-10);
        assert!((p1[0] - 5.0).abs() < 1e-10);
    }

    // ── Linestring clipping ───────────────────────────────────────────────────

    #[test]
    fn test_linestring_fully_inside() {
        let cb = clip_box();
        let coords: Vec<[f64; 2]> = vec![[1.0, 1.0], [5.0, 5.0], [9.0, 2.0]];
        let parts = clip_linestring(&coords, &cb);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].len(), 3);
    }

    #[test]
    fn test_linestring_fully_outside() {
        let cb = clip_box();
        let coords: Vec<[f64; 2]> = vec![[-5.0, 5.0], [-3.0, 5.0]];
        let parts = clip_linestring(&coords, &cb);
        assert!(parts.is_empty());
    }

    #[test]
    fn test_linestring_crosses_one_boundary() {
        let cb = clip_box();
        // Horizontal from (-5, 5) to (15, 5) — should clip to (0,5)-(10,5).
        let coords: Vec<[f64; 2]> = vec![[-5.0, 5.0], [15.0, 5.0]];
        let parts = clip_linestring(&coords, &cb);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].len(), 2);
        assert!((parts[0][0][0] - 0.0).abs() < 1e-9);
        assert!((parts[0][1][0] - 10.0).abs() < 1e-9);
    }

    // ── Sutherland-Hodgman polygon ring ───────────────────────────────────────

    #[test]
    fn test_polygon_ring_fully_inside() {
        let cb = clip_box();
        let ring = vec![[1.0, 1.0], [9.0, 1.0], [9.0, 9.0], [1.0, 9.0], [1.0, 1.0]];
        let clipped = clip_polygon_ring(&ring, &cb);
        // Should be unchanged (4 corners + closure = 5).
        assert_eq!(clipped.len(), 5);
    }

    #[test]
    fn test_polygon_ring_larger_than_box() {
        let cb = clip_box();
        // 20×20 square around the clip box.
        let ring = vec![
            [-5.0, -5.0],
            [15.0, -5.0],
            [15.0, 15.0],
            [-5.0, 15.0],
            [-5.0, -5.0],
        ];
        let clipped = clip_polygon_ring(&ring, &cb);
        assert!(!clipped.is_empty());
        // All resulting vertices should be within the clip box.
        for [x, y] in &clipped {
            assert!(*x >= -1e-9 && *x <= 10.0 + 1e-9);
            assert!(*y >= -1e-9 && *y <= 10.0 + 1e-9);
        }
    }

    #[test]
    fn test_polygon_ring_entirely_outside() {
        let cb = clip_box();
        let ring = vec![
            [20.0, 20.0],
            [30.0, 20.0],
            [30.0, 30.0],
            [20.0, 30.0],
            [20.0, 20.0],
        ];
        let clipped = clip_polygon_ring(&ring, &cb);
        assert!(clipped.is_empty());
    }
}
