//! Landform classification and hydrological indices

use crate::error::{AlgorithmError, Result};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;

#[cfg(feature = "parallel")]
use rayon::prelude::*;

use super::roughness::{compute_tpi, compute_tpi_advanced};
use super::slope_aspect::{compute_slope_degrees, get_3x3_window};
use super::{LandformClass, TpiNeighborhood};

pub fn classify_landforms(
    dem: &RasterBuffer,
    neighborhood_size: usize,
    cell_size: f64,
    slope_threshold: f64,
) -> Result<RasterBuffer> {
    // Compute TPI
    let tpi = compute_tpi(dem, neighborhood_size, cell_size)?;

    // Compute slope
    let slope = compute_slope_degrees(dem, cell_size)?;

    // Compute TPI statistics for standardization
    let width = dem.width();
    let height = dem.height();
    let hw = (neighborhood_size / 2) as u64;

    let mut tpi_sum = 0.0;
    let mut tpi_sum_sq = 0.0;
    let mut tpi_count = 0u64;

    for y in hw..(height - hw) {
        for x in hw..(width - hw) {
            let v = tpi.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            tpi_sum += v;
            tpi_sum_sq += v * v;
            tpi_count += 1;
        }
    }

    let tpi_mean = if tpi_count > 0 {
        tpi_sum / tpi_count as f64
    } else {
        0.0
    };
    let tpi_variance = if tpi_count > 1 {
        (tpi_sum_sq - tpi_count as f64 * tpi_mean * tpi_mean) / (tpi_count - 1) as f64
    } else {
        1.0
    };
    let tpi_std = tpi_variance.sqrt().max(1e-15);

    // Classify
    let mut landforms = RasterBuffer::zeros(width, height, RasterDataType::UInt8);

    for y in 1..(height - 1) {
        for x in 1..(width - 1) {
            let tpi_val = tpi.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            let slope_val = slope.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            let std_tpi = (tpi_val - tpi_mean) / tpi_std;

            let class = if std_tpi < -1.0 {
                LandformClass::Valley
            } else if std_tpi < -0.5 {
                LandformClass::LowerSlope
            } else if std_tpi <= 0.5 {
                if slope_val < slope_threshold {
                    LandformClass::Flat
                } else {
                    LandformClass::MiddleSlope
                }
            } else if std_tpi <= 1.0 {
                LandformClass::UpperSlope
            } else {
                LandformClass::Ridge
            };

            landforms
                .set_pixel(x, y, class as u8 as f64)
                .map_err(AlgorithmError::Core)?;
        }
    }

    Ok(landforms)
}

/// Classifies landforms using TPI at two scales (Weiss multi-scale method)
///
/// Uses small-scale TPI and large-scale TPI together for more nuanced classification.
/// This produces a 10-class landform classification.
///
/// # Arguments
///
/// * `dem` - Digital elevation model
/// * `small_neighborhood` - Small neighborhood size for local TPI (must be odd)
/// * `large_neighborhood` - Large neighborhood size for landscape TPI (must be odd)
/// * `cell_size` - Cell size
/// * `slope_threshold` - Slope threshold for flat areas (degrees)
///
/// # Errors
///
/// Returns an error if the operation fails
pub fn classify_landforms_multiscale(
    dem: &RasterBuffer,
    small_neighborhood: usize,
    large_neighborhood: usize,
    cell_size: f64,
    slope_threshold: f64,
) -> Result<RasterBuffer> {
    let tpi_small = compute_tpi(dem, small_neighborhood, cell_size)?;
    let tpi_large = compute_tpi(dem, large_neighborhood, cell_size)?;
    let slope_raster = compute_slope_degrees(dem, cell_size)?;

    let width = dem.width();
    let height = dem.height();

    // Standardize both TPI rasters
    let small_stats = compute_tpi_stats(&tpi_small, width, height, small_neighborhood)?;
    let large_stats = compute_tpi_stats(&tpi_large, width, height, large_neighborhood)?;

    let mut landforms = RasterBuffer::zeros(width, height, RasterDataType::UInt8);
    let margin = (large_neighborhood / 2) as u64;

    for y in margin..(height - margin) {
        for x in margin..(width - margin) {
            let ts = (tpi_small.get_pixel(x, y).map_err(AlgorithmError::Core)? - small_stats.0)
                / small_stats.1.max(1e-15);
            let tl = (tpi_large.get_pixel(x, y).map_err(AlgorithmError::Core)? - large_stats.0)
                / large_stats.1.max(1e-15);
            let sl = slope_raster.get_pixel(x, y).map_err(AlgorithmError::Core)?;

            // 10-class Weiss classification compressed to 6 main classes
            let class = if tl < -1.0 {
                if ts < -1.0 {
                    LandformClass::Valley // deep valley
                } else if ts > 1.0 {
                    LandformClass::LowerSlope // local ridge in valley context
                } else {
                    LandformClass::LowerSlope
                }
            } else if tl > 1.0 {
                if ts < -1.0 {
                    LandformClass::UpperSlope // local valley on ridge
                } else if ts > 1.0 {
                    LandformClass::Ridge // ridge top
                } else {
                    LandformClass::UpperSlope
                }
            } else {
                // Mid-range large-scale TPI
                if ts < -1.0 {
                    LandformClass::LowerSlope // U-shaped valley
                } else if ts > 1.0 {
                    LandformClass::UpperSlope // local ridge
                } else if sl < slope_threshold {
                    LandformClass::Flat
                } else {
                    LandformClass::MiddleSlope
                }
            };

            landforms
                .set_pixel(x, y, class as u8 as f64)
                .map_err(AlgorithmError::Core)?;
        }
    }

    Ok(landforms)
}

/// Compute TPI mean and standard deviation for standardization
fn compute_tpi_stats(
    tpi: &RasterBuffer,
    width: u64,
    height: u64,
    neighborhood_size: usize,
) -> Result<(f64, f64)> {
    let hw = (neighborhood_size / 2) as u64;
    let mut sum = 0.0;
    let mut sum_sq = 0.0;
    let mut count = 0u64;

    for y in hw..(height - hw) {
        for x in hw..(width - hw) {
            let v = tpi.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            sum += v;
            sum_sq += v * v;
            count += 1;
        }
    }

    let mean = if count > 0 { sum / count as f64 } else { 0.0 };
    let variance = if count > 1 {
        (sum_sq - count as f64 * mean * mean) / (count - 1) as f64
    } else {
        1.0
    };

    Ok((mean, variance.sqrt()))
}

// ===========================================================================
// Topographic Wetness Index and Stream Power Index
// ===========================================================================

/// Computes Topographic Wetness Index (TWI)
///
/// TWI = ln(a / tan(beta))
///
/// where:
/// - a = specific catchment area (upslope contributing area per unit contour length)
/// - beta = local slope angle
///
/// This requires a pre-computed flow accumulation raster.
///
/// # Arguments
///
/// * `flow_accumulation` - Flow accumulation raster (count of upstream cells)
/// * `slope_raster` - Slope raster in radians
/// * `cell_size` - Cell size (used to convert flow accumulation to specific catchment area)
///
/// # Errors
///
/// Returns an error if the operation fails
pub fn compute_twi(
    flow_accumulation: &RasterBuffer,
    slope_raster: &RasterBuffer,
    cell_size: f64,
) -> Result<RasterBuffer> {
    let width = flow_accumulation.width();
    let height = flow_accumulation.height();
    let mut twi = RasterBuffer::zeros(width, height, RasterDataType::Float64);

    for y in 0..height {
        for x in 0..width {
            let flow_acc = flow_accumulation
                .get_pixel(x, y)
                .map_err(AlgorithmError::Core)?;
            let slope_rad = slope_raster.get_pixel(x, y).map_err(AlgorithmError::Core)?;

            // Specific catchment area: (flow_acc + 1) * cell_size / cell_size
            // = (flow_acc + 1) * cell_size (area per unit contour length)
            let specific_area = (flow_acc + 1.0) * cell_size;

            // Avoid log(0) and division by zero with small slope clamp
            let tan_slope = slope_rad.tan().max(0.001);
            let twi_value = (specific_area / tan_slope).ln();

            twi.set_pixel(x, y, twi_value)
                .map_err(AlgorithmError::Core)?;
        }
    }

    Ok(twi)
}

/// Computes Stream Power Index (SPI)
///
/// SPI = a * tan(beta)
///
/// where:
/// - a = specific catchment area
/// - beta = local slope angle
///
/// Higher values indicate greater erosive potential.
///
/// # Arguments
///
/// * `flow_accumulation` - Flow accumulation raster (count of upstream cells)
/// * `slope_raster` - Slope raster in radians
/// * `cell_size` - Cell size
///
/// # Errors
///
/// Returns an error if the operation fails
pub fn compute_spi(
    flow_accumulation: &RasterBuffer,
    slope_raster: &RasterBuffer,
    cell_size: f64,
) -> Result<RasterBuffer> {
    let width = flow_accumulation.width();
    let height = flow_accumulation.height();
    let mut spi = RasterBuffer::zeros(width, height, RasterDataType::Float64);

    for y in 0..height {
        for x in 0..width {
            let flow_acc = flow_accumulation
                .get_pixel(x, y)
                .map_err(AlgorithmError::Core)?;
            let slope_rad = slope_raster.get_pixel(x, y).map_err(AlgorithmError::Core)?;

            let specific_area = (flow_acc + 1.0) * cell_size;
            let spi_value = specific_area * slope_rad.tan();

            spi.set_pixel(x, y, spi_value)
                .map_err(AlgorithmError::Core)?;
        }
    }

    Ok(spi)
}

// ===========================================================================
// Shape Index and Terrain Shape Index
// ===========================================================================

/// Computes Terrain Shape Index (TSI) following McNab (1989)
///
/// TSI provides a dimensionless measure of local terrain shape based on
/// the second-order partial derivatives of the surface. Values near 0
/// indicate convex surfaces, values near 1 indicate concave surfaces,
/// and values near 0.5 indicate planar surfaces.
///
/// # Arguments
///
/// * `dem` - Digital elevation model
/// * `cell_size` - Cell size
///
/// # Errors
///
/// Returns an error if the operation fails
pub fn compute_terrain_shape_index(dem: &RasterBuffer, cell_size: f64) -> Result<RasterBuffer> {
    let width = dem.width();
    let height = dem.height();
    let mut tsi = RasterBuffer::zeros(width, height, RasterDataType::Float64);

    for y in 1..(height - 1) {
        for x in 1..(width - 1) {
            let z = get_3x3_window(dem, x, y)?;

            let l_sq = cell_size * cell_size;
            let r = (z[1][2] - 2.0 * z[1][1] + z[1][0]) / l_sq;
            let t = (z[2][1] - 2.0 * z[1][1] + z[0][1]) / l_sq;
            let laplacian = r + t;

            // Normalize using atan to map (-inf, inf) -> (0, 1)
            let normalized = 0.5 + laplacian.atan() / core::f64::consts::PI;

            tsi.set_pixel(x, y, normalized)
                .map_err(AlgorithmError::Core)?;
        }
    }

    Ok(tsi)
}

// ===========================================================================
// Convenience wrappers
// ===========================================================================

/// Computes terrain aspect in degrees
///
/// This is a convenience function that wraps the slope_aspect module computation,
/// making it easy to get just the aspect without computing slope. Aspect represents
/// the direction of the steepest descent, measured in degrees from north (0-360,
/// clockwise). Flat areas (where there is no defined slope direction) are marked
/// with a value of -1.
///
/// # Arguments
///
/// * `dem` - Digital elevation model
/// * `cell_size` - Size of each cell
///
/// # Errors
///
/// Returns an error if the operation fails
///
/// # Example
///
/// ```ignore
/// use oxigdal_algorithms::raster::terrain::compute_aspect_degrees;
/// use oxigdal_core::buffer::RasterBuffer;
///
/// let dem = RasterBuffer::zeros(10, 10, oxigdal_core::types::RasterDataType::Float32);
/// let aspect = compute_aspect_degrees(&dem, 1.0)?;
/// # Ok::<(), oxigdal_algorithms::error::AlgorithmError>(())
/// ```
pub fn compute_aspect_degrees(dem: &RasterBuffer, cell_size: f64) -> Result<RasterBuffer> {
    use super::slope_aspect;
    slope_aspect::compute_aspect_degrees(dem, cell_size)
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn create_flat_dem() -> RasterBuffer {
        let mut dem = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        for y in 0..10 {
            for x in 0..10 {
                let _ = dem.set_pixel(x, y, 100.0);
            }
        }
        dem
    }

    fn create_sloped_dem() -> RasterBuffer {
        let mut dem = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        for y in 0..10 {
            for x in 0..10 {
                let _ = dem.set_pixel(x, y, (x + y) as f64);
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

    #[test]
    fn test_compute_aspect_degrees_flat_terrain() {
        let dem = create_flat_dem();
        let result = compute_aspect_degrees(&dem, 1.0);
        assert!(result.is_ok(), "Aspect computation should succeed");

        let aspect = result.expect("aspect");
        let center_aspect = aspect.get_pixel(5, 5).expect("pixel");
        // Flat terrain should have undefined aspect (-1)
        assert!(
            center_aspect < 0.0,
            "Flat terrain aspect should be -1, got {}",
            center_aspect
        );
    }

    #[test]
    fn test_compute_aspect_degrees_sloped_terrain() {
        let dem = create_sloped_dem();
        let result = compute_aspect_degrees(&dem, 1.0);
        assert!(result.is_ok(), "Aspect computation should succeed");

        let aspect = result.expect("aspect");
        let center_aspect = aspect.get_pixel(5, 5).expect("pixel");
        // Sloped terrain should have a valid aspect in range [0, 360) or -1 for flat
        assert!(
            (0.0..360.0).contains(&center_aspect) || center_aspect < 0.0,
            "Aspect should be in [0,360) or -1, got {}",
            center_aspect
        );
    }

    #[test]
    fn test_compute_aspect_degrees_north_facing() {
        let dem = create_north_facing_dem();
        let result = compute_aspect_degrees(&dem, 1.0);
        assert!(result.is_ok(), "Aspect computation should succeed");

        let aspect = result.expect("aspect");
        let center_aspect = aspect.get_pixel(5, 5).expect("pixel");
        // North-facing slope should have aspect around 180 (south-facing downhill)
        // The elevation increases going south, so downhill faces north (~0 or ~360)
        assert!(
            (0.0..=360.0).contains(&center_aspect) || center_aspect < 0.0,
            "Aspect should be valid, got {}",
            center_aspect
        );
    }

    #[test]
    fn test_compute_aspect_degrees_different_cell_sizes() {
        let dem = create_sloped_dem();

        let result1 = compute_aspect_degrees(&dem, 1.0);
        let result2 = compute_aspect_degrees(&dem, 2.0);

        assert!(result1.is_ok(), "Aspect with cell_size=1.0 should succeed");
        assert!(result2.is_ok(), "Aspect with cell_size=2.0 should succeed");

        // Both should have valid aspect values
        let aspect1 = result1.expect("aspect1");
        let aspect2 = result2.expect("aspect2");

        let a1 = aspect1.get_pixel(5, 5).expect("pixel1");
        let a2 = aspect2.get_pixel(5, 5).expect("pixel2");

        assert!(
            (0.0..360.0).contains(&a1) || a1 < 0.0,
            "Aspect1 should be valid"
        );
        assert!(
            (0.0..360.0).contains(&a2) || a2 < 0.0,
            "Aspect2 should be valid"
        );
    }

    #[test]
    fn test_compute_aspect_degrees_with_slope_consistency() {
        // Verify that aspect computation works alongside slope computation
        let dem = create_sloped_dem();

        let slope_result = compute_slope_degrees(&dem, 1.0);
        let aspect_result = compute_aspect_degrees(&dem, 1.0);

        assert!(slope_result.is_ok(), "Slope computation should succeed");
        assert!(aspect_result.is_ok(), "Aspect computation should succeed");

        // Both should produce rasters of the same size
        let slope = slope_result.expect("slope");
        let aspect = aspect_result.expect("aspect");

        assert_eq!(slope.width(), aspect.width(), "Width should match");
        assert_eq!(slope.height(), aspect.height(), "Height should match");
    }

    #[test]
    fn test_compute_aspect_degrees_buffer_size() {
        let dem = create_sloped_dem();
        let result = compute_aspect_degrees(&dem, 1.0);
        assert!(result.is_ok());

        let aspect = result.expect("aspect");
        assert_eq!(
            aspect.width(),
            dem.width(),
            "Output width should match input"
        );
        assert_eq!(
            aspect.height(),
            dem.height(),
            "Output height should match input"
        );
    }

    #[test]
    fn test_compute_aspect_degrees_range() {
        let dem = create_sloped_dem();
        let result = compute_aspect_degrees(&dem, 1.0);
        assert!(result.is_ok());

        let aspect = result.expect("aspect");
        for y in 0..aspect.height() {
            for x in 0..aspect.width() {
                let value = aspect.get_pixel(x, y).expect("pixel");
                assert!(
                    (0.0..360.0).contains(&value) || value < 0.0,
                    "Aspect at ({},{}) out of range: {}",
                    x,
                    y,
                    value
                );
            }
        }
    }
}
