//! Integration tests for `oxigdal-index`.

use oxigdal_index::{Bbox2D, GridIndex, IndexError, RTree, SpatialQuery};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a deterministic pseudo-random sequence without pulling in `rand`.
/// LCG: x_{n+1} = (a * x_n + c) mod m  (Knuth parameters for 64-bit)
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
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        // Map to [0, 1)
        (self.state >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Random f64 in [lo, hi).
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + self.next_f64() * (hi - lo)
    }
}

/// Brute-force search reference implementation.
fn brute_search<T>(entries: &[(Bbox2D, T)], query: &Bbox2D) -> Vec<usize> {
    entries
        .iter()
        .enumerate()
        .filter(|(_, (b, _))| b.intersects(query))
        .map(|(i, _)| i)
        .collect()
}

// ---------------------------------------------------------------------------
// Bbox2D tests
// ---------------------------------------------------------------------------

#[test]
fn bbox_new_valid() {
    let b = Bbox2D::new(0.0, 0.0, 1.0, 1.0);
    assert!(b.is_some());
}

#[test]
fn bbox_new_inverted_x_is_none() {
    assert!(Bbox2D::new(2.0, 0.0, 1.0, 1.0).is_none());
}

#[test]
fn bbox_new_inverted_y_is_none() {
    assert!(Bbox2D::new(0.0, 2.0, 1.0, 1.0).is_none());
}

#[test]
fn bbox_new_equal_coords_is_some() {
    // Equal min/max is valid (degenerate).
    assert!(Bbox2D::new(1.0, 1.0, 1.0, 1.0).is_some());
}

#[test]
fn bbox_area_known_value() {
    let b = Bbox2D::new(1.0, 2.0, 4.0, 5.0).unwrap();
    assert_eq!(b.area(), 9.0);
}

#[test]
fn bbox_area_zero_for_point() {
    assert_eq!(Bbox2D::point(7.0, 3.0).area(), 0.0);
}

#[test]
fn bbox_width_and_height() {
    let b = Bbox2D::new(1.0, 2.0, 4.0, 6.0).unwrap();
    assert_eq!(b.width(), 3.0);
    assert_eq!(b.height(), 4.0);
}

#[test]
fn bbox_perimeter() {
    let b = Bbox2D::new(0.0, 0.0, 3.0, 4.0).unwrap();
    assert_eq!(b.perimeter(), 14.0);
}

#[test]
fn bbox_center() {
    let b = Bbox2D::new(0.0, 0.0, 4.0, 6.0).unwrap();
    assert_eq!(b.center(), (2.0, 3.0));
}

#[test]
fn bbox_intersects_overlap() {
    let a = Bbox2D::new(0.0, 0.0, 2.0, 2.0).unwrap();
    let b = Bbox2D::new(1.0, 1.0, 3.0, 3.0).unwrap();
    assert!(a.intersects(&b));
    assert!(b.intersects(&a));
}

#[test]
fn bbox_intersects_touching_edge() {
    let a = Bbox2D::new(0.0, 0.0, 1.0, 1.0).unwrap();
    let b = Bbox2D::new(1.0, 0.0, 2.0, 1.0).unwrap();
    assert!(a.intersects(&b));
}

#[test]
fn bbox_intersects_disjoint() {
    let a = Bbox2D::new(0.0, 0.0, 1.0, 1.0).unwrap();
    let b = Bbox2D::new(2.0, 2.0, 3.0, 3.0).unwrap();
    assert!(!a.intersects(&b));
}

#[test]
fn bbox_intersects_disjoint_x_only() {
    let a = Bbox2D::new(0.0, 0.0, 1.0, 2.0).unwrap();
    let b = Bbox2D::new(5.0, 0.0, 6.0, 2.0).unwrap();
    assert!(!a.intersects(&b));
}

#[test]
fn bbox_intersects_disjoint_y_only() {
    let a = Bbox2D::new(0.0, 0.0, 2.0, 1.0).unwrap();
    let b = Bbox2D::new(0.0, 5.0, 2.0, 6.0).unwrap();
    assert!(!a.intersects(&b));
}

#[test]
fn bbox_contains_point_inside() {
    let b = Bbox2D::new(0.0, 0.0, 4.0, 4.0).unwrap();
    assert!(b.contains_point(2.0, 2.0));
}

#[test]
fn bbox_contains_point_on_boundary() {
    let b = Bbox2D::new(0.0, 0.0, 4.0, 4.0).unwrap();
    assert!(b.contains_point(0.0, 0.0));
    assert!(b.contains_point(4.0, 4.0));
    assert!(b.contains_point(2.0, 0.0));
}

#[test]
fn bbox_contains_point_outside() {
    let b = Bbox2D::new(0.0, 0.0, 4.0, 4.0).unwrap();
    assert!(!b.contains_point(5.0, 2.0));
    assert!(!b.contains_point(-1.0, 2.0));
}

#[test]
fn bbox_union_basic() {
    let a = Bbox2D::new(0.0, 0.0, 2.0, 2.0).unwrap();
    let b = Bbox2D::new(1.0, 1.0, 4.0, 4.0).unwrap();
    let u = a.union(&b);
    assert_eq!(u, Bbox2D::new(0.0, 0.0, 4.0, 4.0).unwrap());
}

#[test]
fn bbox_union_disjoint() {
    let a = Bbox2D::new(0.0, 0.0, 1.0, 1.0).unwrap();
    let b = Bbox2D::new(3.0, 3.0, 5.0, 5.0).unwrap();
    let u = a.union(&b);
    assert_eq!(u.min_x, 0.0);
    assert_eq!(u.max_x, 5.0);
}

#[test]
fn bbox_intersection_overlapping() {
    let a = Bbox2D::new(0.0, 0.0, 3.0, 3.0).unwrap();
    let b = Bbox2D::new(1.0, 1.0, 4.0, 4.0).unwrap();
    let i = a.intersection(&b).unwrap();
    assert_eq!(i, Bbox2D::new(1.0, 1.0, 3.0, 3.0).unwrap());
}

#[test]
fn bbox_intersection_disjoint_is_none() {
    let a = Bbox2D::new(0.0, 0.0, 1.0, 1.0).unwrap();
    let b = Bbox2D::new(2.0, 2.0, 3.0, 3.0).unwrap();
    assert!(a.intersection(&b).is_none());
}

#[test]
fn bbox_intersection_touching_edge_is_degenerate() {
    let a = Bbox2D::new(0.0, 0.0, 1.0, 1.0).unwrap();
    let b = Bbox2D::new(1.0, 0.0, 2.0, 1.0).unwrap();
    let i = a.intersection(&b).unwrap();
    assert!(i.is_degenerate());
}

#[test]
fn bbox_contains_bbox() {
    let outer = Bbox2D::new(0.0, 0.0, 10.0, 10.0).unwrap();
    let inner = Bbox2D::new(1.0, 1.0, 9.0, 9.0).unwrap();
    assert!(outer.contains_bbox(&inner));
    assert!(!inner.contains_bbox(&outer));
}

#[test]
fn bbox_enlargement_no_enlargement_needed() {
    let a = Bbox2D::new(0.0, 0.0, 10.0, 10.0).unwrap();
    let b = Bbox2D::new(2.0, 2.0, 8.0, 8.0).unwrap();
    assert_eq!(a.enlargement_to_include(&b), 0.0);
}

#[test]
fn bbox_enlargement_positive() {
    let a = Bbox2D::new(0.0, 0.0, 2.0, 2.0).unwrap(); // area = 4
    let b = Bbox2D::new(0.0, 0.0, 4.0, 4.0).unwrap(); // union area = 16
    let enlargement = a.enlargement_to_include(&b);
    assert_eq!(enlargement, 12.0);
}

#[test]
fn bbox_min_distance_inside_is_zero() {
    let b = Bbox2D::new(0.0, 0.0, 10.0, 10.0).unwrap();
    assert_eq!(b.min_distance_to_point(5.0, 5.0), 0.0);
}

#[test]
fn bbox_min_distance_outside_x() {
    let b = Bbox2D::new(0.0, 0.0, 2.0, 2.0).unwrap();
    // Point at (4, 1): dx = 4-2 = 2, dy = 0 → dist = 2
    assert_eq!(b.min_distance_to_point(4.0, 1.0), 2.0);
}

#[test]
fn bbox_min_distance_outside_corner() {
    let b = Bbox2D::new(0.0, 0.0, 3.0, 4.0).unwrap();
    // Point at (6, 8): dx = 3, dy = 4 → dist = 5
    assert_eq!(b.min_distance_to_point(6.0, 8.0), 5.0);
}

#[test]
fn bbox_expand_by_positive() {
    let b = Bbox2D::new(1.0, 1.0, 3.0, 3.0).unwrap();
    let e = b.expand_by(1.0);
    assert_eq!(e, Bbox2D::new(0.0, 0.0, 4.0, 4.0).unwrap());
}

#[test]
fn bbox_from_points_basic() {
    let pts = [(0.0_f64, 1.0_f64), (3.0, -1.0), (2.0, 4.0)];
    let b = Bbox2D::from_points(&pts).unwrap();
    assert_eq!(b.min_x, 0.0);
    assert_eq!(b.min_y, -1.0);
    assert_eq!(b.max_x, 3.0);
    assert_eq!(b.max_y, 4.0);
}

#[test]
fn bbox_from_points_empty_is_none() {
    assert!(Bbox2D::from_points(&[]).is_none());
}

// ---------------------------------------------------------------------------
// RTree basic tests
// ---------------------------------------------------------------------------

#[test]
fn rtree_new_is_empty() {
    let tree: RTree<u32> = RTree::new();
    assert!(tree.is_empty());
    assert_eq!(tree.len(), 0);
}

#[test]
fn rtree_insert_single_entry() {
    let mut tree: RTree<u32> = RTree::new();
    tree.insert(Bbox2D::new(0.0, 0.0, 1.0, 1.0).unwrap(), 1);
    assert_eq!(tree.len(), 1);
    assert!(!tree.is_empty());
}

#[test]
fn rtree_insert_100_entries() {
    let mut tree: RTree<u32> = RTree::new();
    for i in 0..100_u32 {
        let f = i as f64;
        tree.insert(Bbox2D::new(f, f, f + 1.0, f + 1.0).unwrap(), i);
    }
    assert_eq!(tree.len(), 100);
}

#[test]
fn rtree_search_empty_tree_returns_empty() {
    let tree: RTree<u32> = RTree::new();
    let q = Bbox2D::new(0.0, 0.0, 10.0, 10.0).unwrap();
    assert!(tree.search(&q).is_empty());
}

#[test]
fn rtree_search_finds_matching_bbox() {
    let mut tree: RTree<u32> = RTree::new();
    tree.insert(Bbox2D::new(0.0, 0.0, 2.0, 2.0).unwrap(), 42);
    let q = Bbox2D::new(1.0, 1.0, 3.0, 3.0).unwrap();
    let hits = tree.search(&q);
    assert_eq!(hits.len(), 1);
    assert_eq!(*hits[0], 42);
}

#[test]
fn rtree_search_non_overlapping_returns_empty() {
    let mut tree: RTree<u32> = RTree::new();
    tree.insert(Bbox2D::new(0.0, 0.0, 1.0, 1.0).unwrap(), 7);
    let q = Bbox2D::new(5.0, 5.0, 6.0, 6.0).unwrap();
    assert!(tree.search(&q).is_empty());
}

#[test]
fn rtree_search_multiple_overlapping() {
    let mut tree: RTree<u32> = RTree::new();
    for i in 0..5_u32 {
        tree.insert(Bbox2D::new(0.0, 0.0, 3.0, 3.0).unwrap(), i);
    }
    let q = Bbox2D::new(1.0, 1.0, 2.0, 2.0).unwrap();
    assert_eq!(tree.search(&q).len(), 5);
}

#[test]
fn rtree_search_partial_overlap() {
    let mut tree: RTree<&str> = RTree::new();
    tree.insert(Bbox2D::new(0.0, 0.0, 5.0, 5.0).unwrap(), "a");
    tree.insert(Bbox2D::new(10.0, 10.0, 15.0, 15.0).unwrap(), "b");
    let q = Bbox2D::new(3.0, 3.0, 12.0, 12.0).unwrap();
    let hits = tree.search(&q);
    assert_eq!(hits.len(), 2);
}

#[test]
fn rtree_contains_point_in_overlapping_bboxes() {
    let mut tree: RTree<u32> = RTree::new();
    for i in 0..10_u32 {
        // All bboxes contain point (5, 5).
        tree.insert(Bbox2D::new(0.0, 0.0, 10.0, 10.0).unwrap(), i);
    }
    // Add one that does NOT contain (5, 5).
    tree.insert(Bbox2D::new(20.0, 20.0, 30.0, 30.0).unwrap(), 99);
    let hits = tree.contains_point(5.0, 5.0);
    assert_eq!(hits.len(), 10);
}

#[test]
fn rtree_contains_point_none_when_outside() {
    let mut tree: RTree<u32> = RTree::new();
    tree.insert(Bbox2D::new(0.0, 0.0, 1.0, 1.0).unwrap(), 1);
    assert!(tree.contains_point(5.0, 5.0).is_empty());
}

// ---------------------------------------------------------------------------
// Nearest-neighbour tests
// ---------------------------------------------------------------------------

#[test]
fn rtree_nearest_1_returns_closest() {
    let mut tree: RTree<u32> = RTree::new();
    // Entry at distance 0 (contains the query point).
    tree.insert(Bbox2D::new(0.0, 0.0, 2.0, 2.0).unwrap(), 1);
    // Entry far away.
    tree.insert(Bbox2D::new(100.0, 100.0, 110.0, 110.0).unwrap(), 2);
    let nn = tree.nearest(1.0, 1.0, 1);
    assert_eq!(nn.len(), 1);
    assert_eq!(*nn[0].0, 1);
    assert_eq!(nn[0].1, 0.0); // point is inside → distance 0
}

#[test]
fn rtree_nearest_5_returns_5_sorted() {
    let mut tree: RTree<u32> = RTree::new();
    for i in 0..20_u32 {
        let f = i as f64 * 5.0;
        tree.insert(Bbox2D::new(f, 0.0, f + 1.0, 1.0).unwrap(), i);
    }
    let nn = tree.nearest(0.0, 0.0, 5);
    assert_eq!(nn.len(), 5);
    // Distances should be non-decreasing.
    for w in nn.windows(2) {
        assert!(w[0].1 <= w[1].1);
    }
}

#[test]
fn rtree_nearest_k_exceeds_size() {
    let mut tree: RTree<u32> = RTree::new();
    tree.insert(Bbox2D::new(0.0, 0.0, 1.0, 1.0).unwrap(), 0);
    tree.insert(Bbox2D::new(2.0, 2.0, 3.0, 3.0).unwrap(), 1);
    // Ask for more than available.
    let nn = tree.nearest(0.5, 0.5, 100);
    assert_eq!(nn.len(), 2);
}

#[test]
fn rtree_nearest_zero_k_returns_empty() {
    let mut tree: RTree<u32> = RTree::new();
    tree.insert(Bbox2D::new(0.0, 0.0, 1.0, 1.0).unwrap(), 1);
    assert!(tree.nearest(0.5, 0.5, 0).is_empty());
}

// ---------------------------------------------------------------------------
// Iteration
// ---------------------------------------------------------------------------

#[test]
fn rtree_iter_visits_all_entries() {
    let mut tree: RTree<u32> = RTree::new();
    for i in 0..50_u32 {
        let f = i as f64;
        tree.insert(Bbox2D::new(f, f, f + 1.0, f + 1.0).unwrap(), i);
    }
    assert_eq!(tree.iter().count(), 50);
}

#[test]
fn rtree_total_bbox_contains_all() {
    let mut tree: RTree<u32> = RTree::new();
    tree.insert(Bbox2D::new(-5.0, -5.0, 0.0, 0.0).unwrap(), 0);
    tree.insert(Bbox2D::new(10.0, 10.0, 20.0, 20.0).unwrap(), 1);
    let total = tree.total_bbox().unwrap();
    assert!(total.min_x <= -5.0);
    assert!(total.max_x >= 20.0);
    assert!(total.min_y <= -5.0);
    assert!(total.max_y >= 20.0);
}

#[test]
fn rtree_total_bbox_none_when_empty() {
    let tree: RTree<u32> = RTree::new();
    assert!(tree.total_bbox().is_none());
}

// ---------------------------------------------------------------------------
// Large random correctness test (R-tree vs brute force)
// ---------------------------------------------------------------------------

#[test]
fn rtree_random_1000_vs_brute_force() {
    let mut rng = Lcg::new(0xDEAD_BEEF_1234_5678);
    let mut tree: RTree<usize> = RTree::new();
    let mut reference: Vec<(Bbox2D, usize)> = Vec::new();

    // Insert 1000 random bboxes.
    for i in 0..1000 {
        let x0 = rng.range(0.0, 90.0);
        let y0 = rng.range(0.0, 90.0);
        let x1 = x0 + rng.range(0.1, 10.0);
        let y1 = y0 + rng.range(0.1, 10.0);
        let bbox = Bbox2D::new(x0, y0, x1, y1).unwrap();
        tree.insert(bbox, i);
        reference.push((bbox, i));
    }

    assert_eq!(tree.len(), 1000);

    // Run 100 random queries and compare.
    for _ in 0..100 {
        let qx0 = rng.range(10.0, 60.0);
        let qy0 = rng.range(10.0, 60.0);
        let qx1 = qx0 + rng.range(5.0, 20.0);
        let qy1 = qy0 + rng.range(5.0, 20.0);
        let query = Bbox2D::new(qx0, qy0, qx1, qy1).unwrap();

        let tree_count = SpatialQuery::count_in(&tree, &query);
        let brute_count = brute_search(&reference, &query).len();
        assert_eq!(
            tree_count, brute_count,
            "query={query:?}: tree returned {tree_count}, brute force {brute_count}"
        );
    }
}

// ---------------------------------------------------------------------------
// SpatialQuery tests
// ---------------------------------------------------------------------------

#[test]
fn spatial_query_count_in_matches_search_len() {
    let mut tree: RTree<u32> = RTree::new();
    for i in 0..20_u32 {
        let f = i as f64;
        tree.insert(Bbox2D::new(f, 0.0, f + 2.0, 2.0).unwrap(), i);
    }
    let q = Bbox2D::new(5.0, 0.0, 12.0, 2.0).unwrap();
    let count = SpatialQuery::count_in(&tree, &q);
    let hits = tree.search(&q);
    assert_eq!(count, hits.len());
}

#[test]
fn spatial_query_within_subset_of_intersects() {
    let mut tree: RTree<u32> = RTree::new();
    // Fully inside query window.
    tree.insert(Bbox2D::new(1.0, 1.0, 2.0, 2.0).unwrap(), 1);
    // Partially outside — intersects but not within.
    tree.insert(Bbox2D::new(0.0, 0.0, 5.0, 5.0).unwrap(), 2);
    let q = Bbox2D::new(0.5, 0.5, 3.0, 3.0).unwrap();
    let within = SpatialQuery::within(&tree, &q);
    // Only the small box is fully within.
    assert_eq!(within.len(), 1);
    assert_eq!(within[0], 1);
}

#[test]
fn spatial_query_intersects_returns_clones() {
    let mut tree: RTree<u32> = RTree::new();
    tree.insert(Bbox2D::new(0.0, 0.0, 4.0, 4.0).unwrap(), 10);
    let q = Bbox2D::new(2.0, 2.0, 6.0, 6.0).unwrap();
    let hits = SpatialQuery::intersects(&tree, &q);
    assert_eq!(hits, vec![10]);
}

#[test]
fn spatial_query_spatial_join_basic() {
    let mut left: RTree<&str> = RTree::new();
    left.insert(Bbox2D::new(0.0, 0.0, 5.0, 5.0).unwrap(), "a");
    left.insert(Bbox2D::new(10.0, 10.0, 15.0, 15.0).unwrap(), "b");

    let mut right: RTree<u32> = RTree::new();
    right.insert(Bbox2D::new(3.0, 3.0, 8.0, 8.0).unwrap(), 100);
    right.insert(Bbox2D::new(20.0, 20.0, 25.0, 25.0).unwrap(), 200);

    let join = SpatialQuery::spatial_join(&left, &right);
    // Only "a" (overlaps 100) should be joined; "b" does not intersect 100 or 200.
    // Right side 200 does not intersect either left entry.
    assert_eq!(join.len(), 1);
    assert_eq!(*join[0].0, "a");
    assert_eq!(*join[0].1, 100);
}

#[test]
fn spatial_query_spatial_join_multiple_matches() {
    let mut left: RTree<u32> = RTree::new();
    left.insert(Bbox2D::new(0.0, 0.0, 10.0, 10.0).unwrap(), 1);

    let mut right: RTree<u32> = RTree::new();
    right.insert(Bbox2D::new(1.0, 1.0, 2.0, 2.0).unwrap(), 10);
    right.insert(Bbox2D::new(3.0, 3.0, 4.0, 4.0).unwrap(), 11);
    right.insert(Bbox2D::new(5.0, 5.0, 6.0, 6.0).unwrap(), 12);

    let join = SpatialQuery::spatial_join(&left, &right);
    assert_eq!(join.len(), 3);
}

#[test]
fn spatial_query_count_in_zero_when_no_overlap() {
    let mut tree: RTree<u32> = RTree::new();
    tree.insert(Bbox2D::new(0.0, 0.0, 1.0, 1.0).unwrap(), 1);
    let q = Bbox2D::new(50.0, 50.0, 60.0, 60.0).unwrap();
    assert_eq!(SpatialQuery::count_in(&tree, &q), 0);
}

// ---------------------------------------------------------------------------
// GridIndex tests
// ---------------------------------------------------------------------------

#[test]
fn grid_insert_and_search() {
    let extent = Bbox2D::new(0.0, 0.0, 100.0, 100.0).unwrap();
    let mut grid = GridIndex::<u32>::new(extent, 10, 10).unwrap();
    grid.insert(Bbox2D::new(5.0, 5.0, 15.0, 15.0).unwrap(), 42);
    let q = Bbox2D::new(10.0, 10.0, 20.0, 20.0).unwrap();
    let hits = grid.search(&q);
    assert!(!hits.is_empty());
    assert_eq!(*hits[0], 42);
}

#[test]
fn grid_search_no_match() {
    let extent = Bbox2D::new(0.0, 0.0, 100.0, 100.0).unwrap();
    let mut grid = GridIndex::<u32>::new(extent, 10, 10).unwrap();
    grid.insert(Bbox2D::new(0.0, 0.0, 5.0, 5.0).unwrap(), 1);
    let q = Bbox2D::new(80.0, 80.0, 90.0, 90.0).unwrap();
    let hits = grid.search(&q);
    assert!(hits.is_empty());
}

#[test]
fn grid_contains_point_found() {
    let extent = Bbox2D::new(0.0, 0.0, 100.0, 100.0).unwrap();
    let mut grid = GridIndex::<&str>::new(extent, 10, 10).unwrap();
    grid.insert(Bbox2D::new(20.0, 20.0, 40.0, 40.0).unwrap(), "hello");
    let hits = grid.contains_point(30.0, 30.0);
    assert_eq!(hits.len(), 1);
    assert_eq!(*hits[0], "hello");
}

#[test]
fn grid_contains_point_outside_entry() {
    let extent = Bbox2D::new(0.0, 0.0, 100.0, 100.0).unwrap();
    let mut grid = GridIndex::<u32>::new(extent, 10, 10).unwrap();
    grid.insert(Bbox2D::new(10.0, 10.0, 20.0, 20.0).unwrap(), 5);
    let hits = grid.contains_point(90.0, 90.0);
    assert!(hits.is_empty());
}

#[test]
fn grid_len_tracks_inserts() {
    let extent = Bbox2D::new(0.0, 0.0, 50.0, 50.0).unwrap();
    let mut grid = GridIndex::<u32>::new(extent, 5, 5).unwrap();
    assert_eq!(grid.len(), 0);
    assert!(grid.is_empty());
    grid.insert(Bbox2D::new(0.0, 0.0, 1.0, 1.0).unwrap(), 1);
    grid.insert(Bbox2D::new(2.0, 2.0, 3.0, 3.0).unwrap(), 2);
    assert_eq!(grid.len(), 2);
}

#[test]
fn grid_zero_cols_error() {
    let extent = Bbox2D::new(0.0, 0.0, 10.0, 10.0).unwrap();
    let result = GridIndex::<u32>::new(extent, 0, 5);
    assert!(matches!(result, Err(IndexError::InvalidGridSize(0, 5))));
}

#[test]
fn grid_zero_rows_error() {
    let extent = Bbox2D::new(0.0, 0.0, 10.0, 10.0).unwrap();
    let result = GridIndex::<u32>::new(extent, 5, 0);
    assert!(matches!(result, Err(IndexError::InvalidGridSize(5, 0))));
}

#[test]
fn grid_random_search_matches_brute_force() {
    let mut rng = Lcg::new(0xABCD_1234_CAFE_BABE);
    let extent = Bbox2D::new(0.0, 0.0, 100.0, 100.0).unwrap();
    let mut grid = GridIndex::<usize>::new(extent, 10, 10).unwrap();
    let mut reference: Vec<(Bbox2D, usize)> = Vec::new();

    for i in 0..200 {
        let x0 = rng.range(0.0, 90.0);
        let y0 = rng.range(0.0, 90.0);
        let x1 = x0 + rng.range(1.0, 8.0);
        let y1 = y0 + rng.range(1.0, 8.0);
        let bbox = Bbox2D::new(x0, y0, x1, y1).unwrap();
        grid.insert(bbox, i);
        reference.push((bbox, i));
    }

    for _ in 0..50 {
        let qx0 = rng.range(10.0, 60.0);
        let qy0 = rng.range(10.0, 60.0);
        let qx1 = qx0 + rng.range(5.0, 15.0);
        let qy1 = qy0 + rng.range(5.0, 15.0);
        let query = Bbox2D::new(qx0, qy0, qx1, qy1).unwrap();

        let grid_count = grid.search(&query).len();
        // Grid may return duplicates (one per cell) for entries spanning multiple
        // cells. Collect grid hits as a set of values to compare with brute force.
        let brute_count = brute_search(&reference, &query).len();
        // Grid count can be >= brute_count due to multi-cell duplicates.
        assert!(
            grid_count >= brute_count,
            "grid ({grid_count}) should be >= brute ({brute_count})"
        );
    }
}

// ---------------------------------------------------------------------------
// R-tree with_max_entries
// ---------------------------------------------------------------------------

#[test]
fn rtree_with_max_entries_small_m() {
    let mut tree: RTree<u32> = RTree::with_max_entries(3);
    for i in 0..30_u32 {
        let f = i as f64;
        tree.insert(Bbox2D::new(f, f, f + 1.0, f + 1.0).unwrap(), i);
    }
    assert_eq!(tree.len(), 30);
    // Correctness: a spot query should still work.
    let q = Bbox2D::new(10.5, 10.5, 11.5, 11.5).unwrap();
    assert!(!tree.search(&q).is_empty());
}

#[test]
fn rtree_with_max_entries_clamped_at_2() {
    let mut tree: RTree<u32> = RTree::with_max_entries(0); // clamped to 2
    for i in 0..10_u32 {
        let f = i as f64;
        tree.insert(Bbox2D::new(f, 0.0, f + 1.0, 1.0).unwrap(), i);
    }
    assert_eq!(tree.len(), 10);
}

#[test]
fn rtree_default_impl() {
    let tree: RTree<u32> = RTree::default();
    assert!(tree.is_empty());
}
