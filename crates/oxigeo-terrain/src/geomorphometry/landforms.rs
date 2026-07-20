//! Landform classification algorithms.

use crate::derivatives::slope::SlopeUnits;
use crate::derivatives::slope::slope_horn;
use crate::derivatives::tpi::tpi;
use crate::error::Result;
use num_traits::Float;
use scirs2_core::prelude::*;

/// Landform classification types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LandformClass {
    /// Flat or nearly flat
    Flat = 1,
    /// Valley or depression
    Valley = 2,
    /// Lower slope
    LowerSlope = 3,
    /// Mid slope
    MidSlope = 4,
    /// Upper slope
    UpperSlope = 5,
    /// Ridge or peak
    Ridge = 6,
}

/// Classify landforms using TPI-based method (Weiss 2001).
pub fn classify_weiss<T>(
    dem: &Array2<T>,
    cell_size: f64,
    small_radius: usize,
    large_radius: usize,
    nodata: Option<T>,
) -> Result<Array2<u8>>
where
    T: Float + Into<f64> + Copy,
{
    let tpi_small = tpi(dem, small_radius, nodata)?;
    let tpi_large = tpi(dem, large_radius, nodata)?;
    let slope = slope_horn(dem, cell_size, SlopeUnits::Degrees, nodata)?;

    let (height, width) = dem.dim();
    let mut landforms = Array2::zeros((height, width));

    for y in 0..height {
        for x in 0..width {
            let tpi_s = tpi_small[[y, x]];
            let tpi_l = tpi_large[[y, x]];
            let slp = slope[[y, x]];

            landforms[[y, x]] = if slp < 2.0 {
                LandformClass::Flat as u8
            } else if tpi_s > 1.0 && tpi_l > 1.0 {
                LandformClass::Ridge as u8
            } else if tpi_s < -1.0 && tpi_l < -1.0 {
                LandformClass::Valley as u8
            } else if tpi_s > 0.5 {
                LandformClass::UpperSlope as u8
            } else if tpi_s < -0.5 {
                LandformClass::LowerSlope as u8
            } else {
                LandformClass::MidSlope as u8
            };
        }
    }

    Ok(landforms)
}

/// Iwahashi-Pike landform classification.
pub fn classify_iwahashi_pike<T>(
    dem: &Array2<T>,
    cell_size: f64,
    nodata: Option<T>,
) -> Result<Array2<u8>>
where
    T: Float + Into<f64> + Copy,
{
    use crate::derivatives::curvature::profile_curvature;
    use crate::derivatives::roughness::roughness_stddev;

    let slope = slope_horn(dem, cell_size, SlopeUnits::Degrees, nodata)?;
    let curvature = profile_curvature(dem, cell_size, nodata)?;
    let roughness = roughness_stddev(dem, 1, nodata)?;

    let (height, width) = dem.dim();
    let mut landforms = Array2::zeros((height, width));

    for y in 0..height {
        for x in 0..width {
            let s = slope[[y, x]];
            let c = curvature[[y, x]];
            let r = roughness[[y, x]];

            // Simplified Iwahashi-Pike classification
            landforms[[y, x]] = if s < 5.0 {
                if r < 2.0 { 1 } else { 2 }
            } else if s < 15.0 {
                if c > 0.0 { 3 } else { 4 }
            } else {
                if c > 0.0 { 5 } else { 6 }
            };
        }
    }

    Ok(landforms)
}
