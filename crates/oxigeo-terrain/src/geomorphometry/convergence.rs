//! Convergence index calculation.

use crate::derivatives::aspect::FlatHandling;
use crate::derivatives::aspect::aspect_horn;
use crate::error::Result;
use num_traits::Float;
use scirs2_core::prelude::*;

/// Calculate convergence index.
///
/// Measures flow convergence (negative) or divergence (positive).
pub fn convergence_index<T>(
    dem: &Array2<T>,
    cell_size: f64,
    nodata: Option<T>,
) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy,
{
    let aspect = aspect_horn(dem, cell_size, FlatHandling::NaN, nodata)?;
    let (height, width) = dem.dim();
    let mut convergence = Array2::zeros((height, width));

    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let center_aspect = aspect[[y, x]];
            if center_aspect.is_nan() || center_aspect < 0.0 {
                continue;
            }

            let mut sum_diff = 0.0;
            let mut count = 0;

            // Check 8 neighbors
            for dy in -1..=1_isize {
                for dx in -1..=1_isize {
                    if dy == 0 && dx == 0 {
                        continue;
                    }

                    let ny = (y as isize + dy) as usize;
                    let nx = (x as isize + dx) as usize;

                    let neighbor_aspect = aspect[[ny, nx]];
                    if neighbor_aspect.is_nan() || neighbor_aspect < 0.0 {
                        continue;
                    }

                    // Calculate angular difference
                    let mut diff = (center_aspect - neighbor_aspect).abs();
                    if diff > 180.0 {
                        diff = 360.0 - diff;
                    }

                    sum_diff += diff;
                    count += 1;
                }
            }

            if count > 0 {
                convergence[[y, x]] = (sum_diff / count as f64) - 90.0;
            }
        }
    }

    Ok(convergence)
}
