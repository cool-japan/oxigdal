//! Topographic Position Index (TPI) calculation.
//!
//! TPI compares the elevation of each cell to the mean elevation of a
//! neighborhood around it. Positive TPI indicates ridges, negative indicates
//! valleys, and near-zero indicates flat or mid-slope.

use crate::error::{Result, TerrainError};
use num_traits::Float;
use scirs2_core::prelude::*;

/// Calculate Topographic Position Index (TPI).
///
/// TPI = elevation - mean(neighborhood)
///
/// # Arguments
/// * `dem` - Input DEM as 2D array
/// * `radius` - Radius of neighborhood in cells
/// * `nodata` - Optional NoData value to skip
///
/// # Returns
/// 2D array of TPI values
pub fn tpi<T>(dem: &Array2<T>, radius: usize, nodata: Option<T>) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy,
{
    validate_inputs(dem, radius)?;

    let (height, width) = dem.dim();
    let mut tpi_result = Array2::zeros((height, width));

    let diameter = 2 * radius + 1;

    for y in 0..height {
        for x in 0..width {
            let center = dem[[y, x]];

            if let Some(nd) = nodata {
                if is_nodata(center, nd) {
                    tpi_result[[y, x]] = f64::NAN;
                    continue;
                }
            }

            let mut sum = 0.0;
            let mut count = 0;

            // Calculate mean of neighborhood
            for dy in 0..diameter {
                for dx in 0..diameter {
                    let ny = y as isize + dy as isize - radius as isize;
                    let nx = x as isize + dx as isize - radius as isize;

                    if ny >= 0 && ny < height as isize && nx >= 0 && nx < width as isize {
                        let val = dem[[ny as usize, nx as usize]];

                        if let Some(nd) = nodata {
                            if is_nodata(val, nd) {
                                continue;
                            }
                        }

                        sum += val.into();
                        count += 1;
                    }
                }
            }

            if count > 0 {
                let mean = sum / (count as f64);
                tpi_result[[y, x]] = center.into() - mean;
            } else {
                tpi_result[[y, x]] = f64::NAN;
            }
        }
    }

    Ok(tpi_result)
}

/// Calculate TPI with optional parallelization.
#[cfg(feature = "parallel")]
pub fn tpi_parallel<T>(dem: &Array2<T>, radius: usize, nodata: Option<T>) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy + Send + Sync,
{
    use rayon::prelude::*;

    validate_inputs(dem, radius)?;

    let (height, width) = dem.dim();
    let diameter = 2 * radius + 1;

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

            let mut sum = 0.0;
            let mut count = 0;

            for dy in 0..diameter {
                for dx in 0..diameter {
                    let ny = y as isize + dy as isize - radius as isize;
                    let nx = x as isize + dx as isize - radius as isize;

                    if ny >= 0 && ny < height as isize && nx >= 0 && nx < width as isize {
                        let val = dem[[ny as usize, nx as usize]];

                        if let Some(nd) = nodata {
                            if is_nodata(val, nd) {
                                continue;
                            }
                        }

                        sum += val.into();
                        count += 1;
                    }
                }
            }

            if count > 0 {
                let mean = sum / (count as f64);
                center.into() - mean
            } else {
                f64::NAN
            }
        })
        .collect();

    Array2::from_shape_vec((height, width), values).map_err(|_| TerrainError::ComputationError {
        message: "Failed to create TPI array".to_string(),
    })
}

// Helper functions

fn validate_inputs<T>(dem: &Array2<T>, radius: usize) -> Result<()> {
    let (height, width) = dem.dim();

    if height < 3 || width < 3 {
        return Err(TerrainError::InvalidDimensions { width, height });
    }

    if radius == 0 {
        return Err(TerrainError::InvalidRadius {
            radius: radius as f64,
        });
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
    fn test_tpi_flat() {
        let dem = Array2::from_elem((10, 10), 100.0_f64);
        let result = tpi(&dem, 1, None).expect("TPI calculation failed");

        // Flat surface should have TPI of 0
        for &val in result.iter() {
            assert_relative_eq!(val, 0.0, epsilon = 1e-10);
        }
    }

    #[test]
    fn test_tpi_ridge() {
        // Create a ridge (high center)
        let mut dem = Array2::from_elem((5, 5), 100.0_f64);
        dem[[2, 2]] = 150.0; // Center is elevated

        let result = tpi(&dem, 1, None).expect("TPI calculation failed");

        // Center should have positive TPI
        assert!(result[[2, 2]] > 0.0, "ridge should have positive TPI");
    }

    #[test]
    fn test_tpi_valley() {
        // Create a valley (low center)
        let mut dem = Array2::from_elem((5, 5), 100.0_f64);
        dem[[2, 2]] = 50.0; // Center is depressed

        let result = tpi(&dem, 1, None).expect("TPI calculation failed");

        // Center should have negative TPI
        assert!(result[[2, 2]] < 0.0, "valley should have negative TPI");
    }

    #[test]
    fn test_tpi_radius() {
        let mut dem = Array2::from_elem((10, 10), 100.0_f64);
        dem[[5, 5]] = 150.0;

        let tpi1 = tpi(&dem, 1, None).expect("failed");
        let tpi2 = tpi(&dem, 2, None).expect("failed");

        // Different radius should give different results
        assert_ne!(tpi1[[5, 5]], tpi2[[5, 5]]);
    }

    #[test]
    fn test_invalid_radius() {
        let dem = Array2::from_elem((10, 10), 100.0_f64);
        let result = tpi(&dem, 0, None);
        assert!(result.is_err());
    }

    #[cfg(feature = "parallel")]
    #[test]
    fn test_tpi_parallel() {
        let mut dem = Array2::from_elem((20, 20), 100.0_f64);
        dem[[10, 10]] = 150.0;

        let result_seq = tpi(&dem, 2, None).expect("sequential TPI failed");
        let result_par = tpi_parallel(&dem, 2, None).expect("parallel TPI failed");

        // Results should be identical
        for y in 0..20 {
            for x in 0..20 {
                assert_relative_eq!(result_seq[[y, x]], result_par[[y, x]], epsilon = 1e-10);
            }
        }
    }
}
