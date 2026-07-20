//! Map generalization operators following the ICA (International Cartographic Association) taxonomy.
//!
//! This module implements three canonical generalization operations:
//!
//! ## Collapse
//! Reduces a polygon or short line below a minimum-size threshold to a representative
//! point (the geometric centroid or midpoint).  This mirrors the ICA "collapse" operator
//! which converts areal/linear features to point symbols when they are too small to be
//! rendered at the target scale.
//!
//! ## Exaggerate
//! Scales a polygon or linestring outward (or inward, for scale < 1) from an anchor
//! point, thereby making the feature visually larger without moving its symbolic
//! location.  Corresponds to the ICA "exaggeration" operator used e.g. for roads or
//! buildings that would otherwise disappear at small scales.
//!
//! ## Displace
//! Resolves positional conflicts between point symbols that are too close together by
//! iteratively pushing overlapping positions apart until all pairwise distances exceed
//! a minimum separation.  Corresponds to the ICA "displacement" operator.
//!
//! # References
//!
//! - Mackaness, W., Ruas, A. & Sarjakoski, L.T. (2007) *Generalisation of Geographic
//!   Information: Cartographic Modelling and Applications*.  ISPRS Book Series, Elsevier.
//! - Ruas, A. (1998) "OO-Generalisation with a multi-agent system", in *Proceedings
//!   of the 8th International Symposium on Spatial Data Handling*, pp. 325–337.

use oxigeo_core::vector::{Coordinate, LineString, Polygon};

// ── Collapse ──────────────────────────────────────────────────────────────────

/// Options that govern when a feature is considered small enough to collapse.
#[derive(Debug, Clone)]
pub struct CollapseOptions {
    /// Area threshold (in the polygon's coordinate units squared).
    /// Polygons whose absolute area is strictly less than this value are collapsed
    /// to a point.  Use `0.0` to disable polygon collapsing.
    pub min_polygon_area: f64,

    /// Length threshold for linestrings (in coordinate units).
    /// When `Some(len)`, linestrings shorter than `len` are collapsed to a point.
    /// `None` disables linestring collapsing entirely.
    pub min_linestring_length: Option<f64>,
}

impl Default for CollapseOptions {
    fn default() -> Self {
        Self {
            min_polygon_area: 0.0,
            min_linestring_length: None,
        }
    }
}

impl CollapseOptions {
    /// Creates options with a minimum polygon area threshold and no linestring threshold.
    pub fn new(min_area: f64) -> Self {
        Self {
            min_polygon_area: min_area,
            min_linestring_length: None,
        }
    }

    /// Builder-style setter for the minimum linestring length threshold.
    pub fn with_min_linestring_length(mut self, min_len: f64) -> Self {
        self.min_linestring_length = Some(min_len);
        self
    }
}

/// Returns `true` when the polygon's absolute planimetric area is strictly
/// below `options.min_polygon_area`, i.e. the polygon should be collapsed.
///
/// # Arguments
///
/// * `poly`    – Reference polygon to test.
/// * `options` – Threshold configuration.
pub fn should_collapse_polygon(poly: &Polygon, options: &CollapseOptions) -> bool {
    polygon_area_abs(poly) < options.min_polygon_area
}

/// Collapses `poly` to its area-weighted 2-D centroid when the polygon's
/// absolute area falls below the configured threshold.
///
/// Returns `Some(centroid_coord)` when collapse occurs, `None` otherwise.
///
/// # Arguments
///
/// * `poly`    – Reference polygon.
/// * `options` – Threshold configuration.
pub fn collapse_polygon_to_point(poly: &Polygon, options: &CollapseOptions) -> Option<Coordinate> {
    if should_collapse_polygon(poly, options) {
        Some(polygon_centroid_2d(poly))
    } else {
        None
    }
}

/// Collapses `line` to its arc-length midpoint when the line's total
/// Euclidean length falls below the configured threshold.
///
/// Returns `Some(midpoint_coord)` when collapse occurs, `None` otherwise.
///
/// # Arguments
///
/// * `line`    – Reference linestring.
/// * `options` – Threshold configuration (uses `min_linestring_length`).
pub fn collapse_linestring_to_point(
    line: &LineString,
    options: &CollapseOptions,
) -> Option<Coordinate> {
    if let Some(min_len) = options.min_linestring_length {
        let len = linestring_length(line);
        if len < min_len {
            return Some(linestring_midpoint(line));
        }
    }
    None
}

// ── Exaggerate ────────────────────────────────────────────────────────────────

/// Specifies the fixed point from which exaggeration is applied.
#[derive(Debug, Clone)]
pub enum ExaggerateAnchor {
    /// Use the area-weighted centroid of the geometry (default, most common).
    Centroid,
    /// Use the centre of the axis-aligned bounding box.
    BoundingBoxCenter,
    /// Use an arbitrary user-supplied coordinate.
    Custom(Coordinate),
}

/// Options controlling the exaggeration transform.
#[derive(Debug, Clone)]
pub struct ExaggerateOptions {
    /// Uniform scale factor applied around the anchor.
    ///
    /// * `> 1.0` enlarges the feature.
    /// * `< 1.0` shrinks the feature.
    /// * `1.0`   leaves positions unchanged.
    pub scale_factor: f64,

    /// Anchor point from which scaling emanates.
    pub anchor: ExaggerateAnchor,
}

impl Default for ExaggerateOptions {
    fn default() -> Self {
        Self {
            scale_factor: 1.0,
            anchor: ExaggerateAnchor::Centroid,
        }
    }
}

/// Applies an affine scale-from-anchor transform to a single coordinate.
///
/// The new position is: `anchor + scale * (point − anchor)`.
///
/// # Arguments
///
/// * `point`  – Coordinate to transform.
/// * `anchor` – Fixed reference point.
/// * `scale`  – Scale factor.
pub fn exaggerate_coord(point: &Coordinate, anchor: &Coordinate, scale: f64) -> Coordinate {
    Coordinate {
        x: anchor.x + scale * (point.x - anchor.x),
        y: anchor.y + scale * (point.y - anchor.y),
        // Preserve optional Z by scaling the offset in 3-D space as well.
        z: match (point.z, anchor.z) {
            (Some(pz), Some(az)) => Some(az + scale * (pz - az)),
            (Some(pz), None) => Some(scale * pz),
            _ => None,
        },
        m: point.m,
    }
}

/// Exaggerates every vertex of `line` from its computed (or specified) anchor.
///
/// # Arguments
///
/// * `line`    – Source linestring.
/// * `options` – Exaggeration parameters.
///
/// # Panics
///
/// Cannot panic: the output coordinate count equals the input count, which is
/// at least 2 (enforced by `LineString::new`).
pub fn exaggerate_linestring(line: &LineString, options: &ExaggerateOptions) -> LineString {
    let anchor = resolve_linestring_anchor(line, options);
    let new_coords: Vec<Coordinate> = line
        .coords()
        .iter()
        .map(|c| exaggerate_coord(c, &anchor, options.scale_factor))
        .collect();
    // Safety: new_coords has same length as the original, which is >= 2.
    LineString::new(new_coords).unwrap_or_else(|_| line.clone())
}

/// Exaggerates all rings of `poly` uniformly from the polygon's anchor.
///
/// Both the exterior ring and every interior (hole) ring are scaled.
///
/// # Arguments
///
/// * `poly`    – Source polygon.
/// * `options` – Exaggeration parameters.
pub fn exaggerate_polygon(poly: &Polygon, options: &ExaggerateOptions) -> Polygon {
    let anchor = resolve_polygon_anchor(poly, options);

    let new_exterior = exaggerate_ring(&poly.exterior, &anchor, options.scale_factor);
    let new_holes: Vec<LineString> = poly
        .interiors
        .iter()
        .map(|ring| exaggerate_ring(ring, &anchor, options.scale_factor))
        .collect();

    Polygon::new(new_exterior, new_holes).unwrap_or_else(|_| poly.clone())
}

// ── Displace ──────────────────────────────────────────────────────────────────

/// Options for the iterative displacement solver.
#[derive(Debug, Clone)]
pub struct DisplaceOptions {
    /// Minimum required Euclidean distance between any two positions (coordinate units).
    pub min_distance: f64,

    /// Maximum number of repulsion iterations before stopping.
    pub max_iterations: usize,

    /// Damping coefficient in `(0, 1]`.
    /// Smaller values yield more conservative per-iteration movements and
    /// smoother convergence; values near 1 converge faster but may overshoot.
    pub damping: f64,

    /// Convergence tolerance: the solver stops early when the largest
    /// single-step displacement is smaller than this value.
    pub convergence_tol: f64,
}

impl Default for DisplaceOptions {
    fn default() -> Self {
        Self {
            min_distance: 1.0,
            max_iterations: 50,
            damping: 0.5,
            convergence_tol: 1e-6,
        }
    }
}

/// Summary statistics returned after the displacement solver finishes.
#[derive(Debug, Clone)]
pub struct DisplaceStats {
    /// Number of iterations actually performed.
    pub iterations: usize,

    /// Maximum per-position movement in the final iteration (coordinate units).
    pub final_max_displacement: f64,

    /// `true` when the solver terminated because `final_max_displacement < convergence_tol`.
    pub converged: bool,
}

/// Displaces a mutable slice of point coordinates so that all pairwise
/// distances are at least `options.min_distance`.
///
/// The algorithm is a damped repulsion loop (O(n² · iterations)):
///
/// 1. For each pair `(i, j)` that violates the minimum distance, compute a
///    repulsion push proportional to the deficit.
/// 2. Accumulate all pushes for each position into a delta vector.
/// 3. Apply all deltas simultaneously (symplectic-style update).
/// 4. Repeat until convergence or `max_iterations` is reached.
///
/// # Arguments
///
/// * `positions` – Mutable slice of coordinates; modified in-place.
/// * `options`   – Solver parameters.
///
/// # Returns
///
/// [`DisplaceStats`] describing solver behaviour.
pub fn displace_points(positions: &mut [Coordinate], options: &DisplaceOptions) -> DisplaceStats {
    if positions.len() < 2 {
        return DisplaceStats {
            iterations: 0,
            final_max_displacement: 0.0,
            converged: true,
        };
    }

    let mut max_disp = 0.0_f64;
    let mut iter = 0_usize;

    for _ in 0..options.max_iterations {
        iter += 1;
        let n = positions.len();
        let mut deltas = vec![(0.0_f64, 0.0_f64); n];

        // Accumulate repulsion deltas for each violating pair (O(n²)).
        for i in 0..n {
            for j in (i + 1)..n {
                let dx = positions[j].x - positions[i].x;
                let dy = positions[j].y - positions[i].y;
                let dist_sq = dx * dx + dy * dy;
                let dist = dist_sq.sqrt();

                if dist < options.min_distance && dist > 1e-12 {
                    let deficit = options.min_distance - dist;
                    // Each side absorbs half the required push, scaled by damping.
                    let push = deficit * 0.5 * options.damping;
                    let ux = dx / dist;
                    let uy = dy / dist;

                    deltas[i].0 -= push * ux;
                    deltas[i].1 -= push * uy;
                    deltas[j].0 += push * ux;
                    deltas[j].1 += push * uy;
                }
            }
        }

        // Apply accumulated deltas and track maximum step size.
        let mut step_max = 0.0_f64;
        for (pos, (ddx, ddy)) in positions.iter_mut().zip(deltas.iter()) {
            pos.x += ddx;
            pos.y += ddy;
            let step = (ddx * ddx + ddy * ddy).sqrt();
            if step > step_max {
                step_max = step;
            }
        }
        max_disp = step_max;

        if max_disp < options.convergence_tol {
            break;
        }
    }

    DisplaceStats {
        iterations: iter,
        final_max_displacement: max_disp,
        converged: max_disp < options.convergence_tol,
    }
}

/// Displaces a mutable slice of polygons by operating on their centroids.
///
/// The algorithm:
///
/// 1. Computes the centroid of each polygon.
/// 2. Runs [`displace_points`] on those centroids.
/// 3. Translates each polygon by the centroid displacement vector.
///
/// This preserves each polygon's shape while resolving centroid conflicts.
///
/// # Arguments
///
/// * `polys`   – Mutable slice of polygons; modified in-place.
/// * `options` – Solver parameters.
///
/// # Returns
///
/// [`DisplaceStats`] from the underlying point-displacement solver.
pub fn displace_polygons_by_centroid(
    polys: &mut [Polygon],
    options: &DisplaceOptions,
) -> DisplaceStats {
    if polys.is_empty() {
        return DisplaceStats {
            iterations: 0,
            final_max_displacement: 0.0,
            converged: true,
        };
    }

    // Collect centroids before displacement.
    let mut centroids: Vec<Coordinate> = polys.iter().map(|p| polygon_centroid_2d(p)).collect();
    let original_centroids = centroids.clone();

    let stats = displace_points(&mut centroids, options);

    // Apply per-polygon translation vectors.
    for (poly, (orig, new_c)) in polys
        .iter_mut()
        .zip(original_centroids.iter().zip(centroids.iter()))
    {
        let dx = new_c.x - orig.x;
        let dy = new_c.y - orig.y;
        *poly = translate_polygon(poly, dx, dy);
    }

    stats
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Returns the absolute planimetric (shoelace) area of `poly`, accounting for
/// interior holes.
fn polygon_area_abs(poly: &Polygon) -> f64 {
    let exterior_area = ring_signed_area(&poly.exterior.coords).abs();
    let holes_area: f64 = poly
        .interiors
        .iter()
        .map(|h| ring_signed_area(&h.coords).abs())
        .sum();
    // Holes reduce the effective area.
    (exterior_area - holes_area).abs()
}

/// Computes the signed area of a closed ring using the shoelace (Gauss) formula.
///
/// Positive for counter-clockwise, negative for clockwise orientation.
fn ring_signed_area(coords: &[Coordinate]) -> f64 {
    if coords.len() < 3 {
        return 0.0;
    }
    let n = coords.len();
    let mut acc = 0.0_f64;
    for i in 0..n {
        let j = (i + 1) % n;
        acc += coords[i].x * coords[j].y;
        acc -= coords[j].x * coords[i].y;
    }
    acc * 0.5
}

/// Computes the area-weighted 2-D centroid of `poly` using the Bourke / Polsby
/// formula, accounting for interior holes.
///
/// Falls back to the simple coordinate average when the exterior ring has zero
/// signed area (degenerate polygon).
fn polygon_centroid_2d(poly: &Polygon) -> Coordinate {
    // ── Exterior ring contribution ─────────────────────────────────────────
    let ext_coords = &poly.exterior.coords;
    let (ext_area, ext_cx, ext_cy) = ring_centroid_parts(ext_coords);

    if ext_area.abs() < f64::EPSILON {
        // Degenerate: fall back to simple average of exterior vertices.
        return coordinate_average(ext_coords);
    }

    let mut total_area = ext_area;
    let mut wx = ext_cx * ext_area;
    let mut wy = ext_cy * ext_area;

    // ── Subtract hole contributions ────────────────────────────────────────
    for hole in &poly.interiors {
        let (h_area, h_cx, h_cy) = ring_centroid_parts(&hole.coords);
        total_area -= h_area;
        wx -= h_cx * h_area;
        wy -= h_cy * h_area;
    }

    if total_area.abs() < f64::EPSILON {
        return coordinate_average(ext_coords);
    }

    Coordinate::new_2d(wx / total_area, wy / total_area)
}

/// Returns `(signed_area, cx, cy)` for a single closed ring using the
/// standard area-weighted centroid formula (Bourke, 1988).
fn ring_centroid_parts(coords: &[Coordinate]) -> (f64, f64, f64) {
    if coords.len() < 3 {
        return (0.0, 0.0, 0.0);
    }
    let n = coords.len();
    let mut area = 0.0_f64;
    let mut cx = 0.0_f64;
    let mut cy = 0.0_f64;

    for i in 0..n {
        let j = (i + 1) % n;
        let cross = coords[i].x * coords[j].y - coords[j].x * coords[i].y;
        area += cross;
        cx += (coords[i].x + coords[j].x) * cross;
        cy += (coords[i].y + coords[j].y) * cross;
    }

    area *= 0.5;

    if area.abs() < f64::EPSILON {
        // Zero-area ring: return simple average position.
        let avg = coordinate_average(coords);
        return (0.0, avg.x, avg.y);
    }

    cx /= 6.0 * area;
    cy /= 6.0 * area;

    (area, cx, cy)
}

/// Returns the simple arithmetic mean of a coordinate slice.
fn coordinate_average(coords: &[Coordinate]) -> Coordinate {
    if coords.is_empty() {
        return Coordinate::new_2d(0.0, 0.0);
    }
    let n = coords.len() as f64;
    let sum_x: f64 = coords.iter().map(|c| c.x).sum();
    let sum_y: f64 = coords.iter().map(|c| c.y).sum();
    Coordinate::new_2d(sum_x / n, sum_y / n)
}

/// Returns the total Euclidean arc-length of `line`.
fn linestring_length(line: &LineString) -> f64 {
    let coords = &line.coords;
    let n = coords.len();
    if n < 2 {
        return 0.0;
    }
    let mut len = 0.0_f64;
    for i in 0..(n - 1) {
        let dx = coords[i + 1].x - coords[i].x;
        let dy = coords[i + 1].y - coords[i].y;
        len += (dx * dx + dy * dy).sqrt();
    }
    len
}

/// Returns the coordinate located at the arc-length midpoint of `line`.
///
/// Interpolates linearly within the segment that straddles the half-length
/// position, preserving sub-segment precision.
fn linestring_midpoint(line: &LineString) -> Coordinate {
    let coords = &line.coords;
    let n = coords.len();

    if n == 0 {
        return Coordinate::new_2d(0.0, 0.0);
    }
    if n == 1 {
        return coords[0];
    }

    let total = linestring_length(line);
    if total < f64::EPSILON {
        // Degenerate – all points coincide; return the first.
        return coords[0];
    }

    let half = total * 0.5;
    let mut accumulated = 0.0_f64;

    for i in 0..(n - 1) {
        let dx = coords[i + 1].x - coords[i].x;
        let dy = coords[i + 1].y - coords[i].y;
        let seg_len = (dx * dx + dy * dy).sqrt();

        if accumulated + seg_len >= half {
            // The midpoint lies within this segment.
            let t = if seg_len > f64::EPSILON {
                (half - accumulated) / seg_len
            } else {
                0.0
            };
            return Coordinate::new_2d(coords[i].x + t * dx, coords[i].y + t * dy);
        }
        accumulated += seg_len;
    }

    // Floating-point rounding: return the last vertex.
    coords[n - 1]
}

/// Scales every vertex of a closed ring outward from `anchor` by `scale`.
///
/// The closure property is preserved because both the first and last
/// coordinate are identical after transformation (they share the same
/// input coordinate).
fn exaggerate_ring(ring: &LineString, anchor: &Coordinate, scale: f64) -> LineString {
    let new_coords: Vec<Coordinate> = ring
        .coords
        .iter()
        .map(|c| exaggerate_coord(c, anchor, scale))
        .collect();
    // new_coords has >= 2 elements because ring validation requires >= 4.
    LineString::new(new_coords).unwrap_or_else(|_| ring.clone())
}

/// Resolves the anchor coordinate for a polygon, given the chosen strategy.
fn resolve_polygon_anchor(poly: &Polygon, options: &ExaggerateOptions) -> Coordinate {
    match &options.anchor {
        ExaggerateAnchor::Centroid => polygon_centroid_2d(poly),
        ExaggerateAnchor::BoundingBoxCenter => {
            polygon_bbox_center(poly).unwrap_or_else(|| polygon_centroid_2d(poly))
        }
        ExaggerateAnchor::Custom(c) => *c,
    }
}

/// Resolves the anchor coordinate for a linestring, given the chosen strategy.
fn resolve_linestring_anchor(line: &LineString, options: &ExaggerateOptions) -> Coordinate {
    match &options.anchor {
        ExaggerateAnchor::Centroid => linestring_arc_centroid(line),
        ExaggerateAnchor::BoundingBoxCenter => {
            linestring_bbox_center(line).unwrap_or_else(|| linestring_arc_centroid(line))
        }
        ExaggerateAnchor::Custom(c) => *c,
    }
}

/// Returns the axis-aligned bounding-box centre of `poly`, or `None` if the
/// polygon has no vertices.
fn polygon_bbox_center(poly: &Polygon) -> Option<Coordinate> {
    if poly.exterior.coords.is_empty() {
        return None;
    }
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    for c in &poly.exterior.coords {
        if c.x < min_x {
            min_x = c.x;
        }
        if c.x > max_x {
            max_x = c.x;
        }
        if c.y < min_y {
            min_y = c.y;
        }
        if c.y > max_y {
            max_y = c.y;
        }
    }
    Some(Coordinate::new_2d(
        (min_x + max_x) * 0.5,
        (min_y + max_y) * 0.5,
    ))
}

/// Returns the axis-aligned bounding-box centre of `line`, or `None` if it
/// has no vertices.
fn linestring_bbox_center(line: &LineString) -> Option<Coordinate> {
    if line.coords.is_empty() {
        return None;
    }
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    for c in &line.coords {
        if c.x < min_x {
            min_x = c.x;
        }
        if c.x > max_x {
            max_x = c.x;
        }
        if c.y < min_y {
            min_y = c.y;
        }
        if c.y > max_y {
            max_y = c.y;
        }
    }
    Some(Coordinate::new_2d(
        (min_x + max_x) * 0.5,
        (min_y + max_y) * 0.5,
    ))
}

/// Arc-length weighted centroid of a linestring (midpoint of each segment,
/// weighted by segment length).  Degenerates to the first vertex for a
/// zero-length linestring.
fn linestring_arc_centroid(line: &LineString) -> Coordinate {
    let coords = &line.coords;
    let n = coords.len();

    if n == 0 {
        return Coordinate::new_2d(0.0, 0.0);
    }
    if n == 1 {
        return coords[0];
    }

    let mut total_len = 0.0_f64;
    let mut wx = 0.0_f64;
    let mut wy = 0.0_f64;

    for i in 0..(n - 1) {
        let dx = coords[i + 1].x - coords[i].x;
        let dy = coords[i + 1].y - coords[i].y;
        let seg_len = (dx * dx + dy * dy).sqrt();

        if seg_len > f64::EPSILON {
            let mid_x = (coords[i].x + coords[i + 1].x) * 0.5;
            let mid_y = (coords[i].y + coords[i + 1].y) * 0.5;
            wx += mid_x * seg_len;
            wy += mid_y * seg_len;
            total_len += seg_len;
        }
    }

    if total_len < f64::EPSILON {
        return coords[0];
    }

    Coordinate::new_2d(wx / total_len, wy / total_len)
}

/// Translates every vertex of `poly` by `(dx, dy)`, returning a new polygon.
///
/// Both the exterior ring and all interior rings are translated.  Returns a
/// clone of the original polygon on the (impossible in practice) event that
/// ring reconstruction fails.
fn translate_polygon(poly: &Polygon, dx: f64, dy: f64) -> Polygon {
    let new_exterior = translate_ring(&poly.exterior, dx, dy);
    let new_holes: Vec<LineString> = poly
        .interiors
        .iter()
        .map(|h| translate_ring(h, dx, dy))
        .collect();
    Polygon::new(new_exterior, new_holes).unwrap_or_else(|_| poly.clone())
}

/// Translates every vertex of `ring` by `(dx, dy)`.
fn translate_ring(ring: &LineString, dx: f64, dy: f64) -> LineString {
    let new_coords: Vec<Coordinate> = ring
        .coords
        .iter()
        .map(|c| Coordinate {
            x: c.x + dx,
            y: c.y + dy,
            z: c.z,
            m: c.m,
        })
        .collect();
    LineString::new(new_coords).unwrap_or_else(|_| ring.clone())
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_square(x0: f64, y0: f64, side: f64) -> Polygon {
        let coords = vec![
            Coordinate::new_2d(x0, y0),
            Coordinate::new_2d(x0 + side, y0),
            Coordinate::new_2d(x0 + side, y0 + side),
            Coordinate::new_2d(x0, y0 + side),
            Coordinate::new_2d(x0, y0),
        ];
        let ext = LineString::new(coords).expect("valid square ring");
        Polygon::new(ext, vec![]).expect("valid square polygon")
    }

    #[test]
    fn test_polygon_area_abs_unit_square() {
        let poly = make_square(0.0, 0.0, 1.0);
        let area = polygon_area_abs(&poly);
        assert!((area - 1.0).abs() < 1e-12);
    }

    #[test]
    fn test_polygon_centroid_2d_unit_square() {
        let poly = make_square(0.0, 0.0, 1.0);
        let c = polygon_centroid_2d(&poly);
        assert!((c.x - 0.5).abs() < 1e-12);
        assert!((c.y - 0.5).abs() < 1e-12);
    }

    #[test]
    fn test_linestring_length_horizontal() {
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(3.0, 0.0),
            Coordinate::new_2d(3.0, 4.0),
        ];
        let line = LineString::new(coords).expect("valid");
        assert!((linestring_length(&line) - 7.0).abs() < 1e-12);
    }

    #[test]
    fn test_linestring_midpoint_two_points() {
        let coords = vec![Coordinate::new_2d(0.0, 0.0), Coordinate::new_2d(4.0, 0.0)];
        let line = LineString::new(coords).expect("valid");
        let mid = linestring_midpoint(&line);
        assert!((mid.x - 2.0).abs() < 1e-12);
        assert!((mid.y - 0.0).abs() < 1e-12);
    }

    #[test]
    fn test_exaggerate_coord_doubles_from_origin() {
        let anchor = Coordinate::new_2d(0.0, 0.0);
        let point = Coordinate::new_2d(1.0, 2.0);
        let result = exaggerate_coord(&point, &anchor, 2.0);
        assert!((result.x - 2.0).abs() < 1e-12);
        assert!((result.y - 4.0).abs() < 1e-12);
    }

    #[test]
    fn test_translate_polygon_moves_by_delta() {
        let poly = make_square(0.0, 0.0, 1.0);
        let moved = translate_polygon(&poly, 5.0, 3.0);
        let c = polygon_centroid_2d(&moved);
        assert!((c.x - 5.5).abs() < 1e-12);
        assert!((c.y - 3.5).abs() < 1e-12);
    }
}
