//! Depression (sink) filling algorithms.
//!
//! Provides two implementations:
//! - [`fill_sinks_priority_flood`]: Wang & Liu (2006) O(n log n) priority-flood — primary export.
//! - [`fill_sinks_iterative`]: renamed legacy implementation (kept for parity / reference).
//! - [`fill_sinks`]: alias for `fill_sinks_priority_flood` on flat-slice inputs.

use crate::error::{Result, TerrainError};
use num_traits::Float;
use ordered_float::NotNan;
use scirs2_core::prelude::*;
use std::cmp::Reverse;
use std::collections::BinaryHeap;

// ---------------------------------------------------------------------------
// Wang & Liu (2006) priority-flood — primary implementation (flat-slice API)
// ---------------------------------------------------------------------------

/// Fill depressions using the Wang & Liu (2006) priority-flood algorithm.
///
/// This is an in-place, slope-preserving implementation. Each unfilled sink
/// pixel is raised to its spillway elevation plus `epsilon` to produce a tiny
/// outward gradient, ensuring that a subsequent D8 or D-infinity flow direction
/// pass will yield a connected drainage network rather than flat regions.
///
/// # Algorithm
/// 1. Push all boundary pixels into a min-heap keyed by elevation; mark visited.
/// 2. Pop the lowest pixel, visit all 8-connected unvisited non-nodata neighbours:
///    - Raise the neighbour to `max(dem[neighbour], dem[current] + epsilon)`.
///    - Mark it visited and push with its new elevation.
/// 3. After exhaustion, all internal depressions have been filled.
///
/// # Arguments
/// * `dem`     — mutable flat row-major slice of elevations
/// * `width`   / `height` — raster dimensions
/// * `epsilon` — slope-preserving increment (default 1e-9); increase to 1e-6 for f32-origin data
/// * `nodata`  — optional nodata sentinel; nodata pixels are skipped entirely
///
/// # Errors
/// Returns an error only if a non-nodata cell contains a NaN elevation while
/// constructing the heap key (which should not occur with well-formed input).
pub fn fill_sinks_priority_flood(
    dem: &mut [f64],
    width: usize,
    height: usize,
    epsilon: f64,
    nodata: Option<f64>,
) -> Result<()> {
    let n = width * height;
    let mut visited = vec![false; n];

    // Min-heap: Reverse(NotNan) gives smallest-first ordering
    let mut heap: BinaryHeap<(Reverse<NotNan<f64>>, usize)> = BinaryHeap::new();

    // Push all boundary pixels
    for row in 0..height {
        for col in 0..width {
            if row == 0 || row == height - 1 || col == 0 || col == width - 1 {
                let idx = row * width + col;
                let elev = dem[idx];
                if is_nodata_f64(elev, nodata) {
                    continue;
                }
                let nn = NotNan::new(elev).map_err(|_| TerrainError::InvalidNoData {
                    message: format!(
                        "NaN elevation at boundary cell ({row},{col}) while filling sinks"
                    ),
                })?;
                heap.push((Reverse(nn), idx));
                visited[idx] = true;
            }
        }
    }

    // 8-connected neighbour offsets
    const NEIGHBORS: [(isize, isize); 8] = [
        (-1, -1),
        (-1, 0),
        (-1, 1),
        (0, -1),
        (0, 1),
        (1, -1),
        (1, 0),
        (1, 1),
    ];

    while let Some((Reverse(current_nn), idx)) = heap.pop() {
        let current_elev = *current_nn;
        let row = idx / width;
        let col = idx % width;

        for (dr, dc) in NEIGHBORS {
            let nr = row as isize + dr;
            let nc = col as isize + dc;
            if nr < 0 || nr >= height as isize || nc < 0 || nc >= width as isize {
                continue;
            }
            let nidx = nr as usize * width + nc as usize;
            if visited[nidx] {
                continue;
            }
            if is_nodata_f64(dem[nidx], nodata) {
                visited[nidx] = true; // skip nodata permanently
                continue;
            }

            // Raise neighbour to spillway elevation + epsilon if it would create a sink
            let new_elev = f64::max(dem[nidx], current_elev + epsilon);
            dem[nidx] = new_elev;

            let nn = NotNan::new(new_elev).map_err(|_| TerrainError::InvalidNoData {
                message: format!(
                    "NaN produced when filling sink at ({nr},{nc}); original elevation was {}",
                    dem[nidx]
                ),
            })?;
            heap.push((Reverse(nn), nidx));
            visited[nidx] = true;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Convenience wrapper: Array2 interface for fill_sinks_priority_flood
// ---------------------------------------------------------------------------

/// Fill depressions in a DEM using Wang & Liu (2006) priority-flood.
///
/// Returns a new `Array2<f64>` with all sinks filled. The original array is
/// not modified. See [`fill_sinks_priority_flood`] for algorithm details.
pub fn fill_sinks<T>(dem: &Array2<T>, nodata: Option<T>) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy,
{
    let (height, width) = dem.dim();
    let mut flat: Vec<f64> = dem.iter().map(|v| (*v).into()).collect();
    let nodata_f64 = nodata.map(|nd| nd.into());
    fill_sinks_priority_flood(&mut flat, width, height, 1e-9, nodata_f64)?;
    Array2::from_shape_vec((height, width), flat).map_err(|_| TerrainError::ComputationError {
        message: "sink fill array reshape failed".to_owned(),
    })
}

// ---------------------------------------------------------------------------
// Legacy iterative variant (renamed from original fill_sinks)
// ---------------------------------------------------------------------------

/// Legacy priority-flood implementation (renamed from original `fill_sinks`).
///
/// **Note:** This implementation uses a max-heap which processes cells in
/// descending order; it is preserved for reference / parity testing only.
/// Prefer [`fill_sinks_priority_flood`] for new code.
pub fn fill_sinks_iterative<T>(dem: &Array2<T>, nodata: Option<T>) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy,
{
    let (height, width) = dem.dim();
    let mut filled = Array2::from_elem((height, width), f64::INFINITY);
    let mut closed = Array2::from_elem((height, width), false);
    let mut open: BinaryHeap<LegacyCell> = BinaryHeap::new();

    // Initialize with edge cells
    for y in 0..height {
        for x in 0..width {
            if y == 0 || y == height - 1 || x == 0 || x == width - 1 {
                let val = dem[[y, x]];
                if let Some(nd) = nodata {
                    if (val - nd).abs() < T::epsilon() {
                        continue;
                    }
                }
                filled[[y, x]] = val.into();
                open.push(LegacyCell {
                    y,
                    x,
                    elevation: val.into(),
                });
            }
        }
    }

    // Iterative flood
    while let Some(cell) = open.pop() {
        if closed[[cell.y, cell.x]] {
            continue;
        }
        closed[[cell.y, cell.x]] = true;

        for dy in -1..=1_isize {
            for dx in -1..=1_isize {
                if dy == 0 && dx == 0 {
                    continue;
                }
                let ny = cell.y as isize + dy;
                let nx = cell.x as isize + dx;
                if ny < 0 || ny >= height as isize || nx < 0 || nx >= width as isize {
                    continue;
                }
                let ny = ny as usize;
                let nx = nx as usize;
                if closed[[ny, nx]] {
                    continue;
                }
                let neighbor_elev: f64 = dem[[ny, nx]].into();
                let new_elev = neighbor_elev.max(cell.elevation);
                if new_elev < filled[[ny, nx]] {
                    filled[[ny, nx]] = new_elev;
                    open.push(LegacyCell {
                        y: ny,
                        x: nx,
                        elevation: new_elev,
                    });
                }
            }
        }
    }

    Ok(filled)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn is_nodata_f64(value: f64, nodata: Option<f64>) -> bool {
    if value.is_nan() {
        return true;
    }
    match nodata {
        Some(nd) if !nd.is_nan() => (value - nd).abs() < f64::EPSILON,
        _ => false,
    }
}

// Legacy helper cell for `fill_sinks_iterative`
#[derive(Copy, Clone, PartialEq)]
struct LegacyCell {
    y: usize,
    x: usize,
    elevation: f64,
}

impl Eq for LegacyCell {}

impl PartialOrd for LegacyCell {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for LegacyCell {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        // Uses total ordering via partial_cmp; NaN elevation would panic here
        // but is prevented by the nodata-skip logic in fill_sinks_iterative.
        self.elevation
            .partial_cmp(&other.elevation)
            .unwrap_or(core::cmp::Ordering::Equal)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_flood_simple_pit() {
        // 3×3 DEM: all cells 100.0 except center = 50.0 (a pit)
        // After fill, center should be ≈ 100.0 + epsilon (spillway + gradient)
        let mut dem = vec![100.0_f64; 9];
        dem[4] = 50.0; // center pit: row=1, col=1 → 1*3+1 = 4

        fill_sinks_priority_flood(&mut dem, 3, 3, 1e-9, None)
            .expect("priority flood should succeed");

        let center = dem[4];
        // Center must be at least as high as its boundary neighbours (100.0)
        assert!(
            center >= 100.0,
            "filled center {center} should be ≥ 100.0 (spillway)"
        );
        // And only marginally above (epsilon chain from boundary to center is short)
        assert!(
            center < 100.0 + 1e-6,
            "filled center {center} should be close to 100.0"
        );
    }

    #[test]
    fn test_priority_flood_complex_basin() {
        // 5×5 DEM:
        //   All border cells  = 100.0
        //   Inner ring (rows/cols 1 & 3) = 60.0 except two pour-point cells
        //   Two pour-point cells at (1,2) and (3,2) = 80.0
        //   Center cell (2,2) = 10.0
        let height = 5usize;
        let width = 5usize;
        let mut dem = vec![100.0_f64; height * width];

        // Inner ring = 60
        for r in 1..4 {
            for c in 1..4 {
                dem[r * width + c] = 60.0;
            }
        }
        // Pour points slightly higher: (row=1,col=2) and (row=3,col=2)
        dem[width + 2] = 80.0;
        dem[3 * width + 2] = 80.0;
        // Deep center
        dem[2 * width + 2] = 10.0;

        fill_sinks_priority_flood(&mut dem, width, height, 1e-9, None)
            .expect("priority flood should succeed");

        let center = dem[2 * width + 2];
        // Center must be raised to at least the surrounding cells
        assert!(
            center >= 60.0,
            "center {center} should be ≥ inner-ring elevation 60.0"
        );
    }

    #[test]
    fn test_priority_flood_preserves_non_sink_pixels() {
        // Monotonically decreasing DEM (no pits): no elevation should change
        let height = 5usize;
        let width = 5usize;
        let mut dem: Vec<f64> = (0..(height * width))
            .map(|i| {
                let row = i / width;
                let col = i % width;
                100.0 - (row + col) as f64
            })
            .collect();
        let original = dem.clone();

        fill_sinks_priority_flood(&mut dem, width, height, 1e-9, None)
            .expect("priority flood should succeed");

        for i in 0..dem.len() {
            assert!(
                (dem[i] - original[i]).abs() < 1e-6,
                "pixel {i}: elevation changed from {} to {} on a pit-free DEM",
                original[i],
                dem[i]
            );
        }
    }

    #[test]
    fn test_priority_flood_no_sinks_no_change() {
        // Verify no pixel changes by more than epsilon on a pit-free DEM
        let height = 4usize;
        let width = 4usize;
        let epsilon = 1e-9_f64;
        let mut dem: Vec<f64> = (0..(height * width))
            .map(|i| {
                let row = i / width;
                let col = i % width;
                // Strictly decreasing so no sinks: elevation = max - row - col
                50.0 - (row + col) as f64
            })
            .collect();
        let original = dem.clone();

        fill_sinks_priority_flood(&mut dem, width, height, epsilon, None)
            .expect("priority flood should succeed");

        for i in 0..dem.len() {
            assert!(
                (dem[i] - original[i]).abs() <= epsilon + f64::EPSILON,
                "pixel {i}: elevation shifted by more than epsilon on a pit-free DEM"
            );
        }
    }

    #[test]
    fn test_fill_sinks_array_interface() {
        // Smoke test for the Array2 convenience wrapper
        let mut dem = Array2::from_elem((5, 5), 100.0_f64);
        dem[[2, 2]] = 50.0;
        let filled = fill_sinks(&dem, None).expect("sink fill failed");
        assert_eq!(filled.dim(), (5, 5));
        assert!(filled[[2, 2]] >= 100.0);
    }
}
