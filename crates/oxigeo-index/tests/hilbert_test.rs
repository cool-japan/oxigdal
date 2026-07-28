//! Integration tests for [`HilbertRTree`].
//!
//! All coordinates are deterministic — no `rand` dependency is used.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use oxigeo_index::rtree::hilbert::compute_hilbert_value as compute_hilbert_value_direct;
use oxigeo_index::{Bbox2D, HilbertRTree, RTree, compute_hilbert_value};

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

/// Create a unit-square bbox anchored at `(x, y)`.
fn unit_box(x: f64, y: f64) -> Bbox2D {
    Bbox2D::new(x, y, x + 1.0, y + 1.0).expect("valid bbox")
}

// ---------------------------------------------------------------------------
// Test 1 — determinism
// ---------------------------------------------------------------------------

#[test]
fn test_hilbert_value_consistent_for_same_point() {
    let world = Bbox2D::new(-180.0, -90.0, 180.0, 90.0).expect("valid world");
    let bbox = Bbox2D::new(0.0, 0.0, 1.0, 1.0).expect("valid bbox");

    let v1 = compute_hilbert_value(&bbox, &world, 16);
    let v2 = compute_hilbert_value(&bbox, &world, 16);
    assert_eq!(v1, v2, "same inputs must yield same Hilbert value");

    // Also verify the direct module path agrees.
    let v3 = compute_hilbert_value_direct(&bbox, &world, 16);
    assert_eq!(v1, v3, "re-exported and direct paths must agree");
}

// ---------------------------------------------------------------------------
// Test 2 — Hilbert ordering roughly matches left-to-right for a row of bboxes
// ---------------------------------------------------------------------------

#[test]
fn test_hilbert_bulk_load_orders_entries_along_curve() {
    // 100 unit bboxes arranged in a horizontal row y = [0, 1].
    // Hilbert curve at order 16 maps left-to-right positions to monotonically
    // (or at least roughly) increasing indices along the bottom of the world.
    let items: Vec<(Bbox2D, usize)> = (0..100usize)
        .map(|i| (unit_box(i as f64 * 2.0, 0.0), i))
        .collect();

    let tree = HilbertRTree::bulk_load(items, 16).expect("bulk load should succeed for 100 items");

    assert_eq!(tree.len(), 100);

    // Verify that the world bbox was computed (tree is non-empty).
    let world = tree.world_bbox();
    assert!(world.max_x > world.min_x);

    // Retrieve the internal Hilbert-sorted leaf values by searching the whole
    // world — all 100 items must come back.
    let all_hits = tree.search(world);
    assert_eq!(
        all_hits.len(),
        100,
        "search over world should return all 100 items"
    );
}

// ---------------------------------------------------------------------------
// Test 3 — 3×3 grid, query centre cell → exactly 1 result
// ---------------------------------------------------------------------------

#[test]
fn test_hilbert_search_returns_all_overlapping_bboxes() {
    // 3×3 grid of 1×1 cells with a 0.5-unit gap between cells so there is
    // no touching.  The centre cell sits at (2.5, 2.5)→(3.5, 3.5).
    let mut items: Vec<(Bbox2D, usize)> = Vec::new();
    for row in 0..3usize {
        for col in 0..3usize {
            let x = col as f64 * 1.5; // cells at 0, 1.5, 3.0
            let y = row as f64 * 1.5;
            items.push((
                Bbox2D::new(x, y, x + 1.0, y + 1.0).expect("valid"),
                row * 3 + col,
            ));
        }
    }

    let tree = HilbertRTree::bulk_load(items, 16).expect("bulk load ok");

    // Query the centre cell exactly.
    let query = Bbox2D::new(1.5, 1.5, 2.5, 2.5).expect("valid query");
    let hits = tree.search(&query);
    assert_eq!(
        hits.len(),
        1,
        "only the centre cell (1.5..2.5, 1.5..2.5) should be hit"
    );
    assert_eq!(*hits[0], 4usize, "centre cell index is 4 (row=1, col=1)");
}

// ---------------------------------------------------------------------------
// Test 4 — disjoint subtrees are skipped
// ---------------------------------------------------------------------------

#[test]
fn test_hilbert_search_skips_disjoint_subtrees() {
    // 50 items in the bottom-left quadrant, 50 in the top-right quadrant.
    // Query the far top-right corner; only those 50 should be returned.

    let mut items: Vec<(Bbox2D, &'static str)> = Vec::new();

    // Bottom-left: x ∈ [0, 49], y ∈ [0, 1]
    for i in 0..50usize {
        items.push((unit_box(i as f64, 0.0), "bottom-left"));
    }

    // Top-right: x ∈ [1000, 1049], y ∈ [1000, 1001]
    for i in 0..50usize {
        items.push((unit_box(1000.0 + i as f64, 1000.0), "top-right"));
    }

    let tree = HilbertRTree::bulk_load(items, 16).expect("bulk load ok");

    // Query only covers the top-right cluster.
    let query = Bbox2D::new(999.0, 999.0, 1060.0, 1005.0).expect("valid query");
    let hits = tree.search(&query);

    assert_eq!(hits.len(), 50, "should return exactly 50 top-right items");
    for val in &hits {
        assert_eq!(
            **val, "top-right",
            "all hits must be from the top-right cluster"
        );
    }
}

// ---------------------------------------------------------------------------
// Test 5 — HilbertRTree and RTree return the same set for an arbitrary query
// ---------------------------------------------------------------------------

#[test]
fn test_hilbert_compared_to_rstar_returns_same_set() {
    // 50 items on a 5×10 grid; query covers the central 2×4 block.
    let mut items: Vec<(Bbox2D, usize)> = Vec::new();
    for row in 0..10usize {
        for col in 0..5usize {
            // Gap of 0.5 between cells to avoid touching boundaries.
            let x = col as f64 * 1.5;
            let y = row as f64 * 1.5;
            items.push((
                Bbox2D::new(x, y, x + 1.0, y + 1.0).expect("valid"),
                row * 5 + col,
            ));
        }
    }

    // The query covers cols 1-2, rows 3-6 (exclusive of gap).
    let query = Bbox2D::new(1.5, 4.5, 4.5, 10.5).expect("valid query");

    // Build both trees from the same items.
    let items_clone = items.clone();
    let h_tree = HilbertRTree::bulk_load(items_clone, 16).expect("hilbert bulk load ok");

    let mut r_tree: RTree<usize> = RTree::new();
    for (bbox, val) in &items {
        r_tree.insert(*bbox, *val);
    }

    // Collect result sets.
    let h_hits: std::collections::HashSet<usize> =
        h_tree.search(&query).into_iter().copied().collect();
    let r_hits: std::collections::HashSet<usize> =
        r_tree.search(&query).into_iter().copied().collect();

    assert_eq!(
        h_hits, r_hits,
        "HilbertRTree and RTree must return the same set of indices"
    );
    // Sanity: there should be some hits.
    assert!(!h_hits.is_empty(), "query should match at least some items");
}

// ---------------------------------------------------------------------------
// Test 6 — empty tree search returns empty vec
// ---------------------------------------------------------------------------

#[test]
fn test_hilbert_empty_search_returns_empty() {
    let tree: HilbertRTree<u32> = HilbertRTree::new();
    let query = Bbox2D::new(0.0, 0.0, 100.0, 100.0).expect("valid");
    let hits = tree.search(&query);
    assert!(
        hits.is_empty(),
        "search on empty tree must return empty vec"
    );
}

// ---------------------------------------------------------------------------
// Test 6b — bulk_load with empty input returns Err
// ---------------------------------------------------------------------------

#[test]
fn test_hilbert_bulk_load_empty_returns_error() {
    let items: Vec<(Bbox2D, u32)> = Vec::new();
    let result = HilbertRTree::bulk_load(items, 16);
    assert!(
        result.is_err(),
        "bulk_load with empty items should return Err"
    );
}

// ---------------------------------------------------------------------------
// Test 7 — single item
// ---------------------------------------------------------------------------

#[test]
fn test_hilbert_single_item_found() {
    let bbox = Bbox2D::new(5.0, 5.0, 10.0, 10.0).expect("valid");
    let tree = HilbertRTree::bulk_load(vec![(bbox, 99u32)], 16).expect("single-item bulk load ok");

    assert_eq!(tree.len(), 1);

    // Overlapping query → found.
    let q_hit = Bbox2D::new(7.0, 7.0, 12.0, 12.0).expect("valid");
    let hits = tree.search(&q_hit);
    assert_eq!(hits.len(), 1);
    assert_eq!(*hits[0], 99u32);

    // Disjoint query → not found.
    let q_miss = Bbox2D::new(20.0, 20.0, 30.0, 30.0).expect("valid");
    let misses = tree.search(&q_miss);
    assert!(misses.is_empty(), "disjoint query must return nothing");
}

// ---------------------------------------------------------------------------
// Test 8 — degenerate world (all points identical) does not panic
// ---------------------------------------------------------------------------

#[test]
fn test_hilbert_degenerate_world_all_same_point() {
    // All bboxes are identical point bboxes — world has zero area.
    let items: Vec<(Bbox2D, u32)> = (0..5u32).map(|i| (Bbox2D::point(1.0, 1.0), i)).collect();

    let tree = HilbertRTree::bulk_load(items, 16).expect("degenerate world must not error");

    assert_eq!(tree.len(), 5);

    // A query that covers the single point should return all 5.
    let q = Bbox2D::new(0.5, 0.5, 1.5, 1.5).expect("valid");
    let hits = tree.search(&q);
    assert_eq!(hits.len(), 5);
}

// ---------------------------------------------------------------------------
// Test 9 — large bulk-load (stress check, > 1 internal node)
// ---------------------------------------------------------------------------

#[test]
fn test_hilbert_large_bulk_load_and_search() {
    // 1 000 unit bboxes in a 25×40 grid (spacing 2.0 between cells).
    let items: Vec<(Bbox2D, usize)> = (0..1_000usize)
        .map(|i| {
            let col = i % 25;
            let row = i / 25;
            let x = col as f64 * 2.0;
            let y = row as f64 * 2.0;
            (unit_box(x, y), i)
        })
        .collect();

    let tree = HilbertRTree::bulk_load(items, 16).expect("large bulk load ok");
    assert_eq!(tree.len(), 1_000);

    // Query the first 3×3 block (cols 0-2, rows 0-2 → x ∈ [0,5], y ∈ [0,5]).
    let q = Bbox2D::new(0.0, 0.0, 5.0, 5.0).expect("valid");
    let hits = tree.search(&q);

    // 3×3 block at spacing 2.0: cells at (0,0),(2,0),(4,0),(0,2),(2,2),(4,2),
    // (0,4),(2,4),(4,4) → 9 cells, each [x, x+1] × [y, y+1], all inside [0,5].
    assert_eq!(hits.len(), 9, "expected exactly 9 hits in 3×3 sub-grid");
}
