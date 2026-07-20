//! Internal node types and R*-tree split algorithm for the 3D R-tree.
//!
//! The split strategy follows the R*-tree paper (Beckmann et al., 1990),
//! adapted to three dimensions.  The key differences from the 2D variant:
//!
//! - **Axis choice** — margin is measured as surface area (not perimeter) of
//!   the union bbox for each candidate distribution.
//! - **Tie-breaking** — within the chosen axis, prefer the distribution with
//!   minimum volume overlap; ties broken by total volume of the two groups.
//! - **Forced reinsertion** — on the first overflow at a given level, the
//!   `p = ceil(0.3 * M)` farthest entries from the enclosing-box centre are
//!   removed and reinserted from the root.

use crate::bbox3d::Bbox3D;

// ---------------------------------------------------------------------------
// Tree-node types
// ---------------------------------------------------------------------------

pub(crate) struct LeafEntry3D<T> {
    pub(crate) bbox: Bbox3D,
    pub(crate) value: T,
}

pub(crate) struct InternalEntry3D<T> {
    pub(crate) bbox: Bbox3D,
    pub(crate) child: Box<Node3D<T>>,
}

pub(crate) enum Node3D<T> {
    Leaf(LeafNode3D<T>),
    Internal(InternalNode3D<T>),
}

pub(crate) struct LeafNode3D<T> {
    pub(crate) entries: Vec<LeafEntry3D<T>>,
}

pub(crate) struct InternalNode3D<T> {
    pub(crate) entries: Vec<InternalEntry3D<T>>,
}

// ---------------------------------------------------------------------------
// R*-tree parameters (3D)
// ---------------------------------------------------------------------------

/// Maximum entries per node (M).
pub(crate) const MAX_ENTRIES_3D: usize = 50;
/// Minimum entries per node (m = ceil(0.4 * M)).
pub(crate) const MIN_ENTRIES_3D: usize = 20;
/// Forced-reinsert count (p = ceil(0.3 * M)).
pub(crate) const REINSERT_P: usize = 15;

// ---------------------------------------------------------------------------
// Bounding-box computations
// ---------------------------------------------------------------------------

pub(crate) fn node3d_bbox<T>(node: &Node3D<T>) -> Bbox3D {
    match node {
        Node3D::Leaf(l) => leaf3d_bbox(l),
        Node3D::Internal(i) => internal3d_bbox(i),
    }
}

pub(crate) fn leaf3d_bbox<T>(leaf: &LeafNode3D<T>) -> Bbox3D {
    leaf.entries
        .iter()
        .map(|e| e.bbox)
        .reduce(|a, b| a.union(&b))
        .unwrap_or(Bbox3D::point(0.0, 0.0, 0.0))
}

pub(crate) fn internal3d_bbox<T>(internal: &InternalNode3D<T>) -> Bbox3D {
    internal
        .entries
        .iter()
        .map(|e| e.bbox)
        .reduce(|a, b| a.union(&b))
        .unwrap_or(Bbox3D::point(0.0, 0.0, 0.0))
}

// ---------------------------------------------------------------------------
// Choose-subtree
// ---------------------------------------------------------------------------

/// Choose the index of the child entry whose volume enlargement is minimised.
/// On equal enlargement, prefer the child with the smallest existing volume.
pub(crate) fn choose_subtree3d<T>(entries: &[InternalEntry3D<T>], bbox: &Bbox3D) -> usize {
    let mut best_idx = 0;
    let mut best_enlargement = f64::INFINITY;
    let mut best_volume = f64::INFINITY;
    for (i, e) in entries.iter().enumerate() {
        let enlargement = e.bbox.enlargement_to_include(bbox);
        if enlargement < best_enlargement
            || (enlargement == best_enlargement && e.bbox.volume() < best_volume)
        {
            best_idx = i;
            best_enlargement = enlargement;
            best_volume = e.bbox.volume();
        }
    }
    best_idx
}

// ---------------------------------------------------------------------------
// R*-tree split algorithm
// ---------------------------------------------------------------------------

/// An axis discriminator used during the split-axis-choice phase.
#[derive(Clone, Copy)]
enum Axis {
    X,
    Y,
    Z,
}

/// Sort entries by the lower edge of the given axis.
fn sort_by_axis<E, F>(entries: &mut [E], axis: Axis, center_fn: F)
where
    F: Fn(&E) -> (f64, f64, f64),
{
    match axis {
        Axis::X => entries.sort_by(|a, b| {
            let (ax, _, _) = center_fn(a);
            let (bx, _, _) = center_fn(b);
            ax.total_cmp(&bx)
        }),
        Axis::Y => entries.sort_by(|a, b| {
            let (_, ay, _) = center_fn(a);
            let (_, by, _) = center_fn(b);
            ay.total_cmp(&by)
        }),
        Axis::Z => entries.sort_by(|a, b| {
            let (_, _, az) = center_fn(a);
            let (_, _, bz) = center_fn(b);
            az.total_cmp(&bz)
        }),
    }
}

/// Compute the union bbox of entries in a slice using a bbox extractor.
fn union_bbox_of<E, F>(entries: &[E], bbox_fn: &F) -> Bbox3D
where
    F: Fn(&E) -> Bbox3D,
{
    entries
        .iter()
        .map(bbox_fn)
        .reduce(|a, b| a.union(&b))
        .unwrap_or(Bbox3D::point(0.0, 0.0, 0.0))
}

/// Compute the total surface-area margin-sum for all valid distributions of
/// `entries` given `min_entries`.
///
/// For a set of `n` entries with `m = min_entries`, there are
/// `n - 2*m + 1` candidate split points.  For each split point we form the
/// union of the left group and the union of the right group, and add both
/// surface areas to the running margin-sum.
fn margin_sum<E, F>(entries: &[E], min_entries: usize, bbox_fn: F) -> f64
where
    F: Fn(&E) -> Bbox3D,
{
    let n = entries.len();
    let mut total = 0.0;
    let last = n - min_entries;
    for split in min_entries..=last {
        let left_bb = union_bbox_of(&entries[..split], &bbox_fn);
        let right_bb = union_bbox_of(&entries[split..], &bbox_fn);
        total += left_bb.surface_area() + right_bb.surface_area();
    }
    total
}

/// Within the already-chosen axis, pick the split index that minimises volume
/// overlap.  Ties are broken by minimum total volume of the two groups.
fn best_split_index<E, F>(entries: &[E], min_entries: usize, bbox_fn: F) -> usize
where
    F: Fn(&E) -> Bbox3D,
{
    let n = entries.len();
    let last = n - min_entries;
    let mut best_split = min_entries;
    let mut best_overlap = f64::INFINITY;
    let mut best_total_vol = f64::INFINITY;

    for split in min_entries..=last {
        let left_bb = union_bbox_of(&entries[..split], &bbox_fn);
        let right_bb = union_bbox_of(&entries[split..], &bbox_fn);
        let overlap = left_bb
            .intersection(&right_bb)
            .map(|i| i.volume())
            .unwrap_or(0.0);
        let total_vol = left_bb.volume() + right_bb.volume();
        if overlap < best_overlap || (overlap == best_overlap && total_vol < best_total_vol) {
            best_split = split;
            best_overlap = overlap;
            best_total_vol = total_vol;
        }
    }
    best_split
}

// ---------------------------------------------------------------------------
// Public split functions
// ---------------------------------------------------------------------------

/// Split a leaf node using the R*-tree split algorithm (3D version).
///
/// Returns two leaf nodes.  The caller is responsible for wrapping them in
/// `Node3D::Leaf` and updating parent bboxes.
pub(crate) fn split_leaf3d<T: Clone>(
    mut leaf: LeafNode3D<T>,
    min_entries: usize,
) -> (LeafNode3D<T>, LeafNode3D<T>) {
    let entries = &mut leaf.entries;

    // Try all three axes, pick the one with the smallest margin-sum.
    let bbox_fn = |e: &LeafEntry3D<T>| e.bbox;
    let center_fn = |e: &LeafEntry3D<T>| e.bbox.center();

    sort_by_axis(entries, Axis::X, center_fn);
    let margin_x = margin_sum(entries, min_entries, bbox_fn);

    sort_by_axis(entries, Axis::Y, center_fn);
    let margin_y = margin_sum(entries, min_entries, bbox_fn);

    sort_by_axis(entries, Axis::Z, center_fn);
    let margin_z = margin_sum(entries, min_entries, bbox_fn);

    // Re-sort on the best axis.
    let best_axis = if margin_x <= margin_y && margin_x <= margin_z {
        Axis::X
    } else if margin_y <= margin_z {
        Axis::Y
    } else {
        Axis::Z
    };
    sort_by_axis(entries, best_axis, center_fn);

    // Find the best split index on that axis.
    let split_at = best_split_index(entries, min_entries, bbox_fn);

    let right_entries = entries.split_off(split_at);
    (
        LeafNode3D {
            entries: leaf.entries,
        },
        LeafNode3D {
            entries: right_entries,
        },
    )
}

/// Split an internal node using the R*-tree split algorithm (3D version).
pub(crate) fn split_internal3d<T: Clone>(
    mut internal: InternalNode3D<T>,
    min_entries: usize,
) -> (InternalNode3D<T>, InternalNode3D<T>) {
    let entries = &mut internal.entries;

    let bbox_fn = |e: &InternalEntry3D<T>| e.bbox;
    let center_fn = |e: &InternalEntry3D<T>| e.bbox.center();

    sort_by_axis(entries, Axis::X, center_fn);
    let margin_x = margin_sum(entries, min_entries, bbox_fn);

    sort_by_axis(entries, Axis::Y, center_fn);
    let margin_y = margin_sum(entries, min_entries, bbox_fn);

    sort_by_axis(entries, Axis::Z, center_fn);
    let margin_z = margin_sum(entries, min_entries, bbox_fn);

    let best_axis = if margin_x <= margin_y && margin_x <= margin_z {
        Axis::X
    } else if margin_y <= margin_z {
        Axis::Y
    } else {
        Axis::Z
    };
    sort_by_axis(entries, best_axis, center_fn);

    let split_at = best_split_index(entries, min_entries, bbox_fn);

    let right_entries = entries.split_off(split_at);
    (
        InternalNode3D {
            entries: internal.entries,
        },
        InternalNode3D {
            entries: right_entries,
        },
    )
}

// ---------------------------------------------------------------------------
// Forced reinsertion helpers
// ---------------------------------------------------------------------------

/// Extract the `p` farthest leaf entries from `entries` by distance of their
/// center to `enclosing_center`, returning them as a separate `Vec`.
pub(crate) fn extract_farthest_leaf_entries<T: Clone>(
    entries: &mut Vec<LeafEntry3D<T>>,
    enclosing_center: (f64, f64, f64),
    p: usize,
) -> Vec<LeafEntry3D<T>> {
    let (cx, cy, cz) = enclosing_center;
    // Sort descending by squared distance of entry center to enclosing center.
    entries.sort_by(|a, b| {
        let (ax, ay, az) = a.bbox.center();
        let (bx, by, bz) = b.bbox.center();
        let da = (ax - cx).powi(2) + (ay - cy).powi(2) + (az - cz).powi(2);
        let db = (bx - cx).powi(2) + (by - cy).powi(2) + (bz - cz).powi(2);
        // Descending
        db.total_cmp(&da)
    });
    let removed: Vec<LeafEntry3D<T>> = entries.drain(..p.min(entries.len())).collect();
    removed
}

// ---------------------------------------------------------------------------
// Recursive traversals
// ---------------------------------------------------------------------------

pub(crate) fn search_node3d<'a, T>(node: &'a Node3D<T>, query: &Bbox3D, results: &mut Vec<&'a T>) {
    match node {
        Node3D::Leaf(leaf) => {
            for e in &leaf.entries {
                if e.bbox.intersects(query) {
                    results.push(&e.value);
                }
            }
        }
        Node3D::Internal(internal) => {
            for e in &internal.entries {
                if e.bbox.intersects(query) {
                    search_node3d(&e.child, query, results);
                }
            }
        }
    }
}

pub(crate) fn collect_all_leaf_values3d<T: Clone>(node: &Node3D<T>, out: &mut Vec<(Bbox3D, T)>) {
    match node {
        Node3D::Leaf(leaf) => {
            for e in &leaf.entries {
                out.push((e.bbox, e.value.clone()));
            }
        }
        Node3D::Internal(internal) => {
            for e in &internal.entries {
                collect_all_leaf_values3d(&e.child, out);
            }
        }
    }
}
