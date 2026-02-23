//! Slope and aspect calculation algorithms

use crate::error::{AlgorithmError, Result};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;

#[cfg(feature = "parallel")]
use rayon::prelude::*;

pub fn compute_aspect_degrees(dem: &RasterBuffer, cell_size: f64) -> Result<RasterBuffer> {
    use crate::raster::slope_aspect;
    let slope_aspect_output = slope_aspect::compute_slope_aspect(dem, cell_size, 1.0)?;
    Ok(slope_aspect_output.aspect)
}

/// Computes terrain slope in degrees
///
/// This is a convenience function that wraps the slope_aspect module.
///
/// # Arguments
///
/// * `dem` - Digital elevation model
/// * `cell_size` - Size of each cell
///
/// # Errors
///
/// Returns an error if the operation fails
pub fn compute_slope_degrees(dem: &RasterBuffer, cell_size: f64) -> Result<RasterBuffer> {
    use crate::raster::slope_aspect;
    let slope_aspect_output = slope_aspect::compute_slope_aspect(dem, cell_size, 1.0)?;
    Ok(slope_aspect_output.slope)
}

// ===========================================================================
// Helpers
// ===========================================================================

/// Extracts a 3x3 window of elevation values around the given pixel
pub(crate) fn get_3x3_window(dem: &RasterBuffer, x: u64, y: u64) -> Result<[[f64; 3]; 3]> {
    Ok([
        [
            dem.get_pixel(x - 1, y - 1).map_err(AlgorithmError::Core)?,
            dem.get_pixel(x, y - 1).map_err(AlgorithmError::Core)?,
            dem.get_pixel(x + 1, y - 1).map_err(AlgorithmError::Core)?,
        ],
        [
            dem.get_pixel(x - 1, y).map_err(AlgorithmError::Core)?,
            dem.get_pixel(x, y).map_err(AlgorithmError::Core)?,
            dem.get_pixel(x + 1, y).map_err(AlgorithmError::Core)?,
        ],
        [
            dem.get_pixel(x - 1, y + 1).map_err(AlgorithmError::Core)?,
            dem.get_pixel(x, y + 1).map_err(AlgorithmError::Core)?,
            dem.get_pixel(x + 1, y + 1).map_err(AlgorithmError::Core)?,
        ],
    ])
}

// ===========================================================================
// Tests
// ===========================================================================
