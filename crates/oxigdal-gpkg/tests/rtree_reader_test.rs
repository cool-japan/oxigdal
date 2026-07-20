//! Integration tests for the GeoPackage R-tree shadow-table reader.
//!
//! These tests exercise [`GpkgRTreeReader`] entirely in memory by constructing
//! synthetic node BLOBs via the crate-internal builder helpers.  No real
//! SQLite / GeoPackage file is required.
//!
//! The seven test cases cover:
//! 1. Empty reader — no nodes → empty result set.
//! 2. Single leaf node with one intersecting entry.
//! 3. Single leaf node with a disjoint entry (miss).
//! 4. Multiple entries with partial overlap.
//! 5. Boundary-touching entries are included (inclusive semantics).
//! 6. Two-level tree: interior root → leaf child.
//! 7. Large entry count (1 000 entries) with range query.

use std::collections::HashMap;

use oxigdal_gpkg::rtree::{GpkgRTreeReader, build_interior_node_blob, build_leaf_node_blob};

// ─────────────────────────────────────────────────────────────────────────────
// Test helper
// ─────────────────────────────────────────────────────────────────────────────

/// Build a [`GpkgRTreeReader`] that contains a single leaf root node (node 1)
/// populated with the given entries.
///
/// `entries` is a slice of `(rowid, min_x, max_x, min_y, max_y)`.
fn single_leaf_reader(entries: &[(i64, f32, f32, f32, f32)]) -> GpkgRTreeReader {
    let blob = build_leaf_node_blob(1, entries);
    let mut nodes = HashMap::new();
    nodes.insert(1i64, blob);
    GpkgRTreeReader::for_testing(nodes, 1)
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 1 — empty reader
// ─────────────────────────────────────────────────────────────────────────────

/// An empty [`GpkgRTreeReader`] (no node blobs at all) must return an empty
/// result set for any query.
#[test]
fn test_rtree_empty_reader_returns_empty_results() {
    let reader = GpkgRTreeReader::for_testing(HashMap::new(), 0);

    assert!(
        reader.is_empty(),
        "reader with no nodes must report is_empty()"
    );
    assert_eq!(reader.len(), 0);
    assert_eq!(reader.max_node_id(), 0);

    let results = reader.search(-180.0, -90.0, 180.0, 90.0);
    assert!(
        results.is_empty(),
        "search on empty reader must return empty Vec; got {results:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2 — single leaf, intersecting entry is found
// ─────────────────────────────────────────────────────────────────────────────

/// A leaf node with one entry at [1.0, 3.0] × [2.0, 4.0] must be found by a
/// query window [2.0, 5.0] × [3.0, 5.0].
#[test]
fn test_rtree_leaf_node_finds_intersecting_entry() {
    // entry: rowid=42, min_x=1.0, max_x=3.0, min_y=2.0, max_y=4.0
    let reader = single_leaf_reader(&[(42, 1.0, 3.0, 2.0, 4.0)]);

    // query: min_x=2.0, min_y=3.0, max_x=5.0, max_y=5.0
    let results = reader.search(2.0, 3.0, 5.0, 5.0);

    assert_eq!(
        results,
        vec![42],
        "entry [1,3]×[2,4] must intersect query [2,5]×[3,5]; got {results:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3 — disjoint entry is missed
// ─────────────────────────────────────────────────────────────────────────────

/// A feature at [10, 11] × [10, 11] must NOT appear in a query for [0, 5] ×
/// [0, 5].
#[test]
fn test_rtree_leaf_node_misses_disjoint_entry() {
    let reader = single_leaf_reader(&[(99, 10.0, 11.0, 10.0, 11.0)]);

    let results = reader.search(0.0, 0.0, 5.0, 5.0);

    assert!(
        results.is_empty(),
        "disjoint entry [10,11]×[10,11] must not appear in [0,5]×[0,5]; got {results:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 4 — multiple entries, partial overlap
// ─────────────────────────────────────────────────────────────────────────────

/// Five features at x = [0,1], [1,2], [2,3], [3,4], [4,5] (all y = [0,1]).
/// A query for x = [2, 4] must return exactly the three entries that overlap:
/// rowids 2, 3, 4  (x-ranges [1,2]→touches, [2,3], [3,4]).
///
/// The query is min_x=2.0, max_x=4.0:
/// * rowid 0: [0,1] → max_x=1 < min_x=2 → miss
/// * rowid 1: [1,2] → max_x=2 == min_x=2 → hit (boundary)
/// * rowid 2: [2,3] → overlap → hit
/// * rowid 3: [3,4] → overlap → hit
/// * rowid 4: [4,5] → min_x=4 == max_x=4 → hit (boundary)
///
/// Four hits: rowids 1, 2, 3, 4.
#[test]
fn test_rtree_multiple_entries_partial_overlap() {
    // Build 5 entries with rowid = index, x = [i, i+1], y = [0, 1].
    let entries: Vec<(i64, f32, f32, f32, f32)> = (0..5)
        .map(|i| (i as i64, i as f32, (i + 1) as f32, 0.0f32, 1.0f32))
        .collect();

    let reader = single_leaf_reader(&entries);

    let mut results = reader.search(2.0, 0.0, 4.0, 1.0);
    results.sort_unstable();

    // Boundary-inclusive: rowids 1,2,3,4 all overlap or touch [2,4].
    let expected: Vec<i64> = vec![1, 2, 3, 4];
    assert_eq!(
        results, expected,
        "query [2,4]×[0,1] must return rowids 1-4; got {results:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 5 — boundary-touching entries are included
// ─────────────────────────────────────────────────────────────────────────────

/// An entry whose bbox coincides exactly with the query boundary must be
/// included in the result (inclusive semantics).
#[test]
fn test_rtree_boundary_touching_is_included() {
    // Entry bbox = query bbox (exact match).
    let reader = single_leaf_reader(&[(7, 3.0, 7.0, -1.0, 1.0)]);

    // Query exactly matches the entry bounds.
    let results = reader.search(3.0, -1.0, 7.0, 1.0);
    assert_eq!(
        results,
        vec![7],
        "entry whose bbox exactly equals the query window must be returned; got {results:?}"
    );

    // Query whose boundary is the right edge of the entry.
    let results = reader.search(7.0, 0.0, 10.0, 2.0);
    assert_eq!(
        results,
        vec![7],
        "entry max_x == query min_x (touching) must be returned; got {results:?}"
    );

    // Query that just misses: query min_x is beyond entry max_x.
    let results = reader.search(7.001, 0.0, 10.0, 2.0);
    assert!(
        results.is_empty(),
        "entry max_x < query min_x must be excluded; got {results:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 6 — two-level tree: interior root → leaf child
// ─────────────────────────────────────────────────────────────────────────────

/// Construct a 2-level R-tree:
/// * Node 1 (root, interior): one cell pointing to node 2, MBR = [0,10]×[0,10]
/// * Node 2 (leaf): one entry, rowid=55, bbox = [3,6]×[3,6]
///
/// A query for [4,5]×[4,5] must traverse the interior node and find rowid 55.
#[test]
fn test_rtree_interior_node_routes_to_leaf() {
    // Node 2 is a leaf with one entry.
    let leaf_blob = build_leaf_node_blob(2, &[(55, 3.0, 6.0, 3.0, 6.0)]);

    // Node 1 is an interior node (root, depth 1: one level above the leaf)
    // with one cell pointing to node 2.
    // Cell format reuses build_interior_node_blob: (child_node_id, min_x, max_x, min_y, max_y).
    let root_blob = build_interior_node_blob(1, 1, &[(2, 0.0, 10.0, 0.0, 10.0)]);

    let mut nodes = HashMap::new();
    nodes.insert(1i64, root_blob);
    nodes.insert(2i64, leaf_blob);

    // Leaf/interior classification is driven by the root's declared depth
    // (encoded in root_blob's header above), not by max_node_id; the second
    // argument here only feeds the informational max_node_id() accessor.
    let reader = GpkgRTreeReader::for_testing(nodes, 2);

    assert_eq!(reader.len(), 2, "reader must hold 2 nodes");

    let results = reader.search(4.0, 4.0, 5.0, 5.0);
    assert_eq!(
        results,
        vec![55],
        "query must route through interior node and find leaf entry 55; got {results:?}"
    );

    // A query that misses the interior cell's MBR must not reach the leaf.
    let results = reader.search(20.0, 20.0, 30.0, 30.0);
    assert!(
        results.is_empty(),
        "query outside root MBR must return empty; got {results:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 7 — 1 000 entries, range query
// ─────────────────────────────────────────────────────────────────────────────

/// Build a single-leaf reader with 1 000 entries: entry i has
/// `min_x = i, max_x = i + 1, min_y = 0, max_y = 1`.
///
/// A query for x = [400, 600] must return exactly the entries whose
/// x-interval overlaps [400, 600] (boundary-inclusive), which are
/// i = 399 (`max_x=400`), 400 through 600 (`overlap`), and no i=601
/// (min_x=601 > max_x=600).
///
/// Wait — let me recount:
/// * i=399: min_x=399, max_x=400 → max_x=400 >= min_x=400 ✓
/// * i=400: min_x=400, max_x=401 → overlap ✓
/// * ...
/// * i=600: min_x=600, max_x=601 → min_x=600 <= max_x=600 ✓
/// * i=601: min_x=601, max_x=602 → min_x=601 > max_x=600 ✗
///
/// So hits: i=399, 400, …, 600  → 202 entries.
#[test]
fn test_rtree_1000_entries_correctness() {
    let entries: Vec<(i64, f32, f32, f32, f32)> = (0..1000i64)
        .map(|i| (i, i as f32, (i + 1) as f32, 0.0f32, 1.0f32))
        .collect();

    let reader = single_leaf_reader(&entries);
    assert_eq!(reader.len(), 1, "must have exactly 1 node");

    let mut results = reader.search(400.0, 0.0, 600.0, 1.0);
    results.sort_unstable();

    // i=399 through i=600 inclusive → 202 entries.
    let expected_count = 202usize;
    assert_eq!(
        results.len(),
        expected_count,
        "expected {expected_count} results for x=[400,600], got {}; first few: {:?}",
        results.len(),
        &results[..results.len().min(10)]
    );

    // Verify the exact rowid range.
    assert_eq!(results[0], 399, "first result must be rowid 399");
    assert_eq!(results[201], 600, "last result must be rowid 600");

    // Spot-check that the neighbours are excluded.
    let results_narrow = reader.search(400.0001, 0.0, 599.9999, 1.0);
    // Only i=400..=599 overlap this slightly tighter window (max_x=400 < 400.0001):
    // i=399: max_x=400 < 400.0001 → miss
    // i=400: min_x=400 <= 400.0001 AND max_x=401 >= 400.0001 → hit
    // i=600: min_x=600 > 599.9999 → miss
    assert!(
        !results_narrow.contains(&399),
        "rowid 399 must be excluded from tighter window"
    );
    assert!(
        !results_narrow.contains(&600),
        "rowid 600 must be excluded from tighter window"
    );
    assert!(
        results_narrow.contains(&400),
        "rowid 400 must be included in tighter window"
    );
}
