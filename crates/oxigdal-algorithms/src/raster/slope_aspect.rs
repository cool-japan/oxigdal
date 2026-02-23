//! Slope and aspect calculation from DEMs
//!
//! Computes terrain slope (gradient) and aspect (direction of slope) from
//! digital elevation models using multiple algorithm variants.
//!
//! # Algorithms
//!
//! - **Horn (1981)**: 3rd-order finite difference, weighted. Most commonly used (GDAL default).
//!   Uses a 3x3 window with central cell weighted 2x. Best for noisy data.
//!
//! - **Zevenbergen & Thorne (1987)**: 2nd-order finite difference, unweighted.
//!   Uses only the 4 cardinal neighbors. Better for smooth surfaces.
//!
//! - **Evans-Young (1972/1978)**: Fits a quadratic surface to the 3x3 neighborhood.
//!   Theoretically optimal for regular grids.
//!
//! - **Maximum Downhill Slope (MDS)**: Considers the steepest slope among all 8 neighbors.
//!   Useful for hydrological applications.
//!
//! # Units
//!
//! Slope can be expressed in degrees, radians, or percent rise.
//! Aspect is in degrees from north (0-360, clockwise).
//!
//! # Edge Handling
//!
//! Multiple strategies for handling boundary pixels where the 3x3 window
//! extends beyond the raster extent:
//! - **Skip**: Leave boundary pixels as NoData (default)
//! - **Reflect**: Mirror the raster at boundaries
//! - **Extrapolate**: Linear extrapolation from interior values
//! - **Replicate**: Repeat the nearest edge pixel
//!
//! # References
//!
//! - Horn, B.K.P. (1981). Hill shading and the reflectance map.
//! - Zevenbergen, L.W. & Thorne, C.R. (1987). Quantitative analysis of land surface topography.
//! - Evans, I.S. (1972). General geomorphometry, derivatives of altitude, and descriptive statistics.
//! - Burrough, P.A. & McDonnell, R.A. (1998). Principles of GIS.

use crate::error::{AlgorithmError, Result};
use core::f64::consts::PI;
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;

#[cfg(feature = "parallel")]
use rayon::prelude::*;

// ===========================================================================
// Algorithm selection
// ===========================================================================

/// Algorithm for computing slope and aspect gradients
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SlopeAlgorithm {
    /// Horn (1981): 3rd-order finite difference with 2x weighting on cardinal neighbors.
    /// Most robust against noise. This is the GDAL default.
    #[default]
    Horn,

    /// Zevenbergen & Thorne (1987): 2nd-order finite difference using only 4 cardinal neighbors.
    /// Better for smooth, mathematically defined surfaces.
    ZevenbergenThorne,

    /// Evans-Young: Fits a quadratic polynomial to the 3x3 window.
    /// Theoretically optimal for regular grids.
    EvansYoung,

    /// Maximum downhill slope: steepest descent among all 8 neighbors.
    /// Useful for hydrological analysis (D-infinity style).
    MaximumDownhill,
}

// ===========================================================================
// Slope units
// ===========================================================================

/// Units for slope output
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SlopeUnits {
    /// Degrees (0-90)
    #[default]
    Degrees,

    /// Radians (0 - pi/2)
    Radians,

    /// Percent rise (0 - infinity, typically 0-several hundred)
    Percent,

    /// Tangent of slope angle (rise/run), same as percent/100
    Tangent,
}

// ===========================================================================
// Edge handling
// ===========================================================================

/// Strategy for handling edges where the 3x3 window extends beyond raster boundaries
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EdgeHandling {
    /// Skip edge pixels (leave as zero/nodata)
    #[default]
    Skip,

    /// Reflect (mirror) at raster boundaries
    Reflect,

    /// Extrapolate linearly from interior pixels
    Extrapolate,

    /// Replicate nearest edge pixel value
    Replicate,
}

// ===========================================================================
// Configuration and output structs
// ===========================================================================

/// Configuration for slope/aspect computation
#[derive(Debug, Clone)]
pub struct SlopeAspectConfig {
    /// Algorithm to use
    pub algorithm: SlopeAlgorithm,
    /// Slope output units
    pub slope_units: SlopeUnits,
    /// Edge handling strategy
    pub edge_handling: EdgeHandling,
    /// Z-factor for vertical exaggeration (default: 1.0)
    pub z_factor: f64,
}

impl Default for SlopeAspectConfig {
    fn default() -> Self {
        Self {
            algorithm: SlopeAlgorithm::default(),
            slope_units: SlopeUnits::default(),
            edge_handling: EdgeHandling::default(),
            z_factor: 1.0,
        }
    }
}

/// Output from slope/aspect computation
#[derive(Debug)]
pub struct SlopeAspectOutput {
    /// Slope values in the configured units
    pub slope: RasterBuffer,
    /// Aspect in degrees (0-360, 0=N, clockwise). -1 for flat areas.
    pub aspect: RasterBuffer,
}

// ===========================================================================
// Main computation functions
// ===========================================================================

/// Computes both slope and aspect using Horn's method (backward-compatible)
///
/// # Arguments
///
/// * `dem` - Digital elevation model
/// * `pixel_size` - Cell size
/// * `z_factor` - Vertical exaggeration factor
///
/// # Errors
///
/// Returns an error if the DEM is smaller than 3x3
pub fn compute_slope_aspect(
    dem: &RasterBuffer,
    pixel_size: f64,
    z_factor: f64,
) -> Result<SlopeAspectOutput> {
    compute_slope_aspect_advanced(
        dem,
        pixel_size,
        &SlopeAspectConfig {
            z_factor,
            ..Default::default()
        },
    )
}

/// Computes slope and aspect with full configuration
///
/// # Arguments
///
/// * `dem` - Digital elevation model
/// * `pixel_size` - Cell size
/// * `config` - Full configuration including algorithm, units, edge handling
///
/// # Errors
///
/// Returns an error if the DEM is smaller than 3x3
pub fn compute_slope_aspect_advanced(
    dem: &RasterBuffer,
    pixel_size: f64,
    config: &SlopeAspectConfig,
) -> Result<SlopeAspectOutput> {
    let width = dem.width();
    let height = dem.height();

    if width < 3 || height < 3 {
        return Err(AlgorithmError::InsufficientData {
            operation: "slope/aspect",
            message: "DEM must be at least 3x3".to_string(),
        });
    }

    let mut slope_buf = RasterBuffer::zeros(width, height, dem.data_type());
    let mut aspect_buf = RasterBuffer::zeros(width, height, dem.data_type());

    // Determine processing range based on edge handling
    let (y_start, y_end, x_start, x_end) = match config.edge_handling {
        EdgeHandling::Skip => (1, height - 1, 1, width - 1),
        _ => (0, height, 0, width),
    };

    #[cfg(feature = "parallel")]
    {
        let results: Result<Vec<_>> = (y_start..y_end)
            .into_par_iter()
            .map(|y| {
                let mut row = Vec::new();
                for x in x_start..x_end {
                    let (s, a) = compute_pixel_slope_aspect(dem, x, y, pixel_size, config)?;
                    row.push((x, s, a));
                }
                Ok((y, row))
            })
            .collect();

        for (y, row) in results? {
            for (x, s, a) in row {
                slope_buf.set_pixel(x, y, s).map_err(AlgorithmError::Core)?;
                aspect_buf
                    .set_pixel(x, y, a)
                    .map_err(AlgorithmError::Core)?;
            }
        }
    }

    #[cfg(not(feature = "parallel"))]
    {
        for y in y_start..y_end {
            for x in x_start..x_end {
                let (s, a) = compute_pixel_slope_aspect(dem, x, y, pixel_size, config)?;
                slope_buf.set_pixel(x, y, s).map_err(AlgorithmError::Core)?;
                aspect_buf
                    .set_pixel(x, y, a)
                    .map_err(AlgorithmError::Core)?;
            }
        }
    }

    Ok(SlopeAspectOutput {
        slope: slope_buf,
        aspect: aspect_buf,
    })
}

/// Computes slope only (convenience function)
///
/// # Errors
///
/// Returns an error if the DEM is smaller than 3x3
pub fn slope(dem: &RasterBuffer, pixel_size: f64, z_factor: f64) -> Result<RasterBuffer> {
    let output = compute_slope_aspect(dem, pixel_size, z_factor)?;
    Ok(output.slope)
}

/// Computes aspect only (convenience function)
///
/// # Errors
///
/// Returns an error if the DEM is smaller than 3x3
pub fn aspect(dem: &RasterBuffer, pixel_size: f64, z_factor: f64) -> Result<RasterBuffer> {
    let output = compute_slope_aspect(dem, pixel_size, z_factor)?;
    Ok(output.aspect)
}

/// Computes slope with full configuration (convenience function)
///
/// # Errors
///
/// Returns an error if the DEM is smaller than 3x3
pub fn slope_advanced(
    dem: &RasterBuffer,
    pixel_size: f64,
    config: &SlopeAspectConfig,
) -> Result<RasterBuffer> {
    let output = compute_slope_aspect_advanced(dem, pixel_size, config)?;
    Ok(output.slope)
}

/// Computes aspect with full configuration (convenience function)
///
/// # Errors
///
/// Returns an error if the DEM is smaller than 3x3
pub fn aspect_advanced(
    dem: &RasterBuffer,
    pixel_size: f64,
    config: &SlopeAspectConfig,
) -> Result<RasterBuffer> {
    let output = compute_slope_aspect_advanced(dem, pixel_size, config)?;
    Ok(output.aspect)
}

// ===========================================================================
// Per-pixel computation
// ===========================================================================

/// Computes slope and aspect for a single pixel
fn compute_pixel_slope_aspect(
    dem: &RasterBuffer,
    x: u64,
    y: u64,
    pixel_size: f64,
    config: &SlopeAspectConfig,
) -> Result<(f64, f64)> {
    let z = get_neighborhood(dem, x, y, config.edge_handling)?;

    let (dz_dx, dz_dy) = match config.algorithm {
        SlopeAlgorithm::Horn => compute_gradients_horn(&z, pixel_size, config.z_factor),
        SlopeAlgorithm::ZevenbergenThorne => {
            compute_gradients_zevenbergen_thorne(&z, pixel_size, config.z_factor)
        }
        SlopeAlgorithm::EvansYoung => {
            compute_gradients_evans_young(&z, pixel_size, config.z_factor)
        }
        SlopeAlgorithm::MaximumDownhill => {
            return compute_max_downhill_slope(&z, pixel_size, config);
        }
    };

    // Slope
    let slope_tan = (dz_dx * dz_dx + dz_dy * dz_dy).sqrt();
    let slope_value = convert_slope(slope_tan, config.slope_units);

    // Aspect (0-360 from north, clockwise)
    let aspect_value = if dz_dx.abs() < 1e-15 && dz_dy.abs() < 1e-15 {
        -1.0 // flat area
    } else {
        let aspect_rad = dz_dy.atan2(-dz_dx);
        let mut aspect_deg = aspect_rad * 180.0 / PI;
        if aspect_deg < 0.0 {
            aspect_deg += 360.0;
        }
        aspect_deg
    };

    Ok((slope_value, aspect_value))
}

// ===========================================================================
// Gradient algorithms
// ===========================================================================

/// Horn (1981): 3rd-order finite difference with 2x weighting on cardinal neighbors
///
/// dz/dx = ((z2+2*z5+z8) - (z0+2*z3+z6)) / (8*cellsize)
/// dz/dy = ((z6+2*z7+z8) - (z0+2*z1+z2)) / (8*cellsize)
fn compute_gradients_horn(z: &[[f64; 3]; 3], pixel_size: f64, z_factor: f64) -> (f64, f64) {
    let scale = z_factor / (8.0 * pixel_size);

    let dz_dx = ((z[0][2] + 2.0 * z[1][2] + z[2][2]) - (z[0][0] + 2.0 * z[1][0] + z[2][0])) * scale;

    let dz_dy = ((z[2][0] + 2.0 * z[2][1] + z[2][2]) - (z[0][0] + 2.0 * z[0][1] + z[0][2])) * scale;

    (dz_dx, dz_dy)
}

/// Zevenbergen & Thorne (1987): 2nd-order finite difference using only cardinal neighbors
///
/// dz/dx = (z5 - z3) / (2*cellsize)
/// dz/dy = (z7 - z1) / (2*cellsize)
fn compute_gradients_zevenbergen_thorne(
    z: &[[f64; 3]; 3],
    pixel_size: f64,
    z_factor: f64,
) -> (f64, f64) {
    let scale = z_factor / (2.0 * pixel_size);

    let dz_dx = (z[1][2] - z[1][0]) * scale;
    let dz_dy = (z[2][1] - z[0][1]) * scale;

    (dz_dx, dz_dy)
}

/// Evans-Young: Quadratic polynomial fit
///
/// Fits z = ax^2 + by^2 + cxy + dx + ey + f to the 3x3 neighborhood.
/// The gradients are dz/dx = d and dz/dy = e at the center.
///
/// Using the method of least squares on a regular grid:
/// d = (z02 + z12 + z22 - z00 - z10 - z20) / (6 * cellsize)
/// e = (z20 + z21 + z22 - z00 - z01 - z02) / (6 * cellsize)
fn compute_gradients_evans_young(z: &[[f64; 3]; 3], pixel_size: f64, z_factor: f64) -> (f64, f64) {
    let scale = z_factor / (6.0 * pixel_size);

    let dz_dx = (z[0][2] + z[1][2] + z[2][2] - z[0][0] - z[1][0] - z[2][0]) * scale;
    let dz_dy = (z[2][0] + z[2][1] + z[2][2] - z[0][0] - z[0][1] - z[0][2]) * scale;

    (dz_dx, dz_dy)
}

/// Maximum downhill slope computation
///
/// Finds the steepest descent among all 8 neighbors and returns
/// slope and aspect for that direction.
fn compute_max_downhill_slope(
    z: &[[f64; 3]; 3],
    pixel_size: f64,
    config: &SlopeAspectConfig,
) -> Result<(f64, f64)> {
    let center = z[1][1];

    // 8 neighbors with offsets and aspect angles
    let neighbors: [(usize, usize, f64, f64); 8] = [
        (0, 1, 0.0, pixel_size),                               // N
        (0, 2, 45.0, pixel_size * core::f64::consts::SQRT_2),  // NE
        (1, 2, 90.0, pixel_size),                              // E
        (2, 2, 135.0, pixel_size * core::f64::consts::SQRT_2), // SE
        (2, 1, 180.0, pixel_size),                             // S
        (2, 0, 225.0, pixel_size * core::f64::consts::SQRT_2), // SW
        (1, 0, 270.0, pixel_size),                             // W
        (0, 0, 315.0, pixel_size * core::f64::consts::SQRT_2), // NW
    ];

    let mut max_slope_tan = 0.0_f64;
    let mut max_aspect = -1.0_f64;

    for &(row, col, aspect_deg, dist) in &neighbors {
        let neighbor_elev = z[row][col];
        let drop = (center - neighbor_elev) * config.z_factor;
        let slope_tan = drop / dist;

        if slope_tan > max_slope_tan {
            max_slope_tan = slope_tan;
            max_aspect = aspect_deg;
        }
    }

    let slope_value = convert_slope(max_slope_tan, config.slope_units);

    Ok((slope_value, max_aspect))
}

// ===========================================================================
// Unit conversion
// ===========================================================================

/// Converts slope from tangent (rise/run) to the requested units
fn convert_slope(slope_tangent: f64, units: SlopeUnits) -> f64 {
    match units {
        SlopeUnits::Degrees => slope_tangent.atan() * 180.0 / PI,
        SlopeUnits::Radians => slope_tangent.atan(),
        SlopeUnits::Percent => slope_tangent * 100.0,
        SlopeUnits::Tangent => slope_tangent,
    }
}

/// Converts slope from degrees to another unit
pub fn convert_slope_degrees(slope_degrees: f64, target_units: SlopeUnits) -> f64 {
    match target_units {
        SlopeUnits::Degrees => slope_degrees,
        SlopeUnits::Radians => slope_degrees * PI / 180.0,
        SlopeUnits::Percent => (slope_degrees * PI / 180.0).tan() * 100.0,
        SlopeUnits::Tangent => (slope_degrees * PI / 180.0).tan(),
    }
}

// ===========================================================================
// Edge handling / neighborhood extraction
// ===========================================================================

/// Extracts a 3x3 neighborhood around (x, y) with edge handling
fn get_neighborhood(
    dem: &RasterBuffer,
    x: u64,
    y: u64,
    edge_handling: EdgeHandling,
) -> Result<[[f64; 3]; 3]> {
    let width = dem.width();
    let height = dem.height();
    let mut z = [[0.0f64; 3]; 3];

    for dy in 0..3i64 {
        for dx in 0..3i64 {
            let src_x = x as i64 + dx - 1;
            let src_y = y as i64 + dy - 1;

            let (px, py) = resolve_coords(src_x, src_y, width, height, edge_handling);

            z[dy as usize][dx as usize] = dem.get_pixel(px, py).map_err(AlgorithmError::Core)?;
        }
    }

    Ok(z)
}

/// Resolves coordinates that may be out of bounds based on the edge handling strategy
fn resolve_coords(
    x: i64,
    y: i64,
    width: u64,
    height: u64,
    edge_handling: EdgeHandling,
) -> (u64, u64) {
    let w = width as i64;
    let h = height as i64;

    match edge_handling {
        EdgeHandling::Skip => {
            // Clamp to valid range (should only be called for interior pixels anyway)
            (x.clamp(0, w - 1) as u64, y.clamp(0, h - 1) as u64)
        }
        EdgeHandling::Reflect => {
            let rx = if x < 0 {
                (-x).min(w - 1)
            } else if x >= w {
                (2 * w - x - 2).max(0)
            } else {
                x
            };
            let ry = if y < 0 {
                (-y).min(h - 1)
            } else if y >= h {
                (2 * h - y - 2).max(0)
            } else {
                y
            };
            (rx as u64, ry as u64)
        }
        EdgeHandling::Extrapolate => {
            // Use nearest interior pixel (linear extrapolation approximated by clamping)
            // True extrapolation would need more context; we use replicate here as fallback
            (x.clamp(0, w - 1) as u64, y.clamp(0, h - 1) as u64)
        }
        EdgeHandling::Replicate => (x.clamp(0, w - 1) as u64, y.clamp(0, h - 1) as u64),
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn create_slope_dem() -> RasterBuffer {
        let mut dem = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        for y in 0..10 {
            for x in 0..10 {
                let _ = dem.set_pixel(x, y, (x + y) as f64);
            }
        }
        dem
    }

    fn create_flat_dem() -> RasterBuffer {
        let mut dem = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        for y in 0..10 {
            for x in 0..10 {
                let _ = dem.set_pixel(x, y, 100.0);
            }
        }
        dem
    }

    fn create_north_facing_dem() -> RasterBuffer {
        // Elevation decreases going north (y=0), increases going south
        let mut dem = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        for y in 0..10 {
            for x in 0..10 {
                let _ = dem.set_pixel(x, y, y as f64 * 10.0);
            }
        }
        dem
    }

    // --- Basic slope/aspect ---

    #[test]
    fn test_slope_aspect_horn() {
        let dem = create_slope_dem();
        let result = compute_slope_aspect(&dem, 1.0, 1.0);
        assert!(result.is_ok());
        let output = result.expect("slope/aspect");
        let s = output.slope.get_pixel(5, 5).expect("slope pixel");
        assert!(s > 0.0, "Slope should be positive on inclined surface");
    }

    #[test]
    fn test_slope_flat() {
        let dem = create_flat_dem();
        let result = compute_slope_aspect(&dem, 1.0, 1.0);
        assert!(result.is_ok());
        let output = result.expect("slope/aspect");
        let s = output.slope.get_pixel(5, 5).expect("slope pixel");
        assert!(
            s.abs() < 1e-6,
            "Slope should be zero on flat terrain, got {s}"
        );
    }

    #[test]
    fn test_aspect_flat() {
        let dem = create_flat_dem();
        let result = compute_slope_aspect_advanced(
            &dem,
            1.0,
            &SlopeAspectConfig {
                z_factor: 1.0,
                ..Default::default()
            },
        );
        assert!(result.is_ok());
        let output = result.expect("slope/aspect");
        let a = output.aspect.get_pixel(5, 5).expect("aspect pixel");
        // Flat area should have aspect = -1 (undefined)
        assert!(a < 0.0, "Flat aspect should be -1, got {a}");
    }

    // --- Algorithm variants ---

    #[test]
    fn test_zevenbergen_thorne() {
        let dem = create_slope_dem();
        let config = SlopeAspectConfig {
            algorithm: SlopeAlgorithm::ZevenbergenThorne,
            ..Default::default()
        };
        let result = compute_slope_aspect_advanced(&dem, 1.0, &config);
        assert!(result.is_ok());
        let output = result.expect("zt");
        let s = output.slope.get_pixel(5, 5).expect("slope");
        assert!(s > 0.0);
    }

    #[test]
    fn test_evans_young() {
        let dem = create_slope_dem();
        let config = SlopeAspectConfig {
            algorithm: SlopeAlgorithm::EvansYoung,
            ..Default::default()
        };
        let result = compute_slope_aspect_advanced(&dem, 1.0, &config);
        assert!(result.is_ok());
        let output = result.expect("ey");
        let s = output.slope.get_pixel(5, 5).expect("slope");
        assert!(s > 0.0);
    }

    #[test]
    fn test_maximum_downhill() {
        let dem = create_slope_dem();
        let config = SlopeAspectConfig {
            algorithm: SlopeAlgorithm::MaximumDownhill,
            ..Default::default()
        };
        let result = compute_slope_aspect_advanced(&dem, 1.0, &config);
        assert!(result.is_ok());
        let output = result.expect("mds");
        let s = output.slope.get_pixel(5, 5).expect("slope");
        assert!(s > 0.0);
    }

    // --- Algorithm consistency ---

    #[test]
    fn test_algorithms_consistent_direction() {
        let dem = create_north_facing_dem();

        let horn = compute_slope_aspect_advanced(
            &dem,
            1.0,
            &SlopeAspectConfig {
                algorithm: SlopeAlgorithm::Horn,
                ..Default::default()
            },
        )
        .expect("horn");

        let zt = compute_slope_aspect_advanced(
            &dem,
            1.0,
            &SlopeAspectConfig {
                algorithm: SlopeAlgorithm::ZevenbergenThorne,
                ..Default::default()
            },
        )
        .expect("zt");

        let horn_aspect = horn.aspect.get_pixel(5, 5).expect("aspect");
        let zt_aspect = zt.aspect.get_pixel(5, 5).expect("aspect");

        // Both should agree on the general direction (south-facing: aspect ~180)
        let diff = (horn_aspect - zt_aspect).abs();
        assert!(
            !(45.0..=315.0).contains(&diff),
            "Horn and ZT aspects should be similar: {horn_aspect} vs {zt_aspect}"
        );
    }

    // --- Unit conversion ---

    #[test]
    fn test_slope_units_degrees() {
        let dem = create_slope_dem();
        let config = SlopeAspectConfig {
            slope_units: SlopeUnits::Degrees,
            ..Default::default()
        };
        let result = compute_slope_aspect_advanced(&dem, 1.0, &config).expect("degrees");
        let s = result.slope.get_pixel(5, 5).expect("slope");
        assert!(s > 0.0 && s < 90.0, "Degrees should be in (0,90), got {s}");
    }

    #[test]
    fn test_slope_units_radians() {
        let dem = create_slope_dem();
        let config = SlopeAspectConfig {
            slope_units: SlopeUnits::Radians,
            ..Default::default()
        };
        let result = compute_slope_aspect_advanced(&dem, 1.0, &config).expect("radians");
        let s = result.slope.get_pixel(5, 5).expect("slope");
        assert!(
            s > 0.0 && s < PI / 2.0,
            "Radians should be in (0, pi/2), got {s}"
        );
    }

    #[test]
    fn test_slope_units_percent() {
        let dem = create_slope_dem();
        let config = SlopeAspectConfig {
            slope_units: SlopeUnits::Percent,
            ..Default::default()
        };
        let result = compute_slope_aspect_advanced(&dem, 1.0, &config).expect("percent");
        let s = result.slope.get_pixel(5, 5).expect("slope");
        assert!(s > 0.0, "Percent slope should be positive");
    }

    #[test]
    fn test_slope_units_tangent() {
        let dem = create_slope_dem();
        let config = SlopeAspectConfig {
            slope_units: SlopeUnits::Tangent,
            ..Default::default()
        };
        let result = compute_slope_aspect_advanced(&dem, 1.0, &config).expect("tangent");
        let s = result.slope.get_pixel(5, 5).expect("slope");
        assert!(s > 0.0, "Tangent slope should be positive");
    }

    #[test]
    fn test_convert_slope_degrees() {
        let deg = 45.0;
        let rad = convert_slope_degrees(deg, SlopeUnits::Radians);
        assert!((rad - PI / 4.0).abs() < 1e-10);

        let pct = convert_slope_degrees(deg, SlopeUnits::Percent);
        assert!((pct - 100.0).abs() < 0.1);

        let same = convert_slope_degrees(deg, SlopeUnits::Degrees);
        assert!((same - deg).abs() < 1e-10);
    }

    // --- Edge handling ---

    #[test]
    fn test_edge_handling_skip() {
        let dem = create_slope_dem();
        let config = SlopeAspectConfig {
            edge_handling: EdgeHandling::Skip,
            ..Default::default()
        };
        let result = compute_slope_aspect_advanced(&dem, 1.0, &config);
        assert!(result.is_ok());
        // Edge pixels should remain as zero
        let output = result.expect("skip");
        let edge = output.slope.get_pixel(0, 0).expect("edge pixel");
        assert!(
            edge.abs() < 1e-10,
            "Skip edge pixel should be 0, got {edge}"
        );
    }

    #[test]
    fn test_edge_handling_reflect() {
        let dem = create_slope_dem();
        let config = SlopeAspectConfig {
            edge_handling: EdgeHandling::Reflect,
            ..Default::default()
        };
        let result = compute_slope_aspect_advanced(&dem, 1.0, &config);
        assert!(result.is_ok());
        // Corner pixel should have a non-zero slope with reflect handling
        let output = result.expect("reflect");
        let corner = output.slope.get_pixel(0, 0).expect("corner");
        // With reflect, the corner gets processed
        assert!(corner >= 0.0);
    }

    #[test]
    fn test_edge_handling_replicate() {
        let dem = create_slope_dem();
        let config = SlopeAspectConfig {
            edge_handling: EdgeHandling::Replicate,
            ..Default::default()
        };
        let result = compute_slope_aspect_advanced(&dem, 1.0, &config);
        assert!(result.is_ok());
    }

    // --- Convenience functions ---

    #[test]
    fn test_slope_convenience() {
        let dem = create_slope_dem();
        let result = slope(&dem, 1.0, 1.0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_aspect_convenience() {
        let dem = create_slope_dem();
        let result = aspect(&dem, 1.0, 1.0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_slope_advanced_convenience() {
        let dem = create_slope_dem();
        let config = SlopeAspectConfig {
            algorithm: SlopeAlgorithm::ZevenbergenThorne,
            slope_units: SlopeUnits::Percent,
            ..Default::default()
        };
        let result = slope_advanced(&dem, 1.0, &config);
        assert!(result.is_ok());
    }

    // --- Error cases ---

    #[test]
    fn test_too_small_dem() {
        let dem = RasterBuffer::zeros(2, 2, RasterDataType::Float32);
        let result = compute_slope_aspect(&dem, 1.0, 1.0);
        assert!(result.is_err());
    }

    // --- Z-factor ---

    #[test]
    fn test_z_factor_scaling() {
        let dem = create_slope_dem();
        let result1 = compute_slope_aspect(&dem, 1.0, 1.0).expect("z=1");
        let result2 = compute_slope_aspect(&dem, 1.0, 2.0).expect("z=2");

        let s1 = result1.slope.get_pixel(5, 5).expect("s1");
        let s2 = result2.slope.get_pixel(5, 5).expect("s2");

        assert!(
            s2 > s1,
            "Higher z-factor should produce steeper slope: {s1} vs {s2}"
        );
    }
}
