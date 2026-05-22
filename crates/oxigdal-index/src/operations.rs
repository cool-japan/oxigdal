//! Geometry operations on polygons and coordinate sequences.
//!
//! Provides area (shoelace), perimeter, centroid, point-in-polygon (ray
//! casting), Douglas–Peucker simplification, Visvalingam–Whyatt simplification,
//! Graham-scan convex hull, and bounding-box utilities.

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
// Visvalingam–Whyatt simplification
// ---------------------------------------------------------------------------

/// Triangle effective area for three consecutive vertices.
///
/// Returns the absolute area of the triangle formed by `p`, `q`, `r`.
#[inline]
fn triangle_area_vw(p: Coord, q: Coord, r: Coord) -> f64 {
    ((q.x - p.x) * (r.y - p.y) - (r.x - p.x) * (q.y - p.y)).abs() * 0.5
}

/// Whether two coordinates are equal within floating-point epsilon.
#[inline]
fn coords_equal(a: Coord, b: Coord) -> bool {
    (a.x - b.x).abs() < 1e-10 && (a.y - b.y).abs() < 1e-10
}

/// Doubly-linked-list state used by the VW heap algorithm.
struct VwState {
    /// Previous active vertex index (wraps for rings).
    prev: Vec<usize>,
    /// Next active vertex index (wraps for rings).
    next: Vec<usize>,
    /// Current effective area for each interior vertex.
    /// For endpoints: f64::INFINITY (never removed).
    areas: Vec<f64>,
    /// Version counter — incremented whenever a vertex's area is recomputed.
    versions: Vec<u64>,
    /// Number of currently active vertices.
    active: usize,
}

impl VwState {
    fn new(n: usize, is_ring: bool) -> Self {
        let prev: Vec<usize> = (0..n)
            .map(|i| {
                if i == 0 {
                    if is_ring { n - 2 } else { 0 }
                } else {
                    i - 1
                }
            })
            .collect();
        let next: Vec<usize> = (0..n)
            .map(|i| {
                if i == n - 1 {
                    if is_ring { 1 } else { n - 1 }
                } else {
                    i + 1
                }
            })
            .collect();
        // For a ring: last vertex is the duplicate of the first; treat vertex 0
        // and vertex n-1 as the same seam.  We mark n-1 removed and let n-2
        // link back to 1.  But it's simpler to just treat the ring as if the
        // closing vertex is permanent and update the linked list accordingly.
        VwState {
            prev,
            next,
            areas: vec![0.0; n],
            versions: vec![0; n],
            active: n,
        }
    }

    /// Compute the effective area for vertex `i` using its current neighbours.
    fn compute_area(&self, coords: &[Coord], i: usize) -> f64 {
        triangle_area_vw(coords[self.prev[i]], coords[i], coords[self.next[i]])
    }

    /// Remove vertex `i` from the linked list.
    fn remove(&mut self, i: usize) {
        let p = self.prev[i];
        let n = self.next[i];
        self.next[p] = n;
        self.prev[n] = p;
        self.active -= 1;
    }
}

/// Min-heap entry for the VW algorithm.
///
/// We encode area as raw `u64` bits using `f64::to_bits()` for total ordering.
/// This is correct for non-NaN, non-negative values: IEEE 754 positive floats
/// sort the same way when their bits are compared as unsigned integers.
#[derive(Debug, PartialEq, Eq)]
struct VwHeapEntry {
    /// Area encoded as u64 bits for Ord (valid for non-NaN non-negative f64).
    area_bits: u64,
    /// Index of the vertex this entry refers to.
    idx: usize,
    /// Version at the time this entry was pushed; stale if != state.versions[idx].
    version: u64,
}

impl VwHeapEntry {
    fn new(area: f64, idx: usize, version: u64) -> Self {
        VwHeapEntry {
            area_bits: area.to_bits(),
            idx,
            version,
        }
    }

    fn area(&self) -> f64 {
        f64::from_bits(self.area_bits)
    }
}

// We want a min-heap, so reverse the natural ordering.
impl PartialOrd for VwHeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for VwHeapEntry {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        // Smaller area_bits = smaller area = higher heap priority (min-heap via reverse).
        other
            .area_bits
            .cmp(&self.area_bits)
            .then(other.idx.cmp(&self.idx))
    }
}

/// Run the Visvalingam–Whyatt removal loop.
///
/// `stop_at_area`: stop removing vertices whose area >= this value.
/// `stop_at_count`: stop when active count would fall below this number.
/// Returns the indices of surviving vertices in original order.
fn vw_run(
    coords: &[Coord],
    is_ring: bool,
    stop_at_area: Option<f64>,
    stop_at_count: Option<usize>,
) -> Vec<usize> {
    use std::collections::BinaryHeap;

    let n = coords.len();

    // Effective stopping count: at minimum keep 2 (open line) or 3 (ring,
    // because ring closing means first==last so 3 gives a triangle).
    let min_keep = match stop_at_count {
        Some(k) => k.max(if is_ring { 3 } else { 2 }),
        None => {
            if is_ring {
                3
            } else {
                2
            }
        }
    };

    let mut state = VwState::new(n, is_ring);

    // For a ring the closing vertex (index n-1) is a duplicate of index 0.
    // We treat it as permanently inactive and let the doubly-linked list skip
    // from index n-2 to index 1 (wrapping around the ring).
    // Re-initialise the linked list for the ring case.
    if is_ring && n >= 4 {
        // Vertices: 0..=n-1 where coords[0] == coords[n-1] (by definition).
        // Interior vertices: 1..=n-2.
        // Treat vertex n-1 as already removed from the list — set prev/next
        // so that index n-2 links forward to index 1, and index 1 links
        // backward to index n-2.
        for i in 0..n {
            state.prev[i] = if i == 0 || i == n - 1 { n - 2 } else { i - 1 };
            state.next[i] = if i == 0 || i == n - 1 { 1 } else { i + 1 };
        }
        state.prev[1] = n - 2;
        state.next[n - 2] = 1;
        // Vertex n-1 is a phantom; don't count it.
        state.active = n - 1;
    }

    let mut heap: BinaryHeap<VwHeapEntry> = BinaryHeap::with_capacity(n);

    // Initialise areas and push interior vertices.
    // For open polyline: interior = 1..n-2.
    // For ring: interior = 1..n-2 (vertex 0 and n-1 are seam endpoints).
    // Endpoint vertices (0 and n-1 for open; 0 for ring) are never removed.
    // Interior vertices are 1..=n-2 for both open polylines and rings.
    // (For open polylines: 0 and n-1 are endpoints; for rings: 0 is the seam
    // anchor and n-1 is the phantom duplicate, both kept permanently.)
    let (first_interior, last_interior) = (1, n - 2);

    for i in first_interior..=last_interior {
        let area = state.compute_area(coords, i);
        state.areas[i] = area;
        heap.push(VwHeapEntry::new(area, i, 0));
    }

    // Track the maximum area removed so far (for monotonicity).
    let mut max_removed_area = 0.0_f64;

    loop {
        // Check count stopping condition first.
        let count_threshold = match stop_at_count {
            Some(k) => state.active <= k.max(min_keep),
            None => state.active <= min_keep,
        };
        if count_threshold {
            break;
        }

        let entry = match heap.pop() {
            Some(e) => e,
            None => break,
        };

        // Stale entry (vertex was already removed or area was recomputed).
        if entry.version != state.versions[entry.idx] {
            continue;
        }

        let vertex_area = entry.area();

        // Area stopping condition.
        if let Some(threshold) = stop_at_area
            && vertex_area >= threshold
        {
            break;
        }

        // Apply the monotonicity rule: effective removal area is the maximum
        // of the vertex's own area and all previously removed areas.
        let effective_area = vertex_area.max(max_removed_area);
        max_removed_area = effective_area;

        let idx = entry.idx;

        // Retrieve neighbours before removing.
        let prev_idx = state.prev[idx];
        let next_idx = state.next[idx];

        // Remove vertex from linked list.
        state.remove(idx);

        // Recompute neighbour areas, but only if they are interior vertices.
        // For open polylines: vertex 0 and n-1 are endpoints — never recomputed.
        // For rings: vertex 0 (and phantom n-1) are seam — never recomputed.
        let update_endpoints = [prev_idx, next_idx];
        for &nb in &update_endpoints {
            let is_endpoint = if is_ring {
                nb == 0
            } else {
                nb == 0 || nb == n - 1
            };
            if is_endpoint {
                continue;
            }
            // The neighbour's prev and next have changed; recompute its area.
            let new_area = state.compute_area(coords, nb);
            // Monotonicity: never allow a neighbour's effective area to be
            // less than the area of the vertex we just removed.
            let monotone_area = new_area.max(effective_area);
            state.areas[nb] = monotone_area;
            state.versions[nb] += 1;
            heap.push(VwHeapEntry::new(monotone_area, nb, state.versions[nb]));
        }
    }

    // Collect surviving vertices in original order.
    let mut result: Vec<usize> = (0..n)
        .filter(|&i| {
            if is_ring {
                // Skip the phantom closing vertex (n-1); it will be re-added.
                i != n - 1 && {
                    // A vertex is alive if its next/prev links are consistent
                    // (i.e., state.prev[state.next[i]] == i).
                    state.prev[state.next[i]] == i
                }
            } else {
                state.prev[state.next[i]] == i
                    || i == 0  // always keep first
                    || i == n - 1 // always keep last
            }
        })
        .collect();
    result.sort_unstable();

    // Ensure first and last are always present (safety net for open polylines).
    if !is_ring {
        if result.first() != Some(&0) {
            result.insert(0, 0);
        }
        if result.last() != Some(&(n - 1)) {
            result.push(n - 1);
        }
    }

    // For rings, ensure vertex 0 is present and re-add closing vertex.
    if is_ring && result.first() != Some(&0) {
        result.insert(0, 0);
    }

    result
}

/// Simplify a polyline using the Visvalingam–Whyatt algorithm.
///
/// VW removes the vertex that forms the triangle of smallest effective area
/// with its immediate neighbours, repeating until no remaining vertex has an
/// area smaller than `min_effective_area`.
///
/// Unlike Douglas–Peucker, VW better preserves the visual area of a polyline,
/// making it preferable for cartographic generalisation.
///
/// The first and last points are always retained.  If the input forms a closed
/// ring (`coords[0] == coords[n-1]`), the ring remains closed in the output.
///
/// # Parameters
///
/// * `coords` — input coordinate sequence (≥ 2 points).
/// * `min_effective_area` — area threshold; vertices with effective area
///   strictly below this value are removed.
pub fn simplify_visvalingam(coords: &[Coord], min_effective_area: f64) -> Vec<Coord> {
    let n = coords.len();
    if n <= 2 {
        return coords.to_vec();
    }

    // Detect whether the input is a closed ring.
    let is_ring = n >= 4 && coords_equal(coords[0], coords[n - 1]);

    let surviving = vw_run(coords, is_ring, Some(min_effective_area), None);

    let mut result: Vec<Coord> = surviving.iter().map(|&i| coords[i]).collect();

    // Re-append closing vertex for rings.
    if is_ring {
        result.push(coords[0]);
    }

    result
}

/// Simplify a polyline using Visvalingam–Whyatt, targeting an exact vertex count.
///
/// Removes vertices one at a time (smallest area first) until the result has
/// exactly `target_count` vertices.  If `target_count >= coords.len()` the
/// input is returned unchanged.
///
/// For closed rings, `target_count` refers to the total number of coordinates
/// including the closing duplicate; the minimum is 4 (triangle + closure).
///
/// # Parameters
///
/// * `coords` — input coordinate sequence.
/// * `target_count` — desired number of vertices in the output.
pub fn simplify_visvalingam_to_count(coords: &[Coord], target_count: usize) -> Vec<Coord> {
    let n = coords.len();
    if target_count >= n {
        return coords.to_vec();
    }

    let is_ring = n >= 4 && coords_equal(coords[0], coords[n - 1]);

    // Clamp target so we always have at least a valid geometry.
    let clamped_target = if is_ring {
        target_count.max(4) // ring: min 3 unique points + closure
    } else {
        target_count.max(2)
    };

    if clamped_target >= n {
        return coords.to_vec();
    }

    // For a ring the linked list tracks n-1 active vertices (phantom closing
    // vertex excluded), so we stop when active == clamped_target - 1.
    let linked_list_target = if is_ring {
        clamped_target - 1 // subtract the phantom closing vertex
    } else {
        clamped_target
    };

    let surviving = vw_run(coords, is_ring, None, Some(linked_list_target));

    let mut result: Vec<Coord> = surviving.iter().map(|&i| coords[i]).collect();

    // Re-append closing vertex for rings.
    if is_ring {
        result.push(coords[0]);
    }

    result
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
