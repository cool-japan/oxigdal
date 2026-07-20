//! Line offset (parallel curve) generation for cartographic styling
//!
//! This module implements **open-curve parallel offset** — shifting a polyline
//! laterally to produce a parallel polyline — distinct from the filled-polygon
//! buffer in `buffer.rs`.
//!
//! # Conventions
//!
//! A **positive** distance places the offset curve on the **left** of the
//! direction of travel (right-hand coordinate system).  A **negative** distance
//! places it on the **right**.
//!
//! # Join styles
//!
//! At interior vertices the two adjacent offset segment directions differ.
//! Three strategies reconcile them:
//!
//! - **Miter** – extend both segments to their intersection point.  If the
//!   resulting extension ratio exceeds `miter_limit`, fall back to `Bevel`.
//! - **Bevel** – insert two points (one per segment), slicing off the sharp
//!   corner.
//! - **Round** – interpolate a circular arc between the two offset directions.
//!   Arc resolution adapts to the turn angle at approximately 8 segments per π.
//!
//! # Example
//!
//! ```
//! use oxigeo_algorithms::vector::{JoinStyle, OffsetOptions, offset_linestring};
//!
//! let coords = vec![(0.0_f64, 0.0_f64), (10.0, 0.0)];
//! let opts = OffsetOptions::default();
//! let result = offset_linestring(&coords, 1.0, &opts)?;
//! assert_eq!(result.coords.len(), 2);
//! assert!((result.coords[0].1 - 1.0).abs() < 1e-10);
//! # Ok::<(), oxigeo_algorithms::error::AlgorithmError>(())
//! ```

use std::f64::consts::PI;

use crate::error::{AlgorithmError, Result};
use crate::vector::simplify_linestring_dp;
use oxigeo_core::vector::{Coordinate, LineString};

// ─────────────────────────────────────────────────────────────────────────────
// Public types
// ─────────────────────────────────────────────────────────────────────────────

/// Join style used when constructing the offset curve at interior vertices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum JoinStyle {
    /// Sharp miter join, clipped to `miter_limit`.
    #[default]
    Miter,
    /// Flat bevel — two points per corner, no extension.
    Bevel,
    /// Circular arc, proportional number of segments.
    Round,
}

/// Options controlling how the offset curve is generated.
#[derive(Debug, Clone)]
pub struct OffsetOptions {
    /// Maximum miter-extension ratio before falling back to bevel.
    ///
    /// The miter length at a corner is `|d / sin(θ/2)|`.  If this ratio
    /// relative to `|d|` exceeds `miter_limit` the corner is bevelled instead.
    /// Default is `10.0`.
    pub miter_limit: f64,
    /// Join style applied at each interior vertex.
    pub join_style: JoinStyle,
    /// Optional Douglas-Peucker tolerance applied to the output coordinates.
    ///
    /// When `Some(tol)` the output polyline is simplified with tolerance `tol`.
    /// `None` means no simplification.
    pub simplify_tolerance: Option<f64>,
}

impl Default for OffsetOptions {
    fn default() -> Self {
        Self {
            miter_limit: 10.0,
            join_style: JoinStyle::Miter,
            simplify_tolerance: None,
        }
    }
}

/// Result returned by [`offset_linestring`] and its variants.
#[derive(Debug, Clone)]
pub struct OffsetResult {
    /// The offset coordinates as `(x, y)` tuples.
    pub coords: Vec<(f64, f64)>,
    /// `true` if `simplify_tolerance` was set and Douglas-Peucker was applied.
    pub was_simplified: bool,
}

// ─────────────────────────────────────────────────────────────────────────────
// Public API
// ─────────────────────────────────────────────────────────────────────────────

/// Compute a parallel offset of an open polyline.
///
/// # Arguments
///
/// * `coords`   – Input vertices as `(x, y)` slice.  Consecutive duplicate
///   vertices are treated as zero-length segments and skipped gracefully.
/// * `distance` – Signed offset distance.  Positive = left of travel direction,
///   negative = right.
/// * `options`  – Join style, miter limit, and optional simplification.
///
/// # Errors
///
/// Returns [`AlgorithmError::InsufficientData`] when `coords.len() < 2`.
///
/// # Panics
///
/// Never panics in production code.
pub fn offset_linestring(
    coords: &[(f64, f64)],
    distance: f64,
    options: &OffsetOptions,
) -> Result<OffsetResult> {
    let n = coords.len();
    if n < 2 {
        return Err(AlgorithmError::InsufficientData {
            operation: "offset_linestring",
            message: format!("need at least 2 vertices, got {n}"),
        });
    }

    // Degenerate: zero distance → return a copy of the input.
    if distance == 0.0 {
        return Ok(OffsetResult {
            coords: coords.to_vec(),
            was_simplified: false,
        });
    }

    // Compute per-segment left-normal vectors (skipping degenerate segs).
    let normals = compute_segment_normals(coords);

    // Build the offset polyline one vertex at a time.
    let mut out: Vec<(f64, f64)> = Vec::with_capacity(n + 8);

    // ── Start vertex ─────────────────────────────────────────────────────────
    let n0 = first_valid_normal(&normals);
    let (px, py) = coords[0];
    out.push((px + distance * n0.0, py + distance * n0.1));

    // ── Interior vertices ─────────────────────────────────────────────────────
    for i in 1..n - 1 {
        let na = prev_valid_normal(&normals, i);
        let nb = next_valid_normal(&normals, i);
        let (vx, vy) = coords[i];
        emit_join_points(&mut out, (vx, vy), na, nb, distance, options);
    }

    // ── End vertex ────────────────────────────────────────────────────────────
    let n_last = last_valid_normal(&normals);
    let (ex, ey) = coords[n - 1];
    out.push((ex + distance * n_last.0, ey + distance * n_last.1));

    // ── Optional Douglas-Peucker simplification ────────────────────────────
    let was_simplified;
    if let Some(tol) = options.simplify_tolerance {
        let ls_coords: Vec<Coordinate> =
            out.iter().map(|&(x, y)| Coordinate::new_2d(x, y)).collect();
        match LineString::new(ls_coords) {
            Ok(ls) => match simplify_linestring_dp(&ls, tol) {
                Ok(simplified) => {
                    out = simplified.coords.iter().map(|c| (c.x, c.y)).collect();
                    was_simplified = true;
                }
                Err(_) => {
                    // Simplification failed – keep raw output.
                    was_simplified = false;
                }
            },
            Err(_) => {
                was_simplified = false;
            }
        }
    } else {
        was_simplified = false;
    }

    Ok(OffsetResult {
        coords: out,
        was_simplified,
    })
}

/// Compute parallel offset for each ring of a polygon independently.
///
/// Each ring is treated as a **closed** curve — all vertices are interior and
/// no endpoint special-casing is applied.  The bisector/miter join logic wraps
/// cyclically around the ring.
///
/// A positive `distance` expands exterior rings outward; the caller is
/// responsible for negating the distance for holes if needed.
///
/// # Errors
///
/// Returns [`AlgorithmError::InsufficientData`] if any input ring has < 3
/// distinct positions (i.e. the raw slice length is < 3).
pub fn offset_polygon_rings(
    rings: &[Vec<(f64, f64)>],
    distance: f64,
    options: &OffsetOptions,
) -> Result<Vec<Vec<(f64, f64)>>> {
    rings
        .iter()
        .map(|ring| offset_closed_ring(ring, distance, options))
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// Closed-ring offset
// ─────────────────────────────────────────────────────────────────────────────

/// Offset a single closed ring (polygon ring).
///
/// The signed-distance convention for closed rings:
/// - Positive distance = **outward** for a CCW exterior ring (left-hand rule
///   means left-normals point inward for CCW, so we negate the distance
///   internally when the ring is CCW).
/// - Positive distance = inward for a CW interior ring (hole), which is the
///   caller's responsibility to handle by negating the distance passed in.
fn offset_closed_ring(
    ring: &[(f64, f64)],
    distance: f64,
    options: &OffsetOptions,
) -> Result<Vec<(f64, f64)>> {
    // Normalise: strip repeated closing vertex if present so all positions
    // are unique.
    let effective_ring = strip_closing_vertex(ring);
    let n = effective_ring.len();
    if n < 3 {
        return Err(AlgorithmError::InsufficientData {
            operation: "offset_polygon_rings",
            message: format!("ring must have at least 3 distinct vertices, got {n}"),
        });
    }

    // Determine ring orientation using the shoelace formula.
    // Positive signed area → CCW winding.
    let signed_area = ring_signed_area(effective_ring);
    // For CCW rings the left-normal points **inward**.  Negate the distance so
    // that the caller's positive distance always means "outward expansion" for
    // an exterior (CCW) ring.
    let effective_distance = if signed_area > 0.0 {
        -distance
    } else {
        distance
    };

    // Compute normals for the n cyclic segments.
    let normals = compute_cyclic_normals(effective_ring);

    let mut out: Vec<(f64, f64)> = Vec::with_capacity(n + 4);

    for i in 0..n {
        // In a closed ring every vertex is an interior join vertex.
        let prev_seg = if i == 0 { n - 1 } else { i - 1 };
        let curr_seg = i;

        let na = normals[prev_seg];
        let nb = normals[curr_seg];
        let (vx, vy) = effective_ring[i];
        emit_join_points(&mut out, (vx, vy), na, nb, effective_distance, options);
    }

    // Close the ring.
    if let Some(&first) = out.first() {
        out.push(first);
    }

    Ok(out)
}

// ─────────────────────────────────────────────────────────────────────────────
// Normal computation helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Per-segment left-normal vectors for an open polyline.
///
/// Returns a `Vec` of length `n - 1`.  Degenerate (zero-length) segments get
/// `(0.0, 0.0)` so they can be skipped later.
fn compute_segment_normals(coords: &[(f64, f64)]) -> Vec<(f64, f64)> {
    coords.windows(2).map(|w| left_normal(w[0], w[1])).collect()
}

/// Cyclic per-segment normals for a closed ring of `n` vertices.
/// Segment `i` runs from `ring[i]` to `ring[(i+1) % n]`.
fn compute_cyclic_normals(ring: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let n = ring.len();
    (0..n)
        .map(|i| left_normal(ring[i], ring[(i + 1) % n]))
        .collect()
}

/// Left-perpendicular unit normal of the directed segment `a → b`.
///
/// Returns `(0.0, 0.0)` for a degenerate (zero-length) segment.
fn left_normal(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    let len = dx.hypot(dy);
    if len < f64::EPSILON {
        return (0.0, 0.0);
    }
    // Left normal: rotate direction 90° counter-clockwise → (-dy, dx) / len
    (-dy / len, dx / len)
}

/// Returns the first non-degenerate normal in `normals`.
fn first_valid_normal(normals: &[(f64, f64)]) -> (f64, f64) {
    normals
        .iter()
        .find(|&&n| normal_is_valid(n))
        .copied()
        .unwrap_or((0.0, 0.0))
}

/// Returns the last non-degenerate normal in `normals`.
fn last_valid_normal(normals: &[(f64, f64)]) -> (f64, f64) {
    normals
        .iter()
        .rev()
        .find(|&&n| normal_is_valid(n))
        .copied()
        .unwrap_or((0.0, 0.0))
}

/// For interior vertex `i`, returns the normal of the **previous** segment
/// (the segment ending at `i`), skipping degenerate ones by scanning backward.
fn prev_valid_normal(normals: &[(f64, f64)], vertex_idx: usize) -> (f64, f64) {
    // segment i-1 ends at vertex i
    let seg_idx = vertex_idx.saturating_sub(1);
    (0..=seg_idx)
        .rev()
        .map(|j| normals[j])
        .find(|&n| normal_is_valid(n))
        .unwrap_or((0.0, 0.0))
}

/// For interior vertex `i`, returns the normal of the **next** segment
/// (the segment starting at `i`), skipping degenerate ones by scanning forward.
fn next_valid_normal(normals: &[(f64, f64)], vertex_idx: usize) -> (f64, f64) {
    // segment i starts at vertex i (index = vertex_idx in the normals array)
    (vertex_idx..normals.len())
        .map(|j| normals[j])
        .find(|&n| normal_is_valid(n))
        .unwrap_or((0.0, 0.0))
}

/// True when the normal is a proper unit vector (not the degenerate `(0,0)`).
#[inline]
fn normal_is_valid(n: (f64, f64)) -> bool {
    n.0 != 0.0 || n.1 != 0.0
}

// ─────────────────────────────────────────────────────────────────────────────
// Join-point emission
// ─────────────────────────────────────────────────────────────────────────────

/// Emit offset point(s) for vertex `v` given the normals of the two adjacent
/// segments.  Mutates `out` by appending 1 (Miter/degenerate), 2 (Bevel), or
/// several (Round) points.
fn emit_join_points(
    out: &mut Vec<(f64, f64)>,
    v: (f64, f64),
    na: (f64, f64),
    nb: (f64, f64),
    distance: f64,
    options: &OffsetOptions,
) {
    // Points on each offset segment, at vertex v.
    let pa = (v.0 + distance * na.0, v.1 + distance * na.1);
    let pb = (v.0 + distance * nb.0, v.1 + distance * nb.1);

    // If both normals are degenerate (collinear duplicates) just push pa.
    if !normal_is_valid(na) && !normal_is_valid(nb) {
        out.push(pa);
        return;
    }
    if !normal_is_valid(na) {
        out.push(pb);
        return;
    }
    if !normal_is_valid(nb) {
        out.push(pa);
        return;
    }

    match options.join_style {
        JoinStyle::Bevel => {
            emit_bevel(out, pa, pb);
        }
        JoinStyle::Round => {
            emit_round(out, v, pa, pb, distance);
        }
        JoinStyle::Miter => {
            emit_miter(out, v, na, nb, pa, pb, distance, options.miter_limit);
        }
    }
}

/// Bevel join: just the two points, one per offset segment.
#[inline]
fn emit_bevel(out: &mut Vec<(f64, f64)>, pa: (f64, f64), pb: (f64, f64)) {
    out.push(pa);
    out.push(pb);
}

/// Miter join.  Computes the bisector and the miter point.  Falls back to
/// bevel when the extension ratio would exceed `miter_limit`.
fn emit_miter(
    out: &mut Vec<(f64, f64)>,
    v: (f64, f64),
    na: (f64, f64),
    nb: (f64, f64),
    pa: (f64, f64),
    pb: (f64, f64),
    distance: f64,
    miter_limit: f64,
) {
    // Bisector direction = normalise(na + nb).
    let bx = na.0 + nb.0;
    let by = na.1 + nb.1;
    let blen = bx.hypot(by);

    // Collinear (anti-parallel) normals: straight continuation, single point.
    if blen < f64::EPSILON {
        out.push(pa);
        return;
    }

    let (bux, buy) = (bx / blen, by / blen);

    // dot(bisector_unit, na) = cos(θ/2) where θ is the exterior turn angle.
    // miter_length = distance / dot(bisector_unit, na)  [signed].
    let dot = bux * na.0 + buy * na.1;

    // Guard: if dot ≈ 0 the lines are almost anti-parallel → very long miter.
    if dot.abs() < f64::EPSILON {
        emit_bevel(out, pa, pb);
        return;
    }

    let miter_length = distance / dot;
    let ratio = (miter_length / distance).abs();

    if ratio > miter_limit.abs() || !miter_length.is_finite() {
        // Clamp to bevel.
        emit_bevel(out, pa, pb);
    } else {
        // Single miter point.
        out.push((v.0 + miter_length * bux, v.1 + miter_length * buy));
    }
}

/// Round join: arc from `pa` to `pb` around vertex `v`.
///
/// Uses approximately 8 segments per π radians (clamped to at least 1).
fn emit_round(
    out: &mut Vec<(f64, f64)>,
    v: (f64, f64),
    pa: (f64, f64),
    pb: (f64, f64),
    distance: f64,
) {
    out.push(pa);

    let radius = distance.abs();
    if radius < f64::EPSILON {
        out.push(pb);
        return;
    }

    let angle_a = (pa.1 - v.1).atan2(pa.0 - v.0);
    let angle_b = (pb.1 - v.1).atan2(pb.0 - v.0);

    // Angular span in the correct winding direction.
    let mut delta = angle_b - angle_a;

    // Normalise delta to (-π, π].
    while delta > PI {
        delta -= 2.0 * PI;
    }
    while delta < -PI {
        delta += 2.0 * PI;
    }

    // ~8 segments per π.
    let segments = (((delta.abs() / PI) * 8.0).ceil() as usize).max(1);

    for k in 1..segments {
        let t = (k as f64) / (segments as f64);
        let angle = angle_a + t * delta;
        out.push((v.0 + radius * angle.cos(), v.1 + radius * angle.sin()));
    }

    out.push(pb);
}

// ─────────────────────────────────────────────────────────────────────────────
// Ring utility
// ─────────────────────────────────────────────────────────────────────────────

/// Shoelace signed area of a closed polygon ring (no closing duplicate vertex).
///
/// Positive = CCW (counter-clockwise) winding in a standard coordinate system
/// (y-up / right-hand).
fn ring_signed_area(ring: &[(f64, f64)]) -> f64 {
    let n = ring.len();
    let mut sum = 0.0_f64;
    for i in 0..n {
        let j = (i + 1) % n;
        sum += ring[i].0 * ring[j].1;
        sum -= ring[j].0 * ring[i].1;
    }
    sum / 2.0
}

/// Returns a sub-slice that strips the repeated closing vertex if the last
/// point equals the first.  Otherwise returns the slice unchanged.
fn strip_closing_vertex(ring: &[(f64, f64)]) -> &[(f64, f64)] {
    if ring.len() >= 2 {
        let first = ring[0];
        let last = ring[ring.len() - 1];
        if (first.0 - last.0).abs() < f64::EPSILON && (first.1 - last.1).abs() < f64::EPSILON {
            return &ring[..ring.len() - 1];
        }
    }
    ring
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn opts_miter() -> OffsetOptions {
        OffsetOptions {
            join_style: JoinStyle::Miter,
            ..Default::default()
        }
    }

    fn opts_bevel() -> OffsetOptions {
        OffsetOptions {
            join_style: JoinStyle::Bevel,
            ..Default::default()
        }
    }

    fn opts_round() -> OffsetOptions {
        OffsetOptions {
            join_style: JoinStyle::Round,
            ..Default::default()
        }
    }

    // ── left_normal ──────────────────────────────────────────────────────────

    #[test]
    fn test_left_normal_east() {
        // Eastward segment: left is north (+y).
        let n = left_normal((0.0, 0.0), (1.0, 0.0));
        assert!((n.0 - 0.0).abs() < 1e-12, "nx should be 0, got {}", n.0);
        assert!((n.1 - 1.0).abs() < 1e-12, "ny should be 1, got {}", n.1);
    }

    #[test]
    fn test_left_normal_north() {
        // Northward segment: left is west (-x).
        let n = left_normal((0.0, 0.0), (0.0, 1.0));
        assert!((n.0 - (-1.0)).abs() < 1e-12);
        assert!((n.1 - 0.0).abs() < 1e-12);
    }

    // ── offset_linestring ────────────────────────────────────────────────────

    #[test]
    fn test_offset_horizontal_line_left_positive() {
        let coords = vec![(0.0, 0.0), (10.0, 0.0)];
        let result = offset_linestring(&coords, 1.0, &opts_miter());
        assert!(result.is_ok(), "expected Ok, got {result:?}");
        let r = result.expect("checked above");
        assert_eq!(r.coords.len(), 2);
        for &(_, y) in &r.coords {
            assert!((y - 1.0).abs() < 1e-10, "expected y=1, got {y}");
        }
    }

    #[test]
    fn test_offset_horizontal_line_right_negative() {
        let coords = vec![(0.0, 0.0), (10.0, 0.0)];
        let result = offset_linestring(&coords, -1.0, &opts_miter());
        assert!(result.is_ok());
        let r = result.expect("checked above");
        assert_eq!(r.coords.len(), 2);
        for &(_, y) in &r.coords {
            assert!((y - (-1.0)).abs() < 1e-10, "expected y=-1, got {y}");
        }
    }

    #[test]
    fn test_offset_zero_distance_returns_input() {
        let coords = vec![(0.0, 0.0), (5.0, 5.0), (10.0, 0.0)];
        let result = offset_linestring(&coords, 0.0, &opts_miter());
        assert!(result.is_ok());
        let r = result.expect("checked above");
        assert_eq!(r.coords, coords);
    }

    #[test]
    fn test_offset_right_angle_miter_within_limit() {
        // L-shape: (0,10)→(0,0)→(10,0).  Left offset +1.
        let coords = vec![(0.0, 10.0), (0.0, 0.0), (10.0, 0.0)];
        let opts = opts_miter();
        let result = offset_linestring(&coords, 1.0, &opts);
        assert!(result.is_ok());
        let r = result.expect("checked above");
        // No NaN.
        for &(x, y) in &r.coords {
            assert!(x.is_finite(), "x must be finite, got {x}");
            assert!(y.is_finite(), "y must be finite, got {y}");
        }
        // Must have at least 3 points (start, corner, end).
        assert!(r.coords.len() >= 3, "got {} points", r.coords.len());
    }

    #[test]
    fn test_offset_right_angle_bevel_style() {
        // Bevel produces two points at the corner → 4 total (start, 2×corner, end).
        let coords = vec![(0.0, 10.0), (0.0, 0.0), (10.0, 0.0)];
        let result = offset_linestring(&coords, 1.0, &opts_bevel());
        assert!(result.is_ok());
        let r = result.expect("checked above");
        assert_eq!(
            r.coords.len(),
            4,
            "bevel should produce 4 points, got {}",
            r.coords.len()
        );
    }

    #[test]
    fn test_offset_miter_limit_clamps_sharp_angle() {
        // Very acute inward turn (almost 180° reversal): normals are nearly
        // anti-parallel → miter would be enormous → clamp to bevel.
        // Spike: (0,0)→(5,0)→(5, 0.001)→(0,0) — use a near-reversal.
        // A near-antiparallel turn: go east then almost-west.
        let coords = vec![(0.0, 0.0), (10.0, 0.0), (10.05, 0.01)];
        let opts = OffsetOptions {
            join_style: JoinStyle::Miter,
            miter_limit: 2.0,
            ..Default::default()
        };
        let result = offset_linestring(&coords, 1.0, &opts);
        assert!(result.is_ok());
        let r = result.expect("checked above");
        for &(x, y) in &r.coords {
            assert!(x.is_finite() && y.is_finite(), "coords must be finite");
        }
    }

    #[test]
    fn test_offset_insufficient_vertices_errors() {
        let coords = vec![(0.0, 0.0)]; // Only 1 point.
        let result = offset_linestring(&coords, 1.0, &opts_miter());
        assert!(result.is_err(), "expected Err for 1-point input");
    }

    #[test]
    fn test_offset_round_style_no_nan() {
        let coords = vec![(0.0, 10.0), (0.0, 0.0), (10.0, 0.0)];
        let result = offset_linestring(&coords, 1.0, &opts_round());
        assert!(result.is_ok());
        let r = result.expect("checked above");
        for &(x, y) in &r.coords {
            assert!(
                x.is_finite() && y.is_finite(),
                "round join produced non-finite coord"
            );
        }
        // Round produces more points than bevel (arc segments in between).
        assert!(
            r.coords.len() > 4,
            "expected >4 coords for round join, got {}",
            r.coords.len()
        );
    }

    #[test]
    fn test_offset_polygon_ring_square() {
        // Square (0,0)→(10,0)→(10,10)→(0,10)→(0,0), offset +1 outward.
        let ring = vec![
            (0.0_f64, 0.0_f64),
            (10.0, 0.0),
            (10.0, 10.0),
            (0.0, 10.0),
            (0.0, 0.0), // closing vertex
        ];
        let opts = OffsetOptions {
            join_style: JoinStyle::Miter,
            ..Default::default()
        };
        let result = offset_polygon_rings(&[ring], 1.0, &opts);
        assert!(result.is_ok());
        let rings = result.expect("checked above");
        assert_eq!(rings.len(), 1);
        let out_ring = &rings[0];

        // All coordinates finite.
        for &(x, y) in out_ring {
            assert!(x.is_finite() && y.is_finite());
        }

        // Rough area check: original 10×10=100, offset +1 on all sides → ~144.
        // Use shoelace on the (closed) output ring.
        let area = shoelace_area(out_ring);
        assert!(
            area > 100.0,
            "expanded area should exceed original; got {area}"
        );
    }

    /// Shoelace formula for a closed ring (last point == first point).
    fn shoelace_area(ring: &[(f64, f64)]) -> f64 {
        let n = ring.len();
        let mut sum = 0.0;
        for i in 0..n - 1 {
            sum += ring[i].0 * ring[i + 1].1;
            sum -= ring[i + 1].0 * ring[i].1;
        }
        sum.abs() / 2.0
    }
}
