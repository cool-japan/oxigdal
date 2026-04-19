//! Sort-Tile-Recursive (STR) bulk loading for the R-tree.
//!
//! Given a vector of `(Bbox2D, T)` pairs, builds a packed R-tree bottom-up in
//! O(n log n) time.  The resulting tree has full leaf nodes (except possibly
//! the last in each slice) and is typically shallower and tighter than one
//! built by repeated insertion.

#[cfg(not(feature = "std"))]
use alloc::{boxed::Box, vec::Vec};

use crate::bbox::Bbox2D;

use super::node::{
    InternalEntry, InternalNode, LeafEntry, LeafNode, Node, internal_bbox, leaf_bbox,
};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Build a packed R-tree from `items` using the Sort-Tile-Recursive algorithm.
///
/// Returns `(root_node, entry_count)`.
///
/// If `items` is empty, returns `None`.
pub(crate) fn str_bulk_load<T: Clone>(
    items: Vec<(Bbox2D, T)>,
    max_entries: usize,
) -> Option<(Node<T>, usize)> {
    if items.is_empty() {
        return None;
    }

    let count = items.len();

    // Convert to leaf entries.
    let leaf_entries: Vec<LeafEntry<T>> = items
        .into_iter()
        .map(|(bbox, value)| LeafEntry { bbox, value })
        .collect();

    // Pack leaf nodes.
    let leaf_nodes = pack_leaves(leaf_entries, max_entries);

    // If everything fits in a single leaf, return immediately.
    if leaf_nodes.len() == 1 {
        let node = leaf_nodes.into_iter().next()?;
        return Some((Node::Leaf(node), count));
    }

    // Wrap leaf nodes into internal entries.
    let mut current_level: Vec<InternalEntry<T>> = leaf_nodes
        .into_iter()
        .map(|leaf| {
            let bbox = leaf_bbox(&leaf);
            InternalEntry {
                bbox,
                child: Box::new(Node::Leaf(leaf)),
            }
        })
        .collect();

    // Recursively build internal levels until we have a single root.
    while current_level.len() > max_entries {
        current_level = pack_internal_level(current_level, max_entries);
    }

    let root = Node::Internal(InternalNode {
        entries: current_level,
    });

    Some((root, count))
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

/// Pack leaf entries into leaf nodes using STR: sort by center-x, partition
/// into vertical slices of `sqrt(n / M)` groups, within each slice sort by
/// center-y, then group into pages of at most `max_entries`.
fn pack_leaves<T: Clone>(mut entries: Vec<LeafEntry<T>>, max_entries: usize) -> Vec<LeafNode<T>> {
    let n = entries.len();
    if n == 0 {
        return Vec::new();
    }

    // Number of leaf pages needed.
    let num_pages = n.div_ceil(max_entries);
    // Number of vertical slices.
    let num_slices = (num_pages as f64).sqrt().ceil() as usize;
    let slice_size = num_slices * max_entries;

    // Sort by center-x.
    entries.sort_by(|a, b| {
        let ca = a.bbox.center().0;
        let cb = b.bbox.center().0;
        ca.partial_cmp(&cb).unwrap_or(core::cmp::Ordering::Equal)
    });

    let mut leaves: Vec<LeafNode<T>> = Vec::with_capacity(num_pages);

    // Process each vertical slice.
    for slice_start in (0..n).step_by(slice_size) {
        let slice_end = (slice_start + slice_size).min(n);
        let slice = &mut entries[slice_start..slice_end];

        // Within the slice, sort by center-y.
        slice.sort_by(|a, b| {
            let ca = a.bbox.center().1;
            let cb = b.bbox.center().1;
            ca.partial_cmp(&cb).unwrap_or(core::cmp::Ordering::Equal)
        });
    }

    // Now group sorted entries into pages.
    let mut page = Vec::with_capacity(max_entries);
    for entry in entries {
        page.push(entry);
        if page.len() == max_entries {
            leaves.push(LeafNode {
                entries: core::mem::replace(&mut page, Vec::with_capacity(max_entries)),
            });
        }
    }
    if !page.is_empty() {
        leaves.push(LeafNode { entries: page });
    }

    leaves
}

/// Pack internal entries one level up: sort by center-x of their bbox,
/// partition into slices, sort within by center-y, and group into internal
/// nodes of at most `max_entries` children.
fn pack_internal_level<T: Clone>(
    mut entries: Vec<InternalEntry<T>>,
    max_entries: usize,
) -> Vec<InternalEntry<T>> {
    let n = entries.len();
    let num_nodes = n.div_ceil(max_entries);
    let num_slices = (num_nodes as f64).sqrt().ceil() as usize;
    let slice_size = num_slices * max_entries;

    // Sort by center-x.
    entries.sort_by(|a, b| {
        let ca = a.bbox.center().0;
        let cb = b.bbox.center().0;
        ca.partial_cmp(&cb).unwrap_or(core::cmp::Ordering::Equal)
    });

    // Within each slice, sort by center-y.
    for slice_start in (0..n).step_by(slice_size) {
        let slice_end = (slice_start + slice_size).min(n);
        let slice = &mut entries[slice_start..slice_end];
        slice.sort_by(|a, b| {
            let ca = a.bbox.center().1;
            let cb = b.bbox.center().1;
            ca.partial_cmp(&cb).unwrap_or(core::cmp::Ordering::Equal)
        });
    }

    // Group into parent internal nodes.
    let mut parent_entries: Vec<InternalEntry<T>> = Vec::with_capacity(num_nodes);
    let mut group: Vec<InternalEntry<T>> = Vec::with_capacity(max_entries);

    for entry in entries {
        group.push(entry);
        if group.len() == max_entries {
            let node = InternalNode {
                entries: core::mem::replace(&mut group, Vec::with_capacity(max_entries)),
            };
            let bbox = internal_bbox(&node);
            parent_entries.push(InternalEntry {
                bbox,
                child: Box::new(Node::Internal(node)),
            });
        }
    }

    if !group.is_empty() {
        let node = InternalNode { entries: group };
        let bbox = internal_bbox(&node);
        parent_entries.push(InternalEntry {
            bbox,
            child: Box::new(Node::Internal(node)),
        });
    }

    parent_entries
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_items(n: usize) -> Vec<(Bbox2D, usize)> {
        (0..n)
            .map(|i| {
                let f = i as f64;
                let bbox = Bbox2D::new(f, f, f + 1.0, f + 1.0).expect("valid bbox");
                (bbox, i)
            })
            .collect()
    }

    #[test]
    fn bulk_load_empty_returns_none() {
        let items: Vec<(Bbox2D, u32)> = Vec::new();
        assert!(str_bulk_load(items, 9).is_none());
    }

    #[test]
    fn bulk_load_single_item() {
        let items = vec![(Bbox2D::new(0.0, 0.0, 1.0, 1.0).expect("valid"), 42_u32)];
        let (node, count) = str_bulk_load(items, 9).expect("non-empty");
        assert_eq!(count, 1);
        assert!(matches!(node, Node::Leaf(_)));
    }

    #[test]
    fn bulk_load_preserves_count() {
        let items = make_items(100);
        let (_, count) = str_bulk_load(items, 9).expect("non-empty");
        assert_eq!(count, 100);
    }

    #[test]
    fn bulk_load_small_m() {
        let items = make_items(50);
        let (_, count) = str_bulk_load(items, 3).expect("non-empty");
        assert_eq!(count, 50);
    }

    #[test]
    fn bulk_load_exact_m_entries() {
        let items = make_items(9);
        let (node, count) = str_bulk_load(items, 9).expect("non-empty");
        assert_eq!(count, 9);
        // All fit in one leaf.
        assert!(matches!(node, Node::Leaf(_)));
    }

    #[test]
    fn bulk_load_bboxes_searchable() {
        let items = make_items(200);
        let (node, _) = str_bulk_load(items, 9).expect("non-empty");
        // Search for an entry that should exist.
        let query = Bbox2D::new(50.0, 50.0, 51.0, 51.0).expect("valid");
        let mut results = Vec::new();
        super::super::node::search_node(&node, &query, &mut results);
        assert!(
            !results.is_empty(),
            "bulk-loaded tree should find entry at (50,50)"
        );
    }
}
