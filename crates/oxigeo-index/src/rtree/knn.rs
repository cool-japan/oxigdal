//! Priority-queue k-nearest-neighbour search for the R-tree.
//!
//! Uses a min-heap (BinaryHeap with reversed ordering) and MINDIST pruning so
//! that nodes whose minimum squared distance exceeds the current k-th best are
//! never visited.

#[cfg(not(feature = "std"))]
use alloc::{collections::BinaryHeap, vec::Vec};

#[cfg(feature = "std")]
use std::collections::BinaryHeap;

use super::node::{InternalNode, LeafNode, Node};

// ---------------------------------------------------------------------------
// NaN-safe ordered distance wrapper
// ---------------------------------------------------------------------------

/// Wrapper around `f64` that provides a total ordering.
///
/// NaN is treated as +infinity so it always sorts last.
#[derive(Debug, Clone, Copy)]
struct OrderedDist(f64);

impl PartialEq for OrderedDist {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == core::cmp::Ordering::Equal
    }
}

impl Eq for OrderedDist {}

impl PartialOrd for OrderedDist {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OrderedDist {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

// ---------------------------------------------------------------------------
// Heap items
// ---------------------------------------------------------------------------

/// An item in the priority queue: either a leaf entry or an internal node to
/// expand.
enum HeapItem<'a, T> {
    /// A leaf entry with its value reference and squared distance.
    Leaf { value: &'a T, dist_sq: f64 },
    /// An internal node to be expanded later.
    Internal {
        node: &'a InternalNode<T>,
        dist_sq: f64,
    },
    /// A leaf node whose entries have not yet been individually pushed.
    LeafNode { node: &'a LeafNode<T>, dist_sq: f64 },
}

impl<'a, T> HeapItem<'a, T> {
    fn dist_sq(&self) -> f64 {
        match self {
            HeapItem::Leaf { dist_sq, .. } => *dist_sq,
            HeapItem::Internal { dist_sq, .. } => *dist_sq,
            HeapItem::LeafNode { dist_sq, .. } => *dist_sq,
        }
    }
}

/// Wrapper for min-heap ordering (BinaryHeap is a max-heap by default).
struct MinItem<'a, T> {
    item: HeapItem<'a, T>,
    key: OrderedDist,
}

impl<'a, T> PartialEq for MinItem<'a, T> {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl<'a, T> Eq for MinItem<'a, T> {}

impl<'a, T> PartialOrd for MinItem<'a, T> {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<'a, T> Ord for MinItem<'a, T> {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        // Reverse: smallest dist at the top of the BinaryHeap.
        other.key.cmp(&self.key)
    }
}

// ---------------------------------------------------------------------------
// Public k-NN function
// ---------------------------------------------------------------------------

/// Priority-queue k-NN search with MINDIST pruning.
///
/// Returns up to `k` entries nearest to `(x, y)`, ordered by ascending
/// Euclidean distance. The distance returned is the ordinary (non-squared)
/// minimum distance from the point to the entry's bounding box.
pub(crate) fn knn_search<'a, T>(root: &'a Node<T>, x: f64, y: f64, k: usize) -> Vec<(&'a T, f64)> {
    let mut heap: BinaryHeap<MinItem<'a, T>> = BinaryHeap::new();
    let mut results: Vec<(&'a T, f64)> = Vec::with_capacity(k);

    // Seed the heap with the root node.
    push_node(&mut heap, root, x, y);

    // Track the k-th best squared distance for pruning.
    let mut kth_dist_sq = f64::INFINITY;

    while let Some(min_item) = heap.pop() {
        let item_dist_sq = min_item.item.dist_sq();

        // Prune: if this item's MINDIST^2 exceeds the k-th best, we're done.
        if results.len() >= k && item_dist_sq > kth_dist_sq {
            break;
        }

        match min_item.item {
            HeapItem::Leaf { value, dist_sq } => {
                results.push((value, dist_sq.sqrt()));
                if results.len() >= k {
                    // Update the pruning threshold (already sorted by heap).
                    // The k-th result's squared distance is the new bound.
                    kth_dist_sq = dist_sq;
                }
                if results.len() >= k {
                    // Drain any remaining items that can't beat the k-th.
                    // (The loop condition above will handle this.)
                }
            }
            HeapItem::Internal { node, .. } => {
                // Expand: push each child into the heap.
                for e in &node.entries {
                    let child_dist_sq = e.bbox.min_distance_sq_to_point(x, y);
                    if results.len() < k || child_dist_sq <= kth_dist_sq {
                        push_node(&mut heap, &e.child, x, y);
                    }
                }
            }
            HeapItem::LeafNode { node, .. } => {
                // Push individual leaf entries.
                for e in &node.entries {
                    let entry_dist_sq = e.bbox.min_distance_sq_to_point(x, y);
                    if results.len() < k || entry_dist_sq <= kth_dist_sq {
                        heap.push(MinItem {
                            key: OrderedDist(entry_dist_sq),
                            item: HeapItem::Leaf {
                                value: &e.value,
                                dist_sq: entry_dist_sq,
                            },
                        });
                    }
                }
            }
        }
    }

    results.truncate(k);
    results
}

/// Push a node into the heap, computing its MINDIST^2 to the query point.
fn push_node<'a, T>(heap: &mut BinaryHeap<MinItem<'a, T>>, node: &'a Node<T>, x: f64, y: f64) {
    match node {
        Node::Leaf(leaf) => {
            // Push the leaf node itself; individual entries will be expanded
            // when it's popped.
            let node_dist_sq = leaf
                .entries
                .iter()
                .map(|e| e.bbox.min_distance_sq_to_point(x, y))
                .fold(f64::INFINITY, f64::min);
            heap.push(MinItem {
                key: OrderedDist(node_dist_sq),
                item: HeapItem::LeafNode {
                    node: leaf,
                    dist_sq: node_dist_sq,
                },
            });
        }
        Node::Internal(internal) => {
            let node_dist_sq = internal
                .entries
                .iter()
                .map(|e| e.bbox.min_distance_sq_to_point(x, y))
                .fold(f64::INFINITY, f64::min);
            heap.push(MinItem {
                key: OrderedDist(node_dist_sq),
                item: HeapItem::Internal {
                    node: internal,
                    dist_sq: node_dist_sq,
                },
            });
        }
    }
}
