//! Pure-Rust R*-tree spatial index.
//!
//! This implementation uses the **R*-tree split heuristic** (Beckmann,
//! Kriegel, Schneider, Seeger — SIGMOD 1990) together with **forced
//! reinsertion** on the first overflow at each level per top-level insert.
//!
//! The maximum node capacity `M` defaults to 9; the minimum fill
//! `m = ceil(M × 0.4)`.  The reinsertion fraction p = ceil(0.3 × M), min 1.

#[cfg(not(feature = "std"))]
use alloc::{boxed::Box, vec::Vec};

#[cfg(feature = "std")]
use std::collections::HashSet;

#[cfg(not(feature = "std"))]
use alloc::collections::BTreeSet as HashSet;

use crate::bbox::Bbox2D;
use crate::error::IndexError;

mod bulk;
pub mod hilbert;
mod knn;
pub(crate) mod node;
mod serial;

pub use hilbert::HilbertRTree;

use node::{
    InternalEntry, InternalNode, LeafEntry, LeafNode, Node, choose_subtree, collect_all_pairs,
    count_in_node, internal_bbox, leaf_bbox, node_bbox, search_node, search_with_bbox_node,
    split_internal, split_leaf, within_node,
};

// ---------------------------------------------------------------------------
// Helper type alias for the recursive insertion return type.
// ---------------------------------------------------------------------------

/// Return type of the recursive `insert_into` helper:
/// `(updated_node, maybe_split, pending_reinserts)`.
type InsertResult<T> = (Node<T>, Option<(Bbox2D, Node<T>)>, Vec<(Bbox2D, T)>);

// ---------------------------------------------------------------------------
// RTree
// ---------------------------------------------------------------------------

/// An R-tree spatial index mapping [`Bbox2D`] regions to values of type `T`.
///
/// Uses the R*-tree split heuristic (Beckmann et al., SIGMOD 1990) and
/// forced reinsertion to minimise overlap between nodes.
///
/// # Example
/// ```
/// use oxigdal_index::{RTree, Bbox2D};
/// let mut tree: RTree<u32> = RTree::new();
/// let bbox = Bbox2D::new(0.0, 0.0, 1.0, 1.0).unwrap();
/// tree.insert(bbox, 42_u32);
/// assert_eq!(tree.len(), 1);
/// ```
pub struct RTree<T> {
    root: Option<Node<T>>,
    size: usize,
    max_entries: usize,
    min_entries: usize,
    /// p = number of entries to reinsert on forced-reinsertion (ceil(0.3 × M)).
    reinsert_p: usize,
}

impl<T> Default for RTree<T>
where
    T: Clone,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone> RTree<T> {
    /// Create a new, empty R-tree with default node capacity (M = 9).
    pub fn new() -> Self {
        let max_entries = 9;
        let min_entries = ((max_entries as f64) * 0.4).ceil() as usize;
        let reinsert_p = compute_reinsert_p(max_entries);
        Self {
            root: None,
            size: 0,
            max_entries,
            min_entries,
            reinsert_p,
        }
    }

    /// Create a new, empty R-tree with a custom maximum node capacity `M`.
    ///
    /// `max_entries` is clamped to a minimum of `2`.
    pub fn with_max_entries(max_entries: usize) -> Self {
        let max_entries = max_entries.max(2);
        let min_entries = ((max_entries as f64) * 0.4).ceil() as usize;
        let reinsert_p = compute_reinsert_p(max_entries);
        Self {
            root: None,
            size: 0,
            max_entries,
            min_entries,
            reinsert_p,
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

    /// Insert `value` associated with `bbox`.
    pub fn insert(&mut self, bbox: Bbox2D, value: T) {
        let entry = LeafEntry { bbox, value };
        // Per-top-level-insert set of levels that have already triggered
        // forced reinsertion.  This prevents infinite loops: once a level L
        // has been reinserted, subsequent overflows at L go straight to split.
        let mut reinsert_levels: HashSet<usize> = HashSet::new();
        self.insert_entry(entry, &mut reinsert_levels);
        self.size += 1;
    }

    /// Core insertion that does NOT increment `self.size`.  Shared by
    /// `insert` (which manages `reinsert_levels` and bumps `size`) and
    /// condense-tree reinsertion in `remove`.
    fn insert_entry(&mut self, entry: LeafEntry<T>, reinsert_levels: &mut HashSet<usize>) {
        match self.root.take() {
            None => {
                self.root = Some(Node::Leaf(LeafNode {
                    entries: vec![entry],
                }));
            }
            Some(root) => {
                let (updated_root, maybe_split, pending_reinserts) =
                    self.insert_into(root, entry, 0, reinsert_levels);

                // Handle root split.
                let new_root = if let Some((split_bbox, split_node)) = maybe_split {
                    let old_bbox = node_bbox(&updated_root);
                    Node::Internal(InternalNode {
                        entries: vec![
                            InternalEntry {
                                bbox: old_bbox,
                                child: Box::new(updated_root),
                            },
                            InternalEntry {
                                bbox: split_bbox,
                                child: Box::new(split_node),
                            },
                        ],
                    })
                } else {
                    updated_root
                };
                self.root = Some(new_root);

                // Re-insert entries that were ejected by forced reinsertion
                // further down the tree.  These are inserted without bumping
                // `self.size` because they were already counted.
                for (rb, rv) in pending_reinserts {
                    let re_entry = LeafEntry {
                        bbox: rb,
                        value: rv,
                    };
                    self.insert_entry_no_count(re_entry, reinsert_levels);
                }
            }
        }
    }

    /// Recursive insertion helper.
    ///
    /// Returns `(updated_node, maybe_split, pending_reinserts)`.
    ///
    /// * `updated_node`      — the node after insertion.
    /// * `maybe_split`       — `Some((bbox, new_node))` if the node was split.
    /// * `pending_reinserts` — entries ejected by forced reinsertion that must
    ///   be reinserted from the root by the caller.
    ///
    /// `level` is 0 at the root and increases going down towards leaves.
    fn insert_into(
        &self,
        node: Node<T>,
        entry: LeafEntry<T>,
        level: usize,
        reinsert_levels: &mut HashSet<usize>,
    ) -> InsertResult<T> {
        match node {
            Node::Leaf(mut leaf) => {
                leaf.entries.push(entry);
                if leaf.entries.len() > self.max_entries {
                    // Forced reinsertion: first time we overflow at this level.
                    if !reinsert_levels.contains(&level) {
                        reinsert_levels.insert(level);
                        let pending = self.forced_reinsert_leaf(&mut leaf);
                        // After ejection the node may no longer overflow.
                        (Node::Leaf(leaf), None, pending)
                    } else {
                        // Already reinserted at this level — do the real split.
                        let (left, right) = split_leaf(leaf, self.min_entries);
                        let right_bbox = leaf_bbox(&right);
                        (
                            Node::Leaf(left),
                            Some((right_bbox, Node::Leaf(right))),
                            Vec::new(),
                        )
                    }
                } else {
                    (Node::Leaf(leaf), None, Vec::new())
                }
            }
            Node::Internal(mut internal) => {
                let best_idx = choose_subtree(&internal.entries, &entry.bbox);
                let chosen = internal.entries.remove(best_idx);
                let (updated_child, maybe_split, mut pending) =
                    self.insert_into(*chosen.child, entry, level + 1, reinsert_levels);
                let updated_bbox = node_bbox(&updated_child);
                internal.entries.push(InternalEntry {
                    bbox: updated_bbox,
                    child: Box::new(updated_child),
                });
                if let Some((split_bbox, split_child)) = maybe_split {
                    internal.entries.push(InternalEntry {
                        bbox: split_bbox,
                        child: Box::new(split_child),
                    });
                }
                if internal.entries.len() > self.max_entries {
                    // Forced reinsertion for internal nodes.
                    if !reinsert_levels.contains(&level) {
                        reinsert_levels.insert(level);
                        let internal_pending = self.forced_reinsert_internal(&mut internal);
                        pending.extend(internal_pending);
                        (Node::Internal(internal), None, pending)
                    } else {
                        let (left, right) = split_internal(internal, self.min_entries);
                        let right_bbox = internal_bbox(&right);
                        (
                            Node::Internal(left),
                            Some((right_bbox, Node::Internal(right))),
                            pending,
                        )
                    }
                } else {
                    (Node::Internal(internal), None, pending)
                }
            }
        }
    }

    /// Eject the `p` farthest leaf entries from `leaf` and return them as
    /// `(Bbox2D, T)` pairs to be reinserted from the root.
    ///
    /// "Farthest" is measured from the centroid of the leaf's current MBR.
    fn forced_reinsert_leaf(&self, leaf: &mut LeafNode<T>) -> Vec<(Bbox2D, T)> {
        let node_mbr = leaf_bbox(leaf);
        let (cx, cy) = node_mbr.center();

        // Sort entries by their centroid distance from the node centroid,
        // descending (farthest first).
        leaf.entries.sort_by(|a, b| {
            let (ax, ay) = a.bbox.center();
            let (bx, by) = b.bbox.center();
            let da = (ax - cx).powi(2) + (ay - cy).powi(2);
            let db = (bx - cx).powi(2) + (by - cy).powi(2);
            db.partial_cmp(&da).unwrap_or(core::cmp::Ordering::Equal)
        });

        // Drain the p farthest entries.
        let p = self.reinsert_p.min(leaf.entries.len().saturating_sub(1));
        let p = p.max(1).min(leaf.entries.len());
        leaf.entries.drain(..p).map(|e| (e.bbox, e.value)).collect()
    }

    /// Eject the `p` farthest internal entries from `internal` (collecting
    /// all their leaf entries) and return them as `(Bbox2D, T)` pairs to be
    /// reinserted from the root.
    fn forced_reinsert_internal(&self, internal: &mut InternalNode<T>) -> Vec<(Bbox2D, T)> {
        let node_mbr = internal_bbox(internal);
        let (cx, cy) = node_mbr.center();

        // Sort entries by their centroid distance from the node centroid,
        // descending (farthest first).
        internal.entries.sort_by(|a, b| {
            let (ax, ay) = a.bbox.center();
            let (bx, by) = b.bbox.center();
            let da = (ax - cx).powi(2) + (ay - cy).powi(2);
            let db = (bx - cx).powi(2) + (by - cy).powi(2);
            db.partial_cmp(&da).unwrap_or(core::cmp::Ordering::Equal)
        });

        let p = self
            .reinsert_p
            .min(internal.entries.len().saturating_sub(1));
        let p = p.max(1).min(internal.entries.len());
        let ejected: Vec<InternalEntry<T>> = internal.entries.drain(..p).collect();

        // Collect all leaf entries from the ejected subtrees.
        let mut pending: Vec<(Bbox2D, T)> = Vec::new();
        for e in ejected {
            collect_leaf_entries_owned(*e.child, &mut pending);
        }
        pending
    }

    /// Internal insert that does NOT increment `self.size`.  Used by
    /// condense-tree reinsertion so the count stays correct, and by
    /// forced-reinsertion of previously-counted entries.
    fn insert_no_count(&mut self, bbox: Bbox2D, value: T) {
        let entry = LeafEntry { bbox, value };
        let mut reinsert_levels: HashSet<usize> = HashSet::new();
        self.insert_entry_no_count(entry, &mut reinsert_levels);
    }

    /// Like `insert_entry` but does not touch `self.size` and accepts an
    /// existing `reinsert_levels` set (forwarded from the parent reinsertion
    /// context to prevent infinite loops).
    fn insert_entry_no_count(&mut self, entry: LeafEntry<T>, reinsert_levels: &mut HashSet<usize>) {
        match self.root.take() {
            None => {
                self.root = Some(Node::Leaf(LeafNode {
                    entries: vec![entry],
                }));
            }
            Some(root) => {
                let (updated_root, maybe_split, pending_reinserts) =
                    self.insert_into(root, entry, 0, reinsert_levels);

                let new_root = if let Some((split_bbox, split_node)) = maybe_split {
                    let old_bbox = node_bbox(&updated_root);
                    Node::Internal(InternalNode {
                        entries: vec![
                            InternalEntry {
                                bbox: old_bbox,
                                child: Box::new(updated_root),
                            },
                            InternalEntry {
                                bbox: split_bbox,
                                child: Box::new(split_node),
                            },
                        ],
                    })
                } else {
                    updated_root
                };
                self.root = Some(new_root);

                for (rb, rv) in pending_reinserts {
                    let re_entry = LeafEntry {
                        bbox: rb,
                        value: rv,
                    };
                    self.insert_entry_no_count(re_entry, reinsert_levels);
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // Search
    // ------------------------------------------------------------------

    /// Find all entries whose bbox intersects `query`.
    pub fn search(&self, query: &Bbox2D) -> Vec<&T> {
        let mut results = Vec::new();
        if let Some(ref root) = self.root {
            search_node(root, query, &mut results);
        }
        results
    }

    /// Find all entries whose bbox contains the point `(x, y)`.
    pub fn contains_point(&self, x: f64, y: f64) -> Vec<&T> {
        let pt = Bbox2D::point(x, y);
        self.search(&pt)
    }

    /// Return all entries whose bounding box lies within `buffer` distance of
    /// any segment of `line`.
    ///
    /// `line` is a sequence of `(x, y)` vertices defining a polyline. Each
    /// consecutive pair of vertices defines one segment. Entries whose bbox
    /// comes within `buffer` of **any** segment are included in the result.
    ///
    /// An entry appears at most once even if it intersects multiple segments
    /// (deduplication by pointer identity).
    ///
    /// Returns an empty `Vec` when `line` has fewer than 2 vertices.
    ///
    /// # Algorithm
    ///
    /// 1. Build a corridor bbox that encloses the entire buffered polyline for
    ///    a root-level broad-phase prune (identical cost to a normal `search`
    ///    at the root).
    /// 2. Recurse into the R-tree:
    ///    - **Internal nodes**: visit if the node MBR intersects the corridor
    ///      bbox (cheap conservative filter).
    ///    - **Leaf entries**: include if the segment–bbox distance is ≤ buffer
    ///      (exact Liang-Barsky test on the bbox expanded by `buffer`).
    /// 3. Deduplicate results by pointer address.
    pub fn search_line(&self, line: &[(f64, f64)], buffer: f64) -> Vec<&T> {
        if line.len() < 2 {
            return vec![];
        }

        let corridor = line_corridor_bbox(line, buffer);
        let mut results: Vec<&T> = Vec::new();

        if let Some(ref root) = self.root {
            search_line_node(root, line, buffer, &corridor, &mut results);
        }

        // Deduplicate: a single entry's bbox may intersect multiple segments.
        // Sort by raw pointer then dedup so each entry appears at most once.
        results.sort_unstable_by_key(|r| *r as *const T as usize);
        results.dedup_by_key(|r| *r as *const T as usize);
        results
    }

    // ------------------------------------------------------------------
    // Window query with result-count limit (top-k within bbox)
    // ------------------------------------------------------------------

    /// Find all entries whose bbox intersects `query`, returning both the
    /// stored bounding box and the value reference for each hit.
    ///
    /// This is the building block for [`search_top_k`](Self::search_top_k):
    /// callers that need the per-entry bounding boxes alongside the values
    /// can use this directly.
    pub fn search_with_bbox<'a>(&'a self, query: &Bbox2D) -> Vec<(&'a Bbox2D, &'a T)> {
        let mut results = Vec::new();
        if let Some(ref root) = self.root {
            search_with_bbox_node(root, query, &mut results);
        }
        results
    }

    /// Return the top-`k` items whose bounding boxes overlap `window`,
    /// sorted by ascending minimum Euclidean distance from the window's
    /// centre point to each item's bounding box (the MINDIST metric).
    ///
    /// Items are ranked by how close their stored bbox is to the geometric
    /// centre of `window`.  An item whose bbox contains the centre has
    /// distance 0 and always ranks first.
    ///
    /// Returns fewer than `k` results if fewer items intersect `window`.
    /// Returns an empty [`Vec`] when `k == 0` or the tree is empty.
    ///
    /// # Algorithm
    ///
    /// 1. Execute `search_with_bbox(window)` to collect all candidates that
    ///    intersect the query window together with their stored bounding boxes.
    /// 2. For each candidate compute `bbox.min_distance_to_point(cx, cy)` where
    ///    `(cx, cy)` is the centre of `window`.
    /// 3. Sort the candidate list ascending by that distance using a
    ///    NaN-safe total ordering.
    /// 4. Truncate to at most `k` results.
    ///
    /// Time complexity: O(m log m) where m is the number of candidates in the
    /// window — the R-tree structure prunes branches whose MBR does not overlap
    /// the window, so m ≪ n in typical workloads.
    pub fn search_top_k(&self, window: &Bbox2D, k: usize) -> Vec<(&T, f64)> {
        if k == 0 {
            return Vec::new();
        }
        let (cx, cy) = window.center();
        let mut candidates: Vec<(&Bbox2D, &T)> = self.search_with_bbox(window);
        // Sort ascending by distance from the window centre to each item's bbox.
        candidates.sort_by(|(bbox_a, _), (bbox_b, _)| {
            let da = bbox_a.min_distance_to_point(cx, cy);
            let db = bbox_b.min_distance_to_point(cx, cy);
            da.total_cmp(&db)
        });
        candidates.truncate(k);
        candidates
            .into_iter()
            .map(|(bbox, v)| {
                let d = bbox.min_distance_to_point(cx, cy);
                (v, d)
            })
            .collect()
    }

    // ------------------------------------------------------------------
    // Nearest neighbour (priority-queue k-NN)
    // ------------------------------------------------------------------

    /// Return up to `k` entries nearest to point `(x, y)`, ordered by
    /// ascending bbox-to-point distance.
    ///
    /// "Distance" is the minimum Euclidean distance from the point to the
    /// entry's bounding box (0 when inside).
    ///
    /// Uses a priority-queue traversal with MINDIST pruning for efficiency.
    pub fn nearest(&self, x: f64, y: f64, k: usize) -> Vec<(&T, f64)> {
        if k == 0 {
            return Vec::new();
        }
        match self.root {
            Some(ref root) => knn::knn_search(root, x, y, k),
            None => Vec::new(),
        }
    }

    // ------------------------------------------------------------------
    // Iteration
    // ------------------------------------------------------------------

    /// Iterate over all `(bbox, value)` pairs in unspecified order.
    pub fn iter(&self) -> impl Iterator<Item = (&Bbox2D, &T)> {
        let mut pairs: Vec<(&Bbox2D, &T)> = Vec::new();
        if let Some(ref root) = self.root {
            collect_all_pairs(root, &mut pairs);
        }
        pairs.into_iter()
    }

    /// The smallest bbox containing all inserted entries, or `None` if empty.
    pub fn total_bbox(&self) -> Option<Bbox2D> {
        self.root.as_ref().map(node_bbox)
    }

    /// Height of the tree (number of levels).
    ///
    /// Returns `0` for an empty tree, `1` for a tree with a single leaf root,
    /// `2` when an internal node sits above leaves, and so on.
    pub fn height(&self) -> usize {
        fn depth<T>(node: &Node<T>) -> usize {
            match node {
                Node::Leaf(_) => 1,
                Node::Internal(internal) => {
                    1 + internal
                        .entries
                        .iter()
                        .map(|e| depth(&e.child))
                        .max()
                        .unwrap_or(0)
                }
            }
        }
        self.root.as_ref().map(depth).unwrap_or(0)
    }

    // ------------------------------------------------------------------
    // Structural validation
    // ------------------------------------------------------------------

    /// Validate the R*-tree minimum-fill invariant.
    ///
    /// Every non-root node must have at least `min_entries` entries.
    /// Returns a list of violation descriptions; an empty list means the
    /// tree is structurally valid.
    pub fn check_min_fill_invariant(&self) -> Vec<String> {
        let mut violations = Vec::new();
        if let Some(ref root) = self.root {
            node::validate_min_fill(root, self.min_entries, true, &mut violations);
        }
        violations
    }

    // ------------------------------------------------------------------
    // Bulk loading (STR)
    // ------------------------------------------------------------------

    /// Build an R-tree from a pre-collected vector of items using the
    /// Sort-Tile-Recursive (STR) algorithm.
    ///
    /// This is significantly faster and produces a better-packed tree than
    /// repeated calls to [`insert`](Self::insert) when all data is available
    /// upfront.  O(n log n) time.
    pub fn bulk_load(items: Vec<(Bbox2D, T)>) -> Self {
        Self::bulk_load_with_max_entries(items, 9)
    }

    /// Like [`bulk_load`](Self::bulk_load) but with a custom maximum node
    /// capacity.
    pub fn bulk_load_with_max_entries(items: Vec<(Bbox2D, T)>, max_entries: usize) -> Self {
        let max_entries = max_entries.max(2);
        let min_entries = ((max_entries as f64) * 0.4).ceil() as usize;
        let reinsert_p = compute_reinsert_p(max_entries);
        match bulk::str_bulk_load(items, max_entries) {
            Some((root, count)) => Self {
                root: Some(root),
                size: count,
                max_entries,
                min_entries,
                reinsert_p,
            },
            None => Self {
                root: None,
                size: 0,
                max_entries,
                min_entries,
                reinsert_p,
            },
        }
    }

    /// If the root is an internal node with exactly one child, replace it
    /// with that child.
    fn collapse_root_if_single_child(&mut self) {
        let should_collapse = matches!(
            &self.root,
            Some(Node::Internal(internal)) if internal.entries.len() == 1
        );
        if should_collapse
            && let Some(Node::Internal(mut internal)) = self.root.take()
            && let Some(entry) = internal.entries.pop()
        {
            self.root = Some(*entry.child);
        }
    }
}

// ---------------------------------------------------------------------------
// Deletion  (requires T: PartialEq)
// ---------------------------------------------------------------------------

impl<T: Clone + PartialEq> RTree<T> {
    /// Remove the first entry matching `(bbox, value)`.
    ///
    /// Returns the removed value on success, or `Err(EntryNotFound)` if no
    /// matching entry exists.
    ///
    /// After removal the tree is condensed: under-full nodes are dissolved and
    /// their entries reinserted.
    pub fn remove(&mut self, bbox: &Bbox2D, value: &T) -> Result<T, IndexError> {
        let root = match self.root.take() {
            Some(r) => r,
            None => return Err(IndexError::EntryNotFound),
        };

        let mut orphans: Vec<(Bbox2D, T)> = Vec::new();
        let result = self.remove_from(root, bbox, value, &mut orphans);

        match result {
            RemoveResult::NotFound(node) => {
                self.root = Some(node);
                Err(IndexError::EntryNotFound)
            }
            RemoveResult::Found { remaining, removed } => {
                self.root = remaining;
                self.size -= 1;

                // If the root is an internal node with exactly one child,
                // collapse it down.
                self.collapse_root_if_single_child();

                // Reinsert orphans from condensed nodes.
                for (ob, ov) in orphans {
                    self.insert_no_count(ob, ov);
                }

                Ok(removed)
            }
        }
    }

    /// Recursive removal from a node.
    fn remove_from(
        &self,
        node: Node<T>,
        bbox: &Bbox2D,
        value: &T,
        orphans: &mut Vec<(Bbox2D, T)>,
    ) -> RemoveResult<T> {
        match node {
            Node::Leaf(mut leaf) => {
                // Search for the matching entry.
                let pos = leaf
                    .entries
                    .iter()
                    .position(|e| e.bbox == *bbox && e.value == *value);
                match pos {
                    Some(idx) => {
                        let removed = leaf.entries.remove(idx);
                        if leaf.entries.is_empty() {
                            RemoveResult::Found {
                                remaining: None,
                                removed: removed.value,
                            }
                        } else {
                            RemoveResult::Found {
                                remaining: Some(Node::Leaf(leaf)),
                                removed: removed.value,
                            }
                        }
                    }
                    None => RemoveResult::NotFound(Node::Leaf(leaf)),
                }
            }
            Node::Internal(mut internal) => {
                // Try each child whose bbox overlaps the target bbox.
                for i in 0..internal.entries.len() {
                    if !internal.entries[i].bbox.intersects(bbox) {
                        continue;
                    }

                    let entry = internal.entries.remove(i);
                    let result = self.remove_from(*entry.child, bbox, value, orphans);

                    match result {
                        RemoveResult::NotFound(child) => {
                            // Put it back.
                            internal.entries.insert(
                                i,
                                InternalEntry {
                                    bbox: entry.bbox,
                                    child: Box::new(child),
                                },
                            );
                        }
                        RemoveResult::Found { remaining, removed } => {
                            if let Some(child_node) = remaining {
                                // Check if the child is under-full and needs
                                // condensing.
                                let child_len = match &child_node {
                                    Node::Leaf(l) => l.entries.len(),
                                    Node::Internal(ii) => ii.entries.len(),
                                };
                                if child_len < self.min_entries {
                                    // Dissolve: collect all leaf entries from
                                    // this child and add them to orphans.
                                    collect_leaf_entries(child_node, orphans);
                                } else {
                                    let new_bbox = node_bbox(&child_node);
                                    internal.entries.insert(
                                        i,
                                        InternalEntry {
                                            bbox: new_bbox,
                                            child: Box::new(child_node),
                                        },
                                    );
                                }
                            }
                            // else: child is empty, just drop it.

                            if internal.entries.is_empty() {
                                return RemoveResult::Found {
                                    remaining: None,
                                    removed,
                                };
                            }

                            return RemoveResult::Found {
                                remaining: Some(Node::Internal(internal)),
                                removed,
                            };
                        }
                    }
                }

                // Not found in any child.
                RemoveResult::NotFound(Node::Internal(internal))
            }
        }
    }
}

/// Result of a recursive remove operation.
enum RemoveResult<T> {
    /// The entry was not found; the node is returned unchanged.
    NotFound(Node<T>),
    /// The entry was found and removed.
    Found {
        /// The node after removal (`None` if it became empty).
        remaining: Option<Node<T>>,
        /// The removed value.
        removed: T,
    },
}

/// Collect all leaf entries from a node (used during condense-tree when
/// dissolving an under-full node).  Takes ownership of the node.
fn collect_leaf_entries<T: Clone>(node: Node<T>, out: &mut Vec<(Bbox2D, T)>) {
    collect_leaf_entries_owned(node, out);
}

/// Collect all leaf entries from an owned node into `out`.
pub(crate) fn collect_leaf_entries_owned<T: Clone>(node: Node<T>, out: &mut Vec<(Bbox2D, T)>) {
    match node {
        Node::Leaf(leaf) => {
            for e in leaf.entries {
                out.push((e.bbox, e.value));
            }
        }
        Node::Internal(internal) => {
            for e in internal.entries {
                collect_leaf_entries_owned(*e.child, out);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Line-segment corridor helpers
// ---------------------------------------------------------------------------

/// Build the axis-aligned corridor bbox for the entire buffered polyline.
///
/// This is the union of all per-segment buffered bboxes; used as a cheap
/// broad-phase filter at the root of the R-tree traversal.
fn line_corridor_bbox(line: &[(f64, f64)], buffer: f64) -> Bbox2D {
    let min_x = line.iter().fold(f64::INFINITY, |acc, p| acc.min(p.0)) - buffer;
    let min_y = line.iter().fold(f64::INFINITY, |acc, p| acc.min(p.1)) - buffer;
    let max_x = line.iter().fold(f64::NEG_INFINITY, |acc, p| acc.max(p.0)) + buffer;
    let max_y = line.iter().fold(f64::NEG_INFINITY, |acc, p| acc.max(p.1)) + buffer;
    Bbox2D {
        min_x,
        min_y,
        max_x,
        max_y,
    }
}

/// Return `true` if the line segment from `p` to `q`, padded by `buffer`,
/// intersects `bbox`.
///
/// The test is performed by expanding the bbox on all four sides by `buffer`
/// and then running the Liang-Barsky parametric clip algorithm to determine
/// whether the segment intersects the expanded rectangle.
///
/// Edge cases handled correctly:
/// * Segment endpoint inside the expanded bbox → true.
/// * Purely horizontal segment (dy = 0) — uses the `|denom| < ε` branch.
/// * Purely vertical segment (dx = 0) — likewise.
/// * Zero-length segment (degenerate point) — degrades to a point-in-bbox check.
fn segment_intersects_bbox_buffered(
    p: (f64, f64),
    q: (f64, f64),
    buffer: f64,
    bbox: &Bbox2D,
) -> bool {
    // Expand bbox by buffer on all sides.
    let exp = Bbox2D {
        min_x: bbox.min_x - buffer,
        min_y: bbox.min_y - buffer,
        max_x: bbox.max_x + buffer,
        max_y: bbox.max_y + buffer,
    };

    // Fast path: either endpoint is inside the expanded bbox.
    if exp.contains_point(p.0, p.1) || exp.contains_point(q.0, q.1) {
        return true;
    }

    // Liang-Barsky parametric clip in t ∈ [0, 1].
    // For each clip boundary we compute t where the ray crosses the edge.
    // denom < 0  →  entering half-plane: update t_min.
    // denom > 0  →  exiting  half-plane: update t_max.
    let dx = q.0 - p.0;
    let dy = q.1 - p.1;

    let mut t_min = 0.0_f64;
    let mut t_max = 1.0_f64;

    // Liang-Barsky: four clip planes (left, right, bottom, top).
    // For each plane: denom is the component of the direction vector
    // pointing away from the outside; num is the signed distance from
    // the start point to the plane.
    //
    // Plane equation: denom * t >= -num  (i.e. t >= -num / denom when denom > 0)
    //
    // Using the standard form: p + t*d, clip against each half-plane:
    //   left:   -dx * t <= p.x - exp.min_x   → denom = -dx, num = p.x - exp.min_x
    //   right:   dx * t <= exp.max_x - p.x   → denom =  dx, num = exp.max_x - p.x
    //   bottom: -dy * t <= p.y - exp.min_y   → denom = -dy, num = p.y - exp.min_y
    //   top:     dy * t <= exp.max_y - p.y   → denom =  dy, num = exp.max_y - p.y
    let clips = [
        (-dx, p.0 - exp.min_x),
        (dx, exp.max_x - p.0),
        (-dy, p.1 - exp.min_y),
        (dy, exp.max_y - p.1),
    ];

    for (denom, num) in clips {
        if denom.abs() < f64::EPSILON {
            // Segment is parallel to this clip boundary.
            if num < 0.0 {
                // Outside and parallel → no intersection possible.
                return false;
            }
            // Inside or on boundary → this boundary doesn't clip anything.
        } else {
            let t = num / denom;
            if denom < 0.0 {
                // Entering half-plane.
                t_min = t_min.max(t);
            } else {
                // Exiting half-plane.
                t_max = t_max.min(t);
            }
            if t_min > t_max {
                return false;
            }
        }
    }

    t_min <= t_max
}

/// Recursive R-tree traversal for `search_line`.
///
/// * **Internal nodes**: descend only when the node MBR overlaps the corridor
///   bbox (cheap broad-phase).
/// * **Leaf entries**: include if any segment of `line` comes within `buffer`
///   of the entry bbox (exact test via `segment_intersects_bbox_buffered`).
fn search_line_node<'a, T>(
    node: &'a Node<T>,
    line: &[(f64, f64)],
    buffer: f64,
    corridor: &Bbox2D,
    results: &mut Vec<&'a T>,
) {
    match node {
        Node::Leaf(leaf) => {
            'entry: for entry in &leaf.entries {
                // Broad-phase: entry bbox must intersect the overall corridor.
                if !entry.bbox.intersects(corridor) {
                    continue;
                }
                // Exact-phase: test each segment individually.
                for i in 0..line.len() - 1 {
                    if segment_intersects_bbox_buffered(line[i], line[i + 1], buffer, &entry.bbox) {
                        results.push(&entry.value);
                        continue 'entry;
                    }
                }
            }
        }
        Node::Internal(internal) => {
            for entry in &internal.entries {
                // Only descend into children whose MBR overlaps the corridor.
                if entry.bbox.intersects(corridor) {
                    search_line_node(&entry.child, line, buffer, corridor, results);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helper: compute reinsertion count p = ceil(0.3 × M), min 1.
// ---------------------------------------------------------------------------

#[inline]
fn compute_reinsert_p(max_entries: usize) -> usize {
    ((max_entries as f64 * 0.3).ceil() as usize).max(1)
}

// ---------------------------------------------------------------------------
// Serialization (requires T: AsRef<[u8]> / From<Vec<u8>>)
// ---------------------------------------------------------------------------

impl<T: Clone + AsRef<[u8]>> RTree<T> {
    /// Serialize the R-tree to a binary blob.
    ///
    /// See the `serial` module for the wire format documentation.
    pub fn to_bytes(&self) -> Vec<u8> {
        serial::serialize(&self.root, self.size, self.max_entries)
    }
}

impl<T: Clone + From<Vec<u8>>> RTree<T> {
    /// Deserialize an R-tree from a binary blob produced by
    /// [`to_bytes`](Self::to_bytes).
    pub fn from_bytes(data: &[u8]) -> Result<Self, IndexError> {
        let (root, count, max_entries) = serial::deserialize(data)?;
        let min_entries = ((max_entries as f64) * 0.4).ceil() as usize;
        let reinsert_p = compute_reinsert_p(max_entries);
        Ok(Self {
            root,
            size: count,
            max_entries,
            min_entries,
            reinsert_p,
        })
    }
}

// ---------------------------------------------------------------------------
// SpatialQuery
// ---------------------------------------------------------------------------

/// Collection of stateless spatial query helpers operating on [`RTree`].
pub struct SpatialQuery;

impl SpatialQuery {
    /// Return clones of all values whose bbox is entirely contained within
    /// `bbox`.
    pub fn within<T: Clone>(rtree: &RTree<T>, bbox: &Bbox2D) -> Vec<T> {
        let mut results = Vec::new();
        if let Some(ref root) = rtree.root {
            within_node(root, bbox, &mut results);
        }
        results
    }

    /// Return clones of all values whose bbox intersects `bbox`.
    pub fn intersects<T: Clone>(rtree: &RTree<T>, bbox: &Bbox2D) -> Vec<T> {
        rtree.search(bbox).into_iter().cloned().collect()
    }

    /// Count entries whose bbox intersects `bbox` without allocating a
    /// result vector.
    pub fn count_in<T>(rtree: &RTree<T>, bbox: &Bbox2D) -> usize {
        let mut count = 0usize;
        if let Some(ref root) = rtree.root {
            count_in_node(root, bbox, &mut count);
        }
        count
    }

    /// Spatial join: for every entry in `left` find all entries in `right`
    /// whose bbox intersects it, returning pairs `(&A, &B)`.
    pub fn spatial_join<'a, A: Clone, B: Clone>(
        left: &'a RTree<A>,
        right: &'a RTree<B>,
    ) -> Vec<(&'a A, &'a B)> {
        let mut results = Vec::new();
        for (bbox_a, val_a) in left.iter() {
            for val_b in right.search(bbox_a) {
                results.push((val_a, val_b));
            }
        }
        results
    }

    /// Window query with result-count limit: return the top-`k` items from
    /// `rtree` whose bounding boxes overlap `window`, sorted by ascending
    /// minimum distance from the window's centre to each item's bbox.
    ///
    /// This is a stateless convenience wrapper around
    /// [`RTree::search_top_k`].
    pub fn top_k_in_window<'a, T: Clone>(
        rtree: &'a RTree<T>,
        window: &Bbox2D,
        k: usize,
    ) -> Vec<(&'a T, f64)> {
        rtree.search_top_k(window, k)
    }

    /// Geographic linear k-NN: find up to `k` nearest points from a
    /// `(GeoPoint, T)` slice to a query location, using haversine distance.
    ///
    /// This is a stateless convenience wrapper around
    /// [`crate::geo_distance::geo_nearest_k`].
    pub fn geo_nearest_k<T: Clone>(
        points: &[(crate::geo_distance::GeoPoint, T)],
        query_lat_deg: f64,
        query_lon_deg: f64,
        k: usize,
    ) -> Vec<crate::geo_distance::GeoNearestResult<T>> {
        crate::geo_distance::geo_nearest_k(points, query_lat_deg, query_lon_deg, k)
    }

    /// Geographic radius filter: return all points in `points` within
    /// `radius_m` metres of the query location (haversine), sorted by
    /// ascending distance.
    ///
    /// This is a stateless convenience wrapper around
    /// [`crate::geo_distance::geo_within_radius`].
    pub fn geo_within_radius<T: Clone>(
        points: &[(crate::geo_distance::GeoPoint, T)],
        query_lat_deg: f64,
        query_lon_deg: f64,
        radius_m: f64,
    ) -> Vec<crate::geo_distance::GeoNearestResult<T>> {
        crate::geo_distance::geo_within_radius(points, query_lat_deg, query_lon_deg, radius_m)
    }
}
