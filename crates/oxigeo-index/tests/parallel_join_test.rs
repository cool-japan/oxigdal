//! Integration tests for the rayon-parallel spatial join
//! ([`oxigeo_index::spatial_join_parallel`]).

#![cfg(feature = "parallel")]

use oxigeo_index::{
    Bbox2D, ParallelJoinOptions, RTree, SpatialQuery, spatial_join_parallel,
    spatial_join_with_options,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// A deterministic 64-bit LCG so that tests do not depend on `rand`.
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
        (self.state >> 11) as f64 / (1u64 << 53) as f64
    }

    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next_f64()
    }
}

fn make_bbox(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Bbox2D {
    Bbox2D::new(min_x, min_y, max_x, max_y).expect("valid bbox in test")
}

/// Build an RTree of small axis-aligned boxes seeded deterministically.
fn build_left(n: usize, seed: u64) -> RTree<u32> {
    let mut rng = Lcg::new(seed);
    let mut tree: RTree<u32> = RTree::new();
    for i in 0..n {
        let x = rng.range(0.0, 100.0);
        let y = rng.range(0.0, 100.0);
        let w = rng.range(0.1, 2.0);
        let h = rng.range(0.1, 2.0);
        tree.insert(make_bbox(x, y, x + w, y + h), i as u32);
    }
    tree
}

/// Build a flat `(bbox, id)` slice seeded deterministically.
fn build_right(n: usize, seed: u64) -> Vec<(Bbox2D, u32)> {
    let mut rng = Lcg::new(seed);
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let x = rng.range(0.0, 100.0);
        let y = rng.range(0.0, 100.0);
        let w = rng.range(0.1, 2.0);
        let h = rng.range(0.1, 2.0);
        out.push((make_bbox(x, y, x + w, y + h), i as u32));
    }
    out
}

/// Run the sequential join `SpatialQuery::spatial_join` and produce the
/// equivalent owned `Vec<(A, B)>` for comparison with the parallel variant.
///
/// `right_rtree` is built from the same `right` slice used by the parallel
/// path so that *every* pair the parallel version can produce is also produced
/// by the sequential one (and vice-versa).
fn sequential_pairs(left: &RTree<u32>, right: &[(Bbox2D, u32)]) -> Vec<(u32, u32)> {
    let mut right_rtree: RTree<u32> = RTree::new();
    for (bbox, val) in right {
        right_rtree.insert(*bbox, *val);
    }
    SpatialQuery::spatial_join(left, &right_rtree)
        .into_iter()
        .map(|(a, b)| (*a, *b))
        .collect()
}

/// Sort `(u32, u32)` pairs so that two result sets can be compared with
/// `assert_eq!` regardless of iteration order.
fn sort_pairs(mut v: Vec<(u32, u32)>) -> Vec<(u32, u32)> {
    v.sort_unstable();
    v
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn test_parallel_join_empty_inputs_returns_empty() {
    let left: RTree<u32> = RTree::new();
    let right: Vec<(Bbox2D, u32)> = Vec::new();
    let out = spatial_join_parallel(&left, &right);
    assert!(out.is_empty(), "empty inputs must return empty result");
}

#[test]
fn test_parallel_join_left_empty_returns_empty() {
    let left: RTree<u32> = RTree::new();
    let right = vec![
        (make_bbox(0.0, 0.0, 1.0, 1.0), 1u32),
        (make_bbox(2.0, 2.0, 3.0, 3.0), 2u32),
    ];
    let out = spatial_join_parallel(&left, &right);
    assert!(
        out.is_empty(),
        "left side empty must yield no pairs (no candidates to match against)"
    );
}

#[test]
fn test_parallel_join_right_empty_returns_empty() {
    let mut left: RTree<u32> = RTree::new();
    left.insert(make_bbox(0.0, 0.0, 1.0, 1.0), 1);
    left.insert(make_bbox(2.0, 2.0, 3.0, 3.0), 2);
    let right: Vec<(Bbox2D, u32)> = Vec::new();
    let out = spatial_join_parallel(&left, &right);
    assert!(
        out.is_empty(),
        "right side empty must short-circuit to empty result"
    );
}

#[test]
fn test_parallel_join_matches_sequential_results() {
    let left = build_left(64, 0xC001_D00D);
    let right = build_right(32, 0xDEAD_BEEF);

    let parallel = sort_pairs(spatial_join_parallel(&left, &right));
    let sequential = sort_pairs(sequential_pairs(&left, &right));

    assert_eq!(
        parallel, sequential,
        "parallel join must yield the same multiset of pairs as the sequential one"
    );
}

#[test]
fn test_parallel_join_handles_no_overlap() {
    // Left bboxes live in x ∈ [0, 10], right bboxes live in x ∈ [100, 110].
    let mut left: RTree<u32> = RTree::new();
    for i in 0..5 {
        let x = i as f64;
        left.insert(make_bbox(x, 0.0, x + 0.5, 0.5), i as u32);
    }
    let right: Vec<(Bbox2D, u32)> = (0..5)
        .map(|i| {
            let x = 100.0 + i as f64;
            (make_bbox(x, 0.0, x + 0.5, 0.5), 100 + i as u32)
        })
        .collect();

    let out = spatial_join_parallel(&left, &right);
    assert!(
        out.is_empty(),
        "disjoint bbox sets must produce zero pairs (got {} pairs)",
        out.len()
    );
}

#[test]
fn test_parallel_join_handles_single_thread_option() {
    let left = build_left(40, 0x1234_5678);
    let right = build_right(20, 0x9ABC_DEF0);

    let opts = ParallelJoinOptions {
        chunk_size: None,
        max_threads: Some(1),
    };
    let single = sort_pairs(spatial_join_with_options(&left, &right, &opts));
    let sequential = sort_pairs(sequential_pairs(&left, &right));

    assert_eq!(
        single, sequential,
        "max_threads=1 must still produce the full set of join pairs"
    );
}

#[test]
fn test_parallel_join_chunk_size_one_works() {
    let left = build_left(40, 0x5555);
    let right = build_right(30, 0xAAAA);

    let opts = ParallelJoinOptions {
        chunk_size: Some(1),
        max_threads: None,
    };
    let got = sort_pairs(spatial_join_with_options(&left, &right, &opts));
    let want = sort_pairs(sequential_pairs(&left, &right));

    assert_eq!(
        got, want,
        "chunk_size=1 must still produce the full set of join pairs"
    );
}

#[test]
fn test_parallel_join_with_options_default_works() {
    let left = build_left(50, 0xF00D);
    let right = build_right(25, 0xBEEF);

    let opts = ParallelJoinOptions::default();
    let got = sort_pairs(spatial_join_with_options(&left, &right, &opts));
    let want = sort_pairs(sequential_pairs(&left, &right));

    assert_eq!(
        got, want,
        "default options must behave identically to spatial_join_parallel"
    );
}

#[test]
fn test_parallel_join_1000_left_500_right_consistent() {
    // Larger, but still bounded so the test runs in a few hundred milliseconds.
    let left = build_left(1_000, 0x0011_2233);
    let right = build_right(500, 0x4455_6677);

    let parallel = sort_pairs(spatial_join_parallel(&left, &right));
    let sequential = sort_pairs(sequential_pairs(&left, &right));

    assert_eq!(
        parallel.len(),
        sequential.len(),
        "parallel and sequential joins must produce the same number of pairs"
    );
    assert_eq!(
        parallel, sequential,
        "parallel and sequential joins must produce identical pair multisets"
    );
}

#[test]
fn test_parallel_join_returns_all_overlapping_pairs() {
    // Build a known overlap pattern: every right bbox covers exactly two left
    // bboxes, so the expected count is `2 * right.len()`.
    let mut left: RTree<u32> = RTree::new();
    // 8 unit boxes at x = 0, 2, 4, ..., 14
    for i in 0..8 {
        let x = (i as f64) * 2.0;
        left.insert(make_bbox(x, 0.0, x + 1.0, 1.0), i as u32);
    }

    // Each right bbox spans [2*k, 2*k+3] which overlaps the left boxes at
    // index k and k+1 (the next box at x=2*k+2 starts within the span).
    let right: Vec<(Bbox2D, u32)> = (0..7)
        .map(|k| {
            let x = (k as f64) * 2.0;
            (make_bbox(x, 0.0, x + 3.0, 1.0), 1000 + k as u32)
        })
        .collect();

    let pairs = spatial_join_parallel(&left, &right);
    assert_eq!(
        pairs.len(),
        2 * right.len(),
        "each right bbox must overlap exactly two left bboxes by construction"
    );

    // Also verify equivalence with the sequential join.
    let parallel = sort_pairs(pairs.clone());
    let sequential = sort_pairs(sequential_pairs(&left, &right));
    assert_eq!(parallel, sequential);
}
