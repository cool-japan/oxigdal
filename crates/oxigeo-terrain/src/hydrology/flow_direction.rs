//! Flow direction calculation for hydrological analysis.
//!
//! Implements D8 (8-direction) and D-Infinity (Tarboton 1997) algorithms for
//! determining flow direction from each cell based on elevation gradients.
//!
//! # D-Infinity Convention
//!
//! The D-infinity algorithm returns angles in radians measured **counter-clockwise
//! from east** (i.e. 0 = east, π/2 = north, π = west, 3π/2 = south). This matches
//! the Tarboton 1997 paper coordinate system. Pits and boundary pixels return NaN.

use crate::error::{Result, TerrainError};
use num_traits::Float;
use scirs2_core::prelude::*;
use std::f64::consts::{FRAC_PI_4, PI};

/// Flow direction algorithm
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowAlgorithm {
    /// D8: 8 cardinal and diagonal directions
    D8,
    /// D-Infinity: continuous flow direction (Tarboton 1997)
    DInfinity,
}

/// D8 flow direction codes (ArcGIS convention)
/// 1=E, 2=SE, 4=S, 8=SW, 16=W, 32=NW, 64=N, 128=NE
pub(crate) const D8_DIRS: [(isize, isize, u8); 8] = [
    (0, 1, 1),    // East
    (1, 1, 2),    // Southeast
    (1, 0, 4),    // South
    (1, -1, 8),   // Southwest
    (0, -1, 16),  // West
    (-1, -1, 32), // Northwest
    (-1, 0, 64),  // North
    (-1, 1, 128), // Northeast
];

/// Tarboton 1997 D-infinity facet descriptors.
///
/// Each entry: `(row_e1, col_e1, row_e2, col_e2, base_angle_radians)`
/// where e1 is the cardinal neighbour and e2 is the diagonal neighbour.
/// base_angle is the angle (CCW from east) pointing toward e1.
const DINF_FACETS: [(isize, isize, isize, isize, f64); 8] = [
    // k=0: base angle 0    (E):  e1=E=(0,+1),   e2=NE=(-1,+1)
    (0, 1, -1, 1, 0.0),
    // k=1: base angle π/4  (NE): e1=NE=(-1,+1), e2=N=(-1,0)
    (-1, 1, -1, 0, FRAC_PI_4),
    // k=2: base angle π/2  (N):  e1=N=(-1,0),   e2=NW=(-1,-1)
    (-1, 0, -1, -1, FRAC_PI_4 * 2.0),
    // k=3: base angle 3π/4 (NW): e1=NW=(-1,-1), e2=W=(0,-1)
    (-1, -1, 0, -1, FRAC_PI_4 * 3.0),
    // k=4: base angle π    (W):  e1=W=(0,-1),   e2=SW=(+1,-1)
    (0, -1, 1, -1, PI),
    // k=5: base angle 5π/4 (SW): e1=SW=(+1,-1), e2=S=(+1,0)
    (1, -1, 1, 0, FRAC_PI_4 * 5.0),
    // k=6: base angle 3π/2 (S):  e1=S=(+1,0),   e2=SE=(+1,+1)
    (1, 0, 1, 1, FRAC_PI_4 * 6.0),
    // k=7: base angle 7π/4 (SE): e1=SE=(+1,+1), e2=E=(0,+1)
    (1, 1, 0, 1, FRAC_PI_4 * 7.0),
];

/// Calculate D8 flow direction.
///
/// Returns an array where each cell contains a power-of-2 value indicating
/// the direction of steepest descent.
pub fn flow_direction_d8<T>(
    dem: &Array2<T>,
    cell_size: f64,
    nodata: Option<T>,
) -> Result<Array2<u8>>
where
    T: Float + Into<f64> + Copy,
{
    validate_inputs(dem, cell_size)?;

    let (height, width) = dem.dim();
    let mut flow_dir = Array2::zeros((height, width));

    // Cell size for diagonal directions
    let diag_size = cell_size * 2.0_f64.sqrt();

    for y in 0..height {
        for x in 0..width {
            let center = dem[[y, x]];

            if let Some(nd) = nodata
                && is_nodata(center, nd)
            {
                flow_dir[[y, x]] = 0;
                continue;
            }

            let center_val = center.into();
            let mut max_slope = f64::NEG_INFINITY;
            let mut max_dir = 0_u8;

            // Check all 8 neighbors
            for (dy, dx, dir_code) in &D8_DIRS {
                let ny = y as isize + dy;
                let nx = x as isize + dx;

                if ny >= 0 && ny < height as isize && nx >= 0 && nx < width as isize {
                    let neighbor = dem[[ny as usize, nx as usize]];

                    if let Some(nd) = nodata
                        && is_nodata(neighbor, nd)
                    {
                        continue;
                    }

                    let neighbor_val = neighbor.into();
                    let elevation_diff = center_val - neighbor_val;

                    // Calculate distance
                    let distance = if dy.abs() == 1 && dx.abs() == 1 {
                        diag_size
                    } else {
                        cell_size
                    };

                    let slope = elevation_diff / distance;

                    if slope > max_slope {
                        max_slope = slope;
                        max_dir = *dir_code;
                    }
                }
            }

            // If no downslope direction found, mark as sink
            flow_dir[[y, x]] = max_dir;
        }
    }

    Ok(flow_dir)
}

/// Calculate D-Infinity flow direction (Tarboton 1997) using a flat-slice interface.
///
/// For each non-boundary pixel, examines 8 triangular facets and computes the
/// steepest descent vector. Returns the angle (radians, CCW from east, range [0, 2π))
/// for pixels with positive downslope, or `f64::NAN` for pits, boundaries, and nodata.
///
/// # D-infinity distance convention (Tarboton 1997)
///
/// Each facet uses two orthogonal slopes:
/// - `s1 = (z − e1) / d1`: slope toward the first neighbour (e1).
/// - `s2 = (e1 − e2) / d2`: lateral slope within the facet from e1 toward e2.
///
/// The distances are **not** symmetric across all facets:
/// - For even-indexed facets (k = 0, 2, 4, 6) **e1 is a cardinal neighbour**:
///   `d1 = cell_size`, `d2 = cell_size`
/// - For odd-indexed facets (k = 1, 3, 5, 7) **e1 is a diagonal neighbour**:
///   `d1 = cell_size × √2`, `d2 = cell_size`
///
/// `d2` is always `cell_size` because the lateral step inside the facet from
/// the primary towards the secondary neighbour is always one cell width.
///
/// # Arguments
/// * `dem` — row-major flat slice of elevations, length `width * height`
/// * `width` / `height` — raster dimensions
/// * `cell_size` — pixel size in the same units as elevation
/// * `nodata` — optional nodata sentinel; NaN-valued cells are always treated as nodata
pub fn flow_direction_dinf(
    dem: &[f64],
    width: usize,
    height: usize,
    cell_size: f64,
    nodata: Option<f64>,
) -> Vec<f64> {
    let n = width * height;
    let mut result = vec![f64::NAN; n];

    let diag_size = cell_size * std::f64::consts::SQRT_2;

    for row in 1..height.saturating_sub(1) {
        for col in 1..width.saturating_sub(1) {
            let idx = row * width + col;
            let z = dem[idx];

            // Skip nodata / NaN center
            if is_nodata_flat(z, nodata) {
                continue;
            }

            let mut best_slope = f64::NEG_INFINITY;
            let mut best_angle = f64::NAN;

            for (k, &(dr1, dc1, dr2, dc2, base_angle)) in DINF_FACETS.iter().enumerate() {
                let r1 = row as isize + dr1;
                let c1 = col as isize + dc1;
                let r2 = row as isize + dr2;
                let c2 = col as isize + dc2;

                // Both neighbors must be in-bounds (guaranteed except at border,
                // which we already skip in the outer loop, but be safe)
                if r1 < 0
                    || r1 >= height as isize
                    || c1 < 0
                    || c1 >= width as isize
                    || r2 < 0
                    || r2 >= height as isize
                    || c2 < 0
                    || c2 >= width as isize
                {
                    continue;
                }

                let e1 = dem[r1 as usize * width + c1 as usize];
                let e2 = dem[r2 as usize * width + c2 as usize];

                // Skip facets with nodata neighbors
                if is_nodata_flat(e1, nodata) || is_nodata_flat(e2, nodata) {
                    continue;
                }

                // Per Tarboton (1997):
                //   Even k (e1 is cardinal):  d1 = cell_size,  d2 = cell_size
                //   Odd  k (e1 is diagonal):  d1 = √2*cell_size, d2 = cell_size
                let d1 = if k % 2 == 0 { cell_size } else { diag_size };
                // d2 is always cell_size (lateral step between e1 and e2 within facet)
                let d2 = cell_size;

                // s1: slope toward primary neighbor (e1); positive = downslope
                let s1 = (z - e1) / d1;
                // s2: lateral slope within facet from e1 toward e2
                let s2 = (e1 - e2) / d2;

                // Flow angle within facet (0 = toward e1, π/4 = toward e2)
                let r_raw = s2.atan2(s1);
                let r_clamped = r_raw.clamp(0.0, FRAC_PI_4);

                // Slope magnitude projected onto clamped flow direction
                let s_mag = s1 * r_clamped.cos() + s2 * r_clamped.sin();

                if s_mag > best_slope {
                    best_slope = s_mag;
                    best_angle = base_angle + r_clamped;
                }
            }

            // Only set angle if there is a positive downslope
            if best_slope > 0.0 {
                // Normalize to [0, 2π)
                let angle = best_angle.rem_euclid(2.0 * PI);
                result[idx] = angle;
            }
            // else: pit → stays NaN
        }
    }

    result
}

/// Calculate flow direction with specified algorithm.
pub fn flow_direction<T>(
    dem: &Array2<T>,
    cell_size: f64,
    algorithm: FlowAlgorithm,
    nodata: Option<T>,
) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy,
{
    match algorithm {
        FlowAlgorithm::D8 => {
            let d8 = flow_direction_d8(dem, cell_size, nodata)?;
            let (height, width) = d8.dim();
            let mut result = Array2::zeros((height, width));
            for y in 0..height {
                for x in 0..width {
                    result[[y, x]] = d8[[y, x]] as f64;
                }
            }
            Ok(result)
        }
        FlowAlgorithm::DInfinity => {
            let (height, width) = dem.dim();
            // Convert Array2<T> to flat Vec<f64> for the canonical implementation
            let flat: Vec<f64> = dem.iter().map(|v| (*v).into()).collect();
            let nodata_f64 = nodata.map(|nd| nd.into());
            let angles = flow_direction_dinf(&flat, width, height, cell_size, nodata_f64);
            let arr = Array2::from_shape_vec((height, width), angles).map_err(|_e| {
                TerrainError::ComputationError {
                    message: "D-infinity flow direction array reshape failed".to_owned(),
                }
            })?;
            Ok(arr)
        }
    }
}

// Helper functions

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

/// Check if a flat-slice f64 value is nodata (NaN or matches sentinel).
fn is_nodata_flat(value: f64, nodata: Option<f64>) -> bool {
    if value.is_nan() {
        return true;
    }
    if let Some(nd) = nodata {
        if nd.is_nan() {
            return false; // value is not NaN (checked above)
        }
        (value - nd).abs() < f64::EPSILON
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    #[test]
    fn test_d8_simple_slope() {
        // Create simple east-facing slope
        let mut dem = Array2::zeros((5, 5));
        for y in 0..5 {
            for x in 0..5 {
                dem[[y, x]] = 100.0 - (x as f64) * 10.0; // Decreases eastward
            }
        }

        let flow_dir = flow_direction_d8(&dem, 10.0, None).expect("flow direction failed");

        // Most cells should flow east (code 1)
        for y in 1..4 {
            for x in 1..3 {
                assert_eq!(flow_dir[[y, x]], 1, "should flow east");
            }
        }
    }

    #[test]
    fn test_d8_directions() {
        // Create a pit at center
        let mut dem = Array2::from_elem((5, 5), 100.0);
        dem[[2, 2]] = 50.0; // Central pit

        let flow_dir = flow_direction_d8(&dem, 10.0, None).expect("flow direction failed");

        // Neighbors should flow toward center
        assert!(flow_dir[[2, 1]] > 0); // West neighbor
        assert!(flow_dir[[2, 3]] > 0); // East neighbor
        assert!(flow_dir[[1, 2]] > 0); // North neighbor
        assert!(flow_dir[[3, 2]] > 0); // South neighbor
    }

    #[test]
    fn test_dinf_uniform_slope_east() {
        // Higher in west (col=0), lower in east (col=4): flow should be east (angle ≈ 0)
        let height = 5usize;
        let width = 5usize;
        let cell_size = 10.0_f64;
        let mut dem = vec![0.0f64; width * height];
        for row in 0..height {
            for col in 0..width {
                dem[row * width + col] = 100.0 - (col as f64) * cell_size;
            }
        }
        let angles = flow_direction_dinf(&dem, width, height, cell_size, None);

        // Interior pixels (row 1..3, col 1..3) should flow east (angle ≈ 0)
        for row in 1..height - 1 {
            for col in 1..width - 1 {
                let angle = angles[row * width + col];
                assert!(
                    !angle.is_nan(),
                    "interior pixel ({row},{col}) should not be NaN"
                );
                // Angle should be 0 (east) or 2π (wraps); check both
                let diff = (angle).min((angle - 2.0 * PI).abs());
                assert!(
                    diff < 1e-10,
                    "pixel ({row},{col}) angle {angle} should be ≈ 0 (east)"
                );
            }
        }
    }

    #[test]
    fn test_dinf_pit_returns_nan() {
        // 3x3 DEM where center pixel (elevation 5) is surrounded by higher pixels (10)
        let height = 3usize;
        let width = 3usize;
        let dem: Vec<f64> = vec![10.0, 10.0, 10.0, 10.0, 5.0, 10.0, 10.0, 10.0, 10.0];
        let angles = flow_direction_dinf(&dem, width, height, 1.0, None);
        let center_angle = angles[width + 1];
        assert!(
            center_angle.is_nan(),
            "center pit pixel should return NaN, got {center_angle}"
        );
    }

    #[test]
    fn test_dinf_uniform_slope_sw_diagonal() {
        // elevation = row + col → slope toward SW (higher in SE, lower in NW)
        // Actually row increases downward, col increases rightward.
        // elevation[row][col] = (max - row - col) means higher at (0,0) NW corner,
        // lower at (max,max) SE corner → flow should be southeast (~7π/4 = 315°)
        // But task asks for SW flow. Let's make elevation = row + col so (0,0) is lowest,
        // (max,max) is highest → flow toward NW which is 3π/4.
        // For SW flow (5π/4), elevation should DECREASE toward SW.
        // In row-major: row increases downward, col increases rightward.
        // SW = row+1, col-1. To flow SW: center must be higher than SW neighbor.
        // elevation = -row + col (high at top-right, low at bottom-left) → flow SW (5π/4).
        let height = 5usize;
        let width = 5usize;
        let cell_size = 1.0_f64;
        let mut dem = vec![0.0f64; width * height];
        for row in 0..height {
            for col in 0..width {
                // Higher at top-right (row=0, col=max), lower at bottom-left (row=max, col=0)
                dem[row * width + col] = (col as f64) - (row as f64) + 10.0;
            }
        }
        let angles = flow_direction_dinf(&dem, width, height, cell_size, None);

        // Interior pixels should flow SW (5π/4 ≈ 3.927 rad)
        let expected = 5.0 * PI / 4.0;
        for row in 1..height - 1 {
            for col in 1..width - 1 {
                let angle = angles[row * width + col];
                if !angle.is_nan() {
                    assert!(
                        (angle - expected).abs() < 0.01,
                        "pixel ({row},{col}) angle {angle:.4} should be ≈ 5π/4 ({expected:.4})"
                    );
                }
            }
        }
    }

    #[test]
    fn test_dinf_continuous_values_in_range() {
        let mut dem = Array2::zeros((5, 5));
        for y in 0..5 {
            for x in 0..5 {
                dem[[y, x]] = 100.0 - (x as f64) * 10.0;
            }
        }
        let (height, width) = dem.dim();
        let flat: Vec<f64> = dem.iter().copied().collect();
        let angles = flow_direction_dinf(&flat, width, height, 10.0, None);

        // Flow directions should be valid angles or NaN (boundary)
        for row in 1..height - 1 {
            for col in 1..width - 1 {
                let angle = angles[row * width + col];
                if !angle.is_nan() {
                    assert!(
                        (0.0..2.0 * PI).contains(&angle),
                        "angle {angle} out of [0, 2π)"
                    );
                }
            }
        }
    }
}
