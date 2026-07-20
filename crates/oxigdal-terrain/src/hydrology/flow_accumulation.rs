//! Flow accumulation calculation.
//!
//! Provides D8 accumulation (`flow_accumulation`) and D-Infinity fractional
//! accumulation (`flow_accumulation_dinf`).

use crate::error::Result;
use crate::hydrology::flow_direction::{D8_DIRS, flow_direction_d8};
use num_traits::Float;
use ordered_float::NotNan;
use scirs2_core::prelude::*;
use std::collections::BinaryHeap;
use std::f64::consts::FRAC_PI_4;

/// Calculate flow accumulation from D8 flow direction.
pub fn flow_accumulation<T>(
    dem: &Array2<T>,
    cell_size: f64,
    nodata: Option<T>,
) -> Result<Array2<u32>>
where
    T: Float + Into<f64> + Copy,
{
    let (height, width) = dem.dim();
    let flow_dir = flow_direction_d8(dem, cell_size, nodata)?;
    let mut accumulation = Array2::zeros((height, width));

    // Count upstream cells for each cell
    for y in 0..height {
        for x in 0..width {
            accumulation[[y, x]] = 1; // Each cell contributes 1
        }
    }

    // Process cells from highest to lowest elevation
    let mut cells: Vec<(usize, usize, f64)> = Vec::new();
    for y in 0..height {
        for x in 0..width {
            if let Some(nd) = nodata
                && (dem[[y, x]] - nd).abs() < T::epsilon()
            {
                continue;
            }
            cells.push((y, x, dem[[y, x]].into()));
        }
    }
    cells.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(core::cmp::Ordering::Equal));

    // Accumulate flow
    for (y, x, _) in cells {
        let dir = flow_dir[[y, x]];
        if dir == 0 {
            continue; // Sink or no data
        }

        // Find downstream cell
        if let Some((dy, dx, _)) = D8_DIRS.iter().find(|(_, _, code)| *code == dir) {
            let ny = (y as isize + dy) as usize;
            let nx = (x as isize + dx) as usize;
            if ny < height && nx < width {
                accumulation[[ny, nx]] += accumulation[[y, x]];
            }
        }
    }

    Ok(accumulation)
}

/// Calculate D-Infinity flow accumulation (Tarboton 1997) using fractional weights.
///
/// Each pixel distributes its accumulated area to one or two downstream neighbours,
/// weighted by the angular proximity of the D-infinity flow angle to the two
/// bracketing cardinal/diagonal directions.
///
/// # Arguments
/// * `dem`          — row-major flat slice of elevations (for elevation-sorted order)
/// * `flow_angles`  — output of [`flow_direction_dinf`][crate::hydrology::flow_direction_dinf],
///   same length as `dem`; NaN pixels do not distribute
/// * `width` / `height` — raster dimensions
/// * `cell_size`    — unused in accumulation but kept for API symmetry
///
/// # Returns
/// Flat Vec of floating-point accumulated area values (each pixel counts as 1.0).
/// Pixels with NaN angles (pits / boundary) still receive contributions from upstream.
pub fn flow_accumulation_dinf(
    dem: &[f64],
    flow_angles: &[f64],
    width: usize,
    height: usize,
    _cell_size: f64,
) -> Vec<f64> {
    let n = width * height;
    let mut acc = vec![1.0_f64; n]; // Each pixel counts itself

    // --- Build elevation-descending order via a max-heap ---
    // BinaryHeap<(NotNan<f64>, usize)> is a max-heap: largest elevation pops first.
    // We process highest-elevation pixels first so flow only goes downhill.
    let mut heap: BinaryHeap<(NotNan<f64>, usize)> = BinaryHeap::new();
    for (idx, &elev) in dem.iter().enumerate() {
        if let Ok(nn) = NotNan::new(elev) {
            heap.push((nn, idx));
        }
    }

    // Pop in descending elevation order (highest first)
    while let Some((_, idx)) = heap.pop() {
        let angle = flow_angles[idx];
        if angle.is_nan() {
            // Pit or boundary — does not distribute
            continue;
        }

        let row = idx / width;
        let col = idx % width;

        // Determine the two bracketing facet angles
        let facet_idx = angle / FRAC_PI_4; // float index in [0, 8)
        let k = facet_idx.floor() as usize % 8; // lower facet
        let k_next = (k + 1) % 8;

        let alpha = k as f64 * FRAC_PI_4;
        let beta = alpha + FRAC_PI_4;

        // Normalise angle to [alpha, beta] range accounting for 2π wrap
        let theta = if k == 7 && angle < alpha {
            angle + 2.0 * std::f64::consts::PI
        } else {
            angle
        };

        let w_e1 = (beta - theta) / FRAC_PI_4; // weight toward lower-angle neighbour
        let w_e2 = (theta - alpha) / FRAC_PI_4; // weight toward higher-angle neighbour

        // Neighbour offsets for each of the 8 directions (k=0..7 CCW from east)
        // Cardinal/diagonal directions matching the facet table in flow_direction.rs
        // k=0 (E):  e1=(0,+1)
        // k=1 (NE): e1=(-1,+1)
        // k=2 (N):  e1=(-1,0)
        // k=3 (NW): e1=(-1,-1)
        // k=4 (W):  e1=(0,-1)
        // k=5 (SW): e1=(+1,-1)
        // k=6 (S):  e1=(+1,0)
        // k=7 (SE): e1=(+1,+1)
        let dir_offsets: [(isize, isize); 8] = [
            (0, 1),   // k=0 E
            (-1, 1),  // k=1 NE
            (-1, 0),  // k=2 N
            (-1, -1), // k=3 NW
            (0, -1),  // k=4 W
            (1, -1),  // k=5 SW
            (1, 0),   // k=6 S
            (1, 1),   // k=7 SE
        ];

        let (dr1, dc1) = dir_offsets[k];
        let (dr2, dc2) = dir_offsets[k_next];

        let contrib = acc[idx];

        // Distribute to neighbour 1 (lower-angle direction)
        if w_e1.abs() > 1e-15 {
            let nr1 = row as isize + dr1;
            let nc1 = col as isize + dc1;
            if nr1 >= 0 && nr1 < height as isize && nc1 >= 0 && nc1 < width as isize {
                acc[nr1 as usize * width + nc1 as usize] += contrib * w_e1;
            }
        }

        // Distribute to neighbour 2 (higher-angle direction)
        if w_e2.abs() > 1e-15 {
            let nr2 = row as isize + dr2;
            let nc2 = col as isize + dc2;
            if nr2 >= 0 && nr2 < height as isize && nc2 >= 0 && nc2 < width as isize {
                acc[nr2 as usize * width + nc2 as usize] += contrib * w_e2;
            }
        }
    }

    acc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flow_accumulation() {
        let mut dem = Array2::zeros((5, 5));
        for y in 0..5 {
            for x in 0..5 {
                dem[[y, x]] = 100.0 - (x as f64);
            }
        }

        let accum = flow_accumulation(&dem, 10.0, None).expect("failed");
        // Eastward flow should accumulate
        assert!(accum[[2, 4]] > accum[[2, 0]]);
    }

    #[test]
    fn test_dinf_accumulation_uniform_slope() {
        use crate::hydrology::flow_direction::flow_direction_dinf;

        // 5×5 DEM with uniform east-sloping elevation (higher in west)
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
        let acc = flow_accumulation_dinf(&dem, &angles, width, height, cell_size);

        // The rightmost column (col=4) is the outlet; it should receive more than 1
        // contribution (flow from upstream cells)
        let outlet_acc: f64 = (1..height - 1).map(|r| acc[r * width + (width - 1)]).sum();
        assert!(
            outlet_acc > (height - 2) as f64,
            "outlet accumulation {outlet_acc} should exceed interior row count"
        );
    }

    #[test]
    fn test_dinf_accumulation_splits_proportional() {
        use crate::hydrology::flow_direction::flow_direction_dinf;
        use std::f64::consts::PI;

        // Design: 3×3 DEM where only center (row=1, col=1) is an interior pixel.
        // All 8 border pixels get NaN flow angles and do not distribute area.
        //
        // Facet k=0 wins (e1=E, e2=NE) and produces angle exactly π/8.
        //
        // Facet k=0 (even, e1=E cardinal, d1=cell_size, d2=cell_size):
        //   s1 = (z − e_E) / cell_size
        //   s2 = (e_E − e_NE) / cell_size
        //   angle = atan2(s2, s1) = π/8  ⟺  s2/s1 = tan(π/8)
        //
        // Choose cell_size=1, s1=1 → e_E = z − 1.
        // Then s2 = tan(π/8) * 1 → e_NE = e_E − tan(π/8).
        //
        // All other border pixels (N, W, S, NW, SW, SE) are set high except:
        //   - SE = e_E  (neutralises facet k=7's lateral s2 = e_SE − e_E = 0)
        //   This ensures facet k=0 wins the steepest-descent competition.
        //
        // At flow_angle = π/8:  w_E = (π/4 − π/8)/(π/4) = 0.5, w_NE = 0.5.
        // Area split: acc[E] = 1.0 + 0.5 = 1.5, acc[NE] = 1.0 + 0.5 = 1.5.

        let height = 3usize;
        let width = 3usize;
        let cell_size = 1.0_f64;

        let tan_pi_8 = (PI / 8.0).tan(); // ≈ 0.41421356

        let z = 10.0_f64;
        let e_e = z - cell_size; // E  = 9.0
        let e_ne = e_e - tan_pi_8 * cell_size; // NE ≈ 8.5858
        let high = z + 2.0_f64; // 12.0 — all other neighbours

        // Flat indices in the 3×3 grid
        let center_idx = width + 1; // 4: center (1,1)
        let e_idx = width + 2; // 5: East  (1,2) — boundary
        let ne_idx = 2; // 2: NE    (0,2) — boundary
        let se_idx = 2 * width + 2; // 8: SE    (2,2) — boundary

        // Build DEM: start everything at `high`, then overwrite key cells
        let mut dem = vec![high; width * height];
        dem[center_idx] = z;
        dem[e_idx] = e_e;
        dem[ne_idx] = e_ne;
        dem[se_idx] = e_e; // neutralise k=7 lateral slope

        // flow_direction_dinf: only center is interior; all border cells → NaN
        let angles = flow_direction_dinf(&dem, width, height, cell_size, None);

        let center_angle = angles[center_idx];
        assert!(
            !center_angle.is_nan(),
            "center pixel angle should not be NaN"
        );
        assert!(
            (center_angle - PI / 8.0).abs() < 1e-6,
            "center angle {center_angle:.6} should be ≈ π/8 ({:.6})",
            PI / 8.0
        );

        // flow_accumulation_dinf: only center distributes
        let acc = flow_accumulation_dinf(&dem, &angles, width, height, cell_size);

        let acc_e = acc[e_idx];
        let acc_ne = acc[ne_idx];
        // center (1.0) splits 0.5 → E, 0.5 → NE; each also counts itself = 1.5
        assert!(
            (acc_e - 1.5).abs() < 1e-6,
            "E accumulation {acc_e:.6} should be ≈ 1.5"
        );
        assert!(
            (acc_ne - 1.5).abs() < 1e-6,
            "NE accumulation {acc_ne:.6} should be ≈ 1.5"
        );
        // Center itself has no upstream contributors
        assert!(
            (acc[center_idx] - 1.0).abs() < 1e-12,
            "center accumulation {} should remain 1.0",
            acc[center_idx]
        );
    }
}
