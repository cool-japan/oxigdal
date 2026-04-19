//! Pure-Rust R*-tree (R-tree variant) spatial index.
//!
//! This implementation uses a **linear split** strategy which is simple and
//! fast.  The maximum node capacity `M` defaults to 9; the minimum fill
//! `m = ceil(M * 0.4)`.

#[cfg(not(feature = "std"))]
use alloc::{boxed::Box, vec::Vec};

use crate::bbox::Bbox2D;
use crate::error::IndexError;

mod bulk;
mod knn;
pub(crate) mod node;
mod serial;

use node::{
    InternalEntry, InternalNode, LeafEntry, LeafNode, Node, choose_subtree, collect_all_pairs,
    count_in_node, internal_bbox, leaf_bbox, node_bbox, search_node, split_internal, split_leaf,
    within_node,
};

// ---------------------------------------------------------------------------
// RTree
// ---------------------------------------------------------------------------

/// An R-tree spatial index mapping [`Bbox2D`] regions to values of type `T`.
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
        Self {
            root: None,
            size: 0,
            max_entries,
            min_entries,
        }
    }

    /// Create a new, empty R-tree with a custom maximum node capacity `M`.
    ///
    /// `max_entries` is clamped to a minimum of `2`.
    pub fn with_max_entries(max_entries: usize) -> Self {
        let max_entries = max_entries.max(2);
        let min_entries = ((max_entries as f64) * 0.4).ceil() as usize;
        Self {
            root: None,
            size: 0,
            max_entries,
            min_entries,
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
        match self.root.take() {
            None => {
                self.root = Some(Node::Leaf(LeafNode {
                    entries: vec![entry],
                }));
            }
            Some(root) => {
                let (updated_root, maybe_split) = self.insert_into(root, entry);
                if let Some((split_bbox, split_node)) = maybe_split {
                    let old_bbox = node_bbox(&updated_root);
                    let new_root = Node::Internal(InternalNode {
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
                    });
                    self.root = Some(new_root);
                } else {
                    self.root = Some(updated_root);
                }
            }
        }
        self.size += 1;
    }

    /// Recursive insertion helper.
    fn insert_into(
        &self,
        node: Node<T>,
        entry: LeafEntry<T>,
    ) -> (Node<T>, Option<(Bbox2D, Node<T>)>) {
        match node {
            Node::Leaf(mut leaf) => {
                leaf.entries.push(entry);
                if leaf.entries.len() > self.max_entries {
                    let (left, right) = split_leaf(leaf, self.min_entries);
                    let right_bbox = leaf_bbox(&right);
                    (Node::Leaf(left), Some((right_bbox, Node::Leaf(right))))
                } else {
                    (Node::Leaf(leaf), None)
                }
            }
            Node::Internal(mut internal) => {
                let best_idx = choose_subtree(&internal.entries, &entry.bbox);
                let chosen = internal.entries.remove(best_idx);
                let (updated_child, maybe_split) = self.insert_into(*chosen.child, entry);
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
                    let (left, right) = split_internal(internal, self.min_entries);
                    let right_bbox = internal_bbox(&right);
                    (
                        Node::Internal(left),
                        Some((right_bbox, Node::Internal(right))),
                    )
                } else {
                    (Node::Internal(internal), None)
                }
            }
        }
    }

    /// Internal insert that does NOT increment `self.size`.  Used by
    /// condense-tree reinsertion so the count stays correct.
    fn insert_no_count(&mut self, bbox: Bbox2D, value: T) {
        let entry = LeafEntry { bbox, value };
        match self.root.take() {
            None => {
                self.root = Some(Node::Leaf(LeafNode {
                    entries: vec![entry],
                }));
            }
            Some(root) => {
                let (updated_root, maybe_split) = self.insert_into(root, entry);
                if let Some((split_bbox, split_node)) = maybe_split {
                    let old_bbox = node_bbox(&updated_root);
                    let new_root = Node::Internal(InternalNode {
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
                    });
                    self.root = Some(new_root);
                } else {
                    self.root = Some(updated_root);
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
        match bulk::str_bulk_load(items, max_entries) {
            Some((root, count)) => Self {
                root: Some(root),
                size: count,
                max_entries,
                min_entries,
            },
            None => Self {
                root: None,
                size: 0,
                max_entries,
                min_entries,
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
/// dissolving an under-full node).
fn collect_leaf_entries<T: Clone>(node: Node<T>, out: &mut Vec<(Bbox2D, T)>) {
    match node {
        Node::Leaf(leaf) => {
            for e in leaf.entries {
                out.push((e.bbox, e.value));
            }
        }
        Node::Internal(internal) => {
            for e in internal.entries {
                collect_leaf_entries(*e.child, out);
            }
        }
    }
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
        Ok(Self {
            root,
            size: count,
            max_entries,
            min_entries,
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
}
