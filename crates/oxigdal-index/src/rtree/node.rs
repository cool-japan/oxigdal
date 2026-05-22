//! Internal node types and helper functions for the R-tree.
//!
//! All items are `pub(crate)` so that sibling modules (`bulk`, `knn`,
//! `serial`) can access them without leaking implementation details from the
//! crate.
//!
//! The split heuristics implemented here follow the R*-tree paper:
//! Beckmann, Kriegel, Schneider, Seeger — "The R*-tree: An Efficient and
//! Robust Access Method for Points and Rectangles", SIGMOD 1990.

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
// R*-tree split heuristic (Beckmann et al., SIGMOD 1990)
// ---------------------------------------------------------------------------

/// Compute the union MBR of a slice of bboxes.
fn mbr_of_bboxes(bboxes: &[Bbox2D]) -> Bbox2D {
    bboxes
        .iter()
        .copied()
        .reduce(|a, b| a.union(&b))
        .unwrap_or(Bbox2D::point(0.0, 0.0))
}

/// Compute the overlap area between two MBRs.
fn overlap_area(a: &Bbox2D, b: &Bbox2D) -> f64 {
    a.intersection(b).map(|i| i.area()).unwrap_or(0.0)
}

/// R*-tree axis-selection and distribution-selection for a generic set of bboxes.
///
/// Returns the split index `k` (number of entries in the first group) and
/// the axis (0 = X, 1 = Y) sorted order used. The caller is responsible for
/// sorting the entry vector by the chosen axis before applying the split.
///
/// The algorithm:
/// 1. For each axis, sort by lower bound (ties broken by upper bound).
/// 2. For all valid distributions k = min_entries .. (len - min_entries + 1),
///    compute the sum of perimeters of the two group MBRs.
/// 3. Choose the axis with the smallest total perimeter-sum.
/// 4. On the chosen axis, pick the distribution minimising overlap (tie: area).
///
/// Returns `(split_index, chosen_axis)`.
fn rstar_choose_split_axis_and_index(bboxes: &mut [Bbox2D], min_entries: usize) -> (usize, u8) {
    let total = bboxes.len();
    // At least 2*min_entries entries are needed (ensured by caller).
    debug_assert!(total >= 2 * min_entries);

    // For each axis we compute the total margin (perimeter sum) across all
    // valid distributions.
    let compute_margin_sum = |axis: u8| -> f64 {
        // Sort by lower bound on the given axis; ties broken by upper bound.
        let mut sorted = bboxes.to_owned();
        sorted.sort_by(|a, b| {
            let (lo_a, hi_a, lo_b, hi_b) = if axis == 0 {
                (a.min_x, a.max_x, b.min_x, b.max_x)
            } else {
                (a.min_y, a.max_y, b.min_y, b.max_y)
            };
            lo_a.partial_cmp(&lo_b)
                .unwrap_or(core::cmp::Ordering::Equal)
                .then_with(|| {
                    hi_a.partial_cmp(&hi_b)
                        .unwrap_or(core::cmp::Ordering::Equal)
                })
        });

        let mut margin_sum = 0.0_f64;
        for k in min_entries..=(total - min_entries) {
            let left_mbr = mbr_of_bboxes(&sorted[..k]);
            let right_mbr = mbr_of_bboxes(&sorted[k..]);
            margin_sum += left_mbr.perimeter() + right_mbr.perimeter();
        }
        margin_sum
    };

    let margin_x = compute_margin_sum(0);
    let margin_y = compute_margin_sum(1);

    // Choose the axis with the smaller perimeter-sum.
    let chosen_axis: u8 = if margin_x <= margin_y { 0 } else { 1 };

    // Sort the original bboxes by the chosen axis.
    bboxes.sort_by(|a, b| {
        let (lo_a, hi_a, lo_b, hi_b) = if chosen_axis == 0 {
            (a.min_x, a.max_x, b.min_x, b.max_x)
        } else {
            (a.min_y, a.max_y, b.min_y, b.max_y)
        };
        lo_a.partial_cmp(&lo_b)
            .unwrap_or(core::cmp::Ordering::Equal)
            .then_with(|| {
                hi_a.partial_cmp(&hi_b)
                    .unwrap_or(core::cmp::Ordering::Equal)
            })
    });

    // Among all valid distributions along the chosen axis, pick the one
    // minimising overlap; ties broken by minimising total area.
    let mut best_k = min_entries;
    let mut best_overlap = f64::INFINITY;
    let mut best_area = f64::INFINITY;

    for k in min_entries..=(total - min_entries) {
        let left_mbr = mbr_of_bboxes(&bboxes[..k]);
        let right_mbr = mbr_of_bboxes(&bboxes[k..]);
        let ov = overlap_area(&left_mbr, &right_mbr);
        let total_area = left_mbr.area() + right_mbr.area();
        if ov < best_overlap || (ov == best_overlap && total_area < best_area) {
            best_k = k;
            best_overlap = ov;
            best_area = total_area;
        }
    }

    (best_k, chosen_axis)
}

/// Split a leaf node using the R*-tree split heuristic.
///
/// Both returned nodes are guaranteed to contain at least `min_entries`
/// entries.
pub(crate) fn split_leaf<T>(
    mut leaf: LeafNode<T>,
    min_entries: usize,
) -> (LeafNode<T>, LeafNode<T>) {
    let entries = &mut leaf.entries;
    let total = entries.len();

    // Ensure we have enough entries to split with the minimum fill constraint.
    // (min_entries must be at least 1 and at most total/2.)
    let effective_min = min_entries.max(1).min(total / 2);

    // Extract bboxes for the axis/distribution selection.
    let mut bboxes: Vec<Bbox2D> = entries.iter().map(|e| e.bbox).collect();
    let (split_k, chosen_axis) = rstar_choose_split_axis_and_index(&mut bboxes, effective_min);

    // Sort the actual entries by the same key used when choosing the axis.
    entries.sort_by(|a, b| {
        let (lo_a, hi_a, lo_b, hi_b) = if chosen_axis == 0 {
            (a.bbox.min_x, a.bbox.max_x, b.bbox.min_x, b.bbox.max_x)
        } else {
            (a.bbox.min_y, a.bbox.max_y, b.bbox.min_y, b.bbox.max_y)
        };
        lo_a.partial_cmp(&lo_b)
            .unwrap_or(core::cmp::Ordering::Equal)
            .then_with(|| {
                hi_a.partial_cmp(&hi_b)
                    .unwrap_or(core::cmp::Ordering::Equal)
            })
    });

    let right_entries = entries.split_off(split_k);
    (
        LeafNode {
            entries: leaf.entries,
        },
        LeafNode {
            entries: right_entries,
        },
    )
}

/// Split an internal node using the R*-tree split heuristic.
///
/// Both returned nodes are guaranteed to contain at least `min_entries`
/// entries.
pub(crate) fn split_internal<T>(
    mut internal: InternalNode<T>,
    min_entries: usize,
) -> (InternalNode<T>, InternalNode<T>) {
    let entries = &mut internal.entries;
    let total = entries.len();

    let effective_min = min_entries.max(1).min(total / 2);

    let mut bboxes: Vec<Bbox2D> = entries.iter().map(|e| e.bbox).collect();
    let (split_k, chosen_axis) = rstar_choose_split_axis_and_index(&mut bboxes, effective_min);

    entries.sort_by(|a, b| {
        let (lo_a, hi_a, lo_b, hi_b) = if chosen_axis == 0 {
            (a.bbox.min_x, a.bbox.max_x, b.bbox.min_x, b.bbox.max_x)
        } else {
            (a.bbox.min_y, a.bbox.max_y, b.bbox.min_y, b.bbox.max_y)
        };
        lo_a.partial_cmp(&lo_b)
            .unwrap_or(core::cmp::Ordering::Equal)
            .then_with(|| {
                hi_a.partial_cmp(&hi_b)
                    .unwrap_or(core::cmp::Ordering::Equal)
            })
    });

    let right_entries = entries.split_off(split_k);
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

/// Like `search_node` but returns both the stored bounding box and the value
/// reference for each matching entry.
///
/// Used by `RTree::search_with_bbox` to support distance-ranked window queries.
pub(crate) fn search_with_bbox_node<'a, T>(
    node: &'a Node<T>,
    query: &Bbox2D,
    results: &mut Vec<(&'a Bbox2D, &'a T)>,
) {
    match node {
        Node::Leaf(leaf) => {
            for e in &leaf.entries {
                if e.bbox.intersects(query) {
                    results.push((&e.bbox, &e.value));
                }
            }
        }
        Node::Internal(internal) => {
            for e in &internal.entries {
                if e.bbox.intersects(query) {
                    search_with_bbox_node(&e.child, query, results);
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

/// Validate that every non-root node in the subtree has at least `min_entries`
/// entries.  Leaf nodes and internal nodes are both checked.  Returns the
/// total number of leaf entries found (for cross-validation by callers).
pub(crate) fn validate_min_fill<T>(
    node: &Node<T>,
    min_entries: usize,
    is_root: bool,
    violations: &mut Vec<String>,
) -> usize {
    match node {
        Node::Leaf(leaf) => {
            if !is_root && leaf.entries.len() < min_entries {
                violations.push(format!(
                    "leaf node has {} entries, minimum is {}",
                    leaf.entries.len(),
                    min_entries
                ));
            }
            leaf.entries.len()
        }
        Node::Internal(internal) => {
            if !is_root && internal.entries.len() < min_entries {
                violations.push(format!(
                    "internal node has {} entries, minimum is {}",
                    internal.entries.len(),
                    min_entries
                ));
            }
            internal
                .entries
                .iter()
                .map(|e| validate_min_fill(&e.child, min_entries, false, violations))
                .sum()
        }
    }
}
