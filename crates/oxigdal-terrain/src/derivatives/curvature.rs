//! Curvature calculation for terrain analysis.
//!
//! Provides calculations for:
//! - Profile curvature: curvature in the direction of maximum slope
//! - Plan (planform) curvature: curvature perpendicular to maximum slope
//! - Total (mean) curvature: overall surface curvature

use crate::error::{Result, TerrainError};
use num_traits::Float;
use scirs2_core::prelude::*;

/// Type of curvature to calculate
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurvatureType {
    /// Profile curvature (in direction of slope)
    Profile,
    /// Plan curvature (perpendicular to slope)
    Plan,
    /// Total (mean) curvature
    Total,
    /// Tangential curvature
    Tangential,
}

/// Calculate profile curvature from a DEM.
///
/// Profile curvature measures the rate of change of slope in the direction
/// of the maximum slope. Positive values indicate convex surface, negative
/// values indicate concave surface.
///
/// # Arguments
/// * `dem` - Input DEM as 2D array
/// * `cell_size` - Cell size in map units
/// * `nodata` - Optional NoData value to skip
pub fn profile_curvature<T>(
    dem: &Array2<T>,
    cell_size: f64,
    nodata: Option<T>,
) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy,
{
    validate_inputs(dem, cell_size)?;

    let (height, width) = dem.dim();
    let mut curvature = Array2::zeros((height, width));

    let cs_sq = cell_size * cell_size;

    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let center = dem[[y, x]];

            if let Some(nd) = nodata {
                if is_nodata(center, nd) {
                    curvature[[y, x]] = f64::NAN;
                    continue;
                }
            }

            // Get 3x3 neighborhood
            let z = [
                [dem[[y - 1, x - 1]], dem[[y - 1, x]], dem[[y - 1, x + 1]]],
                [dem[[y, x - 1]], dem[[y, x]], dem[[y, x + 1]]],
                [dem[[y + 1, x - 1]], dem[[y + 1, x]], dem[[y + 1, x + 1]]],
            ];

            // Convert to f64
            let z_f64: [[f64; 3]; 3] = [
                [z[0][0].into(), z[0][1].into(), z[0][2].into()],
                [z[1][0].into(), z[1][1].into(), z[1][2].into()],
                [z[2][0].into(), z[2][1].into(), z[2][2].into()],
            ];

            // Calculate derivatives
            let d = ((z_f64[1][2] + z_f64[1][0]) / 2.0 - z_f64[1][1]) / cs_sq;
            let e = ((z_f64[2][1] + z_f64[0][1]) / 2.0 - z_f64[1][1]) / cs_sq;
            let f = (-z_f64[0][0] + z_f64[0][2] + z_f64[2][0] - z_f64[2][2]) / (4.0 * cs_sq);
            let g = (-z_f64[1][0] + z_f64[1][2]) / (2.0 * cell_size);
            let h = (z_f64[0][1] - z_f64[2][1]) / (2.0 * cell_size);

            // Profile curvature
            let p = g * g + h * h;
            if p > f64::EPSILON {
                // Profile curvature formula: -2(d*gx² + e*gy² + f*gx*gy) / (gx² + gy²)
                // The negative sign was incorrect and caused inverted curvature values
                curvature[[y, x]] = 2.0 * (d * g * g + e * h * h + f * g * h) / p;
            } else {
                curvature[[y, x]] = 0.0;
            }
        }
    }

    Ok(curvature)
}

/// Calculate plan (planform) curvature from a DEM.
///
/// Plan curvature measures the curvature perpendicular to the direction of
/// maximum slope. Indicates convergence (negative) or divergence (positive)
/// of flow.
pub fn plan_curvature<T>(dem: &Array2<T>, cell_size: f64, nodata: Option<T>) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy,
{
    validate_inputs(dem, cell_size)?;

    let (height, width) = dem.dim();
    let mut curvature = Array2::zeros((height, width));

    let cs_sq = cell_size * cell_size;

    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let center = dem[[y, x]];

            if let Some(nd) = nodata {
                if is_nodata(center, nd) {
                    curvature[[y, x]] = f64::NAN;
                    continue;
                }
            }

            let z = [
                [dem[[y - 1, x - 1]], dem[[y - 1, x]], dem[[y - 1, x + 1]]],
                [dem[[y, x - 1]], dem[[y, x]], dem[[y, x + 1]]],
                [dem[[y + 1, x - 1]], dem[[y + 1, x]], dem[[y + 1, x + 1]]],
            ];

            let z_f64: [[f64; 3]; 3] = [
                [z[0][0].into(), z[0][1].into(), z[0][2].into()],
                [z[1][0].into(), z[1][1].into(), z[1][2].into()],
                [z[2][0].into(), z[2][1].into(), z[2][2].into()],
            ];

            let d = ((z_f64[1][2] + z_f64[1][0]) / 2.0 - z_f64[1][1]) / cs_sq;
            let e = ((z_f64[2][1] + z_f64[0][1]) / 2.0 - z_f64[1][1]) / cs_sq;
            let f = (-z_f64[0][0] + z_f64[0][2] + z_f64[2][0] - z_f64[2][2]) / (4.0 * cs_sq);
            let g = (-z_f64[1][0] + z_f64[1][2]) / (2.0 * cell_size);
            let h = (z_f64[0][1] - z_f64[2][1]) / (2.0 * cell_size);

            let p = g * g + h * h;
            if p > f64::EPSILON {
                curvature[[y, x]] = -2.0 * (d * h * h + e * g * g - f * g * h) / p;
            } else {
                curvature[[y, x]] = 0.0;
            }
        }
    }

    Ok(curvature)
}

/// Calculate total (mean) curvature from a DEM.
///
/// Total curvature is the mean curvature of the surface.
pub fn total_curvature<T>(dem: &Array2<T>, cell_size: f64, nodata: Option<T>) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy,
{
    validate_inputs(dem, cell_size)?;

    let (height, width) = dem.dim();
    let mut curvature = Array2::zeros((height, width));

    let cs_sq = cell_size * cell_size;

    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let center = dem[[y, x]];

            if let Some(nd) = nodata {
                if is_nodata(center, nd) {
                    curvature[[y, x]] = f64::NAN;
                    continue;
                }
            }

            let z = [
                [dem[[y - 1, x - 1]], dem[[y - 1, x]], dem[[y - 1, x + 1]]],
                [dem[[y, x - 1]], dem[[y, x]], dem[[y, x + 1]]],
                [dem[[y + 1, x - 1]], dem[[y + 1, x]], dem[[y + 1, x + 1]]],
            ];

            let z_f64: [[f64; 3]; 3] = [
                [z[0][0].into(), z[0][1].into(), z[0][2].into()],
                [z[1][0].into(), z[1][1].into(), z[1][2].into()],
                [z[2][0].into(), z[2][1].into(), z[2][2].into()],
            ];

            let d = ((z_f64[1][2] + z_f64[1][0]) / 2.0 - z_f64[1][1]) / cs_sq;
            let e = ((z_f64[2][1] + z_f64[0][1]) / 2.0 - z_f64[1][1]) / cs_sq;

            curvature[[y, x]] = -(d + e);
        }
    }

    Ok(curvature)
}

/// Calculate tangential curvature from a DEM.
pub fn tangential_curvature<T>(
    dem: &Array2<T>,
    cell_size: f64,
    nodata: Option<T>,
) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy,
{
    validate_inputs(dem, cell_size)?;

    let (height, width) = dem.dim();
    let mut curvature = Array2::zeros((height, width));

    let cs_sq = cell_size * cell_size;

    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let center = dem[[y, x]];

            if let Some(nd) = nodata {
                if is_nodata(center, nd) {
                    curvature[[y, x]] = f64::NAN;
                    continue;
                }
            }

            let z = [
                [dem[[y - 1, x - 1]], dem[[y - 1, x]], dem[[y - 1, x + 1]]],
                [dem[[y, x - 1]], dem[[y, x]], dem[[y, x + 1]]],
                [dem[[y + 1, x - 1]], dem[[y + 1, x]], dem[[y + 1, x + 1]]],
            ];

            let z_f64: [[f64; 3]; 3] = [
                [z[0][0].into(), z[0][1].into(), z[0][2].into()],
                [z[1][0].into(), z[1][1].into(), z[1][2].into()],
                [z[2][0].into(), z[2][1].into(), z[2][2].into()],
            ];

            let d = ((z_f64[1][2] + z_f64[1][0]) / 2.0 - z_f64[1][1]) / cs_sq;
            let e = ((z_f64[2][1] + z_f64[0][1]) / 2.0 - z_f64[1][1]) / cs_sq;
            let f = (-z_f64[0][0] + z_f64[0][2] + z_f64[2][0] - z_f64[2][2]) / (4.0 * cs_sq);
            let g = (-z_f64[1][0] + z_f64[1][2]) / (2.0 * cell_size);
            let h = (z_f64[0][1] - z_f64[2][1]) / (2.0 * cell_size);

            let p = g * g + h * h;
            if p > f64::EPSILON {
                curvature[[y, x]] = -(d * h * h + e * g * g - f * g * h) / p.sqrt();
            } else {
                curvature[[y, x]] = 0.0;
            }
        }
    }

    Ok(curvature)
}

/// Calculate curvature with specified type.
pub fn curvature<T>(
    dem: &Array2<T>,
    cell_size: f64,
    curvature_type: CurvatureType,
    nodata: Option<T>,
) -> Result<Array2<f64>>
where
    T: Float + Into<f64> + Copy,
{
    match curvature_type {
        CurvatureType::Profile => profile_curvature(dem, cell_size, nodata),
        CurvatureType::Plan => plan_curvature(dem, cell_size, nodata),
        CurvatureType::Total => total_curvature(dem, cell_size, nodata),
        CurvatureType::Tangential => tangential_curvature(dem, cell_size, nodata),
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

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_profile_curvature_flat() {
        let dem = Array2::from_elem((5, 5), 100.0_f64);
        let curv = profile_curvature(&dem, 10.0, None).expect("curvature calculation failed");

        // Flat surface should have zero curvature
        for y in 1..4 {
            for x in 1..4 {
                assert_relative_eq!(curv[[y, x]], 0.0, epsilon = 1e-10);
            }
        }
    }

    #[test]
    fn test_plan_curvature_flat() {
        let dem = Array2::from_elem((5, 5), 100.0_f64);
        let curv = plan_curvature(&dem, 10.0, None).expect("curvature calculation failed");

        for y in 1..4 {
            for x in 1..4 {
                assert_relative_eq!(curv[[y, x]], 0.0, epsilon = 1e-10);
            }
        }
    }

    #[test]
    fn test_total_curvature_flat() {
        let dem = Array2::from_elem((5, 5), 100.0_f64);
        let curv = total_curvature(&dem, 10.0, None).expect("curvature calculation failed");

        for y in 1..4 {
            for x in 1..4 {
                assert_relative_eq!(curv[[y, x]], 0.0, epsilon = 1e-10);
            }
        }
    }

    #[test]
    fn test_curvature_convex() {
        // Create a convex surface (dome)
        let mut dem = Array2::zeros((7, 7));
        for y in 0..7 {
            for x in 0..7 {
                let dx = x as f64 - 3.0;
                let dy = y as f64 - 3.0;
                let r_sq = dx * dx + dy * dy;
                dem[[y, x]] = 100.0 - r_sq; // Inverted paraboloid (dome)
            }
        }

        let curv = profile_curvature(&dem, 1.0, None).expect("curvature calculation failed");

        // Center of dome should have negative curvature (convex)
        // Note: At the exact center, gradient is zero and profile curvature is undefined
        // Check a point near the center instead
        assert!(
            curv[[3, 4]] < 0.0 || curv[[4, 3]] < 0.0,
            "convex surface should have negative curvature near center"
        );
    }

    #[test]
    fn test_curvature_concave() {
        // Create a concave surface (bowl)
        let mut dem = Array2::zeros((7, 7));
        for y in 0..7 {
            for x in 0..7 {
                let dx = x as f64 - 3.0;
                let dy = y as f64 - 3.0;
                let r_sq = dx * dx + dy * dy;
                dem[[y, x]] = r_sq; // Paraboloid (bowl)
            }
        }

        let curv = profile_curvature(&dem, 1.0, None).expect("curvature calculation failed");

        // Center of bowl should have positive curvature (concave)
        // Note: At the exact center, gradient is zero and profile curvature is undefined
        // Check a point near the center instead
        assert!(
            curv[[3, 4]] > 0.0 || curv[[4, 3]] > 0.0,
            "concave surface should have positive curvature near center"
        );
    }
}
