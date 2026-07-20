//! Catchment / sub-watershed delineation from one or more pour points.
//!
//! Given a digital elevation model, a sink-filled D8 flow-direction grid, and
//! a list of pour-point world coordinates, produce a labelled raster where
//! each cell carries the integer ID of the pour point whose catchment it
//! belongs to (0 = outside any catchment), plus a per-catchment summary
//! (`Vec<CatchmentInfo>`).
//!
//! # Coordinate convention
//!
//! World coordinates are interpreted with **GIS-standard north-up at top-left**.
//! Given `origin = (origin_x, origin_y)` (the world coordinate of the
//! upper-left corner of cell (0, 0)) and `cell_size`:
//!
//! ```text
//! col = ((x_world - origin_x) / cell_size).round() as isize
//! row = ((origin_y - y_world) / cell_size).round() as isize
//! ```
//!
//! Pour points whose rounded (row, col) falls outside the DEM raise
//! `TerrainError::ComputationError` with a descriptive message.
//!
//! Pour-point coordinates **must share the DEM's CRS**. Mixing geographic and
//! projected systems yields silent area miscounts.
//!
//! # Determinism
//!
//! Catchments are written in input-list order. When two pour points'
//! catchments overlap, the **earlier** pour point in the input list wins —
//! this is documented behaviour, not a coincidence of iteration order. BFS
//! visits cells via a row-major-keyed `VecDeque`, never a hash-based
//! container.

use crate::error::{Result, TerrainError};
use crate::hydrology::flow_direction::D8_DIRS;
use num_traits::Float;
use scirs2_core::prelude::*;
use std::collections::VecDeque;

/// Strategy for resolving a world-coordinate pour point onto a discrete cell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SnapPolicy {
    /// Snap to the highest-flow-accumulation cell within `radius_cells`
    /// (Chebyshev distance) of the rounded pour-point cell.
    ///
    /// Default radius in production code is 3 cells; callers can override.
    /// Radius 0 collapses to "use the rounded cell as-is".
    ToHighestAccum {
        /// Search radius in cells (Chebyshev / max-norm).
        radius_cells: u32,
    },
    /// Use the rounded cell exactly. No snap. The function still tolerates a
    /// rounded cell on a sink (`flow_dir == 0`) — callers wanting a pre-flight
    /// check should pre-validate.
    Exact,
}

impl Default for SnapPolicy {
    fn default() -> Self {
        SnapPolicy::ToHighestAccum { radius_cells: 3 }
    }
}

/// Per-catchment summary record.
#[derive(Debug, Clone, PartialEq)]
pub struct CatchmentInfo {
    /// Catchment ID (1..N, in input-list order).
    pub id: u32,
    /// Row of the (snapped) pour-point cell.
    pub pour_row: usize,
    /// Column of the (snapped) pour-point cell.
    pub pour_col: usize,
    /// Number of cells assigned to this catchment.
    pub area_cells: u64,
    /// Catchment area in m² (assumes `cell_size` is in metres or that the
    /// caller is comfortable with the unit).
    pub area_m2: f64,
}

/// Delineate sub-watersheds from a list of pour-point world coordinates.
///
/// # Parameters
/// * `dem` — elevation raster (used only to honour the optional nodata mask
///   and to size the result; flow-direction logic is delegated to
///   `flow_dir_d8`).
/// * `flow_dir_d8` — D8 flow-direction codes (1, 2, 4, 8, 16, 32, 64, 128).
///   Must be sink-filled or callers will see fragmented basins.
/// * `pour_points` — slice of world coordinates `(x_world, y_world)`.
/// * `cell_size` — pixel size in DEM linear units (metres for projected CRS).
/// * `origin` — world coordinate of the upper-left corner of cell (0, 0).
/// * `snap_policy` — how to resolve pour-point coordinates onto a cell.
///
/// # Returns
/// A pair `(labels, summaries)`:
/// * `labels: Array2<u32>` — same shape as `dem`. Cell value = catchment ID
///   (1..N) or 0 if the cell drains outside any provided pour point.
/// * `summaries: Vec<CatchmentInfo>` — exactly one record per input pour
///   point, in input order.
///
/// # Errors
/// * `TerrainError::InvalidDimensions` — if `dem` and `flow_dir_d8` shapes
///   disagree, or either is smaller than 1×1.
/// * `TerrainError::InvalidCellSize` — if `cell_size <= 0`.
/// * `TerrainError::ComputationError` — if any pour-point's rounded cell is
///   outside the DEM, or if `SnapPolicy::ToHighestAccum` finds no candidate
///   cell within `radius_cells` (e.g. radius too small + pour point falls on
///   nodata).
pub fn delineate_catchments<T>(
    dem: &Array2<T>,
    flow_dir_d8: &Array2<u8>,
    pour_points: &[(f64, f64)],
    cell_size: f64,
    origin: (f64, f64),
    snap_policy: SnapPolicy,
) -> Result<(Array2<u32>, Vec<CatchmentInfo>)>
where
    T: Float + Into<f64> + Copy,
{
    let (height, width) = dem.dim();
    let (fh, fw) = flow_dir_d8.dim();
    if (fh, fw) != (height, width) {
        return Err(TerrainError::InvalidDimensions {
            width: fw,
            height: fh,
        });
    }
    if height == 0 || width == 0 {
        return Err(TerrainError::InvalidDimensions { width, height });
    }
    if cell_size <= 0.0 {
        return Err(TerrainError::InvalidCellSize { size: cell_size });
    }

    let mut labels = Array2::<u32>::zeros((height, width));
    let mut summaries: Vec<CatchmentInfo> = Vec::with_capacity(pour_points.len());
    let pixel_area_m2 = cell_size * cell_size;

    // Lazily compute the flow-accumulation grid only when SnapPolicy needs it.
    let accumulation: Option<Array2<u32>> = match snap_policy {
        SnapPolicy::ToHighestAccum { .. } => Some(
            crate::hydrology::flow_accumulation::flow_accumulation(dem, cell_size, None)?,
        ),
        SnapPolicy::Exact => None,
    };

    // Iterate input list in order: earlier pour points win on overlap.
    for (idx, &(x_world, y_world)) in pour_points.iter().enumerate() {
        let id = (idx as u32) + 1;
        let (raw_row, raw_col) = world_to_cell(x_world, y_world, origin, cell_size, height, width)?;

        let (pour_row, pour_col) = match snap_policy {
            SnapPolicy::ToHighestAccum { radius_cells } => snap_to_max_accum(
                accumulation
                    .as_ref()
                    .ok_or_else(|| TerrainError::ComputationError {
                        message: "internal: accumulation grid missing under ToHighestAccum"
                            .to_owned(),
                    })?,
                raw_row,
                raw_col,
                radius_cells,
                height,
                width,
            )?,
            SnapPolicy::Exact => (raw_row, raw_col),
        };

        // BFS upslope using inverse D8 adjacency. Earlier pour points have
        // already painted their basins; we leave those cells alone.
        let mut area_cells: u64 = 0;
        let pour_idx = pour_row * width + pour_col;
        let mut queue: VecDeque<usize> = VecDeque::new();

        if labels[[pour_row, pour_col]] == 0 {
            labels[[pour_row, pour_col]] = id;
            area_cells += 1;
            queue.push_back(pour_idx);
        }
        // If the pour cell is already painted (overlap with earlier pour),
        // we still record the summary with the snapped coords but contribute
        // zero new cells — this matches "earlier wins" semantics.

        while let Some(cell_idx) = queue.pop_front() {
            let row = cell_idx / width;
            let col = cell_idx % width;
            // For each of the 8 neighbours, ask: does it flow INTO (row, col)?
            // If yes, claim it for this catchment.
            for &(dr, dc, _code) in &D8_DIRS {
                let nr = row as isize + dr;
                let nc = col as isize + dc;
                if nr < 0 || nr >= height as isize || nc < 0 || nc >= width as isize {
                    continue;
                }
                let nu = nr as usize;
                let nv = nc as usize;
                if labels[[nu, nv]] != 0 {
                    continue; // already claimed (by this or an earlier pour)
                }
                if neighbour_flows_into(flow_dir_d8, nu, nv, row, col) {
                    labels[[nu, nv]] = id;
                    area_cells += 1;
                    queue.push_back(nu * width + nv);
                }
            }
        }

        summaries.push(CatchmentInfo {
            id,
            pour_row,
            pour_col,
            area_cells,
            area_m2: (area_cells as f64) * pixel_area_m2,
        });
    }

    Ok((labels, summaries))
}

/// Convert world coordinates into discrete (row, col) using the GIS
/// north-up-top-left convention. Returns `Err` if the rounded cell falls
/// outside the DEM.
fn world_to_cell(
    x_world: f64,
    y_world: f64,
    origin: (f64, f64),
    cell_size: f64,
    height: usize,
    width: usize,
) -> Result<(usize, usize)> {
    let col_f = (x_world - origin.0) / cell_size;
    let row_f = (origin.1 - y_world) / cell_size;
    let col_i = col_f.round() as isize;
    let row_i = row_f.round() as isize;
    if row_i < 0 || row_i >= height as isize || col_i < 0 || col_i >= width as isize {
        return Err(TerrainError::ComputationError {
            message: format!(
                "pour point ({x_world}, {y_world}) maps to cell (row {row_i}, col {col_i}) \
                 which lies outside the DEM bounds ({height}×{width})."
            ),
        });
    }
    Ok((row_i as usize, col_i as usize))
}

/// Within a Chebyshev radius of `(row, col)`, locate the cell with the
/// highest flow-accumulation value. Ties are broken by row-major order
/// (deterministic).
fn snap_to_max_accum(
    accumulation: &Array2<u32>,
    row: usize,
    col: usize,
    radius_cells: u32,
    height: usize,
    width: usize,
) -> Result<(usize, usize)> {
    let r = radius_cells as isize;
    let mut best_row = row;
    let mut best_col = col;
    let mut best_acc = accumulation[[row, col]];
    let mut found_any = false;
    // Iterate top-left → bottom-right for deterministic tie-breaking.
    for dr in -r..=r {
        for dc in -r..=r {
            let nr = row as isize + dr;
            let nc = col as isize + dc;
            if nr < 0 || nr >= height as isize || nc < 0 || nc >= width as isize {
                continue;
            }
            let nu = nr as usize;
            let nv = nc as usize;
            let val = accumulation[[nu, nv]];
            if !found_any || val > best_acc {
                best_acc = val;
                best_row = nu;
                best_col = nv;
                found_any = true;
            }
        }
    }
    if !found_any {
        return Err(TerrainError::ComputationError {
            message: format!(
                "snap radius {radius_cells} found no candidate cells around (row {row}, col {col})."
            ),
        });
    }
    Ok((best_row, best_col))
}

/// True iff cell `(from_row, from_col)`'s D8 flow direction points to
/// `(to_row, to_col)`. Robust against `usize` underflow — uses signed
/// arithmetic with explicit sign comparison.
fn neighbour_flows_into(
    flow_dir: &Array2<u8>,
    from_row: usize,
    from_col: usize,
    to_row: usize,
    to_col: usize,
) -> bool {
    let dir = flow_dir[[from_row, from_col]];
    if dir == 0 {
        return false;
    }
    let dr_target = (to_row as isize) - (from_row as isize);
    let dc_target = (to_col as isize) - (from_col as isize);
    // The cell flows into the neighbour iff the D8 offset matches.
    D8_DIRS
        .iter()
        .any(|&(dr, dc, code)| code == dir && dr == dr_target && dc == dc_target)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a D8 flow-direction grid from row-major literals.
    fn dir_grid(rows: usize, cols: usize, data: &[u8]) -> Array2<u8> {
        assert_eq!(rows * cols, data.len());
        let mut a = Array2::<u8>::zeros((rows, cols));
        for r in 0..rows {
            for c in 0..cols {
                a[[r, c]] = data[r * cols + c];
            }
        }
        a
    }

    fn dem_dummy(rows: usize, cols: usize) -> Array2<f64> {
        // Simple monotonic east-sloping DEM — the actual values barely matter
        // since flow_dir is supplied directly, but flow_accumulation needs
        // sane values for SnapPolicy::ToHighestAccum.
        let mut a = Array2::<f64>::zeros((rows, cols));
        for r in 0..rows {
            for c in 0..cols {
                a[[r, c]] = 100.0 - (c as f64) * 10.0;
            }
        }
        a
    }

    /// Single pour point at the outlet of a 3×4 east-flowing basin. Every
    /// cell except the rightmost column flows east; the rightmost column is
    /// the outlet (dir=0 because we leave the raster). Pour at the bottom-
    /// right cell — every interior cell should be claimed.
    #[test]
    fn test_catchment_single_pour_point_simple_basin() {
        // 3×4. All cells flow east (1) except the last column which would
        // exit east — we leave dir=1 there, the BFS will claim by inverse
        // adjacency from the pour point.
        let dir = dir_grid(
            3,
            4,
            &[
                1, 1, 1, 1, //
                1, 1, 1, 1, //
                1, 1, 1, 1,
            ],
        );
        let dem = dem_dummy(3, 4);
        // Pour point at world coordinate (3.0, -2.0) with cell_size=1 and
        // origin (0.0, 0.0): col = 3, row = 2 → bottom-right cell.
        // Wait — row = (origin_y - y_world)/cs = (0 - (-2))/1 = 2.
        let pour = vec![(3.0, -2.0)];
        let (labels, summaries) =
            delineate_catchments(&dem, &dir, &pour, 1.0, (0.0, 0.0), SnapPolicy::Exact)
                .expect("delineation failed");
        // Bottom-right is the pour cell.
        assert_eq!(labels[[2, 3]], 1);
        // (2,2)'s flow_dir=1 (E) → does it flow into (2,3)? Yes.
        assert_eq!(labels[[2, 2]], 1);
        // (1,3) flow_dir=1 → flows to (1,4) which is off-grid; (1,3) does
        // NOT flow to (2,3). So (1,3) should not be in the catchment of (2,3).
        assert_eq!(labels[[1, 3]], 0);
        // Verify summary: at least 2 cells (the pour and (2,2), (2,1), (2,0))
        let info = &summaries[0];
        assert_eq!(info.id, 1);
        assert_eq!(info.pour_row, 2);
        assert_eq!(info.pour_col, 3);
        assert_eq!(info.area_cells, 4); // (2,0)→(2,1)→(2,2)→(2,3)
        assert!((info.area_m2 - 4.0).abs() < 1e-12);
    }

    /// Two completely disjoint basins, two pour points → two labelled regions.
    #[test]
    fn test_catchment_two_disjoint_basins() {
        // 5 rows × 4 cols. Top half drains east on row 0; bottom half on row 4.
        // Middle rows are sinks (dir=0) so they don't propagate.
        let dir = dir_grid(
            5,
            4,
            &[
                1, 1, 1, 1, //
                0, 0, 0, 0, //
                0, 0, 0, 0, //
                0, 0, 0, 0, //
                1, 1, 1, 1,
            ],
        );
        let dem = dem_dummy(5, 4);
        let pour = vec![(3.0, 0.0), (3.0, -4.0)];
        let (labels, summaries) =
            delineate_catchments(&dem, &dir, &pour, 1.0, (0.0, 0.0), SnapPolicy::Exact)
                .expect("delineation failed");
        // Pour 1 at (0,3); chain (0,0)→(0,1)→(0,2)→(0,3): 4 cells.
        assert_eq!(labels[[0, 3]], 1);
        assert_eq!(labels[[0, 0]], 1);
        // Pour 2 at (4,3); chain (4,0)→...→(4,3): 4 cells, label=2.
        assert_eq!(labels[[4, 3]], 2);
        assert_eq!(labels[[4, 0]], 2);
        // Middle rows have dir=0 → not in any basin.
        assert_eq!(labels[[2, 2]], 0);
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].area_cells, 4);
        assert_eq!(summaries[1].area_cells, 4);
    }

    /// Two pour points whose drainage basins overlap. Earlier in the input
    /// list wins.
    #[test]
    fn test_catchment_overlapping_pour_points_first_wins() {
        // 1 row × 5 cols, all flowing east. Pour at (4) and (3): pour 1 owns
        // cells (0,0..0,4), pour 2's catchment would also include (0,0..0,3)
        // but those are already claimed.
        let dir = dir_grid(1, 5, &[1, 1, 1, 1, 1]);
        let dem = dem_dummy(1, 5);
        // Pour 1 at col=4 (label 1); pour 2 at col=3 (label 2).
        let pour = vec![(4.0, 0.0), (3.0, 0.0)];
        let (labels, summaries) =
            delineate_catchments(&dem, &dir, &pour, 1.0, (0.0, 0.0), SnapPolicy::Exact)
                .expect("delineation failed");
        // (0,0..0,4) all carry label 1 (earlier pour).
        for c in 0..5 {
            assert_eq!(
                labels[[0, c]],
                1,
                "col {c} should belong to first pour point"
            );
        }
        // Summary for pour 1: 5 cells. Pour 2: 0 new cells (cell (0,3)
        // already claimed).
        assert_eq!(summaries[0].area_cells, 5);
        assert_eq!(summaries[1].area_cells, 0);
        // Pour 2's recorded location is its snapped cell, not a "no-op".
        assert_eq!(summaries[1].pour_row, 0);
        assert_eq!(summaries[1].pour_col, 3);
    }

    /// SnapPolicy::ToHighestAccum should drag a near-miss pour point to the
    /// neighbouring high-accumulation cell.
    #[test]
    fn test_catchment_snap_to_max_accum() {
        // 3 rows × 5 cols. All cells flow east; rightmost cells accumulate
        // the most. Pour world coord lands at (1, 0.0) but with snap radius 2
        // we should drift to the rightmost column where accumulation is
        // largest (4 in row 1).
        let dir = dir_grid(
            3,
            5,
            &[
                1, 1, 1, 1, 1, //
                1, 1, 1, 1, 1, //
                1, 1, 1, 1, 1,
            ],
        );
        let dem = dem_dummy(3, 5);
        // World coordinate that maps to (row=0, col=2). With snap radius 2
        // we look at columns 0..=4 in rows 0..=2 and pick the highest accum.
        // Row-major tie-break inside snap_to_max_accum starts at top-left.
        let pour = vec![(2.0, 0.0)];
        let (labels, summaries) = delineate_catchments(
            &dem,
            &dir,
            &pour,
            1.0,
            (0.0, 0.0),
            SnapPolicy::ToHighestAccum { radius_cells: 2 },
        )
        .expect("delineation failed");
        // Snapped pour should be in the rightmost column where accumulation
        // is maximised. Each row's outlet cell carries the highest accum
        // within row=0..=2, col=0..=4 (since flow goes east and accumulates).
        // The exact (row, col) depends on tie-breaking; assert col=4.
        let info = &summaries[0];
        assert_eq!(info.pour_col, 4, "snap should drag pour to outlet column");
        // The catchment must include at least the snapped cell.
        assert_eq!(labels[[info.pour_row, info.pour_col]], 1);
    }

    /// SnapPolicy::Exact disables snapping and uses the rounded cell as-is.
    #[test]
    fn test_catchment_exact_no_snap() {
        let dir = dir_grid(
            3,
            5,
            &[
                1, 1, 1, 1, 1, //
                1, 1, 1, 1, 1, //
                1, 1, 1, 1, 1,
            ],
        );
        let dem = dem_dummy(3, 5);
        // Pour world coord maps to (row=0, col=2). With Exact, the pour cell
        // stays at (0,2) regardless of accumulation.
        let pour = vec![(2.0, 0.0)];
        let (_labels, summaries) =
            delineate_catchments(&dem, &dir, &pour, 1.0, (0.0, 0.0), SnapPolicy::Exact)
                .expect("delineation failed");
        assert_eq!(summaries[0].pour_row, 0);
        assert_eq!(summaries[0].pour_col, 2);
        // Catchment area: cells that drain into (0,2) = (0,0), (0,1), (0,2).
        assert_eq!(summaries[0].area_cells, 3);
    }

    /// Pour point outside the DEM raises ComputationError.
    #[test]
    fn test_catchment_pour_point_outside_dem_errors() {
        let dir = dir_grid(2, 2, &[1, 1, 1, 1]);
        let dem = dem_dummy(2, 2);
        // World coordinate (10, -10) with cell_size 1 and origin (0,0):
        // col = 10, row = 10 → far outside the 2×2 DEM.
        let pour = vec![(10.0, -10.0)];
        let err = delineate_catchments(&dem, &dir, &pour, 1.0, (0.0, 0.0), SnapPolicy::Exact)
            .expect_err("expected out-of-bounds error");
        assert!(
            matches!(
                &err,
                TerrainError::ComputationError { message }
                    if message.contains("outside")
                        || message.contains("bounds")
                        || message.contains("DEM")
            ),
            "expected ComputationError mentioning out-of-bounds; got: {err:?}"
        );
    }
}
