//! Arc extraction, deduplication, and delta-encoding for TopoJSON.
//!
//! ## Algorithm overview
//!
//! 1. **Normalise** each ring: remove the duplicate closing vertex (the ring is
//!    stored open in TopoJSON).
//! 2. **Detect junctions**: a vertex is a junction when it is shared by two
//!    rings with *different* neighbours, or when it is the first vertex of any
//!    ring (always an arc endpoint).
//! 3. **Cut rings into arcs** at junctions, deduplicating using a canonical
//!    key (lexicographic minimum of the arc and its reverse).
//! 4. **Delta-encode** each arc: the first point is absolute; subsequent
//!    points are relative to the previous point.
//!
//! ## Arc reversal encoding
//!
//! Reverse arc index `i` is encoded as `!(i as i32)` per TopoJSON spec §2.1.4
//! — bitwise NOT, not negation.

use std::collections::{HashMap, HashSet};

use super::quantize::QuantPoint;

// ─── Normalise ───────────────────────────────────────────────────────────────

/// Remove the duplicate closing vertex from a ring so it is stored *open*.
///
/// A GeoJSON ring has its first and last positions identical; TopoJSON expects
/// an open ring (the arc processor closes it implicitly).
pub(crate) fn normalize_ring(ring: Vec<QuantPoint>) -> Vec<QuantPoint> {
    if ring.len() < 2 {
        return ring;
    }
    let last = ring.len() - 1;
    if ring[0] == ring[last] {
        ring[..last].to_vec()
    } else {
        ring
    }
}

// ─── Junction detection ──────────────────────────────────────────────────────

/// A junction is a vertex that must become an arc endpoint.
///
/// Paths may be *closed rings* (polygon boundaries) or *open chains*
/// (LineString / MultiLineString paths); the parallel `is_ring` slice flags
/// which is which.  Rings and chains are processed together so that arcs are
/// shared across both.
///
/// A vertex is a junction when, comparing its neighbours across every path that
/// passes through it, two paths disagree on the neighbour pair.  The comparison
/// is *unordered*: a path traversing the same two neighbours in the opposite
/// direction is **not** a junction (this is what allows a shared sub-path to be
/// reused as a single reversed arc).  In addition:
///
/// * the first vertex of every **ring** is always a junction (so each ring is
///   cut at a deterministic starting point), and
/// * both endpoints of every **chain** are always junctions (a line's ends are
///   always arc endpoints).
pub(crate) fn detect_junctions(paths: &[Vec<QuantPoint>], is_ring: &[bool]) -> HashSet<QuantPoint> {
    // Map: vertex → the (prev, next) neighbour pair first observed there.
    let mut vertex_neighbours: HashMap<QuantPoint, (QuantPoint, QuantPoint)> = HashMap::new();
    let mut junctions: HashSet<QuantPoint> = HashSet::new();

    for (path_idx, path) in paths.iter().enumerate() {
        let n = path.len();
        if n == 0 {
            continue;
        }
        let ring = is_ring.get(path_idx).copied().unwrap_or(true);
        if ring {
            // Closed ring: every vertex has *cyclic* neighbours.
            for i in 0..n {
                let v = path[i];
                let prev = path[(i + n - 1) % n];
                let next = path[(i + 1) % n];
                mark_neighbour(v, prev, next, &mut vertex_neighbours, &mut junctions);
            }
            // The starting vertex of each ring is always an arc endpoint.
            junctions.insert(path[0]);
        } else {
            // Open chain: both endpoints are always arc endpoints.
            junctions.insert(path[0]);
            junctions.insert(path[n - 1]);
            // Interior vertices use *linear* (non-wrapping) neighbours.
            for i in 1..n.saturating_sub(1) {
                let v = path[i];
                let prev = path[i - 1];
                let next = path[i + 1];
                mark_neighbour(v, prev, next, &mut vertex_neighbours, &mut junctions);
            }
        }
    }

    junctions
}

/// Record a vertex's `(prev, next)` neighbours, marking it a junction when a
/// later path passes through it with a genuinely different neighbour pair.
///
/// The comparison is *unordered*: a reversed traversal (`prev`/`next` swapped)
/// of the same neighbours does not create a junction.
fn mark_neighbour(
    v: QuantPoint,
    prev: QuantPoint,
    next: QuantPoint,
    vertex_neighbours: &mut HashMap<QuantPoint, (QuantPoint, QuantPoint)>,
    junctions: &mut HashSet<QuantPoint>,
) {
    match vertex_neighbours.get(&v) {
        Some(&(left, right)) => {
            let same_forward = left == prev && right == next;
            let same_reverse = left == next && right == prev;
            if !same_forward && !same_reverse {
                junctions.insert(v);
            }
        }
        None => {
            vertex_neighbours.insert(v, (prev, next));
        }
    }
}

// ─── Arc extraction ──────────────────────────────────────────────────────────

/// Extract arcs from a list of normalised paths, deduplicating shared arcs.
///
/// Each path is either a closed ring or an open chain, per the parallel
/// `is_ring` slice.  Rings are cut cyclically starting at their first junction;
/// chains are cut linearly between their endpoints.  Arcs are deduplicated with
/// a single canonical-key store shared across rings *and* chains, so a
/// sub-path common to a line and a polygon boundary is emitted exactly once.
///
/// Returns:
/// - `arcs`: the unique arcs as sequences of absolute `QuantPoint`s (not yet
///   delta-encoded).
/// - `ring_arc_indices`: for each input path, an ordered list of arc
///   references.  A non-negative entry `i` means arc `i` is used in the forward
///   direction; a negative entry `!(i as i32)` (bitwise NOT) means arc `i` is
///   used in reverse.
pub(crate) fn extract_arcs(
    paths: &[Vec<QuantPoint>],
    is_ring: &[bool],
    junctions: &HashSet<QuantPoint>,
) -> (Vec<Vec<QuantPoint>>, Vec<Vec<i32>>) {
    // Storage for unique arcs (stored in canonical, forward direction)
    let mut arcs: Vec<Vec<QuantPoint>> = Vec::new();
    // Map: canonical arc key → index in `arcs`
    let mut arc_index: HashMap<Vec<QuantPoint>, usize> = HashMap::new();
    // Arc indices for each input path
    let mut ring_arc_indices: Vec<Vec<i32>> = Vec::with_capacity(paths.len());

    for (path_idx, path) in paths.iter().enumerate() {
        let ring = is_ring.get(path_idx).copied().unwrap_or(true);
        let path_refs = if ring {
            cut_ring_into_arcs(path, junctions, &mut arcs, &mut arc_index)
        } else {
            cut_chain_into_arcs(path, junctions, &mut arcs, &mut arc_index)
        };
        ring_arc_indices.push(path_refs);
    }

    (arcs, ring_arc_indices)
}

/// Cut a single ring into arcs at junctions and return the signed arc index
/// sequence for this ring.
fn cut_ring_into_arcs(
    ring: &[QuantPoint],
    junctions: &HashSet<QuantPoint>,
    arcs: &mut Vec<Vec<QuantPoint>>,
    arc_index: &mut HashMap<Vec<QuantPoint>, usize>,
) -> Vec<i32> {
    let n = ring.len();
    if n == 0 {
        return Vec::new();
    }

    let mut ring_refs: Vec<i32> = Vec::new();

    // Find the first junction index to use as the starting point.
    // detect_junctions guarantees ring[0] is always a junction, so start=0.
    let start = find_start_junction_idx(ring, junctions).unwrap_or(0);

    // We walk the ring starting from `start`, completing one full loop.
    // Each time we hit a junction (other than the starting point of the current
    // arc segment), we close an arc.
    let mut current: Vec<QuantPoint> = vec![ring[start % n]];

    for step in 1..=n {
        let idx = (start + step) % n;
        current.push(ring[idx]);

        let at_junction = junctions.contains(&ring[idx]);
        let full_loop = step == n;

        if at_junction || full_loop {
            // The current arc segment is complete — commit it.
            let arc_ref = commit_arc(current, arcs, arc_index);
            ring_refs.push(arc_ref);

            if full_loop {
                break;
            }

            // Start the next segment from the current junction vertex.
            current = vec![ring[idx]];
        }
    }

    ring_refs
}

/// Cut an open chain (a LineString path) into arcs at junctions and return the
/// signed arc-index sequence for this chain.
///
/// Unlike [`cut_ring_into_arcs`], the chain is walked *linearly* from its first
/// to its last vertex with **no** rotation.  Both endpoints are guaranteed
/// junctions (see [`detect_junctions`]); every interior junction closes the
/// current arc and begins the next.  A chain that never touches an interior
/// junction becomes a single arc spanning its full length (this covers the
/// simple single-segment and closed-loop cases).
fn cut_chain_into_arcs(
    chain: &[QuantPoint],
    junctions: &HashSet<QuantPoint>,
    arcs: &mut Vec<Vec<QuantPoint>>,
    arc_index: &mut HashMap<Vec<QuantPoint>, usize>,
) -> Vec<i32> {
    let n = chain.len();
    if n < 2 {
        // A degenerate chain (empty or single point) contributes no arc.
        return Vec::new();
    }

    let mut chain_refs: Vec<i32> = Vec::new();
    let mut current: Vec<QuantPoint> = vec![chain[0]];

    for (i, &vertex) in chain.iter().enumerate().skip(1) {
        current.push(vertex);

        let at_junction = junctions.contains(&vertex);
        let at_end = i == n - 1;

        if at_junction || at_end {
            let segment = std::mem::take(&mut current);
            let arc_ref = commit_arc(segment, arcs, arc_index);
            chain_refs.push(arc_ref);

            if !at_end {
                // Start the next segment at this interior junction vertex.
                current.push(vertex);
            }
        }
    }

    chain_refs
}

/// Find the index of the first junction in a ring.
/// Since `detect_junctions` always marks `ring[0]` as a junction, this
/// returns `Some(0)` unless the ring is empty.
fn find_start_junction_idx(ring: &[QuantPoint], junctions: &HashSet<QuantPoint>) -> Option<usize> {
    ring.iter().position(|pt| junctions.contains(pt))
}

/// Canonicalise an arc (lexicographic minimum of forward/reverse), look it up
/// or insert it into the arc store, and return the signed arc reference.
///
/// Returns `idx as i32` if stored in forward direction,
/// `!(idx as i32)` (bitwise NOT) if stored reversed.
fn commit_arc(
    arc: Vec<QuantPoint>,
    arcs: &mut Vec<Vec<QuantPoint>>,
    arc_index: &mut HashMap<Vec<QuantPoint>, usize>,
) -> i32 {
    if arc.len() < 2 {
        // Degenerate single-point arc — store as-is, no reversal concept
        let idx = insert_arc(arc, arcs, arc_index);
        return idx as i32;
    }

    // Build reversed arc
    let reversed: Vec<QuantPoint> = arc.iter().rev().cloned().collect();

    // Choose the canonical form: lexicographic minimum
    let (canonical, is_reversed) = if reversed < arc {
        (reversed, true)
    } else {
        (arc, false)
    };

    let idx = insert_arc(canonical, arcs, arc_index);

    if is_reversed {
        !(idx as i32) // bitwise NOT per TopoJSON spec §2.1.4
    } else {
        idx as i32
    }
}

/// Insert an arc into the store if not already present, returning its index.
fn insert_arc(
    arc: Vec<QuantPoint>,
    arcs: &mut Vec<Vec<QuantPoint>>,
    arc_index: &mut HashMap<Vec<QuantPoint>, usize>,
) -> usize {
    if let Some(&idx) = arc_index.get(&arc) {
        return idx;
    }
    let idx = arcs.len();
    arc_index.insert(arc.clone(), idx);
    arcs.push(arc);
    idx
}

// ─── Delta encoding ──────────────────────────────────────────────────────────

/// Delta-encode an arc: the first point is absolute; subsequent points are
/// stored as deltas relative to the previous point.
///
/// This is the compact wire format required by the TopoJSON specification.
pub(crate) fn delta_encode(arc: &[QuantPoint]) -> Vec<[i32; 2]> {
    let mut result = Vec::with_capacity(arc.len());
    let mut prev = (0_i32, 0_i32);
    for &(x, y) in arc {
        result.push([x - prev.0, y - prev.1]);
        prev = (x, y);
    }
    result
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_ring_removes_closing_point() {
        let ring = vec![(0, 0), (1, 0), (1, 1), (0, 0)];
        let norm = normalize_ring(ring);
        assert_eq!(norm, vec![(0, 0), (1, 0), (1, 1)]);
    }

    #[test]
    fn normalize_ring_no_change_when_open() {
        let ring = vec![(0, 0), (1, 0), (1, 1)];
        let norm = normalize_ring(ring.clone());
        assert_eq!(norm, ring);
    }

    #[test]
    fn delta_encode_basic() {
        let arc = vec![(0, 0), (1, 2), (3, 1)];
        let encoded = delta_encode(&arc);
        assert_eq!(encoded, vec![[0, 0], [1, 2], [2, -1]]);
    }

    #[test]
    fn detect_junctions_marks_ring_start() {
        let rings = vec![vec![(0, 0), (1, 0), (1, 1)]];
        let junctions = detect_junctions(&rings, &[true]);
        assert!(junctions.contains(&(0, 0)));
    }

    #[test]
    fn detect_junctions_shared_vertex_different_neighbours() {
        // Two rings sharing vertex (1, 0) with different neighbours
        let r1 = vec![(0, 0), (1, 0), (1, 1)]; // neighbours of (1,0): (0,0) and (1,1)
        let r2 = vec![(1, 0), (2, 0), (2, 1)]; // neighbours of (1,0): (2,1) and (2,0)
        let rings = vec![r1, r2];
        let junctions = detect_junctions(&rings, &[true, true]);
        // (1,0) appears in both rings with different neighbours → junction
        assert!(junctions.contains(&(1, 0)));
    }

    #[test]
    fn extract_arcs_single_ring() {
        let ring = vec![(0, 0), (1, 0), (1, 1)];
        let rings = vec![ring.clone()];
        let junctions = detect_junctions(&rings, &[true]);
        let (arcs, ring_indices) = extract_arcs(&rings, &[true], &junctions);
        // Single closed ring → single arc
        assert_eq!(arcs.len(), 1);
        assert_eq!(ring_indices.len(), 1);
        assert_eq!(ring_indices[0].len(), 1);
    }

    #[test]
    fn detect_junctions_marks_chain_endpoints() {
        // A single open chain: both endpoints are junctions, interior is not.
        let paths = vec![vec![(0, 0), (1, 0), (2, 0)]];
        let junctions = detect_junctions(&paths, &[false]);
        assert!(junctions.contains(&(0, 0)), "start endpoint is a junction");
        assert!(junctions.contains(&(2, 0)), "end endpoint is a junction");
        assert!(
            !junctions.contains(&(1, 0)),
            "lone interior vertex is not a junction"
        );
    }

    #[test]
    fn single_segment_chain_is_one_arc() {
        let paths = vec![vec![(0, 0), (5, 3)]];
        let junctions = detect_junctions(&paths, &[false]);
        let (arcs, refs) = extract_arcs(&paths, &[false], &junctions);
        assert_eq!(arcs.len(), 1, "single segment → single arc");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].len(), 1, "chain references exactly one arc");
    }

    #[test]
    fn closed_line_chain_is_one_arc() {
        // A LineString whose first and last vertices coincide (a loop) is still
        // an open chain — it must NOT be normalised, and it becomes one arc.
        let paths = vec![vec![(0, 0), (2, 0), (2, 2), (0, 0)]];
        let junctions = detect_junctions(&paths, &[false]);
        let (arcs, refs) = extract_arcs(&paths, &[false], &junctions);
        assert_eq!(arcs.len(), 1);
        assert_eq!(refs[0].len(), 1);
        // The arc preserves the full loop including the closing vertex.
        assert_eq!(arcs[0].len(), 4);
    }

    #[test]
    fn two_chains_share_reversed_subpath_as_one_arc() {
        // Line 1: A - J1 - M - J2 - B  (forward through the shared J1-M-J2)
        // Line 2: C - J2 - M - J1 - D  (reverse through the shared J1-M-J2)
        let a = (0, 10);
        let j1 = (2, 0);
        let m = (3, 0);
        let j2 = (4, 0);
        let b = (6, 10);
        let c = (6, -10);
        let d = (0, -10);
        let paths = vec![vec![a, j1, m, j2, b], vec![c, j2, m, j1, d]];
        let is_ring = [false, false];
        let junctions = detect_junctions(&paths, &is_ring);
        // M is interior to the shared sub-path in *both* lines (reversed) → not a junction.
        assert!(
            !junctions.contains(&m),
            "shared interior vertex is not a junction"
        );
        assert!(junctions.contains(&j1));
        assert!(junctions.contains(&j2));

        let (arcs, refs) = extract_arcs(&paths, &is_ring, &junctions);
        // The shared J1-M-J2 arc must be stored exactly once.
        let shared_present = arcs.iter().any(|arc| arc.len() == 3 && arc.contains(&m));
        assert!(shared_present, "shared 3-point arc stored once");

        // Exactly one referencing line uses the shared arc reversed (negative index).
        let all_refs: Vec<i32> = refs.iter().flatten().copied().collect();
        assert!(
            all_refs.iter().any(|&r| r < 0),
            "one line references the shared arc with a negative (reversed) index"
        );
    }

    #[test]
    fn commit_arc_reversal_encoding() {
        // The reversed arc should produce !(idx) = -1 for idx=0
        let arc_fwd = vec![(0, 0), (1, 1), (2, 0)];
        let arc_rev = vec![(2, 0), (1, 1), (0, 0)];
        let mut arcs: Vec<Vec<QuantPoint>> = Vec::new();
        let mut arc_index: HashMap<Vec<QuantPoint>, usize> = HashMap::new();

        let ref1 = commit_arc(arc_fwd, &mut arcs, &mut arc_index);
        let ref2 = commit_arc(arc_rev, &mut arcs, &mut arc_index);

        // Both point to the same arc (index 0), one forward, one reversed
        assert_eq!(arcs.len(), 1);
        // One should be 0 (forward), the other !0 = -1 (reversed)
        assert!(ref1 == 0 || ref1 == !0_i32);
        assert!(ref2 == 0 || ref2 == !0_i32);
        assert_ne!(ref1, ref2);
    }
}
