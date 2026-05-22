//! Integration tests for `RTree::search_top_k` and
//! `SpatialQuery::top_k_in_window`.
//!
//! These tests verify:
//!  - correctness of result count and ordering,
//!  - edge cases (k = 0, empty tree, fewer candidates than k),
//!  - window filtering (items outside the window are excluded),
//!  - that the item at the window centre has distance 0 and ranks first,
//!  - parity between `RTree::search_top_k` and `SpatialQuery::top_k_in_window`.

#![allow(clippy::unwrap_used, missing_docs)]

use oxigdal_index::{Bbox2D, RTree, SpatialQuery};

// ---------------------------------------------------------------------------
// Helper: build a bbox centred at (cx, cy) with given half-widths.
// ---------------------------------------------------------------------------

fn bbox_at(cx: f64, cy: f64, hw: f64, hh: f64) -> Bbox2D {
    Bbox2D::new(cx - hw, cy - hh, cx + hw, cy + hh).unwrap()
}

// ---------------------------------------------------------------------------
// Test 1 — search_top_k returns exactly k closest results
// ---------------------------------------------------------------------------

#[test]
fn test_search_top_k_returns_k_closest() {
    let mut tree: RTree<u32> = RTree::new();

    // Insert 10 items at known positions.  The world window covers all of them.
    for i in 0u32..10 {
        let cx = (i as f64) * 10.0;
        let b = bbox_at(cx, 0.0, 1.0, 1.0);
        tree.insert(b, i);
    }

    // The world bbox covers everything.
    let world = Bbox2D::new(-5.0, -5.0, 105.0, 5.0).unwrap();
    let result = tree.search_top_k(&world, 3);

    assert_eq!(result.len(), 3, "expect exactly 3 results");
}

// ---------------------------------------------------------------------------
// Test 2 — k = 0 yields an empty result
// ---------------------------------------------------------------------------

#[test]
fn test_search_top_k_zero_k_returns_empty() {
    let mut tree: RTree<u32> = RTree::new();
    let b = bbox_at(1.0, 1.0, 0.5, 0.5);
    tree.insert(b, 99);

    let world = Bbox2D::new(-10.0, -10.0, 10.0, 10.0).unwrap();
    let result = tree.search_top_k(&world, 0);

    assert!(result.is_empty(), "k=0 must return empty vec");
}

// ---------------------------------------------------------------------------
// Test 3 — fewer candidates than k → return all of them
// ---------------------------------------------------------------------------

#[test]
fn test_search_top_k_fewer_than_k_in_window() {
    let mut tree: RTree<u32> = RTree::new();

    // Insert only 2 items inside a small window.
    tree.insert(bbox_at(1.0, 1.0, 0.3, 0.3), 1);
    tree.insert(bbox_at(2.0, 1.0, 0.3, 0.3), 2);
    // Insert a third item far outside the query window.
    tree.insert(bbox_at(100.0, 100.0, 0.3, 0.3), 3);

    let window = Bbox2D::new(0.0, 0.0, 5.0, 5.0).unwrap();
    let result = tree.search_top_k(&window, 10);

    assert_eq!(result.len(), 2, "only 2 items are inside the window");
}

// ---------------------------------------------------------------------------
// Test 4 — empty tree returns empty vec regardless of k
// ---------------------------------------------------------------------------

#[test]
fn test_search_top_k_empty_tree() {
    let tree: RTree<u32> = RTree::new();
    let world = Bbox2D::new(-100.0, -100.0, 100.0, 100.0).unwrap();

    for k in [0usize, 1, 5, 100] {
        let result = tree.search_top_k(&world, k);
        assert!(
            result.is_empty(),
            "empty tree must always yield empty result (k={k})"
        );
    }
}

// ---------------------------------------------------------------------------
// Test 5 — distances in the result are non-decreasing (sorted ascending)
// ---------------------------------------------------------------------------

#[test]
fn test_search_top_k_sorted_ascending_distance() {
    let mut tree: RTree<u32> = RTree::new();

    // Insert items at increasing distances from (5, 5) — the window centre.
    let window = Bbox2D::new(0.0, 0.0, 10.0, 10.0).unwrap();
    let (cx, cy) = window.center(); // (5.0, 5.0)

    // Items at distances 0, 1, 2, 3, 4, 5 (measured bbox-to-centre).
    // Item with dist 0: covers the centre.
    tree.insert(Bbox2D::new(4.0, 4.0, 6.0, 6.0).unwrap(), 0u32);
    // Items outside the centre with increasing x offset.
    for offset in 1u32..=5 {
        // bbox centred at (5 + offset + 1, 5) so that left edge is at 5+offset+0.5,
        // giving MINDIST = offset from centre horizontally.
        let left = cx + (offset as f64);
        let right = left + 1.0;
        tree.insert(
            Bbox2D::new(left, cy - 0.25, right, cy + 0.25).unwrap(),
            offset,
        );
    }

    let result = tree.search_top_k(&window, 6);
    assert_eq!(result.len(), 6);

    // Verify non-decreasing distances.
    for pair in result.windows(2) {
        let (_, d0) = pair[0];
        let (_, d1) = pair[1];
        assert!(
            d0 <= d1 + 1e-12,
            "distances must be non-decreasing: {d0} > {d1}"
        );
    }
}

// ---------------------------------------------------------------------------
// Test 6 — items outside the window are excluded
// ---------------------------------------------------------------------------

#[test]
fn test_search_top_k_window_filters_outside() {
    let mut tree: RTree<&str> = RTree::new();

    tree.insert(bbox_at(2.0, 2.0, 0.4, 0.4), "inside_a");
    tree.insert(bbox_at(3.0, 3.0, 0.4, 0.4), "inside_b");
    tree.insert(bbox_at(20.0, 20.0, 0.4, 0.4), "outside");

    let window = Bbox2D::new(0.0, 0.0, 5.0, 5.0).unwrap();
    let result = tree.search_top_k(&window, 10);

    let values: Vec<&str> = result.iter().map(|(v, _)| **v).collect();
    assert!(values.contains(&"inside_a"), "inside_a must be included");
    assert!(values.contains(&"inside_b"), "inside_b must be included");
    assert!(
        !values.contains(&"outside"),
        "outside item must be excluded"
    );
    assert_eq!(result.len(), 2);
}

// ---------------------------------------------------------------------------
// Test 7 — item at (or covering) the window centre has distance 0 and ranks first
// ---------------------------------------------------------------------------

#[test]
fn test_search_top_k_centre_item_first() {
    let window = Bbox2D::new(0.0, 0.0, 10.0, 10.0).unwrap();
    let (cx, cy) = window.center(); // (5.0, 5.0)

    let mut tree: RTree<&str> = RTree::new();

    // Item that contains the exact centre → MINDIST = 0.
    tree.insert(
        Bbox2D::new(cx - 0.5, cy - 0.5, cx + 0.5, cy + 0.5).unwrap(),
        "centre",
    );
    // Items farther away (bbox does NOT contain centre).
    tree.insert(Bbox2D::new(7.0, 7.0, 9.0, 9.0).unwrap(), "far_a");
    tree.insert(Bbox2D::new(1.0, 1.0, 2.0, 2.0).unwrap(), "far_b");

    let result = tree.search_top_k(&window, 3);
    assert_eq!(result.len(), 3);

    let (first_val, first_dist) = result[0];
    assert_eq!(
        *first_val, "centre",
        "item covering the centre must rank first"
    );
    assert!(
        first_dist < 1e-12,
        "MINDIST for centre item must be ~0, got {first_dist}"
    );
}

// ---------------------------------------------------------------------------
// Test 8 — SpatialQuery::top_k_in_window gives the same result as search_top_k
// ---------------------------------------------------------------------------

#[test]
fn test_spatial_query_top_k_in_window_static() {
    let mut tree: RTree<u32> = RTree::new();

    for i in 0u32..8 {
        let cx = (i as f64) * 5.0;
        tree.insert(bbox_at(cx, 0.0, 1.0, 1.0), i);
    }

    let window = Bbox2D::new(-2.0, -2.0, 20.0, 2.0).unwrap();
    let k = 4;

    let direct = tree.search_top_k(&window, k);
    let via_sq = SpatialQuery::top_k_in_window(&tree, &window, k);

    assert_eq!(direct.len(), via_sq.len(), "result lengths must match");

    for ((v_d, d_d), (v_sq, d_sq)) in direct.iter().zip(via_sq.iter()) {
        assert_eq!(v_d, v_sq, "values must match at each rank");
        assert!(
            (d_d - d_sq).abs() < 1e-12,
            "distances must match at each rank: {d_d} vs {d_sq}"
        );
    }
}
