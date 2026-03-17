//! Regular-grid spatial index for uniformly distributed data.
//!
//! The grid covers a fixed extent divided into `cols × rows` cells.  Each
//! entry is placed into every cell whose extent intersects the entry's bbox,
//! so searches are O(k) in the number of cells touching the query window.

use crate::bbox::Bbox2D;
use crate::error::IndexError;

/// A regular-grid spatial index.
///
/// Trades insertion speed and memory for faster retrieval when data are
/// roughly uniformly distributed.
pub struct GridIndex<T> {
    /// Flat array of cells, row-major (row * cols + col).
    cells: Vec<Vec<(Bbox2D, T)>>,
    grid_min_x: f64,
    grid_min_y: f64,
    cell_size_x: f64,
    cell_size_y: f64,
    cols: usize,
    rows: usize,
    size: usize,
}

impl<T: Clone> GridIndex<T> {
    /// Create a new grid covering `extent`, divided into `cols × rows` cells.
    ///
    /// Returns an error when `cols == 0` or `rows == 0`.
    pub fn new(extent: Bbox2D, cols: usize, rows: usize) -> Result<Self, IndexError> {
        if cols == 0 || rows == 0 {
            return Err(IndexError::InvalidGridSize(cols, rows));
        }
        let cell_size_x = extent.width() / cols as f64;
        let cell_size_y = extent.height() / rows as f64;
        // Guard against degenerate extents (zero width/height).
        let cell_size_x = if cell_size_x == 0.0 { 1.0 } else { cell_size_x };
        let cell_size_y = if cell_size_y == 0.0 { 1.0 } else { cell_size_y };
        Ok(Self {
            cells: vec![Vec::new(); cols * rows],
            grid_min_x: extent.min_x,
            grid_min_y: extent.min_y,
            cell_size_x,
            cell_size_y,
            cols,
            rows,
            size: 0,
        })
    }

    /// Insert `value` associated with `bbox`.
    ///
    /// The value is stored once per grid cell that overlaps the bbox.
    pub fn insert(&mut self, bbox: Bbox2D, value: T) {
        for (col, row) in self.cell_indices(&bbox) {
            let idx = row * self.cols + col;
            self.cells[idx].push((bbox, value.clone()));
        }
        self.size += 1;
    }

    /// Find all values whose bbox intersects `query`.
    ///
    /// Results may contain duplicates if an entry spans multiple cells.
    /// Callers should deduplicate if uniqueness is required.
    pub fn search(&self, query: &Bbox2D) -> Vec<&T> {
        let mut results: Vec<&T> = Vec::new();
        for (col, row) in self.cell_indices(query) {
            let idx = row * self.cols + col;
            for (entry_bbox, value) in &self.cells[idx] {
                if entry_bbox.intersects(query) {
                    results.push(value);
                }
            }
        }
        results
    }

    /// Find all values whose bbox contains the point `(x, y)`.
    pub fn contains_point(&self, x: f64, y: f64) -> Vec<&T> {
        let pt = Bbox2D::point(x, y);
        // Only query the single cell the point falls into.
        let col = self.clamp_col(((x - self.grid_min_x) / self.cell_size_x).floor() as isize);
        let row = self.clamp_row(((y - self.grid_min_y) / self.cell_size_y).floor() as isize);
        let idx = row * self.cols + col;
        self.cells[idx]
            .iter()
            .filter(|(bbox, _)| bbox.intersects(&pt))
            .map(|(_, v)| v)
            .collect()
    }

    /// Total number of distinct insertions (not counting cell duplicates).
    #[inline]
    pub fn len(&self) -> usize {
        self.size
    }

    /// Whether no entries have been inserted.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    // ------------------------------------------------------------------
    // Private helpers
    // ------------------------------------------------------------------

    /// Return the list of `(col, row)` pairs for all cells intersecting `bbox`.
    fn cell_indices(&self, bbox: &Bbox2D) -> Vec<(usize, usize)> {
        let col_min = ((bbox.min_x - self.grid_min_x) / self.cell_size_x).floor() as isize;
        let col_max = ((bbox.max_x - self.grid_min_x) / self.cell_size_x).floor() as isize;
        let row_min = ((bbox.min_y - self.grid_min_y) / self.cell_size_y).floor() as isize;
        let row_max = ((bbox.max_y - self.grid_min_y) / self.cell_size_y).floor() as isize;

        let col_min = self.clamp_col(col_min);
        let col_max = self.clamp_col(col_max);
        let row_min = self.clamp_row(row_min);
        let row_max = self.clamp_row(row_max);

        let mut indices = Vec::new();
        for row in row_min..=row_max {
            for col in col_min..=col_max {
                indices.push((col, row));
            }
        }
        indices
    }

    #[inline]
    fn clamp_col(&self, col: isize) -> usize {
        col.max(0).min(self.cols as isize - 1) as usize
    }

    #[inline]
    fn clamp_row(&self, row: isize) -> usize {
        row.max(0).min(self.rows as isize - 1) as usize
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn grid_zero_cols_errors() {
        let extent = Bbox2D::new(0.0, 0.0, 10.0, 10.0).expect("valid 10x10 bbox");
        assert!(GridIndex::<u32>::new(extent, 0, 5).is_err());
    }

    #[test]
    fn grid_zero_rows_errors() {
        let extent = Bbox2D::new(0.0, 0.0, 10.0, 10.0).expect("valid 10x10 bbox");
        assert!(GridIndex::<u32>::new(extent, 5, 0).is_err());
    }
}
