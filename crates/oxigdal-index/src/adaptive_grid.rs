//! Adaptive grid (loose-quadtree) spatial index.
//!
//! A [`AdaptiveGrid`] starts as a single root leaf covering a fixed world
//! bounding box.  When the number of items in a leaf exceeds
//! `max_items_per_cell` and the leaf's depth is still strictly less than
//! `max_depth`, the leaf is **subdivided** into four equal child quadrants
//! (NE, NW, SE, SW at the midpoint).
//!
//! # Loose-quadtree storage policy
//!
//! Items whose stored bbox fits entirely inside a single child quadrant migrate
//! down into that child during subdivision.  Items whose bbox straddles two or
//! more child quadrants stay attached to the parent internal node so they only
//! need to be stored (and visited) once.  This is the classic "loose quadtree"
//! pattern.  A search must therefore:
//!
//! 1. Visit the internal node's own `items` list, **and**
//! 2. Recurse into every child whose extent intersects the query.
//!
//! # When to use
//!
//! Adaptive grids shine when the data distribution is **highly non-uniform**:
//! dense regions are subdivided to keep per-cell cost low, while sparse regions
//! remain a single large leaf and incur no per-cell overhead.  For uniformly
//! distributed data, a flat [`crate::grid_index::GridIndex`] is usually faster
//! and simpler.
//!
//! # Complexity
//!
//! | Operation | Average | Worst case (`max_depth` reached) |
//! |-----------|---------|----------------------------------|
//! | `insert`  | O(log n) | O(max_depth) |
//! | `search`  | O(log n + r) where r = matching items | O(n) |
//!
//! # Example
//!
//! ```rust
//! use oxigdal_index::{AdaptiveGrid, Bbox2D};
//!
//! let world = Bbox2D::new(0.0, 0.0, 100.0, 100.0).unwrap();
//! let mut grid: AdaptiveGrid<u32> = AdaptiveGrid::new(world, 4, 6);
//!
//! grid.insert(Bbox2D::new(1.0, 1.0, 2.0, 2.0).unwrap(), 1);
//! grid.insert(Bbox2D::new(3.0, 3.0, 4.0, 4.0).unwrap(), 2);
//!
//! let hits = grid.search(Bbox2D::new(0.0, 0.0, 5.0, 5.0).unwrap());
//! assert_eq!(hits.len(), 2);
//! ```

use std::collections::HashSet;

use crate::bbox::Bbox2D;

// ---------------------------------------------------------------------------
// Node representation
// ---------------------------------------------------------------------------

/// Internal tree node.  Each node owns the bbox describing its spatial extent
/// and either a list of items (leaf) or four children plus a list of items that
/// span across multiple children (internal).
#[derive(Debug)]
enum AdaptiveNode<T> {
    /// A leaf cell.  All items belonging to this cell are stored directly here.
    Leaf {
        /// Spatial extent of this leaf.
        bbox: Bbox2D,
        /// Stored items `(item_bbox, value)` pairs.
        items: Vec<(Bbox2D, T)>,
    },
    /// An internal cell with four children at `[NE, NW, SE, SW]`.
    Internal {
        /// Spatial extent of this internal node.
        bbox: Bbox2D,
        /// Items whose bbox straddles two or more children and so cannot be
        /// pushed down to a single child without duplication.
        items: Vec<(Bbox2D, T)>,
        /// `[NE, NW, SE, SW]` — always present once subdivision has occurred.
        children: Box<[AdaptiveNode<T>; 4]>,
    },
}

impl<T> AdaptiveNode<T> {
    /// Return the bbox covering this node's spatial extent.
    #[inline]
    fn bbox(&self) -> &Bbox2D {
        match self {
            AdaptiveNode::Leaf { bbox, .. } => bbox,
            AdaptiveNode::Internal { bbox, .. } => bbox,
        }
    }
}

// ---------------------------------------------------------------------------
// Public type
// ---------------------------------------------------------------------------

/// Adaptive (loose-quadtree) grid index.
///
/// The grid lives within a fixed `world_bbox`.  Items outside the world bbox
/// can still be inserted and searched, but they will reside in the root cell
/// only (no subdivision can localise them further).
///
/// See the module documentation for the storage policy and complexity
/// characteristics.
#[derive(Debug)]
pub struct AdaptiveGrid<T> {
    root: AdaptiveNode<T>,
    max_items_per_cell: usize,
    max_depth: u8,
    len: usize,
}

// ---------------------------------------------------------------------------
// Construction and basic accessors
// ---------------------------------------------------------------------------

impl<T> AdaptiveGrid<T> {
    /// Create a new adaptive grid covering `world_bbox`.
    ///
    /// * `max_items_per_cell` — when a leaf accumulates strictly **more** than
    ///   this number of items it will attempt to subdivide.  Values below `1`
    ///   are clamped to `1` to avoid infinite recursion.
    /// * `max_depth` — hard upper bound on subdivision depth.  Once a cell's
    ///   depth equals `max_depth`, it never subdivides again, even if it
    ///   exceeds the item threshold.  A `max_depth` of `0` disables all
    ///   subdivision and keeps the grid as a single root leaf.
    pub fn new(world_bbox: Bbox2D, max_items_per_cell: usize, max_depth: u8) -> Self {
        Self {
            root: AdaptiveNode::Leaf {
                bbox: world_bbox,
                items: Vec::new(),
            },
            max_items_per_cell: max_items_per_cell.max(1),
            max_depth,
            len: 0,
        }
    }

    /// Number of items inserted into this grid.
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether no items have been inserted.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Configured maximum number of items per leaf before subdivision is
    /// attempted.
    #[inline]
    pub fn max_items_per_cell(&self) -> usize {
        self.max_items_per_cell
    }

    /// Configured maximum tree depth.
    #[inline]
    pub fn max_depth(&self) -> u8 {
        self.max_depth
    }

    /// Bounding box originally supplied at construction time.
    #[inline]
    pub fn world_bbox(&self) -> &Bbox2D {
        self.root.bbox()
    }

    /// Depth of the deepest leaf in the current tree (root counts as depth 0).
    pub fn depth(&self) -> u8 {
        Self::depth_of(&self.root, 0)
    }

    /// Number of leaf cells in the tree.
    pub fn leaf_count(&self) -> usize {
        Self::leaf_count_of(&self.root)
    }

    /// Total number of cells in the tree (leaves + internal nodes).
    pub fn cell_count(&self) -> usize {
        Self::cell_count_of(&self.root)
    }
}

// ---------------------------------------------------------------------------
// Insertion
// ---------------------------------------------------------------------------

impl<T> AdaptiveGrid<T> {
    /// Insert `value` with bounding box `bbox`.
    ///
    /// The item is placed in the smallest tree cell whose extent fully contains
    /// `bbox`.  When that cell is a leaf and would exceed
    /// `max_items_per_cell`, it is subdivided (provided depth budget remains)
    /// and items are redistributed.
    pub fn insert(&mut self, bbox: Bbox2D, value: T) {
        Self::insert_into(
            &mut self.root,
            bbox,
            value,
            0,
            self.max_items_per_cell,
            self.max_depth,
        );
        self.len += 1;
    }

    fn insert_into(
        node: &mut AdaptiveNode<T>,
        bbox: Bbox2D,
        value: T,
        depth: u8,
        max_items: usize,
        max_depth: u8,
    ) {
        match node {
            AdaptiveNode::Leaf { items, .. } => {
                items.push((bbox, value));
                if items.len() > max_items && depth < max_depth {
                    Self::subdivide(node, depth, max_items, max_depth);
                }
            }
            AdaptiveNode::Internal {
                items: parent_items,
                children,
                ..
            } => {
                // Determine whether the item fits inside exactly one child.
                let child_idx = Self::which_child_fully_contains(children, &bbox);
                if let Some(i) = child_idx {
                    Self::insert_into(
                        &mut children[i],
                        bbox,
                        value,
                        depth + 1,
                        max_items,
                        max_depth,
                    );
                } else {
                    // Spans multiple children — store at this internal node.
                    parent_items.push((bbox, value));
                }
            }
        }
    }

    /// Replace a leaf with an internal node, redistributing its items.
    ///
    /// Items whose bbox fits entirely within one child quadrant are pushed
    /// down (recursively, in case the child itself overflows).  Items that
    /// span multiple quadrants are retained on the new internal node.
    fn subdivide(node: &mut AdaptiveNode<T>, depth: u8, max_items: usize, max_depth: u8) {
        // Take the existing leaf out so we can move its items.
        let placeholder = AdaptiveNode::Leaf {
            bbox: *node.bbox(),
            items: Vec::new(),
        };
        let old = std::mem::replace(node, placeholder);

        let (bbox, items) = match old {
            AdaptiveNode::Leaf { bbox, items } => (bbox, items),
            // Subdivide is only called on a leaf in `insert_into`.  Restore
            // and return early if mis-invoked.
            other @ AdaptiveNode::Internal { .. } => {
                *node = other;
                return;
            }
        };

        let (ne_bbox, nw_bbox, se_bbox, sw_bbox) = Self::split_bbox(&bbox);
        let child_bboxes = [ne_bbox, nw_bbox, se_bbox, sw_bbox];
        let mut children: [AdaptiveNode<T>; 4] = [
            AdaptiveNode::Leaf {
                bbox: ne_bbox,
                items: Vec::new(),
            },
            AdaptiveNode::Leaf {
                bbox: nw_bbox,
                items: Vec::new(),
            },
            AdaptiveNode::Leaf {
                bbox: se_bbox,
                items: Vec::new(),
            },
            AdaptiveNode::Leaf {
                bbox: sw_bbox,
                items: Vec::new(),
            },
        ];

        let mut parent_items: Vec<(Bbox2D, T)> = Vec::new();
        for (item_bbox, value) in items {
            match Self::which_child_fully_contains_bboxes(&child_bboxes, &item_bbox) {
                Some(i) => Self::insert_into(
                    &mut children[i],
                    item_bbox,
                    value,
                    depth + 1,
                    max_items,
                    max_depth,
                ),
                None => parent_items.push((item_bbox, value)),
            }
        }

        *node = AdaptiveNode::Internal {
            bbox,
            items: parent_items,
            children: Box::new(children),
        };
    }
}

// ---------------------------------------------------------------------------
// Geometry helpers
// ---------------------------------------------------------------------------

impl<T> AdaptiveGrid<T> {
    /// Split `b` at its midpoint into four sub-rectangles `(NE, NW, SE, SW)`.
    ///
    /// Construction uses `Bbox2D::new` and unwraps with `.unwrap_or(*b)` as a
    /// defensive fallback for pathologically degenerate inputs (e.g. when
    /// `min_x == max_x` and rounding produces a non-monotone midpoint).  This
    /// keeps the function infallible without panicking.
    fn split_bbox(b: &Bbox2D) -> (Bbox2D, Bbox2D, Bbox2D, Bbox2D) {
        let mx = (b.min_x + b.max_x) * 0.5;
        let my = (b.min_y + b.max_y) * 0.5;
        let ne = Bbox2D::new(mx, my, b.max_x, b.max_y).unwrap_or(*b);
        let nw = Bbox2D::new(b.min_x, my, mx, b.max_y).unwrap_or(*b);
        let se = Bbox2D::new(mx, b.min_y, b.max_x, my).unwrap_or(*b);
        let sw = Bbox2D::new(b.min_x, b.min_y, mx, my).unwrap_or(*b);
        (ne, nw, se, sw)
    }

    /// Return the index of the child node whose bbox fully contains `item`, or
    /// `None` if no single child does (i.e. `item` straddles a boundary).
    fn which_child_fully_contains(children: &[AdaptiveNode<T>; 4], item: &Bbox2D) -> Option<usize> {
        for (i, child) in children.iter().enumerate() {
            if child.bbox().contains_bbox(item) {
                return Some(i);
            }
        }
        None
    }

    /// Same as [`Self::which_child_fully_contains`] but operates on raw bboxes
    /// (used during subdivision before the child nodes have been constructed).
    fn which_child_fully_contains_bboxes(children: &[Bbox2D; 4], item: &Bbox2D) -> Option<usize> {
        for (i, cell) in children.iter().enumerate() {
            if cell.contains_bbox(item) {
                return Some(i);
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

impl<T> AdaptiveGrid<T> {
    /// Return references to every stored value whose bbox intersects `query`.
    ///
    /// Results are deduplicated by pointer identity, so even if the recursive
    /// descent visited an item from multiple paths it would only appear once.
    /// In practice each item is stored exactly once in a loose quadtree, so the
    /// dedup acts as a defensive safety net.
    pub fn search(&self, query: Bbox2D) -> Vec<&T> {
        let mut seen: HashSet<*const T> = HashSet::new();
        let mut out: Vec<&T> = Vec::new();
        Self::search_node(&self.root, &query, &mut out, &mut seen);
        out
    }

    fn search_node<'a>(
        node: &'a AdaptiveNode<T>,
        query: &Bbox2D,
        out: &mut Vec<&'a T>,
        seen: &mut HashSet<*const T>,
    ) {
        match node {
            AdaptiveNode::Leaf { bbox, items } => {
                if !bbox.intersects(query) {
                    return;
                }
                for (item_bbox, value) in items {
                    if item_bbox.intersects(query) {
                        let ptr = value as *const T;
                        if seen.insert(ptr) {
                            out.push(value);
                        }
                    }
                }
            }
            AdaptiveNode::Internal {
                bbox,
                items,
                children,
            } => {
                if !bbox.intersects(query) {
                    return;
                }
                // Items pinned at this internal node (because they span
                // multiple children) are checked here.
                for (item_bbox, value) in items {
                    if item_bbox.intersects(query) {
                        let ptr = value as *const T;
                        if seen.insert(ptr) {
                            out.push(value);
                        }
                    }
                }
                // Recurse into every child whose extent intersects the query.
                for child in children.iter() {
                    Self::search_node(child, query, out, seen);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tree-structure introspection helpers
// ---------------------------------------------------------------------------

impl<T> AdaptiveGrid<T> {
    fn depth_of(node: &AdaptiveNode<T>, current: u8) -> u8 {
        match node {
            AdaptiveNode::Leaf { .. } => current,
            AdaptiveNode::Internal { children, .. } => children
                .iter()
                .map(|c| Self::depth_of(c, current + 1))
                .max()
                .unwrap_or(current),
        }
    }

    fn leaf_count_of(node: &AdaptiveNode<T>) -> usize {
        match node {
            AdaptiveNode::Leaf { .. } => 1,
            AdaptiveNode::Internal { children, .. } => {
                children.iter().map(Self::leaf_count_of).sum()
            }
        }
    }

    fn cell_count_of(node: &AdaptiveNode<T>) -> usize {
        match node {
            AdaptiveNode::Leaf { .. } => 1,
            AdaptiveNode::Internal { children, .. } => {
                1 + children.iter().map(Self::cell_count_of).sum::<usize>()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Inline unit tests covering the geometry helpers
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod unit_tests {
    use super::*;

    #[test]
    fn split_bbox_produces_four_equal_quadrants() {
        let b = Bbox2D::new(0.0, 0.0, 10.0, 10.0).expect("valid bbox");
        let (ne, nw, se, sw) = AdaptiveGrid::<u32>::split_bbox(&b);
        assert_eq!(ne, Bbox2D::new(5.0, 5.0, 10.0, 10.0).unwrap());
        assert_eq!(nw, Bbox2D::new(0.0, 5.0, 5.0, 10.0).unwrap());
        assert_eq!(se, Bbox2D::new(5.0, 0.0, 10.0, 5.0).unwrap());
        assert_eq!(sw, Bbox2D::new(0.0, 0.0, 5.0, 5.0).unwrap());
    }

    #[test]
    fn empty_grid_has_one_leaf_zero_depth() {
        let world = Bbox2D::new(0.0, 0.0, 10.0, 10.0).expect("valid world");
        let grid: AdaptiveGrid<u32> = AdaptiveGrid::new(world, 4, 4);
        assert_eq!(grid.leaf_count(), 1);
        assert_eq!(grid.depth(), 0);
        assert_eq!(grid.cell_count(), 1);
        assert!(grid.is_empty());
    }

    #[test]
    fn max_items_per_cell_clamped_to_one() {
        let world = Bbox2D::new(0.0, 0.0, 10.0, 10.0).expect("valid world");
        let grid: AdaptiveGrid<u32> = AdaptiveGrid::new(world, 0, 4);
        assert_eq!(grid.max_items_per_cell(), 1);
    }
}
