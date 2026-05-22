//! Dynamic spatial hash grid index.
//!
//! Unlike [`crate::grid_index::GridIndex`], `SpatialHashGrid` does not require
//! a pre-declared extent or fixed grid dimensions.  The plane is divided into
//! uniform `cell_size × cell_size` cells addressed by `(i64, i64)` integer
//! keys derived via Euclidean (`div_euclid`) division, so negative coordinates
//! are handled correctly.
//!
//! # Complexity
//!
//! | Operation | Cost |
//! |-----------|------|
//! | `insert`  | O(k) where k = number of cells the bbox overlaps |
//! | `search`  | O(k + r) where r = matching items |
//! | `remove`  | O(1) — lazy tombstone |
//! | `compact` | O(n + c) where n = items, c = live cell entries |
//!
//! # Example
//!
//! ```rust
//! use oxigdal_index::{Bbox2D, SpatialHashGrid};
//!
//! let mut grid: SpatialHashGrid<&str> = SpatialHashGrid::new(1.0);
//! let bbox = Bbox2D::new(0.5, 0.5, 2.5, 2.5).unwrap();
//! let idx = grid.insert(bbox, "city polygon");
//!
//! let query = Bbox2D::new(1.0, 1.0, 3.0, 3.0).unwrap();
//! let hits = grid.search(&query);
//! assert_eq!(hits.len(), 1);
//! assert_eq!(*hits[0], "city polygon");
//!
//! grid.remove(idx);
//! assert!(grid.search(&query).is_empty());
//! ```

use std::collections::{HashMap, HashSet};

use crate::bbox::Bbox2D;

/// Arena slot: `(bounding_box, user_value, alive)`.
type ArenaSlot<T> = (Bbox2D, T, bool);

/// Dynamic spatial hash grid — no pre-declared extent required.
///
/// Divides the plane into a regular grid of `cell_size × cell_size` cells.
/// Each cell is addressed by `(i64, i64)` derived via Euclidean floor division
/// of coordinates by `cell_size`, so the grid handles negative coordinates
/// without special cases.
///
/// An object with a bounding box is stored in **all** cells its bbox overlaps.
/// Removal is lazy (tombstone flag in the arena); call [`compact`](SpatialHashGrid::compact)
/// periodically to reclaim memory.
///
/// Good for uniform-density datasets.  Query cost is O(k) where k is the
/// number of candidate items in the touched cells.
#[derive(Debug)]
pub struct SpatialHashGrid<T> {
    /// Side length of each cell in coordinate units.
    cell_size: f64,
    /// Map from `(col, row)` to a list of arena indices.
    ///
    /// Cells are created on demand; no memory is allocated for empty areas.
    cells: HashMap<(i64, i64), Vec<usize>>,
    /// Flat arena that stores every inserted item alongside its bbox and
    /// liveness flag.  Indices are stable until [`compact`](SpatialHashGrid::compact) is called.
    arena: Vec<ArenaSlot<T>>,
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

impl<T> SpatialHashGrid<T> {
    /// Create a new grid with the given `cell_size` (in coordinate units).
    ///
    /// A larger cell size reduces the total number of cells but increases the
    /// number of false positives per query.  A smaller cell size gives finer
    /// filtering at the cost of more cell overhead for large items.
    ///
    /// # Panics
    ///
    /// Does not panic, but a `cell_size` of `0.0` or negative would make every
    /// coordinate map to the same cell.  Use a positive, finite value.
    #[inline]
    pub fn new(cell_size: f64) -> Self {
        Self {
            cell_size,
            cells: HashMap::new(),
            arena: Vec::new(),
        }
    }

    /// Create a grid pre-allocated for `capacity` items.
    #[inline]
    pub fn with_capacity(cell_size: f64, capacity: usize) -> Self {
        Self {
            cell_size,
            cells: HashMap::new(),
            arena: Vec::with_capacity(capacity),
        }
    }
}

// ---------------------------------------------------------------------------
// Cell-address helpers
// ---------------------------------------------------------------------------

impl<T> SpatialHashGrid<T> {
    /// Returns the grid cell `(col, row)` that the point `(x, y)` falls into.
    ///
    /// Uses Euclidean division (`div_euclid`) so that negative coordinates are
    /// handled symmetrically: e.g. with `cell_size = 10.0` the point `(-1, -1)`
    /// maps to cell `(-1, -1)` rather than `(0, 0)`.
    #[inline]
    fn cell_for_point(cell_size: f64, x: f64, y: f64) -> (i64, i64) {
        let col = (x / cell_size).floor() as i64;
        let row = (y / cell_size).floor() as i64;
        // `div_euclid` semantics: ensure consistent negative-coordinate handling.
        // For `x = -0.5, cell_size = 1.0`:
        //   `(x / cell_size).floor()` == -1  ✓  (same as div_euclid)
        // The floor of the true quotient gives the correct Euclidean cell.
        (col, row)
    }

    /// Returns the range of cells `[col_min..=col_max] × [row_min..=row_max]`
    /// that a bounding box overlaps.
    ///
    /// Yields `(col, row)` pairs via a flat iterator.  The number of cells
    /// produced is `(col_max - col_min + 1) * (row_max - row_min + 1)`.
    fn cells_for_bbox(cell_size: f64, bbox: &Bbox2D) -> impl Iterator<Item = (i64, i64)> {
        let (col_min, row_min) = Self::cell_for_point(cell_size, bbox.min_x, bbox.min_y);
        let (col_max, row_max) = Self::cell_for_point(cell_size, bbox.max_x, bbox.max_y);
        // Degenerate bboxes (point, or width/height == 0) still produce at
        // least one cell because col_min == col_max and row_min == row_max.
        (row_min..=row_max).flat_map(move |row| (col_min..=col_max).map(move |col| (col, row)))
    }
}

// ---------------------------------------------------------------------------
// Core operations
// ---------------------------------------------------------------------------

impl<T> SpatialHashGrid<T> {
    /// Insert an item with the given bounding box.
    ///
    /// The item is registered in every cell that its bbox overlaps.  Returns
    /// the **arena index** — a stable handle that can be passed to
    /// [`remove`](SpatialHashGrid::remove) to delete the item later.
    ///
    /// The arena index remains valid until the next call to
    /// [`compact`](SpatialHashGrid::compact).
    pub fn insert(&mut self, bbox: Bbox2D, value: T) -> usize {
        let idx = self.arena.len();
        self.arena.push((bbox, value, true));
        let cell_size = self.cell_size;
        // Borrow arena slot to read the stored bbox (avoids double-borrow).
        let stored_bbox = &self.arena[idx].0;
        for cell in Self::cells_for_bbox(cell_size, stored_bbox) {
            self.cells.entry(cell).or_default().push(idx);
        }
        idx
    }

    /// Search for all **live** items whose bbox intersects the query bbox.
    ///
    /// Results are **deduplicated** — an item that spans multiple cells is
    /// returned at most once.  Only items whose stored bbox actually intersects
    /// `query` are included (the cell lookup is a coarse filter; we verify
    /// each candidate).
    pub fn search(&self, query: &Bbox2D) -> Vec<&T> {
        let mut seen: HashSet<usize> = HashSet::new();
        let mut result: Vec<&T> = Vec::new();
        for cell in Self::cells_for_bbox(self.cell_size, query) {
            if let Some(indices) = self.cells.get(&cell) {
                for &idx in indices {
                    let (item_bbox, value, alive) = &self.arena[idx];
                    if *alive && seen.insert(idx) && item_bbox.intersects(query) {
                        result.push(value);
                    }
                }
            }
        }
        result
    }

    /// Search and return `(arena_index, &value)` pairs for all matching items.
    ///
    /// Behaves identically to [`search`](SpatialHashGrid::search) but also
    /// exposes the arena index so callers can later call
    /// [`remove`](SpatialHashGrid::remove) on specific results.
    pub fn search_with_index(&self, query: &Bbox2D) -> Vec<(usize, &T)> {
        let mut seen: HashSet<usize> = HashSet::new();
        let mut result: Vec<(usize, &T)> = Vec::new();
        for cell in Self::cells_for_bbox(self.cell_size, query) {
            if let Some(indices) = self.cells.get(&cell) {
                for &idx in indices {
                    let (item_bbox, value, alive) = &self.arena[idx];
                    if *alive && seen.insert(idx) && item_bbox.intersects(query) {
                        result.push((idx, value));
                    }
                }
            }
        }
        result
    }

    /// Search and return `(arena_index, &Bbox2D, &value)` triples.
    ///
    /// Useful when the caller also needs the stored bounding box of each result.
    pub fn search_with_bbox(&self, query: &Bbox2D) -> Vec<(usize, &Bbox2D, &T)> {
        let mut seen: HashSet<usize> = HashSet::new();
        let mut result: Vec<(usize, &Bbox2D, &T)> = Vec::new();
        for cell in Self::cells_for_bbox(self.cell_size, query) {
            if let Some(indices) = self.cells.get(&cell) {
                for &idx in indices {
                    let (item_bbox, value, alive) = &self.arena[idx];
                    if *alive && seen.insert(idx) && item_bbox.intersects(query) {
                        result.push((idx, item_bbox, value));
                    }
                }
            }
        }
        result
    }

    /// Mark the item at `arena_index` as dead.
    ///
    /// After removal the item will not appear in any future search results.
    /// The underlying memory is not freed until [`compact`](SpatialHashGrid::compact)
    /// is called.
    ///
    /// Returns `true` if the item existed and was live, `false` if the index is
    /// out of range or the item was already removed.
    pub fn remove(&mut self, arena_index: usize) -> bool {
        if arena_index >= self.arena.len() {
            return false;
        }
        let alive = &mut self.arena[arena_index].2;
        if !*alive {
            return false;
        }
        *alive = false;
        true
    }

    /// Retrieve a reference to the value stored at `arena_index`, if it is live.
    ///
    /// Returns `None` when the index is out of range or the item has been
    /// removed.
    #[inline]
    pub fn get(&self, arena_index: usize) -> Option<&T> {
        let (_, value, alive) = self.arena.get(arena_index)?;
        if *alive { Some(value) } else { None }
    }

    /// Retrieve a mutable reference to the value stored at `arena_index`, if it is live.
    #[inline]
    pub fn get_mut(&mut self, arena_index: usize) -> Option<&mut T> {
        let slot = self.arena.get_mut(arena_index)?;
        if slot.2 { Some(&mut slot.1) } else { None }
    }

    /// Retrieve a reference to the bbox stored at `arena_index`, if it is live.
    #[inline]
    pub fn get_bbox(&self, arena_index: usize) -> Option<&Bbox2D> {
        let (bbox, _, alive) = self.arena.get(arena_index)?;
        if *alive { Some(bbox) } else { None }
    }
}

// ---------------------------------------------------------------------------
// Compaction
// ---------------------------------------------------------------------------

impl<T> SpatialHashGrid<T> {
    /// Remove dead arena slots and rebuild the cell map.
    ///
    /// After compaction, previously returned arena indices are **invalidated**.
    /// Call this periodically when many items have been removed and memory
    /// pressure is a concern.
    ///
    /// Runs in O(n + c) where n = live items, c = live cell entries.
    pub fn compact(&mut self) {
        // Collect live items; record new arena positions.
        let old_arena = std::mem::take(&mut self.arena);
        let mut new_arena: Vec<ArenaSlot<T>> =
            Vec::with_capacity(old_arena.iter().filter(|s| s.2).count());

        // Build a mapping from old index → new index for live items.
        let mut old_to_new: HashMap<usize, usize> = HashMap::new();
        for (old_idx, slot) in old_arena.into_iter().enumerate() {
            if slot.2 {
                let new_idx = new_arena.len();
                old_to_new.insert(old_idx, new_idx);
                new_arena.push(slot);
            }
        }

        // Rebuild the cell map: remap indices, drop entries for dead items,
        // and drop cells that become empty.
        let cell_size = self.cell_size;
        let mut new_cells: HashMap<(i64, i64), Vec<usize>> =
            HashMap::with_capacity(new_arena.len());
        for (bbox, _, _) in &new_arena {
            for cell in Self::cells_for_bbox(cell_size, bbox) {
                // The new_arena is already built; we need the new index of each item.
                // Rebuild from scratch via a second pass.
                let _ = cell; // suppress unused warning — rebuilt below
            }
        }

        // Actually rebuild by iterating live items in new_arena order.
        new_cells.clear();
        for (new_idx, (bbox, _, _)) in new_arena.iter().enumerate() {
            for cell in Self::cells_for_bbox(cell_size, bbox) {
                new_cells.entry(cell).or_default().push(new_idx);
            }
        }

        // Suppress the unused variable; old_to_new is available for callers
        // who extend this struct, but we rebuilt from new_arena directly.
        let _ = old_to_new;

        self.arena = new_arena;
        self.cells = new_cells;
    }
}

// ---------------------------------------------------------------------------
// Accessors and iteration
// ---------------------------------------------------------------------------

impl<T> SpatialHashGrid<T> {
    /// Number of **live** items currently stored.
    ///
    /// O(n) scan over the arena.
    #[inline]
    pub fn len(&self) -> usize {
        self.arena.iter().filter(|(.., alive)| *alive).count()
    }

    /// Whether no live items are stored.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The cell size this grid was created with.
    #[inline]
    pub fn cell_size(&self) -> f64 {
        self.cell_size
    }

    /// Total number of `(cell → arena_index)` entries, including duplicates
    /// for items that span multiple cells.
    ///
    /// Includes dead (tombstoned) items until the next [`compact`](SpatialHashGrid::compact).
    pub fn cell_entry_count(&self) -> usize {
        self.cells.values().map(Vec::len).sum()
    }

    /// Number of distinct non-empty cells currently in the hash map.
    pub fn occupied_cell_count(&self) -> usize {
        self.cells.len()
    }

    /// Total capacity of the arena (live + dead slots).
    pub fn arena_capacity(&self) -> usize {
        self.arena.len()
    }

    /// Iterate over all live `(&Bbox2D, &T)` pairs.
    ///
    /// Order is the insertion order of live items.
    pub fn iter(&self) -> impl Iterator<Item = (&Bbox2D, &T)> {
        self.arena
            .iter()
            .filter(|(.., alive)| *alive)
            .map(|(bbox, v, _)| (bbox, v))
    }

    /// Iterate over all live `(arena_index, &Bbox2D, &T)` triples.
    ///
    /// The `arena_index` is stable until the next [`compact`](SpatialHashGrid::compact).
    pub fn iter_with_index(&self) -> impl Iterator<Item = (usize, &Bbox2D, &T)> {
        self.arena
            .iter()
            .enumerate()
            .filter(|(_, (.., alive))| *alive)
            .map(|(idx, (bbox, v, _))| (idx, bbox, v))
    }

    /// Returns a bounding box that covers all live items, or `None` if empty.
    pub fn extent(&self) -> Option<Bbox2D> {
        let mut iter = self.iter();
        let (first_bbox, _) = iter.next()?;
        let initial = *first_bbox;
        Some(iter.fold(initial, |acc, (bbox, _)| acc.union(bbox)))
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn cell_for_origin() {
        assert_eq!(
            SpatialHashGrid::<()>::cell_for_point(10.0, 0.0, 0.0),
            (0, 0)
        );
    }

    #[test]
    fn cell_for_positive_coords() {
        // x=15, y=25 with cell_size=10 → col=1, row=2
        assert_eq!(
            SpatialHashGrid::<()>::cell_for_point(10.0, 15.0, 25.0),
            (1, 2)
        );
    }

    #[test]
    fn cell_for_negative_coords() {
        // x=-1, y=-1 with cell_size=10 → col=-1, row=-1
        assert_eq!(
            SpatialHashGrid::<()>::cell_for_point(10.0, -1.0, -1.0),
            (-1, -1)
        );
    }

    #[test]
    fn cell_for_negative_coords_boundary() {
        // x=-10 (exactly on boundary) → col=-1
        assert_eq!(
            SpatialHashGrid::<()>::cell_for_point(10.0, -10.0, -10.0),
            (-1, -1)
        );
    }

    #[test]
    fn cells_for_bbox_single_cell() {
        // A tiny bbox entirely within one cell.
        let bbox = Bbox2D::new(0.1, 0.1, 0.9, 0.9).unwrap();
        let cells: Vec<_> = SpatialHashGrid::<()>::cells_for_bbox(1.0, &bbox).collect();
        assert_eq!(cells, vec![(0, 0)]);
    }

    #[test]
    fn cells_for_bbox_four_cells() {
        // A bbox straddling 4 cells with cell_size=1.
        let bbox = Bbox2D::new(0.5, 0.5, 1.5, 1.5).unwrap();
        let cells: Vec<_> = SpatialHashGrid::<()>::cells_for_bbox(1.0, &bbox).collect();
        assert_eq!(cells.len(), 4);
        assert!(cells.contains(&(0, 0)));
        assert!(cells.contains(&(1, 0)));
        assert!(cells.contains(&(0, 1)));
        assert!(cells.contains(&(1, 1)));
    }

    #[test]
    fn insert_increases_arena_len() {
        let mut g: SpatialHashGrid<u32> = SpatialHashGrid::new(1.0);
        let b = Bbox2D::new(0.0, 0.0, 1.0, 1.0).unwrap();
        g.insert(b, 42);
        assert_eq!(g.arena_capacity(), 1);
    }

    #[test]
    fn get_returns_value_for_live_item() {
        let mut g: SpatialHashGrid<u32> = SpatialHashGrid::new(1.0);
        let b = Bbox2D::new(0.0, 0.0, 1.0, 1.0).unwrap();
        let idx = g.insert(b, 99);
        assert_eq!(g.get(idx), Some(&99));
    }

    #[test]
    fn get_returns_none_after_remove() {
        let mut g: SpatialHashGrid<u32> = SpatialHashGrid::new(1.0);
        let b = Bbox2D::new(0.0, 0.0, 1.0, 1.0).unwrap();
        let idx = g.insert(b, 99);
        g.remove(idx);
        assert_eq!(g.get(idx), None);
    }
}
