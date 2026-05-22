//! Stream network extraction and Strahler ordering.
//!
//! Strahler stream ordering classifies channels by topological position within
//! a drainage network. Channel heads are order σ = 1; at a confluence where ≥ 2
//! incoming children share the maximum order σ_max, the resulting downstream
//! channel takes σ = σ_max + 1; otherwise the downstream channel inherits
//! σ_max. The algorithm is **always** evaluated on the D8 graph because
//! D-infinity flow direction splits a cell's outflow fractionally between two
//! neighbours and therefore cannot define a unique downstream parent
//! (Tarboton 1991; matches TauDEM/GRASS/SAGA conventions).

use crate::error::{Result, TerrainError};
use crate::hydrology::flow_accumulation::flow_accumulation;
use crate::hydrology::flow_direction::{D8_DIRS, flow_direction_d8};
use num_traits::Float;
use scirs2_core::prelude::*;
use std::collections::VecDeque;

/// Extract stream network from flow accumulation.
pub fn extract_streams<T>(
    dem: &Array2<T>,
    cell_size: f64,
    threshold: u32,
    nodata: Option<T>,
) -> Result<Array2<u8>>
where
    T: Float + Into<f64> + Copy,
{
    let accumulation = flow_accumulation(dem, cell_size, nodata)?;
    let (height, width) = accumulation.dim();
    let mut streams = Array2::zeros((height, width));

    for y in 0..height {
        for x in 0..width {
            if accumulation[[y, x]] >= threshold {
                streams[[y, x]] = 1;
            }
        }
    }

    Ok(streams)
}

/// Calculate Strahler stream order from a DEM.
///
/// Computes the channel mask via the standard pipeline
/// `flow_direction_d8` → `flow_accumulation` → `extract_streams(threshold)`,
/// then performs Kahn's topological sort over the D8 channel graph and
/// assigns Strahler order σ to each channel cell. Off-channel cells stay 0.
///
/// # Pre-conditions
/// The DEM should be sink-filled (call `fill_sinks_priority_flood` first).
/// If a channel cell has D8 flow direction 0 (i.e. an unfilled sink), the
/// function returns `TerrainError::ComputationError` listing the count.
///
/// # Determinism
/// Uses a `Vec<bool>` visited mask plus a `VecDeque` FIFO seeded in row-major
/// order — never a hash-based collection — so output is stable across runs.
pub fn strahler_order<T>(
    dem: &Array2<T>,
    cell_size: f64,
    threshold: u32,
    nodata: Option<T>,
) -> Result<Array2<u8>>
where
    T: Float + Into<f64> + Copy,
{
    let flow_dir = flow_direction_d8(dem, cell_size, nodata)?;
    let channels = extract_streams(dem, cell_size, threshold, nodata)?;
    strahler_order_from_d8(&channels, &flow_dir)
}

/// Strahler ordering from a precomputed channel mask and D8 flow-direction grid.
///
/// Use this entry point when you already have both grids and want to skip the
/// re-derivation pipeline (e.g. when stamping segment metadata in
/// [`channel_network::extract_channel_network`][crate::hydrology::channel_network::extract_channel_network]
/// or when reusing a sink-filled flow grid across multiple analyses).
///
/// # Inputs
/// * `channel_mask` — 0/1 raster of equal shape to `flow_dir_d8` flagging
///   channel cells.
/// * `flow_dir_d8` — D8 flow-direction codes (1, 2, 4, 8, 16, 32, 64, 128).
///   A code of 0 indicates a pit / sink / nodata.
///
/// # Errors
/// * `TerrainError::InvalidDimensions` — if the two grids disagree in shape.
/// * `TerrainError::ComputationError` — if any channel cell carries
///   `flow_dir == 0` (unfilled sink) or if Kahn's algorithm leaves any
///   channel cell unprocessed (cycle from epsilon underflow on f32).
pub fn strahler_order_from_d8(
    channel_mask: &Array2<u8>,
    flow_dir_d8: &Array2<u8>,
) -> Result<Array2<u8>> {
    let (height, width) = channel_mask.dim();
    let (fh, fw) = flow_dir_d8.dim();
    if (fh, fw) != (height, width) {
        return Err(TerrainError::InvalidDimensions {
            width: fw,
            height: fh,
        });
    }

    let n = height * width;
    let idx = |row: usize, col: usize| row * width + col;
    let row_of = |i: usize| i / width;
    let col_of = |i: usize| i % width;

    // -----------------------------------------------------------------
    // 1. Sink-on-channel diagnostic (pre-condition: DEM sink-filled).
    // -----------------------------------------------------------------
    let mut sink_count: usize = 0;
    let mut is_channel = vec![false; n];
    for row in 0..height {
        for col in 0..width {
            if channel_mask[[row, col]] != 0 {
                is_channel[idx(row, col)] = true;
                if flow_dir_d8[[row, col]] == 0 {
                    sink_count += 1;
                }
            }
        }
    }
    if sink_count > 0 {
        return Err(TerrainError::ComputationError {
            message: format!(
                "{sink_count} channel cell(s) carry flow_dir == 0 (unfilled sinks). \
                 Call fill_sinks_priority_flood on the DEM before computing Strahler order."
            ),
        });
    }

    // -----------------------------------------------------------------
    // 2. Compute downstream-target index and incoming-channel count for
    //    every channel cell, in a single pass.
    //
    //    `downstream[i]` is `Some(j)` when the D8 step from i lands on
    //    another channel cell (i.e. an internal edge in the channel graph)
    //    and `None` for off-grid outlets (final segment exits the raster).
    // -----------------------------------------------------------------
    let mut downstream: Vec<Option<usize>> = vec![None; n];
    let mut indegree: Vec<u32> = vec![0; n];

    for row in 0..height {
        for col in 0..width {
            let i = idx(row, col);
            if !is_channel[i] {
                continue;
            }
            let dir = flow_dir_d8[[row, col]];
            // Locate the D8 offset corresponding to this code.
            let step = D8_DIRS.iter().find(|(_, _, code)| *code == dir);
            if let Some(&(dy, dx, _)) = step {
                let nr = row as isize + dy;
                let nc = col as isize + dx;
                if nr >= 0 && nr < height as isize && nc >= 0 && nc < width as isize {
                    let nu = nr as usize;
                    let nv = nc as usize;
                    if is_channel[idx(nu, nv)] {
                        let j = idx(nu, nv);
                        downstream[i] = Some(j);
                        indegree[j] += 1;
                    }
                    // else: downstream cell is not a channel — this is a leaf
                    //       in the channel graph (channel exits into the
                    //       sub-threshold zone). Treat as off-grid outlet.
                }
                // else: D8 step leaves the raster — true off-grid outlet.
            }
        }
    }

    // -----------------------------------------------------------------
    // 3. Strahler accumulator (advisor pattern #3): per-cell track
    //    (max_order, max_count). When an upstream child is finalized,
    //    update its single downstream parent. When the parent's remaining
    //    indegree hits zero, finalize it.
    //
    //    Heads (indegree 0) start with max_count = 0 → finalize σ = 1.
    //    Confluence: if a child's σ equals the running max, increment
    //    count; if it's greater, reset (max, count) to (child_σ, 1).
    //    σ_self = max_order + 1 if max_count >= 2, else max_order
    //    (or 1 for a head).
    // -----------------------------------------------------------------
    let mut order: Vec<u8> = vec![0; n];
    let mut max_order: Vec<u8> = vec![0; n];
    let mut max_count: Vec<u8> = vec![0; n];
    let mut remaining: Vec<u32> = indegree.clone();

    let mut queue: VecDeque<usize> = VecDeque::new();
    // Seed the queue with channel heads in row-major order (deterministic).
    for row in 0..height {
        for col in 0..width {
            let i = idx(row, col);
            if is_channel[i] && indegree[i] == 0 {
                queue.push_back(i);
            }
        }
    }

    let mut processed: usize = 0;
    while let Some(i) = queue.pop_front() {
        let sigma_i = if max_count[i] == 0 {
            1
        } else if max_count[i] >= 2 {
            // Saturating in case of pathological input; max σ < 256 in
            // realistic DEMs but cheap to guard.
            max_order[i].saturating_add(1)
        } else {
            max_order[i]
        };
        order[i] = sigma_i;
        processed += 1;

        if let Some(j) = downstream[i] {
            // Update parent's accumulator.
            let parent_max = max_order[j];
            if sigma_i > parent_max {
                max_order[j] = sigma_i;
                max_count[j] = 1;
            } else if sigma_i == parent_max {
                max_count[j] = max_count[j].saturating_add(1);
            }
            // else: child contributes nothing (its σ is already dominated).

            remaining[j] -= 1;
            if remaining[j] == 0 {
                queue.push_back(j);
            }
        }
    }

    // -----------------------------------------------------------------
    // 4. Cycle diagnostic. If Kahn's left any channel cell unprocessed,
    //    the channel graph contains a cycle (typically caused by an
    //    f32 DEM whose post-fill epsilon was too small).
    // -----------------------------------------------------------------
    let total_channel_cells: usize = is_channel.iter().filter(|&&b| b).count();
    if processed < total_channel_cells {
        let stuck = total_channel_cells - processed;
        // Find one stuck cell to include in the diagnostic for debuggability.
        let example = (0..n).find(|&i| is_channel[i] && order[i] == 0);
        let example_str = match example {
            Some(i) => format!(" (example: row {}, col {})", row_of(i), col_of(i)),
            None => String::new(),
        };
        return Err(TerrainError::ComputationError {
            message: format!(
                "{stuck} channel cell(s) unreachable by Kahn's topological sort{example_str}; \
                 channel graph contains a cycle (try a larger sink-fill epsilon)."
            ),
        });
    }

    // -----------------------------------------------------------------
    // 5. Project the flat order vector back into Array2<u8>.
    // -----------------------------------------------------------------
    let mut result = Array2::<u8>::zeros((height, width));
    for row in 0..height {
        for col in 0..width {
            result[[row, col]] = order[idx(row, col)];
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `(width, height)` grid from row-major literals for clarity.
    fn array_from(rows: usize, cols: usize, data: &[u8]) -> Array2<u8> {
        assert_eq!(rows * cols, data.len());
        let mut a = Array2::<u8>::zeros((rows, cols));
        for r in 0..rows {
            for c in 0..cols {
                a[[r, c]] = data[r * cols + c];
            }
        }
        a
    }

    /// Y-junction: two heads (σ=1) confluence into a σ=2 trunk.
    ///
    /// Layout (channel cells marked X, flow direction shown as arrow):
    /// ```text
    /// X→ X→ X↘
    ///         X→ X→ X
    /// X→ X→ X↗
    /// ```
    /// Two horizontal arms at rows 0 and 2 each consist of 3 heads-then-bend
    /// cells; they meet at row 1 col 3, then a 3-cell trunk continues east.
    #[test]
    fn test_strahler_simple_y_junction() {
        // 3 rows × 6 cols. Channel cells:
        //   (0,0) (0,1) (0,2) → SE diag at (0,2)
        //   (1,3) (1,4) (1,5)
        //   (2,0) (2,1) (2,2) → NE diag at (2,2)
        let mask = array_from(
            3,
            6,
            &[
                1, 1, 1, 0, 0, 0, //
                0, 0, 0, 1, 1, 1, //
                1, 1, 1, 0, 0, 0,
            ],
        );
        // Flow codes: 1=E, 2=SE, 128=NE.
        let dir = array_from(
            3,
            6,
            &[
                1, 1, 2, 0, 0, 0, //
                0, 0, 0, 1, 1, 1, //
                1, 1, 128, 0, 0, 0,
            ],
        );
        let order = strahler_order_from_d8(&mask, &dir).expect("strahler computation failed");
        // Top-arm heads: (0,0)=1, (0,1)=1, (0,2)=1
        assert_eq!(order[[0, 0]], 1);
        assert_eq!(order[[0, 1]], 1);
        assert_eq!(order[[0, 2]], 1);
        // Bottom-arm heads: (2,0)=1, (2,1)=1, (2,2)=1
        assert_eq!(order[[2, 0]], 1);
        assert_eq!(order[[2, 2]], 1);
        // Trunk after junction: σ=2 (two equal-rank tributaries).
        assert_eq!(order[[1, 3]], 2);
        assert_eq!(order[[1, 4]], 2);
        assert_eq!(order[[1, 5]], 2);
    }

    /// Three tributaries of equal order σ=1 meet at a single junction.
    /// Strahler rule "≥2 share the max" pushes parent to σ=2 (not σ=3 —
    /// only Shreve magnitude does that).
    #[test]
    fn test_strahler_three_way_tied_max() {
        // 3 rows × 3 cols, three single-cell tributaries flowing into (1,1).
        // (0,1) flows S → (1,1); (1,0) flows E → (1,1); (2,1) flows N → (1,1).
        // (1,1) flows E → (1,2) (off-channel; treated as off-grid outlet).
        let mask = array_from(
            3,
            3,
            &[
                0, 1, 0, //
                1, 1, 0, //
                0, 1, 0,
            ],
        );
        // Codes: 4=S, 1=E, 64=N.
        let dir = array_from(
            3,
            3,
            &[
                0, 4, 0, //
                1, 1, 0, //
                0, 64, 0,
            ],
        );
        let order = strahler_order_from_d8(&mask, &dir).expect("strahler computation failed");
        assert_eq!(order[[0, 1]], 1);
        assert_eq!(order[[1, 0]], 1);
        assert_eq!(order[[2, 1]], 1);
        // Three σ=1 children meet — max_count = 3 ≥ 2 → σ_self = 2.
        assert_eq!(order[[1, 1]], 2);
    }

    /// One dominant tributary (σ=2) and one minor (σ=1) → junction stays σ=2.
    #[test]
    fn test_strahler_one_dominant_tributary() {
        // 5 rows × 5 cols.
        // Dominant arm (top-left): two heads forming a σ=2 segment.
        //   (0,0) E→ (0,1) SE→ (1,2) E→ (2,2) E→ (2,3) E→ (2,4)
        //   (1,0) NE→ (0,1)  ← second head into the same σ=2 trunk start at (0,1)
        // Minor arm: single head (2,1) flowing E→ (2,2).
        // The junction at (2,2) sees:
        //   - upstream (1,2) which carries σ=2 (after Y-junction at (0,1))
        //   - upstream (2,1) which carries σ=1
        //   → max_order=2, max_count=1 → σ_self = 2 (no bump, only one max-rank child).
        let mask = array_from(
            5,
            5,
            &[
                1, 1, 0, 0, 0, //
                1, 0, 1, 0, 0, //
                0, 1, 1, 1, 1, //
                0, 0, 0, 0, 0, //
                0, 0, 0, 0, 0,
            ],
        );
        // Codes: 1=E, 2=SE, 128=NE.
        let dir = array_from(
            5,
            5,
            &[
                1, 2, 0, 0, 0, //
                128, 0, 4, 0, 0, //
                0, 1, 1, 1, 1, //
                0, 0, 0, 0, 0, //
                0, 0, 0, 0, 0,
            ],
        );
        let order = strahler_order_from_d8(&mask, &dir).expect("strahler computation failed");
        // Heads of dominant arm
        assert_eq!(order[[0, 0]], 1);
        assert_eq!(order[[1, 0]], 1);
        // Y-junction at (0,1): (0,0)→E and (1,0)→NE both land here. σ=2.
        assert_eq!(order[[0, 1]], 2);
        // (1,2) has only (0,1)=σ2 upstream → σ=2.
        assert_eq!(order[[1, 2]], 2);
        // Minor head
        assert_eq!(order[[2, 1]], 1);
        // Confluence at (2,2): upstream σ values {2 from (1,2), 1 from (2,1)}.
        // max_order=2, max_count=1 (only (1,2) has σ=2) → σ_self = 2.
        assert_eq!(order[[2, 2]], 2);
        // Continuation: trunk stays σ=2 to outlet.
        assert_eq!(order[[2, 3]], 2);
        assert_eq!(order[[2, 4]], 2);
    }

    /// Two completely disjoint channel components are ordered independently.
    #[test]
    fn test_strahler_disconnected_components() {
        // 3 rows × 6 cols. Two separate east-flowing channels.
        let mask = array_from(
            3,
            6,
            &[
                1, 1, 1, 0, 0, 0, //
                0, 0, 0, 0, 0, 0, //
                0, 0, 0, 1, 1, 1,
            ],
        );
        let dir = array_from(
            3,
            6,
            &[
                1, 1, 1, 0, 0, 0, //
                0, 0, 0, 0, 0, 0, //
                0, 0, 0, 1, 1, 1,
            ],
        );
        let order = strahler_order_from_d8(&mask, &dir).expect("strahler computation failed");
        // Each component is a linear chain of σ=1 cells.
        for col in 0..3 {
            assert_eq!(
                order[[0, col]],
                1,
                "top component cell ({col}) should be σ=1"
            );
        }
        for col in 3..6 {
            assert_eq!(
                order[[2, col]],
                1,
                "bottom component cell ({col}) should be σ=1"
            );
        }
        // Off-channel cells stay 0.
        assert_eq!(order[[1, 0]], 0);
    }

    /// A channel head whose only upstream neighbour is non-channel still
    /// receives σ=1 (no upstream channel children).
    #[test]
    fn test_strahler_channel_head_only_non_channel_upstream() {
        // 3 rows × 3 cols. (1,0) is a single-cell channel that flows E to
        // (1,1). (0,0) is not a channel but has flow direction set; it should
        // not contribute to (1,0).
        let mask = array_from(
            3,
            3,
            &[
                0, 0, 0, //
                1, 1, 0, //
                0, 0, 0,
            ],
        );
        let dir = array_from(
            3,
            3,
            &[
                4, 0, 0, // (0,0) flows S → (1,0) but (0,0) is not channel
                1, 1, 0, //
                0, 0, 0,
            ],
        );
        let order = strahler_order_from_d8(&mask, &dir).expect("strahler computation failed");
        assert_eq!(order[[1, 0]], 1, "channel head should be σ=1");
        assert_eq!(order[[1, 1]], 1, "single downstream cell stays σ=1");
        assert_eq!(order[[0, 0]], 0, "non-channel cell should remain 0");
    }

    /// The outlet flows off-grid. Function must succeed and assign the
    /// outlet σ correctly.
    #[test]
    fn test_strahler_off_grid_outlet() {
        // 2 rows × 3 cols. Channel: (0,0) E→ (0,1) E→ (0,2) E→ off-grid.
        let mask = array_from(2, 3, &[1, 1, 1, 0, 0, 0]);
        let dir = array_from(2, 3, &[1, 1, 1, 0, 0, 0]);
        let order = strahler_order_from_d8(&mask, &dir).expect("strahler computation failed");
        assert_eq!(order[[0, 0]], 1);
        assert_eq!(order[[0, 1]], 1);
        assert_eq!(order[[0, 2]], 1);
    }

    /// A channel cell with `flow_dir == 0` indicates an unfilled sink.
    /// Strahler must return ComputationError, not silently truncate.
    #[test]
    fn test_strahler_unfilled_sink_returns_diagnostic() {
        let mask = array_from(2, 3, &[1, 1, 1, 0, 0, 0]);
        // (0,1) is a sink (dir==0) but on a channel.
        let dir = array_from(2, 3, &[1, 0, 1, 0, 0, 0]);
        let err = strahler_order_from_d8(&mask, &dir).expect_err("expected diagnostic");
        assert!(
            matches!(
                &err,
                TerrainError::ComputationError { message }
                    if (message.contains("unfilled sink") || message.contains("flow_dir == 0"))
                        && message.contains("1")
            ),
            "expected ComputationError mentioning a sink and cell count 1; got: {err:?}"
        );
    }

    /// Confirm the Strahler entry point operates against an arbitrary channel
    /// mask (one whose threshold could plausibly come from a D-inf
    /// accumulation grid) while still using the D8 graph for ordering.
    ///
    /// We synthesise the channel mask + D8 grid directly (rather than running
    /// the full DEM pipeline) so the test isolates the contract: Strahler
    /// honours whatever channel definition we hand it, but resolves topology
    /// strictly through the supplied D8 grid.
    #[test]
    fn test_strahler_dinf_accumulation_threshold_d8_graph() {
        // 3×4 grid. Pretend D-inf produced a channel mask covering only the
        // outermost-east two columns of each row (top 50% by accumulation).
        // The supplied D8 graph wires each row as a linear east-flowing chain.
        let mask = array_from(
            3,
            4,
            &[
                0, 0, 1, 1, //
                0, 0, 1, 1, //
                0, 0, 1, 1,
            ],
        );
        // Channel cells flow east; the rightmost column has its direction
        // ALSO set to E (1). My Strahler treats that as an off-grid outlet
        // because the E step lands outside the raster (no channel edge added).
        // No flow_dir == 0 anywhere → no sink-on-channel error.
        let dir = array_from(
            3,
            4,
            &[
                0, 0, 1, 1, //
                0, 0, 1, 1, //
                0, 0, 1, 1,
            ],
        );
        let order = strahler_order_from_d8(&mask, &dir).expect("strahler computation failed");
        // Each row is an isolated head→outlet chain → all σ=1.
        for r in 0..3 {
            assert_eq!(order[[r, 2]], 1);
            assert_eq!(order[[r, 3]], 1);
            // Off-channel cells stay 0.
            assert_eq!(order[[r, 0]], 0);
            assert_eq!(order[[r, 1]], 0);
        }
    }
}
