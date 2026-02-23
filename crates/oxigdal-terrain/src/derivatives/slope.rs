//! Slope calculation algorithms for terrain analysis.
//!
//! Provides multiple methods for computing slope from Digital Elevation Models (DEMs):
//! - Horn's method (3x3 kernel, most common)
//! - Zevenbergen-Thorne (2nd order polynomial, smooth surfaces)

use crate::error::{Result, TerrainError};
use num_traits::Float;
use scirs2_core::prelude::*;

/// Slope units for output
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlopeUnits {
    /// Degrees (0-90)
    Degrees,
    /// Percent rise (100 * rise/run)
    Percent,
    /// Radians (0-π/2)
    Radians,
}

/// Algorithm for slope calculation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlopeAlgorithm {
    /// Horn's method (3x3 kernel, default)
    Horn,
    /// Zevenbergen-Thorne (2nd order polynomial)
    ZevenbergenThorne,
}

/// Edge handling strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeStrategy {
    /// Extend edge values
    Extend,
    /// Mirror edge values
    Mirror,
    /// Use constant value
    Constant(i32),
}

/// Calculate slope from a DEM using Horn's method.
///
/// # Arguments
/// * `dem` - Input DEM as 2D array
/// * `cell_size` - Cell size in map units
/// * `units` - Output units (degrees, percent, or radians)
/// * `nodata` - Optional NoData value to skip
///
/// # Returns
/// 2D array of slope values
pub fn slope_horn<T>(
    dem: &Array2<T>,
    cell_size: f64,
    units: SlopeUnits,
    nodata: Option<T>,
) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy,
{
    validate_inputs(dem, cell_size)?;

    let (height, width) = dem.dim();
    let mut slope = Array2::zeros((height, width));

    // Horn's method uses 3x3 kernel
    // dz/dx = ((c + 2f + i) - (a + 2d + g)) / (8 * cell_size)
    // dz/dy = ((g + 2h + i) - (a + 2b + c)) / (8 * cell_size)
    // slope = arctan(sqrt(dz/dx^2 + dz/dy^2))

    for y in 0..height {
        for x in 0..width {
            let center = dem[[y, x]];

            // Skip NoData values
            if let Some(nd) = nodata {
                if is_nodata(center, nd) {
                    slope[[y, x]] = f64::NAN;
                    continue;
                }
            }

            // Get 3x3 neighborhood
            let a = get_value(
                dem,
                y.wrapping_sub(1),
                x.wrapping_sub(1),
                EdgeStrategy::Extend,
            );
            let b = get_value(dem, y.wrapping_sub(1), x, EdgeStrategy::Extend);
            let c = get_value(dem, y.wrapping_sub(1), x + 1, EdgeStrategy::Extend);
            let d = get_value(dem, y, x.wrapping_sub(1), EdgeStrategy::Extend);
            let f = get_value(dem, y, x + 1, EdgeStrategy::Extend);
            let g = get_value(dem, y + 1, x.wrapping_sub(1), EdgeStrategy::Extend);
            let h = get_value(dem, y + 1, x, EdgeStrategy::Extend);
            let i = get_value(dem, y + 1, x + 1, EdgeStrategy::Extend);

            // Calculate gradients
            let dzdx = ((c.into() + 2.0 * f.into() + i.into())
                - (a.into() + 2.0 * d.into() + g.into()))
                / (8.0 * cell_size);
            let dzdy = ((g.into() + 2.0 * h.into() + i.into())
                - (a.into() + 2.0 * b.into() + c.into()))
                / (8.0 * cell_size);

            // Calculate slope
            let slope_rad = (dzdx * dzdx + dzdy * dzdy).sqrt().atan();

            slope[[y, x]] = match units {
                SlopeUnits::Radians => slope_rad,
                SlopeUnits::Degrees => slope_rad.to_degrees(),
                SlopeUnits::Percent => slope_rad.tan() * 100.0,
            };
        }
    }

    Ok(slope)
}

/// Calculate slope using Zevenbergen-Thorne method.
///
/// Better for smooth surfaces, uses 2nd order polynomial fit.
pub fn slope_zevenbergen_thorne<T>(
    dem: &Array2<T>,
    cell_size: f64,
    units: SlopeUnits,
    nodata: Option<T>,
) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy,
{
    validate_inputs(dem, cell_size)?;

    let (height, width) = dem.dim();
    let mut slope = Array2::zeros((height, width));

    // Zevenbergen-Thorne method
    // dz/dx = (f - d) / (2 * cell_size)
    // dz/dy = (h - b) / (2 * cell_size)

    for y in 0..height {
        for x in 0..width {
            let center = dem[[y, x]];

            if let Some(nd) = nodata {
                if is_nodata(center, nd) {
                    slope[[y, x]] = f64::NAN;
                    continue;
                }
            }

            let b = get_value(dem, y.wrapping_sub(1), x, EdgeStrategy::Extend);
            let d = get_value(dem, y, x.wrapping_sub(1), EdgeStrategy::Extend);
            let f = get_value(dem, y, x + 1, EdgeStrategy::Extend);
            let h = get_value(dem, y + 1, x, EdgeStrategy::Extend);

            let dzdx = (f.into() - d.into()) / (2.0 * cell_size);
            let dzdy = (h.into() - b.into()) / (2.0 * cell_size);

            let slope_rad = (dzdx * dzdx + dzdy * dzdy).sqrt().atan();

            slope[[y, x]] = match units {
                SlopeUnits::Radians => slope_rad,
                SlopeUnits::Degrees => slope_rad.to_degrees(),
                SlopeUnits::Percent => slope_rad.tan() * 100.0,
            };
        }
    }

    Ok(slope)
}

/// Calculate slope with specified algorithm.
pub fn slope<T>(
    dem: &Array2<T>,
    cell_size: f64,
    units: SlopeUnits,
    algorithm: SlopeAlgorithm,
    nodata: Option<T>,
) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy,
{
    match algorithm {
        SlopeAlgorithm::Horn => slope_horn(dem, cell_size, units, nodata),
        SlopeAlgorithm::ZevenbergenThorne => {
            slope_zevenbergen_thorne(dem, cell_size, units, nodata)
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

fn get_value<T: Copy>(dem: &Array2<T>, y: usize, x: usize, strategy: EdgeStrategy) -> T {
    let (height, width) = dem.dim();

    // Check bounds
    if y < height && x < width {
        dem[[y, x]]
    } else {
        match strategy {
            EdgeStrategy::Extend => {
                let y_clamped = y.min(height - 1);
                let x_clamped = x.min(width - 1);
                dem[[y_clamped, x_clamped]]
            }
            EdgeStrategy::Mirror => {
                let y_mirror = if y >= height { 2 * height - y - 2 } else { y };
                let x_mirror = if x >= width { 2 * width - x - 2 } else { x };
                dem[[y_mirror, x_mirror]]
            }
            EdgeStrategy::Constant(_val) => {
                // This is a bit of a hack - we convert i32 to T
                // In practice, this would need proper type conversion
                dem[[0, 0]] // Placeholder
            }
        }
    }
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
    fn test_slope_horn_flat() {
        let dem = Array2::from_elem((5, 5), 100.0_f64);
        let slope =
            slope_horn(&dem, 10.0, SlopeUnits::Degrees, None).expect("slope calculation failed");

        // Flat surface should have zero slope
        for &s in slope.iter() {
            assert_relative_eq!(s, 0.0, epsilon = 1e-10);
        }
    }

    #[test]
    fn test_slope_horn_inclined() {
        // Create a simple inclined plane
        let mut dem = Array2::zeros((5, 5));
        for y in 0..5 {
            for x in 0..5 {
                dem[[y, x]] = (x as f64) * 10.0; // 10 unit rise per cell
            }
        }

        let slope =
            slope_horn(&dem, 10.0, SlopeUnits::Degrees, None).expect("slope calculation failed");

        // Central cells should have slope of 45 degrees (rise = run)
        let center_slope = slope[[2, 2]];
        assert_relative_eq!(center_slope, 45.0, epsilon = 1.0);
    }

    #[test]
    fn test_slope_units() {
        let mut dem = Array2::zeros((5, 5));
        for y in 0..5 {
            for x in 0..5 {
                dem[[y, x]] = (x as f64) * 10.0;
            }
        }

        let slope_deg = slope_horn(&dem, 10.0, SlopeUnits::Degrees, None).expect("degrees failed");
        let slope_rad = slope_horn(&dem, 10.0, SlopeUnits::Radians, None).expect("radians failed");
        let slope_pct = slope_horn(&dem, 10.0, SlopeUnits::Percent, None).expect("percent failed");

        // Check unit conversion
        assert_relative_eq!(
            slope_deg[[2, 2]],
            slope_rad[[2, 2]].to_degrees(),
            epsilon = 1e-6
        );
        assert_relative_eq!(
            slope_pct[[2, 2]] / 100.0,
            slope_rad[[2, 2]].tan(),
            epsilon = 1e-2
        );
    }

    #[test]
    fn test_invalid_cell_size() {
        let dem = Array2::from_elem((5, 5), 100.0_f64);
        let result = slope_horn(&dem, -10.0, SlopeUnits::Degrees, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_dimensions() {
        let dem = Array2::from_elem((2, 2), 100.0_f64);
        let result = slope_horn(&dem, 10.0, SlopeUnits::Degrees, None);
        assert!(result.is_err());
    }
}
