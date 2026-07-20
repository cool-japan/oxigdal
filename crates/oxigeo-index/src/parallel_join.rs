//! Parallel spatial join using rayon.
//!
//! This module provides a feature-gated parallel implementation of the R-tree
//! spatial join.  It is conceptually equivalent to
//! [`SpatialQuery::spatial_join`](crate::rtree::SpatialQuery::spatial_join),
//! but the right-hand input is supplied as a flat slice
//! `&[(Bbox2D, B)]` instead of a second [`RTree`], so that the work can be
//! partitioned across rayon worker threads.
//!
//! # Why a slice instead of an `RTree`?
//!
//! For a spatial join `J = { (a, b) | bbox(a) ∩ bbox(b) ≠ ∅ }` the natural
//! parallelisation strategy is to partition the *probe* side (`right`) into
//! independent chunks and run the join in parallel: each worker only needs
//! shared read-only access to `left` (the indexed side) and writes its own
//! local results vector.  The two vectors are then merged via rayon's parallel
//! `collect`.
//!
//! Supplying `right` as a slice avoids paying the cost of building a second
//! R-tree for the probe side, which would dominate the cost of the join for
//! moderate input sizes.  If the user already has an `RTree` for the right
//! side they can simply collect its entries (e.g. via `RTree::iter()`) into a
//! `Vec<(Bbox2D, B)>` once and reuse the slice for multiple joins.
//!
//! # Result ordering
//!
//! Because chunks are processed in parallel, the order of returned pairs may
//! differ from the sequential join.  Tests that compare against the
//! sequential variant must therefore sort both result vectors first.
//!
//! # Determinism
//!
//! For a fixed input, the *set* of returned pairs is fully deterministic and
//! identical to the sequential join.  Only the ordering inside the result
//! vector depends on rayon's scheduling.

#![cfg(feature = "parallel")]

use crate::bbox::Bbox2D;
use crate::rtree::RTree;
use rayon::prelude::*;

/// Options that control the behaviour of [`spatial_join_with_options`].
///
/// # Tuning
///
/// * [`chunk_size`](Self::chunk_size) controls the granularity of work each
///   rayon task processes.  Smaller chunks improve load balance at the cost
///   of scheduling overhead; larger chunks reduce overhead but can leave
///   workers idle at the tail of the computation.  When `None`, an automatic
///   chunk size is chosen so that there are roughly `4 * nthreads` chunks in
///   total, which empirically balances overhead and load on most workloads.
/// * [`max_threads`](Self::max_threads) restricts the number of rayon
///   workers used for the join.  When `None`, the call inherits the
///   current rayon pool (typically the global one).  When `Some(n)`, a
///   private [`rayon::ThreadPool`] with `n` workers is constructed for the
///   duration of the call.  This is useful for benchmarking and for tests
///   that pin execution to a single thread for determinism.
#[derive(Debug, Clone, Copy, Default)]
pub struct ParallelJoinOptions {
    /// Number of right-hand entries processed by each rayon task.
    pub chunk_size: Option<usize>,
    /// Maximum number of rayon worker threads to use for this join.
    pub max_threads: Option<usize>,
}

/// Parallel R-tree spatial join with default options.
///
/// For each entry `(bbox, b)` in `right`, every value in `left` whose bbox
/// intersects `bbox` is paired with `b` and emitted.  The result is a flat
/// `Vec<(A, B)>`; pairs are cloned out of the underlying storage so the
/// caller owns the returned data.
///
/// The order of pairs in the returned vector is *unspecified* and may vary
/// between runs.  The *set* of pairs is identical to that produced by the
/// sequential [`SpatialQuery::spatial_join`](crate::rtree::SpatialQuery::spatial_join).
///
/// # Examples
///
/// ```ignore
/// use oxigeo_index::{Bbox2D, RTree, spatial_join_parallel};
///
/// let mut left: RTree<&'static str> = RTree::new();
/// left.insert(Bbox2D::new(0.0, 0.0, 1.0, 1.0).unwrap(), "A");
/// left.insert(Bbox2D::new(2.0, 2.0, 3.0, 3.0).unwrap(), "B");
///
/// let right = vec![
///     (Bbox2D::new(0.5, 0.5, 0.6, 0.6).unwrap(), 1u32),
///     (Bbox2D::new(2.5, 2.5, 2.6, 2.6).unwrap(), 2u32),
/// ];
///
/// let pairs = spatial_join_parallel(&left, &right);
/// assert_eq!(pairs.len(), 2);
/// ```
pub fn spatial_join_parallel<A, B>(left: &RTree<A>, right: &[(Bbox2D, B)]) -> Vec<(A, B)>
where
    A: Clone + Send + Sync,
    B: Clone + Send + Sync,
{
    spatial_join_with_options(left, right, &ParallelJoinOptions::default())
}

/// Parallel R-tree spatial join with explicit tuning options.
///
/// See [`ParallelJoinOptions`] for the meaning of each field.
///
/// # Behaviour
///
/// * When `right` is empty, an empty `Vec` is returned immediately without
///   touching the rayon pool.
/// * When `left` is empty, every probe returns no matches and an empty
///   `Vec` is returned (no allocation for results).
/// * When [`max_threads`](ParallelJoinOptions::max_threads) is `Some(n)`,
///   a private [`rayon::ThreadPool`] with `n` workers is built.  If pool
///   construction fails (extremely rare — e.g. when the OS refuses to spawn
///   threads), the join transparently falls back to the ambient rayon pool
///   so callers never observe a panic from this function.
pub fn spatial_join_with_options<A, B>(
    left: &RTree<A>,
    right: &[(Bbox2D, B)],
    opts: &ParallelJoinOptions,
) -> Vec<(A, B)>
where
    A: Clone + Send + Sync,
    B: Clone + Send + Sync,
{
    if right.is_empty() {
        return Vec::new();
    }

    // Auto chunk_size: target ~4 chunks per thread for good load balance.
    // We must clamp to >= 1 so that par_chunks() never sees a zero step.
    let nthreads = opts
        .max_threads
        .unwrap_or_else(rayon::current_num_threads)
        .max(1);
    let chunk_size = opts
        .chunk_size
        .unwrap_or_else(|| (right.len() / (nthreads * 4)).max(1))
        .max(1);

    let do_join = || -> Vec<(A, B)> {
        right
            .par_chunks(chunk_size)
            .flat_map_iter(|chunk| {
                let mut out = Vec::new();
                for (rbbox, rval) in chunk {
                    for left_val in left.search(rbbox) {
                        out.push((left_val.clone(), rval.clone()));
                    }
                }
                out.into_iter()
            })
            .collect()
    };

    if let Some(max) = opts.max_threads {
        match rayon::ThreadPoolBuilder::new().num_threads(max).build() {
            Ok(pool) => pool.install(do_join),
            Err(_) => do_join(),
        }
    } else {
        do_join()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_bbox(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Bbox2D {
        Bbox2D::new(min_x, min_y, max_x, max_y).expect("valid bbox in test")
    }

    #[test]
    fn default_options_are_none() {
        let opts = ParallelJoinOptions::default();
        assert!(opts.chunk_size.is_none());
        assert!(opts.max_threads.is_none());
    }

    #[test]
    fn empty_right_short_circuits() {
        let mut left: RTree<u32> = RTree::new();
        left.insert(make_bbox(0.0, 0.0, 1.0, 1.0), 1);
        let right: Vec<(Bbox2D, u32)> = Vec::new();
        let pairs = spatial_join_parallel(&left, &right);
        assert!(pairs.is_empty());
    }

    #[test]
    fn empty_left_returns_empty() {
        let left: RTree<u32> = RTree::new();
        let right = vec![(make_bbox(0.0, 0.0, 1.0, 1.0), 10u32)];
        let pairs = spatial_join_parallel(&left, &right);
        assert!(pairs.is_empty());
    }

    #[test]
    fn auto_chunk_size_is_at_least_one() {
        // Tiny right slice (smaller than nthreads * 4) — the computed chunk
        // size would be zero without the .max(1) clamp.
        let mut left: RTree<u32> = RTree::new();
        left.insert(make_bbox(0.0, 0.0, 10.0, 10.0), 1);
        let right = vec![(make_bbox(1.0, 1.0, 2.0, 2.0), 100u32)];
        let pairs = spatial_join_parallel(&left, &right);
        assert_eq!(pairs.len(), 1);
    }
}
