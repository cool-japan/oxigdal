//! Topology-preserving simplification for polygon collections
//!
//! Shared edges between adjacent polygons are simplified once so that both polygons
//! receive the identical simplified edge. Junction vertices (where three or more
//! polygons meet or chain boundaries) are always preserved.
//!
//! # Algorithm
//!
//! 1. **Edge decomposition** -- every directed segment of every ring is assigned a
//!    canonical `EdgeKey` (quantized, ordered endpoint pair). Each key maps to a
//!    list of `EdgeRecord`s that identify which polygon/ring/segment produced it.
//! 2. **Chain building** -- segments within a ring are grouped into maximal
//!    contiguous chains that share the same neighbour polygon (or are unshared).
//! 3. **Simplification** -- shared chains are Douglas-Peucker simplified once,
//!    with endpoints locked. Non-shared chains are simplified independently.
//! 4. **Reassembly** -- rings are reconstructed from simplified chains, reversing
//!    coordinate order where the original edge winding was opposite.
//! 5. **Validation** -- a self-intersection check is run on every ring. If any
//!    ring fails, non-shared chains are re-simplified at half tolerance.
//!
//! # Examples
//!
//! ```
//! # use oxigdal_algorithms::error::Result;
//! use oxigdal_algorithms::vector::{Coordinate, LineString, Polygon};
//! use oxigdal_algorithms::vector::{simplify_topology, TopologySimplifyOptions};
//!
//! # fn main() -> Result<()> {
//! // Two adjacent squares sharing edge at x=1
//! let sq_a = Polygon::new(
//!     LineString::new(vec![
//!         Coordinate::new_2d(0.0, 0.0),
//!         Coordinate::new_2d(1.0, 0.0),
//!         Coordinate::new_2d(1.0, 1.0),
//!         Coordinate::new_2d(0.0, 1.0),
//!         Coordinate::new_2d(0.0, 0.0),
//!     ])?,
//!     vec![],
//! )?;
//! let sq_b = Polygon::new(
//!     LineString::new(vec![
//!         Coordinate::new_2d(1.0, 0.0),
//!         Coordinate::new_2d(2.0, 0.0),
//!         Coordinate::new_2d(2.0, 1.0),
//!         Coordinate::new_2d(1.0, 1.0),
//!         Coordinate::new_2d(1.0, 0.0),
//!     ])?,
//!     vec![],
//! )?;
//! let simplified = simplify_topology(&[sq_a, sq_b], 0.1)?;
//! assert_eq!(simplified.len(), 2);
//! # Ok(())
//! # }
//! ```

use crate::error::{AlgorithmError, Result};
use oxigdal_core::vector::{Coordinate, LineString, Polygon};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Quantisation factor for snapping endpoints to a discrete grid.
/// Coordinates are multiplied by this before rounding to i64.
const QUANTIZE_FACTOR: f64 = 1e6;

// ---------------------------------------------------------------------------
// Public API types
// ---------------------------------------------------------------------------

/// Options for topology-preserving simplification.
#[derive(Debug, Clone)]
pub struct TopologySimplifyOptions {
    /// Douglas-Peucker tolerance.
    pub tolerance: f64,
    /// Maximum retry attempts when self-intersection is detected.
    pub max_retries: usize,
    /// Tolerance reduction factor per retry (applied to non-shared chains).
    pub retry_factor: f64,
}

impl Default for TopologySimplifyOptions {
    fn default() -> Self {
        Self {
            tolerance: 1.0,
            max_retries: 5,
            retry_factor: 0.5,
        }
    }
}

impl TopologySimplifyOptions {
    /// Create options with only a tolerance, using defaults for the rest.
    #[must_use]
    pub fn with_tolerance(tolerance: f64) -> Self {
        Self {
            tolerance,
            ..Self::default()
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Simplify a collection of polygons while preserving shared-edge topology.
///
/// Shared edges between adjacent polygons are simplified exactly once so that
/// both polygons receive the identical simplified result. Junction vertices
/// (chain boundaries) are always preserved.
///
/// # Errors
///
/// Returns an error if tolerance is negative or reassembled rings are degenerate.
pub fn simplify_topology(polygons: &[Polygon], tolerance: f64) -> Result<Vec<Polygon>> {
    let opts = TopologySimplifyOptions::with_tolerance(tolerance);
    simplify_topology_with_options(polygons, &opts)
}

/// Simplify a collection of polygons with detailed options.
///
/// See [`simplify_topology`] for the simpler interface.
///
/// # Errors
///
/// Returns an error if tolerance is negative or reassembled rings are degenerate.
pub fn simplify_topology_with_options(
    polygons: &[Polygon],
    options: &TopologySimplifyOptions,
) -> Result<Vec<Polygon>> {
    if options.tolerance < 0.0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "tolerance",
            message: "tolerance must be non-negative".to_string(),
        });
    }
    if polygons.is_empty() {
        return Ok(Vec::new());
    }
    // tolerance == 0 means no simplification
    if options.tolerance < f64::EPSILON {
        return Ok(polygons.to_vec());
    }

    // -- Step 1: edge decomposition ----------------------------------------
    let edge_map = build_edge_map(polygons);

    // -- Step 2+3+4: per-ring chain building, simplification, reassembly ---
    let mut result_polygons = Vec::with_capacity(polygons.len());

    // Shared-chain cache: key = (canonical chain key), value = simplified coords
    let mut shared_cache: HashMap<SharedChainKey, Vec<Coordinate>> = HashMap::new();

    for (poly_idx, polygon) in polygons.iter().enumerate() {
        let simplified_exterior = simplify_ring(
            &polygon.exterior,
            poly_idx,
            0, // ring_idx 0 = exterior
            &edge_map,
            &mut shared_cache,
            options,
        )?;

        let mut simplified_interiors = Vec::with_capacity(polygon.interiors.len());
        for (hole_idx, interior) in polygon.interiors.iter().enumerate() {
            let simplified_hole = simplify_ring(
                interior,
                poly_idx,
                hole_idx + 1, // ring_idx > 0 = interior
                &edge_map,
                &mut shared_cache,
                options,
            )?;
            // Only keep holes that survive simplification
            if simplified_hole.coords.len() >= 4 {
                simplified_interiors.push(simplified_hole);
            }
        }

        // Ensure exterior still valid
        if simplified_exterior.coords.len() < 4 {
            return Err(AlgorithmError::GeometryError {
                message: format!(
                    "simplified exterior of polygon {poly_idx} has fewer than 4 points"
                ),
            });
        }

        let poly = Polygon::new(simplified_exterior, simplified_interiors)
            .map_err(AlgorithmError::Core)?;
        result_polygons.push(poly);
    }

    Ok(result_polygons)
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

/// Quantized 2D point (i64 pair).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct QPoint(i64, i64);

/// Canonical edge key: ordered pair of quantized endpoints so that the
/// "smaller" endpoint comes first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct EdgeKey {
    a: QPoint,
    b: QPoint,
}

/// Record of one directed segment contributing to an EdgeKey.
#[derive(Debug, Clone)]
struct EdgeRecord {
    poly_idx: usize,
    ring_idx: usize,
    seg_idx: usize,
    /// True if the segment direction (start->end) was reversed to form the canonical key.
    is_reversed: bool,
}

/// Key for caching shared-chain simplification results.
/// (poly_a, poly_b, chain start QPoint, chain end QPoint)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SharedChainKey {
    poly_a: usize,
    poly_b: usize,
    start: QPoint,
    end: QPoint,
}

/// Describes one contiguous run of segments within a ring.
#[derive(Debug)]
struct Chain {
    /// Starting segment index in the ring.
    seg_start: usize,
    /// One-past-the-end segment index.
    seg_end: usize,
    /// If Some, this chain is shared with the given polygon index.
    shared_with: Option<usize>,
}

// ---------------------------------------------------------------------------
// Quantization helpers
// ---------------------------------------------------------------------------

fn quantize(c: &Coordinate) -> QPoint {
    QPoint(
        (c.x * QUANTIZE_FACTOR).round() as i64,
        (c.y * QUANTIZE_FACTOR).round() as i64,
    )
}

fn make_edge_key(a: &Coordinate, b: &Coordinate) -> (EdgeKey, bool) {
    let qa = quantize(a);
    let qb = quantize(b);
    if (qa.0, qa.1) <= (qb.0, qb.1) {
        (EdgeKey { a: qa, b: qb }, false)
    } else {
        (EdgeKey { a: qb, b: qa }, true)
    }
}

// ---------------------------------------------------------------------------
// Step 1: edge map
// ---------------------------------------------------------------------------

type EdgeMap = HashMap<EdgeKey, Vec<EdgeRecord>>;

fn build_edge_map(polygons: &[Polygon]) -> EdgeMap {
    let mut map: EdgeMap = HashMap::new();

    for (poly_idx, polygon) in polygons.iter().enumerate() {
        insert_ring_edges(&mut map, &polygon.exterior, poly_idx, 0);
        for (hole_idx, interior) in polygon.interiors.iter().enumerate() {
            insert_ring_edges(&mut map, interior, poly_idx, hole_idx + 1);
        }
    }

    map
}

fn insert_ring_edges(map: &mut EdgeMap, ring: &LineString, poly_idx: usize, ring_idx: usize) {
    let n = ring.coords.len();
    if n < 2 {
        return;
    }
    // A closed ring has n-1 segments (the last coord == first coord).
    let seg_count = n - 1;
    for seg_idx in 0..seg_count {
        let (key, is_reversed) = make_edge_key(&ring.coords[seg_idx], &ring.coords[seg_idx + 1]);
        // Skip zero-length edges after quantization
        if key.a == key.b {
            continue;
        }
        map.entry(key).or_default().push(EdgeRecord {
            poly_idx,
            ring_idx,
            seg_idx,
            is_reversed,
        });
    }
}

// ---------------------------------------------------------------------------
// Step 2: chain building within a ring
// ---------------------------------------------------------------------------

/// For a given ring in a given polygon, classify each segment as shared-with-X
/// or non-shared, then merge adjacent segments with the same classification
/// into chains.
fn build_chains(
    ring: &LineString,
    poly_idx: usize,
    ring_idx: usize,
    edge_map: &EdgeMap,
) -> Vec<Chain> {
    let n = ring.coords.len();
    if n < 2 {
        return Vec::new();
    }
    let seg_count = n - 1;

    // Classify each segment
    let mut seg_class: Vec<Option<usize>> = Vec::with_capacity(seg_count);
    for seg_idx in 0..seg_count {
        let (key, _) = make_edge_key(&ring.coords[seg_idx], &ring.coords[seg_idx + 1]);
        if key.a == key.b {
            // zero-length after quantization, treat as non-shared
            seg_class.push(None);
            continue;
        }
        let neighbour = edge_map.get(&key).and_then(|records| {
            // Find a record from a *different* polygon
            records.iter().find_map(|r| {
                if r.poly_idx != poly_idx {
                    Some(r.poly_idx)
                } else {
                    None
                }
            })
        });
        seg_class.push(neighbour);
    }

    // Merge contiguous segments with same classification into chains
    let mut chains = Vec::new();
    let mut i = 0;
    while i < seg_count {
        let cls = seg_class[i];
        let start = i;
        while i < seg_count && seg_class[i] == cls {
            i += 1;
        }
        chains.push(Chain {
            seg_start: start,
            seg_end: i,
            shared_with: cls,
        });
    }

    chains
}

// ---------------------------------------------------------------------------
// Step 3+4: simplify chains and reassemble
// ---------------------------------------------------------------------------

fn simplify_ring(
    ring: &LineString,
    poly_idx: usize,
    ring_idx: usize,
    edge_map: &EdgeMap,
    shared_cache: &mut HashMap<SharedChainKey, Vec<Coordinate>>,
    options: &TopologySimplifyOptions,
) -> Result<LineString> {
    let chains = build_chains(ring, poly_idx, ring_idx, edge_map);
    if chains.is_empty() {
        return Ok(ring.clone());
    }

    let mut result_coords: Vec<Coordinate> = Vec::new();

    for chain in &chains {
        // Extract the coordinate sub-sequence for this chain.
        // A chain of segments [seg_start..seg_end) covers coords[seg_start..=seg_end].
        let chain_coords = &ring.coords[chain.seg_start..=chain.seg_end];

        let simplified = match chain.shared_with {
            Some(other_poly) => simplify_shared_chain(
                chain_coords,
                poly_idx,
                other_poly,
                shared_cache,
                options.tolerance,
            ),
            None => dp_simplify_coords(chain_coords, options.tolerance),
        };

        // Append simplified coords, skipping the first if it duplicates the
        // last coordinate already in result_coords.
        if result_coords.is_empty() {
            result_coords.extend_from_slice(&simplified);
        } else if let Some(first) = simplified.first() {
            if let Some(last) = result_coords.last() {
                if coords_close(last, first) {
                    // skip duplicate junction vertex
                    if simplified.len() > 1 {
                        result_coords.extend_from_slice(&simplified[1..]);
                    }
                } else {
                    result_coords.extend_from_slice(&simplified);
                }
            } else {
                result_coords.extend_from_slice(&simplified);
            }
        }
    }

    // Close the ring: ensure last == first (exact copy)
    if result_coords.len() >= 2 {
        let first = result_coords[0];
        let last_idx = result_coords.len() - 1;
        if !coords_exact(&result_coords[last_idx], &first) {
            if coords_close(&result_coords[last_idx], &first) {
                // snap to exact
                result_coords[last_idx] = first;
            } else {
                result_coords.push(first);
            }
        }
    }

    // Self-intersection validation with retry
    if result_coords.len() >= 4 {
        let test_ls = LineString::new(result_coords.clone()).map_err(AlgorithmError::Core)?;
        if has_ring_self_intersection(&test_ls) {
            // Retry: re-simplify non-shared chains at reduced tolerance
            let reduced = attempt_retry_simplification(
                ring,
                poly_idx,
                ring_idx,
                edge_map,
                shared_cache,
                options,
            )?;
            return Ok(reduced);
        }
    }

    LineString::new(result_coords).map_err(AlgorithmError::Core)
}

/// Attempt re-simplification with progressively halved tolerance on non-shared
/// chains until the ring has no self-intersections or retries are exhausted.
fn attempt_retry_simplification(
    ring: &LineString,
    poly_idx: usize,
    ring_idx: usize,
    edge_map: &EdgeMap,
    shared_cache: &mut HashMap<SharedChainKey, Vec<Coordinate>>,
    options: &TopologySimplifyOptions,
) -> Result<LineString> {
    let chains = build_chains(ring, poly_idx, ring_idx, edge_map);
    let mut current_tol = options.tolerance * options.retry_factor;

    for _ in 0..options.max_retries {
        let mut result_coords: Vec<Coordinate> = Vec::new();

        for chain in &chains {
            let chain_coords = &ring.coords[chain.seg_start..=chain.seg_end];

            let simplified = match chain.shared_with {
                Some(other_poly) => {
                    // Shared chains keep original tolerance (they must stay consistent)
                    simplify_shared_chain(
                        chain_coords,
                        poly_idx,
                        other_poly,
                        shared_cache,
                        options.tolerance,
                    )
                }
                None => dp_simplify_coords(chain_coords, current_tol),
            };

            if result_coords.is_empty() {
                result_coords.extend_from_slice(&simplified);
            } else if let Some(first) = simplified.first() {
                if let Some(last) = result_coords.last() {
                    if coords_close(last, first) {
                        if simplified.len() > 1 {
                            result_coords.extend_from_slice(&simplified[1..]);
                        }
                    } else {
                        result_coords.extend_from_slice(&simplified);
                    }
                } else {
                    result_coords.extend_from_slice(&simplified);
                }
            }
        }

        // Close ring
        if result_coords.len() >= 2 {
            let first = result_coords[0];
            let last_idx = result_coords.len() - 1;
            if !coords_exact(&result_coords[last_idx], &first) {
                if coords_close(&result_coords[last_idx], &first) {
                    result_coords[last_idx] = first;
                } else {
                    result_coords.push(first);
                }
            }
        }

        if result_coords.len() >= 4 {
            let test_ls = LineString::new(result_coords.clone()).map_err(AlgorithmError::Core)?;
            if !has_ring_self_intersection(&test_ls) {
                return LineString::new(result_coords).map_err(AlgorithmError::Core);
            }
        }

        current_tol *= options.retry_factor;
        if current_tol < 1e-12 {
            break;
        }
    }

    // All retries exhausted, return original ring
    Ok(ring.clone())
}

/// Simplify a shared chain. Uses a cache so that the same chain between two
/// polygons is simplified exactly once.
fn simplify_shared_chain(
    chain_coords: &[Coordinate],
    poly_a: usize,
    poly_b: usize,
    cache: &mut HashMap<SharedChainKey, Vec<Coordinate>>,
    tolerance: f64,
) -> Vec<Coordinate> {
    // Canonical ordering: smaller polygon index first
    let (ca, cb) = if poly_a <= poly_b {
        (poly_a, poly_b)
    } else {
        (poly_b, poly_a)
    };

    let start_q = quantize(&chain_coords[0]);
    let end_q = quantize(&chain_coords[chain_coords.len() - 1]);

    // Build a canonical key: endpoints ordered by the canonical polygon order.
    // If poly_a == ca, the chain_coords are in "forward" order for the canonical key.
    // If poly_a != ca, they're reversed.
    let is_reversed = poly_a != ca;

    let (key_start, key_end) = if is_reversed {
        (end_q, start_q)
    } else {
        (start_q, end_q)
    };

    let key = SharedChainKey {
        poly_a: ca,
        poly_b: cb,
        start: key_start,
        end: key_end,
    };

    if let Some(cached) = cache.get(&key) {
        // Return in the caller's winding order
        if is_reversed {
            let mut rev = cached.clone();
            rev.reverse();
            return rev;
        }
        return cached.clone();
    }

    // Simplify in canonical (forward for poly_a=ca) direction
    let canonical_coords: Vec<Coordinate> = if is_reversed {
        chain_coords.iter().rev().copied().collect()
    } else {
        chain_coords.to_vec()
    };

    let simplified = dp_simplify_coords(&canonical_coords, tolerance);
    cache.insert(key, simplified.clone());

    if is_reversed {
        let mut rev = simplified;
        rev.reverse();
        rev
    } else {
        simplified
    }
}

// ---------------------------------------------------------------------------
// Douglas-Peucker on raw coordinate slices
// ---------------------------------------------------------------------------

/// Douglas-Peucker simplification on a coordinate slice.
/// Endpoints are always preserved.
fn dp_simplify_coords(coords: &[Coordinate], tolerance: f64) -> Vec<Coordinate> {
    if coords.len() <= 2 {
        return coords.to_vec();
    }

    let n = coords.len();
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[n - 1] = true;

    dp_recursive(coords, &mut keep, 0, n - 1, tolerance);

    coords
        .iter()
        .zip(keep.iter())
        .filter(|&(_, &k)| k)
        .map(|(c, _)| *c)
        .collect()
}

fn dp_recursive(coords: &[Coordinate], keep: &mut [bool], start: usize, end: usize, tol: f64) {
    if end <= start + 1 {
        return;
    }

    let mut max_dist: f64 = 0.0;
    let mut max_idx = start;

    for i in (start + 1)..end {
        let d = perpendicular_distance(&coords[i], &coords[start], &coords[end]);
        if d > max_dist {
            max_dist = d;
            max_idx = i;
        }
    }

    if max_dist > tol {
        keep[max_idx] = true;
        dp_recursive(coords, keep, start, max_idx, tol);
        dp_recursive(coords, keep, max_idx, end, tol);
    }
}

fn perpendicular_distance(point: &Coordinate, a: &Coordinate, b: &Coordinate) -> f64 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let len_sq = dx * dx + dy * dy;

    if len_sq < f64::EPSILON * f64::EPSILON {
        let ex = point.x - a.x;
        let ey = point.y - a.y;
        return (ex * ex + ey * ey).sqrt();
    }

    let numerator = (dy * point.x - dx * point.y + b.x * a.y - b.y * a.x).abs();
    let denominator = len_sq.sqrt();
    numerator / denominator
}

// ---------------------------------------------------------------------------
// Self-intersection detection (local implementation)
// ---------------------------------------------------------------------------

/// Check if a closed ring has self-intersections (ignoring the closing edge
/// that shares endpoints with adjacent edges).
fn has_ring_self_intersection(ring: &LineString) -> bool {
    let n = ring.coords.len();
    if n < 4 {
        return false;
    }

    let seg_count = n - 1;
    for i in 0..seg_count {
        // Only test non-adjacent segment pairs
        let j_start = if i == 0 { i + 2 } else { i + 2 };
        for j in j_start..seg_count {
            // Skip the pair formed by the last and first segment of a closed ring
            if i == 0 && j == seg_count - 1 {
                continue;
            }
            if segments_intersect(
                &ring.coords[i],
                &ring.coords[i + 1],
                &ring.coords[j],
                &ring.coords[j + 1],
            ) {
                return true;
            }
        }
    }

    false
}

fn segments_intersect(p1: &Coordinate, p2: &Coordinate, p3: &Coordinate, p4: &Coordinate) -> bool {
    let d1 = cross_direction(p3, p4, p1);
    let d2 = cross_direction(p3, p4, p2);
    let d3 = cross_direction(p1, p2, p3);
    let d4 = cross_direction(p1, p2, p4);

    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
    {
        return true;
    }

    // Collinear cases
    if d1.abs() < f64::EPSILON && on_segment(p3, p1, p4) {
        return true;
    }
    if d2.abs() < f64::EPSILON && on_segment(p3, p2, p4) {
        return true;
    }
    if d3.abs() < f64::EPSILON && on_segment(p1, p3, p2) {
        return true;
    }
    if d4.abs() < f64::EPSILON && on_segment(p1, p4, p2) {
        return true;
    }

    false
}

fn cross_direction(a: &Coordinate, b: &Coordinate, p: &Coordinate) -> f64 {
    (b.x - a.x) * (p.y - a.y) - (p.x - a.x) * (b.y - a.y)
}

fn on_segment(p: &Coordinate, q: &Coordinate, r: &Coordinate) -> bool {
    q.x <= p.x.max(r.x) && q.x >= p.x.min(r.x) && q.y <= p.y.max(r.y) && q.y >= p.y.min(r.y)
}

// ---------------------------------------------------------------------------
// Coordinate comparison helpers
// ---------------------------------------------------------------------------

fn coords_close(a: &Coordinate, b: &Coordinate) -> bool {
    (a.x - b.x).abs() < 1e-10 && (a.y - b.y).abs() < 1e-10
}

fn coords_exact(a: &Coordinate, b: &Coordinate) -> bool {
    a.x == b.x && a.y == b.y
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn make_polygon(coords: Vec<(f64, f64)>) -> Polygon {
        let cs: Vec<Coordinate> = coords
            .iter()
            .map(|&(x, y)| Coordinate::new_2d(x, y))
            .collect();
        let exterior = LineString::new(cs).expect("valid exterior");
        Polygon::new(exterior, vec![]).expect("valid polygon")
    }

    fn make_polygon_with_holes(
        exterior_coords: Vec<(f64, f64)>,
        holes: Vec<Vec<(f64, f64)>>,
    ) -> Polygon {
        let ext: Vec<Coordinate> = exterior_coords
            .iter()
            .map(|&(x, y)| Coordinate::new_2d(x, y))
            .collect();
        let exterior = LineString::new(ext).expect("valid exterior");
        let interiors: Vec<LineString> = holes
            .into_iter()
            .map(|h| {
                let cs: Vec<Coordinate> =
                    h.iter().map(|&(x, y)| Coordinate::new_2d(x, y)).collect();
                LineString::new(cs).expect("valid hole")
            })
            .collect();
        Polygon::new(exterior, interiors).expect("valid polygon with holes")
    }

    // -----------------------------------------------------------------------
    // 1. Adjacent squares — shared edge at x=1
    // -----------------------------------------------------------------------

    #[test]
    fn test_adjacent_squares_basic() {
        let sq_a = make_polygon(vec![
            (0.0, 0.0),
            (1.0, 0.0),
            (1.0, 1.0),
            (0.0, 1.0),
            (0.0, 0.0),
        ]);
        let sq_b = make_polygon(vec![
            (1.0, 0.0),
            (2.0, 0.0),
            (2.0, 1.0),
            (1.0, 1.0),
            (1.0, 0.0),
        ]);

        let result = simplify_topology(&[sq_a, sq_b], 0.1);
        assert!(result.is_ok());
        let polygons = result.expect("simplify should succeed");
        assert_eq!(polygons.len(), 2);

        // Both polygons must have valid, closed rings
        for p in &polygons {
            assert!(p.exterior.coords.len() >= 4);
            let first = &p.exterior.coords[0];
            let last = &p.exterior.coords[p.exterior.coords.len() - 1];
            assert!(coords_exact(first, last), "ring must be closed");
        }
    }

    // -----------------------------------------------------------------------
    // 2. Adjacent squares with jagged shared edge — simplification removes jag
    // -----------------------------------------------------------------------

    #[test]
    fn test_adjacent_squares_jagged_shared_edge() {
        // Square A: left side, with jagged shared edge at x~1
        let sq_a = make_polygon(vec![
            (0.0, 0.0),
            (1.0, 0.0),
            (1.0, 0.25),
            (1.01, 0.5), // jag
            (1.0, 0.75),
            (1.0, 1.0),
            (0.0, 1.0),
            (0.0, 0.0),
        ]);
        // Square B: right side, same jagged shared edge (reversed winding)
        let sq_b = make_polygon(vec![
            (1.0, 0.0),
            (2.0, 0.0),
            (2.0, 1.0),
            (1.0, 1.0),
            (1.0, 0.75),
            (1.01, 0.5), // same jag
            (1.0, 0.25),
            (1.0, 0.0),
        ]);

        let result = simplify_topology(&[sq_a, sq_b], 0.05);
        assert!(result.is_ok());
        let polygons = result.expect("simplify should succeed");
        assert_eq!(polygons.len(), 2);

        // Extract the shared edge coords from each polygon (those near x=1.0)
        let shared_a: Vec<(f64, f64)> = polygons[0]
            .exterior
            .coords
            .iter()
            .filter(|c| (c.x - 1.0).abs() < 0.02)
            .map(|c| (c.x, c.y))
            .collect();

        let shared_b: Vec<(f64, f64)> = polygons[1]
            .exterior
            .coords
            .iter()
            .filter(|c| (c.x - 1.0).abs() < 0.02)
            .map(|c| (c.x, c.y))
            .collect();

        // Shared edges should contain matching coordinates (possibly reversed).
        // A closed ring repeats its first/last vertex; if the ring starts at a
        // junction on the shared edge, that vertex appears twice in the coord
        // list. Deduplicate consecutive duplicates after sorting so the
        // comparison reflects the *set* of shared-edge vertices, not the ring
        // closure artifact.
        let mut sorted_a = shared_a.clone();
        sorted_a.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        sorted_a.dedup();
        let mut sorted_b = shared_b.clone();
        sorted_b.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        sorted_b.dedup();
        assert_eq!(
            sorted_a, sorted_b,
            "shared edge vertices must match between adjacent polygons"
        );
    }

    // -----------------------------------------------------------------------
    // 3. 2x2 grid — four squares, shared edges, junction at center
    // -----------------------------------------------------------------------

    #[test]
    fn test_2x2_grid() {
        let polys = vec![
            // bottom-left
            make_polygon(vec![
                (0.0, 0.0),
                (1.0, 0.0),
                (1.0, 1.0),
                (0.0, 1.0),
                (0.0, 0.0),
            ]),
            // bottom-right
            make_polygon(vec![
                (1.0, 0.0),
                (2.0, 0.0),
                (2.0, 1.0),
                (1.0, 1.0),
                (1.0, 0.0),
            ]),
            // top-left
            make_polygon(vec![
                (0.0, 1.0),
                (1.0, 1.0),
                (1.0, 2.0),
                (0.0, 2.0),
                (0.0, 1.0),
            ]),
            // top-right
            make_polygon(vec![
                (1.0, 1.0),
                (2.0, 1.0),
                (2.0, 2.0),
                (1.0, 2.0),
                (1.0, 1.0),
            ]),
        ];

        let result = simplify_topology(&polys, 0.1);
        assert!(result.is_ok());
        let simplified = result.expect("simplify should succeed");
        assert_eq!(simplified.len(), 4);

        // Every polygon must remain valid
        for (i, p) in simplified.iter().enumerate() {
            assert!(
                p.exterior.coords.len() >= 4,
                "polygon {i} has too few exterior coords"
            );
        }

        // Junction vertex (1,1) must be preserved in all four polygons
        for (i, p) in simplified.iter().enumerate() {
            let has_junction = p
                .exterior
                .coords
                .iter()
                .any(|c| (c.x - 1.0).abs() < 1e-10 && (c.y - 1.0).abs() < 1e-10);
            assert!(
                has_junction,
                "polygon {i} must preserve junction vertex (1,1)"
            );
        }
    }

    // -----------------------------------------------------------------------
    // 4. Non-adjacent polygons — no shared edges, each simplified independently
    // -----------------------------------------------------------------------

    #[test]
    fn test_non_adjacent_polygons() {
        let sq_a = make_polygon(vec![
            (0.0, 0.0),
            (1.0, 0.0),
            (1.0, 1.0),
            (0.0, 1.0),
            (0.0, 0.0),
        ]);
        let sq_b = make_polygon(vec![
            (5.0, 5.0),
            (6.0, 5.0),
            (6.0, 6.0),
            (5.0, 6.0),
            (5.0, 5.0),
        ]);

        let result = simplify_topology(&[sq_a.clone(), sq_b.clone()], 0.1);
        assert!(result.is_ok());
        let simplified = result.expect("simplify should succeed");
        assert_eq!(simplified.len(), 2);

        // Squares with no intermediate points should remain unchanged
        assert_eq!(simplified[0].exterior.coords.len(), 5);
        assert_eq!(simplified[1].exterior.coords.len(), 5);
    }

    // -----------------------------------------------------------------------
    // 5. Polygon with holes
    // -----------------------------------------------------------------------

    #[test]
    fn test_polygon_with_hole() {
        let outer = vec![
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 10.0),
            (0.0, 10.0),
            (0.0, 0.0),
        ];
        let hole = vec![(2.0, 2.0), (8.0, 2.0), (8.0, 8.0), (2.0, 8.0), (2.0, 2.0)];
        let poly = make_polygon_with_holes(outer, vec![hole]);

        let result = simplify_topology(&[poly], 0.5);
        assert!(result.is_ok());
        let simplified = result.expect("simplify should succeed");
        assert_eq!(simplified.len(), 1);
        assert_eq!(simplified[0].interiors.len(), 1);
        assert!(simplified[0].exterior.coords.len() >= 4);
        assert!(simplified[0].interiors[0].coords.len() >= 4);
    }

    // -----------------------------------------------------------------------
    // 6. Junction preservation — three polygons meeting at a single vertex
    // -----------------------------------------------------------------------

    #[test]
    fn test_junction_preservation() {
        // Three triangles meeting at (0,0)
        let t1 = make_polygon(vec![(0.0, 0.0), (2.0, 0.0), (1.0, 2.0), (0.0, 0.0)]);
        let t2 = make_polygon(vec![(0.0, 0.0), (1.0, 2.0), (-1.0, 2.0), (0.0, 0.0)]);
        let t3 = make_polygon(vec![(0.0, 0.0), (-1.0, 2.0), (-2.0, 0.0), (0.0, 0.0)]);

        let result = simplify_topology(&[t1, t2, t3], 0.1);
        assert!(result.is_ok());
        let simplified = result.expect("simplify should succeed");
        assert_eq!(simplified.len(), 3);

        // The junction vertex (0,0) must be present in all three
        for (i, p) in simplified.iter().enumerate() {
            let has_origin = p
                .exterior
                .coords
                .iter()
                .any(|c| c.x.abs() < 1e-10 && c.y.abs() < 1e-10);
            assert!(
                has_origin,
                "polygon {i} must preserve junction vertex (0,0)"
            );
        }
    }

    // -----------------------------------------------------------------------
    // 7. Self-intersection prevention
    // -----------------------------------------------------------------------

    #[test]
    fn test_self_intersection_prevention() {
        // A polygon with many intermediate points on each edge that should
        // simplify cleanly.
        let mut coords = Vec::new();
        // Bottom edge
        for i in 0..=20 {
            let x = i as f64 * 0.5;
            let y = if i % 2 == 1 { 0.01 } else { 0.0 };
            coords.push((x, y));
        }
        // Right edge
        coords.push((10.0, 10.0));
        // Top edge
        coords.push((0.0, 10.0));
        // Close
        coords.push((0.0, 0.0));

        let poly = make_polygon(coords);
        let result = simplify_topology(&[poly], 0.5);
        assert!(result.is_ok());
        let simplified = result.expect("simplify should succeed");
        assert_eq!(simplified.len(), 1);
        let ring = &simplified[0].exterior;
        assert!(!has_ring_self_intersection(ring));
    }

    // -----------------------------------------------------------------------
    // 8. Tolerance = 0 is a no-op
    // -----------------------------------------------------------------------

    #[test]
    fn test_tolerance_zero_noop() {
        let sq = make_polygon(vec![
            (0.0, 0.0),
            (1.0, 0.0),
            (1.0, 0.5),
            (1.0, 1.0),
            (0.0, 1.0),
            (0.0, 0.0),
        ]);
        let original_len = sq.exterior.coords.len();

        let result = simplify_topology(std::slice::from_ref(&sq), 0.0);
        assert!(result.is_ok());
        let simplified = result.expect("simplify should succeed");
        assert_eq!(simplified[0].exterior.coords.len(), original_len);
        assert_eq!(simplified[0].exterior.coords, sq.exterior.coords);
    }

    // -----------------------------------------------------------------------
    // 9. Negative tolerance is an error
    // -----------------------------------------------------------------------

    #[test]
    fn test_negative_tolerance_error() {
        let sq = make_polygon(vec![
            (0.0, 0.0),
            (1.0, 0.0),
            (1.0, 1.0),
            (0.0, 1.0),
            (0.0, 0.0),
        ]);
        let result = simplify_topology(&[sq], -1.0);
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // 10. Empty input
    // -----------------------------------------------------------------------

    #[test]
    fn test_empty_input() {
        let result = simplify_topology(&[], 1.0);
        assert!(result.is_ok());
        let simplified = result.expect("simplify should succeed");
        assert!(simplified.is_empty());
    }

    // -----------------------------------------------------------------------
    // 11. Single polygon — no shared edges
    // -----------------------------------------------------------------------

    #[test]
    fn test_single_polygon() {
        // Pentagon with intermediate collinear-ish points on one edge
        let poly = make_polygon(vec![
            (0.0, 0.0),
            (5.0, 0.0),
            (5.0, 2.5),
            (5.0, 5.0),
            (2.5, 5.0),
            (0.0, 5.0),
            (0.0, 0.0),
        ]);

        let result = simplify_topology(&[poly], 0.5);
        assert!(result.is_ok());
        let simplified = result.expect("simplify should succeed");
        assert_eq!(simplified.len(), 1);
        // The collinear intermediate points should be removed
        assert!(simplified[0].exterior.coords.len() <= 6);
        assert!(simplified[0].exterior.coords.len() >= 4);
    }

    // -----------------------------------------------------------------------
    // 12. Options — custom retry settings
    // -----------------------------------------------------------------------

    #[test]
    fn test_custom_options() {
        let sq_a = make_polygon(vec![
            (0.0, 0.0),
            (1.0, 0.0),
            (1.0, 1.0),
            (0.0, 1.0),
            (0.0, 0.0),
        ]);

        let opts = TopologySimplifyOptions {
            tolerance: 0.1,
            max_retries: 10,
            retry_factor: 0.25,
        };
        let result = simplify_topology_with_options(&[sq_a], &opts);
        assert!(result.is_ok());
        let simplified = result.expect("simplify should succeed");
        assert_eq!(simplified.len(), 1);
    }

    // -----------------------------------------------------------------------
    // 13. Shared edge consistency — identical simplified coords
    // -----------------------------------------------------------------------

    #[test]
    fn test_shared_edge_consistency() {
        // Two rectangles sharing a wavy edge at x=5.
        // Each has the same intermediate vertices along the shared edge.
        let mut left_coords = vec![(0.0, 0.0), (5.0, 0.0)];
        let mut right_coords = vec![(5.0, 0.0), (10.0, 0.0), (10.0, 10.0), (5.0, 10.0)];

        // Add wavy shared-edge points going up
        for i in 1..10 {
            let y = i as f64;
            let x = 5.0 + 0.01 * (i as f64 * 0.7).sin();
            left_coords.push((x, y));
        }
        left_coords.push((5.0, 10.0));
        left_coords.push((0.0, 10.0));
        left_coords.push((0.0, 0.0));

        // Right polygon has the same points in reverse
        for i in (1..10).rev() {
            let y = i as f64;
            let x = 5.0 + 0.01 * (i as f64 * 0.7).sin();
            right_coords.push((x, y));
        }
        right_coords.push((5.0, 0.0));

        let left = make_polygon(left_coords);
        let right = make_polygon(right_coords);

        let result = simplify_topology(&[left, right], 0.05);
        assert!(result.is_ok());
        let simplified = result.expect("simplify should succeed");

        // Gather shared-edge coords from each polygon
        let near_five_left: Vec<(f64, f64)> = simplified[0]
            .exterior
            .coords
            .iter()
            .filter(|c| (c.x - 5.0).abs() < 0.02)
            .map(|c| (c.x, c.y))
            .collect();
        let near_five_right: Vec<(f64, f64)> = simplified[1]
            .exterior
            .coords
            .iter()
            .filter(|c| (c.x - 5.0).abs() < 0.02)
            .map(|c| (c.x, c.y))
            .collect();

        // Sort by y to compare. Rings close by repeating the first coord; if
        // the repeated coord sits on the shared edge, dedup() collapses that
        // closure artifact so we compare the true set of shared vertices.
        let mut sorted_l = near_five_left;
        sorted_l.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        sorted_l.dedup();
        let mut sorted_r = near_five_right;
        sorted_r.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        sorted_r.dedup();

        assert_eq!(
            sorted_l, sorted_r,
            "shared edge must have identical simplified vertices"
        );
    }

    // -----------------------------------------------------------------------
    // 14. Large polygon with many points
    // -----------------------------------------------------------------------

    #[test]
    fn test_large_polygon_simplification() {
        // Circle approximation with 100 points
        let n = 100;
        let mut coords = Vec::with_capacity(n + 1);
        for i in 0..n {
            let angle = 2.0 * std::f64::consts::PI * (i as f64) / (n as f64);
            coords.push((10.0 * angle.cos(), 10.0 * angle.sin()));
        }
        coords.push(coords[0]); // close
        let poly = make_polygon(coords);

        let result = simplify_topology(&[poly], 0.5);
        assert!(result.is_ok());
        let simplified = result.expect("simplify should succeed");
        assert_eq!(simplified.len(), 1);
        // Should have significantly fewer points
        assert!(simplified[0].exterior.coords.len() < 80);
        assert!(simplified[0].exterior.coords.len() >= 4);
        assert!(!has_ring_self_intersection(&simplified[0].exterior));
    }

    // -----------------------------------------------------------------------
    // 15. Internal DP on coords
    // -----------------------------------------------------------------------

    #[test]
    fn test_dp_simplify_coords_straight() {
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 0.0),
            Coordinate::new_2d(2.0, 0.0),
            Coordinate::new_2d(3.0, 0.0),
        ];
        let simplified = dp_simplify_coords(&coords, 0.1);
        // Straight line => only endpoints
        assert_eq!(simplified.len(), 2);
        assert!(coords_exact(&simplified[0], &coords[0]));
        assert!(coords_exact(&simplified[1], &coords[3]));
    }

    #[test]
    fn test_dp_simplify_coords_zigzag() {
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(2.0, 0.0),
            Coordinate::new_2d(3.0, 1.0),
            Coordinate::new_2d(4.0, 0.0),
        ];
        // Small tolerance keeps all points
        let simplified = dp_simplify_coords(&coords, 0.01);
        assert_eq!(simplified.len(), 5);
    }

    // -----------------------------------------------------------------------
    // 16. Edge key ordering
    // -----------------------------------------------------------------------

    #[test]
    fn test_edge_key_canonical() {
        let a = Coordinate::new_2d(1.0, 2.0);
        let b = Coordinate::new_2d(3.0, 4.0);

        let (key_ab, rev_ab) = make_edge_key(&a, &b);
        let (key_ba, rev_ba) = make_edge_key(&b, &a);

        assert_eq!(
            key_ab, key_ba,
            "canonical keys must match regardless of direction"
        );
        assert_ne!(rev_ab, rev_ba, "reversal flags must differ");
    }

    // -----------------------------------------------------------------------
    // 17. Quantize round-trip
    // -----------------------------------------------------------------------

    #[test]
    fn test_quantize() {
        let c = Coordinate::new_2d(1.23456789, 9.87654321);
        let q = quantize(&c);
        assert_eq!(q, QPoint(1234568, 9876543));
    }

    // -----------------------------------------------------------------------
    // 18. Perpendicular distance
    // -----------------------------------------------------------------------

    #[test]
    fn test_perpendicular_distance_basic() {
        let p = Coordinate::new_2d(1.0, 1.0);
        let a = Coordinate::new_2d(0.0, 0.0);
        let b = Coordinate::new_2d(2.0, 0.0);
        let d = perpendicular_distance(&p, &a, &b);
        assert!((d - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_perpendicular_distance_zero_length_segment() {
        let p = Coordinate::new_2d(3.0, 4.0);
        let a = Coordinate::new_2d(0.0, 0.0);
        let d = perpendicular_distance(&p, &a, &a);
        assert!((d - 5.0).abs() < 1e-10);
    }

    // -----------------------------------------------------------------------
    // 19. has_ring_self_intersection
    // -----------------------------------------------------------------------

    #[test]
    fn test_no_self_intersection_square() {
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 0.0),
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(0.0, 1.0),
            Coordinate::new_2d(0.0, 0.0),
        ];
        let ring = LineString::new(coords).expect("valid ring");
        assert!(!has_ring_self_intersection(&ring));
    }

    #[test]
    fn test_self_intersection_bowtie() {
        // Bowtie shape: crosses itself
        let coords = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(2.0, 2.0),
            Coordinate::new_2d(2.0, 0.0),
            Coordinate::new_2d(0.0, 2.0),
            Coordinate::new_2d(0.0, 0.0),
        ];
        let ring = LineString::new(coords).expect("valid ring");
        assert!(has_ring_self_intersection(&ring));
    }

    // -----------------------------------------------------------------------
    // 20. Default options
    // -----------------------------------------------------------------------

    #[test]
    fn test_default_options() {
        let opts = TopologySimplifyOptions::default();
        assert!((opts.tolerance - 1.0).abs() < f64::EPSILON);
        assert_eq!(opts.max_retries, 5);
        assert!((opts.retry_factor - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_options_with_tolerance() {
        let opts = TopologySimplifyOptions::with_tolerance(2.5);
        assert!((opts.tolerance - 2.5).abs() < f64::EPSILON);
        assert_eq!(opts.max_retries, 5);
    }
}
