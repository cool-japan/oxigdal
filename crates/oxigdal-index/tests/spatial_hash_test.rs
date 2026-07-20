//! Integration tests for [`SpatialHashGrid`].
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use oxigdal_index::{Bbox2D, SpatialHashGrid};

// ---------------------------------------------------------------------------
// Deterministic LCG (no `rand` crate) — identical to the one used in rtree_test.
// ---------------------------------------------------------------------------

struct Lcg {
    state: u64,
}

impl Lcg {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_f64(&mut self) -> f64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Random f64 in [lo, hi).
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + self.next_f64() * (hi - lo)
    }
}

// ---------------------------------------------------------------------------
// Helper: build a non-degenerate bbox with given centre and half-extents.
// ---------------------------------------------------------------------------

fn bbox_centred(cx: f64, cy: f64, hw: f64, hh: f64) -> Bbox2D {
    Bbox2D::new(cx - hw, cy - hh, cx + hw, cy + hh).unwrap()
}

// ---------------------------------------------------------------------------
// Test 1: simple insert and search
// ---------------------------------------------------------------------------

#[test]
fn test_spatial_hash_insert_and_search_simple() {
    let mut grid: SpatialHashGrid<u32> = SpatialHashGrid::new(10.0);

    let item_bbox = Bbox2D::new(1.0, 1.0, 4.0, 4.0).unwrap();
    grid.insert(item_bbox, 42_u32);

    let query = Bbox2D::new(2.0, 2.0, 5.0, 5.0).unwrap();
    let hits = grid.search(&query);

    assert_eq!(hits.len(), 1, "expected one hit");
    assert_eq!(*hits[0], 42);
}

// ---------------------------------------------------------------------------
// Test 2: query disjoint from all items returns empty
// ---------------------------------------------------------------------------

#[test]
fn test_spatial_hash_search_non_overlapping_returns_empty() {
    let mut grid: SpatialHashGrid<&str> = SpatialHashGrid::new(5.0);

    grid.insert(Bbox2D::new(0.0, 0.0, 3.0, 3.0).unwrap(), "alpha");
    grid.insert(Bbox2D::new(10.0, 10.0, 12.0, 12.0).unwrap(), "beta");

    // Query is far from both items.
    let query = Bbox2D::new(100.0, 100.0, 110.0, 110.0).unwrap();
    let hits = grid.search(&query);

    assert!(hits.is_empty(), "expected no hits, got {}", hits.len());
}

// ---------------------------------------------------------------------------
// Test 3: large item spanning multiple cells — returned exactly once
// ---------------------------------------------------------------------------

#[test]
fn test_spatial_hash_dedup_item_spanning_multiple_cells() {
    // cell_size=1.0, so a bbox from (0.5,0.5) to (2.5,2.5) spans a 3×3 = 9 cells.
    let mut grid: SpatialHashGrid<&str> = SpatialHashGrid::new(1.0);

    let big_bbox = Bbox2D::new(0.5, 0.5, 2.5, 2.5).unwrap();
    grid.insert(big_bbox, "big polygon");

    // Confirm the item occupies multiple cells internally.
    assert!(
        grid.cell_entry_count() > 1,
        "expected item to span multiple cells, got {} entries",
        grid.cell_entry_count()
    );

    // A query that touches several of those cells.
    let query = Bbox2D::new(0.0, 0.0, 3.0, 3.0).unwrap();
    let hits = grid.search(&query);

    assert_eq!(
        hits.len(),
        1,
        "item spanning multiple cells must be returned exactly once"
    );
    assert_eq!(*hits[0], "big polygon");
}

// ---------------------------------------------------------------------------
// Test 4: remove makes item invisible to search
// ---------------------------------------------------------------------------

#[test]
fn test_spatial_hash_remove_makes_item_invisible() {
    let mut grid: SpatialHashGrid<u32> = SpatialHashGrid::new(10.0);

    let b = Bbox2D::new(0.0, 0.0, 5.0, 5.0).unwrap();
    let idx = grid.insert(b, 7_u32);

    // Confirm it is visible before removal.
    let query = Bbox2D::new(0.0, 0.0, 10.0, 10.0).unwrap();
    assert_eq!(
        grid.search(&query).len(),
        1,
        "item should be found before remove"
    );

    // Remove it.
    let removed = grid.remove(idx);
    assert!(removed, "remove should return true for a live item");

    // Now it must be invisible.
    let hits = grid.search(&query);
    assert!(hits.is_empty(), "item should not be found after remove");

    // Double-remove must return false.
    assert!(!grid.remove(idx), "double-remove should return false");
}

// ---------------------------------------------------------------------------
// Test 5: len counts only live items
// ---------------------------------------------------------------------------

#[test]
fn test_spatial_hash_len_counts_live_items() {
    let mut grid: SpatialHashGrid<u32> = SpatialHashGrid::new(10.0);

    let i0 = grid.insert(Bbox2D::new(0.0, 0.0, 1.0, 1.0).unwrap(), 10);
    let _i1 = grid.insert(Bbox2D::new(5.0, 5.0, 6.0, 6.0).unwrap(), 20);
    let _i2 = grid.insert(Bbox2D::new(15.0, 15.0, 16.0, 16.0).unwrap(), 30);

    assert_eq!(grid.len(), 3, "three live items");

    grid.remove(i0);
    assert_eq!(grid.len(), 2, "two live items after one removal");

    assert!(!grid.is_empty());
}

// ---------------------------------------------------------------------------
// Test 6: items at negative coordinates are found correctly
// ---------------------------------------------------------------------------

#[test]
fn test_spatial_hash_negative_coords_handled() {
    let mut grid: SpatialHashGrid<&str> = SpatialHashGrid::new(10.0);

    // Item entirely in the negative quadrant.
    let neg_bbox = Bbox2D::new(-20.0, -20.0, -5.0, -5.0).unwrap();
    grid.insert(neg_bbox, "negative region");

    // Item straddling the origin.
    let cross_bbox = Bbox2D::new(-3.0, -3.0, 3.0, 3.0).unwrap();
    grid.insert(cross_bbox, "origin straddler");

    // Query covering negative region.
    let q_neg = Bbox2D::new(-25.0, -25.0, -4.0, -4.0).unwrap();
    let hits_neg = grid.search(&q_neg);
    assert_eq!(hits_neg.len(), 1);
    assert_eq!(*hits_neg[0], "negative region");

    // Query near origin — should find the straddling item.
    let q_origin = Bbox2D::new(-1.0, -1.0, 1.0, 1.0).unwrap();
    let hits_origin = grid.search(&q_origin);
    assert_eq!(hits_origin.len(), 1);
    assert_eq!(*hits_origin[0], "origin straddler");

    // A wide query covering both.
    let q_wide = Bbox2D::new(-25.0, -25.0, 5.0, 5.0).unwrap();
    let hits_wide = grid.search(&q_wide);
    assert_eq!(hits_wide.len(), 2, "both items in wide query");
}

// ---------------------------------------------------------------------------
// Test 7: compact after removing all items → cell_entry_count == 0
// ---------------------------------------------------------------------------

#[test]
fn test_spatial_hash_compact_reduces_memory() {
    let mut grid: SpatialHashGrid<u32> = SpatialHashGrid::new(1.0);

    // Insert several items scattered around.
    let mut indices = Vec::new();
    let mut lcg = Lcg::new(0xDEAD_BEEF_0001);
    for i in 0..20_u32 {
        let cx = lcg.range(-50.0, 50.0);
        let cy = lcg.range(-50.0, 50.0);
        let b = bbox_centred(cx, cy, 1.5, 1.5);
        indices.push(grid.insert(b, i));
    }

    assert_eq!(grid.len(), 20);
    assert!(grid.cell_entry_count() > 0, "should have cell entries");

    // Remove all items.
    for idx in &indices {
        grid.remove(*idx);
    }

    assert_eq!(grid.len(), 0);
    assert!(grid.is_empty());

    // Before compact, cell entries still linger (tombstoned arena slots).
    // After compact they should all be gone.
    grid.compact();

    assert_eq!(
        grid.cell_entry_count(),
        0,
        "after compact, no cell entries remain: got {}",
        grid.cell_entry_count()
    );
    assert_eq!(
        grid.arena_capacity(),
        0,
        "arena should be empty after full compact"
    );
    assert_eq!(
        grid.occupied_cell_count(),
        0,
        "no occupied cells after compact"
    );
}

// ---------------------------------------------------------------------------
// Test 8: insert 1000 items, world-bbox query finds all 1000
// ---------------------------------------------------------------------------

#[test]
fn test_spatial_hash_thousand_items_all_found() {
    const N: usize = 1000;
    let mut grid: SpatialHashGrid<usize> = SpatialHashGrid::with_capacity(50.0, N);

    let mut lcg = Lcg::new(0xCAFE_BABE_1234);
    for i in 0..N {
        let cx = lcg.range(-500.0, 500.0);
        let cy = lcg.range(-500.0, 500.0);
        let hw = lcg.range(0.5, 25.0);
        let hh = lcg.range(0.5, 25.0);
        let b = Bbox2D::new(cx - hw, cy - hh, cx + hw, cy + hh).unwrap();
        grid.insert(b, i);
    }

    assert_eq!(grid.len(), N, "all items live");

    // A world bbox that certainly covers all inserted items (since coords are bounded).
    let world = Bbox2D::new(-1000.0, -1000.0, 1000.0, 1000.0).unwrap();
    let hits = grid.search(&world);

    assert_eq!(
        hits.len(),
        N,
        "world-bbox query must return all {N} items, got {}",
        hits.len()
    );
}

// ---------------------------------------------------------------------------
// Test 9: zero-area bbox (point item) found by an overlapping query
// ---------------------------------------------------------------------------

#[test]
fn test_spatial_hash_point_item_found_by_containing_query() {
    let mut grid: SpatialHashGrid<&str> = SpatialHashGrid::new(10.0);

    // A point bbox.
    let point_bbox = Bbox2D::point(3.5, 7.2);
    grid.insert(point_bbox, "pin");

    // The point is degenerate but still has a cell address; any query whose
    // range includes that cell and whose bbox intersects the point bbox should
    // return the item.
    let query = Bbox2D::new(0.0, 0.0, 10.0, 10.0).unwrap();
    let hits = grid.search(&query);
    assert_eq!(
        hits.len(),
        1,
        "point item must be found by overlapping query"
    );
    assert_eq!(*hits[0], "pin");

    // A query that does not contain the point must not return it.
    let miss = Bbox2D::new(5.0, 5.0, 20.0, 20.0).unwrap();
    // point (3.5, 7.2): 3.5 < 5.0, so disjoint in x from [5,20].
    let misses = grid.search(&miss);
    assert!(
        misses.is_empty(),
        "point item must not be found by non-overlapping query"
    );
}

// ---------------------------------------------------------------------------
// Test 10: search_with_index returns correct indices
// ---------------------------------------------------------------------------

#[test]
fn test_spatial_hash_search_with_index_returns_correct_handles() {
    let mut grid: SpatialHashGrid<u32> = SpatialHashGrid::new(5.0);

    let idx_a = grid.insert(Bbox2D::new(0.0, 0.0, 2.0, 2.0).unwrap(), 100_u32);
    let idx_b = grid.insert(Bbox2D::new(3.0, 3.0, 5.0, 5.0).unwrap(), 200_u32);
    let _idx_c = grid.insert(Bbox2D::new(20.0, 20.0, 22.0, 22.0).unwrap(), 300_u32);

    let query = Bbox2D::new(0.0, 0.0, 6.0, 6.0).unwrap();
    let hits = grid.search_with_index(&query);

    // Exactly two items in the queried region.
    assert_eq!(hits.len(), 2, "expected two hits");

    let hit_indices: Vec<usize> = hits.iter().map(|(i, _)| *i).collect();
    let hit_values: Vec<u32> = hits.iter().map(|(_, v)| **v).collect();

    assert!(hit_indices.contains(&idx_a));
    assert!(hit_indices.contains(&idx_b));
    assert!(hit_values.contains(&100));
    assert!(hit_values.contains(&200));

    // Collect indices first to drop the borrow on `grid`, then remove.
    let indices_to_remove: Vec<usize> = hits.iter().map(|(i, _)| *i).collect();
    drop(hits);
    for idx in indices_to_remove {
        grid.remove(idx);
    }
    let after = grid.search(&query);
    assert!(
        after.is_empty(),
        "after removing all hits, query should be empty"
    );
}

// ---------------------------------------------------------------------------
// Test 11: iter visits every live item
// ---------------------------------------------------------------------------

#[test]
fn test_spatial_hash_iter_visits_all_live_items() {
    let mut grid: SpatialHashGrid<u32> = SpatialHashGrid::new(10.0);

    let i0 = grid.insert(Bbox2D::new(0.0, 0.0, 1.0, 1.0).unwrap(), 1_u32);
    let _i1 = grid.insert(Bbox2D::new(5.0, 5.0, 6.0, 6.0).unwrap(), 2_u32);
    let _i2 = grid.insert(Bbox2D::new(15.0, 15.0, 16.0, 16.0).unwrap(), 3_u32);

    grid.remove(i0);

    let values: Vec<u32> = grid.iter().map(|(_, v)| *v).collect();
    assert_eq!(values.len(), 2, "iter must skip dead items");
    assert!(values.contains(&2));
    assert!(values.contains(&3));
    assert!(!values.contains(&1), "removed item must not appear in iter");
}

// ---------------------------------------------------------------------------
// Test 12: compact preserves correct search results
// ---------------------------------------------------------------------------

#[test]
fn test_spatial_hash_compact_preserves_search_results() {
    let mut grid: SpatialHashGrid<u32> = SpatialHashGrid::new(5.0);

    // Insert several items; remove some; compact; verify search still works.
    let _ia = grid.insert(Bbox2D::new(0.0, 0.0, 3.0, 3.0).unwrap(), 10_u32);
    let ib = grid.insert(Bbox2D::new(4.0, 4.0, 7.0, 7.0).unwrap(), 20_u32);
    let _ic = grid.insert(Bbox2D::new(8.0, 8.0, 10.0, 10.0).unwrap(), 30_u32);
    let id = grid.insert(Bbox2D::new(20.0, 20.0, 25.0, 25.0).unwrap(), 40_u32);

    // Remove two items (including a non-adjacent pair).
    grid.remove(ib);
    grid.remove(id);

    assert_eq!(grid.len(), 2, "two live items before compact");

    grid.compact();

    assert_eq!(grid.len(), 2, "two live items after compact");

    // Items with value 10 and 30 remain.
    let q_near = Bbox2D::new(0.0, 0.0, 11.0, 11.0).unwrap();
    let hits = grid.search(&q_near);
    assert_eq!(hits.len(), 2, "both remaining items found after compact");

    let vals: Vec<u32> = hits.iter().map(|v| **v).collect();
    assert!(vals.contains(&10));
    assert!(vals.contains(&30));
    assert!(
        !vals.contains(&20),
        "removed item must not appear after compact"
    );
    assert!(
        !vals.contains(&40),
        "removed item must not appear after compact"
    );

    // A query in the far region should find nothing.
    let q_far = Bbox2D::new(20.0, 20.0, 30.0, 30.0).unwrap();
    let far_hits = grid.search(&q_far);
    assert!(
        far_hits.is_empty(),
        "removed far item must not be found after compact"
    );
}

// ---------------------------------------------------------------------------
// Test 13: extent covers all live items
// ---------------------------------------------------------------------------

#[test]
fn test_spatial_hash_extent_covers_all_live_items() {
    let mut grid: SpatialHashGrid<u32> = SpatialHashGrid::new(10.0);

    grid.insert(Bbox2D::new(-5.0, -3.0, 2.0, 1.0).unwrap(), 1);
    grid.insert(Bbox2D::new(10.0, 8.0, 15.0, 20.0).unwrap(), 2);

    let ext = grid.extent().expect("non-empty grid must have an extent");

    assert!(ext.min_x <= -5.0, "extent.min_x must cover leftmost item");
    assert!(ext.min_y <= -3.0, "extent.min_y must cover bottommost item");
    assert!(ext.max_x >= 15.0, "extent.max_x must cover rightmost item");
    assert!(ext.max_y >= 20.0, "extent.max_y must cover topmost item");
}

// ---------------------------------------------------------------------------
// Test 14: out-of-range remove returns false
// ---------------------------------------------------------------------------

#[test]
fn test_spatial_hash_remove_out_of_range_returns_false() {
    let mut grid: SpatialHashGrid<u32> = SpatialHashGrid::new(10.0);

    // No items; index 0 is out of range.
    assert!(!grid.remove(0), "remove on empty grid must return false");
    assert!(
        !grid.remove(999),
        "remove of nonexistent index must return false"
    );

    grid.insert(Bbox2D::new(0.0, 0.0, 1.0, 1.0).unwrap(), 1);
    assert!(!grid.remove(999), "remove of index > len must return false");
}

// ---------------------------------------------------------------------------
// Test 15: search_with_bbox returns bounding boxes
// ---------------------------------------------------------------------------

#[test]
fn test_spatial_hash_search_with_bbox_returns_bboxes() {
    let mut grid: SpatialHashGrid<u32> = SpatialHashGrid::new(5.0);

    let stored_bbox = Bbox2D::new(1.0, 1.0, 3.0, 3.0).unwrap();
    grid.insert(stored_bbox, 77_u32);

    let query = Bbox2D::new(0.0, 0.0, 4.0, 4.0).unwrap();
    let results = grid.search_with_bbox(&query);

    assert_eq!(results.len(), 1);
    let (_, found_bbox, found_val) = &results[0];
    assert_eq!(**found_val, 77);
    assert_eq!(found_bbox.min_x, 1.0);
    assert_eq!(found_bbox.max_y, 3.0);
}
