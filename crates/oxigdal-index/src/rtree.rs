//! Pure-Rust R*-tree (R-tree variant) spatial index.
//!
//! This implementation uses a **linear split** strategy which is simple and
//! fast.  The maximum node capacity `M` defaults to 9; the minimum fill
//! `m = ceil(M * 0.4)`.

use crate::bbox::Bbox2D;

// ---------------------------------------------------------------------------
// Node types
// ---------------------------------------------------------------------------

struct LeafEntry<T> {
    bbox: Bbox2D,
    value: T,
}

struct InternalEntry<T> {
    bbox: Bbox2D,
    child: Box<Node<T>>,
}

enum Node<T> {
    Leaf(LeafNode<T>),
    Internal(InternalNode<T>),
}

struct LeafNode<T> {
    entries: Vec<LeafEntry<T>>,
}

struct InternalNode<T> {
    entries: Vec<InternalEntry<T>>,
}

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
                // First entry — create a leaf root.
                self.root = Some(Node::Leaf(LeafNode {
                    entries: vec![entry],
                }));
            }
            Some(root) => {
                // Insert into the tree; may return a split sibling.
                let (updated_root, maybe_split) = self.insert_into(root, entry);
                if let Some((split_bbox, split_node)) = maybe_split {
                    // Root was split — create a new internal root.
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
    ///
    /// Returns `(updated_node, Option<(split_bbox, split_node)>)`.
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
                // Choose the child whose bbox needs least enlargement.
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
    // Nearest neighbour
    // ------------------------------------------------------------------

    /// Return up to `k` entries nearest to point `(x, y)`, ordered by
    /// ascending bbox-to-point distance.
    ///
    /// "Distance" is the minimum Euclidean distance from the point to the
    /// entry's bounding box (0 when inside).
    pub fn nearest(&self, x: f64, y: f64, k: usize) -> Vec<(&T, f64)> {
        if k == 0 {
            return Vec::new();
        }
        // Collect all entries with their distances, then sort.
        let mut candidates: Vec<(&T, f64)> = Vec::new();
        if let Some(ref root) = self.root {
            collect_all_with_dist(root, x, y, &mut candidates);
        }
        candidates.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(core::cmp::Ordering::Equal));
        candidates.truncate(k);
        candidates
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
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Compute the bounding box of a node.
fn node_bbox<T>(node: &Node<T>) -> Bbox2D {
    match node {
        Node::Leaf(l) => leaf_bbox(l),
        Node::Internal(i) => internal_bbox(i),
    }
}

fn leaf_bbox<T>(leaf: &LeafNode<T>) -> Bbox2D {
    leaf.entries
        .iter()
        .map(|e| e.bbox)
        .reduce(|a, b| a.union(&b))
        .unwrap_or(Bbox2D::point(0.0, 0.0))
}

fn internal_bbox<T>(internal: &InternalNode<T>) -> Bbox2D {
    internal
        .entries
        .iter()
        .map(|e| e.bbox)
        .reduce(|a, b| a.union(&b))
        .unwrap_or(Bbox2D::point(0.0, 0.0))
}

/// Choose the index of the child whose enlargement is minimised.
fn choose_subtree<T>(entries: &[InternalEntry<T>], bbox: &Bbox2D) -> usize {
    let mut best_idx = 0;
    let mut best_enlargement = f64::INFINITY;
    let mut best_area = f64::INFINITY;
    for (i, e) in entries.iter().enumerate() {
        let enlargement = e.bbox.enlargement_to_include(bbox);
        if enlargement < best_enlargement
            || (enlargement == best_enlargement && e.bbox.area() < best_area)
        {
            best_idx = i;
            best_enlargement = enlargement;
            best_area = e.bbox.area();
        }
    }
    best_idx
}

// ---------------------------------------------------------------------------
// Linear split
// ---------------------------------------------------------------------------

/// Split a leaf node using the linear split algorithm.
fn split_leaf<T: Clone>(mut leaf: LeafNode<T>, min_entries: usize) -> (LeafNode<T>, LeafNode<T>) {
    let entries = &mut leaf.entries;
    // Sort by min_x as a simple seed selection.
    entries.sort_by(|a, b| {
        a.bbox
            .min_x
            .partial_cmp(&b.bbox.min_x)
            .unwrap_or(core::cmp::Ordering::Equal)
    });
    let split_at = split_index(entries.len(), min_entries);
    let right_entries = entries.split_off(split_at);
    (
        LeafNode {
            entries: leaf.entries,
        },
        LeafNode {
            entries: right_entries,
        },
    )
}

/// Split an internal node using the linear split algorithm.
fn split_internal<T: Clone>(
    mut internal: InternalNode<T>,
    min_entries: usize,
) -> (InternalNode<T>, InternalNode<T>) {
    let entries = &mut internal.entries;
    entries.sort_by(|a, b| {
        a.bbox
            .min_x
            .partial_cmp(&b.bbox.min_x)
            .unwrap_or(core::cmp::Ordering::Equal)
    });
    let split_at = split_index(entries.len(), min_entries);
    let right_entries = entries.split_off(split_at);
    (
        InternalNode {
            entries: internal.entries,
        },
        InternalNode {
            entries: right_entries,
        },
    )
}

/// Determine the split point given the current length and minimum fill.
#[inline]
fn split_index(len: usize, min_entries: usize) -> usize {
    // We want each group to have at least `min_entries`.
    // Simple: give left half; clamp so that right also has `min_entries`.
    let half = len / 2;
    half.max(min_entries).min(len - min_entries)
}

// ---------------------------------------------------------------------------
// Recursive traversals
// ---------------------------------------------------------------------------

fn search_node<'a, T>(node: &'a Node<T>, query: &Bbox2D, results: &mut Vec<&'a T>) {
    match node {
        Node::Leaf(leaf) => {
            for e in &leaf.entries {
                if e.bbox.intersects(query) {
                    results.push(&e.value);
                }
            }
        }
        Node::Internal(internal) => {
            for e in &internal.entries {
                if e.bbox.intersects(query) {
                    search_node(&e.child, query, results);
                }
            }
        }
    }
}

fn collect_all_with_dist<'a, T>(node: &'a Node<T>, x: f64, y: f64, out: &mut Vec<(&'a T, f64)>) {
    match node {
        Node::Leaf(leaf) => {
            for e in &leaf.entries {
                let dist = e.bbox.min_distance_to_point(x, y);
                out.push((&e.value, dist));
            }
        }
        Node::Internal(internal) => {
            for e in &internal.entries {
                collect_all_with_dist(&e.child, x, y, out);
            }
        }
    }
}

fn collect_all_pairs<'a, T>(node: &'a Node<T>, out: &mut Vec<(&'a Bbox2D, &'a T)>) {
    match node {
        Node::Leaf(leaf) => {
            for e in &leaf.entries {
                out.push((&e.bbox, &e.value));
            }
        }
        Node::Internal(internal) => {
            for e in &internal.entries {
                collect_all_pairs(&e.child, out);
            }
        }
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

fn within_node<T: Clone>(node: &Node<T>, query: &Bbox2D, results: &mut Vec<T>) {
    match node {
        Node::Leaf(leaf) => {
            for e in &leaf.entries {
                if query.contains_bbox(&e.bbox) {
                    results.push(e.value.clone());
                }
            }
        }
        Node::Internal(internal) => {
            for e in &internal.entries {
                if e.bbox.intersects(query) {
                    within_node(&e.child, query, results);
                }
            }
        }
    }
}

fn count_in_node<T>(node: &Node<T>, query: &Bbox2D, count: &mut usize) {
    match node {
        Node::Leaf(leaf) => {
            for e in &leaf.entries {
                if e.bbox.intersects(query) {
                    *count += 1;
                }
            }
        }
        Node::Internal(internal) => {
            for e in &internal.entries {
                if e.bbox.intersects(query) {
                    count_in_node(&e.child, query, count);
                }
            }
        }
    }
}
