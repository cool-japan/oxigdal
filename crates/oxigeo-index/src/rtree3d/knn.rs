//! Priority-queue k-nearest-neighbour search for the 3D R-tree.
//!
//! The algorithm is a direct 3D extension of the 2D knn module:
//! a min-heap is seeded with the root and expanded node-by-node, using
//! the MINDIST³ metric (`min_distance_sq_to_point`) for pruning so that
//! subtrees farther than the k-th found result are never visited.

use std::collections::BinaryHeap;

use super::node::{InternalNode3D, LeafNode3D, Node3D};

// ---------------------------------------------------------------------------
// NaN-safe ordered distance wrapper
// ---------------------------------------------------------------------------

/// Wrapper around `f64` providing a total ordering.
/// NaN is treated as +∞ so it always sorts last.
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

enum HeapItem3D<'a, T> {
    Leaf {
        value: &'a T,
        dist_sq: f64,
    },
    Internal {
        node: &'a InternalNode3D<T>,
        dist_sq: f64,
    },
    LeafNode {
        node: &'a LeafNode3D<T>,
        dist_sq: f64,
    },
}

impl<'a, T> HeapItem3D<'a, T> {
    fn dist_sq(&self) -> f64 {
        match self {
            HeapItem3D::Leaf { dist_sq, .. } => *dist_sq,
            HeapItem3D::Internal { dist_sq, .. } => *dist_sq,
            HeapItem3D::LeafNode { dist_sq, .. } => *dist_sq,
        }
    }
}

/// Min-heap wrapper (BinaryHeap is a max-heap; we reverse ordering).
struct MinItem3D<'a, T> {
    item: HeapItem3D<'a, T>,
    key: OrderedDist,
}

impl<'a, T> PartialEq for MinItem3D<'a, T> {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl<'a, T> Eq for MinItem3D<'a, T> {}

impl<'a, T> PartialOrd for MinItem3D<'a, T> {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<'a, T> Ord for MinItem3D<'a, T> {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        // Reverse: smallest dist at the top of the BinaryHeap.
        other.key.cmp(&self.key)
    }
}

// ---------------------------------------------------------------------------
// Public k-NN function
// ---------------------------------------------------------------------------

/// Priority-queue k-NN search with MINDIST³ pruning.
///
/// Returns up to `k` references to values nearest to `(x, y, z)`, ordered by
/// ascending Euclidean distance from the point to the entry's bounding box.
pub(crate) fn knn_search3d<'a, T>(
    root: &'a Node3D<T>,
    x: f64,
    y: f64,
    z: f64,
    k: usize,
) -> Vec<&'a T> {
    let mut heap: BinaryHeap<MinItem3D<'a, T>> = BinaryHeap::new();
    let mut results: Vec<&'a T> = Vec::with_capacity(k);
    let mut kth_dist_sq = f64::INFINITY;

    push_node3d(&mut heap, root, x, y, z);

    while let Some(min_item) = heap.pop() {
        let item_dist_sq = min_item.item.dist_sq();

        // Prune: nothing farther than the k-th best can contribute.
        if results.len() >= k && item_dist_sq > kth_dist_sq {
            break;
        }

        match min_item.item {
            HeapItem3D::Leaf { value, dist_sq } => {
                results.push(value);
                if results.len() >= k {
                    kth_dist_sq = dist_sq;
                }
            }
            HeapItem3D::Internal { node, .. } => {
                for e in &node.entries {
                    let child_dist_sq = e.bbox.min_distance_sq_to_point(x, y, z);
                    if results.len() < k || child_dist_sq <= kth_dist_sq {
                        push_node3d(&mut heap, &e.child, x, y, z);
                    }
                }
            }
            HeapItem3D::LeafNode { node, .. } => {
                for e in &node.entries {
                    let entry_dist_sq = e.bbox.min_distance_sq_to_point(x, y, z);
                    if results.len() < k || entry_dist_sq <= kth_dist_sq {
                        heap.push(MinItem3D {
                            key: OrderedDist(entry_dist_sq),
                            item: HeapItem3D::Leaf {
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

/// Push a node into the priority queue.
fn push_node3d<'a, T>(
    heap: &mut BinaryHeap<MinItem3D<'a, T>>,
    node: &'a Node3D<T>,
    x: f64,
    y: f64,
    z: f64,
) {
    match node {
        Node3D::Leaf(leaf) => {
            let node_dist_sq = leaf
                .entries
                .iter()
                .map(|e| e.bbox.min_distance_sq_to_point(x, y, z))
                .fold(f64::INFINITY, f64::min);
            heap.push(MinItem3D {
                key: OrderedDist(node_dist_sq),
                item: HeapItem3D::LeafNode {
                    node: leaf,
                    dist_sq: node_dist_sq,
                },
            });
        }
        Node3D::Internal(internal) => {
            let node_dist_sq = internal
                .entries
                .iter()
                .map(|e| e.bbox.min_distance_sq_to_point(x, y, z))
                .fold(f64::INFINITY, f64::min);
            heap.push(MinItem3D {
                key: OrderedDist(node_dist_sq),
                item: HeapItem3D::Internal {
                    node: internal,
                    dist_sq: node_dist_sq,
                },
            });
        }
    }
}
