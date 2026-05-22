//! Topographic Wetness Index (TWI).
//!
//! Beven, K. J. & Kirkby, M. J., *A physically based, variable contributing
//! area model of basin hydrology*, Hydrological Sciences Bulletin 24 (1979),
//! 43–69, define
//!
//! ```text
//! TWI = ln( a / tan β )
//! ```
//!
//! where `a` is the **specific catchment area** (upslope drainage area per
//! unit contour length) and `β` is the local slope angle.  High TWI → wet,
//! valley-bottom-like behaviour; low TWI → ridge-like, well-drained terrain.
//!
//! # Implementation
//!
//! - **Flow accumulation**: D-infinity (Tarboton 1997) via
//!   [`crate::hydrology::flow_accumulation_dinf`]. Fractional flow division
//!   avoids the over-concentrated channel lines D8 produces, which otherwise
//!   biases TWI on hillslopes.
//! - **Slope**: Horn's 3×3 finite difference via
//!   [`crate::derivatives::slope_horn`] in radians (the canonical TWI input).
//! - **Specific catchment area**: `a = (A_total × pixel_area) / contour_width`
//!   where `contour_width` is the cell-side length facing the dominant flow
//!   direction. For D-infinity this is the angle-weighted blend of the two
//!   bracketing facets:
//!
//!   ```text
//!   contour_width = w_e1 · L_e1 + w_e2 · L_e2
//!   ```
//!
//!   with `L_x = cell_size` for cardinal directions and
//!   `L_x = cell_size · √2` for diagonals (TauDEM convention).
//! - **Slope floor**: `tan β` is clamped at `1×10⁻⁴` (≈ 0.006°) so the index
//!   stays finite on perfectly flat areas. The clamp threshold is documented
//!   here in code; callers can post-process the output if they need a
//!   different convention.
//!
//! # Outputs
//!
//! `f64::NAN` is propagated for nodata cells, boundary cells (where the
//! D-infinity stencil cannot be evaluated), and cells whose accumulation
//! came back as a non-finite value (should not happen in practice but
//! defended against).

use crate::derivatives::{SlopeUnits, slope_horn};
use crate::error::{Result, TerrainError};
use crate::hydrology::{flow_accumulation_dinf, flow_direction_dinf};
use num_traits::Float;
use scirs2_core::prelude::*;
use std::f64::consts::{FRAC_PI_4, SQRT_2};

/// Lower bound applied to `tan β` before taking the logarithm.
///
/// Values below this floor (≈ 0.006°) are replaced with the floor itself
/// so TWI stays finite on flat terrain. The exact value is borrowed from
/// SAGA GIS `ta_hydrology.4` (`Wetness Index`), which also clamps slopes
/// at this magnitude.
const TAN_SLOPE_FLOOR: f64 = 1.0e-4;

/// Compute the Topographic Wetness Index for the given DEM.
///
/// # Arguments
///
/// - `dem` — input elevation raster.
/// - `cell_size` — square pixel size (must be positive); same length unit
///   as `dem` elevations.
/// - `nodata` — optional sentinel; matches NaN as well.
///
/// # Returns
///
/// `Array2<f64>` the same shape as `dem`. Cells matching the nodata
/// sentinel — and the outermost row/column where the D-infinity stencil
/// cannot be evaluated — are set to [`f64::NAN`].
///
/// # Errors
///
/// [`TerrainError::InvalidDimensions`] when the DEM is smaller than 3×3,
/// [`TerrainError::InvalidCellSize`] when `cell_size <= 0`,
/// [`TerrainError::ComputationError`] when the underlying flow grid cannot
/// be reshaped (should not occur with valid inputs).
pub fn compute_twi<T>(dem: &Array2<T>, cell_size: f64, nodata: Option<T>) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy,
{
    let (height, width) = dem.dim();
    validate_inputs(dem, cell_size)?;

    // Flatten the DEM to f64 once for D-infinity.
    let dem_f64: Vec<f64> = dem.iter().map(|v| (*v).into()).collect();
    let nodata_f64 = nodata.map(|nd| nd.into());

    // 1. D-infinity flow direction (radians, CCW from east, NaN at boundary/pits).
    let angles = flow_direction_dinf(&dem_f64, width, height, cell_size, nodata_f64);

    // 2. D-infinity flow accumulation (cell counts, fractional split).
    let acc_flat = flow_accumulation_dinf(&dem_f64, &angles, width, height, cell_size);

    // 3. Slope in radians (Horn's method, edge-extended).
    let slope_rad = slope_horn(dem, cell_size, SlopeUnits::Radians, nodata)?;

    // 4. Combine into TWI.
    let mut twi = Array2::from_elem((height, width), f64::NAN);
    let pixel_area = cell_size * cell_size;

    for row in 0..height {
        for col in 0..width {
            // nodata at the centre cell propagates.
            if let Some(nd) = nodata {
                if is_nodata(dem[[row, col]], nd) {
                    continue;
                }
            } else if dem[[row, col]].is_nan() {
                continue;
            }

            let idx = row * width + col;
            let a_cells = acc_flat[idx];
            if !a_cells.is_finite() {
                continue;
            }

            let contour_width = contour_width_dinf(angles[idx], cell_size);

            let a = a_cells * pixel_area / contour_width;

            let slope = slope_rad[[row, col]];
            if !slope.is_finite() {
                continue;
            }

            let tan_slope = slope.tan().max(TAN_SLOPE_FLOOR);

            twi[[row, col]] = (a / tan_slope).ln();
        }
    }

    Ok(twi)
}

/// Compute the angle-blended contour width for a D-infinity flow angle.
///
/// `angle` is in radians, CCW from east, in `[0, 2π)` or `f64::NAN`. NaN
/// (boundary/pit) maps to `cell_size` — these cells have no flow, so the
/// specific catchment area collapses to `pixel_area / cell_size = cell_size`
/// regardless of the angle, which is harmless because the accumulation at
/// such cells is just `1`.
fn contour_width_dinf(angle: f64, cell_size: f64) -> f64 {
    if !angle.is_finite() {
        return cell_size;
    }
    // Identify the bracketing facet [k·π/4, (k+1)·π/4) with k ∈ 0..8.
    let facet = angle / FRAC_PI_4;
    let k = facet.floor() as i64;
    let k_mod = ((k % 8) + 8) % 8; // safe positive modulo
    let alpha = (k_mod as f64) * FRAC_PI_4;
    let theta = angle - alpha; // 0..π/4
    // Linear blend within the facet between the cardinal and diagonal
    // directions. cardinal width = L, diagonal width = L·√2.
    // For even k, the lower-angle neighbour (e1) is cardinal, higher (e2) is diagonal.
    // For odd  k, e1 is diagonal, e2 is cardinal.
    let w_e2 = theta / FRAC_PI_4;
    let w_e1 = 1.0 - w_e2;
    let (l_e1, l_e2) = if k_mod % 2 == 0 {
        (cell_size, cell_size * SQRT_2)
    } else {
        (cell_size * SQRT_2, cell_size)
    };
    w_e1 * l_e1 + w_e2 * l_e2
}

fn validate_inputs<T>(dem: &Array2<T>, cell_size: f64) -> Result<()> {
    let (height, width) = dem.dim();
    if height < 3 || width < 3 {
        return Err(TerrainError::InvalidDimensions { width, height });
    }
    if cell_size <= 0.0 {
        return Err(TerrainError::InvalidCellSize { size: cell_size });
    }
    Ok(())
}

fn is_nodata<T: Float>(value: T, nodata: T) -> bool {
    if value.is_nan() && nodata.is_nan() {
        true
    } else {
        (value - nodata).abs() < T::epsilon()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_uniform_east_slope(
        height: usize,
        width: usize,
        cell: f64,
        slope_rate: f64,
    ) -> Array2<f64> {
        // Higher in the west, lower in the east. Constant slope.
        let mut dem = Array2::zeros((height, width));
        for row in 0..height {
            for col in 0..width {
                dem[[row, col]] = -(col as f64) * cell * slope_rate;
            }
        }
        dem
    }

    #[test]
    fn test_twi_uniform_slope_constant() {
        // For a perfectly uniform slope, all *interior* cells in any given
        // row should receive the same accumulation pattern (modulo D-inf
        // boundary effects). TWI within the deep interior should therefore
        // sit in a tight band — we only assert that nothing blows up and
        // values are within an order of magnitude of each other.
        let cell = 10.0;
        let dem = build_uniform_east_slope(7, 9, cell, 0.05);
        let twi = compute_twi(&dem, cell, None).expect("compute_twi");

        let mut samples = Vec::new();
        for row in 2..5 {
            for col in 2..7 {
                let v = twi[[row, col]];
                assert!(v.is_finite(), "interior TWI [{row},{col}] not finite: {v}");
                samples.push(v);
            }
        }
        let max = samples.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let min = samples.iter().cloned().fold(f64::INFINITY, f64::min);
        // On a uniform slope the accumulation still grows with downslope
        // distance (column index), so TWI naturally varies along x by
        // roughly `ln(A_max / A_min)`. For our 7×9 sample window
        // (cols 2..7, ~6 columns of growth) the expected ln-ratio is
        // ≈ ln(8/3) ≈ 0.98 plus D-inf boundary effects. Allow up to one
        // order of magnitude of variation as a sanity bound (the docstring
        // already calls "an order of magnitude" out as the contract).
        assert!(
            max - min < 2.3,
            "TWI spread too wide on uniform slope: min={min}, max={max}"
        );
    }

    #[test]
    fn test_twi_higher_in_valley_than_ridge() {
        // Build a synthetic V-shaped valley along the central column. Cells
        // along the valley should accumulate flow from both sides → higher
        // TWI than cells near the ridges (left/right edges).
        let h = 11;
        let w = 11;
        let cell = 1.0;
        let mut dem = Array2::zeros((h, w));
        for row in 0..h {
            for col in 0..w {
                // V-valley centred at col=5, with mild downstream slope along row.
                let dist_to_centre = (col as f64 - 5.0).abs();
                // Valley deepens slightly toward the bottom (row index large)
                // so flow has somewhere to go.
                dem[[row, col]] = dist_to_centre + 0.05 * (h - 1 - row) as f64;
            }
        }
        let twi = compute_twi(&dem, cell, None).expect("compute_twi");

        // Sample a deep interior valley cell vs an edge ridge cell.
        let valley = twi[[5, 5]];
        let ridge_l = twi[[5, 1]];
        let ridge_r = twi[[5, 9]];
        assert!(valley.is_finite(), "valley TWI not finite: {valley}");
        assert!(ridge_l.is_finite(), "ridge-L TWI not finite: {ridge_l}");
        assert!(ridge_r.is_finite(), "ridge-R TWI not finite: {ridge_r}");
        assert!(
            valley > ridge_l + 0.1,
            "valley TWI ({valley}) should exceed left ridge ({ridge_l})"
        );
        assert!(
            valley > ridge_r + 0.1,
            "valley TWI ({valley}) should exceed right ridge ({ridge_r})"
        );
    }

    #[test]
    fn test_twi_flat_cell_clamped_finite() {
        // A perfectly flat plateau has no slope; TWI must stay finite via
        // the tan(slope) floor. The clamp is `TAN_SLOPE_FLOOR = 1e-4`, so
        // TWI = ln(pixel_area · acc / cell_size / 1e-4).
        let cell = 1.0;
        let dem = Array2::<f64>::from_elem((7, 7), 100.0);
        let twi = compute_twi(&dem, cell, None).expect("compute_twi");
        for row in 0..7 {
            for col in 0..7 {
                let v = twi[[row, col]];
                assert!(
                    v.is_finite(),
                    "flat-plateau TWI must be finite at ({row},{col}), got {v}"
                );
            }
        }
        // Order-of-magnitude check: floor → tan ≈ 1e-4, accum = 1, so
        // TWI ≈ ln(1 · 1 / 1 / 1e-4) ≈ ln(1e4) ≈ 9.21.
        let centre = twi[[3, 3]];
        assert!(
            centre > 8.0 && centre < 11.0,
            "flat-plateau centre TWI ≈ ln(1e4) ≈ 9.21 expected, got {centre}"
        );
    }

    #[test]
    fn test_twi_nodata_propagates_to_nan() {
        let cell = 1.0;
        let mut dem = Array2::<f64>::from_elem((5, 5), 50.0);
        // Mark a single interior cell as nodata.
        let nd = -9999.0;
        dem[[2, 2]] = nd;
        let twi = compute_twi(&dem, cell, Some(nd)).expect("compute_twi");
        assert!(
            twi[[2, 2]].is_nan(),
            "nodata cell must propagate NaN, got {}",
            twi[[2, 2]]
        );
    }

    #[test]
    fn test_twi_d8_vs_dinf_consistency_smoke() {
        // We don't have a public D8 TWI helper, so this smoke test
        // exercises the contour-width helper across both even and odd
        // facets and confirms the magnitudes are in the documented range
        // [L, L·√2]. (Spatial pattern fidelity is covered by other tests.)
        let cell = 5.0;
        let east = contour_width_dinf(0.0, cell);
        let ne_diag = contour_width_dinf(FRAC_PI_4, cell); // boundary E↔NE
        let ne_mid = contour_width_dinf(FRAC_PI_4 * 0.5, cell);
        // East: pure cardinal → cell_size.
        assert!(
            (east - cell).abs() < 1e-12,
            "east contour width {east} ≠ {cell}"
        );
        // Pure diagonal → cell_size * √2 (we hit the *upper* facet boundary,
        // which has e1=diagonal cardinal blend → max width).
        assert!(
            (ne_diag - cell * SQRT_2).abs() < 1e-12,
            "diagonal contour width {ne_diag} ≠ {}",
            cell * SQRT_2
        );
        // Halfway through the first facet → strict interior of [L, L·√2].
        assert!(
            ne_mid > cell && ne_mid < cell * SQRT_2,
            "mid-facet contour width {ne_mid} not strictly between L and L√2"
        );

        // Now run TWI end-to-end on a small ramp and confirm finite output.
        let dem = build_uniform_east_slope(5, 5, cell, 0.02);
        let twi = compute_twi(&dem, cell, None).expect("compute_twi");
        // Interior cells should be finite.
        for row in 1..4 {
            for col in 1..4 {
                let v = twi[[row, col]];
                assert!(v.is_finite(), "TWI interior [{row},{col}] not finite: {v}");
            }
        }
    }

    #[test]
    fn test_twi_invalid_dimensions_errors() {
        let dem = Array2::<f64>::zeros((2, 4));
        let err = compute_twi(&dem, 1.0, None).expect_err("must error");
        matches!(err, TerrainError::InvalidDimensions { .. });
    }

    #[test]
    fn test_twi_invalid_cell_size_errors() {
        let dem = Array2::<f64>::zeros((5, 5));
        let err = compute_twi(&dem, -1.0, None).expect_err("must error");
        matches!(err, TerrainError::InvalidCellSize { .. });
    }
}
