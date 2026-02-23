//! Surface roughness calculation.
//!
//! Roughness measures local terrain variation using different methods:
//! - Standard deviation of elevation
//! - Range (max - min)
//! - Vector ruggedness measure (VRM)

use crate::error::{Result, TerrainError};
use num_traits::Float;
use scirs2_core::prelude::*;

/// Roughness calculation method
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoughnessMethod {
    /// Standard deviation of neighborhood
    StdDev,
    /// Range (max - min)
    Range,
    /// Vector Ruggedness Measure
    VectorRuggedness,
}

/// Calculate surface roughness using standard deviation.
///
/// # Arguments
/// * `dem` - Input DEM as 2D array
/// * `radius` - Radius of neighborhood in cells
/// * `nodata` - Optional NoData value to skip
pub fn roughness_stddev<T>(dem: &Array2<T>, radius: usize, nodata: Option<T>) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy,
{
    validate_inputs(dem, radius)?;

    let (height, width) = dem.dim();
    let mut roughness = Array2::zeros((height, width));

    let diameter = 2 * radius + 1;

    for y in 0..height {
        for x in 0..width {
            let center = dem[[y, x]];

            if let Some(nd) = nodata {
                if is_nodata(center, nd) {
                    roughness[[y, x]] = f64::NAN;
                    continue;
                }
            }

            let mut values = Vec::new();

            // Collect neighborhood values
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

                        values.push(val.into());
                    }
                }
            }

            if values.len() > 1 {
                let mean = values.iter().sum::<f64>() / values.len() as f64;
                let variance = values.iter().map(|&v| (v - mean) * (v - mean)).sum::<f64>()
                    / values.len() as f64;
                roughness[[y, x]] = variance.sqrt();
            } else {
                roughness[[y, x]] = 0.0;
            }
        }
    }

    Ok(roughness)
}

/// Calculate surface roughness using range (max - min).
pub fn roughness_range<T>(dem: &Array2<T>, radius: usize, nodata: Option<T>) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy,
{
    validate_inputs(dem, radius)?;

    let (height, width) = dem.dim();
    let mut roughness = Array2::zeros((height, width));

    let diameter = 2 * radius + 1;

    for y in 0..height {
        for x in 0..width {
            let center = dem[[y, x]];

            if let Some(nd) = nodata {
                if is_nodata(center, nd) {
                    roughness[[y, x]] = f64::NAN;
                    continue;
                }
            }

            let mut min_val = f64::INFINITY;
            let mut max_val = f64::NEG_INFINITY;

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

                        let val_f64 = val.into();
                        min_val = min_val.min(val_f64);
                        max_val = max_val.max(val_f64);
                    }
                }
            }

            if min_val.is_finite() && max_val.is_finite() {
                roughness[[y, x]] = max_val - min_val;
            } else {
                roughness[[y, x]] = 0.0;
            }
        }
    }

    Ok(roughness)
}

/// Calculate Vector Ruggedness Measure (VRM).
///
/// VRM measures terrain ruggedness as the dispersion of unit vectors normal
/// to the surface. Values range from 0 (flat) to 1 (extremely rugged).
pub fn vector_ruggedness_measure<T>(
    dem: &Array2<T>,
    cell_size: f64,
    nodata: Option<T>,
) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy,
{
    validate_inputs(dem, 1)?;

    let (height, width) = dem.dim();
    let mut vrm = Array2::zeros((height, width));

    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let center = dem[[y, x]];

            if let Some(nd) = nodata {
                if is_nodata(center, nd) {
                    vrm[[y, x]] = f64::NAN;
                    continue;
                }
            }

            let mut vectors = Vec::new();

            // Calculate normal vectors for each cell in 3x3 neighborhood
            for dy in -1..=1_isize {
                for dx in -1..=1_isize {
                    let ny = (y as isize + dy) as usize;
                    let nx = (x as isize + dx) as usize;

                    if ny < height && nx < width {
                        let val = dem[[ny, nx]];

                        if let Some(nd) = nodata {
                            if is_nodata(val, nd) {
                                continue;
                            }
                        }

                        // Calculate normal vector components
                        let z = val.into();
                        let x_comp = -(dx as f64) * cell_size;
                        let y_comp = -(dy as f64) * cell_size;
                        let z_comp = z;

                        // Normalize
                        let mag = (x_comp * x_comp + y_comp * y_comp + z_comp * z_comp).sqrt();
                        if mag > f64::EPSILON {
                            vectors.push((x_comp / mag, y_comp / mag, z_comp / mag));
                        }
                    }
                }
            }

            if !vectors.is_empty() {
                // Calculate resultant vector
                let sum_x: f64 = vectors.iter().map(|(x, _, _)| x).sum();
                let sum_y: f64 = vectors.iter().map(|(_, y, _)| y).sum();
                let sum_z: f64 = vectors.iter().map(|(_, _, z)| z).sum();

                let resultant_mag = (sum_x * sum_x + sum_y * sum_y + sum_z * sum_z).sqrt();
                let n = vectors.len() as f64;

                // VRM = 1 - (|R| / n) where R is resultant vector
                vrm[[y, x]] = 1.0 - (resultant_mag / n);
            } else {
                vrm[[y, x]] = 0.0;
            }
        }
    }

    Ok(vrm)
}

/// Calculate roughness with specified method.
pub fn roughness<T>(
    dem: &Array2<T>,
    method: RoughnessMethod,
    radius: usize,
    cell_size: f64,
    nodata: Option<T>,
) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy,
{
    match method {
        RoughnessMethod::StdDev => roughness_stddev(dem, radius, nodata),
        RoughnessMethod::Range => roughness_range(dem, radius, nodata),
        RoughnessMethod::VectorRuggedness => vector_ruggedness_measure(dem, cell_size, nodata),
    }
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
    fn test_roughness_stddev_flat() {
        let dem = Array2::from_elem((10, 10), 100.0_f64);
        let result = roughness_stddev(&dem, 1, None).expect("roughness calculation failed");

        // Flat surface should have roughness of 0
        for &val in result.iter() {
            assert_relative_eq!(val, 0.0, epsilon = 1e-10);
        }
    }

    #[test]
    fn test_roughness_range_flat() {
        let dem = Array2::from_elem((10, 10), 100.0_f64);
        let result = roughness_range(&dem, 1, None).expect("roughness calculation failed");

        for &val in result.iter() {
            assert_relative_eq!(val, 0.0, epsilon = 1e-10);
        }
    }

    #[test]
    fn test_roughness_stddev_variable() {
        // Create variable terrain
        let mut dem = Array2::from_elem((5, 5), 100.0_f64);
        dem[[2, 2]] = 110.0;
        dem[[2, 1]] = 90.0;

        let result = roughness_stddev(&dem, 1, None).expect("roughness calculation failed");

        // Center should have non-zero roughness
        assert!(
            result[[2, 2]] > 0.0,
            "variable terrain should have roughness"
        );
    }

    #[test]
    fn test_roughness_range_variable() {
        let mut dem = Array2::from_elem((5, 5), 100.0_f64);
        dem[[2, 2]] = 120.0;
        dem[[2, 1]] = 80.0;

        let result = roughness_range(&dem, 1, None).expect("roughness calculation failed");

        // Range should capture the 40-unit difference
        assert!(
            result[[2, 2]] >= 40.0 - 1.0,
            "range should capture variation"
        );
    }

    #[test]
    fn test_vrm_flat() {
        let dem = Array2::from_elem((10, 10), 100.0_f64);
        let result = vector_ruggedness_measure(&dem, 10.0, None).expect("VRM calculation failed");

        // Flat surface should have VRM near 0
        for y in 1..9 {
            for x in 1..9 {
                assert!(result[[y, x]] < 0.1, "flat surface should have low VRM");
            }
        }
    }

    #[test]
    fn test_vrm_rugged() {
        // Create highly variable terrain
        let mut dem = Array2::zeros((10, 10));
        for y in 0..10 {
            for x in 0..10 {
                dem[[y, x]] = if (x + y) % 2 == 0 { 100.0 } else { 50.0 };
            }
        }

        let result = vector_ruggedness_measure(&dem, 10.0, None).expect("VRM calculation failed");

        // Should detect high ruggedness
        assert!(
            result[[5, 5]] > 0.0,
            "rugged terrain should have positive VRM"
        );
    }

    #[test]
    fn test_roughness_methods() {
        let mut dem = Array2::zeros((10, 10));
        for y in 0..10 {
            for x in 0..10 {
                dem[[y, x]] = ((x as f64).sin() + (y as f64).cos()) * 20.0 + 100.0;
            }
        }

        let stddev =
            roughness(&dem, RoughnessMethod::StdDev, 1, 10.0, None).expect("stddev failed");
        let range = roughness(&dem, RoughnessMethod::Range, 1, 10.0, None).expect("range failed");
        let vrm =
            roughness(&dem, RoughnessMethod::VectorRuggedness, 1, 10.0, None).expect("vrm failed");

        // All methods should detect variation
        assert!(stddev[[5, 5]] > 0.0);
        assert!(range[[5, 5]] > 0.0);
        assert!(vrm[[5, 5]] >= 0.0);
    }
}
