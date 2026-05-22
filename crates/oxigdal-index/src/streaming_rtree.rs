//! Streaming / online R*-tree insertion with a pending-buffer abstraction.
//!
//! [`StreamingRTree`] wraps the underlying [`RTree`] with a configurable
//! pending-insertion buffer.  Individual inserts push into the buffer; when
//! the buffer reaches a threshold (by count **or** by fraction of total items),
//! it is flushed into the stable tree via bulk `insert` calls.
//!
//! Queries are always answered by unioning results from the stable tree and
//! the pending buffer, so no insertions are ever invisible to callers.
//!
//! # Why a pending buffer?
//!
//! The R*-tree's `insert` keeps the tree well-balanced but pays O(log n) per
//! call with forced-reinsertion overhead.  When a data source delivers items in
//! short bursts, batching them first and flushing as a group amortises that
//! cost.  A full STR bulk-rebuild of the whole tree would be even faster but
//! requires draining and refilling the stable tree, which this implementation
//! supports via [`StreamingRTree::rebuild_from_scratch`].
//!
//! # Example
//!
//! ```rust
//! use oxigdal_index::{Bbox2D, StreamingRTree, StreamingInsertConfig};
//!
//! let config = StreamingInsertConfig {
//!     rebalance_threshold: 4,
//!     max_pending_fraction: 0.5,
//! };
//! let mut st: StreamingRTree<u32> = StreamingRTree::with_config(config);
//!
//! for i in 0..4_u32 {
//!     let f = i as f64;
//!     let bbox = Bbox2D::new(f, f, f + 1.0, f + 1.0).unwrap();
//!     st.insert(bbox, i);
//! }
//! // At this point either a rebuild happened or items sit in pending.
//! assert_eq!(st.len(), 4);
//! ```

use crate::bbox::Bbox2D;
use crate::rtree::RTree;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for [`StreamingRTree`] rebalancing behaviour.
///
/// When *either* trigger fires, the pending buffer is flushed into the stable
/// tree.
///
/// | Field                 | Default | Description                                     |
/// |-----------------------|---------|-------------------------------------------------|
/// | `rebalance_threshold` | `512`   | Flush when pending item count reaches this.     |
/// | `max_pending_fraction`| `0.3`   | Flush when pending / total exceeds this ratio.  |
#[derive(Debug, Clone)]
pub struct StreamingInsertConfig {
    /// Number of pending insertions before triggering a rebuild.
    ///
    /// A threshold of `0` means *always flush immediately* (i.e. no buffering).
    pub rebalance_threshold: usize,

    /// If `pending.len() / (stable.len() + pending.len())` exceeds this
    /// fraction, force an immediate rebuild regardless of the threshold.
    ///
    /// Must be in `[0.0, 1.0]`.  A value of `0.0` means flush every time a
    /// single item is inserted (like threshold = 0).  A value of `1.0`
    /// effectively disables the fraction trigger.
    pub max_pending_fraction: f64,
}

impl Default for StreamingInsertConfig {
    fn default() -> Self {
        Self {
            rebalance_threshold: 512,
            max_pending_fraction: 0.3,
        }
    }
}

// ---------------------------------------------------------------------------
// StreamingRTree
// ---------------------------------------------------------------------------

/// An [`RTree`] wrapper that amortises online insertions via a pending buffer.
///
/// New items are pushed into `pending`.  When `pending.len() >=
/// config.rebalance_threshold` **or** `pending / total > max_pending_fraction`,
/// the pending items are flushed into the stable tree with individual
/// [`RTree::insert`] calls.
///
/// Queries always union results from both the stable tree and the pending
/// buffer — no insertion is ever invisible.
///
/// # Type parameter
///
/// `T` must satisfy `Clone` (required by the underlying [`RTree`]).
///
/// # Thread safety
///
/// `StreamingRTree` is **not** `Sync` — concurrent readers + a writer require
/// external synchronisation (e.g. `RwLock<StreamingRTree<T>>`).
pub struct StreamingRTree<T: Clone> {
    /// The stable, fully-indexed portion of the data set.
    stable: RTree<T>,
    /// Items that have been inserted but not yet flushed into `stable`.
    pending: Vec<(Bbox2D, T)>,
    /// Monotonically increasing count of every `insert` call ever made.
    total_inserted: usize,
    /// Number of flush (rebuild) operations performed so far.
    total_rebuilds: usize,
    /// Runtime configuration.
    config: StreamingInsertConfig,
}

impl<T: Clone> Default for StreamingRTree<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone> StreamingRTree<T> {
    // ------------------------------------------------------------------
    // Constructors
    // ------------------------------------------------------------------

    /// Create a new, empty `StreamingRTree` with the default configuration
    /// (`rebalance_threshold = 512`, `max_pending_fraction = 0.3`).
    pub fn new() -> Self {
        Self::with_config(StreamingInsertConfig::default())
    }

    /// Create a new, empty `StreamingRTree` with a custom [`StreamingInsertConfig`].
    pub fn with_config(config: StreamingInsertConfig) -> Self {
        Self {
            stable: RTree::new(),
            pending: Vec::new(),
            total_inserted: 0,
            total_rebuilds: 0,
            config,
        }
    }

    // ------------------------------------------------------------------
    // Insertion
    // ------------------------------------------------------------------

    /// Insert an item.
    ///
    /// The item is placed in the pending buffer.  If the pending buffer now
    /// satisfies either rebuild trigger, a flush is performed immediately
    /// before returning.
    ///
    /// # Cost
    ///
    /// O(1) amortised.  The flush, when it occurs, is O(p log(n + p)) where
    /// `n` is the current stable tree size and `p` is the number of pending
    /// items.
    pub fn insert(&mut self, bbox: Bbox2D, value: T) {
        self.pending.push((bbox, value));
        self.total_inserted += 1;
        if self.should_flush() {
            self.flush_pending();
        }
    }

    // ------------------------------------------------------------------
    // Manual flush / rebuild
    // ------------------------------------------------------------------

    /// Flush all pending items into the stable tree.
    ///
    /// Does nothing when the pending buffer is already empty.
    ///
    /// This is identical to what is done automatically when a trigger fires.
    /// Call it explicitly when you want deterministic flush behaviour (e.g.
    /// before running a large query or handing the index off to another thread).
    pub fn rebuild(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        self.flush_pending();
    }

    /// Rebuild the entire tree from scratch using the STR bulk-load algorithm.
    ///
    /// This collects **all** entries (stable + pending) into a temporary
    /// vector and builds a brand-new [`RTree`] using
    /// [`RTree::bulk_load`](RTree::bulk_load).  The result is usually a more
    /// compactly packed tree than repeated individual insertions can produce.
    ///
    /// After this call `pending` is empty and `stable` contains all items.
    ///
    /// # Cost
    ///
    /// O(n log n) where `n` is the total number of entries.
    pub fn rebuild_from_scratch(&mut self) {
        // Collect everything from the stable tree.
        let mut all_items: Vec<(Bbox2D, T)> = self
            .stable
            .iter()
            .map(|(bbox, val)| (*bbox, val.clone()))
            .collect();

        // Append the pending items.
        all_items.append(&mut self.pending);

        // Replace stable with a freshly bulk-loaded tree.
        self.stable = RTree::bulk_load(all_items);
        self.total_rebuilds += 1;
    }

    // ------------------------------------------------------------------
    // Search
    // ------------------------------------------------------------------

    /// Find all items whose bounding box intersects `query`.
    ///
    /// Results are collected from both the stable tree (via
    /// [`RTree::search`]) and the pending buffer (linear scan with bbox
    /// intersection test).  The order of results is unspecified.
    ///
    /// Returns references into both `self.stable` and `self.pending`; the
    /// lifetime `'a` is tied to `&'a self`.
    pub fn search(&self, query: &Bbox2D) -> Vec<&T> {
        let mut results: Vec<&T> = self.stable.search(query);

        // Linear scan of the pending buffer.
        for (bbox, val) in &self.pending {
            if bbox.intersects(query) {
                results.push(val);
            }
        }

        results
    }

    // ------------------------------------------------------------------
    // Size / state accessors
    // ------------------------------------------------------------------

    /// Number of items currently in the stable tree.
    #[inline]
    pub fn stable_len(&self) -> usize {
        self.stable.len()
    }

    /// Number of items currently awaiting flush.
    #[inline]
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// Total number of items across both stable tree and pending buffer.
    #[inline]
    pub fn len(&self) -> usize {
        self.stable.len() + self.pending.len()
    }

    /// Returns `true` if the index contains no items (neither stable nor pending).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Number of flush operations performed so far (auto + manual).
    #[inline]
    pub fn rebuild_count(&self) -> usize {
        self.total_rebuilds
    }

    /// Cumulative count of every [`insert`](Self::insert) call ever made,
    /// including items that have since been merged into the stable tree.
    #[inline]
    pub fn total_inserted(&self) -> usize {
        self.total_inserted
    }

    /// Reference to the current configuration.
    #[inline]
    pub fn config(&self) -> &StreamingInsertConfig {
        &self.config
    }

    /// Mutable reference to the current configuration.
    ///
    /// Changes take effect on the next insertion check; they do not
    /// retroactively trigger a flush.
    #[inline]
    pub fn config_mut(&mut self) -> &mut StreamingInsertConfig {
        &mut self.config
    }

    // ------------------------------------------------------------------
    // Internal helpers
    // ------------------------------------------------------------------

    /// Determine whether the pending buffer should be flushed right now.
    ///
    /// Returns `true` when *either* condition is satisfied:
    ///
    /// 1. `pending.len() >= rebalance_threshold`, **or**
    /// 2. `pending / total > max_pending_fraction` (only when `total > 0`).
    #[inline]
    fn should_flush(&self) -> bool {
        let pending = self.pending.len();
        if pending >= self.config.rebalance_threshold {
            return true;
        }
        let total = self.stable.len() + pending;
        if total > 0 {
            let fraction = pending as f64 / total as f64;
            if fraction > self.config.max_pending_fraction {
                return true;
            }
        }
        false
    }

    /// Drain the pending buffer into the stable tree and increment the rebuild
    /// counter.
    fn flush_pending(&mut self) {
        for (bbox, value) in self.pending.drain(..) {
            self.stable.insert(bbox, value);
        }
        self.total_rebuilds += 1;
    }
}

// ---------------------------------------------------------------------------
// Statistics / diagnostic helpers
// ---------------------------------------------------------------------------

/// A snapshot of `StreamingRTree` metrics at a point in time.
///
/// Returned by [`StreamingRTree::stats`].
#[derive(Debug, Clone, PartialEq)]
pub struct StreamingRTreeStats {
    /// Items in the stable tree.
    pub stable_len: usize,
    /// Items in the pending buffer.
    pub pending_len: usize,
    /// Total items (stable + pending).
    pub total_len: usize,
    /// Total insertions ever made.
    pub total_inserted: usize,
    /// Total flush operations (auto + manual).
    pub total_rebuilds: usize,
    /// Current pending fraction (`pending / total`, or `0.0` when empty).
    pub pending_fraction: f64,
}

impl<T: Clone> StreamingRTree<T> {
    /// Collect diagnostic metrics into a [`StreamingRTreeStats`] snapshot.
    pub fn stats(&self) -> StreamingRTreeStats {
        let stable_len = self.stable.len();
        let pending_len = self.pending.len();
        let total_len = stable_len + pending_len;
        let pending_fraction = if total_len > 0 {
            pending_len as f64 / total_len as f64
        } else {
            0.0
        };
        StreamingRTreeStats {
            stable_len,
            pending_len,
            total_len,
            total_inserted: self.total_inserted,
            total_rebuilds: self.total_rebuilds,
            pending_fraction,
        }
    }
}
