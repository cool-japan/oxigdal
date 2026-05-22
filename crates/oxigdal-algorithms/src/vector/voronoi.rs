//! Voronoi diagram generation
//!
//! Compute Voronoi diagrams (Thiessen polygons) from point sets.

use crate::error::{AlgorithmError, Result};
use oxigdal_core::vector::{Coordinate, LineString, Point, Polygon};

/// Options for Voronoi diagram generation
#[derive(Debug, Clone, Default)]
pub struct VoronoiOptions {
    /// Bounding box for clipping diagram
    pub bounds: Option<(f64, f64, f64, f64)>, // (min_x, min_y, max_x, max_y)
    /// Whether to include infinite cells
    pub include_infinite: bool,
}

/// A Voronoi cell (polygon)
#[derive(Debug, Clone)]
pub struct VoronoiCell {
    /// Site (generating point) for this cell
    pub site: Point,
    /// Site index
    pub site_index: usize,
    /// Polygon representing the cell
    pub polygon: Option<Polygon>,
    /// Whether this is an infinite cell
    pub is_infinite: bool,
}

/// Voronoi diagram result
#[derive(Debug, Clone)]
pub struct VoronoiDiagram {
    /// Voronoi cells
    pub cells: Vec<VoronoiCell>,
    /// Number of sites
    pub num_sites: usize,
}

/// Generate Voronoi diagram from points
///
/// # Arguments
///
/// * `points` - Input points (sites)
/// * `options` - Voronoi options
///
/// # Returns
///
/// Voronoi diagram with cells for each point
///
/// # Examples
///
/// ```
/// use oxigdal_algorithms::vector::voronoi::{voronoi_diagram, VoronoiOptions};
/// use oxigdal_algorithms::Point;
/// # use oxigdal_algorithms::error::Result;
///
/// # fn main() -> Result<()> {
/// let points = vec![
///     Point::new(0.0, 0.0),
///     Point::new(5.0, 0.0),
///     Point::new(2.5, 5.0),
/// ];
///
/// let options = VoronoiOptions {
///     bounds: Some((0.0, 0.0, 10.0, 10.0)),
///     include_infinite: false,
/// };
///
/// let diagram = voronoi_diagram(&points, &options)?;
/// assert_eq!(diagram.num_sites, 3);
/// # Ok(())
/// # }
/// ```
pub fn voronoi_diagram(points: &[Point], options: &VoronoiOptions) -> Result<VoronoiDiagram> {
    if points.len() < 3 {
        return Err(AlgorithmError::InvalidInput(
            "Need at least 3 points for Voronoi diagram".to_string(),
        ));
    }

    // Convert points to delaunator format
    let delaunator_points: Vec<delaunator::Point> = points
        .iter()
        .map(|p| delaunator::Point {
            x: p.coord.x,
            y: p.coord.y,
        })
        .collect();

    // Compute Delaunay triangulation
    let delaunay = delaunator::triangulate(&delaunator_points);

    // Build Voronoi cells from dual graph
    let mut cells = Vec::new();

    for (site_idx, point) in points.iter().enumerate() {
        let cell = build_voronoi_cell(site_idx, point, &delaunay, points, options)?;
        cells.push(cell);
    }

    Ok(VoronoiDiagram {
        cells,
        num_sites: points.len(),
    })
}

/// Build a Voronoi cell for a site
fn build_voronoi_cell(
    site_idx: usize,
    site: &Point,
    delaunay: &delaunator::Triangulation,
    points: &[Point],
    options: &VoronoiOptions,
) -> Result<VoronoiCell> {
    // Find all triangles containing this site
    let mut cell_vertices = Vec::new();
    let mut is_infinite = false;

    // Compute circumcenters of triangles
    for tri_idx in 0..(delaunay.triangles.len() / 3) {
        let a = delaunay.triangles[tri_idx * 3];
        let b = delaunay.triangles[tri_idx * 3 + 1];
        let c = delaunay.triangles[tri_idx * 3 + 2];

        if a == site_idx || b == site_idx || c == site_idx {
            // This triangle contains our site
            let pa = &points[a];
            let pb = &points[b];
            let pc = &points[c];

            let circumcenter = compute_circumcenter(
                pa.coord.x, pa.coord.y, pb.coord.x, pb.coord.y, pc.coord.x, pc.coord.y,
            )?;

            cell_vertices.push(circumcenter);
        }
    }

    // Check if cell is bounded
    if let Some((min_x, min_y, max_x, max_y)) = options.bounds {
        // Clip vertices to bounds
        cell_vertices.retain(|coord| {
            coord.x >= min_x && coord.x <= max_x && coord.y >= min_y && coord.y <= max_y
        });

        is_infinite = cell_vertices.len() < 3;
    }

    // Create polygon from vertices
    let polygon =
        if cell_vertices.len() >= 3 {
            // Sort vertices by angle around site
            cell_vertices.sort_by(|a, b| {
                let angle_a = (a.y - site.coord.y).atan2(a.x - site.coord.x);
                let angle_b = (b.y - site.coord.y).atan2(b.x - site.coord.x);
                angle_a
                    .partial_cmp(&angle_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

            // Close the ring
            if let Some(first) = cell_vertices.first().copied() {
                cell_vertices.push(first);
            }

            // Create polygon
            let exterior = LineString::new(cell_vertices.clone()).map_err(|e| {
                AlgorithmError::InvalidGeometry(format!("Invalid cell exterior: {}", e))
            })?;
            Some(Polygon::new(exterior, vec![]).map_err(|e| {
                AlgorithmError::InvalidGeometry(format!("Invalid cell polygon: {}", e))
            })?)
        } else {
            None
        };

    Ok(VoronoiCell {
        site: site.clone(),
        site_index: site_idx,
        polygon,
        is_infinite,
    })
}

/// Compute circumcenter of a triangle
fn compute_circumcenter(
    ax: f64,
    ay: f64,
    bx: f64,
    by: f64,
    cx: f64,
    cy: f64,
) -> Result<Coordinate> {
    let d = 2.0 * (ax * (by - cy) + bx * (cy - ay) + cx * (ay - by));

    if d.abs() < 1e-10 {
        return Err(AlgorithmError::ComputationError(
            "Degenerate triangle".to_string(),
        ));
    }

    let ux = ((ax * ax + ay * ay) * (by - cy)
        + (bx * bx + by * by) * (cy - ay)
        + (cx * cx + cy * cy) * (ay - by))
        / d;
    let uy = ((ax * ax + ay * ay) * (cx - bx)
        + (bx * bx + by * by) * (ax - cx)
        + (cx * cx + cy * cy) * (bx - ax))
        / d;

    Ok(Coordinate::new_2d(ux, uy))
}

// ============================================================================
// Power Diagram (Weighted Voronoi) — Aurenhammer 1987
// ============================================================================
//
// A power diagram generalises the Voronoi diagram: each site p_i has a weight
// w_i.  The *power distance* from a query point x to site i is:
//
//   pd(x, p_i) = |x - p_i|² − w_i
//
// The power cell of site i is the set of all x for which pd(x, p_i) is
// minimal.  When all weights are equal the diagram reduces to the standard
// Voronoi diagram.
//
// Implementation: half-plane intersection via Sutherland–Hodgman clipping.
// Each cell starts as the bounding rectangle and is successively clipped by
// the radical-axis half-plane for every other site.  Time: O(N² · V) where V
// is the vertex count of the intermediate polygon (at most O(N)).

/// A weighted site for power diagram construction.
///
/// The `weight` field acts as the squared radius of an influence circle centred
/// at `point`.  A larger weight enlarges the power cell of that site.
#[derive(Debug, Clone)]
pub struct WeightedPoint {
    /// The geometric location of the site.
    pub point: Point,
    /// Power weight (equivalent to squared circle radius).  Use 0.0 for
    /// unweighted sites — the result is then a standard Voronoi cell.
    pub weight: f64,
}

impl WeightedPoint {
    /// Create a new weighted site.
    pub fn new(x: f64, y: f64, weight: f64) -> Self {
        Self {
            point: Point::new(x, y),
            weight,
        }
    }

    /// Create a site with weight 0.0 (equivalent to a standard Voronoi site).
    pub fn unweighted(x: f64, y: f64) -> Self {
        Self::new(x, y, 0.0)
    }
}

/// One cell of a power diagram.
#[derive(Debug, Clone)]
pub struct PowerCell {
    /// Index of the generating site in the input slice.
    pub site_index: usize,
    /// Vertices of the cell polygon in counter-clockwise order.
    /// Empty when the site is dominated by its neighbours (see `is_empty`).
    pub polygon: Vec<Coordinate>,
    /// `true` when every point in the bounding box is closer (in power
    /// distance) to some other site, so this site's cell has zero area.
    pub is_empty: bool,
}

/// The complete result of `power_diagram`.
#[derive(Debug, Clone)]
pub struct PowerDiagram {
    /// One `PowerCell` per input site, in the same order as the input slice.
    pub cells: Vec<PowerCell>,
}

/// Options controlling `power_diagram`.
#[derive(Debug, Clone)]
pub struct PowerDiagramOptions {
    /// Explicit clipping rectangle `(min_x, min_y, max_x, max_y)`.
    ///
    /// If `None` the bounding box is derived from the input point extents
    /// expanded by 50 % on each side (plus an absolute minimum padding of
    /// 1.0 unit so single-point inputs get a sensible box).
    pub bounding_box: Option<(f64, f64, f64, f64)>,
}

impl Default for PowerDiagramOptions {
    fn default() -> Self {
        Self { bounding_box: None }
    }
}

/// Compute a power diagram (weighted Voronoi diagram) from a set of weighted
/// point sites.
///
/// # Algorithm
///
/// For each site `i` the cell is constructed by starting with the bounding
/// rectangle and successively clipping it with the radical-axis half-plane
/// defined by the pair `(i, j)` for every other site `j ≠ i`.  The radical
/// axis is the locus of points equidistant (in power distance) from both
/// sites:
///
/// ```text
/// 2(xj−xi)·x + 2(yj−yi)·y = (xj²+yj²−wj) − (xi²+yi²−wi)
/// ```
///
/// Points satisfying `a·x + b·y ≤ c` belong to site `i`'s side.
///
/// # Performance
///
/// O(N² · V) where V ≤ N+4 is the polygon vertex count.  Suitable for
/// N < 1 000 sites; for larger inputs consider a fortune-sweep approach.
///
/// # Examples
///
/// ```
/// use oxigdal_algorithms::{power_diagram, WeightedPoint, PowerDiagramOptions};
///
/// let sites = vec![
///     WeightedPoint::new(0.0, 0.0, 0.0),
///     WeightedPoint::new(4.0, 0.0, 0.0),
/// ];
/// let opts = PowerDiagramOptions {
///     bounding_box: Some((-1.0, -1.0, 5.0, 1.0)),
/// };
/// let diagram = power_diagram(&sites, &opts).unwrap();
/// assert_eq!(diagram.cells.len(), 2);
/// ```
pub fn power_diagram(
    weighted_points: &[WeightedPoint],
    options: &PowerDiagramOptions,
) -> crate::error::Result<PowerDiagram> {
    if weighted_points.is_empty() {
        return Ok(PowerDiagram { cells: vec![] });
    }

    let bbox = compute_power_bbox(weighted_points, options);

    if weighted_points.len() == 1 {
        // Single site: the entire bounding box is the cell.
        let polygon = bbox_to_polygon(bbox);
        return Ok(PowerDiagram {
            cells: vec![PowerCell {
                site_index: 0,
                polygon,
                is_empty: false,
            }],
        });
    }

    let mut cells = Vec::with_capacity(weighted_points.len());

    for i in 0..weighted_points.len() {
        let pi = &weighted_points[i];
        // Start each cell as the full bounding box.
        let mut polygon = bbox_to_polygon(bbox);

        for (j, pj) in weighted_points.iter().enumerate() {
            if i == j || polygon.is_empty() {
                continue;
            }
            // weighted_bisector returns (a,b,c) where a·x+b·y ≤ c is site i's
            // side.  Negate to get the ≥ form that half_plane_clip expects.
            let (a, b, c) = weighted_bisector(
                pi.point.coord.x,
                pi.point.coord.y,
                pi.weight,
                pj.point.coord.x,
                pj.point.coord.y,
                pj.weight,
            );
            polygon = half_plane_clip(&polygon, -a, -b, -c);
        }

        let is_empty = polygon.is_empty();
        cells.push(PowerCell {
            site_index: i,
            polygon,
            is_empty,
        });
    }

    Ok(PowerDiagram { cells })
}

// ----------------------------------------------------------------------------
// Public helper — useful for testing and for custom diagram implementations.
// ----------------------------------------------------------------------------

/// Compute the radical-axis half-plane coefficients `(a, b, c)` such that
/// `a·x + b·y ≤ c` selects the region closer (in power distance) to site
/// `(xi, yi, wi)` than to site `(xj, yj, wj)`.
///
/// Derivation of the radical axis (bisector locus):
/// ```text
///   pd(x, pi) ≤ pd(x, pj)
///   ⟺  |x−pi|² − wi  ≤  |x−pj|² − wj
///   ⟺  −2xi·x + xi² − 2yi·y + yi² − wi  ≤  −2xj·x + xj² − 2yj·y + yj² − wj
///   ⟺  2(xj−xi)·x + 2(yj−yi)·y  ≤  (xj²+yj²−wj) − (xi²+yi²−wi)
///   ⟺  a·x + b·y  ≤  c
/// ```
///
/// The bisector (radical axis) itself is `a·x + b·y = c`.
/// The `≤` side contains site `i`; the `≥` side contains site `j`.
///
/// # Note
///
/// Call `half_plane_clip` with `(-a, -b, -c)` to use the standard `≥` form,
/// or use `weighted_bisector_ge` which returns negated coefficients directly.
pub fn weighted_bisector(xi: f64, yi: f64, wi: f64, xj: f64, yj: f64, wj: f64) -> (f64, f64, f64) {
    let a = 2.0 * (xj - xi);
    let b = 2.0 * (yj - yi);
    // RHS of the radical-axis inequality (a·x + b·y ≤ c keeps pi's side)
    let c = (xj * xj + yj * yj - wj) - (xi * xi + yi * yi - wi);
    (a, b, c)
}

// ----------------------------------------------------------------------------
// Private helpers
// ----------------------------------------------------------------------------

/// Derive a bounding box from the input sites, adding 50 % padding plus a
/// minimum absolute pad of 1.0 unit on each axis, unless an explicit box was
/// supplied in `options`.
fn compute_power_bbox(
    points: &[WeightedPoint],
    options: &PowerDiagramOptions,
) -> (f64, f64, f64, f64) {
    if let Some(bbox) = options.bounding_box {
        return bbox;
    }
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for wp in points {
        let x = wp.point.coord.x;
        let y = wp.point.coord.y;
        if x < min_x {
            min_x = x;
        }
        if x > max_x {
            max_x = x;
        }
        if y < min_y {
            min_y = y;
        }
        if y > max_y {
            max_y = y;
        }
    }
    let pad_x = (max_x - min_x) * 0.5 + 1.0;
    let pad_y = (max_y - min_y) * 0.5 + 1.0;
    (min_x - pad_x, min_y - pad_y, max_x + pad_x, max_y + pad_y)
}

/// Convert a bounding box to a CCW polygon (four corners, no closing vertex).
fn bbox_to_polygon(bbox: (f64, f64, f64, f64)) -> Vec<Coordinate> {
    let (min_x, min_y, max_x, max_y) = bbox;
    vec![
        Coordinate::new_2d(min_x, min_y),
        Coordinate::new_2d(max_x, min_y),
        Coordinate::new_2d(max_x, max_y),
        Coordinate::new_2d(min_x, max_y),
    ]
}

/// Clip a convex polygon against the half-plane `a·x + b·y ≥ c` using
/// the Sutherland–Hodgman algorithm.
///
/// Returns the vertices of the clipped polygon (counter-clockwise).
/// Returns an empty `Vec` if the polygon is entirely outside the half-plane.
fn half_plane_clip(polygon: &[Coordinate], a: f64, b: f64, c: f64) -> Vec<Coordinate> {
    if polygon.is_empty() {
        return vec![];
    }

    let inside = |p: &Coordinate| a * p.x + b * p.y >= c;

    let n = polygon.len();
    let mut output: Vec<Coordinate> = Vec::with_capacity(n + 1);

    for i in 0..n {
        let curr = &polygon[i];
        let next = &polygon[(i + 1) % n];
        let curr_in = inside(curr);
        let next_in = inside(next);

        if curr_in {
            output.push(*curr);
        }
        if curr_in != next_in {
            // Compute the parametric intersection point.
            let t = intersect_segment_with_halfplane(curr, next, a, b, c);
            output.push(Coordinate::new_2d(
                curr.x + t * (next.x - curr.x),
                curr.y + t * (next.y - curr.y),
            ));
        }
    }
    output
}

/// Return the parametric `t ∈ [0, 1]` at which the segment `[p1, p2]`
/// crosses the plane `a·x + b·y = c`.
///
/// Falls back to `0.5` for degenerate (parallel) cases that should not
/// occur in well-formed input.
fn intersect_segment_with_halfplane(
    p1: &Coordinate,
    p2: &Coordinate,
    a: f64,
    b: f64,
    c: f64,
) -> f64 {
    let d1 = a * p1.x + b * p1.y - c;
    let d2 = a * p2.x + b * p2.y - c;
    let denom = d1 - d2;
    if denom.abs() < f64::EPSILON {
        return 0.5;
    }
    d1 / denom
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_voronoi_simple() {
        let points = vec![
            Point::new(0.0, 0.0),
            Point::new(5.0, 0.0),
            Point::new(2.5, 5.0),
        ];

        let options = VoronoiOptions {
            bounds: Some((0.0, 0.0, 10.0, 10.0)),
            include_infinite: false,
        };

        let result = voronoi_diagram(&points, &options);
        assert!(result.is_ok());

        let diagram = result.expect("Voronoi failed");
        assert_eq!(diagram.num_sites, 3);
    }

    #[test]
    fn test_circumcenter() {
        let result = compute_circumcenter(0.0, 0.0, 1.0, 0.0, 0.0, 1.0);
        assert!(result.is_ok());

        let center = result.expect("Failed to compute circumcenter");
        assert!((center.x - 0.5).abs() < 1e-6);
        assert!((center.y - 0.5).abs() < 1e-6);
    }
}
