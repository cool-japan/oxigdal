//! Voronoi diagram builder.
//!
//! Given a set of seed points and an axis-aligned bounding box, this module
//! computes the planar Voronoi diagram — the partition of the plane into cells
//! where each cell contains all points closer to one seed than to any other.
//!
//! # Algorithm
//!
//! For each seed `v`, its Voronoi cell is the intersection of half-planes:
//!
//! ```text
//! dist(p, v)² ≤ dist(p, u)²  for all other seeds u
//! ↔  2(u−v)·p ≤ ‖u‖² − ‖v‖²
//! ```
//!
//! Starting from the full bbox polygon, each pair `(v, u)` clips the current
//! polygon to the half containing `v`.  This is O(n²) per cell but exact and
//! free of halfedge-walk complexity.
//!
//! Neighbor information is derived from the Delaunay triangulation via
//! `delaunator`, which also handles the early-exit optimisation for interior
//! cells (only Delaunay neighbours need to be checked).

use crate::error::IndexError;

// ---------------------------------------------------------------------------
// Public data types
// ---------------------------------------------------------------------------

/// A 2D seed point for Voronoi construction.
#[derive(Debug, Clone, PartialEq)]
pub struct VoronoiPoint {
    /// X coordinate.
    pub x: f64,
    /// Y coordinate.
    pub y: f64,
}

/// A single Voronoi cell.
#[derive(Debug, Clone)]
pub struct VoronoiCell {
    /// The seed point that generated this cell.
    pub seed: VoronoiPoint,
    /// CCW-ordered vertices of the cell polygon (clipped to `bbox`).
    pub vertices: Vec<(f64, f64)>,
    /// Indices into [`VoronoiDiagram::cells`] of cells that share an edge
    /// with this cell.
    pub neighbors: Vec<usize>,
}

/// The complete planar Voronoi diagram.
#[derive(Debug, Clone)]
pub struct VoronoiDiagram {
    /// One cell per input seed, in the same order as the input slice.
    pub cells: Vec<VoronoiCell>,
    /// The axis-aligned clipping bounding box `(min_x, min_y, max_x, max_y)`.
    pub bbox: (f64, f64, f64, f64),
}

// ---------------------------------------------------------------------------
// Circumcenter
// ---------------------------------------------------------------------------

/// Compute the circumcenter of triangle `a`–`b`–`c`.
///
/// Returns `None` when the three points are collinear (determinant < 1e-12).
pub fn circumcenter(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> Option<(f64, f64)> {
    let ax = a.0;
    let ay = a.1;
    let bx = b.0;
    let by = b.1;
    let cx = c.0;
    let cy = c.1;

    let d = 2.0 * (ax * (by - cy) + bx * (cy - ay) + cx * (ay - by));
    if d.abs() < 1e-12 {
        return None;
    }

    let aa = ax * ax + ay * ay;
    let bb = bx * bx + by * by;
    let cc = cx * cx + cy * cy;

    let ux = (aa * (by - cy) + bb * (cy - ay) + cc * (ay - by)) / d;
    let uy = (aa * (cx - bx) + bb * (ax - cx) + cc * (bx - ax)) / d;
    Some((ux, uy))
}

// ---------------------------------------------------------------------------
// Shoelace area + winding helpers
// ---------------------------------------------------------------------------

/// Shoelace signed area (positive = CCW).
fn signed_area(poly: &[(f64, f64)]) -> f64 {
    let n = poly.len();
    if n < 3 {
        return 0.0;
    }
    let mut sum = 0.0_f64;
    for i in 0..n {
        let j = (i + 1) % n;
        sum += poly[i].0 * poly[j].1;
        sum -= poly[j].0 * poly[i].1;
    }
    sum * 0.5
}

/// Ensure a polygon is wound CCW; reverses in place if CW.
fn ensure_ccw(poly: &mut [(f64, f64)]) {
    if signed_area(poly) < 0.0 {
        poly.reverse();
    }
}

// ---------------------------------------------------------------------------
// Sutherland–Hodgman polygon clipping
// ---------------------------------------------------------------------------

/// Clip `poly` against the half-plane `nx*x + ny*y + d >= 0`.
fn sutherland_clip_one_edge(poly: Vec<(f64, f64)>, nx: f64, ny: f64, d: f64) -> Vec<(f64, f64)> {
    if poly.is_empty() {
        return vec![];
    }
    let mut output = Vec::with_capacity(poly.len() + 1);
    let n = poly.len();

    for i in 0..n {
        let cur = poly[i];
        let nxt = poly[(i + 1) % n];

        let cur_inside = nx * cur.0 + ny * cur.1 + d >= 0.0;
        let nxt_inside = nx * nxt.0 + ny * nxt.1 + d >= 0.0;

        if cur_inside {
            output.push(cur);
            if !nxt_inside {
                output.push(intersect_edge(cur, nxt, nx, ny, d));
            }
        } else if nxt_inside {
            output.push(intersect_edge(cur, nxt, nx, ny, d));
        }
    }
    output
}

/// Line–halfplane intersection: point on segment `a`→`b` where
/// `nx*x + ny*y + d = 0`.
fn intersect_edge(a: (f64, f64), b: (f64, f64), nx: f64, ny: f64, d: f64) -> (f64, f64) {
    let fa = nx * a.0 + ny * a.1 + d;
    let fb = nx * b.0 + ny * b.1 + d;
    let denom = fa - fb;
    if denom.abs() < 1e-14 {
        return a;
    }
    let t = fa / denom;
    (a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1))
}

/// Clip a polygon against an axis-aligned bounding box.
pub(crate) fn clip_polygon_to_bbox(
    polygon: Vec<(f64, f64)>,
    bbox: (f64, f64, f64, f64),
) -> Vec<(f64, f64)> {
    let (min_x, min_y, max_x, max_y) = bbox;
    let mut poly = polygon;
    for &(nx, ny, d) in &[
        (1.0_f64, 0.0_f64, -min_x),
        (-1.0, 0.0, max_x),
        (0.0, 1.0, -min_y),
        (0.0, -1.0, max_y),
    ] {
        poly = sutherland_clip_one_edge(poly, nx, ny, d);
        if poly.is_empty() {
            return vec![];
        }
    }
    poly
}

// ---------------------------------------------------------------------------
// Core Voronoi cell computation via half-plane intersection
// ---------------------------------------------------------------------------

/// Build the bbox polygon (CCW).
fn bbox_polygon(bbox: (f64, f64, f64, f64)) -> Vec<(f64, f64)> {
    let (min_x, min_y, max_x, max_y) = bbox;
    vec![
        (min_x, min_y),
        (max_x, min_y),
        (max_x, max_y),
        (min_x, max_y),
    ]
}

/// Compute the Voronoi cell of seed `vi` by clipping the bbox polygon against
/// the perpendicular bisector half-planes for each other seed `vj`.
///
/// The half-plane "closer to vi than vj" is:
///   2*(vj−vi)·p ≤ ‖vj‖² − ‖vi‖²
/// i.e., `(2*(vjx−vix))*x + (2*(vjy−viy))*y + (vix²+viy² − vjx²−vjy²) >= 0`
fn voronoi_cell_by_halfplanes(
    vi: (f64, f64),
    seeds: &[VoronoiPoint],
    bbox: (f64, f64, f64, f64),
    skip: &[bool],
) -> Vec<(f64, f64)> {
    let mut poly = bbox_polygon(bbox);

    for (j, seed_j) in seeds.iter().enumerate() {
        if skip[j] {
            continue;
        }
        let vj = (seed_j.x, seed_j.y);
        if (vj.0 - vi.0).abs() < 1e-14 && (vj.1 - vi.1).abs() < 1e-14 {
            continue; // same point
        }

        // Half-plane: closer to vi than vj.
        // 2*(vjx-vix)*x + 2*(vjy-viy)*y <= vjx²+vjy² - vix²-viy²
        // Rewrite as: -2*(vjx-vix)*x - 2*(vjy-viy)*y + (vjx²+vjy² - vix²-viy²) >= 0
        let nx = -2.0 * (vj.0 - vi.0);
        let ny = -2.0 * (vj.1 - vi.1);
        let d = (vj.0 * vj.0 + vj.1 * vj.1) - (vi.0 * vi.0 + vi.1 * vi.1);

        poly = sutherland_clip_one_edge(poly, nx, ny, d);
        if poly.is_empty() {
            return vec![];
        }
    }

    poly
}

// ---------------------------------------------------------------------------
// Point-in-polygon (winding number)
// ---------------------------------------------------------------------------

/// Winding-number point-in-polygon test.
///
/// Returns `true` if `pt` is inside the polygon `poly`.
pub(crate) fn point_in_polygon_winding(pt: (f64, f64), poly: &[(f64, f64)]) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let (px, py) = pt;
    let mut winding = 0i32;
    for i in 0..n {
        let (ax, ay) = poly[i];
        let (bx, by) = poly[(i + 1) % n];
        if ay <= py {
            if by > py {
                let cross = (bx - ax) * (py - ay) - (px - ax) * (by - ay);
                if cross > 0.0 {
                    winding += 1;
                }
            }
        } else if by <= py {
            let cross = (bx - ax) * (py - ay) - (px - ax) * (by - ay);
            if cross < 0.0 {
                winding -= 1;
            }
        }
    }
    winding != 0
}

// ---------------------------------------------------------------------------
// Special cases
// ---------------------------------------------------------------------------

/// Build cells for the 2-point case.
fn two_point_voronoi(pts: &[VoronoiPoint], bbox: (f64, f64, f64, f64)) -> Vec<VoronoiCell> {
    debug_assert_eq!(pts.len(), 2);
    let skip = vec![false; 2];
    let mut v0 = voronoi_cell_by_halfplanes((pts[0].x, pts[0].y), pts, bbox, &[false, false]);
    let mut v1 = voronoi_cell_by_halfplanes((pts[1].x, pts[1].y), pts, bbox, &[false, false]);
    let _ = skip;
    ensure_ccw(&mut v0);
    ensure_ccw(&mut v1);
    vec![
        VoronoiCell {
            seed: pts[0].clone(),
            vertices: v0,
            neighbors: vec![1],
        },
        VoronoiCell {
            seed: pts[1].clone(),
            vertices: v1,
            neighbors: vec![0],
        },
    ]
}

/// Build slab cells for collinear inputs (all seeds on a line).
fn collinear_voronoi(pts: &[VoronoiPoint], bbox: (f64, f64, f64, f64)) -> Vec<VoronoiCell> {
    let n = pts.len();

    let dx = pts[n - 1].x - pts[0].x;
    let dy = pts[n - 1].y - pts[0].y;
    let len = (dx * dx + dy * dy).sqrt().max(1e-14);
    let ux = dx / len;
    let uy = dy / len;
    let nx = -uy;
    let ny = ux;

    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| {
        let ta = pts[a].x * ux + pts[a].y * uy;
        let tb = pts[b].x * ux + pts[b].y * uy;
        ta.partial_cmp(&tb).unwrap_or(core::cmp::Ordering::Equal)
    });

    let full = bbox_polygon(bbox);
    let mut cells_by_rank: Vec<VoronoiCell> = Vec::with_capacity(n);

    for rank in 0..n {
        let idx = order[rank];
        let mut poly = full.clone();

        if rank + 1 < n {
            let next_idx = order[rank + 1];
            let bmx = (pts[idx].x + pts[next_idx].x) * 0.5;
            let bmy = (pts[idx].y + pts[next_idx].y) * 0.5;
            let rhs = nx * bmx + ny * bmy;
            poly = sutherland_clip_one_edge(poly, -nx, -ny, rhs);
        }
        if rank > 0 {
            let prev_idx = order[rank - 1];
            let bmx = (pts[idx].x + pts[prev_idx].x) * 0.5;
            let bmy = (pts[idx].y + pts[prev_idx].y) * 0.5;
            let rhs = nx * bmx + ny * bmy;
            poly = sutherland_clip_one_edge(poly, nx, ny, -rhs);
        }

        let mut poly = clip_polygon_to_bbox(poly, bbox);
        ensure_ccw(&mut poly);

        let mut neighbors = Vec::new();
        if rank > 0 {
            neighbors.push(order[rank - 1]);
        }
        if rank + 1 < n {
            neighbors.push(order[rank + 1]);
        }

        cells_by_rank.push(VoronoiCell {
            seed: pts[idx].clone(),
            vertices: poly,
            neighbors,
        });
    }

    let mut result: Vec<Option<VoronoiCell>> = (0..n).map(|_| None).collect();
    for (rank, &idx) in order.iter().enumerate() {
        result[idx] = Some(cells_by_rank[rank].clone());
    }
    result.into_iter().flatten().collect()
}

// ---------------------------------------------------------------------------
// Main public API
// ---------------------------------------------------------------------------

/// Build the Voronoi diagram for `points` clipped to `bbox`.
///
/// Uses half-plane intersection (Sutherland–Hodgman) for correctness, and
/// the Delaunay triangulation to extract neighbor information.
///
/// # Errors
///
/// Returns an error if the bounding box is degenerate (zero width or height).
pub fn build_voronoi(
    points: &[VoronoiPoint],
    bbox: (f64, f64, f64, f64),
) -> Result<VoronoiDiagram, IndexError> {
    let (min_x, min_y, max_x, max_y) = bbox;
    if max_x <= min_x || max_y <= min_y {
        return Err(IndexError::InvalidBbox(
            "Voronoi bbox must have positive width and height".into(),
        ));
    }

    match points.len() {
        0 => {
            return Ok(VoronoiDiagram {
                cells: vec![],
                bbox,
            });
        }
        1 => {
            let cell = VoronoiCell {
                seed: points[0].clone(),
                vertices: bbox_polygon(bbox),
                neighbors: vec![],
            };
            return Ok(VoronoiDiagram {
                cells: vec![cell],
                bbox,
            });
        }
        2 => {
            let cells = two_point_voronoi(points, bbox);
            return Ok(VoronoiDiagram { cells, bbox });
        }
        _ => {}
    }

    // Build the Delaunay triangulation for neighbor information.
    let del_pts: Vec<delaunator::Point> = points
        .iter()
        .map(|p| delaunator::Point { x: p.x, y: p.y })
        .collect();

    let tri = delaunator::triangulate(&del_pts);

    // Collinear degenerate case.
    if tri.triangles.is_empty() {
        let cells = collinear_voronoi(points, bbox);
        return Ok(VoronoiDiagram { cells, bbox });
    }

    let n_points = points.len();
    let n_triangles = tri.triangles.len() / 3;

    // Build Delaunay neighbor sets.
    let mut neighbor_sets: Vec<std::collections::BTreeSet<usize>> =
        vec![std::collections::BTreeSet::new(); n_points];
    for t in 0..n_triangles {
        let a = tri.triangles[3 * t];
        let b = tri.triangles[3 * t + 1];
        let c = tri.triangles[3 * t + 2];
        neighbor_sets[a].insert(b);
        neighbor_sets[a].insert(c);
        neighbor_sets[b].insert(a);
        neighbor_sets[b].insert(c);
        neighbor_sets[c].insert(a);
        neighbor_sets[c].insert(b);
    }

    // Build cells using half-plane intersection.
    // For efficiency, we only clip against Delaunay neighbors (which are the
    // only seeds that can define a Voronoi edge for this cell).  We mark all
    // non-neighbors as "skip" to avoid needless clipping.
    let mut cells: Vec<VoronoiCell> = Vec::with_capacity(n_points);

    for p_idx in 0..n_points {
        let vi = (points[p_idx].x, points[p_idx].y);
        let neighbors: Vec<usize> = neighbor_sets[p_idx].iter().copied().collect();

        // Build a skip mask: skip all seeds that are not Delaunay neighbours
        // and not the seed itself.
        let mut skip = vec![true; n_points];
        skip[p_idx] = true; // skip self
        for &nb in &neighbors {
            skip[nb] = false; // include Delaunay neighbours
        }

        let mut poly = voronoi_cell_by_halfplanes(vi, points, bbox, &skip);
        ensure_ccw(&mut poly);

        cells.push(VoronoiCell {
            seed: points[p_idx].clone(),
            vertices: poly,
            neighbors,
        });
    }

    Ok(VoronoiDiagram { cells, bbox })
}

// ---------------------------------------------------------------------------
// Query helpers
// ---------------------------------------------------------------------------

/// Find which Voronoi cell contains the query point.
///
/// Performs a linear scan over all cells using winding-number point-in-polygon.
/// Returns `None` if the query is outside all cells (e.g. outside the bbox).
pub fn find_cell_containing(diagram: &VoronoiDiagram, query: (f64, f64)) -> Option<usize> {
    let (min_x, min_y, max_x, max_y) = diagram.bbox;
    if query.0 < min_x || query.0 > max_x || query.1 < min_y || query.1 > max_y {
        return None;
    }

    for (i, cell) in diagram.cells.iter().enumerate() {
        if point_in_polygon_winding(query, &cell.vertices) {
            return Some(i);
        }
    }
    None
}

/// Compute the unsigned area of each Voronoi cell using the shoelace formula.
pub fn cell_areas(diagram: &VoronoiDiagram) -> Vec<f64> {
    diagram
        .cells
        .iter()
        .map(|c| signed_area(&c.vertices).abs())
        .collect()
}
