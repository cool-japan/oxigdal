//! Cost-distance and least-cost-path analysis.
//!
//! Implements Dijkstra-based cost-distance propagation over a friction (cost)
//! surface. Three public functions are provided:
//!
//! - [`cost_distance`]: cumulative cost-distance from source cells.
//! - [`least_cost_path`]: optimal path from any source to a destination cell.
//! - [`cost_allocation`]: assigns each cell to its nearest source by cost.

use crate::error::{Result, TerrainError};
use num_traits::Float;
use ordered_float::NotNan;
use scirs2_core::prelude::*;
use std::cmp::Reverse;
use std::collections::BinaryHeap;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// 8-connected neighbour offsets: (row_delta, col_delta).
/// Cardinals first, then diagonals — order does not affect correctness.
const NEIGHBORS: [(isize, isize); 8] = [
    (-1, 0),
    (0, 1),
    (1, 0),
    (0, -1),
    (-1, 1),
    (1, 1),
    (1, -1),
    (-1, -1),
];

/// Whether a given neighbour offset is diagonal (Chebyshev distance > 1 along one axis).
#[inline]
fn is_diagonal(dr: isize, dc: isize) -> bool {
    dr != 0 && dc != 0
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Returns `true` when `value` should be treated as nodata.
///
/// A value is nodata when:
/// - it is NaN, **or**
/// - `nodata` is `Some(nd)` and `|value - nd| < ε` (for finite `nd`).
#[inline]
fn is_nodata<T: Float>(value: T, nodata: Option<T>) -> bool {
    if value.is_nan() {
        return true;
    }
    match nodata {
        Some(nd) if nd.is_finite() => (value - nd).abs() < T::epsilon(),
        _ => false,
    }
}

/// Convert (row, col) to a flat index.
#[inline]
fn flat(row: usize, col: usize, cols: usize) -> usize {
    row * cols + col
}

/// Return `(rows, cols)` from `Array2::dim()`, and error when either is 0.
fn checked_dims<T>(cost: &Array2<T>) -> Result<(usize, usize)> {
    let (rows, cols) = cost.dim();
    if rows == 0 || cols == 0 {
        return Err(TerrainError::InvalidDimensions {
            width: cols,
            height: rows,
        });
    }
    Ok((rows, cols))
}

/// Validate that `cell_size` is strictly positive and finite.
fn check_cell_size(cell_size: f64) -> Result<()> {
    if cell_size <= 0.0 || !cell_size.is_finite() {
        return Err(TerrainError::InvalidCellSize { size: cell_size });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Core Dijkstra kernel
// ---------------------------------------------------------------------------

/// Internal Dijkstra state returned after a full run.
struct DijkstraResult {
    /// Minimum cost to reach each cell (flat index).
    dist: Vec<f64>,
    /// Back-pointer: flat index of the cell we came from (usize::MAX = no predecessor).
    backlink: Vec<usize>,
    /// Index (into the caller-supplied `sources` slice) of the nearest source.
    source_idx: Vec<usize>,
}

/// Run Dijkstra over `cost` starting from `sources`.
///
/// `nodata_f64` marks impassable cells (skipped as neighbours and never relaxed).
fn dijkstra<T>(
    cost: &Array2<T>,
    rows: usize,
    cols: usize,
    sources: &[(usize, usize)],
    cell_size: f64,
    nodata: Option<T>,
) -> Result<DijkstraResult>
where
    T: Float + Into<f64> + Copy,
{
    let n = rows * cols;
    let sqrt2 = std::f64::consts::SQRT_2;

    let mut dist = vec![f64::INFINITY; n];
    let mut backlink = vec![usize::MAX; n];
    let mut source_idx = vec![usize::MAX; n];

    // Min-heap: Reverse(NotNan<f64>) gives smallest-first ordering.
    // Heap element: (Reverse(accumulated_cost), flat_index)
    let mut heap: BinaryHeap<(Reverse<NotNan<f64>>, usize)> = BinaryHeap::new();

    // Seed with source cells.
    for (si, &(sr, sc)) in sources.iter().enumerate() {
        // Guard: source must be in-bounds and not nodata.
        if sr >= rows || sc >= cols {
            continue;
        }
        let src_val = cost[[sr, sc]];
        if is_nodata(src_val, nodata) {
            continue;
        }
        let idx = flat(sr, sc, cols);
        if dist[idx] > 0.0 {
            dist[idx] = 0.0;
            backlink[idx] = idx; // self-loop marks a source
            source_idx[idx] = si;
            let key = NotNan::new(0.0).map_err(|_| TerrainError::ComputationError {
                message: "NaN in source cell cost during cost-distance init".to_owned(),
            })?;
            heap.push((Reverse(key), idx));
        }
    }

    while let Some((Reverse(d_nn), idx)) = heap.pop() {
        let d = *d_nn;
        // Stale entry check.
        if d > dist[idx] {
            continue;
        }

        let row = idx / cols;
        let col = idx % cols;
        let cur_cost: f64 = cost[[row, col]].into();

        for (dr, dc) in NEIGHBORS {
            let nr = row as isize + dr;
            let nc = col as isize + dc;
            if nr < 0 || nr >= rows as isize || nc < 0 || nc >= cols as isize {
                continue;
            }
            let nr = nr as usize;
            let nc = nc as usize;
            let nidx = flat(nr, nc, cols);

            let neighbor_val = cost[[nr, nc]];
            if is_nodata(neighbor_val, nodata) {
                continue;
            }
            let neighbor_cost: f64 = neighbor_val.into();

            // Edge cost = mean friction × Euclidean distance to neighbour.
            let mean_friction = (cur_cost + neighbor_cost) * 0.5;
            let dist_factor = if is_diagonal(dr, dc) {
                cell_size * sqrt2
            } else {
                cell_size
            };
            let edge_cost = mean_friction * dist_factor;

            let new_dist = d + edge_cost;
            if new_dist < dist[nidx] {
                dist[nidx] = new_dist;
                backlink[nidx] = idx;
                source_idx[nidx] = source_idx[idx];

                let key = NotNan::new(new_dist).map_err(|_| TerrainError::ComputationError {
                    message: format!(
                        "NaN cost produced at cell ({nr},{nc}) during Dijkstra relaxation"
                    ),
                })?;
                heap.push((Reverse(key), nidx));
            }
        }
    }

    Ok(DijkstraResult {
        dist,
        backlink,
        source_idx,
    })
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Compute cumulative cost-distance from source cells over a friction surface.
///
/// Each cell in the returned `Array2<f64>` holds the minimum accumulated cost
/// to reach it from *any* source cell.  The cost of traversing an edge between
/// two adjacent cells is:
///
/// ```text
/// edge_cost = mean(friction_current, friction_neighbour) × travel_distance
/// ```
///
/// where `travel_distance` is `cell_size` for cardinal neighbours and
/// `cell_size × √2` for diagonal neighbours.
///
/// Cells whose friction value matches `nodata` (or is NaN) are impassable;
/// they are never updated and never used as intermediaries.
///
/// # Arguments
/// * `cost`      — 2-D friction surface (row-major, `[row][col]`).
/// * `sources`   — slice of `(row, col)` seed cells with accumulated cost 0.
/// * `cell_size` — physical size of one cell (must be > 0).
/// * `nodata`    — optional sentinel value marking impassable cells.
///
/// # Errors
/// - [`TerrainError::InvalidDimensions`] if the array is empty.
/// - [`TerrainError::InvalidCellSize`]   if `cell_size <= 0` or non-finite.
/// - [`TerrainError::ComputationError`]  if a NaN friction value is encountered
///   during relaxation (well-formed input should never trigger this).
pub fn cost_distance<T>(
    cost: &Array2<T>,
    sources: &[(usize, usize)],
    cell_size: f64,
    nodata: Option<T>,
) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy,
{
    let (rows, cols) = checked_dims(cost)?;
    check_cell_size(cell_size)?;

    let result = dijkstra(cost, rows, cols, sources, cell_size, nodata)?;

    Array2::from_shape_vec((rows, cols), result.dist).map_err(|_| TerrainError::ComputationError {
        message: "cost_distance: failed to reshape output array".to_owned(),
    })
}

/// Compute the least-cost path from any source cell to `dest`.
///
/// Runs `cost_distance` internally and then traces back through the backlink
/// array from `dest` to the nearest source, returning the path as a
/// `Vec<(row, col)>` ordered from source → dest.
///
/// # Errors
/// - [`TerrainError::NoPath`] if `dest` is unreachable from all sources.
/// - Same dimension / cell-size errors as [`cost_distance`].
pub fn least_cost_path<T>(
    cost: &Array2<T>,
    sources: &[(usize, usize)],
    dest: (usize, usize),
    cell_size: f64,
    nodata: Option<T>,
) -> Result<Vec<(usize, usize)>>
where
    T: Float + Into<f64> + Copy,
{
    let (rows, cols) = checked_dims(cost)?;
    check_cell_size(cell_size)?;

    let result = dijkstra(cost, rows, cols, sources, cell_size, nodata)?;

    let dest_idx = flat(dest.0, dest.1, cols);
    if result.dist[dest_idx].is_infinite() {
        return Err(TerrainError::NoPath {
            message: format!(
                "destination ({}, {}) is unreachable from all {} source(s)",
                dest.0,
                dest.1,
                sources.len()
            ),
        });
    }

    // Trace back from dest to the source via backlinks.
    let mut path_rev: Vec<(usize, usize)> = Vec::new();
    let mut current = dest_idx;
    loop {
        path_rev.push((current / cols, current % cols));
        let prev = result.backlink[current];
        // A source cell's backlink points to itself.
        if prev == current || prev == usize::MAX {
            break;
        }
        current = prev;
    }

    path_rev.reverse();
    Ok(path_rev)
}

/// For each cell, record the 0-based index of the nearest source by cost-distance.
///
/// Returns an `Array2<usize>` where `output[[r, c]]` is the index of the source
/// in `sources` that can reach `(r, c)` with minimum accumulated cost.
/// Unreachable cells (cost = ∞, or nodata) are marked with [`usize::MAX`].
///
/// # Errors
/// Same as [`cost_distance`].
pub fn cost_allocation<T>(
    cost: &Array2<T>,
    sources: &[(usize, usize)],
    cell_size: f64,
    nodata: Option<T>,
) -> Result<Array2<usize>>
where
    T: Float + Into<f64> + Copy,
{
    let (rows, cols) = checked_dims(cost)?;
    check_cell_size(cell_size)?;

    let result = dijkstra(cost, rows, cols, sources, cell_size, nodata)?;

    Array2::from_shape_vec((rows, cols), result.source_idx).map_err(|_| {
        TerrainError::ComputationError {
            message: "cost_allocation: failed to reshape output array".to_owned(),
        }
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a uniform `Array2<f64>` of given shape filled with `val`.
    fn uniform(rows: usize, cols: usize, val: f64) -> Array2<f64> {
        Array2::from_elem((rows, cols), val)
    }

    // ------------------------------------------------------------------
    // 1. Single source, uniform cost — check corner distances
    // ------------------------------------------------------------------
    #[test]
    fn test_uniform_cost_single_source() {
        // 5×5 grid, all friction = 1.0, source at center (2,2), cell_size=1.0.
        // Shortest path to (0,0): 2 diagonal steps → cost = 2 × 1.0 × √2 × 1.0 = 2√2.
        let grid = uniform(5, 5, 1.0);
        let sources = vec![(2, 2)];
        let dist = cost_distance(&grid, &sources, 1.0, None).expect("cost_distance failed");

        // Center must be 0.
        assert!(
            dist[[2, 2]].abs() < 1e-9,
            "source cell cost should be 0, got {}",
            dist[[2, 2]]
        );

        // Corner (0,0) is 2 diagonal moves from (2,2): expected ≈ 2√2
        let expected_corner = 2.0 * std::f64::consts::SQRT_2;
        assert!(
            (dist[[0, 0]] - expected_corner).abs() < 1e-6,
            "corner (0,0) expected ≈{expected_corner:.6}, got {:.6}",
            dist[[0, 0]]
        );
        // All four corners should be symmetric.
        for &(r, c) in &[(0usize, 0usize), (0, 4), (4, 0), (4, 4)] {
            assert!(
                (dist[[r, c]] - expected_corner).abs() < 1e-6,
                "corner ({r},{c}) expected ≈{expected_corner:.6}, got {:.6}",
                dist[[r, c]]
            );
        }
    }

    // ------------------------------------------------------------------
    // 2. Multiple sources — each cell should get the distance to nearer source
    // ------------------------------------------------------------------
    #[test]
    fn test_uniform_cost_multiple_sources() {
        // 1×9 row vector, friction=1.0, sources at col 0 and col 8.
        // Cell at col 4 (center) should get distance 4.0 (4 cardinal steps from either source).
        let grid = uniform(1, 9, 1.0);
        let sources = vec![(0, 0), (0, 8)];
        let dist = cost_distance(&grid, &sources, 1.0, None).expect("cost_distance failed");

        assert!(dist[[0, 0]].abs() < 1e-9, "source col 0 should be 0");
        assert!(dist[[0, 8]].abs() < 1e-9, "source col 8 should be 0");

        // Middle cell equidistant from both sources: min dist = 4.0
        assert!(
            (dist[[0, 4]] - 4.0).abs() < 1e-6,
            "center col 4 expected 4.0, got {}",
            dist[[0, 4]]
        );
        // Left half closer to source 0; right half closer to source 8.
        for c in 0..4 {
            assert!(
                dist[[0, c]] <= dist[[0, 9 - 1 - c]] + 1e-9,
                "left cell col {c} should be <= mirrored right cell"
            );
        }
    }

    // ------------------------------------------------------------------
    // 3. Barrier wall of nodata — path must go around
    // ------------------------------------------------------------------
    #[test]
    fn test_barrier_wall() {
        // 5×5 grid, friction=1.0, nodata=0.0.
        // Place a vertical wall at col=2 from row=0 to row=3 (leaving row=4 open).
        // Source at (0,0), destination at (0,4) — must detour via bottom row.
        let mut grid = uniform(5, 5, 1.0);
        for r in 0..4 {
            grid[[r, 2]] = 0.0; // nodata marker
        }
        let sources = vec![(0usize, 0usize)];
        let nodata = Some(0.0_f64);
        let dist = cost_distance(&grid, &sources, 1.0, nodata).expect("cost_distance failed");

        // (0,4) must be reachable (finite).
        assert!(
            dist[[0, 4]].is_finite(),
            "destination should be reachable via detour, got {}",
            dist[[0, 4]]
        );

        // Direct cost (ignoring barrier): would be 4 cardinal steps = 4.0.
        // Actual path through detour must be longer.
        assert!(
            dist[[0, 4]] > 4.0 + 1e-9,
            "cost around barrier must exceed direct cost 4.0, got {}",
            dist[[0, 4]]
        );
    }

    // ------------------------------------------------------------------
    // 4. LCP simple — uniform 3×5, source at left column, dest at right column
    // ------------------------------------------------------------------
    #[test]
    fn test_lcp_simple() {
        // 3×5 grid, friction=1.0, source at (1,0), dest at (1,4).
        // Optimal path: straight horizontal, 4 steps → cost = 4.0.
        let grid = uniform(3, 5, 1.0);
        let sources = vec![(1usize, 0usize)];
        let path =
            least_cost_path(&grid, &sources, (1, 4), 1.0, None).expect("least_cost_path failed");

        // Path must start at source and end at dest.
        assert_eq!(
            path.first(),
            Some(&(1usize, 0usize)),
            "path should start at source (1,0)"
        );
        assert_eq!(
            path.last(),
            Some(&(1usize, 4usize)),
            "path should end at dest (1,4)"
        );

        // All cells must be within the grid.
        for &(r, c) in &path {
            assert!(r < 3 && c < 5, "path cell ({r},{c}) out of bounds");
        }

        // Minimum possible path length is 5 cells (4 steps).
        assert!(path.len() >= 5, "path must visit at least 5 cells");
    }

    // ------------------------------------------------------------------
    // 5. LCP no path — source isolated by nodata ring
    // ------------------------------------------------------------------
    #[test]
    fn test_lcp_no_path() {
        // 5×5 grid. Surround source (2,2) completely with nodata cells.
        let mut grid = uniform(5, 5, 1.0);
        let nodata_val = -1.0_f64;
        for (r, c) in [
            (1usize, 1usize),
            (1, 2),
            (1, 3),
            (2, 1),
            (2, 3),
            (3, 1),
            (3, 2),
            (3, 3),
        ] {
            grid[[r, c]] = nodata_val;
        }
        let sources = vec![(2usize, 2usize)];
        let result = least_cost_path(&grid, &sources, (0, 0), 1.0, Some(nodata_val));

        assert!(
            matches!(result, Err(TerrainError::NoPath { .. })),
            "expected NoPath error, got: {result:?}"
        );
    }

    // ------------------------------------------------------------------
    // 6. Cost allocation — 5×5, two sources, each cell assigned to nearest
    // ------------------------------------------------------------------
    #[test]
    fn test_cost_allocation() {
        // 5×5 uniform friction=1.0.
        // Source 0 at (0,0), source 1 at (4,4).
        // Cells closer to top-left → index 0; closer to bottom-right → index 1.
        let grid = uniform(5, 5, 1.0);
        let sources = vec![(0usize, 0usize), (4usize, 4usize)];
        let alloc = cost_allocation(&grid, &sources, 1.0, None).expect("cost_allocation failed");

        // Top-left corner is source 0 itself.
        assert_eq!(alloc[[0, 0]], 0, "top-left should belong to source 0");
        // Bottom-right corner is source 1 itself.
        assert_eq!(alloc[[4, 4]], 1, "bottom-right should belong to source 1");

        // Cells in top-left quadrant should all be 0.
        for r in 0..2 {
            for c in 0..2 {
                assert_eq!(
                    alloc[[r, c]],
                    0,
                    "cell ({r},{c}) should be allocated to source 0"
                );
            }
        }

        // Cells in bottom-right quadrant should all be 1.
        for r in 3..5 {
            for c in 3..5 {
                assert_eq!(
                    alloc[[r, c]],
                    1,
                    "cell ({r},{c}) should be allocated to source 1"
                );
            }
        }
    }

    // ------------------------------------------------------------------
    // 7. Invalid cell size → Err(InvalidCellSize)
    // ------------------------------------------------------------------
    #[test]
    fn test_invalid_cell_size() {
        let grid = uniform(3, 3, 1.0);
        let sources = vec![(1usize, 1usize)];

        let result_zero = cost_distance(&grid, &sources, 0.0, None);
        assert!(
            matches!(result_zero, Err(TerrainError::InvalidCellSize { size }) if (size - 0.0).abs() < f64::EPSILON),
            "expected InvalidCellSize for size=0, got: {result_zero:?}"
        );

        let result_neg = cost_distance(&grid, &sources, -1.5, None);
        assert!(
            matches!(result_neg, Err(TerrainError::InvalidCellSize { size }) if (size - (-1.5)).abs() < f64::EPSILON),
            "expected InvalidCellSize for size=-1.5, got: {result_neg:?}"
        );
    }

    // ------------------------------------------------------------------
    // 8. Empty sources — all cells remain infinity
    // ------------------------------------------------------------------
    #[test]
    fn test_empty_sources() {
        let grid = uniform(3, 3, 1.0);
        let sources: Vec<(usize, usize)> = vec![];
        let dist = cost_distance(&grid, &sources, 1.0, None).expect("cost_distance failed");
        for v in dist.iter() {
            assert!(v.is_infinite(), "with no sources all distances must be ∞");
        }
    }

    // ------------------------------------------------------------------
    // 9. Empty grid → Err(InvalidDimensions)
    // ------------------------------------------------------------------
    #[test]
    fn test_empty_grid() {
        let grid: Array2<f64> = Array2::from_shape_vec((0, 5), vec![]).expect("shape_vec");
        let result = cost_distance(&grid, &[], 1.0, None);
        assert!(
            matches!(result, Err(TerrainError::InvalidDimensions { .. })),
            "expected InvalidDimensions for empty grid, got: {result:?}"
        );
    }
}
