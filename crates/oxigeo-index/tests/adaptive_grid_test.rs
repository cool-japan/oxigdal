//! Integration tests for [`AdaptiveGrid`] — the loose-quadtree spatial index.
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use oxigeo_index::{AdaptiveGrid, Bbox2D};

// ---------------------------------------------------------------------------
// Deterministic LCG (no `rand` crate) — same constants used elsewhere in the
// crate's test suite for reproducibility.
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

    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + self.next_f64() * (hi - lo)
    }
}

// ---------------------------------------------------------------------------
// Small helpers used in multiple tests
// ---------------------------------------------------------------------------

fn world_unit() -> Bbox2D {
    Bbox2D::new(0.0, 0.0, 100.0, 100.0).unwrap()
}

fn small_bbox(x: f64, y: f64) -> Bbox2D {
    // 0.1×0.1 square — sits entirely inside a single deep quadrant so it can
    // migrate as far down as `max_depth` allows.
    Bbox2D::new(x, y, x + 0.1, y + 0.1).unwrap()
}

// ---------------------------------------------------------------------------
// Test 1 — search on an empty grid returns nothing
// ---------------------------------------------------------------------------

#[test]
fn test_adaptive_grid_empty_search_returns_empty() {
    let grid: AdaptiveGrid<u32> = AdaptiveGrid::new(world_unit(), 4, 4);
    let hits = grid.search(world_unit());
    assert!(hits.is_empty());
    assert_eq!(grid.len(), 0);
    assert!(grid.is_empty());
    assert_eq!(grid.leaf_count(), 1);
    assert_eq!(grid.cell_count(), 1);
}

// ---------------------------------------------------------------------------
// Test 2 — insert one item, query overlapping it returns it
// ---------------------------------------------------------------------------

#[test]
fn test_adaptive_grid_single_insert_and_search() {
    let mut grid: AdaptiveGrid<u32> = AdaptiveGrid::new(world_unit(), 4, 4);
    let item = Bbox2D::new(10.0, 10.0, 20.0, 20.0).unwrap();
    grid.insert(item, 42_u32);

    assert_eq!(grid.len(), 1);
    assert!(!grid.is_empty());

    // Query overlapping
    let query = Bbox2D::new(15.0, 15.0, 25.0, 25.0).unwrap();
    let hits = grid.search(query);
    assert_eq!(hits.len(), 1);
    assert_eq!(*hits[0], 42_u32);
}

// ---------------------------------------------------------------------------
// Test 3 — leaf splits when `max_items_per_cell` is exceeded
// ---------------------------------------------------------------------------

#[test]
fn test_adaptive_grid_splits_when_max_items_exceeded() {
    // World 0..100, max_items=2.  We deliberately insert 3 small items all
    // sitting inside the south-west quadrant of the root (0..50, 0..50) so
    // they can migrate down after the split.
    let mut grid: AdaptiveGrid<u32> = AdaptiveGrid::new(world_unit(), 2, 4);

    grid.insert(small_bbox(1.0, 1.0), 1);
    assert_eq!(grid.leaf_count(), 1, "no subdivision after first insert");

    grid.insert(small_bbox(2.0, 2.0), 2);
    assert_eq!(grid.leaf_count(), 1, "no subdivision while at threshold");

    // Third item — exceeds threshold, root should subdivide.
    grid.insert(small_bbox(3.0, 3.0), 3);
    assert!(
        grid.leaf_count() > 1,
        "leaf_count should grow after subdivision (got {})",
        grid.leaf_count()
    );
    assert!(
        grid.depth() >= 1,
        "depth should be at least 1 after one split (got {})",
        grid.depth()
    );
}

// ---------------------------------------------------------------------------
// Test 4 — never split past `max_depth`
// ---------------------------------------------------------------------------

#[test]
fn test_adaptive_grid_does_not_split_beyond_max_depth() {
    // max_depth=0 disables subdivision entirely — everything lives at the root.
    let mut grid: AdaptiveGrid<u32> = AdaptiveGrid::new(world_unit(), 2, 0);

    for i in 0..50 {
        grid.insert(small_bbox(i as f64, i as f64), i as u32);
    }
    assert_eq!(grid.depth(), 0, "depth must stay 0 when max_depth=0");
    assert_eq!(grid.leaf_count(), 1, "root remains the only leaf");
    assert_eq!(grid.len(), 50);

    // All items should still be searchable.
    let hits = grid.search(world_unit());
    assert_eq!(hits.len(), 50);
}

// ---------------------------------------------------------------------------
// Test 5 — search returns only items whose bbox overlaps the query
// ---------------------------------------------------------------------------

#[test]
fn test_adaptive_grid_search_returns_overlapping_items() {
    let mut grid: AdaptiveGrid<u32> = AdaptiveGrid::new(world_unit(), 4, 6);

    // Place ten well-separated items along the diagonal x=y.
    for i in 0..10 {
        let base = i as f64 * 10.0;
        grid.insert(
            Bbox2D::new(base, base, base + 1.0, base + 1.0).unwrap(),
            i as u32,
        );
    }

    // Query covering items 0..=4 only.
    let query = Bbox2D::new(0.0, 0.0, 41.5, 41.5).unwrap();
    let hits: Vec<u32> = grid.search(query).into_iter().copied().collect();
    let mut sorted = hits;
    sorted.sort_unstable();
    assert_eq!(
        sorted,
        vec![0, 1, 2, 3, 4],
        "only the first five items should be returned"
    );
}

// ---------------------------------------------------------------------------
// Test 6 — disjoint items must not appear in the query result
// ---------------------------------------------------------------------------

#[test]
fn test_adaptive_grid_search_excludes_disjoint() {
    let mut grid: AdaptiveGrid<u32> = AdaptiveGrid::new(world_unit(), 4, 6);

    // Insert item in the south-west corner.
    grid.insert(small_bbox(1.0, 1.0), 1);
    // Insert item in the north-east corner.
    grid.insert(small_bbox(90.0, 90.0), 2);

    // Query the south-west corner only.
    let query = Bbox2D::new(0.0, 0.0, 5.0, 5.0).unwrap();
    let hits: Vec<u32> = grid.search(query).into_iter().copied().collect();
    assert_eq!(hits.len(), 1, "only the SW item should be in range");
    assert_eq!(hits[0], 1, "the SW item is the one we expect");
}

// ---------------------------------------------------------------------------
// Test 7 — items that span multiple children stay pinned at the internal node
// ---------------------------------------------------------------------------

#[test]
fn test_adaptive_grid_large_item_stays_at_internal_node() {
    // max_items=1 forces a split as soon as the second item arrives.
    let mut grid: AdaptiveGrid<u32> = AdaptiveGrid::new(world_unit(), 1, 4);

    // Small item that fits entirely in the SW child quadrant.
    let small = Bbox2D::new(1.0, 1.0, 2.0, 2.0).unwrap();
    grid.insert(small, 10_u32);

    // Large item spanning the full world — straddles all four child quadrants.
    let large = Bbox2D::new(0.0, 0.0, 100.0, 100.0).unwrap();
    grid.insert(large, 20_u32);

    // The grid should have subdivided (we exceeded max_items=1).
    assert!(
        grid.leaf_count() > 1,
        "expected subdivision after second insert"
    );

    // Both items must be findable.
    let hits_full: Vec<u32> = grid.search(world_unit()).into_iter().copied().collect();
    let mut sorted_full = hits_full;
    sorted_full.sort_unstable();
    assert_eq!(sorted_full, vec![10, 20]);

    // Query covering only the NE corner — the small (SW) item must not appear,
    // but the large spanning item must.
    let ne_query = Bbox2D::new(80.0, 80.0, 100.0, 100.0).unwrap();
    let hits_ne: Vec<u32> = grid.search(ne_query).into_iter().copied().collect();
    assert_eq!(
        hits_ne,
        vec![20],
        "spanning item must be returned for any sub-query"
    );

    // Query restricted to the SW corner — both items must appear (the small
    // one because it's there, the large one because it spans everywhere).
    let sw_query = Bbox2D::new(0.0, 0.0, 3.0, 3.0).unwrap();
    let hits_sw: Vec<u32> = grid.search(sw_query).into_iter().copied().collect();
    let mut sorted_sw = hits_sw;
    sorted_sw.sort_unstable();
    assert_eq!(sorted_sw, vec![10, 20]);
}

// ---------------------------------------------------------------------------
// Test 8 — 1000 random inserts and a window query agree with brute-force
// ---------------------------------------------------------------------------

#[test]
fn test_adaptive_grid_1000_random_inserts_search_correctness() {
    let mut grid: AdaptiveGrid<u32> = AdaptiveGrid::new(world_unit(), 8, 8);
    let mut rng = Lcg::new(0xC0FFEE);

    // Generate 1000 small bboxes scattered uniformly across the world.
    let mut entries: Vec<(Bbox2D, u32)> = Vec::with_capacity(1000);
    for i in 0..1000 {
        let cx = rng.range(0.0, 100.0);
        let cy = rng.range(0.0, 100.0);
        let half = rng.range(0.05, 0.5);
        let bbox = Bbox2D::new(cx - half, cy - half, cx + half, cy + half).unwrap();
        entries.push((bbox, i as u32));
        grid.insert(bbox, i as u32);
    }
    assert_eq!(grid.len(), 1000);

    // Choose a fixed query window and validate against a brute-force scan.
    let query = Bbox2D::new(20.0, 30.0, 60.0, 70.0).unwrap();
    let mut tree_hits: Vec<u32> = grid.search(query).into_iter().copied().collect();
    tree_hits.sort_unstable();

    let mut brute_hits: Vec<u32> = entries
        .iter()
        .filter(|(b, _)| b.intersects(&query))
        .map(|(_, v)| *v)
        .collect();
    brute_hits.sort_unstable();

    assert_eq!(
        tree_hits, brute_hits,
        "adaptive grid result must match brute-force result"
    );
    assert!(
        !tree_hits.is_empty(),
        "the chosen window should match at least some items"
    );
}

// ---------------------------------------------------------------------------
// Test 9 — leaf_count grows as more items force more subdivisions
// ---------------------------------------------------------------------------

#[test]
fn test_adaptive_grid_leaf_count_grows_with_subdivision() {
    let mut grid: AdaptiveGrid<u32> = AdaptiveGrid::new(world_unit(), 2, 6);
    let initial = grid.leaf_count();
    assert_eq!(initial, 1);

    // Insert many small items clustered in the SW corner so every split keeps
    // pushing items deeper into the same sub-tree.
    let mut prev = initial;
    let mut grew = false;
    for i in 0..40 {
        let x = (i as f64) * 0.05;
        let y = (i as f64) * 0.05;
        grid.insert(small_bbox(x, y), i as u32);
        let now = grid.leaf_count();
        if now > prev {
            grew = true;
        }
        prev = now;
    }
    assert!(
        grew,
        "leaf_count should have grown at least once during the 40 inserts"
    );
    assert!(
        grid.leaf_count() > initial,
        "final leaf_count {} should exceed initial {}",
        grid.leaf_count(),
        initial
    );
}

// ---------------------------------------------------------------------------
// Test 10 — depth() reports the deepest leaf accurately
// ---------------------------------------------------------------------------

#[test]
fn test_adaptive_grid_depth_reports_correctly() {
    // Single-leaf tree
    let grid: AdaptiveGrid<u32> = AdaptiveGrid::new(world_unit(), 4, 4);
    assert_eq!(grid.depth(), 0, "fresh grid is a single leaf at depth 0");

    // Force exactly one subdivision: max_items=1, max_depth=1, insert two
    // items in different quadrants so they migrate down to depth 1.
    let mut grid: AdaptiveGrid<u32> = AdaptiveGrid::new(world_unit(), 1, 1);
    grid.insert(small_bbox(1.0, 1.0), 1); // SW quadrant
    grid.insert(small_bbox(60.0, 60.0), 2); // NE quadrant
    assert_eq!(
        grid.depth(),
        1,
        "after one split, deepest leaf is at depth 1 (got {})",
        grid.depth()
    );
    // Subdivision happened: there should be 4 leaves now (the four quadrants).
    assert_eq!(grid.leaf_count(), 4);

    // Both items should still be findable in their respective corners.
    let sw_hits: Vec<u32> = grid
        .search(Bbox2D::new(0.0, 0.0, 5.0, 5.0).unwrap())
        .into_iter()
        .copied()
        .collect();
    assert_eq!(sw_hits, vec![1]);
    let ne_hits: Vec<u32> = grid
        .search(Bbox2D::new(55.0, 55.0, 100.0, 100.0).unwrap())
        .into_iter()
        .copied()
        .collect();
    assert_eq!(ne_hits, vec![2]);
}
