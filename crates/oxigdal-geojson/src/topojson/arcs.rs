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
/// Junctions are detected by comparing, for each vertex, the ordered pair
/// `(prev, next)` across all rings that contain it.  If two rings disagree on
/// the neighbours of a vertex, that vertex is a junction.  Additionally, the
/// first (and therefore last) vertex of every ring is always a junction.
pub(crate) fn detect_junctions(rings: &[Vec<QuantPoint>]) -> HashSet<QuantPoint> {
    // Map: vertex → canonical (prev, next) seen first
    let mut vertex_neighbours: HashMap<QuantPoint, (QuantPoint, QuantPoint)> = HashMap::new();
    let mut junctions: HashSet<QuantPoint> = HashSet::new();

    for ring in rings {
        let n = ring.len();
        if n == 0 {
            continue;
        }
        for i in 0..n {
            let v = ring[i];
            let prev = ring[(i + n - 1) % n];
            let next = ring[(i + 1) % n];
            match vertex_neighbours.get(&v) {
                Some(&(ep, en)) => {
                    if ep != prev || en != next {
                        junctions.insert(v);
                    }
                }
                None => {
                    vertex_neighbours.insert(v, (prev, next));
                }
            }
        }
        // The starting vertex of each ring is always a junction
        if !ring.is_empty() {
            junctions.insert(ring[0]);
        }
    }

    junctions
}

// ─── Arc extraction ──────────────────────────────────────────────────────────

/// Extract arcs from a list of normalised rings, deduplicating shared arcs.
///
/// Returns:
/// - `arcs`: the unique arcs as sequences of absolute `QuantPoint`s (not yet
///   delta-encoded).
/// - `ring_arc_indices`: for each ring, an ordered list of arc references.
///   A non-negative entry `i` means arc `i` is used in forward direction;
///   a negative entry `!(i as i32)` (bitwise NOT) means arc `i` is used in
///   reverse direction.
pub(crate) fn extract_arcs(
    rings: &[Vec<QuantPoint>],
    junctions: &HashSet<QuantPoint>,
) -> (Vec<Vec<QuantPoint>>, Vec<Vec<i32>>) {
    // Storage for unique arcs (stored in canonical, forward direction)
    let mut arcs: Vec<Vec<QuantPoint>> = Vec::new();
    // Map: canonical arc key → index in `arcs`
    let mut arc_index: HashMap<Vec<QuantPoint>, usize> = HashMap::new();
    // Arc indices for each input ring
    let mut ring_arc_indices: Vec<Vec<i32>> = Vec::with_capacity(rings.len());

    for ring in rings {
        let ring_refs = cut_ring_into_arcs(ring, junctions, &mut arcs, &mut arc_index);
        ring_arc_indices.push(ring_refs);
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
        let junctions = detect_junctions(&rings);
        assert!(junctions.contains(&(0, 0)));
    }

    #[test]
    fn detect_junctions_shared_vertex_different_neighbours() {
        // Two rings sharing vertex (1, 0) with different neighbours
        let r1 = vec![(0, 0), (1, 0), (1, 1)]; // neighbours of (1,0): (0,0) and (1,1)
        let r2 = vec![(1, 0), (2, 0), (2, 1)]; // neighbours of (1,0): (2,1) and (2,0)
        let rings = vec![r1, r2];
        let junctions = detect_junctions(&rings);
        // (1,0) appears in both rings with different neighbours → junction
        assert!(junctions.contains(&(1, 0)));
    }

    #[test]
    fn extract_arcs_single_ring() {
        let ring = vec![(0, 0), (1, 0), (1, 1)];
        let rings = vec![ring.clone()];
        let junctions = detect_junctions(&rings);
        let (arcs, ring_indices) = extract_arcs(&rings, &junctions);
        // Single closed ring → single arc
        assert_eq!(arcs.len(), 1);
        assert_eq!(ring_indices.len(), 1);
        assert_eq!(ring_indices[0].len(), 1);
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
