//! Tests for `RTree::search_line` — line-segment intersection query (Slice 7 W4).

use oxigeo_index::{Bbox2D, RTree};

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn make_tree<I>(items: I) -> RTree<u32>
where
    I: IntoIterator<Item = (f64, f64, f64, f64, u32)>,
{
    let mut tree: RTree<u32> = RTree::new();
    for (min_x, min_y, max_x, max_y, val) in items {
        let bbox = Bbox2D::new(min_x, min_y, max_x, max_y).expect("valid bbox");
        tree.insert(bbox, val);
    }
    tree
}

// ---------------------------------------------------------------------------
// 1. Both entries on a diagonal path are found.
// ---------------------------------------------------------------------------
#[test]
fn test_search_line_finds_entry_on_path() {
    let tree = make_tree([(0.0, 0.0, 1.0, 1.0, 1), (5.0, 5.0, 6.0, 6.0, 2)]);
    let line = [(0.0f64, 0.0f64), (10.0, 10.0)];
    let results = tree.search_line(&line, 0.1);
    let mut vals: Vec<u32> = results.iter().map(|v| **v).collect();
    vals.sort_unstable();
    assert_eq!(
        vals,
        vec![1, 2],
        "both bboxes lie on the diagonal, should be found"
    );
}

// ---------------------------------------------------------------------------
// 2. An entry far from the path is not found.
// ---------------------------------------------------------------------------
#[test]
fn test_search_line_misses_entry_far_from_path() {
    let tree = make_tree([(10.0, 0.0, 11.0, 1.0, 99)]);
    let line = [(0.0f64, 0.0f64), (5.0, 5.0)];
    let results = tree.search_line(&line, 0.1);
    assert!(
        results.is_empty(),
        "bbox at x=10..11 is far from the diagonal, must not be found"
    );
}

// ---------------------------------------------------------------------------
// 3. Buffer controls whether a nearby entry is captured.
// ---------------------------------------------------------------------------
#[test]
fn test_search_line_with_buffer_catches_nearby_entry() {
    // Entry is offset 0.3 units above the line y=3.
    let tree = make_tree([(3.0, 3.3, 3.5, 3.8, 42)]);
    let line = [(0.0f64, 3.0f64), (10.0, 3.0)];

    // buffer 0.0: the entry bbox starts at y=3.3, line is at y=3.0 → not found.
    let no_hit = tree.search_line(&line, 0.0);
    assert!(
        no_hit.is_empty(),
        "with buffer=0 the entry at y=3.3..3.8 should not be found for line y=3"
    );

    // buffer 1.0: corridor extends up to y=4.0 → found.
    let hit = tree.search_line(&line, 1.0);
    assert_eq!(
        hit.len(),
        1,
        "with buffer=1 the entry at y=3.3 should be found"
    );
}

// ---------------------------------------------------------------------------
// 4. Empty / single-point line returns nothing.
// ---------------------------------------------------------------------------
#[test]
fn test_search_line_empty_line_returns_empty() {
    let tree = make_tree([(0.0, 0.0, 5.0, 5.0, 7)]);

    let empty: &[(f64, f64)] = &[];
    assert!(
        tree.search_line(empty, 1.0).is_empty(),
        "empty slice should return empty"
    );

    let single = [(3.0f64, 3.0f64)];
    assert!(
        tree.search_line(&single, 1.0).is_empty(),
        "single-vertex polyline has no segments — must return empty"
    );
}

// ---------------------------------------------------------------------------
// 5. An entry matched by multiple segments appears exactly once (dedup).
// ---------------------------------------------------------------------------
#[test]
fn test_search_line_no_dedup_across_segments() {
    // The entry spans the whole canvas; every segment passes through it.
    let tree = make_tree([(0.0, 0.0, 20.0, 20.0, 55)]);
    // Three segments, all inside the big bbox.
    let line = [(1.0f64, 1.0f64), (5.0, 5.0), (10.0, 2.0), (15.0, 15.0)];
    let results = tree.search_line(&line, 0.0);
    assert_eq!(
        results.len(),
        1,
        "entry intersected by all segments must appear exactly once after dedup"
    );
}

// ---------------------------------------------------------------------------
// 6. Vertical segment clipped correctly by Liang-Barsky.
// ---------------------------------------------------------------------------
#[test]
fn test_search_line_vertical_segment() {
    // Vertical line x=3, y=0..10, buffer 0.5 → corridor x=2.5..3.5.
    let tree = make_tree([
        // Inside corridor: x=3.3..3.7 is within 0.5 of x=3.
        (3.3, 4.0, 3.7, 6.0, 1),
        // Outside corridor: x=4.0..4.5 is more than 0.5 away from x=3.
        (4.0, 4.0, 4.5, 6.0, 2),
    ]);
    let line = [(3.0f64, 0.0f64), (3.0, 10.0)];
    let results = tree.search_line(&line, 0.5);
    let vals: Vec<u32> = results.iter().map(|v| **v).collect();
    assert!(
        vals.contains(&1),
        "entry at x=3.3..3.7 must be within buffer 0.5 of x=3"
    );
    assert!(
        !vals.contains(&2),
        "entry at x=4.0..4.5 must NOT be within buffer 0.5 of x=3"
    );
}

// ---------------------------------------------------------------------------
// 7. Large dataset: horizontal then vertical path covers ~500 entries.
// ---------------------------------------------------------------------------
#[test]
fn test_search_line_1000_entries_path_query() {
    // 1000 unit bboxes along y=0..1 at x positions 0..1000.
    let items = (0u32..1000).map(|i| {
        let x = i as f64;
        (x, 0.0, x + 1.0, 1.0, i)
    });
    let tree = make_tree(items);

    // Line: (0,0.5) → (500,0.5) → (500,10) — horizontal then vertical kink.
    // The horizontal segment y=0.5 passes through x=0..500.
    // Entries 0..500 have y range 0..1 which covers y=0.5 → should be found.
    // Entries 500..1000 are at x>500 and the vertical segment x=500 y=0.5..10
    // only touches x=500 exactly (boundary of entry 500, but entry 499 ends at x=500).
    let line = [(0.0f64, 0.5f64), (500.0, 0.5), (500.0, 10.0)];
    let results = tree.search_line(&line, 0.0);

    // Allow boundary precision: at least 498 of the 500 horizontal entries hit.
    assert!(
        results.len() >= 498,
        "expected at least 498 entries on the horizontal segment 0..500, got {}",
        results.len()
    );
    // Sanity cap: cannot exceed 502 (entries 0..500 + boundary entries 500/501).
    assert!(
        results.len() <= 502,
        "unexpectedly many entries returned: {}",
        results.len()
    );
}
