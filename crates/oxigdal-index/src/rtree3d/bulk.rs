//! Sort-Tile-Recursive (STR) bulk loading for the 3D R-tree.
//!
//! The 3D STR algorithm tiles along the X axis, then within each X-slab
//! tiles along Y, then within each XY-tile sorts by Z and groups into leaf
//! pages.  This produces a tight, balanced tree in O(n log n) time.
//!
//! Reference: Leutenegger, Lopez & Edgington (1997) — "STR: A Simple and
//! Efficient Algorithm for R-Tree Packing".

use crate::bbox3d::Bbox3D;

use super::node::{
    InternalEntry3D, InternalNode3D, LeafEntry3D, LeafNode3D, Node3D, internal3d_bbox, leaf3d_bbox,
};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Build a packed 3D R-tree from `items` using Sort-Tile-Recursive.
///
/// Returns `(root_node, entry_count)`, or `None` if `items` is empty.
pub(crate) fn str_bulk_load3d<T: Clone>(
    items: Vec<(Bbox3D, T)>,
    max_entries: usize,
) -> Option<(Node3D<T>, usize)> {
    if items.is_empty() {
        return None;
    }

    let count = items.len();
    let leaf_entries: Vec<LeafEntry3D<T>> = items
        .into_iter()
        .map(|(bbox, value)| LeafEntry3D { bbox, value })
        .collect();

    let leaf_nodes = pack_leaves3d(leaf_entries, max_entries);

    if leaf_nodes.len() == 1 {
        let node = leaf_nodes.into_iter().next()?;
        return Some((Node3D::Leaf(node), count));
    }

    // Wrap leaf nodes into internal entries.
    let mut current_level: Vec<InternalEntry3D<T>> = leaf_nodes
        .into_iter()
        .map(|leaf| {
            let bbox = leaf3d_bbox(&leaf);
            InternalEntry3D {
                bbox,
                child: Box::new(Node3D::Leaf(leaf)),
            }
        })
        .collect();

    // Recursively pack internal levels until a single root remains.
    while current_level.len() > max_entries {
        current_level = pack_internal_level3d(current_level, max_entries);
    }

    let root = Node3D::Internal(InternalNode3D {
        entries: current_level,
    });

    Some((root, count))
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

/// Pack leaf entries into leaf nodes using 3D STR:
///
/// 1. Sort all entries by center-x.
/// 2. Divide into `s_x = ceil(cbrt(n / M))` X-slabs of `s_x * s_x * M`
///    entries each.
/// 3. Within each X-slab, sort by center-y and divide into `s_x` Y-tiles.
/// 4. Within each Y-tile, sort by center-z and group into pages of ≤ M.
fn pack_leaves3d<T: Clone>(
    mut entries: Vec<LeafEntry3D<T>>,
    max_entries: usize,
) -> Vec<LeafNode3D<T>> {
    let n = entries.len();
    if n == 0 {
        return Vec::new();
    }

    let num_pages = n.div_ceil(max_entries);
    // Number of X-slabs (and Y-tiles within each slab).
    let s = (num_pages as f64).cbrt().ceil() as usize;
    let s = s.max(1);
    // Entries per X-slab (= s * s * max_entries, rounded up to cover all).
    let xslab_size = (s * s * max_entries).max(1);

    // Step 1: sort by center-x.
    entries.sort_by(|a, b| {
        let (ax, _, _) = a.bbox.center();
        let (bx, _, _) = b.bbox.center();
        ax.total_cmp(&bx)
    });

    let mut leaves: Vec<LeafNode3D<T>> = Vec::with_capacity(num_pages);

    // Step 2 & 3: process each X-slab.
    let mut slab_start = 0;
    while slab_start < n {
        let slab_end = (slab_start + xslab_size).min(n);
        let slab = &mut entries[slab_start..slab_end];
        let slab_len = slab.len();

        // Sort the slab by center-y.
        slab.sort_by(|a, b| {
            let (_, ay, _) = a.bbox.center();
            let (_, by, _) = b.bbox.center();
            ay.total_cmp(&by)
        });

        // Y-tiles within the slab.
        let yslab_size = (s * max_entries).max(1);
        let mut yslab_start = 0;
        while yslab_start < slab_len {
            let yslab_end = (yslab_start + yslab_size).min(slab_len);
            let ytile = &mut slab[yslab_start..yslab_end];

            // Step 4: sort Y-tile by center-z and group into pages.
            ytile.sort_by(|a, b| {
                let (_, _, az) = a.bbox.center();
                let (_, _, bz) = b.bbox.center();
                az.total_cmp(&bz)
            });

            yslab_start = yslab_end;
        }

        slab_start = slab_end;
    }

    // Group the globally sorted entries into leaf pages.
    let mut page: Vec<LeafEntry3D<T>> = Vec::with_capacity(max_entries);
    for entry in entries {
        page.push(entry);
        if page.len() == max_entries {
            leaves.push(LeafNode3D {
                entries: core::mem::replace(&mut page, Vec::with_capacity(max_entries)),
            });
        }
    }
    if !page.is_empty() {
        leaves.push(LeafNode3D { entries: page });
    }

    leaves
}

/// Pack internal entries one level up using the same 3D STR strategy.
fn pack_internal_level3d<T: Clone>(
    mut entries: Vec<InternalEntry3D<T>>,
    max_entries: usize,
) -> Vec<InternalEntry3D<T>> {
    let n = entries.len();
    let num_nodes = n.div_ceil(max_entries);
    let s = (num_nodes as f64).cbrt().ceil() as usize;
    let s = s.max(1);
    let xslab_size = (s * s * max_entries).max(1);

    entries.sort_by(|a, b| {
        let (ax, _, _) = a.bbox.center();
        let (bx, _, _) = b.bbox.center();
        ax.total_cmp(&bx)
    });

    let mut slab_start = 0;
    while slab_start < n {
        let slab_end = (slab_start + xslab_size).min(n);
        let slab = &mut entries[slab_start..slab_end];
        let slab_len = slab.len();

        slab.sort_by(|a, b| {
            let (_, ay, _) = a.bbox.center();
            let (_, by, _) = b.bbox.center();
            ay.total_cmp(&by)
        });

        let yslab_size = (s * max_entries).max(1);
        let mut yslab_start = 0;
        while yslab_start < slab_len {
            let yslab_end = (yslab_start + yslab_size).min(slab_len);
            let ytile = &mut slab[yslab_start..yslab_end];
            ytile.sort_by(|a, b| {
                let (_, _, az) = a.bbox.center();
                let (_, _, bz) = b.bbox.center();
                az.total_cmp(&bz)
            });
            yslab_start = yslab_end;
        }

        slab_start = slab_end;
    }

    // Group into parent internal nodes.
    let mut parent_entries: Vec<InternalEntry3D<T>> = Vec::with_capacity(num_nodes);
    let mut group: Vec<InternalEntry3D<T>> = Vec::with_capacity(max_entries);

    for entry in entries {
        group.push(entry);
        if group.len() == max_entries {
            let node = InternalNode3D {
                entries: core::mem::replace(&mut group, Vec::with_capacity(max_entries)),
            };
            let bbox = internal3d_bbox(&node);
            parent_entries.push(InternalEntry3D {
                bbox,
                child: Box::new(Node3D::Internal(node)),
            });
        }
    }

    if !group.is_empty() {
        let node = InternalNode3D { entries: group };
        let bbox = internal3d_bbox(&node);
        parent_entries.push(InternalEntry3D {
            bbox,
            child: Box::new(Node3D::Internal(node)),
        });
    }

    parent_entries
}
