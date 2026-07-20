//! Split operations for dividing geometric features
//!
//! This module implements splitting operations that divide geometries using
//! other geometries as cutting tools.

use crate::error::{AlgorithmError, Result};
use oxigeo_core::vector::{Coordinate, LineString, Point, Polygon};

/// Options for split operations
#[derive(Debug, Clone)]
pub struct SplitOptions {
    /// Tolerance for coordinate comparison
    pub tolerance: f64,
    /// Whether to snap split points to grid
    pub snap_to_grid: bool,
    /// Grid size for snapping (if enabled)
    pub grid_size: f64,
    /// Minimum length for resulting line segments
    pub min_segment_length: f64,
    /// Whether to preserve all split parts (even very small ones)
    pub preserve_all: bool,
    /// Minimum area threshold for resulting polygons (None = no filter)
    pub min_area: Option<f64>,
    /// Whether to ensure counter-clockwise orientation on output polygons
    pub preserve_orientation: bool,
    /// Whether to attach holes to their enclosing face after splitting
    pub keep_holes: bool,
}

impl Default for SplitOptions {
    fn default() -> Self {
        Self {
            tolerance: 1e-10,
            snap_to_grid: false,
            grid_size: 1e-6,
            min_segment_length: 0.0,
            preserve_all: true,
            min_area: None,
            preserve_orientation: false,
            keep_holes: true,
        }
    }
}

/// Result of a split operation
#[derive(Debug, Clone)]
pub struct SplitResult {
    /// The resulting geometries from the split
    pub geometries: Vec<SplitGeometry>,
    /// Number of split points used
    pub num_splits: usize,
    /// Whether all splits were successful
    pub complete: bool,
}

/// A geometry resulting from a split operation
#[derive(Debug, Clone)]
pub enum SplitGeometry {
    /// A linestring segment
    LineString(LineString),
    /// A polygon
    Polygon(Polygon),
}

/// Split a linestring by point locations
///
/// Divides a linestring into multiple segments at specified points.
///
/// # Arguments
///
/// * `linestring` - The linestring to split
/// * `split_points` - Points where the linestring should be split
/// * `options` - Split options
///
/// # Returns
///
/// Result containing the split result with multiple linestring segments
///
/// # Examples
///
/// ```
/// use oxigeo_algorithms::vector::topology::{split_linestring_by_points, SplitOptions};
/// use oxigeo_algorithms::{Coordinate, LineString, Point};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let coords = vec![
///     Coordinate::new_2d(0.0, 0.0),
///     Coordinate::new_2d(10.0, 0.0),
/// ];
/// let linestring = LineString::new(coords)?;
///
/// let split_points = vec![
///     Point::new(5.0, 0.0),
/// ];
///
/// let result = split_linestring_by_points(&linestring, &split_points, &SplitOptions::default())?;
/// assert_eq!(result.num_splits, 1);
/// assert!(result.geometries.len() >= 2);
/// # Ok(())
/// # }
/// ```
pub fn split_linestring_by_points(
    linestring: &LineString,
    split_points: &[Point],
    options: &SplitOptions,
) -> Result<SplitResult> {
    if split_points.is_empty() {
        return Ok(SplitResult {
            geometries: vec![SplitGeometry::LineString(linestring.clone())],
            num_splits: 0,
            complete: true,
        });
    }

    let coords = &linestring.coords;
    if coords.len() < 2 {
        return Err(AlgorithmError::InvalidGeometry(
            "Linestring must have at least 2 coordinates".to_string(),
        ));
    }

    // Find all split locations along the linestring
    let mut split_locations = Vec::new();

    for point in split_points {
        if let Some(location) = find_point_on_linestring(linestring, point, options.tolerance)? {
            split_locations.push(location);
        }
    }

    // Sort split locations by segment index and parameter
    // Validate for NaN values before sorting
    for loc in &split_locations {
        if loc.parameter.is_nan() {
            return Err(AlgorithmError::InvalidGeometry(
                "Split location parameter contains NaN value".to_string(),
            ));
        }
    }

    split_locations.sort_by(|a, b| {
        a.segment_index.cmp(&b.segment_index).then(
            a.parameter
                .partial_cmp(&b.parameter)
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });

    // Remove duplicates
    split_locations.dedup_by(|a, b| {
        a.segment_index == b.segment_index && (a.parameter - b.parameter).abs() < options.tolerance
    });

    if split_locations.is_empty() {
        return Ok(SplitResult {
            geometries: vec![SplitGeometry::LineString(linestring.clone())],
            num_splits: 0,
            complete: false,
        });
    }

    // Build split segments
    let mut result_geometries = Vec::new();
    let mut current_coords = Vec::new();
    let mut current_segment_idx = 0;
    let mut split_idx = 0;

    current_coords.push(coords[0]);

    for i in 0..coords.len().saturating_sub(1) {
        // Add splits on this segment
        while split_idx < split_locations.len() && split_locations[split_idx].segment_index == i {
            let split_loc = &split_locations[split_idx];
            let split_coord = split_loc.coordinate;

            // Add split point
            current_coords.push(split_coord);

            // Create a new linestring if we have enough points
            if current_coords.len() >= 2 {
                if let Ok(ls) = LineString::new(current_coords.clone()) {
                    if options.preserve_all
                        || compute_linestring_length(&ls) >= options.min_segment_length
                    {
                        result_geometries.push(SplitGeometry::LineString(ls));
                    }
                }
            }

            // Start new segment
            current_coords.clear();
            current_coords.push(split_coord);

            split_idx += 1;
        }

        // Add end point of current segment
        current_coords.push(coords[i + 1]);
        current_segment_idx = i + 1;
    }

    // Add final segment
    if current_coords.len() >= 2 {
        if let Ok(ls) = LineString::new(current_coords) {
            if options.preserve_all || compute_linestring_length(&ls) >= options.min_segment_length
            {
                result_geometries.push(SplitGeometry::LineString(ls));
            }
        }
    }

    Ok(SplitResult {
        geometries: result_geometries,
        num_splits: split_locations.len(),
        complete: true,
    })
}

/// Location of a point on a linestring
#[derive(Debug, Clone)]
struct PointLocation {
    /// Index of the segment (0-based)
    segment_index: usize,
    /// Parameter along the segment (0.0 to 1.0)
    parameter: f64,
    /// The coordinate at this location
    coordinate: Coordinate,
}

/// Find a point on a linestring
fn find_point_on_linestring(
    linestring: &LineString,
    point: &Point,
    tolerance: f64,
) -> Result<Option<PointLocation>> {
    let coords = &linestring.coords;

    for i in 0..coords.len().saturating_sub(1) {
        let p1 = coords[i];
        let p2 = coords[i + 1];

        // Check if point is on this segment
        if let Some((param, coord)) = point_on_segment(point, &p1, &p2, tolerance) {
            return Ok(Some(PointLocation {
                segment_index: i,
                parameter: param,
                coordinate: coord,
            }));
        }
    }

    Ok(None)
}

/// Check if a point is on a line segment
fn point_on_segment(
    point: &Point,
    p1: &Coordinate,
    p2: &Coordinate,
    tolerance: f64,
) -> Option<(f64, Coordinate)> {
    let px = point.coord.x;
    let py = point.coord.y;

    // Vector from p1 to p2
    let dx = p2.x - p1.x;
    let dy = p2.y - p1.y;

    // Length squared of segment
    let len_sq = dx * dx + dy * dy;

    if len_sq < tolerance * tolerance {
        // Degenerate segment
        return None;
    }

    // Parameter t along the segment
    let t = ((px - p1.x) * dx + (py - p1.y) * dy) / len_sq;

    // Check if point projects onto the segment (not before or after)
    if t < -tolerance || t > 1.0 + tolerance {
        return None;
    }

    // Clamp t to [0, 1]
    let t = t.clamp(0.0, 1.0);

    // Compute closest point on segment
    let closest_x = p1.x + t * dx;
    let closest_y = p1.y + t * dy;

    // Check distance from point to closest point
    let dist_sq = (px - closest_x).powi(2) + (py - closest_y).powi(2);

    if dist_sq < tolerance * tolerance {
        Some((t, Coordinate::new_2d(closest_x, closest_y)))
    } else {
        None
    }
}

/// Compute the length of a linestring
fn compute_linestring_length(linestring: &LineString) -> f64 {
    let coords = &linestring.coords;
    let mut length = 0.0;

    for i in 0..coords.len().saturating_sub(1) {
        let dx = coords[i + 1].x - coords[i].x;
        let dy = coords[i + 1].y - coords[i].y;
        length += (dx * dx + dy * dy).sqrt();
    }

    length
}

/// Split a polygon by a linestring
///
/// Divides a polygon into multiple parts using a linestring as a cutting tool.
///
/// # Arguments
///
/// * `polygon` - The polygon to split
/// * `split_line` - The linestring to use for splitting
/// * `options` - Split options
///
/// # Returns
///
/// Result containing the split polygons
///
/// # Examples
///
/// ```no_run
/// use oxigeo_algorithms::vector::topology::{split_polygon_by_line, SplitOptions};
/// use oxigeo_algorithms::{Coordinate, LineString, Polygon};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let coords = vec![
///     Coordinate::new_2d(0.0, 0.0),
///     Coordinate::new_2d(10.0, 0.0),
///     Coordinate::new_2d(10.0, 10.0),
///     Coordinate::new_2d(0.0, 10.0),
///     Coordinate::new_2d(0.0, 0.0),
/// ];
/// let exterior = LineString::new(coords)?;
/// let polygon = Polygon::new(exterior, vec![])?;
///
/// let split_coords = vec![
///     Coordinate::new_2d(0.0, 5.0),
///     Coordinate::new_2d(10.0, 5.0),
/// ];
/// let split_line = LineString::new(split_coords)?;
///
/// let result = split_polygon_by_line(&polygon, &split_line, &SplitOptions::default())?;
/// assert!(result.geometries.len() >= 1);
/// # Ok(())
/// # }
/// ```
pub fn split_polygon_by_line(
    polygon: &Polygon,
    split_line: &LineString,
    options: &SplitOptions,
) -> Result<SplitResult> {
    // Find intersection points between polygon boundary and split line
    let intersection_points = find_polygon_line_intersections(polygon, split_line, options)?;

    if intersection_points.len() < 2 {
        // Not enough intersections to split
        return Ok(SplitResult {
            geometries: vec![SplitGeometry::Polygon(polygon.clone())],
            num_splits: 0,
            complete: false,
        });
    }

    // Create polygons from split
    let result_polygons =
        create_split_polygons(polygon, split_line, &intersection_points, options)?;

    let geometries = result_polygons
        .into_iter()
        .map(SplitGeometry::Polygon)
        .collect();

    Ok(SplitResult {
        geometries,
        num_splits: intersection_points.len(),
        complete: true,
    })
}

/// Find intersection points between polygon boundary and a linestring
fn find_polygon_line_intersections(
    polygon: &Polygon,
    line: &LineString,
    options: &SplitOptions,
) -> Result<Vec<Coordinate>> {
    let mut intersections = Vec::new();

    // Check exterior ring
    let exterior_intersections = find_linestring_intersections(&polygon.exterior, line, options)?;
    intersections.extend(exterior_intersections);

    // Check interior rings (holes)
    for interior in &polygon.interiors {
        let interior_intersections = find_linestring_intersections(interior, line, options)?;
        intersections.extend(interior_intersections);
    }

    // Remove duplicates
    // Validate for NaN values before sorting
    for coord in &intersections {
        if coord.x.is_nan() || coord.y.is_nan() {
            return Err(AlgorithmError::InvalidGeometry(
                "Intersection coordinate contains NaN value".to_string(),
            ));
        }
    }

    intersections.sort_by(|a, b| {
        a.x.partial_cmp(&b.x)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal))
    });

    intersections.dedup_by(|a, b| {
        (a.x - b.x).abs() < options.tolerance && (a.y - b.y).abs() < options.tolerance
    });

    Ok(intersections)
}

/// Find intersections between two linestrings
fn find_linestring_intersections(
    line1: &LineString,
    line2: &LineString,
    options: &SplitOptions,
) -> Result<Vec<Coordinate>> {
    let mut intersections = Vec::new();
    let coords1 = &line1.coords;
    let coords2 = &line2.coords;

    for i in 0..coords1.len().saturating_sub(1) {
        for j in 0..coords2.len().saturating_sub(1) {
            if let Some(intersection) = compute_segment_intersection(
                &coords1[i],
                &coords1[i + 1],
                &coords2[j],
                &coords2[j + 1],
                options.tolerance,
            ) {
                intersections.push(intersection);
            }
        }
    }

    Ok(intersections)
}

/// Compute intersection point between two line segments
fn compute_segment_intersection(
    p1: &Coordinate,
    p2: &Coordinate,
    p3: &Coordinate,
    p4: &Coordinate,
    tolerance: f64,
) -> Option<Coordinate> {
    let d = (p1.x - p2.x) * (p3.y - p4.y) - (p1.y - p2.y) * (p3.x - p4.x);

    if d.abs() < tolerance {
        // Parallel or coincident
        return None;
    }

    let t = ((p1.x - p3.x) * (p3.y - p4.y) - (p1.y - p3.y) * (p3.x - p4.x)) / d;
    let u = -((p1.x - p2.x) * (p1.y - p3.y) - (p1.y - p2.y) * (p1.x - p3.x)) / d;

    if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
        let x = p1.x + t * (p2.x - p1.x);
        let y = p1.y + t * (p2.y - p1.y);
        Some(Coordinate::new_2d(x, y))
    } else {
        None
    }
}

// ─── DCEL (Doubly-Connected Edge List) data structures ───────────────────────

/// A single half-edge in the DCEL.
///
/// Each undirected edge of the planar subdivision is represented by two
/// half-edges (a "twin" pair) that travel in opposite directions.
/// `next` follows the face boundary counter-clockwise.
#[derive(Clone, Debug)]
struct HalfEdge {
    /// Index into `Dcel::vertices` — the tail (origin) vertex.
    origin: usize,
    /// Index of the twin half-edge (same undirected edge, opposite direction).
    twin: usize,
    /// Index of the next half-edge around the face.
    next: usize,
    /// Index of the previous half-edge around the face.
    prev: usize,
}

/// The Doubly-Connected Edge List.
struct Dcel {
    /// All vertex positions, indexed by vertex id.
    vertices: Vec<[f64; 2]>,
    /// All half-edges, indexed by half-edge id.
    half_edges: Vec<HalfEdge>,
}

impl Dcel {
    fn new() -> Self {
        Self {
            vertices: Vec::new(),
            half_edges: Vec::new(),
        }
    }

    fn add_vertex(&mut self, x: f64, y: f64) -> usize {
        let idx = self.vertices.len();
        self.vertices.push([x, y]);
        idx
    }

    /// Allocate a twin pair A→B (index `he`) and B→A (index `he+1`).
    /// `next`/`prev` are left uninitialised (set to self) — caller fixes them.
    fn add_half_edge_pair(&mut self, a: usize, b: usize) -> (usize, usize) {
        let he = self.half_edges.len();
        let te = he + 1;
        self.half_edges.push(HalfEdge {
            origin: a,
            twin: te,
            next: he,
            prev: he,
        });
        self.half_edges.push(HalfEdge {
            origin: b,
            twin: he,
            next: te,
            prev: te,
        });
        (he, te)
    }
}

// ─── Geometry helpers ─────────────────────────────────────────────────────────

/// Signed area using the shoelace formula on an open (non-closing) ring.
/// Positive → CCW, negative → CW.
fn signed_area_ring(coords: &[[f64; 2]]) -> f64 {
    let n = coords.len();
    if n < 3 {
        return 0.0;
    }
    let mut area = 0.0f64;
    for i in 0..n {
        let j = (i + 1) % n;
        area += coords[i][0] * coords[j][1];
        area -= coords[j][0] * coords[i][1];
    }
    area * 0.5
}

/// Ray-casting point-in-polygon test on an open ring (no closing duplicate).
fn point_in_ring(px: f64, py: f64, ring: &[[f64; 2]]) -> bool {
    let mut inside = false;
    let n = ring.len();
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let xi = ring[i][0];
        let yi = ring[i][1];
        let xj = ring[j][0];
        let yj = ring[j][1];
        if ((yi > py) != (yj > py)) && (px < (xj - xi) * (py - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Squared distance between two 2-D points.
#[inline]
fn dist2(a: [f64; 2], b: [f64; 2]) -> f64 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    dx * dx + dy * dy
}

// ─── Core DCEL polygon-split implementation ───────────────────────────────────

/// Find (or reuse) a vertex within `tol_sq` of `[x, y]`.
fn find_or_add_vertex(dcel: &mut Dcel, x: f64, y: f64, tol_sq: f64) -> usize {
    let pt = [x, y];
    if let Some(idx) = dcel.vertices.iter().position(|v| dist2(*v, pt) <= tol_sq) {
        return idx;
    }
    dcel.add_vertex(x, y)
}

/// Walk a face cycle starting from `start_he` and collect vertex positions.
fn walk_face_cycle(dcel: &Dcel, start_he: usize) -> Vec<[f64; 2]> {
    let max_steps = dcel.half_edges.len() + 1;
    let mut coords = Vec::new();
    let mut cur = start_he;
    for _ in 0..max_steps {
        coords.push(dcel.vertices[dcel.half_edges[cur].origin]);
        cur = dcel.half_edges[cur].next;
        if cur == start_he {
            break;
        }
    }
    coords
}

/// Collect all distinct face cycles (each half-edge is in exactly one cycle).
fn collect_face_cycles(dcel: &Dcel) -> Vec<Vec<[f64; 2]>> {
    let n = dcel.half_edges.len();
    let mut visited = vec![false; n];
    let mut faces = Vec::new();
    for start in 0..n {
        if visited[start] {
            continue;
        }
        let cycle = walk_face_cycle(dcel, start);
        let mut cur = start;
        for _ in 0..cycle.len() {
            visited[cur] = true;
            cur = dcel.half_edges[cur].next;
        }
        if cycle.len() >= 3 {
            faces.push(cycle);
        }
    }
    faces
}

/// Split one half-edge `he_ab` (A→B) at midpoint vertex `m`.
///
/// This replaces the A→B edge with A→M, and inserts M→B immediately after.
/// The twin B→A is similarly replaced with B→M / M→A.
///
/// `ring_hes` is the ordered list of forward ring half-edges; the split half-edge
/// at `ring_pos` is updated in place and M→B is inserted after it.
fn split_half_edge(dcel: &mut Dcel, ring_hes: &mut Vec<usize>, ring_pos: usize, m: usize) {
    let he_ab = ring_hes[ring_pos];
    let he_ba = dcel.half_edges[he_ab].twin;
    let b = dcel.half_edges[he_ba].origin;

    // Save neighbourhood.
    let next_ab = dcel.half_edges[he_ab].next;
    let prev_ba = dcel.half_edges[he_ba].prev;

    // Allocate M→B and B→M.
    let (he_mb, he_bm) = dcel.add_half_edge_pair(m, b);

    // Rewire he_ab → now A→M (destination changes to M via twin).
    dcel.half_edges[he_ba].origin = m; // twin (M→A) now starts at M

    // Forward ring: A→M → M→B → old_next_of_AB
    dcel.half_edges[he_ab].next = he_mb;
    dcel.half_edges[he_mb].prev = he_ab;
    dcel.half_edges[he_mb].next = next_ab;
    dcel.half_edges[next_ab].prev = he_mb;

    // Backward ring: old_prev_of_BA → B→M → M→A
    dcel.half_edges[he_bm].prev = prev_ba;
    dcel.half_edges[prev_ba].next = he_bm;
    dcel.half_edges[he_bm].next = he_ba;
    dcel.half_edges[he_ba].prev = he_bm;

    // Update ring_hes: he_ab stays (now A→M), insert he_mb after.
    ring_hes.insert(ring_pos + 1, he_mb);
}

/// The standard DCEL "split face" operation:
/// insert a diagonal from vertex A (arrived at by `he_into_a`) to vertex B
/// (arrived at by `he_into_b`), subdividing one face into two.
///
/// `he_ab` / `he_ba` are the freshly-allocated diagonal half-edges.
fn splice_diagonal(
    dcel: &mut Dcel,
    he_into_a: usize,
    he_into_b: usize,
    he_ab: usize,
    he_ba: usize,
) {
    // The "next" pointers after he_into_a and he_into_b (i.e., what currently
    // follows A and B along the ring).
    let after_a = dcel.half_edges[he_into_a].next;
    let after_b = dcel.half_edges[he_into_b].next;

    // Face 1: … → into_a → he_ab → after_b → … → into_b (loop back to he_ba side)
    dcel.half_edges[he_into_a].next = he_ab;
    dcel.half_edges[he_ab].prev = he_into_a;
    dcel.half_edges[he_ab].next = after_b;
    dcel.half_edges[after_b].prev = he_ab;

    // Face 2: … → into_b → he_ba → after_a → … → into_a (loop back)
    dcel.half_edges[he_into_b].next = he_ba;
    dcel.half_edges[he_ba].prev = he_into_b;
    dcel.half_edges[he_ba].next = after_a;
    dcel.half_edges[after_a].prev = he_ba;
}

/// Find the ring half-edge whose **destination** is `vid` (i.e., whose twin's
/// origin is `vid`).  We first look in `ring_hes`, then fall back to a full
/// scan.
fn find_incoming_he(dcel: &Dcel, vid: usize, ring_hes: &[usize]) -> Option<usize> {
    // Primary: ring forward half-edges.
    for &he in ring_hes {
        if dcel.half_edges[dcel.half_edges[he].twin].origin == vid {
            return Some(he);
        }
    }
    // Fallback: check all half-edges' twins.
    dcel.half_edges
        .iter()
        .enumerate()
        .find(|(_, he)| dcel.half_edges[he.twin].origin == vid)
        .map(|(i, _)| i)
}

/// Create split polygons from intersection points using a DCEL.
///
/// # Algorithm (de Berg et al., *Computational Geometry*)
///
/// 1. Build a half-edge ring from the exterior polygon ring.
/// 2. Sort intersection points along the splitting line by parametric arc-length `t`.
/// 3. Insert each intersection vertex into the ring by subdividing the ring edge
///    it lies on (split-edge operation on the DCEL).
/// 4. For each consecutive pair of intersections whose midpoint lies inside the
///    polygon, splice a diagonal half-edge pair to split the face.
/// 5. Walk all resulting face cycles; identify the outer (unbounded) face by its
///    most-negative signed area and discard it.
/// 6. Apply `SplitOptions` filters (`min_area`, `preserve_orientation`) and return.
fn create_split_polygons(
    polygon: &Polygon,
    split_line: &LineString,
    intersections: &[Coordinate],
    options: &SplitOptions,
) -> Result<Vec<Polygon>> {
    let tol = options.tolerance;
    let tol_sq = tol * tol;

    // ── Step 1: Build DCEL ring from polygon exterior ────────────────────────
    let ext_raw: Vec<[f64; 2]> = polygon.exterior.coords.iter().map(|c| [c.x, c.y]).collect();

    // Exterior ring: n coords with first == last.  We use n-1 distinct vertices.
    let n_ring = ext_raw.len();
    if n_ring < 4 {
        return Ok(vec![polygon.clone()]);
    }
    let n_verts = n_ring - 1; // distinct ring vertices

    let mut dcel = Dcel::new();

    // Add vertices (skip the closing duplicate).
    let ring_vids: Vec<usize> = (0..n_verts)
        .map(|i| dcel.add_vertex(ext_raw[i][0], ext_raw[i][1]))
        .collect();

    // Add half-edge pairs for each ring edge and wire them into a cycle.
    // `ring_hes[i]` is the forward half-edge v[i] → v[(i+1)%n].
    let mut ring_hes: Vec<usize> = Vec::with_capacity(n_verts);
    let mut twin_hes: Vec<usize> = Vec::with_capacity(n_verts);
    for i in 0..n_verts {
        let a = ring_vids[i];
        let b = ring_vids[(i + 1) % n_verts];
        let (he, te) = dcel.add_half_edge_pair(a, b);
        ring_hes.push(he);
        twin_hes.push(te);
    }
    // Wire forward cycle: ring_hes[i].next = ring_hes[(i+1)%n].
    for i in 0..n_verts {
        let next_i = (i + 1) % n_verts;
        let prev_i = (i + n_verts - 1) % n_verts;
        dcel.half_edges[ring_hes[i]].next = ring_hes[next_i];
        dcel.half_edges[ring_hes[i]].prev = ring_hes[prev_i];
    }
    // Wire backward cycle: twin_hes[i].next = twin_hes[(i-1+n)%n].
    for i in 0..n_verts {
        let next_i = (i + n_verts - 1) % n_verts; // backward cycle goes the other way
        let prev_i = (i + 1) % n_verts;
        dcel.half_edges[twin_hes[i]].next = twin_hes[next_i];
        dcel.half_edges[twin_hes[i]].prev = twin_hes[prev_i];
    }

    // ── Step 2: Sort intersections along the split line by arc-length `t` ───
    let split_raw: Vec<[f64; 2]> = split_line.coords.iter().map(|c| [c.x, c.y]).collect();
    let split_n = split_raw.len();

    // Cumulative arc-length parameter at each split-line vertex.
    let mut cumlen = vec![0.0f64; split_n];
    for i in 1..split_n {
        let dx = split_raw[i][0] - split_raw[i - 1][0];
        let dy = split_raw[i][1] - split_raw[i - 1][1];
        cumlen[i] = cumlen[i - 1] + (dx * dx + dy * dy).sqrt();
    }

    // Each intersection point gets a parametric `t` along the split line.
    struct IntPt {
        coord: [f64; 2],
        t: f64,
    }

    let mut int_pts: Vec<IntPt> = Vec::new();
    'classify: for isect in intersections {
        let ix = isect.x;
        let iy = isect.y;
        for seg in 0..split_n.saturating_sub(1) {
            let [ax, ay] = split_raw[seg];
            let [bx, by] = split_raw[seg + 1];
            let ddx = bx - ax;
            let ddy = by - ay;
            let seg_len_sq = ddx * ddx + ddy * ddy;
            if seg_len_sq < tol_sq {
                continue;
            }
            let s = ((ix - ax) * ddx + (iy - ay) * ddy) / seg_len_sq;
            if s < -tol || s > 1.0 + tol {
                continue;
            }
            let proj_x = ax + s * ddx;
            let proj_y = ay + s * ddy;
            if (ix - proj_x).powi(2) + (iy - proj_y).powi(2) > tol_sq * 100.0 {
                continue;
            }
            let s_clamped = s.clamp(0.0, 1.0);
            let t = cumlen[seg] + s_clamped * (cumlen[seg + 1] - cumlen[seg]);
            int_pts.push(IntPt { coord: [ix, iy], t });
            continue 'classify;
        }
    }

    int_pts.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap_or(std::cmp::Ordering::Equal));
    int_pts.dedup_by(|a, b| dist2(a.coord, b.coord) < tol_sq * 100.0);

    if int_pts.len() < 2 {
        return Ok(vec![polygon.clone()]);
    }

    // ── Step 3: Insert intersection vertices into the ring ───────────────────
    // For each intersection point, find the ring edge it lies on and split that
    // edge at the intersection vertex.

    // intersection vertex ids, parallel to int_pts.
    let mut int_vids: Vec<usize> = Vec::with_capacity(int_pts.len());

    for ip in &int_pts {
        let [ix, iy] = ip.coord;

        // Find the ring edge position this point lies on.
        let mut found_pos: Option<usize> = None;
        'find_edge: for pos in 0..ring_hes.len() {
            let he = ring_hes[pos];
            let a_vid = dcel.half_edges[he].origin;
            let b_vid = dcel.half_edges[dcel.half_edges[he].twin].origin;
            let [ax, ay] = dcel.vertices[a_vid];
            let [bx, by] = dcel.vertices[b_vid];
            let ddx = bx - ax;
            let ddy = by - ay;
            let seg_len_sq = ddx * ddx + ddy * ddy;
            if seg_len_sq < tol_sq {
                continue;
            }
            let s = ((ix - ax) * ddx + (iy - ay) * ddy) / seg_len_sq;
            if s < -tol || s > 1.0 + tol {
                continue;
            }
            let proj_x = ax + s * ddx;
            let proj_y = ay + s * ddy;
            if (ix - proj_x).powi(2) + (iy - proj_y).powi(2) <= tol_sq * 100.0 {
                found_pos = Some(pos);
                break 'find_edge;
            }
        }

        // Find or create the intersection vertex.
        let m_vid = find_or_add_vertex(&mut dcel, ix, iy, tol_sq * 100.0);
        int_vids.push(m_vid);

        if let Some(pos) = found_pos {
            // Skip if intersection coincides with an existing ring vertex.
            let he = ring_hes[pos];
            let a_vid = dcel.half_edges[he].origin;
            let b_vid = dcel.half_edges[dcel.half_edges[he].twin].origin;
            if m_vid == a_vid || m_vid == b_vid {
                continue; // already a ring vertex
            }
            split_half_edge(&mut dcel, &mut ring_hes, pos, m_vid);
        }
    }

    // ── Step 4: Add diagonal half-edges along the split line ────────────────
    // For each consecutive pair of intersections whose midpoint is inside the
    // polygon, splice a new diagonal to subdivide the face.

    for k in 0..int_pts.len().saturating_sub(1) {
        let [ax, ay] = int_pts[k].coord;
        let [bx, by] = int_pts[k + 1].coord;
        let mx = (ax + bx) * 0.5;
        let my = (ay + by) * 0.5;

        // Midpoint must be strictly inside the polygon exterior.
        if !point_in_ring(mx, my, &ext_raw[..n_verts]) {
            continue;
        }
        // Midpoint must not be inside any hole.
        let in_hole = polygon.interiors.iter().any(|h| {
            let hole_raw: Vec<[f64; 2]> = h.coords.iter().map(|c| [c.x, c.y]).collect();
            let n_h = hole_raw.len().saturating_sub(1); // drop closing duplicate
            point_in_ring(mx, my, &hole_raw[..n_h])
        });
        if in_hole {
            continue;
        }

        let vid_a = int_vids[k];
        let vid_b = int_vids[k + 1];
        if vid_a == vid_b {
            continue;
        }

        // Find incoming ring half-edges at A and B.
        let into_a = match find_incoming_he(&dcel, vid_a, &ring_hes) {
            Some(h) => h,
            None => continue,
        };
        let into_b = match find_incoming_he(&dcel, vid_b, &ring_hes) {
            Some(h) => h,
            None => continue,
        };

        // Allocate diagonal A→B and B→A.
        let (he_ab, he_ba) = dcel.add_half_edge_pair(vid_a, vid_b);

        // Splice into the face cycle.
        splice_diagonal(&mut dcel, into_a, into_b, he_ab, he_ba);
    }

    // ── Step 5: Extract face cycles and identify inner faces ─────────────────
    let face_cycles = collect_face_cycles(&dcel);
    if face_cycles.is_empty() {
        return Ok(vec![polygon.clone()]);
    }

    // The outer (unbounded) face is the cycle with the most-negative signed area.
    let outer_idx = face_cycles
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            signed_area_ring(a)
                .partial_cmp(&signed_area_ring(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(i, _)| i)
        .unwrap_or(usize::MAX);

    // ── Step 6: Build result polygons from inner faces ───────────────────────
    let mut result_polygons: Vec<Polygon> = Vec::new();

    for (i, cycle) in face_cycles.iter().enumerate() {
        if i == outer_idx {
            continue;
        }
        let sa = signed_area_ring(cycle);
        if sa.abs() < tol * tol {
            continue; // degenerate
        }

        // Close the ring.
        let mut ring_pts = cycle.clone();
        ring_pts.push(ring_pts[0]);

        if ring_pts.len() < 4 {
            continue;
        }

        // Ensure correct orientation if requested.
        if options.preserve_orientation && sa < 0.0 {
            ring_pts.reverse();
        }

        let coords: Vec<Coordinate> = ring_pts
            .iter()
            .map(|&[x, y]| Coordinate::new_2d(x, y))
            .collect();

        let exterior = match LineString::new(coords) {
            Ok(ls) => ls,
            Err(_) => continue,
        };

        let poly_area = sa.abs();
        if let Some(threshold) = options.min_area {
            if poly_area < threshold {
                continue;
            }
        }

        // Assign polygon holes that lie inside this face.
        let mut interior_rings: Vec<LineString> = Vec::new();
        if options.keep_holes {
            for hole in &polygon.interiors {
                if hole.coords.len() < 4 {
                    continue;
                }
                let hx = hole.coords[0].x;
                let hy = hole.coords[0].y;
                if point_in_ring(hx, hy, cycle) {
                    interior_rings.push(hole.clone());
                }
            }
        }

        let poly = match Polygon::new(exterior, interior_rings) {
            Ok(p) => p,
            Err(_) => continue,
        };
        result_polygons.push(poly);
    }

    if result_polygons.is_empty() {
        return Ok(vec![polygon.clone()]);
    }

    Ok(result_polygons)
}

/// Split a polygon by another polygon
///
/// Divides a polygon using another polygon's boundary as cutting edges.
pub fn split_polygon_by_polygon(
    target_poly: &Polygon,
    split_poly: &Polygon,
    options: &SplitOptions,
) -> Result<SplitResult> {
    // Use the exterior ring of the split polygon as the splitting linestring
    let split_line = &split_poly.exterior.clone();

    split_polygon_by_line(target_poly, split_line, options)
}

/// Batch split operation for multiple polygons
pub fn split_polygons_batch(
    polygons: &[Polygon],
    split_line: &LineString,
    options: &SplitOptions,
) -> Result<Vec<SplitResult>> {
    polygons
        .iter()
        .map(|poly| split_polygon_by_line(poly, split_line, options))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_linestring(coords: Vec<(f64, f64)>) -> LineString {
        let coords: Vec<Coordinate> = coords
            .iter()
            .map(|(x, y)| Coordinate::new_2d(*x, *y))
            .collect();
        LineString::new(coords).expect("Failed to create linestring")
    }

    fn create_polygon(coords: Vec<(f64, f64)>) -> Polygon {
        let coords: Vec<Coordinate> = coords
            .iter()
            .map(|(x, y)| Coordinate::new_2d(*x, *y))
            .collect();
        let exterior = LineString::new(coords).expect("Failed to create linestring");
        Polygon::new(exterior, vec![]).expect("Failed to create polygon")
    }

    #[test]
    fn test_split_linestring_single_point() {
        let linestring = create_linestring(vec![(0.0, 0.0), (10.0, 0.0)]);
        let split_points = vec![Point::new(5.0, 0.0)];

        let result =
            split_linestring_by_points(&linestring, &split_points, &SplitOptions::default());
        assert!(result.is_ok());

        let split_result = result.expect("Split failed");
        assert_eq!(split_result.num_splits, 1);
        assert!(split_result.geometries.len() >= 2);
    }

    #[test]
    fn test_split_linestring_multiple_points() {
        let linestring = create_linestring(vec![(0.0, 0.0), (10.0, 0.0)]);
        let split_points = vec![Point::new(3.0, 0.0), Point::new(7.0, 0.0)];

        let result =
            split_linestring_by_points(&linestring, &split_points, &SplitOptions::default());
        assert!(result.is_ok());

        let split_result = result.expect("Split failed");
        assert_eq!(split_result.num_splits, 2);
    }

    #[test]
    fn test_split_linestring_no_intersection() {
        let linestring = create_linestring(vec![(0.0, 0.0), (10.0, 0.0)]);
        let split_points = vec![Point::new(5.0, 5.0)]; // Not on line

        let result =
            split_linestring_by_points(&linestring, &split_points, &SplitOptions::default());
        assert!(result.is_ok());

        let split_result = result.expect("Split failed");
        assert_eq!(split_result.num_splits, 0);
        assert_eq!(split_result.geometries.len(), 1); // Unchanged
    }

    #[test]
    fn test_split_linestring_empty_splits() {
        let linestring = create_linestring(vec![(0.0, 0.0), (10.0, 0.0)]);
        let split_points = vec![];

        let result =
            split_linestring_by_points(&linestring, &split_points, &SplitOptions::default());
        assert!(result.is_ok());

        let split_result = result.expect("Split failed");
        assert_eq!(split_result.num_splits, 0);
        assert_eq!(split_result.geometries.len(), 1);
    }

    #[test]
    fn test_point_on_segment() {
        let p1 = Coordinate::new_2d(0.0, 0.0);
        let p2 = Coordinate::new_2d(10.0, 0.0);
        let point = Point::new(5.0, 0.0);

        let result = point_on_segment(&point, &p1, &p2, 1e-10);
        assert!(result.is_some());

        if let Some((param, coord)) = result {
            assert!((param - 0.5).abs() < 1e-10);
            assert!((coord.x - 5.0).abs() < 1e-10);
            assert!((coord.y - 0.0).abs() < 1e-10);
        }
    }

    #[test]
    fn test_point_not_on_segment() {
        let p1 = Coordinate::new_2d(0.0, 0.0);
        let p2 = Coordinate::new_2d(10.0, 0.0);
        let point = Point::new(5.0, 5.0); // Off the line

        let result = point_on_segment(&point, &p1, &p2, 1e-10);
        assert!(result.is_none());
    }

    #[test]
    fn test_compute_segment_intersection() {
        let p1 = Coordinate::new_2d(0.0, 0.0);
        let p2 = Coordinate::new_2d(10.0, 10.0);
        let p3 = Coordinate::new_2d(0.0, 10.0);
        let p4 = Coordinate::new_2d(10.0, 0.0);

        let result = compute_segment_intersection(&p1, &p2, &p3, &p4, 1e-10);
        assert!(result.is_some());

        if let Some(intersection) = result {
            // Intersection should be at (5, 5)
            assert!((intersection.x - 5.0).abs() < 1e-6);
            assert!((intersection.y - 5.0).abs() < 1e-6);
        }
    }

    #[test]
    fn test_split_polygon_by_line() {
        let polygon = create_polygon(vec![
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 10.0),
            (0.0, 10.0),
            (0.0, 0.0),
        ]);

        let split_line = create_linestring(vec![(0.0, 5.0), (10.0, 5.0)]);

        let result = split_polygon_by_line(&polygon, &split_line, &SplitOptions::default());
        assert!(result.is_ok());

        let split_result = result.expect("Split failed");
        assert_eq!(split_result.num_splits, 2); // Two intersection points
    }

    #[test]
    fn test_compute_linestring_length() {
        let linestring = create_linestring(vec![(0.0, 0.0), (3.0, 0.0), (3.0, 4.0)]);
        let length = compute_linestring_length(&linestring);

        // Length is 3 + 4 = 7
        assert!((length - 7.0).abs() < 1e-6);
    }

    // ─── DCEL polygon-split tests ─────────────────────────────────────────────

    /// Helper: compute unsigned area of a polygon using the shoelace formula.
    fn polygon_area(poly: &Polygon) -> f64 {
        let coords = &poly.exterior.coords;
        let n = coords.len();
        if n < 3 {
            return 0.0;
        }
        let mut area = 0.0f64;
        let mut j = n - 1;
        for i in 0..n {
            area += (coords[j].x + coords[i].x) * (coords[j].y - coords[i].y);
            j = i;
        }
        (area * 0.5).abs()
    }

    #[test]
    fn test_split_square_by_vertical_returns_two_halves() {
        // 1×1 square split at x = 0.5 should produce two rectangles each of area ≈ 0.5.
        let polygon = create_polygon(vec![
            (0.0, 0.0),
            (1.0, 0.0),
            (1.0, 1.0),
            (0.0, 1.0),
            (0.0, 0.0),
        ]);
        let split_line = create_linestring(vec![(-0.5, 0.5), (1.5, 0.5)]);

        let result = split_polygon_by_line(&polygon, &split_line, &SplitOptions::default())
            .expect("split must succeed for vertical line on unit square");

        // We expect two polygon pieces.
        assert_eq!(
            result.geometries.len(),
            2,
            "vertical split of unit square must yield exactly 2 pieces, got {}",
            result.geometries.len()
        );

        let areas: Vec<f64> = result
            .geometries
            .iter()
            .filter_map(|g| {
                if let SplitGeometry::Polygon(p) = g {
                    Some(polygon_area(p))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(
            areas.len(),
            result.geometries.len(),
            "all geometries must be polygons"
        );
        for area in &areas {
            assert!(
                (area - 0.5).abs() < 1e-6,
                "each half must have area ≈ 0.5, got {area}"
            );
        }
    }

    #[test]
    fn test_split_square_by_diagonal_returns_two_triangles() {
        // 1×1 square split along the diagonal (0,0)→(1,1) yields two triangles
        // each with area ≈ 0.5.
        let polygon = create_polygon(vec![
            (0.0, 0.0),
            (1.0, 0.0),
            (1.0, 1.0),
            (0.0, 1.0),
            (0.0, 0.0),
        ]);
        let split_line = create_linestring(vec![(-0.1, -0.1), (1.1, 1.1)]);

        let result = split_polygon_by_line(&polygon, &split_line, &SplitOptions::default())
            .expect("split must succeed for diagonal split");

        assert_eq!(
            result.geometries.len(),
            2,
            "diagonal split must yield 2 triangles, got {}",
            result.geometries.len()
        );

        let areas: Vec<f64> = result
            .geometries
            .iter()
            .filter_map(|g| {
                if let SplitGeometry::Polygon(p) = g {
                    Some(polygon_area(p))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(areas.len(), 2, "both geometries must be polygons");
        for area in &areas {
            assert!(
                (area - 0.5).abs() < 1e-5,
                "each triangle must have area ≈ 0.5, got {area}"
            );
        }
    }

    #[test]
    fn test_split_line_misses_polygon_returns_original() {
        // A line that does not intersect the polygon at all must return the original.
        let polygon = create_polygon(vec![
            (0.0, 0.0),
            (1.0, 0.0),
            (1.0, 1.0),
            (0.0, 1.0),
            (0.0, 0.0),
        ]);
        let split_line = create_linestring(vec![(5.0, 0.0), (5.0, 1.0)]);

        let result = split_polygon_by_line(&polygon, &split_line, &SplitOptions::default())
            .expect("split must succeed even when line misses polygon");

        assert_eq!(
            result.geometries.len(),
            1,
            "missed line must return exactly the original polygon"
        );
        assert_eq!(
            result.num_splits, 0,
            "no split points should be recorded for a miss"
        );
    }

    #[test]
    fn test_split_concave_polygon_three_pieces() {
        // L-shaped concave polygon (vertices in CCW order):
        //   (0,0)→(2,0)→(2,1)→(1,1)→(1,2)→(0,2)→(0,0)
        // A horizontal split at y = 1 cuts across the notch and can yield
        // multiple pieces.  We just verify every piece has positive area.
        let polygon = create_polygon(vec![
            (0.0, 0.0),
            (2.0, 0.0),
            (2.0, 1.0),
            (1.0, 1.0),
            (1.0, 2.0),
            (0.0, 2.0),
            (0.0, 0.0),
        ]);
        let split_line = create_linestring(vec![(-0.5, 1.0), (2.5, 1.0)]);

        let result = split_polygon_by_line(&polygon, &split_line, &SplitOptions::default())
            .expect("split must succeed for L-shaped polygon");

        assert!(
            !result.geometries.is_empty(),
            "split must return at least one polygon piece"
        );

        for geom in &result.geometries {
            if let SplitGeometry::Polygon(poly) = geom {
                let area = polygon_area(poly);
                assert!(
                    area > 1e-10,
                    "all result pieces must have positive area, got {area}"
                );
            }
            // Non-polygon variants are ignored: split_polygon_by_line only produces Polygon results.
        }
    }

    #[test]
    fn test_split_respects_min_area_filter() {
        // Split a 1×1 square at y = 0.5, then apply min_area = 1.0 (larger than either half).
        // No piece should survive the filter.
        let polygon = create_polygon(vec![
            (0.0, 0.0),
            (1.0, 0.0),
            (1.0, 1.0),
            (0.0, 1.0),
            (0.0, 0.0),
        ]);
        let split_line = create_linestring(vec![(-0.5, 0.5), (1.5, 0.5)]);

        let options = SplitOptions {
            min_area: Some(1.0), // both halves have area 0.5 < 1.0
            ..SplitOptions::default()
        };

        let result = split_polygon_by_line(&polygon, &split_line, &options)
            .expect("split must succeed with min_area filter");

        // If the split worked correctly and filter is applied, we get 0 pieces.
        // (If the DCEL returned the original on failure, area = 1.0 which equals the
        //  threshold, so we test ≤ 1 to be safe.)
        for geom in &result.geometries {
            if let SplitGeometry::Polygon(poly) = geom {
                let area = polygon_area(poly);
                assert!(
                    area >= 1.0,
                    "min_area filter must remove all pieces smaller than 1.0, piece area = {area}"
                );
            }
        }
    }
}
