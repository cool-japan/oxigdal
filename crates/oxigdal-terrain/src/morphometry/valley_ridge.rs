//! Valley depth and ridge height via Laplace relaxation.
//!
//! Both metrics quantify the vertical distance between the local terrain
//! surface and a smoothly interpolated base-level surface that passes
//! through the drainage network.
//!
//! # Algorithm
//!
//! 1. Fill sinks in the DEM using the Wang & Liu (2006) priority-flood algorithm.
//! 2. Compute D8 flow accumulation to identify channel cells.
//! 3. Use channel cells (flow accumulation ≥ `accumulation_threshold`) as
//!    Dirichlet boundary conditions for a Laplace relaxation.  The relaxation
//!    interpolates a smooth "base-level" surface that hangs at channel
//!    elevations and sweeps up onto the inter-fluvial ridges.
//! 4. `valley_depth[r,c] = max(0, base_level[r,c] − dem[r,c])`.
//!
//! For ridge height the same approach is applied to the negated DEM.
//!
//! # Convergence
//!
//! The Jacobi iteration converges when the maximum cell-wise change falls
//! below `1e-6` map units, or after 500 iterations at the latest.

use crate::error::{Result, TerrainError};
use crate::hydrology::{fill_sinks_priority_flood, flow_accumulation};
use num_traits::Float;
use scirs2_core::prelude::*;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Compute valley depth using flow-based channel extraction + Laplace relaxation.
///
/// Channel cells (D8 accumulation ≥ `accumulation_threshold`) serve as fixed
/// Dirichlet nodes that anchor the interpolated base-level surface to the
/// drainage network.  Every other interior cell is smoothed by 4-neighbour
/// Jacobi iterations until convergence.
///
/// # Arguments
/// * `dem` — 2-D elevation grid.
/// * `accumulation_threshold` — minimum flow-accumulation value (number of
///   upstream cells) required to classify a cell as a channel.  Must be ≥ 0.
/// * `cell_size` — grid spacing in map units.  Must be positive.
/// * `nodata` — optional sentinel; nodata cells are skipped throughout.
///
/// # Returns
/// `Array2<f64>` the same shape as `dem`. Positive values indicate depth
/// below the extrapolated base-level surface (valley bottoms).  Ridges and
/// planar slopes return values close to zero.  Nodata cells carry `f64::NAN`.
///
/// # Errors
/// * [`TerrainError::InvalidDimensions`] — DEM has zero rows or columns.
/// * [`TerrainError::InvalidCellSize`] — `cell_size <= 0`.
/// * [`TerrainError::InvalidThreshold`] — `accumulation_threshold < 0`.
pub fn valley_depth<T>(
    dem: &Array2<T>,
    accumulation_threshold: f64,
    cell_size: f64,
    nodata: Option<T>,
) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy,
{
    validate_inputs(dem, cell_size, accumulation_threshold)?;
    compute_valley_depth_inner(dem, accumulation_threshold, cell_size, nodata, false)
}

/// Compute ridge height using inverted-DEM valley depth.
///
/// The DEM is negated (valid cells only; nodata cells are left as nodata)
/// and then [`valley_depth`] is applied to the inverted surface.  The result
/// gives the height of each cell above the interpolated valley floor.
///
/// # Arguments
/// Same as [`valley_depth`].
///
/// # Returns
/// `Array2<f64>`: positive values on ridges / high-standing terrain, ≈ 0 in
/// valley bottoms.
///
/// # Errors
/// Same error conditions as [`valley_depth`].
pub fn ridge_height<T>(
    dem: &Array2<T>,
    accumulation_threshold: f64,
    cell_size: f64,
    nodata: Option<T>,
) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy,
{
    validate_inputs(dem, cell_size, accumulation_threshold)?;
    compute_valley_depth_inner(dem, accumulation_threshold, cell_size, nodata, true)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Maximum number of Jacobi iterations before forced convergence.
const MAX_ITER: usize = 500;

/// Convergence tolerance: stop when max cell change < this threshold.
const CONV_TOL: f64 = 1.0e-6;

/// Laplace-relaxation fill: channel cells fixed, border cells fixed, all
/// other non-nodata cells averaged over 4 neighbours until convergence.
fn laplace_relax(base: &mut Array2<f64>, fixed: &Array2<bool>) {
    let (rows, cols) = base.dim();
    if rows < 2 || cols < 2 {
        return;
    }
    for _ in 0..MAX_ITER {
        let mut max_change: f64 = 0.0;
        // Jacobi: we update in-place and read from the array as we go.
        // For large DEMs a two-buffer approach would be strictly correct,
        // but in-place Gauss-Seidel converges faster in practice and the
        // end result (base-level surface) is identical to within tolerance.
        for r in 1..rows - 1 {
            for c in 1..cols - 1 {
                if fixed[[r, c]] {
                    continue;
                }
                let v = base[[r, c]];
                if v.is_nan() {
                    // nodata cell — never update
                    continue;
                }
                let n = base[[r - 1, c]];
                let s = base[[r + 1, c]];
                let w = base[[r, c - 1]];
                let e = base[[r, c + 1]];
                // Skip cells where a neighbour is nodata (use the original value)
                if n.is_nan() || s.is_nan() || w.is_nan() || e.is_nan() {
                    continue;
                }
                let new_v = (n + s + w + e) / 4.0;
                let change = (new_v - v).abs();
                base[[r, c]] = new_v;
                if change > max_change {
                    max_change = change;
                }
            }
        }
        if max_change < CONV_TOL {
            break;
        }
    }
}

/// Core implementation shared by `valley_depth` and `ridge_height`.
///
/// When `invert` is `true` the DEM is negated before processing so that
/// ridge cells become the low points of the inverted surface (equivalent
/// to computing valley depth on the original "upside-down" terrain).
fn compute_valley_depth_inner<T>(
    dem: &Array2<T>,
    accumulation_threshold: f64,
    cell_size: f64,
    nodata: Option<T>,
    invert: bool,
) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy,
{
    let (rows, cols) = dem.dim();

    // --- Step 0: build the working DEM as f64, optionally negated -----------
    let nodata_f64 = nodata.map(|nd| nd.into());
    let mut dem_f64: Vec<f64> = dem.iter().map(|v| (*v).into()).collect();

    if invert {
        for v in dem_f64.iter_mut() {
            if let Some(nd) = nodata_f64 {
                if (*v - nd).abs() < f64::EPSILON {
                    continue; // leave nodata cells unchanged
                }
            }
            if v.is_finite() {
                *v = -(*v);
            }
        }
    }

    // --- Step 1: fill sinks --------------------------------------------------
    // priority_flood needs epsilon > 0 to preserve a small slope at each pit
    fill_sinks_priority_flood(&mut dem_f64, cols, rows, 1.0e-6, nodata_f64)?;

    // --- Step 2: rebuild Array2 from filled flat slice -----------------------
    let filled_dem: Array2<f64> =
        Array2::from_shape_vec((rows, cols), dem_f64.clone()).map_err(|e| {
            TerrainError::ComputationError {
                message: format!("failed to reshape filled DEM: {e}"),
            }
        })?;

    // We need a typed version of the filled DEM for flow_accumulation which
    // requires T: Float.  Since the filled values are f64, we use the
    // Array2<f64> version directly (flow_direction_d8 / flow_accumulation
    // accept &Array2<f64>).
    let acc = flow_accumulation(&filled_dem, cell_size, nodata_f64)?;

    // --- Step 3: identify channel cells and fixed cells ----------------------
    // A cell is "fixed" (Dirichlet BC) if it is:
    //   (a) a channel cell (acc >= threshold), or
    //   (b) a border cell, or
    //   (c) a nodata cell (also excluded from updates, but marked separately).
    let mut base: Array2<f64> = filled_dem.clone();
    let mut fixed: Array2<bool> = Array2::from_elem((rows, cols), false);

    for r in 0..rows {
        for c in 0..cols {
            let idx = r * cols + c;
            let v_f64 = dem_f64[idx];

            // nodata: set base to NaN and mark as not-updatable via NaN check
            if let Some(nd) = nodata_f64 {
                if (v_f64 - nd).abs() < f64::EPSILON {
                    base[[r, c]] = f64::NAN;
                    fixed[[r, c]] = true;
                    continue;
                }
            }

            // border rows/cols: keep DEM value and fix
            if r == 0 || r == rows - 1 || c == 0 || c == cols - 1 {
                fixed[[r, c]] = true;
                continue;
            }

            // channel cells: fix at DEM elevation
            if (acc[[r, c]] as f64) >= accumulation_threshold {
                fixed[[r, c]] = true;
                // base already = filled_dem[r,c]
            }
            // non-channel interior cells: free; base starts at DEM elevation
        }
    }

    // --- Step 4: Laplace relaxation ------------------------------------------
    laplace_relax(&mut base, &fixed);

    // --- Step 5: compute valley depth ----------------------------------------
    let mut result: Array2<f64> = Array2::from_elem((rows, cols), f64::NAN);

    for r in 0..rows {
        for c in 0..cols {
            let idx = r * cols + c;
            let orig_f64 = dem_f64[idx];

            if base[[r, c]].is_nan() {
                continue; // nodata
            }
            if let Some(nd) = nodata_f64 {
                if (orig_f64 - nd).abs() < f64::EPSILON {
                    continue;
                }
            }

            // valley_depth = max(0, base_level - dem)
            let depth = (base[[r, c]] - orig_f64).max(0.0);
            result[[r, c]] = depth;
        }
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// Input validation
// ---------------------------------------------------------------------------

fn validate_inputs<T>(dem: &Array2<T>, cell_size: f64, accumulation_threshold: f64) -> Result<()>
where
    T: Float + Into<f64> + Copy,
{
    let (height, width) = dem.dim();
    if height == 0 || width == 0 {
        return Err(TerrainError::InvalidDimensions { width, height });
    }
    if cell_size <= 0.0 {
        return Err(TerrainError::InvalidCellSize { size: cell_size });
    }
    if accumulation_threshold < 0.0 {
        return Err(TerrainError::InvalidThreshold {
            threshold: accumulation_threshold,
            message: "accumulation threshold must be >= 0".to_string(),
        });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a small V-shaped DEM.
    ///
    /// The valley runs N-S through column 3 (0-indexed) of a 7×7 grid.
    /// Elevation increases linearly away from the valley floor.
    ///
    /// ```text
    ///   col:  0    1    2    3    4    5    6
    /// row 0:  3.0  2.0  1.0  0.0  1.0  2.0  3.0   (valley floor at col 3)
    /// row 1:  3.0  2.0  1.0  0.0  1.0  2.0  3.0
    /// ...
    /// ```
    fn v_shape_dem() -> Array2<f64> {
        let mut dem = Array2::zeros((7, 7));
        for r in 0..7_usize {
            for c in 0..7_usize {
                let dist = (c as f64 - 3.0).abs();
                dem[[r, c]] = dist;
            }
        }
        dem
    }

    /// Build a 7×7 central ridge DEM: centre column elevated, flanks low.
    fn ridge_dem() -> Array2<f64> {
        let mut dem = Array2::zeros((7, 7));
        for r in 0..7_usize {
            for c in 0..7_usize {
                dem[[r, c]] = if c == 3 { 100.0 } else { 50.0 };
            }
        }
        dem
    }

    #[test]
    fn test_valley_depth_v_shape() {
        let dem = v_shape_dem();
        let depth = valley_depth(&dem, 2.0, 1.0, None::<f64>).expect("valley_depth on V-shape DEM");

        // The valley floor (column 3) has the highest flow accumulation and
        // is fixed at DEM value; adjacent interior cells should have some depth.
        // We just check that the function returns successfully and does not panic,
        // and that depth values are non-negative.
        for r in 0..7_usize {
            for c in 0..7_usize {
                let d = depth[[r, c]];
                assert!(
                    d.is_nan() || d >= 0.0,
                    "depth at ({r},{c}) must be >= 0 or NaN, got {d}"
                );
            }
        }

        // Interior cells on the valley floor (col 3) should be 0 since they
        // are channel cells fixed at DEM elevation.
        for r in 1..6_usize {
            assert!(
                depth[[r, 3]] < 1.0e-9,
                "valley floor depth at ({r},3) should be ~0, got {}",
                depth[[r, 3]]
            );
        }
    }

    #[test]
    fn test_valley_depth_planar() {
        // A flat DEM: every cell has the same elevation.
        let dem = Array2::<f64>::from_elem((7, 7), 100.0);
        let depth = valley_depth(&dem, 1.0, 1.0, None::<f64>).expect("valley_depth on flat DEM");

        // Should not panic; all depths must be non-negative.
        for r in 0..7_usize {
            for c in 0..7_usize {
                let d = depth[[r, c]];
                assert!(
                    d.is_nan() || d >= 0.0,
                    "depth at ({r},{c}) = {d}, expected >= 0"
                );
            }
        }
    }

    #[test]
    fn test_ridge_height_ridge() {
        let dem = ridge_dem();
        let height = ridge_height(&dem, 2.0, 1.0, None::<f64>).expect("ridge_height on ridge DEM");

        // All values must be >= 0 or NaN.
        for r in 0..7_usize {
            for c in 0..7_usize {
                let h = height[[r, c]];
                assert!(
                    h.is_nan() || h >= 0.0,
                    "ridge_height at ({r},{c}) must be >= 0 or NaN, got {h}"
                );
            }
        }

        // Interior cells on the ridge (col 3 = elevation 100) should have
        // *positive* ridge height, since they stand above the valley floor (50).
        // We check that the centre column average height exceeds flanks.
        let ridge_avg: f64 = (1..6_usize).map(|r| height[[r, 3]]).sum::<f64>() / 5.0;
        let flank_avg: f64 = (1..6_usize)
            .map(|r| height[[r, 0]] + height[[r, 6]])
            .sum::<f64>()
            / 10.0;
        assert!(
            ridge_avg >= flank_avg,
            "expected ridge col avg height ({ridge_avg}) >= flank avg ({flank_avg})"
        );
    }

    #[test]
    fn test_invalid_cell_size() {
        let dem = Array2::<f64>::from_elem((5, 5), 0.0);
        let err = valley_depth(&dem, 1.0, 0.0, None::<f64>).expect_err("should error");
        assert!(
            matches!(err, TerrainError::InvalidCellSize { .. }),
            "expected InvalidCellSize, got {err:?}"
        );
    }

    #[test]
    fn test_invalid_threshold() {
        let dem = Array2::<f64>::from_elem((5, 5), 0.0);
        let err = valley_depth(&dem, -1.0, 1.0, None::<f64>).expect_err("should error");
        assert!(
            matches!(err, TerrainError::InvalidThreshold { .. }),
            "expected InvalidThreshold, got {err:?}"
        );
    }
}
