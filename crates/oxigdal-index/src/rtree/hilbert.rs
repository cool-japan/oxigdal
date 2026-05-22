//! Hilbert R-tree variant for bulk-loaded spatial indices.
//!
//! Implements the Kamel & Faloutsos (1994) Hilbert R-tree: entries are sorted
//! by the Hilbert curve value of their bounding-box centre before being packed
//! into a flat, immutable index.  This improves spatial locality for
//! sequential disk reads compared to a naïve R-tree.
//!
//! # Design
//!
//! * **Bulk-load only** – insertion after construction is not supported because
//!   it would invalidate the Hilbert order.
//! * **Two-level structure** – one layer of internal nodes (fan-out `MAX_ENTRIES`
//!   = 16) over a flat sorted leaf array.  A single internal level handles
//!   millions of entries at this fan-out.
//! * **Zero `unwrap`** in production code – `bulk_load` returns `Result`.
//!
//! # Algorithm sketch
//!
//! 1. Compute the world bbox from all input items.
//! 2. Normalise each bbox centre to a `2^order × 2^order` integer grid.
//! 3. Encode each grid point as a Hilbert curve index.
//! 4. Sort leaf entries by Hilbert index (stable sort preserves ties'
//!    original order, which is deterministic for identical inputs).
//! 5. Group every `MAX_ENTRIES` leaves into one internal node whose MBR is
//!    the union of its children's bboxes.

use crate::bbox::Bbox2D;
use crate::error::IndexError;

// ---------------------------------------------------------------------------
// Hilbert curve math (ported inline from oxigdal-pmtiles/src/hilbert.rs)
// ---------------------------------------------------------------------------

/// Rotate/flip a quadrant for the Hilbert curve.
///
/// This is the standard "rotate" subroutine used in the iterative
/// Hilbert-index computation.
#[inline]
fn hilbert_rotate(n: u32, x: &mut u32, y: &mut u32, rx: u32, ry: u32) {
    if ry == 0 {
        if rx == 1 {
            *x = n.wrapping_sub(1).wrapping_sub(*x);
            *y = n.wrapping_sub(1).wrapping_sub(*y);
        }
        core::mem::swap(x, y);
    }
}

/// Convert `(x, y)` grid coordinates to the Hilbert curve index for a grid of
/// size `2^order × 2^order`.
///
/// `order = 16` gives a 65 536 × 65 536 grid — the recommended default.
/// `order = 0` always returns 0.
pub fn xy_to_hilbert(x: u32, y: u32, order: u32) -> u64 {
    if order == 0 {
        return 0;
    }
    let mut d: u64 = 0;
    let mut mx = x;
    let mut my = y;
    let mut s = 1u32 << (order - 1);

    while s > 0 {
        let rx: u32 = if (mx & s) > 0 { 1 } else { 0 };
        let ry: u32 = if (my & s) > 0 { 1 } else { 0 };
        d += u64::from(s) * u64::from(s) * u64::from((3 * rx) ^ ry);
        hilbert_rotate(s, &mut mx, &mut my, rx, ry);
        s >>= 1;
    }
    d
}

/// Compute the Hilbert curve value for the centre of `bbox` relative to `world`.
///
/// The centre is normalised to an integer grid of size `2^order × 2^order`.
/// If `world` has zero width or height in a dimension the grid coordinate for
/// that dimension is set to 0.
///
/// # Arguments
///
/// * `bbox`  – the bounding box whose Hilbert value is desired.
/// * `world` – overall extent used for normalisation (must span the whole dataset).
/// * `order` – Hilbert order; 16 is recommended (gives 65 536 × 65 536 grid).
pub fn compute_hilbert_value(bbox: &Bbox2D, world: &Bbox2D, order: u32) -> u64 {
    let (cx, cy) = bbox.center();
    let max_coord = (1u64 << order) as f64;

    let world_w = world.max_x - world.min_x;
    let world_h = world.max_y - world.min_y;

    let nx = if world_w > 0.0 {
        ((cx - world.min_x) / world_w * max_coord).clamp(0.0, max_coord - 1.0) as u32
    } else {
        0u32
    };
    let ny = if world_h > 0.0 {
        ((cy - world.min_y) / world_h * max_coord).clamp(0.0, max_coord - 1.0) as u32
    } else {
        0u32
    };

    xy_to_hilbert(nx, ny, order)
}

// ---------------------------------------------------------------------------
// Internal node fan-out
// ---------------------------------------------------------------------------

/// Maximum number of leaf entries per internal node (fan-out).
const MAX_ENTRIES: usize = 16;

// ---------------------------------------------------------------------------
// Internal node structure
// ---------------------------------------------------------------------------

/// An internal node covering a contiguous slice of sorted leaf entries.
struct HilbertInternal {
    /// MBR (minimum bounding rectangle) of all leaf entries in `[start, end)`.
    bbox: Bbox2D,
    /// Maximum Hilbert value of leaves in this node (last entry's value).
    max_hilbert: u64,
    /// First index (inclusive) into the leaf array.
    start: usize,
    /// Last index (exclusive) into the leaf array.
    end: usize,
}

// ---------------------------------------------------------------------------
// HilbertRTree
// ---------------------------------------------------------------------------

/// A read-only spatial index ordered by Hilbert curve value for sequential
/// disk-locality.
///
/// Entries are bulk-loaded and packed bottom-up using Hilbert-curve sorting.
/// After [`bulk_load`](HilbertRTree::bulk_load) the tree is immutable —
/// incremental insertion would invalidate the Hilbert order.
///
/// # Search
///
/// [`search`](HilbertRTree::search) uses the internal node layer to skip whole
/// groups whose MBR does not intersect the query, then linearly scans the
/// matching leaf entries.
///
/// # Example
///
/// ```rust
/// use oxigdal_index::{Bbox2D, HilbertRTree};
///
/// let items = vec![
///     (Bbox2D::new(0.0, 0.0, 1.0, 1.0).unwrap(), "a"),
///     (Bbox2D::new(2.0, 2.0, 3.0, 3.0).unwrap(), "b"),
/// ];
/// let tree = HilbertRTree::bulk_load(items, 16).unwrap();
/// let q = Bbox2D::new(0.5, 0.5, 1.5, 1.5).unwrap();
/// assert_eq!(tree.search(&q), vec![&"a"]);
/// ```
pub struct HilbertRTree<T: Clone> {
    /// Leaf entries sorted by Hilbert value: `(hilbert_value, bbox, user_value)`.
    entries: Vec<(u64, Bbox2D, T)>,
    /// Internal nodes covering contiguous slices of `entries`.
    internal: Vec<HilbertInternal>,
    /// Total number of leaf entries.
    size: usize,
    /// Overall bounding box of all entries (world extent used for normalisation).
    world: Bbox2D,
    /// Hilbert curve order used at bulk-load time.
    order: u32,
}

impl<T: Clone> HilbertRTree<T> {
    /// Create an empty `HilbertRTree` with a unit world bbox and default order 16.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            internal: Vec::new(),
            size: 0,
            world: Bbox2D {
                min_x: 0.0,
                min_y: 0.0,
                max_x: 1.0,
                max_y: 1.0,
            },
            order: 16,
        }
    }

    /// Whether the tree contains no entries.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    /// Number of leaf entries in the tree.
    #[inline]
    pub fn len(&self) -> usize {
        self.size
    }

    /// Build a `HilbertRTree` from a collection of `(bbox, value)` pairs.
    ///
    /// The process is O(n log n):
    ///
    /// 1. Compute the world bbox (union of all input bboxes).
    /// 2. Normalise each bbox centre to a `2^order × 2^order` grid and
    ///    compute its Hilbert index.
    /// 3. Sort all entries by Hilbert index.
    /// 4. Group every `MAX_ENTRIES` (= 16) consecutive leaves into an internal
    ///    node whose MBR is the union of its children's bboxes.
    ///
    /// # Arguments
    ///
    /// * `items` – collection of `(Bbox2D, T)` to index.
    /// * `order` – Hilbert order (16 recommended).
    ///
    /// # Errors
    ///
    /// Returns [`IndexError::EmptyInput`] if `items` is empty.
    pub fn bulk_load(items: Vec<(Bbox2D, T)>, order: u32) -> Result<Self, IndexError> {
        if items.is_empty() {
            return Err(IndexError::EmptyInput);
        }

        // --- Step 1: compute the world bbox from all items ---
        let world = compute_world_bbox(&items);

        // --- Step 2: assign Hilbert values ---
        let mut entries: Vec<(u64, Bbox2D, T)> = items
            .into_iter()
            .map(|(bbox, val)| {
                let hv = compute_hilbert_value(&bbox, &world, order);
                (hv, bbox, val)
            })
            .collect();

        // --- Step 3: stable sort by Hilbert value ---
        entries.sort_by_key(|e| e.0);

        let size = entries.len();

        // --- Step 4: build internal node layer ---
        let internal = build_internal_nodes(&entries);

        Ok(Self {
            entries,
            internal,
            size,
            world,
            order,
        })
    }

    /// Search for all entries whose bounding box intersects `query`.
    ///
    /// Uses the internal node layer to prune whole groups that are disjoint
    /// from `query`, then linearly scans matching leaf slices.
    ///
    /// Returns references to the stored values (in Hilbert-curve order).
    pub fn search(&self, query: &Bbox2D) -> Vec<&T> {
        if self.internal.is_empty() {
            // Tree has entries but no internal nodes (shouldn't happen after
            // bulk_load, but handle gracefully).
            return self
                .entries
                .iter()
                .filter(|(_, bbox, _)| bbox.intersects(query))
                .map(|(_, _, v)| v)
                .collect();
        }

        let mut results: Vec<&T> = Vec::new();

        for node in &self.internal {
            // Prune: skip this internal node if its MBR doesn't touch query.
            if !node.bbox.intersects(query) {
                continue;
            }
            // Scan its leaf slice.
            for entry in &self.entries[node.start..node.end] {
                if entry.1.intersects(query) {
                    results.push(&entry.2);
                }
            }
        }

        results
    }

    /// The world bounding box used during bulk-load normalisation.
    pub fn world_bbox(&self) -> &Bbox2D {
        &self.world
    }

    /// The Hilbert order used during bulk-load.
    pub fn order(&self) -> u32 {
        self.order
    }
}

impl<T: Clone> Default for HilbertRTree<T> {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Compute the union of all bboxes in `items`, with a small epsilon guard so
/// that a degenerate world (all identical points) does not cause division by
/// zero during normalisation.
fn compute_world_bbox<T>(items: &[(Bbox2D, T)]) -> Bbox2D {
    debug_assert!(!items.is_empty(), "caller guarantees non-empty");

    let mut world = items[0].0;
    for (bbox, _) in items.iter().skip(1) {
        world = world.union(bbox);
    }

    // Guard: if the world has zero width or height, expand it slightly so
    // normalisation never divides by zero.
    const EPSILON: f64 = 1e-10;
    if (world.max_x - world.min_x) < EPSILON {
        world.min_x -= EPSILON;
        world.max_x += EPSILON;
    }
    if (world.max_y - world.min_y) < EPSILON {
        world.min_y -= EPSILON;
        world.max_y += EPSILON;
    }
    world
}

/// Build the internal node layer from a Hilbert-sorted leaf slice.
///
/// Groups every `MAX_ENTRIES` leaves into one internal node.  The last node
/// may contain fewer entries.
fn build_internal_nodes<T>(entries: &[(u64, Bbox2D, T)]) -> Vec<HilbertInternal> {
    if entries.is_empty() {
        return Vec::new();
    }

    let node_count = entries.len().div_ceil(MAX_ENTRIES);
    let mut nodes = Vec::with_capacity(node_count);

    let mut start = 0usize;
    while start < entries.len() {
        let end = (start + MAX_ENTRIES).min(entries.len());

        // Compute MBR of this slice.
        let mut node_bbox = entries[start].1;
        for entry in entries.iter().take(end).skip(start + 1) {
            node_bbox = node_bbox.union(&entry.1);
        }

        // The maximum Hilbert value in this slice is the last entry's value.
        let max_hilbert = entries[end - 1].0;

        nodes.push(HilbertInternal {
            bbox: node_bbox,
            max_hilbert,
            start,
            end,
        });

        start = end;
    }

    nodes
}

// ---------------------------------------------------------------------------
// Unused-field suppression: HilbertInternal.max_hilbert is intentionally
// kept for future range-query pruning but not used in the current search path.
// ---------------------------------------------------------------------------

impl HilbertInternal {
    /// Maximum Hilbert value stored in this internal node's leaf slice.
    ///
    /// Reserved for future Hilbert-range queries (not used by the current
    /// bbox-intersection search).
    #[allow(dead_code)]
    #[inline]
    pub fn max_hilbert(&self) -> u64 {
        self.max_hilbert
    }
}

// ---------------------------------------------------------------------------
// Unit tests (module-internal)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hilbert_rotate_identity_ry1() {
        // When ry == 1, rotate is a no-op.
        let mut x = 3u32;
        let mut y = 7u32;
        hilbert_rotate(8, &mut x, &mut y, 0, 1);
        assert_eq!((x, y), (3, 7));
    }

    #[test]
    fn hilbert_rotate_swap_when_rx0_ry0() {
        // ry == 0, rx == 0 → swap x and y.
        let mut x = 2u32;
        let mut y = 5u32;
        hilbert_rotate(8, &mut x, &mut y, 0, 0);
        assert_eq!((x, y), (5, 2));
    }

    #[test]
    fn xy_to_hilbert_order0_is_zero() {
        assert_eq!(xy_to_hilbert(0, 0, 0), 0);
        assert_eq!(xy_to_hilbert(100, 200, 0), 0);
    }

    #[test]
    fn xy_to_hilbert_order1_known_values() {
        // Order-1 Hilbert curve on a 2×2 grid:
        // d=0 → (0,0), d=1 → (0,1), d=2 → (1,1), d=3 → (1,0)
        assert_eq!(xy_to_hilbert(0, 0, 1), 0);
        assert_eq!(xy_to_hilbert(0, 1, 1), 1);
        assert_eq!(xy_to_hilbert(1, 1, 1), 2);
        assert_eq!(xy_to_hilbert(1, 0, 1), 3);
    }

    #[test]
    fn compute_hilbert_value_deterministic() {
        let world = Bbox2D::new(-180.0, -90.0, 180.0, 90.0).expect("valid world");
        let bbox = Bbox2D::new(0.0, 0.0, 1.0, 1.0).expect("valid bbox");
        let v1 = compute_hilbert_value(&bbox, &world, 16);
        let v2 = compute_hilbert_value(&bbox, &world, 16);
        assert_eq!(v1, v2);
    }

    #[test]
    fn compute_world_bbox_expands_degenerate() {
        // All items at the same point → world must still have positive extent.
        let items = vec![
            (Bbox2D::point(1.0, 1.0), 0u32),
            (Bbox2D::point(1.0, 1.0), 1u32),
        ];
        let world = compute_world_bbox(&items);
        assert!(world.max_x > world.min_x);
        assert!(world.max_y > world.min_y);
    }

    #[test]
    fn build_internal_nodes_groups_correctly() {
        // 17 entries → 2 internal nodes (16 + 1).
        let entries: Vec<(u64, Bbox2D, u32)> = (0..17u32)
            .map(|i| {
                let f = f64::from(i);
                (
                    u64::from(i),
                    Bbox2D::new(f, f, f + 1.0, f + 1.0).unwrap(),
                    i,
                )
            })
            .collect();
        let nodes = build_internal_nodes(&entries);
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].start, 0);
        assert_eq!(nodes[0].end, 16);
        assert_eq!(nodes[1].start, 16);
        assert_eq!(nodes[1].end, 17);
    }
}
