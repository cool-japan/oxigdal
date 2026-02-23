//! Aspect calculation algorithms for terrain analysis.
//!
//! Aspect represents the direction of the steepest slope, measured in degrees
//! clockwise from north (0-360 degrees).

use crate::derivatives::slope::EdgeStrategy;
use crate::error::{Result, TerrainError};
use num_traits::Float;
use scirs2_core::prelude::*;

/// Aspect calculation algorithm
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AspectAlgorithm {
    /// Horn's method (3x3 kernel)
    Horn,
    /// Zevenbergen-Thorne
    ZevenbergenThorne,
}

/// Flat area handling for aspect
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlatHandling {
    /// Set flat areas to -1 (no direction)
    NoDirection,
    /// Set flat areas to 0 (north)
    North,
    /// Set flat areas to NaN
    NaN,
}

/// Calculate aspect from a DEM using Horn's method.
///
/// # Arguments
/// * `dem` - Input DEM as 2D array
/// * `cell_size` - Cell size in map units
/// * `flat_handling` - How to handle flat areas
/// * `nodata` - Optional NoData value to skip
///
/// # Returns
/// 2D array of aspect values in degrees (0-360, clockwise from north)
pub fn aspect_horn<T>(
    dem: &Array2<T>,
    cell_size: f64,
    flat_handling: FlatHandling,
    nodata: Option<T>,
) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy,
{
    validate_inputs(dem, cell_size)?;

    let (height, width) = dem.dim();
    let mut aspect = Array2::zeros((height, width));

    for y in 0..height {
        for x in 0..width {
            let center = dem[[y, x]];

            if let Some(nd) = nodata {
                if is_nodata(center, nd) {
                    aspect[[y, x]] = f64::NAN;
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

            // Calculate aspect
            aspect[[y, x]] = calculate_aspect_from_gradients(dzdx, dzdy, flat_handling);
        }
    }

    Ok(aspect)
}

/// Calculate aspect using Zevenbergen-Thorne method.
pub fn aspect_zevenbergen_thorne<T>(
    dem: &Array2<T>,
    cell_size: f64,
    flat_handling: FlatHandling,
    nodata: Option<T>,
) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy,
{
    validate_inputs(dem, cell_size)?;

    let (height, width) = dem.dim();
    let mut aspect = Array2::zeros((height, width));

    for y in 0..height {
        for x in 0..width {
            let center = dem[[y, x]];

            if let Some(nd) = nodata {
                if is_nodata(center, nd) {
                    aspect[[y, x]] = f64::NAN;
                    continue;
                }
            }

            let b = get_value(dem, y.wrapping_sub(1), x, EdgeStrategy::Extend);
            let d = get_value(dem, y, x.wrapping_sub(1), EdgeStrategy::Extend);
            let f = get_value(dem, y, x + 1, EdgeStrategy::Extend);
            let h = get_value(dem, y + 1, x, EdgeStrategy::Extend);

            let dzdx = (f.into() - d.into()) / (2.0 * cell_size);
            let dzdy = (h.into() - b.into()) / (2.0 * cell_size);

            aspect[[y, x]] = calculate_aspect_from_gradients(dzdx, dzdy, flat_handling);
        }
    }

    Ok(aspect)
}

/// Calculate aspect with specified algorithm.
pub fn aspect<T>(
    dem: &Array2<T>,
    cell_size: f64,
    algorithm: AspectAlgorithm,
    flat_handling: FlatHandling,
    nodata: Option<T>,
) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy,
{
    match algorithm {
        AspectAlgorithm::Horn => aspect_horn(dem, cell_size, flat_handling, nodata),
        AspectAlgorithm::ZevenbergenThorne => {
            aspect_zevenbergen_thorne(dem, cell_size, flat_handling, nodata)
        }
    }
}

// Helper functions

fn calculate_aspect_from_gradients(dzdx: f64, dzdy: f64, flat_handling: FlatHandling) -> f64 {
    // Check for flat areas
    if dzdx.abs() < f64::EPSILON && dzdy.abs() < f64::EPSILON {
        return match flat_handling {
            FlatHandling::NoDirection => -1.0,
            FlatHandling::North => 0.0,
            FlatHandling::NaN => f64::NAN,
        };
    }

    // Calculate aspect in radians
    // atan2(dzdy, dzdx) gives direction of upslope (aspect direction)
    // Aspect is the direction the slope faces, not the downslope direction
    let aspect_rad = dzdy.atan2(dzdx);

    // Convert to degrees clockwise from north
    let mut aspect_deg = aspect_rad.to_degrees();

    // Convert from standard math convention (E=0, CCW) to
    // geographic convention (N=0, CW)
    aspect_deg = 90.0 - aspect_deg;

    // Normalize to 0-360
    if aspect_deg < 0.0 {
        aspect_deg += 360.0;
    }
    if aspect_deg >= 360.0 {
        aspect_deg -= 360.0;
    }

    aspect_deg
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

fn get_value<T: Copy>(dem: &Array2<T>, y: usize, x: usize, strategy: EdgeStrategy) -> T {
    let (height, width) = dem.dim();

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
            EdgeStrategy::Constant(_) => {
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
    fn test_aspect_flat() {
        let dem = Array2::from_elem((5, 5), 100.0_f64);
        let aspect = aspect_horn(&dem, 10.0, FlatHandling::NoDirection, None)
            .expect("aspect calculation failed");

        // Flat surface should have no direction (-1)
        for &a in aspect.iter() {
            assert_relative_eq!(a, -1.0, epsilon = 1e-10);
        }
    }

    #[test]
    fn test_aspect_east_slope() {
        // Create east-facing slope
        let mut dem = Array2::zeros((5, 5));
        for y in 0..5 {
            for x in 0..5 {
                dem[[y, x]] = (x as f64) * 10.0;
            }
        }

        let aspect = aspect_horn(&dem, 10.0, FlatHandling::NoDirection, None)
            .expect("aspect calculation failed");

        // East-facing slope should have aspect around 90 degrees
        let center_aspect = aspect[[2, 2]];
        assert_relative_eq!(center_aspect, 90.0, epsilon = 5.0);
    }

    #[test]
    fn test_aspect_north_slope() {
        // Create north-facing slope
        let mut dem = Array2::zeros((5, 5));
        for y in 0..5 {
            for x in 0..5 {
                dem[[y, x]] = (y as f64) * 10.0;
            }
        }

        let aspect = aspect_horn(&dem, 10.0, FlatHandling::NoDirection, None)
            .expect("aspect calculation failed");

        // North-facing slope should have aspect around 0 or 360 degrees
        let center_aspect = aspect[[2, 2]];
        assert!(!(5.0..=355.0).contains(&center_aspect));
    }

    #[test]
    fn test_flat_handling() {
        let dem = Array2::from_elem((5, 5), 100.0_f64);

        let aspect_neg1 = aspect_horn(&dem, 10.0, FlatHandling::NoDirection, None).expect("failed");
        assert_relative_eq!(aspect_neg1[[2, 2]], -1.0, epsilon = 1e-10);

        let aspect_north = aspect_horn(&dem, 10.0, FlatHandling::North, None).expect("failed");
        assert_relative_eq!(aspect_north[[2, 2]], 0.0, epsilon = 1e-10);

        let aspect_nan = aspect_horn(&dem, 10.0, FlatHandling::NaN, None).expect("failed");
        assert!(aspect_nan[[2, 2]].is_nan());
    }

    #[test]
    fn test_aspect_range() {
        let mut dem = Array2::zeros((10, 10));
        for y in 0..10 {
            for x in 0..10 {
                dem[[y, x]] = ((x as f64).sin() + (y as f64).cos()) * 100.0;
            }
        }

        let aspect = aspect_horn(&dem, 10.0, FlatHandling::NoDirection, None)
            .expect("aspect calculation failed");

        // All aspect values should be in valid range
        for &a in aspect.iter() {
            if !a.is_nan() && a >= 0.0 {
                assert!((0.0..360.0).contains(&a), "aspect {} out of range", a);
            }
        }
    }
}
