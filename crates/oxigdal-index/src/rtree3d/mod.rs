//! 3D R*-tree spatial index.
//!
//! Provides [`RTree3D`] — a parallel implementation to the 2D [`crate::rtree::RTree`]
//! designed for point clouds and volumetric datasets.  The two trees share no
//! code so that enhancements to 3D never retroactively affect the well-tested
//! 2D tree, and vice-versa.
//!
//! ## Algorithm highlights
//!
//! * **Split strategy** — R*-tree axis-split: pick the axis with the smallest
//!   surface-area margin-sum; within that axis pick the distribution with
//!   minimum volume overlap (total-volume tie-break).
//! * **Forced reinsertion** — on the first overflow per level, the p = 15
//!   farthest entries from the node centre are removed and reinserted.
//! * **Bulk loading** — Sort-Tile-Recursive tiling along X → Y → Z for a
//!   tight, balanced initial tree in O(n log n).
//! * **k-NN** — min-heap priority-queue traversal with 3D MINDIST² pruning.

use crate::bbox3d::Bbox3D;

mod bulk;
mod knn;
pub(crate) mod node;

use node::{
    InternalEntry3D, InternalNode3D, LeafEntry3D, LeafNode3D, MAX_ENTRIES_3D, MIN_ENTRIES_3D,
    Node3D, REINSERT_P, collect_all_leaf_values3d, internal3d_bbox, leaf3d_bbox, node3d_bbox,
    search_node3d, split_internal3d, split_leaf3d,
};

// ---------------------------------------------------------------------------
// RTree3D
// ---------------------------------------------------------------------------

/// A 3D R*-tree spatial index mapping [`Bbox3D`] volumes to values of type `T`.
///
/// # Example
/// ```
/// use oxigdal_index::{RTree3D, Bbox3D};
/// let mut tree: RTree3D<u32> = RTree3D::new();
/// let bbox = Bbox3D::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0).unwrap();
/// tree.insert(bbox, 42_u32);
/// assert_eq!(tree.len(), 1);
/// ```
pub struct RTree3D<T: Clone> {
    root: Option<Node3D<T>>,
    size: usize,
    max_entries: usize,
    min_entries: usize,
    /// Per-level reinsert flag — tracks whether we have already done a forced
    /// reinsertion at the given depth during the current `insert` call.  We
    /// reset it before every top-level insertion.
    reinsert_done: Vec<bool>,
}

impl<T: Clone> Default for RTree3D<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone> RTree3D<T> {
    /// Create a new, empty `RTree3D` with the default R*-tree capacity
    /// (`M = 50`, `m = 20`).
    pub fn new() -> Self {
        Self {
            root: None,
            size: 0,
            max_entries: MAX_ENTRIES_3D,
            min_entries: MIN_ENTRIES_3D,
            reinsert_done: Vec::new(),
        }
    }

    /// Number of entries stored in the tree.
    #[inline]
    pub fn len(&self) -> usize {
        self.size
    }

    /// Whether the tree contains no entries.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    // ------------------------------------------------------------------
    // Insertion (with forced reinsertion)
    // ------------------------------------------------------------------

    /// Insert `value` associated with bounding box `bbox`.
    pub fn insert(&mut self, bbox: Bbox3D, value: T) {
        // Reset the per-level reinsertion tracking for this call.
        self.reinsert_done.clear();

        let entry = LeafEntry3D { bbox, value };

        // We maintain a pending_reinsert queue: pairs (depth, entry) collected
        // during forced reinsertion that must be re-inserted at the root.
        let mut pending: Vec<LeafEntry3D<T>> = Vec::new();

        self.insert_entry(entry, &mut pending);

        // Drain re-insert queue, re-inserting each entry from the root without
        // incrementing size (they are already counted).
        for pending_entry in pending {
            self.insert_entry_no_count(pending_entry);
        }

        self.size += 1;
    }

    /// Core entry insertion that propagates splits and forced reinsertion.
    fn insert_entry(&mut self, entry: LeafEntry3D<T>, pending: &mut Vec<LeafEntry3D<T>>) {
        match self.root.take() {
            None => {
                self.root = Some(Node3D::Leaf(LeafNode3D {
                    entries: vec![entry],
                }));
            }
            Some(root) => {
                let (updated_root, maybe_split) = self.insert_into(root, entry, 0, pending);
                if let Some((split_bbox, split_node)) = maybe_split {
                    let old_bbox = node3d_bbox(&updated_root);
                    let new_root = Node3D::Internal(InternalNode3D {
                        entries: vec![
                            InternalEntry3D {
                                bbox: old_bbox,
                                child: Box::new(updated_root),
                            },
                            InternalEntry3D {
                                bbox: split_bbox,
                                child: Box::new(split_node),
                            },
                        ],
                    });
                    self.root = Some(new_root);
                } else {
                    self.root = Some(updated_root);
                }
            }
        }
    }

    /// Re-insert an entry that was ejected during forced reinsertion.
    /// Does NOT increment `self.size`.
    fn insert_entry_no_count(&mut self, entry: LeafEntry3D<T>) {
        match self.root.take() {
            None => {
                self.root = Some(Node3D::Leaf(LeafNode3D {
                    entries: vec![entry],
                }));
            }
            Some(root) => {
                let mut dummy: Vec<LeafEntry3D<T>> = Vec::new();
                let (updated_root, maybe_split) = self.insert_into(root, entry, 0, &mut dummy);
                if let Some((split_bbox, split_node)) = maybe_split {
                    let old_bbox = node3d_bbox(&updated_root);
                    let new_root = Node3D::Internal(InternalNode3D {
                        entries: vec![
                            InternalEntry3D {
                                bbox: old_bbox,
                                child: Box::new(updated_root),
                            },
                            InternalEntry3D {
                                bbox: split_bbox,
                                child: Box::new(split_node),
                            },
                        ],
                    });
                    self.root = Some(new_root);
                } else {
                    self.root = Some(updated_root);
                }
                // Drain any secondary pending — they become additional no-count reinsertions.
                for e in dummy {
                    self.insert_entry_no_count(e);
                }
            }
        }
    }

    /// Recursive insertion.
    ///
    /// Returns `(updated_node, Option<(split_bbox, split_node)>)`.
    /// When a forced reinsertion is triggered at a leaf, the ejected entries
    /// are appended to `pending` and `None` is returned for the split.
    fn insert_into(
        &mut self,
        node: Node3D<T>,
        entry: LeafEntry3D<T>,
        depth: usize,
        pending: &mut Vec<LeafEntry3D<T>>,
    ) -> (Node3D<T>, Option<(Bbox3D, Node3D<T>)>) {
        // Ensure the reinsert_done vector is long enough.
        if self.reinsert_done.len() <= depth {
            self.reinsert_done.resize(depth + 1, false);
        }

        match node {
            Node3D::Leaf(mut leaf) => {
                leaf.entries.push(entry);

                if leaf.entries.len() > self.max_entries {
                    // Forced reinsertion: first overflow at this level.
                    if !self.reinsert_done[depth] {
                        self.reinsert_done[depth] = true;

                        // Compute the enclosing center.
                        let enclosing = leaf3d_bbox(&leaf);
                        let center = enclosing.center();

                        // Eject the p farthest entries.
                        let ejected = node::extract_farthest_leaf_entries(
                            &mut leaf.entries,
                            center,
                            REINSERT_P,
                        );
                        pending.extend(ejected);

                        // The leaf is now within capacity.
                        return (Node3D::Leaf(leaf), None);
                    }

                    // Already did a reinsertion at this level — split.
                    let (left, right) = split_leaf3d(leaf, self.min_entries);
                    let right_bbox = leaf3d_bbox(&right);
                    return (Node3D::Leaf(left), Some((right_bbox, Node3D::Leaf(right))));
                }

                (Node3D::Leaf(leaf), None)
            }

            Node3D::Internal(mut internal) => {
                let best_idx = node::choose_subtree3d(&internal.entries, &entry.bbox);
                let chosen = internal.entries.remove(best_idx);
                let (updated_child, maybe_split) =
                    self.insert_into(*chosen.child, entry, depth + 1, pending);

                let updated_bbox = node3d_bbox(&updated_child);
                internal.entries.push(InternalEntry3D {
                    bbox: updated_bbox,
                    child: Box::new(updated_child),
                });

                if let Some((split_bbox, split_child)) = maybe_split {
                    internal.entries.push(InternalEntry3D {
                        bbox: split_bbox,
                        child: Box::new(split_child),
                    });
                }

                if internal.entries.len() > self.max_entries {
                    if !self.reinsert_done[depth] {
                        self.reinsert_done[depth] = true;
                        // For internal nodes, reinsertion is more complex.
                        // We fall through to splitting to keep the tree valid.
                    }
                    let (left, right) = split_internal3d(internal, self.min_entries);
                    let right_bbox = internal3d_bbox(&right);
                    return (
                        Node3D::Internal(left),
                        Some((right_bbox, Node3D::Internal(right))),
                    );
                }

                (Node3D::Internal(internal), None)
            }
        }
    }

    // ------------------------------------------------------------------
    // Search
    // ------------------------------------------------------------------

    /// Find all entries whose bbox intersects `query`.
    pub fn search(&self, query: &Bbox3D) -> Vec<&T> {
        let mut results = Vec::new();
        if let Some(ref root) = self.root {
            search_node3d(root, query, &mut results);
        }
        results
    }

    // ------------------------------------------------------------------
    // k-NN
    // ------------------------------------------------------------------

    /// Return up to `k` entries nearest to point `(x, y, z)`, ordered by
    /// ascending bbox-to-point distance.
    ///
    /// Uses 3D MINDIST² pruning for efficiency.  Returns references to values
    /// only (not distances); use `Bbox3D::min_distance_to_point` for exact
    /// distances if needed.
    pub fn nearest_k(&self, x: f64, y: f64, z: f64, k: usize) -> Vec<&T> {
        if k == 0 {
            return Vec::new();
        }
        match self.root {
            Some(ref root) => knn::knn_search3d(root, x, y, z, k),
            None => Vec::new(),
        }
    }

    // ------------------------------------------------------------------
    // Bulk loading
    // ------------------------------------------------------------------

    /// Build an `RTree3D` from pre-collected items using Sort-Tile-Recursive.
    ///
    /// Much faster than repeated `insert` calls when all data is available
    /// upfront.  O(n log n) time, produces a well-packed tree.
    pub fn bulk_load(items: Vec<(Bbox3D, T)>) -> Self {
        let max_entries = MAX_ENTRIES_3D;
        let min_entries = MIN_ENTRIES_3D;
        match bulk::str_bulk_load3d(items, max_entries) {
            Some((root, count)) => Self {
                root: Some(root),
                size: count,
                max_entries,
                min_entries,
                reinsert_done: Vec::new(),
            },
            None => Self {
                root: None,
                size: 0,
                max_entries,
                min_entries,
                reinsert_done: Vec::new(),
            },
        }
    }

    // ------------------------------------------------------------------
    // Collapse root
    // ------------------------------------------------------------------

    fn collapse_root_if_single_child(&mut self) {
        let should_collapse = matches!(
            &self.root,
            Some(Node3D::Internal(internal)) if internal.entries.len() == 1
        );
        if should_collapse
            && let Some(Node3D::Internal(mut internal)) = self.root.take()
            && let Some(entry) = internal.entries.pop()
        {
            self.root = Some(*entry.child);
        }
    }
}

// ---------------------------------------------------------------------------
// Deletion (requires T: PartialEq)
// ---------------------------------------------------------------------------

impl<T: Clone + PartialEq> RTree3D<T> {
    /// Remove the first entry matching `(bbox, value)`.
    ///
    /// Returns `true` on success, `false` if no matching entry was found.
    ///
    /// After removal the tree is condensed: under-full nodes are dissolved and
    /// their entries reinserted.
    pub fn remove(&mut self, bbox: &Bbox3D, value: &T) -> bool {
        let root = match self.root.take() {
            Some(r) => r,
            None => return false,
        };

        let mut orphans: Vec<(Bbox3D, T)> = Vec::new();
        let result = self.remove_from(root, bbox, value, &mut orphans);

        match result {
            RemoveResult3D::NotFound(node) => {
                self.root = Some(node);
                false
            }
            RemoveResult3D::Found { remaining } => {
                self.root = remaining;
                self.size -= 1;

                self.collapse_root_if_single_child();

                // Reinsert orphaned entries without changing size.
                for (ob, ov) in orphans {
                    self.insert_entry_no_count(LeafEntry3D {
                        bbox: ob,
                        value: ov,
                    });
                }

                true
            }
        }
    }

    fn remove_from(
        &self,
        node: Node3D<T>,
        bbox: &Bbox3D,
        value: &T,
        orphans: &mut Vec<(Bbox3D, T)>,
    ) -> RemoveResult3D<T> {
        match node {
            Node3D::Leaf(mut leaf) => {
                let pos = leaf
                    .entries
                    .iter()
                    .position(|e| e.bbox == *bbox && e.value == *value);
                match pos {
                    Some(idx) => {
                        leaf.entries.remove(idx);
                        if leaf.entries.is_empty() {
                            RemoveResult3D::Found { remaining: None }
                        } else {
                            RemoveResult3D::Found {
                                remaining: Some(Node3D::Leaf(leaf)),
                            }
                        }
                    }
                    None => RemoveResult3D::NotFound(Node3D::Leaf(leaf)),
                }
            }
            Node3D::Internal(mut internal) => {
                for i in 0..internal.entries.len() {
                    if !internal.entries[i].bbox.intersects(bbox) {
                        continue;
                    }

                    let entry = internal.entries.remove(i);
                    let result = self.remove_from(*entry.child, bbox, value, orphans);

                    match result {
                        RemoveResult3D::NotFound(child) => {
                            internal.entries.insert(
                                i,
                                InternalEntry3D {
                                    bbox: entry.bbox,
                                    child: Box::new(child),
                                },
                            );
                        }
                        RemoveResult3D::Found { remaining } => {
                            if let Some(child_node) = remaining {
                                let child_len = match &child_node {
                                    Node3D::Leaf(l) => l.entries.len(),
                                    Node3D::Internal(ii) => ii.entries.len(),
                                };
                                if child_len < self.min_entries {
                                    collect_all_leaf_values3d(&child_node, orphans);
                                } else {
                                    let new_bbox = node3d_bbox(&child_node);
                                    internal.entries.insert(
                                        i,
                                        InternalEntry3D {
                                            bbox: new_bbox,
                                            child: Box::new(child_node),
                                        },
                                    );
                                }
                            }

                            if internal.entries.is_empty() {
                                return RemoveResult3D::Found { remaining: None };
                            }
                            return RemoveResult3D::Found {
                                remaining: Some(Node3D::Internal(internal)),
                            };
                        }
                    }
                }

                RemoveResult3D::NotFound(Node3D::Internal(internal))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// RemoveResult helper
// ---------------------------------------------------------------------------

enum RemoveResult3D<T> {
    NotFound(Node3D<T>),
    Found { remaining: Option<Node3D<T>> },
}
