//! Internal node types and helper functions for the R-tree.
//!
//! All items are `pub(crate)` so that sibling modules (`bulk`, `knn`,
//! `serial`) can access them without leaking implementation details from the
//! crate.

#[cfg(not(feature = "std"))]
use alloc::{boxed::Box, vec, vec::Vec};

use crate::bbox::Bbox2D;

// ---------------------------------------------------------------------------
// Node types
// ---------------------------------------------------------------------------

pub(crate) struct LeafEntry<T> {
    pub(crate) bbox: Bbox2D,
    pub(crate) value: T,
}

pub(crate) struct InternalEntry<T> {
    pub(crate) bbox: Bbox2D,
    pub(crate) child: Box<Node<T>>,
}

pub(crate) enum Node<T> {
    Leaf(LeafNode<T>),
    Internal(InternalNode<T>),
}

pub(crate) struct LeafNode<T> {
    pub(crate) entries: Vec<LeafEntry<T>>,
}

pub(crate) struct InternalNode<T> {
    pub(crate) entries: Vec<InternalEntry<T>>,
}

// ---------------------------------------------------------------------------
// Bounding-box computations
// ---------------------------------------------------------------------------

/// Compute the bounding box of a node.
pub(crate) fn node_bbox<T>(node: &Node<T>) -> Bbox2D {
    match node {
        Node::Leaf(l) => leaf_bbox(l),
        Node::Internal(i) => internal_bbox(i),
    }
}

pub(crate) fn leaf_bbox<T>(leaf: &LeafNode<T>) -> Bbox2D {
    leaf.entries
        .iter()
        .map(|e| e.bbox)
        .reduce(|a, b| a.union(&b))
        .unwrap_or(Bbox2D::point(0.0, 0.0))
}

pub(crate) fn internal_bbox<T>(internal: &InternalNode<T>) -> Bbox2D {
    internal
        .entries
        .iter()
        .map(|e| e.bbox)
        .reduce(|a, b| a.union(&b))
        .unwrap_or(Bbox2D::point(0.0, 0.0))
}

// ---------------------------------------------------------------------------
// Subtree selection
// ---------------------------------------------------------------------------

/// Choose the index of the child whose enlargement is minimised.
pub(crate) fn choose_subtree<T>(entries: &[InternalEntry<T>], bbox: &Bbox2D) -> usize {
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

/// Determine the split point given the current length and minimum fill.
#[inline]
pub(crate) fn split_index(len: usize, min_entries: usize) -> usize {
    let half = len / 2;
    half.max(min_entries).min(len - min_entries)
}

/// Split a leaf node using the linear split algorithm.
pub(crate) fn split_leaf<T: Clone>(
    mut leaf: LeafNode<T>,
    min_entries: usize,
) -> (LeafNode<T>, LeafNode<T>) {
    let entries = &mut leaf.entries;
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
pub(crate) fn split_internal<T: Clone>(
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

// ---------------------------------------------------------------------------
// Recursive traversals
// ---------------------------------------------------------------------------

pub(crate) fn search_node<'a, T>(node: &'a Node<T>, query: &Bbox2D, results: &mut Vec<&'a T>) {
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

pub(crate) fn collect_all_pairs<'a, T>(node: &'a Node<T>, out: &mut Vec<(&'a Bbox2D, &'a T)>) {
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

pub(crate) fn within_node<T: Clone>(node: &Node<T>, query: &Bbox2D, results: &mut Vec<T>) {
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

pub(crate) fn count_in_node<T>(node: &Node<T>, query: &Bbox2D, count: &mut usize) {
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
