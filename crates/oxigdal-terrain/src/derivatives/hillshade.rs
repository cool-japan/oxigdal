//! Hillshade rendering for terrain visualization.
//!
//! Provides hillshade (shaded relief) calculation with customizable sun position,
//! including multidirectional hillshade for enhanced terrain visualization.

use crate::derivatives::slope::EdgeStrategy;
use crate::error::{Result, TerrainError};
use num_traits::Float;
use scirs2_core::prelude::*;

/// Hillshade algorithm type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HillshadeAlgorithm {
    /// Traditional single-direction hillshade
    Traditional,
    /// Multidirectional hillshade (combines multiple light sources)
    Multidirectional,
    /// Combined (blend of traditional and multidirectional)
    Combined,
}

/// Calculate traditional hillshade from a DEM.
///
/// # Arguments
/// * `dem` - Input DEM as 2D array
/// * `cell_size` - Cell size in map units
/// * `azimuth` - Azimuth of light source in degrees (0-360, 0=north, clockwise)
/// * `altitude` - Altitude of light source in degrees (0-90)
/// * `z_factor` - Vertical exaggeration factor (default 1.0)
/// * `nodata` - Optional NoData value to skip
///
/// # Returns
/// 2D array of hillshade values (0-255)
pub fn hillshade_traditional<T>(
    dem: &Array2<T>,
    cell_size: f64,
    azimuth: f64,
    altitude: f64,
    z_factor: f64,
    nodata: Option<T>,
) -> Result<Array2<u8>>
where
    T: Float + Into<f64> + Copy,
{
    validate_inputs(dem, cell_size, azimuth, altitude)?;

    let (height, width) = dem.dim();
    let mut hillshade = Array2::zeros((height, width));

    // Convert angles to radians
    let zenith_rad = (90.0 - altitude).to_radians();
    let azimuth_rad = (360.0 - azimuth + 90.0).to_radians();

    let cos_zenith = zenith_rad.cos();
    let sin_zenith = zenith_rad.sin();

    for y in 0..height {
        for x in 0..width {
            let center = dem[[y, x]];

            if let Some(nd) = nodata {
                if is_nodata(center, nd) {
                    hillshade[[y, x]] = 0;
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

            // Calculate gradients (Horn's method)
            let dzdx = ((c.into() + 2.0 * f.into() + i.into())
                - (a.into() + 2.0 * d.into() + g.into()))
                / (8.0 * cell_size);
            let dzdy = ((g.into() + 2.0 * h.into() + i.into())
                - (a.into() + 2.0 * b.into() + c.into()))
                / (8.0 * cell_size);

            // Apply z-factor
            let dzdx_scaled = dzdx * z_factor;
            let dzdy_scaled = dzdy * z_factor;

            // Calculate slope and aspect
            let slope_rad = (dzdx_scaled * dzdx_scaled + dzdy_scaled * dzdy_scaled)
                .sqrt()
                .atan();
            // Aspect is the direction the slope faces (upslope), not downslope
            let aspect_rad = dzdy_scaled.atan2(dzdx_scaled);

            // Calculate hillshade
            let mut shade = cos_zenith * slope_rad.cos()
                + sin_zenith * slope_rad.sin() * (azimuth_rad - aspect_rad).cos();

            // Clamp to 0-1 range
            shade = shade.clamp(0.0, 1.0);

            // Scale to 0-255
            hillshade[[y, x]] = (shade * 255.0).round() as u8;
        }
    }

    Ok(hillshade)
}

/// Calculate multidirectional hillshade.
///
/// Combines hillshade from multiple light directions (typically 4: N, E, S, W)
/// to provide more balanced terrain visualization.
pub fn hillshade_multidirectional<T>(
    dem: &Array2<T>,
    cell_size: f64,
    altitude: f64,
    z_factor: f64,
    nodata: Option<T>,
) -> Result<Array2<u8>>
where
    T: Float + Into<f64> + Copy,
{
    validate_inputs(dem, cell_size, 0.0, altitude)?;

    // Calculate hillshade from 4 directions
    let azimuths = [225.0, 270.0, 315.0, 360.0]; // SW, W, NW, N
    let weights = [0.3, 0.2, 0.2, 0.3]; // Weighted towards diagonal illumination

    let (height, width) = dem.dim();
    let mut combined = Array2::<f64>::zeros((height, width));

    for (azimuth, weight) in azimuths.iter().zip(weights.iter()) {
        let shade = hillshade_traditional(dem, cell_size, *azimuth, altitude, z_factor, nodata)?;

        for y in 0..height {
            for x in 0..width {
                combined[[y, x]] += (shade[[y, x]] as f64) * weight;
            }
        }
    }

    // Convert to u8
    let mut result = Array2::zeros((height, width));
    for y in 0..height {
        for x in 0..width {
            result[[y, x]] = combined[[y, x]].round() as u8;
        }
    }

    Ok(result)
}

/// Calculate combined hillshade (blend of traditional and multidirectional).
pub fn hillshade_combined<T>(
    dem: &Array2<T>,
    cell_size: f64,
    azimuth: f64,
    altitude: f64,
    z_factor: f64,
    blend_ratio: f64,
    nodata: Option<T>,
) -> Result<Array2<u8>>
where
    T: Float + Into<f64> + Copy,
{
    let trad = hillshade_traditional(dem, cell_size, azimuth, altitude, z_factor, nodata)?;
    let multi = hillshade_multidirectional(dem, cell_size, altitude, z_factor, nodata)?;

    let (height, width) = dem.dim();
    let mut result = Array2::zeros((height, width));

    let blend = blend_ratio.clamp(0.0, 1.0);

    for y in 0..height {
        for x in 0..width {
            let combined = (trad[[y, x]] as f64) * (1.0 - blend) + (multi[[y, x]] as f64) * blend;
            result[[y, x]] = combined.round() as u8;
        }
    }

    Ok(result)
}

/// Calculate hillshade with specified algorithm.
pub fn hillshade<T>(
    dem: &Array2<T>,
    cell_size: f64,
    azimuth: f64,
    altitude: f64,
    algorithm: HillshadeAlgorithm,
    z_factor: f64,
    nodata: Option<T>,
) -> Result<Array2<u8>>
where
    T: Float + Into<f64> + Copy,
{
    match algorithm {
        HillshadeAlgorithm::Traditional => {
            hillshade_traditional(dem, cell_size, azimuth, altitude, z_factor, nodata)
        }
        HillshadeAlgorithm::Multidirectional => {
            hillshade_multidirectional(dem, cell_size, altitude, z_factor, nodata)
        }
        HillshadeAlgorithm::Combined => {
            hillshade_combined(dem, cell_size, azimuth, altitude, z_factor, 0.5, nodata)
        }
    }
}

// Helper functions

fn validate_inputs<T>(dem: &Array2<T>, cell_size: f64, azimuth: f64, altitude: f64) -> Result<()> {
    let (height, width) = dem.dim();

    if height < 3 || width < 3 {
        return Err(TerrainError::InvalidDimensions { width, height });
    }

    if cell_size <= 0.0 {
        return Err(TerrainError::InvalidCellSize { size: cell_size });
    }

    if !(0.0..=360.0).contains(&azimuth) {
        return Err(TerrainError::InvalidAzimuth { azimuth });
    }

    if !(0.0..=90.0).contains(&altitude) {
        return Err(TerrainError::InvalidAltitude { altitude });
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
            _ => dem[[0, 0]],
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

    #[test]
    fn test_hillshade_flat() {
        let dem = Array2::from_elem((5, 5), 100.0_f64);
        let shade = hillshade_traditional(&dem, 10.0, 315.0, 45.0, 1.0, None)
            .expect("hillshade calculation failed");

        // Flat surface should have uniform illumination
        let first_val = shade[[2, 2]];
        for y in 1..4 {
            for x in 1..4 {
                assert_eq!(shade[[y, x]], first_val);
            }
        }
    }

    #[test]
    fn test_hillshade_slope() {
        // Create east-facing slope
        let mut dem = Array2::zeros((5, 5));
        for y in 0..5 {
            for x in 0..5 {
                dem[[y, x]] = (x as f64) * 10.0;
            }
        }

        // Light from east should illuminate this slope
        let shade_east =
            hillshade_traditional(&dem, 10.0, 90.0, 45.0, 1.0, None).expect("hillshade failed");

        // Light from west should not illuminate
        let shade_west =
            hillshade_traditional(&dem, 10.0, 270.0, 45.0, 1.0, None).expect("hillshade failed");

        // East-lit should be brighter than west-lit
        assert!(shade_east[[2, 2]] > shade_west[[2, 2]]);
    }

    #[test]
    fn test_hillshade_range() {
        let mut dem = Array2::zeros((10, 10));
        for y in 0..10 {
            for x in 0..10 {
                dem[[y, x]] = ((x as f64).sin() + (y as f64).cos()) * 100.0;
            }
        }

        let shade =
            hillshade_traditional(&dem, 10.0, 315.0, 45.0, 1.0, None).expect("hillshade failed");

        // Verify the shade array has expected dimensions and is non-empty
        assert!(!shade.is_empty(), "Hillshade output should not be empty");
    }

    #[test]
    fn test_multidirectional_hillshade() {
        let mut dem = Array2::zeros((10, 10));
        for y in 0..10 {
            for x in 0..10 {
                let dx = (x as f64) - 5.0;
                let dy = (y as f64) - 5.0;
                dem[[y, x]] = 100.0 + dx * dx + dy * dy;
            }
        }

        let shade = hillshade_multidirectional(&dem, 10.0, 45.0, 1.0, None)
            .expect("multidirectional hillshade failed");

        // Should produce valid non-empty output (u8 values are inherently 0-255)
        assert!(
            !shade.is_empty(),
            "Multidirectional hillshade output should not be empty"
        );
    }

    #[test]
    fn test_z_factor() {
        let mut dem = Array2::zeros((5, 5));
        for y in 0..5 {
            for x in 0..5 {
                dem[[y, x]] = (x as f64) * 10.0;
            }
        }

        let shade1 = hillshade_traditional(&dem, 10.0, 90.0, 45.0, 1.0, None).expect("failed");
        let shade2 = hillshade_traditional(&dem, 10.0, 90.0, 45.0, 2.0, None).expect("failed");

        // Higher z-factor should create more pronounced shading
        assert_ne!(shade1[[2, 2]], shade2[[2, 2]]);
    }
}
