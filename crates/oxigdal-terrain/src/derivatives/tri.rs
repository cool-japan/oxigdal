//! Terrain Ruggedness Index (TRI) calculation.
//!
//! TRI measures terrain heterogeneity by calculating the sum of absolute
//! differences between a center cell and its neighbors.

use crate::error::{Result, TerrainError};
use num_traits::Float;
use scirs2_core::prelude::*;

/// Calculate Terrain Ruggedness Index (TRI).
///
/// TRI = sqrt(sum((z_i - z_center)^2)) / n
///
/// where z_i are the neighboring elevations.
///
/// # Arguments
/// * `dem` - Input DEM as 2D array
/// * `nodata` - Optional NoData value to skip
///
/// # Returns
/// 2D array of TRI values
pub fn tri<T>(dem: &Array2<T>, nodata: Option<T>) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy,
{
    validate_inputs(dem)?;

    let (height, width) = dem.dim();
    let mut tri_result = Array2::zeros((height, width));

    for y in 0..height {
        for x in 0..width {
            let center = dem[[y, x]];

            if let Some(nd) = nodata {
                if is_nodata(center, nd) {
                    tri_result[[y, x]] = f64::NAN;
                    continue;
                }
            }

            let center_val = center.into();
            let mut sum_sq_diff = 0.0;
            let mut count = 0;

            // Check 8 neighbors
            for dy in -1..=1_isize {
                for dx in -1..=1_isize {
                    if dy == 0 && dx == 0 {
                        continue;
                    }

                    let ny = y as isize + dy;
                    let nx = x as isize + dx;

                    if ny >= 0 && ny < height as isize && nx >= 0 && nx < width as isize {
                        let neighbor = dem[[ny as usize, nx as usize]];

                        if let Some(nd) = nodata {
                            if is_nodata(neighbor, nd) {
                                continue;
                            }
                        }

                        let diff = neighbor.into() - center_val;
                        sum_sq_diff += diff * diff;
                        count += 1;
                    }
                }
            }

            if count > 0 {
                tri_result[[y, x]] = (sum_sq_diff / count as f64).sqrt();
            } else {
                tri_result[[y, x]] = 0.0;
            }
        }
    }

    Ok(tri_result)
}

/// Calculate TRI using Riley's original method (sum of absolute differences).
pub fn tri_riley<T>(dem: &Array2<T>, nodata: Option<T>) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy,
{
    validate_inputs(dem)?;

    let (height, width) = dem.dim();
    let mut tri_result = Array2::zeros((height, width));

    for y in 0..height {
        for x in 0..width {
            let center = dem[[y, x]];

            if let Some(nd) = nodata {
                if is_nodata(center, nd) {
                    tri_result[[y, x]] = f64::NAN;
                    continue;
                }
            }

            let center_val = center.into();
            let mut sum_abs_diff = 0.0;
            let mut count = 0;

            for dy in -1..=1_isize {
                for dx in -1..=1_isize {
                    if dy == 0 && dx == 0 {
                        continue;
                    }

                    let ny = y as isize + dy;
                    let nx = x as isize + dx;

                    if ny >= 0 && ny < height as isize && nx >= 0 && nx < width as isize {
                        let neighbor = dem[[ny as usize, nx as usize]];

                        if let Some(nd) = nodata {
                            if is_nodata(neighbor, nd) {
                                continue;
                            }
                        }

                        let diff = (neighbor.into() - center_val).abs();
                        sum_abs_diff += diff;
                        count += 1;
                    }
                }
            }

            if count > 0 {
                tri_result[[y, x]] = sum_abs_diff;
            } else {
                tri_result[[y, x]] = 0.0;
            }
        }
    }

    Ok(tri_result)
}

/// Calculate TRI with optional parallelization.
#[cfg(feature = "parallel")]
pub fn tri_parallel<T>(dem: &Array2<T>, nodata: Option<T>) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy + Send + Sync,
{
    use rayon::prelude::*;

    validate_inputs(dem)?;

    let (height, width) = dem.dim();

    let values: Vec<f64> = (0..height * width)
        .into_par_iter()
        .map(|idx| {
            let y = idx / width;
            let x = idx % width;
            let center = dem[[y, x]];

            if let Some(nd) = nodata {
                if is_nodata(center, nd) {
                    return f64::NAN;
                }
            }

            let center_val = center.into();
            let mut sum_sq_diff = 0.0;
            let mut count = 0;

            for dy in -1..=1_isize {
                for dx in -1..=1_isize {
                    if dy == 0 && dx == 0 {
                        continue;
                    }

                    let ny = y as isize + dy;
                    let nx = x as isize + dx;

                    if ny >= 0 && ny < height as isize && nx >= 0 && nx < width as isize {
                        let neighbor = dem[[ny as usize, nx as usize]];

                        if let Some(nd) = nodata {
                            if is_nodata(neighbor, nd) {
                                continue;
                            }
                        }

                        let diff = neighbor.into() - center_val;
                        sum_sq_diff += diff * diff;
                        count += 1;
                    }
                }
            }

            if count > 0 {
                (sum_sq_diff / count as f64).sqrt()
            } else {
                0.0
            }
        })
        .collect();

    Array2::from_shape_vec((height, width), values).map_err(|_| TerrainError::ComputationError {
        message: "Failed to create TRI array".to_string(),
    })
}

// Helper functions

fn validate_inputs<T>(dem: &Array2<T>) -> Result<()> {
    let (height, width) = dem.dim();

    if height < 3 || width < 3 {
        return Err(TerrainError::InvalidDimensions { width, height });
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
    use approx::assert_relative_eq;

    #[test]
    fn test_tri_flat() {
        let dem = Array2::from_elem((10, 10), 100.0_f64);
        let result = tri(&dem, None).expect("TRI calculation failed");

        // Flat surface should have TRI of 0
        for &val in result.iter() {
            assert_relative_eq!(val, 0.0, epsilon = 1e-10);
        }
    }

    #[test]
    fn test_tri_rugged() {
        // Create a rugged surface
        let mut dem = Array2::from_elem((5, 5), 100.0_f64);
        // Add variation
        dem[[2, 1]] = 110.0;
        dem[[2, 3]] = 90.0;
        dem[[1, 2]] = 105.0;
        dem[[3, 2]] = 95.0;

        let result = tri(&dem, None).expect("TRI calculation failed");

        // Center should have positive TRI (rugged)
        assert!(result[[2, 2]] > 0.0, "rugged area should have positive TRI");
    }

    #[test]
    fn test_tri_riley() {
        let mut dem = Array2::from_elem((5, 5), 100.0_f64);
        dem[[2, 2]] = 110.0;

        let tri_std = tri(&dem, None).expect("failed");
        let tri_ril = tri_riley(&dem, None).expect("failed");

        // Both should detect ruggedness but with different values
        assert!(tri_std[[2, 2]] > 0.0);
        assert!(tri_ril[[2, 2]] > 0.0);
    }

    #[test]
    fn test_tri_gradient() {
        // Create a gradient
        let mut dem = Array2::zeros((5, 5));
        for y in 0..5 {
            for x in 0..5 {
                dem[[y, x]] = (x + y) as f64 * 10.0;
            }
        }

        let result = tri(&dem, None).expect("TRI calculation failed");

        // Uniform gradient should have relatively low TRI
        for &val in result.iter() {
            assert!(val < 20.0, "uniform gradient should have low TRI");
        }
    }

    #[cfg(feature = "parallel")]
    #[test]
    fn test_tri_parallel() {
        let mut dem = Array2::from_elem((20, 20), 100.0_f64);
        for y in 0..20 {
            for x in 0..20 {
                if (x + y) % 3 == 0 {
                    dem[[y, x]] += 10.0;
                }
            }
        }

        let result_seq = tri(&dem, None).expect("sequential TRI failed");
        let result_par = tri_parallel(&dem, None).expect("parallel TRI failed");

        for y in 0..20 {
            for x in 0..20 {
                assert_relative_eq!(result_seq[[y, x]], result_par[[y, x]], epsilon = 1e-10);
            }
        }
    }
}
