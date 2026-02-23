//! Terrain roughness metrics: TPI, TRI, Roughness, VRM, and Convergence Index

use crate::error::{AlgorithmError, Result};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;

#[cfg(feature = "parallel")]
use rayon::prelude::*;

use super::{RoughnessMethod, TpiNeighborhood, TriMethod, slope_aspect::get_3x3_window};

pub fn compute_tpi(
    dem: &RasterBuffer,
    neighborhood_size: usize,
    cell_size: f64,
) -> Result<RasterBuffer> {
    compute_tpi_advanced(
        dem,
        TpiNeighborhood::Rectangular(neighborhood_size),
        cell_size,
    )
}

/// Computes TPI with advanced neighborhood configuration
///
/// # Arguments
///
/// * `dem` - Digital elevation model
/// * `neighborhood` - Neighborhood configuration (rectangular or annular)
/// * `cell_size` - Size of each cell (for scaling)
///
/// # Errors
///
/// Returns an error if the parameters are invalid
pub fn compute_tpi_advanced(
    dem: &RasterBuffer,
    neighborhood: TpiNeighborhood,
    cell_size: f64,
) -> Result<RasterBuffer> {
    match neighborhood {
        TpiNeighborhood::Rectangular(size) => compute_tpi_rectangular(dem, size, cell_size),
        TpiNeighborhood::Annular {
            inner_radius,
            outer_radius,
        } => compute_tpi_annular(dem, inner_radius, outer_radius, cell_size),
    }
}

/// Rectangular TPI implementation
fn compute_tpi_rectangular(
    dem: &RasterBuffer,
    neighborhood_size: usize,
    cell_size: f64,
) -> Result<RasterBuffer> {
    if neighborhood_size % 2 == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "neighborhood_size",
            message: "Neighborhood size must be odd".to_string(),
        });
    }

    let width = dem.width();
    let height = dem.height();
    let mut tpi = RasterBuffer::zeros(width, height, RasterDataType::Float64);
    let hw = (neighborhood_size / 2) as i64;

    #[cfg(feature = "parallel")]
    {
        let results: Result<Vec<_>> = (hw as u64..(height - hw as u64))
            .into_par_iter()
            .map(|y| {
                let mut row_data = Vec::new();
                for x in hw as u64..(width - hw as u64) {
                    let center_elev = dem.get_pixel(x, y).map_err(AlgorithmError::Core)?;
                    let mut sum = 0.0;
                    let mut count = 0;

                    for dy in -hw..=hw {
                        for dx in -hw..=hw {
                            if dx == 0 && dy == 0 {
                                continue;
                            }
                            let nx = (x as i64 + dx) as u64;
                            let ny = (y as i64 + dy) as u64;
                            sum += dem.get_pixel(nx, ny).map_err(AlgorithmError::Core)?;
                            count += 1;
                        }
                    }

                    let mean_elev = if count > 0 {
                        sum / count as f64
                    } else {
                        center_elev
                    };
                    let tpi_value = (center_elev - mean_elev) / cell_size;
                    row_data.push((x, tpi_value));
                }
                Ok((y, row_data))
            })
            .collect();

        for (y, row_data) in results? {
            for (x, value) in row_data {
                tpi.set_pixel(x, y, value).map_err(AlgorithmError::Core)?;
            }
        }
    }

    #[cfg(not(feature = "parallel"))]
    {
        for y in hw as u64..(height - hw as u64) {
            for x in hw as u64..(width - hw as u64) {
                let center_elev = dem.get_pixel(x, y).map_err(AlgorithmError::Core)?;
                let mut sum = 0.0;
                let mut count = 0;

                for dy in -hw..=hw {
                    for dx in -hw..=hw {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        let nx = (x as i64 + dx) as u64;
                        let ny = (y as i64 + dy) as u64;
                        sum += dem.get_pixel(nx, ny).map_err(AlgorithmError::Core)?;
                        count += 1;
                    }
                }

                let mean_elev = if count > 0 {
                    sum / count as f64
                } else {
                    center_elev
                };
                let tpi_value = (center_elev - mean_elev) / cell_size;
                tpi.set_pixel(x, y, tpi_value)
                    .map_err(AlgorithmError::Core)?;
            }
        }
    }

    Ok(tpi)
}

/// Annular (ring) TPI implementation
///
/// Computes TPI using only cells within the annular ring defined by
/// [inner_radius, outer_radius]. This is particularly useful for
/// multi-scale landform analysis (Weiss, 2001).
fn compute_tpi_annular(
    dem: &RasterBuffer,
    inner_radius: f64,
    outer_radius: f64,
    cell_size: f64,
) -> Result<RasterBuffer> {
    if inner_radius < 0.0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "inner_radius",
            message: "Inner radius must be non-negative".to_string(),
        });
    }
    if outer_radius <= inner_radius {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "outer_radius",
            message: "Outer radius must be greater than inner radius".to_string(),
        });
    }

    let width = dem.width();
    let height = dem.height();
    let mut tpi = RasterBuffer::zeros(width, height, RasterDataType::Float64);

    let outer_r_cells = outer_radius.ceil() as i64;
    let inner_r_sq = inner_radius * inner_radius;
    let outer_r_sq = outer_radius * outer_radius;

    let margin = outer_r_cells as u64;

    #[cfg(feature = "parallel")]
    {
        let results: Result<Vec<_>> = (margin..(height - margin))
            .into_par_iter()
            .map(|y| {
                let mut row_data = Vec::new();
                for x in margin..(width - margin) {
                    let center_elev = dem.get_pixel(x, y).map_err(AlgorithmError::Core)?;
                    let mut sum = 0.0;
                    let mut count = 0;

                    for dy in -outer_r_cells..=outer_r_cells {
                        for dx in -outer_r_cells..=outer_r_cells {
                            let dist_sq = (dx * dx + dy * dy) as f64;
                            if dist_sq < inner_r_sq || dist_sq > outer_r_sq {
                                continue;
                            }
                            if dx == 0 && dy == 0 {
                                continue;
                            }
                            let nx = (x as i64 + dx) as u64;
                            let ny = (y as i64 + dy) as u64;
                            sum += dem.get_pixel(nx, ny).map_err(AlgorithmError::Core)?;
                            count += 1;
                        }
                    }

                    let mean_elev = if count > 0 {
                        sum / count as f64
                    } else {
                        center_elev
                    };
                    let tpi_value = (center_elev - mean_elev) / cell_size;
                    row_data.push((x, tpi_value));
                }
                Ok((y, row_data))
            })
            .collect();

        for (y, row_data) in results? {
            for (x, value) in row_data {
                tpi.set_pixel(x, y, value).map_err(AlgorithmError::Core)?;
            }
        }
    }

    #[cfg(not(feature = "parallel"))]
    {
        for y in margin..(height - margin) {
            for x in margin..(width - margin) {
                let center_elev = dem.get_pixel(x, y).map_err(AlgorithmError::Core)?;
                let mut sum = 0.0;
                let mut count = 0;

                for dy in -outer_r_cells..=outer_r_cells {
                    for dx in -outer_r_cells..=outer_r_cells {
                        let dist_sq = (dx * dx + dy * dy) as f64;
                        if dist_sq < inner_r_sq || dist_sq > outer_r_sq {
                            continue;
                        }
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        let nx = (x as i64 + dx) as u64;
                        let ny = (y as i64 + dy) as u64;
                        sum += dem.get_pixel(nx, ny).map_err(AlgorithmError::Core)?;
                        count += 1;
                    }
                }

                let mean_elev = if count > 0 {
                    sum / count as f64
                } else {
                    center_elev
                };
                let tpi_value = (center_elev - mean_elev) / cell_size;
                tpi.set_pixel(x, y, tpi_value)
                    .map_err(AlgorithmError::Core)?;
            }
        }
    }

    Ok(tpi)
}

// ===========================================================================
// TRI
// ===========================================================================

/// Computes Terrain Ruggedness Index (TRI) using the Riley et al. (1999) method
///
/// TRI = sqrt(sum((center - neighbor)^2)) for all 8 neighbors
///
/// # Arguments
///
/// * `dem` - Digital elevation model
/// * `cell_size` - Size of each cell (for scaling)
///
/// # Errors
///
/// Returns an error if the operation fails
pub fn compute_tri(dem: &RasterBuffer, cell_size: f64) -> Result<RasterBuffer> {
    compute_tri_advanced(dem, cell_size, TriMethod::Riley)
}

/// Computes TRI with selectable method
///
/// # Methods
///
/// - **Riley**: sqrt(sum((center - neighbor)^2)) -- original Riley et al. (1999)
/// - **MeanAbsoluteDifference**: mean(|center - neighbor|) -- simple average
/// - **RootMeanSquare**: sqrt(mean((center - neighbor)^2)) -- Wilson et al. (2007)
///
/// # Arguments
///
/// * `dem` - Digital elevation model
/// * `cell_size` - Size of each cell
/// * `method` - TRI method variant
///
/// # Errors
///
/// Returns an error if the operation fails
pub fn compute_tri_advanced(
    dem: &RasterBuffer,
    cell_size: f64,
    method: TriMethod,
) -> Result<RasterBuffer> {
    let width = dem.width();
    let height = dem.height();
    let mut tri = RasterBuffer::zeros(width, height, RasterDataType::Float64);

    #[cfg(feature = "parallel")]
    {
        let results: Result<Vec<_>> = (1..(height - 1))
            .into_par_iter()
            .map(|y| {
                let mut row_data = Vec::new();
                for x in 1..(width - 1) {
                    let value = compute_local_tri(dem, x, y, cell_size, method)?;
                    row_data.push((x, value));
                }
                Ok((y, row_data))
            })
            .collect();

        for (y, row_data) in results? {
            for (x, value) in row_data {
                tri.set_pixel(x, y, value).map_err(AlgorithmError::Core)?;
            }
        }
    }

    #[cfg(not(feature = "parallel"))]
    {
        for y in 1..(height - 1) {
            for x in 1..(width - 1) {
                let value = compute_local_tri(dem, x, y, cell_size, method)?;
                tri.set_pixel(x, y, value).map_err(AlgorithmError::Core)?;
            }
        }
    }

    Ok(tri)
}

/// Local TRI computation for a single pixel
fn compute_local_tri(
    dem: &RasterBuffer,
    x: u64,
    y: u64,
    cell_size: f64,
    method: TriMethod,
) -> Result<f64> {
    let center = dem.get_pixel(x, y).map_err(AlgorithmError::Core)?;
    let mut sum_sq_diff = 0.0;
    let mut sum_abs_diff = 0.0;
    let mut count = 0u32;

    for dy in -1..=1i64 {
        for dx in -1..=1i64 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let nx = (x as i64 + dx) as u64;
            let ny = (y as i64 + dy) as u64;
            let neighbor = dem.get_pixel(nx, ny).map_err(AlgorithmError::Core)?;
            let diff = center - neighbor;
            sum_sq_diff += diff * diff;
            sum_abs_diff += diff.abs();
            count += 1;
        }
    }

    let tri_value = match method {
        TriMethod::Riley => sum_sq_diff.sqrt() / cell_size,
        TriMethod::MeanAbsoluteDifference => {
            if count > 0 {
                sum_abs_diff / (count as f64 * cell_size)
            } else {
                0.0
            }
        }
        TriMethod::RootMeanSquare => {
            if count > 0 {
                (sum_sq_diff / count as f64).sqrt() / cell_size
            } else {
                0.0
            }
        }
    };

    Ok(tri_value)
}

// ===========================================================================
// Roughness
// ===========================================================================

/// Computes surface roughness as standard deviation of elevations
///
/// # Arguments
///
/// * `dem` - Digital elevation model
/// * `neighborhood_size` - Size of neighborhood (must be odd)
///
/// # Errors
///
/// Returns an error if the neighborhood size is even
pub fn compute_roughness(dem: &RasterBuffer, neighborhood_size: usize) -> Result<RasterBuffer> {
    compute_roughness_advanced(dem, neighborhood_size, RoughnessMethod::StandardDeviation)
}

/// Computes surface roughness with selectable method
///
/// # Methods
///
/// - **StandardDeviation**: Standard deviation of elevations in neighborhood
/// - **Range**: Difference between max and min elevation in neighborhood
/// - **CoefficientOfVariation**: stddev / mean (dimensionless)
///
/// # Arguments
///
/// * `dem` - Digital elevation model
/// * `neighborhood_size` - Size of neighborhood (must be odd)
/// * `method` - Roughness method variant
///
/// # Errors
///
/// Returns an error if the neighborhood size is even
pub fn compute_roughness_advanced(
    dem: &RasterBuffer,
    neighborhood_size: usize,
    method: RoughnessMethod,
) -> Result<RasterBuffer> {
    if neighborhood_size % 2 == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "neighborhood_size",
            message: "Neighborhood size must be odd".to_string(),
        });
    }

    let width = dem.width();
    let height = dem.height();
    let mut roughness = RasterBuffer::zeros(width, height, RasterDataType::Float64);
    let hw = (neighborhood_size / 2) as i64;

    #[cfg(feature = "parallel")]
    {
        let results: Result<Vec<_>> = (hw as u64..(height - hw as u64))
            .into_par_iter()
            .map(|y| {
                let mut row_data = Vec::new();
                for x in hw as u64..(width - hw as u64) {
                    let value = compute_local_roughness(dem, x, y, hw, method)?;
                    row_data.push((x, value));
                }
                Ok((y, row_data))
            })
            .collect();

        for (y, row_data) in results? {
            for (x, value) in row_data {
                roughness
                    .set_pixel(x, y, value)
                    .map_err(AlgorithmError::Core)?;
            }
        }
    }

    #[cfg(not(feature = "parallel"))]
    {
        for y in hw as u64..(height - hw as u64) {
            for x in hw as u64..(width - hw as u64) {
                let value = compute_local_roughness(dem, x, y, hw, method)?;
                roughness
                    .set_pixel(x, y, value)
                    .map_err(AlgorithmError::Core)?;
            }
        }
    }

    Ok(roughness)
}

/// Local roughness computation for a single pixel
fn compute_local_roughness(
    dem: &RasterBuffer,
    x: u64,
    y: u64,
    hw: i64,
    method: RoughnessMethod,
) -> Result<f64> {
    let mut elevations = Vec::new();

    for dy in -hw..=hw {
        for dx in -hw..=hw {
            let nx = (x as i64 + dx) as u64;
            let ny = (y as i64 + dy) as u64;
            elevations.push(dem.get_pixel(nx, ny).map_err(AlgorithmError::Core)?);
        }
    }

    if elevations.is_empty() {
        return Ok(0.0);
    }

    match method {
        RoughnessMethod::StandardDeviation => {
            let mean = elevations.iter().sum::<f64>() / elevations.len() as f64;
            let variance = elevations.iter().map(|&e| (e - mean).powi(2)).sum::<f64>()
                / elevations.len() as f64;
            Ok(variance.sqrt())
        }
        RoughnessMethod::Range => {
            let min_val = elevations.iter().copied().fold(f64::INFINITY, f64::min);
            let max_val = elevations.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            Ok(max_val - min_val)
        }
        RoughnessMethod::CoefficientOfVariation => {
            let mean = elevations.iter().sum::<f64>() / elevations.len() as f64;
            if mean.abs() < 1e-15 {
                return Ok(0.0);
            }
            let variance = elevations.iter().map(|&e| (e - mean).powi(2)).sum::<f64>()
                / elevations.len() as f64;
            Ok(variance.sqrt() / mean.abs())
        }
    }
}

// ===========================================================================
// Curvature
// ===========================================================================

/// Computes surface curvature
///
/// Curvature is computed using a 3x3 window following Evans (1972) and
/// Zevenbergen & Thorne (1987) formulations:
///
/// - Profile: curvature in the direction of maximum slope
/// - Planform: curvature perpendicular to the direction of maximum slope
/// - Total: Laplacian (d2z/dx2 + d2z/dy2)
/// - Mean: average of principal curvatures
/// - Gaussian: product of principal curvatures
/// - Tangential: curvature of a normal section tangent to a contour line
///
/// # Arguments
///
/// * `dem` - Digital elevation model
/// * `cell_size` - Size of each cell
/// * `curvature_type` - Type of curvature to compute
///
/// # Errors
///
/// Returns an error if the operation fails
pub fn compute_convergence_index(dem: &RasterBuffer, cell_size: f64) -> Result<RasterBuffer> {
    let width = dem.width();
    let height = dem.height();
    let mut convergence = RasterBuffer::zeros(width, height, RasterDataType::Float64);

    for y in 1..(height - 1) {
        for x in 1..(width - 1) {
            let value = compute_local_convergence(dem, x, y, cell_size)?;
            convergence
                .set_pixel(x, y, value)
                .map_err(AlgorithmError::Core)?;
        }
    }

    Ok(convergence)
}

/// Local convergence index computation
fn compute_local_convergence(dem: &RasterBuffer, cx: u64, cy: u64, cell_size: f64) -> Result<f64> {
    use core::f64::consts::PI;

    // Direction angles from center to each of the 8 neighbors (in radians)
    // N=0, NE=pi/4, E=pi/2, SE=3pi/4, S=pi, SW=5pi/4, W=3pi/2, NW=7pi/4
    let direction_angles: [f64; 8] = [
        0.0,
        PI / 4.0,
        PI / 2.0,
        3.0 * PI / 4.0,
        PI,
        5.0 * PI / 4.0,
        3.0 * PI / 2.0,
        7.0 * PI / 4.0,
    ];

    // Offsets for 8 neighbors: N, NE, E, SE, S, SW, W, NW
    let offsets: [(i64, i64); 8] = [
        (0, -1),
        (1, -1),
        (1, 0),
        (1, 1),
        (0, 1),
        (-1, 1),
        (-1, 0),
        (-1, -1),
    ];

    let mut sum_deviation = 0.0;
    let mut count = 0;

    for (i, &(dx, dy)) in offsets.iter().enumerate() {
        let nx = (cx as i64 + dx) as u64;
        let ny = (cy as i64 + dy) as u64;

        // Compute aspect at neighbor cell (need 3x3 window around it)
        if nx == 0 || nx >= dem.width() - 1 || ny == 0 || ny >= dem.height() - 1 {
            continue;
        }

        let z = get_3x3_window(dem, nx, ny)?;
        let dz_dx = ((z[0][2] + 2.0 * z[1][2] + z[2][2]) - (z[0][0] + 2.0 * z[1][0] + z[2][0]))
            / (8.0 * cell_size);
        let dz_dy = ((z[2][0] + 2.0 * z[2][1] + z[2][2]) - (z[0][0] + 2.0 * z[0][1] + z[0][2]))
            / (8.0 * cell_size);

        let neighbor_aspect = dz_dy.atan2(-dz_dx);

        // Expected direction: from neighbor toward center (opposite of direction from center)
        let expected_dir = direction_angles[i] + PI;

        // Angular deviation between neighbor aspect and expected direction
        let mut deviation = (neighbor_aspect - expected_dir).abs();
        if deviation > PI {
            deviation = 2.0 * PI - deviation;
        }

        // Convert to percentage: 0 = converging, 180 = diverging
        sum_deviation += deviation;
        count += 1;
    }

    if count == 0 {
        return Ok(0.0);
    }

    // Scale to [-100, 100]: -100 = convergence, +100 = divergence
    let mean_deviation_degrees = sum_deviation * 180.0 / (PI * count as f64);
    let convergence_value = (mean_deviation_degrees - 90.0) * 100.0 / 90.0;

    Ok(convergence_value)
}

// ===========================================================================
// VRM
// ===========================================================================

/// Computes Vector Ruggedness Measure (VRM)
///
/// VRM quantifies terrain ruggedness by calculating the dispersion of
/// normal vectors to the surface within a neighborhood.
/// Based on Sappington et al. (2007).
///
/// VRM = 1 - |resultant| / n
///
/// Values range from 0 (perfectly flat) to ~1 (extremely rugged).
///
/// # Arguments
///
/// * `dem` - Digital elevation model
/// * `neighborhood_size` - Size of neighborhood (must be odd)
/// * `cell_size` - Size of each cell
///
/// # Errors
///
/// Returns an error if the neighborhood size is even
pub fn compute_vrm(
    dem: &RasterBuffer,
    neighborhood_size: usize,
    cell_size: f64,
) -> Result<RasterBuffer> {
    if neighborhood_size % 2 == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "neighborhood_size",
            message: "Neighborhood size must be odd".to_string(),
        });
    }

    let width = dem.width();
    let height = dem.height();
    let mut vrm = RasterBuffer::zeros(width, height, RasterDataType::Float64);
    let hw = (neighborhood_size / 2) as i64;

    #[cfg(feature = "parallel")]
    {
        let results: Result<Vec<_>> = (hw as u64 + 1..(height - hw as u64 - 1))
            .into_par_iter()
            .map(|y| {
                let mut row_data = Vec::new();
                for x in (hw as u64 + 1)..(width - hw as u64 - 1) {
                    let value = compute_local_vrm(dem, x, y, hw, cell_size)?;
                    row_data.push((x, value));
                }
                Ok((y, row_data))
            })
            .collect();

        for (y, row_data) in results? {
            for (x, value) in row_data {
                vrm.set_pixel(x, y, value).map_err(AlgorithmError::Core)?;
            }
        }
    }

    #[cfg(not(feature = "parallel"))]
    {
        for y in (hw as u64 + 1)..(height - hw as u64 - 1) {
            for x in (hw as u64 + 1)..(width - hw as u64 - 1) {
                let value = compute_local_vrm(dem, x, y, hw, cell_size)?;
                vrm.set_pixel(x, y, value).map_err(AlgorithmError::Core)?;
            }
        }
    }

    Ok(vrm)
}

/// Computes VRM for a single pixel
fn compute_local_vrm(dem: &RasterBuffer, cx: u64, cy: u64, hw: i64, cell_size: f64) -> Result<f64> {
    let mut sum_x = 0.0_f64;
    let mut sum_y = 0.0_f64;
    let mut sum_z = 0.0_f64;
    let mut n = 0u32;

    // Compute normal vectors for each cell in neighborhood
    for dy in -hw..hw {
        for dx in -hw..hw {
            let x = (cx as i64 + dx) as u64;
            let y = (cy as i64 + dy) as u64;

            // Compute normal vector using cross product of gradients
            let z_center = dem.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            let z_east = dem.get_pixel(x + 1, y).map_err(AlgorithmError::Core)?;
            let z_north = dem
                .get_pixel(x, y.wrapping_sub(1))
                .map_err(AlgorithmError::Core)?;

            let dz_dx = (z_east - z_center) / cell_size;
            let dz_dy = (z_north - z_center) / cell_size;

            // Normal vector: (-dz/dx, -dz/dy, 1)
            let nx = -dz_dx;
            let ny = -dz_dy;
            let nz = 1.0;

            // Normalize
            let mag = (nx * nx + ny * ny + nz * nz).sqrt();
            if mag > 1e-15 {
                sum_x += nx / mag;
                sum_y += ny / mag;
                sum_z += nz / mag;
                n += 1;
            }
        }
    }

    if n == 0 {
        return Ok(0.0);
    }

    let resultant_mag = (sum_x * sum_x + sum_y * sum_y + sum_z * sum_z).sqrt();
    let vrm_value = 1.0 - (resultant_mag / n as f64);

    Ok(vrm_value)
}

// ===========================================================================
// Terrain classification
// ===========================================================================

// Note: Landform classification using TPI is implemented in the landform.rs module.
// See `classify_landforms()` for single-scale classification and
// `classify_landforms_multiscale()` for multi-scale classification.
// These implementations follow Weiss (2001) classification rules:
// - Valley: TPI < -1 * std_dev
// - Lower Slope: TPI in [-1, -0.5] * std_dev
// - Flat: TPI in [-0.5, 0.5] * std_dev AND slope < slope_threshold
// - Middle Slope: TPI in [-0.5, 0.5] * std_dev AND slope >= slope_threshold
// - Upper Slope: TPI in [0.5, 1.0] * std_dev
// - Ridge: TPI > 1 * std_dev
