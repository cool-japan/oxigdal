//! Integration tests for [`oxigeo_index::StreamingRTree`].
//!
//! All tests use `(Bbox2D, u32)` entries — identical to the style used in
//! `rtree_test.rs` — so no external dependencies beyond `oxigeo_index` are
//! required.

use oxigeo_index::{Bbox2D, StreamingInsertConfig, StreamingRTree};

// ---------------------------------------------------------------------------
// Tiny helper: build a Bbox2D from (min_x, min_y) with unit size.
// ---------------------------------------------------------------------------

fn unit_bbox(min_x: f64, min_y: f64) -> Bbox2D {
    Bbox2D::new(min_x, min_y, min_x + 1.0, min_y + 1.0).unwrap()
}

// ---------------------------------------------------------------------------
// 1.  A freshly created index is empty.
// ---------------------------------------------------------------------------

#[test]
fn test_streaming_rtree_new_is_empty() {
    let st: StreamingRTree<u32> = StreamingRTree::new();
    assert_eq!(st.len(), 0, "expected len 0 for a new tree");
    assert!(st.is_empty(), "expected is_empty() == true for a new tree");
    assert_eq!(st.stable_len(), 0);
    assert_eq!(st.pending_len(), 0);
}

// ---------------------------------------------------------------------------
// 2.  A single insertion lands in `pending`, not in `stable`.
//
//     We use a custom config that disables *both* triggers so the item
//     definitively stays in the pending buffer:
//       - rebalance_threshold = usize::MAX  (count trigger never fires)
//       - max_pending_fraction = 1.0        (fraction trigger never fires)
// ---------------------------------------------------------------------------

#[test]
fn test_streaming_rtree_insert_one_item_appears_in_pending() {
    let config = StreamingInsertConfig {
        rebalance_threshold: usize::MAX,
        max_pending_fraction: 1.0,
    };
    let mut st: StreamingRTree<u32> = StreamingRTree::with_config(config);
    st.insert(unit_bbox(0.0, 0.0), 1_u32);

    assert_eq!(st.pending_len(), 1, "single insert should land in pending");
    assert_eq!(st.stable_len(), 0, "stable tree should be untouched");
    assert_eq!(st.len(), 1, "total len should be 1");
    assert!(!st.is_empty());
}

// ---------------------------------------------------------------------------
// 3.  Inserting exactly `threshold` items triggers a rebuild, leaving
//     `pending` empty.
// ---------------------------------------------------------------------------

#[test]
fn test_streaming_rtree_rebuild_triggered_at_threshold() {
    let threshold = 8_usize;
    let config = StreamingInsertConfig {
        rebalance_threshold: threshold,
        max_pending_fraction: 1.0, // disable fraction trigger
    };
    let mut st: StreamingRTree<u32> = StreamingRTree::with_config(config);

    // Insert threshold - 1 items: no rebuild should occur yet.
    for i in 0..(threshold - 1) {
        st.insert(unit_bbox(i as f64, 0.0), i as u32);
    }
    assert_eq!(
        st.pending_len(),
        threshold - 1,
        "pending should hold threshold-1 items before rebuild"
    );
    assert_eq!(st.rebuild_count(), 0, "no rebuild yet");

    // The threshold-th insert triggers the rebuild.
    st.insert(
        unit_bbox((threshold - 1) as f64, 0.0),
        (threshold - 1) as u32,
    );

    assert_eq!(
        st.pending_len(),
        0,
        "pending must be empty after auto-rebuild"
    );
    assert_eq!(
        st.stable_len(),
        threshold,
        "all items must be in stable tree"
    );
    assert_eq!(
        st.rebuild_count(),
        1,
        "exactly one rebuild should have fired"
    );
}

// ---------------------------------------------------------------------------
// 4.  The rebuild counter increments once per flush.
// ---------------------------------------------------------------------------

#[test]
fn test_streaming_rtree_rebuild_count_increments() {
    let threshold = 4_usize;
    let config = StreamingInsertConfig {
        rebalance_threshold: threshold,
        max_pending_fraction: 1.0,
    };
    let mut st: StreamingRTree<u32> = StreamingRTree::with_config(config);

    // First batch: triggers rebuild #1.
    for i in 0..threshold {
        st.insert(unit_bbox(i as f64, 0.0), i as u32);
    }
    assert_eq!(st.rebuild_count(), 1);

    // Second batch: triggers rebuild #2.
    for i in threshold..(2 * threshold) {
        st.insert(unit_bbox(i as f64, 0.0), i as u32);
    }
    assert_eq!(st.rebuild_count(), 2);
}

// ---------------------------------------------------------------------------
// 5.  `len()` always equals `stable_len() + pending_len()`.
// ---------------------------------------------------------------------------

#[test]
fn test_streaming_rtree_total_len_equals_sum() {
    let config = StreamingInsertConfig {
        rebalance_threshold: 10,
        max_pending_fraction: 1.0,
    };
    let mut st: StreamingRTree<u32> = StreamingRTree::with_config(config);

    for i in 0_u32..7 {
        st.insert(unit_bbox(i as f64, 0.0), i);
        assert_eq!(
            st.len(),
            st.stable_len() + st.pending_len(),
            "len invariant broken after insert {i}"
        );
    }

    // Manually flush and check again.
    st.rebuild();
    assert_eq!(
        st.len(),
        st.stable_len() + st.pending_len(),
        "len invariant broken after manual rebuild"
    );
}

// ---------------------------------------------------------------------------
// 6.  An item inserted but not yet flushed is still visible via `search`.
// ---------------------------------------------------------------------------

#[test]
fn test_streaming_rtree_search_finds_item_in_pending() {
    let config = StreamingInsertConfig {
        rebalance_threshold: 1000,
        max_pending_fraction: 1.0,
    };
    let mut st: StreamingRTree<u32> = StreamingRTree::with_config(config);

    let item_bbox = Bbox2D::new(2.0, 2.0, 4.0, 4.0).unwrap();
    st.insert(item_bbox, 42_u32);

    // The item is still in pending (threshold not reached).
    assert_eq!(st.pending_len(), 1);
    assert_eq!(st.stable_len(), 0);

    // A query that overlaps the item's bbox must find it.
    let query = Bbox2D::new(3.0, 3.0, 5.0, 5.0).unwrap();
    let hits = st.search(&query);
    assert_eq!(hits.len(), 1, "expected the pending item to be found");
    assert_eq!(*hits[0], 42_u32);
}

// ---------------------------------------------------------------------------
// 7.  After a flush the item is still visible via `search`.
// ---------------------------------------------------------------------------

#[test]
fn test_streaming_rtree_search_finds_item_after_rebuild() {
    let config = StreamingInsertConfig {
        rebalance_threshold: 1000,
        max_pending_fraction: 1.0,
    };
    let mut st: StreamingRTree<u32> = StreamingRTree::with_config(config);

    let item_bbox = Bbox2D::new(2.0, 2.0, 4.0, 4.0).unwrap();
    st.insert(item_bbox, 99_u32);

    // Force a manual rebuild.
    st.rebuild();
    assert_eq!(st.pending_len(), 0, "pending must be empty after rebuild");
    assert_eq!(st.stable_len(), 1);

    // The item must still be found in the stable tree.
    let query = Bbox2D::new(0.0, 0.0, 5.0, 5.0).unwrap();
    let hits = st.search(&query);
    assert_eq!(hits.len(), 1, "item must survive the rebuild");
    assert_eq!(*hits[0], 99_u32);
}

// ---------------------------------------------------------------------------
// 8.  Search correctly unions items from both the stable tree and pending.
// ---------------------------------------------------------------------------

#[test]
fn test_streaming_rtree_search_union_stable_and_pending() {
    let threshold = 3_usize;
    let config = StreamingInsertConfig {
        rebalance_threshold: threshold,
        max_pending_fraction: 1.0,
    };
    let mut st: StreamingRTree<u32> = StreamingRTree::with_config(config);

    // Insert enough to trigger one rebuild: items 0..threshold go into stable.
    for i in 0..threshold {
        st.insert(unit_bbox(i as f64 * 2.0, 0.0), i as u32);
    }
    assert_eq!(st.stable_len(), threshold, "first batch must be in stable");
    assert_eq!(st.pending_len(), 0);

    // Insert two more items that will sit in pending.
    let pending_val_a = 100_u32;
    let pending_val_b = 200_u32;
    st.insert(unit_bbox(100.0, 100.0), pending_val_a);
    st.insert(unit_bbox(200.0, 200.0), pending_val_b);
    assert_eq!(st.pending_len(), 2);

    // Build a global bbox that covers the pending items but also a stable item.
    // item 0 is at [0,1] x [0,1]; pending_val_a is at [100,101] x [100,101].
    let huge_query = Bbox2D::new(-1.0, -1.0, 300.0, 300.0).unwrap();
    let hits = st.search(&huge_query);
    assert_eq!(
        hits.len(),
        threshold + 2,
        "search must include stable and pending items"
    );

    // A narrow query that only covers the pending item at (100, 100).
    let narrow_query = Bbox2D::new(99.0, 99.0, 102.0, 102.0).unwrap();
    let narrow_hits = st.search(&narrow_query);
    assert_eq!(narrow_hits.len(), 1);
    assert_eq!(*narrow_hits[0], pending_val_a);
}

// ---------------------------------------------------------------------------
// 9.  Calling `rebuild()` manually drains the pending buffer.
// ---------------------------------------------------------------------------

#[test]
fn test_streaming_rtree_manual_rebuild_merges_pending() {
    let config = StreamingInsertConfig {
        rebalance_threshold: 1000,
        max_pending_fraction: 1.0,
    };
    let mut st: StreamingRTree<u32> = StreamingRTree::with_config(config);

    for i in 0_u32..5 {
        st.insert(unit_bbox(i as f64, 0.0), i);
    }
    assert_eq!(st.pending_len(), 5, "all items should be in pending");
    assert_eq!(
        st.stable_len(),
        0,
        "stable must be empty before manual flush"
    );

    st.rebuild();

    assert_eq!(
        st.pending_len(),
        0,
        "pending must be empty after manual rebuild"
    );
    assert_eq!(st.stable_len(), 5, "all items must move to stable");
    assert_eq!(st.rebuild_count(), 1);
}

// ---------------------------------------------------------------------------
// 10. The default configuration has `rebalance_threshold == 512`.
// ---------------------------------------------------------------------------

#[test]
fn test_streaming_rtree_config_default_threshold_512() {
    let cfg = StreamingInsertConfig::default();
    assert_eq!(
        cfg.rebalance_threshold, 512,
        "default threshold must be 512"
    );
    assert!(
        (cfg.max_pending_fraction - 0.3).abs() < f64::EPSILON,
        "default max_pending_fraction must be 0.3"
    );
}

// ---------------------------------------------------------------------------
// 11. A custom threshold of 10 triggers a rebuild when 10 items are inserted.
// ---------------------------------------------------------------------------

#[test]
fn test_streaming_rtree_with_custom_config() {
    let threshold = 10_usize;
    let config = StreamingInsertConfig {
        rebalance_threshold: threshold,
        max_pending_fraction: 1.0, // disable fraction trigger
    };
    let mut st: StreamingRTree<u32> = StreamingRTree::with_config(config);

    // Insert threshold - 1 items: no rebuild.
    for i in 0..(threshold - 1) {
        st.insert(unit_bbox(i as f64, 0.0), i as u32);
    }
    assert_eq!(st.rebuild_count(), 0, "no rebuild before threshold");

    // Insert the 10th item: rebuild fires.
    st.insert(
        unit_bbox((threshold - 1) as f64, 0.0),
        (threshold - 1) as u32,
    );
    assert_eq!(st.rebuild_count(), 1, "rebuild must fire at threshold");
    assert_eq!(st.pending_len(), 0);
    assert_eq!(st.stable_len(), threshold);
}

// ---------------------------------------------------------------------------
// 12. `total_inserted` counts every `insert` call across multiple rebuild cycles.
// ---------------------------------------------------------------------------

#[test]
fn test_streaming_rtree_total_inserted_counts_all() {
    let threshold = 5_usize;
    let config = StreamingInsertConfig {
        rebalance_threshold: threshold,
        max_pending_fraction: 1.0,
    };
    let mut st: StreamingRTree<u32> = StreamingRTree::with_config(config);

    // First cycle: 5 inserts → rebuild.
    for i in 0..threshold {
        st.insert(unit_bbox(i as f64, 0.0), i as u32);
    }
    assert_eq!(st.total_inserted(), threshold);
    assert_eq!(st.rebuild_count(), 1);

    // Second cycle: 5 more inserts → second rebuild.
    for i in threshold..(2 * threshold) {
        st.insert(unit_bbox(i as f64, 0.0), i as u32);
    }
    assert_eq!(
        st.total_inserted(),
        2 * threshold,
        "total_inserted must count all insertions across cycles"
    );
    assert_eq!(st.rebuild_count(), 2);
    assert_eq!(
        st.stable_len(),
        2 * threshold,
        "all items must be in stable after two rebuild cycles"
    );
}
