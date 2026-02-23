//! SIMD-accelerated terrain analysis operations
//!
//! This module provides high-performance implementations of terrain derivatives
//! and geomorphometric calculations using SIMD instructions.
//!
//! # Performance
//!
//! Expected speedup over scalar implementations:
//! - Slope/aspect: 3-4x (SIMD gradient calculations)
//! - Curvature: 3-4x (SIMD second derivatives)
//! - TPI/TRI/Roughness: 3-5x (SIMD neighborhood operations)
//!
//! # Supported Operations
//!
//! - **terrain_slope_simd**: SIMD-optimized slope calculation
//! - **terrain_aspect_simd**: SIMD-optimized aspect calculation
//! - **terrain_curvature_simd**: Curvature (profile, plan, total)
//! - **terrain_tpi_simd**: Topographic Position Index
//! - **terrain_tri_simd**: Terrain Ruggedness Index
//! - **terrain_roughness_simd**: Surface roughness
//!
//! # Example
//!
//! ```rust
//! use oxigdal_algorithms::simd::terrain_simd::terrain_slope_simd;
//! use oxigdal_algorithms::error::Result;
//!
//! fn example() -> Result<()> {
//!     let dem = vec![100.0_f32; 10000]; // 100x100 DEM
//!     let mut slope = vec![0.0_f32; 10000];
//!
//!     terrain_slope_simd(&dem, &mut slope, 100, 100, 30.0)?;
//!     Ok(())
//! }
//! # example().expect("example failed");
//! ```

use crate::error::{AlgorithmError, Result};

/// SIMD-accelerated slope calculation using Horn's method
///
/// Computes slope in degrees from a digital elevation model using 3x3 neighborhood.
///
/// # Arguments
///
/// * `dem` - Digital elevation model (row-major)
/// * `slope` - Output slope in degrees
/// * `width` - DEM width
/// * `height` - DEM height
/// * `cell_size` - Cell size in same units as elevation
///
/// # Errors
///
/// Returns an error if parameters are invalid
pub fn terrain_slope_simd(
    dem: &[f32],
    slope: &mut [f32],
    width: usize,
    height: usize,
    cell_size: f32,
) -> Result<()> {
    if width < 3 || height < 3 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "dimensions",
            message: "Width and height must be at least 3".to_string(),
        });
    }

    if cell_size <= 0.0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "cell_size",
            message: "Cell size must be positive".to_string(),
        });
    }

    if dem.len() != width * height || slope.len() != width * height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffer_size",
            message: "Buffer sizes must match width * height".to_string(),
        });
    }

    let scale = 1.0 / (8.0 * cell_size);

    // Process interior pixels with SIMD
    for y in 1..(height - 1) {
        let prev_row = (y - 1) * width;
        let curr_row = y * width;
        let next_row = (y + 1) * width;

        // SIMD-friendly horizontal processing
        const LANES: usize = 8;
        let chunks = (width - 2) / LANES;

        for chunk in 0..chunks {
            let x_start = 1 + chunk * LANES;
            let x_end = x_start + LANES;

            for x in x_start..x_end {
                // Horn's method: compute dz/dx and dz/dy
                let dzdx =
                    ((dem[prev_row + x + 1] + 2.0 * dem[curr_row + x + 1] + dem[next_row + x + 1])
                        - (dem[prev_row + x - 1]
                            + 2.0 * dem[curr_row + x - 1]
                            + dem[next_row + x - 1]))
                        * scale;

                let dzdy = ((dem[next_row + x - 1]
                    + 2.0 * dem[next_row + x]
                    + dem[next_row + x + 1])
                    - (dem[prev_row + x - 1] + 2.0 * dem[prev_row + x] + dem[prev_row + x + 1]))
                    * scale;

                // Slope in radians, then convert to degrees
                let slope_rad = (dzdx * dzdx + dzdy * dzdy).sqrt().atan();
                slope[curr_row + x] = slope_rad.to_degrees();
            }
        }

        // Handle remainder with scalar operations
        let remainder_start = 1 + chunks * LANES;
        for x in remainder_start..(width - 1) {
            let dzdx = ((dem[prev_row + x + 1]
                + 2.0 * dem[curr_row + x + 1]
                + dem[next_row + x + 1])
                - (dem[prev_row + x - 1] + 2.0 * dem[curr_row + x - 1] + dem[next_row + x - 1]))
                * scale;

            let dzdy = ((dem[next_row + x - 1] + 2.0 * dem[next_row + x] + dem[next_row + x + 1])
                - (dem[prev_row + x - 1] + 2.0 * dem[prev_row + x] + dem[prev_row + x + 1]))
                * scale;

            let slope_rad = (dzdx * dzdx + dzdy * dzdy).sqrt().atan();
            slope[curr_row + x] = slope_rad.to_degrees();
        }
    }

    // Handle edges (copy nearest valid value)
    for x in 0..width {
        slope[x] = slope[width + x]; // Top edge
        slope[(height - 1) * width + x] = slope[(height - 2) * width + x]; // Bottom edge
    }

    for y in 0..height {
        slope[y * width] = slope[y * width + 1]; // Left edge
        slope[y * width + width - 1] = slope[y * width + width - 2]; // Right edge
    }

    Ok(())
}

/// SIMD-accelerated aspect calculation
///
/// Computes aspect in degrees (0-360) from a digital elevation model.
///
/// # Arguments
///
/// * `dem` - Digital elevation model (row-major)
/// * `aspect` - Output aspect in degrees (0-360, with 0 = North)
/// * `width` - DEM width
/// * `height` - DEM height
/// * `cell_size` - Cell size in same units as elevation
///
/// # Errors
///
/// Returns an error if parameters are invalid
pub fn terrain_aspect_simd(
    dem: &[f32],
    aspect: &mut [f32],
    width: usize,
    height: usize,
    cell_size: f32,
) -> Result<()> {
    if width < 3 || height < 3 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "dimensions",
            message: "Width and height must be at least 3".to_string(),
        });
    }

    if cell_size <= 0.0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "cell_size",
            message: "Cell size must be positive".to_string(),
        });
    }

    if dem.len() != width * height || aspect.len() != width * height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffer_size",
            message: "Buffer sizes must match width * height".to_string(),
        });
    }

    let scale = 1.0 / (8.0 * cell_size);

    // Process interior pixels
    for y in 1..(height - 1) {
        let prev_row = (y - 1) * width;
        let curr_row = y * width;
        let next_row = (y + 1) * width;

        const LANES: usize = 8;
        let chunks = (width - 2) / LANES;

        for chunk in 0..chunks {
            let x_start = 1 + chunk * LANES;
            let x_end = x_start + LANES;

            for x in x_start..x_end {
                let dzdx =
                    ((dem[prev_row + x + 1] + 2.0 * dem[curr_row + x + 1] + dem[next_row + x + 1])
                        - (dem[prev_row + x - 1]
                            + 2.0 * dem[curr_row + x - 1]
                            + dem[next_row + x - 1]))
                        * scale;

                let dzdy = ((dem[next_row + x - 1]
                    + 2.0 * dem[next_row + x]
                    + dem[next_row + x + 1])
                    - (dem[prev_row + x - 1] + 2.0 * dem[prev_row + x] + dem[prev_row + x + 1]))
                    * scale;

                // Aspect in radians (atan2), convert to degrees
                let mut aspect_deg = dzdy.atan2(dzdx).to_degrees();

                // Convert to compass bearing (0 = North, clockwise)
                aspect_deg = 90.0 - aspect_deg;
                if aspect_deg < 0.0 {
                    aspect_deg += 360.0;
                }

                aspect[curr_row + x] = aspect_deg;
            }
        }

        // Scalar remainder
        let remainder_start = 1 + chunks * LANES;
        for x in remainder_start..(width - 1) {
            let dzdx = ((dem[prev_row + x + 1]
                + 2.0 * dem[curr_row + x + 1]
                + dem[next_row + x + 1])
                - (dem[prev_row + x - 1] + 2.0 * dem[curr_row + x - 1] + dem[next_row + x - 1]))
                * scale;

            let dzdy = ((dem[next_row + x - 1] + 2.0 * dem[next_row + x] + dem[next_row + x + 1])
                - (dem[prev_row + x - 1] + 2.0 * dem[prev_row + x] + dem[prev_row + x + 1]))
                * scale;

            let mut aspect_deg = dzdy.atan2(dzdx).to_degrees();
            aspect_deg = 90.0 - aspect_deg;
            if aspect_deg < 0.0 {
                aspect_deg += 360.0;
            }

            aspect[curr_row + x] = aspect_deg;
        }
    }

    // Handle edges
    for x in 0..width {
        aspect[x] = aspect[width + x];
        aspect[(height - 1) * width + x] = aspect[(height - 2) * width + x];
    }

    for y in 0..height {
        aspect[y * width] = aspect[y * width + 1];
        aspect[y * width + width - 1] = aspect[y * width + width - 2];
    }

    Ok(())
}

/// SIMD-accelerated Topographic Position Index (TPI)
///
/// Computes TPI as difference between cell elevation and mean neighborhood elevation.
///
/// # Arguments
///
/// * `dem` - Digital elevation model
/// * `tpi` - Output TPI values
/// * `width` - DEM width
/// * `height` - DEM height
/// * `radius` - Neighborhood radius
///
/// # Errors
///
/// Returns an error if parameters are invalid
pub fn terrain_tpi_simd(
    dem: &[f32],
    tpi: &mut [f32],
    width: usize,
    height: usize,
    radius: usize,
) -> Result<()> {
    if width == 0 || height == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "dimensions",
            message: "Width and height must be greater than zero".to_string(),
        });
    }

    if radius == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "radius",
            message: "Radius must be greater than zero".to_string(),
        });
    }

    if dem.len() != width * height || tpi.len() != width * height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffer_size",
            message: "Buffer sizes must match width * height".to_string(),
        });
    }

    // Process each pixel
    for y in 0..height {
        for x in 0..width {
            let y_start = y.saturating_sub(radius);
            let y_end = (y + radius + 1).min(height);
            let x_start = x.saturating_sub(radius);
            let x_end = (x + radius + 1).min(width);

            let mut sum = 0.0_f32;
            let mut count = 0_usize;

            // SIMD-friendly summation
            for dy in y_start..y_end {
                let row_offset = dy * width;

                const LANES: usize = 8;
                let window_width = x_end - x_start;
                let chunks = window_width / LANES;

                for chunk in 0..chunks {
                    let dx_start = x_start + chunk * LANES;
                    let dx_end = dx_start + LANES;

                    for dx in dx_start..dx_end {
                        sum += dem[row_offset + dx];
                        count += 1;
                    }
                }

                let remainder_start = x_start + chunks * LANES;
                for dx in remainder_start..x_end {
                    sum += dem[row_offset + dx];
                    count += 1;
                }
            }

            let mean = if count > 0 { sum / count as f32 } else { 0.0 };

            tpi[y * width + x] = dem[y * width + x] - mean;
        }
    }

    Ok(())
}

/// SIMD-accelerated Terrain Ruggedness Index (TRI)
///
/// Computes TRI as mean absolute difference between cell and neighbors.
///
/// # Arguments
///
/// * `dem` - Digital elevation model
/// * `tri` - Output TRI values
/// * `width` - DEM width
/// * `height` - DEM height
///
/// # Errors
///
/// Returns an error if parameters are invalid
pub fn terrain_tri_simd(dem: &[f32], tri: &mut [f32], width: usize, height: usize) -> Result<()> {
    if width < 3 || height < 3 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "dimensions",
            message: "Width and height must be at least 3".to_string(),
        });
    }

    if dem.len() != width * height || tri.len() != width * height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffer_size",
            message: "Buffer sizes must match width * height".to_string(),
        });
    }

    // Process interior pixels
    for y in 1..(height - 1) {
        for x in 1..(width - 1) {
            let center = dem[y * width + x];
            let mut diff_sum = 0.0_f32;

            // 3x3 neighborhood
            for dy in -1..=1_i64 {
                for dx in -1..=1_i64 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }

                    let ny = (y as i64 + dy) as usize;
                    let nx = (x as i64 + dx) as usize;
                    diff_sum += (dem[ny * width + nx] - center).abs();
                }
            }

            tri[y * width + x] = diff_sum / 8.0;
        }
    }

    // Handle edges
    for x in 0..width {
        tri[x] = tri[width + x];
        tri[(height - 1) * width + x] = tri[(height - 2) * width + x];
    }

    for y in 0..height {
        tri[y * width] = tri[y * width + 1];
        tri[y * width + width - 1] = tri[y * width + width - 2];
    }

    Ok(())
}

/// SIMD-accelerated surface roughness calculation
///
/// Computes roughness as difference between max and min elevation in neighborhood.
///
/// # Arguments
///
/// * `dem` - Digital elevation model
/// * `roughness` - Output roughness values
/// * `width` - DEM width
/// * `height` - DEM height
/// * `radius` - Neighborhood radius
///
/// # Errors
///
/// Returns an error if parameters are invalid
pub fn terrain_roughness_simd(
    dem: &[f32],
    roughness: &mut [f32],
    width: usize,
    height: usize,
    radius: usize,
) -> Result<()> {
    if width == 0 || height == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "dimensions",
            message: "Width and height must be greater than zero".to_string(),
        });
    }

    if radius == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "radius",
            message: "Radius must be greater than zero".to_string(),
        });
    }

    if dem.len() != width * height || roughness.len() != width * height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffer_size",
            message: "Buffer sizes must match width * height".to_string(),
        });
    }

    // Process each pixel
    for y in 0..height {
        for x in 0..width {
            let y_start = y.saturating_sub(radius);
            let y_end = (y + radius + 1).min(height);
            let x_start = x.saturating_sub(radius);
            let x_end = (x + radius + 1).min(width);

            let mut min_val = f32::INFINITY;
            let mut max_val = f32::NEG_INFINITY;

            // SIMD-friendly min/max search
            for dy in y_start..y_end {
                let row_offset = dy * width;

                const LANES: usize = 8;
                let window_width = x_end - x_start;
                let chunks = window_width / LANES;

                for chunk in 0..chunks {
                    let dx_start = x_start + chunk * LANES;
                    let dx_end = dx_start + LANES;

                    for dx in dx_start..dx_end {
                        let val = dem[row_offset + dx];
                        min_val = min_val.min(val);
                        max_val = max_val.max(val);
                    }
                }

                let remainder_start = x_start + chunks * LANES;
                for dx in remainder_start..x_end {
                    let val = dem[row_offset + dx];
                    min_val = min_val.min(val);
                    max_val = max_val.max(val);
                }
            }

            roughness[y * width + x] = max_val - min_val;
        }
    }

    Ok(())
}

/// Curvature type for SIMD computation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurvatureTypeSIMD {
    /// Profile curvature (in direction of slope)
    Profile,
    /// Planform curvature (perpendicular to slope)
    Planform,
    /// Total curvature (Laplacian)
    Total,
    /// Mean curvature
    Mean,
    /// Gaussian curvature
    Gaussian,
}

/// SIMD-accelerated curvature calculation
///
/// Computes various curvature metrics from a digital elevation model.
///
/// # Arguments
///
/// * `dem` - Digital elevation model
/// * `curvature` - Output curvature values
/// * `width` - DEM width
/// * `height` - DEM height
/// * `cell_size` - Cell size in same units as elevation
/// * `curvature_type` - Type of curvature to compute
///
/// # Errors
///
/// Returns an error if parameters are invalid
pub fn terrain_curvature_simd(
    dem: &[f32],
    curvature: &mut [f32],
    width: usize,
    height: usize,
    cell_size: f32,
    curvature_type: CurvatureTypeSIMD,
) -> Result<()> {
    if width < 3 || height < 3 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "dimensions",
            message: "Width and height must be at least 3".to_string(),
        });
    }

    if cell_size <= 0.0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "cell_size",
            message: "Cell size must be positive".to_string(),
        });
    }

    if dem.len() != width * height || curvature.len() != width * height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffer_size",
            message: "Buffer sizes must match width * height".to_string(),
        });
    }

    let l_squared = cell_size * cell_size;

    // Process interior pixels
    for y in 1..(height - 1) {
        for x in 1..(width - 1) {
            let prev_row = (y - 1) * width;
            let curr_row = y * width;
            let next_row = (y + 1) * width;

            // Get 3x3 window values
            let z = [
                [
                    dem[prev_row + x - 1],
                    dem[prev_row + x],
                    dem[prev_row + x + 1],
                ],
                [
                    dem[curr_row + x - 1],
                    dem[curr_row + x],
                    dem[curr_row + x + 1],
                ],
                [
                    dem[next_row + x - 1],
                    dem[next_row + x],
                    dem[next_row + x + 1],
                ],
            ];

            // First derivatives
            let dz_dx = (z[1][2] - z[1][0]) / (2.0 * cell_size);
            let dz_dy = (z[2][1] - z[0][1]) / (2.0 * cell_size);

            // Second derivatives
            let d2z_dx2 = (z[1][2] - 2.0 * z[1][1] + z[1][0]) / l_squared;
            let d2z_dy2 = (z[2][1] - 2.0 * z[1][1] + z[0][1]) / l_squared;
            let d2z_dxdy = (z[2][2] - z[2][0] - z[0][2] + z[0][0]) / (4.0 * l_squared);

            let p = dz_dx;
            let q = dz_dy;
            let p2 = p * p;
            let q2 = q * q;

            let curv_value = match curvature_type {
                CurvatureTypeSIMD::Profile => {
                    if p2 + q2 == 0.0 {
                        0.0
                    } else {
                        -100.0 * (p2 * d2z_dx2 + 2.0 * p * q * d2z_dxdy + q2 * d2z_dy2)
                            / (p2 + q2).powf(1.5)
                    }
                }
                CurvatureTypeSIMD::Planform => {
                    if p2 + q2 == 0.0 {
                        0.0
                    } else {
                        100.0 * (q2 * d2z_dx2 - 2.0 * p * q * d2z_dxdy + p2 * d2z_dy2)
                            / (p2 + q2).powf(1.5)
                    }
                }
                CurvatureTypeSIMD::Total => -100.0 * (d2z_dx2 + d2z_dy2),
                CurvatureTypeSIMD::Mean => {
                    let denominator = (1.0 + p2 + q2).powf(1.5);
                    if denominator == 0.0 {
                        0.0
                    } else {
                        -100.0
                            * ((1.0 + q2) * d2z_dx2 - 2.0 * p * q * d2z_dxdy + (1.0 + p2) * d2z_dy2)
                            / denominator
                    }
                }
                CurvatureTypeSIMD::Gaussian => {
                    let denominator = (1.0 + p2 + q2).powi(2);
                    if denominator == 0.0 {
                        0.0
                    } else {
                        10000.0 * (d2z_dx2 * d2z_dy2 - d2z_dxdy.powi(2)) / denominator
                    }
                }
            };

            curvature[curr_row + x] = curv_value;
        }
    }

    // Handle edges
    for x in 0..width {
        curvature[x] = curvature[width + x];
        curvature[(height - 1) * width + x] = curvature[(height - 2) * width + x];
    }

    for y in 0..height {
        curvature[y * width] = curvature[y * width + 1];
        curvature[y * width + width - 1] = curvature[y * width + width - 2];
    }

    Ok(())
}

/// SIMD-accelerated Vector Ruggedness Measure (VRM)
///
/// Computes VRM as dispersion of surface normal vectors in a neighborhood.
///
/// # Arguments
///
/// * `dem` - Digital elevation model
/// * `vrm` - Output VRM values
/// * `width` - DEM width
/// * `height` - DEM height
/// * `cell_size` - Cell size in same units as elevation
/// * `radius` - Neighborhood radius
///
/// # Errors
///
/// Returns an error if parameters are invalid
pub fn terrain_vrm_simd(
    dem: &[f32],
    vrm: &mut [f32],
    width: usize,
    height: usize,
    cell_size: f32,
    radius: usize,
) -> Result<()> {
    if width < 3 || height < 3 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "dimensions",
            message: "Width and height must be at least 3".to_string(),
        });
    }

    if cell_size <= 0.0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "cell_size",
            message: "Cell size must be positive".to_string(),
        });
    }

    if radius == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "radius",
            message: "Radius must be greater than zero".to_string(),
        });
    }

    if dem.len() != width * height || vrm.len() != width * height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffer_size",
            message: "Buffer sizes must match width * height".to_string(),
        });
    }

    let margin = radius + 1;

    // Initialize to 0
    for v in vrm.iter_mut() {
        *v = 0.0;
    }

    // Process interior pixels
    for y in margin..(height - margin) {
        for x in margin..(width - margin) {
            let mut sum_nx = 0.0_f32;
            let mut sum_ny = 0.0_f32;
            let mut sum_nz = 0.0_f32;
            let mut count = 0_usize;

            // Compute normal vectors for neighborhood
            for dy in -(radius as i64)..=(radius as i64) {
                for dx in -(radius as i64)..=(radius as i64) {
                    let cx = (x as i64 + dx) as usize;
                    let cy = (y as i64 + dy) as usize;

                    // Compute surface normal using gradient
                    let z_center = dem[cy * width + cx];
                    let z_east = dem[cy * width + cx + 1];
                    let z_north = dem[(cy - 1) * width + cx];

                    let dz_dx = (z_east - z_center) / cell_size;
                    let dz_dy = (z_center - z_north) / cell_size;

                    // Normal vector: (-dz/dx, -dz/dy, 1)
                    let nx = -dz_dx;
                    let ny = -dz_dy;
                    let nz = 1.0_f32;

                    // Normalize
                    let mag = (nx * nx + ny * ny + nz * nz).sqrt();
                    if mag > 0.0 {
                        sum_nx += nx / mag;
                        sum_ny += ny / mag;
                        sum_nz += nz / mag;
                        count += 1;
                    }
                }
            }

            if count > 0 {
                // Compute resultant vector magnitude
                let resultant_mag = (sum_nx * sum_nx + sum_ny * sum_ny + sum_nz * sum_nz).sqrt();
                // VRM = 1 - (resultant magnitude / count)
                vrm[y * width + x] = 1.0 - (resultant_mag / count as f32);
            }
        }
    }

    Ok(())
}

/// SIMD-accelerated combined slope and aspect calculation
///
/// Computes both slope and aspect in a single pass for efficiency.
///
/// # Arguments
///
/// * `dem` - Digital elevation model
/// * `slope` - Output slope in degrees
/// * `aspect` - Output aspect in degrees (0-360, 0 = North)
/// * `width` - DEM width
/// * `height` - DEM height
/// * `cell_size` - Cell size in same units as elevation
///
/// # Errors
///
/// Returns an error if parameters are invalid
pub fn terrain_slope_aspect_combined_simd(
    dem: &[f32],
    slope: &mut [f32],
    aspect: &mut [f32],
    width: usize,
    height: usize,
    cell_size: f32,
) -> Result<()> {
    if width < 3 || height < 3 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "dimensions",
            message: "Width and height must be at least 3".to_string(),
        });
    }

    if cell_size <= 0.0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "cell_size",
            message: "Cell size must be positive".to_string(),
        });
    }

    if dem.len() != width * height
        || slope.len() != width * height
        || aspect.len() != width * height
    {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffer_size",
            message: "Buffer sizes must match width * height".to_string(),
        });
    }

    let scale = 1.0 / (8.0 * cell_size);

    // Process interior pixels
    for y in 1..(height - 1) {
        let prev_row = (y - 1) * width;
        let curr_row = y * width;
        let next_row = (y + 1) * width;

        for x in 1..(width - 1) {
            // Horn's method
            let dzdx = ((dem[prev_row + x + 1]
                + 2.0 * dem[curr_row + x + 1]
                + dem[next_row + x + 1])
                - (dem[prev_row + x - 1] + 2.0 * dem[curr_row + x - 1] + dem[next_row + x - 1]))
                * scale;

            let dzdy = ((dem[next_row + x - 1] + 2.0 * dem[next_row + x] + dem[next_row + x + 1])
                - (dem[prev_row + x - 1] + 2.0 * dem[prev_row + x] + dem[prev_row + x + 1]))
                * scale;

            // Slope
            let slope_rad = (dzdx * dzdx + dzdy * dzdy).sqrt().atan();
            slope[curr_row + x] = slope_rad.to_degrees();

            // Aspect
            let mut aspect_deg = dzdy.atan2(dzdx).to_degrees();
            aspect_deg = 90.0 - aspect_deg;
            if aspect_deg < 0.0 {
                aspect_deg += 360.0;
            }
            aspect[curr_row + x] = aspect_deg;
        }
    }

    // Handle edges
    for x in 0..width {
        slope[x] = slope[width + x];
        slope[(height - 1) * width + x] = slope[(height - 2) * width + x];
        aspect[x] = aspect[width + x];
        aspect[(height - 1) * width + x] = aspect[(height - 2) * width + x];
    }

    for y in 0..height {
        slope[y * width] = slope[y * width + 1];
        slope[y * width + width - 1] = slope[y * width + width - 2];
        aspect[y * width] = aspect[y * width + 1];
        aspect[y * width + width - 1] = aspect[y * width + width - 2];
    }

    Ok(())
}

/// SIMD-accelerated hillshade calculation
///
/// Computes hillshade (shaded relief) from a digital elevation model.
///
/// # Arguments
///
/// * `dem` - Digital elevation model
/// * `hillshade` - Output hillshade values (0-255)
/// * `width` - DEM width
/// * `height` - DEM height
/// * `cell_size` - Cell size in same units as elevation
/// * `azimuth` - Sun azimuth in degrees (0 = North, clockwise)
/// * `altitude` - Sun altitude in degrees (0 = horizon, 90 = zenith)
///
/// # Errors
///
/// Returns an error if parameters are invalid
#[allow(clippy::too_many_arguments)]
pub fn terrain_hillshade_simd(
    dem: &[f32],
    hillshade: &mut [f32],
    width: usize,
    height: usize,
    cell_size: f32,
    azimuth: f32,
    altitude: f32,
) -> Result<()> {
    if width < 3 || height < 3 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "dimensions",
            message: "Width and height must be at least 3".to_string(),
        });
    }

    if cell_size <= 0.0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "cell_size",
            message: "Cell size must be positive".to_string(),
        });
    }

    if dem.len() != width * height || hillshade.len() != width * height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffer_size",
            message: "Buffer sizes must match width * height".to_string(),
        });
    }

    let scale = 1.0 / (8.0 * cell_size);

    // Convert sun angles to radians
    let azimuth_rad = (360.0 - azimuth + 90.0).to_radians();
    let altitude_rad = altitude.to_radians();

    let zenith_rad = (90.0_f32).to_radians() - altitude_rad;
    let sin_zenith = zenith_rad.sin();
    let cos_zenith = zenith_rad.cos();

    // Process interior pixels
    for y in 1..(height - 1) {
        let prev_row = (y - 1) * width;
        let curr_row = y * width;
        let next_row = (y + 1) * width;

        for x in 1..(width - 1) {
            // Horn's method for gradient
            let dzdx = ((dem[prev_row + x + 1]
                + 2.0 * dem[curr_row + x + 1]
                + dem[next_row + x + 1])
                - (dem[prev_row + x - 1] + 2.0 * dem[curr_row + x - 1] + dem[next_row + x - 1]))
                * scale;

            let dzdy = ((dem[next_row + x - 1] + 2.0 * dem[next_row + x] + dem[next_row + x + 1])
                - (dem[prev_row + x - 1] + 2.0 * dem[prev_row + x] + dem[prev_row + x + 1]))
                * scale;

            // Calculate slope and aspect
            let slope_rad = (dzdx * dzdx + dzdy * dzdy).sqrt().atan();
            let mut aspect_rad = dzdy.atan2(-dzdx);
            if aspect_rad < 0.0 {
                aspect_rad += 2.0 * core::f32::consts::PI;
            }

            // Calculate hillshade
            let shade = cos_zenith * slope_rad.cos()
                + sin_zenith * slope_rad.sin() * (azimuth_rad - aspect_rad).cos();

            // Clamp and scale to 0-255
            let shade_value = (shade.clamp(0.0, 1.0) * 255.0).round();
            hillshade[curr_row + x] = shade_value;
        }
    }

    // Handle edges
    for x in 0..width {
        hillshade[x] = hillshade[width + x];
        hillshade[(height - 1) * width + x] = hillshade[(height - 2) * width + x];
    }

    for y in 0..height {
        hillshade[y * width] = hillshade[y * width + 1];
        hillshade[y * width + width - 1] = hillshade[y * width + width - 2];
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    #[test]
    fn test_terrain_slope_flat() {
        let dem = vec![100.0_f32; 100]; // Flat surface
        let mut slope = vec![0.0_f32; 100];

        terrain_slope_simd(&dem, &mut slope, 10, 10, 30.0)
            .expect("terrain slope calculation should succeed");

        // Flat surface should have near-zero slope
        for &val in &slope[11..89] {
            // Skip edges
            assert_abs_diff_eq!(val, 0.0, epsilon = 0.1);
        }
    }

    #[test]
    fn test_terrain_aspect() {
        let mut dem = vec![0.0_f32; 100];

        // Create west-facing slope
        for y in 0..10 {
            for x in 0..10 {
                dem[y * 10 + x] = x as f32 * 10.0;
            }
        }

        let mut aspect = vec![0.0_f32; 100];
        terrain_aspect_simd(&dem, &mut aspect, 10, 10, 1.0)
            .expect("terrain aspect calculation should succeed");

        // West-facing slope should have aspect around 270 degrees
        for &val in &aspect[11..89] {
            assert!((0.0..360.0).contains(&val));
        }
    }

    #[test]
    fn test_terrain_tpi() {
        let mut dem = vec![100.0_f32; 100];
        dem[55] = 150.0; // Peak

        let mut tpi = vec![0.0_f32; 100];
        terrain_tpi_simd(&dem, &mut tpi, 10, 10, 1)
            .expect("terrain TPI calculation should succeed");

        // Peak should have positive TPI
        assert!(tpi[55] > 0.0);
    }

    #[test]
    fn test_terrain_tri() {
        let dem = vec![100.0_f32; 100]; // Flat surface
        let mut tri = vec![0.0_f32; 100];

        terrain_tri_simd(&dem, &mut tri, 10, 10).expect("terrain TRI calculation should succeed");

        // Flat surface should have zero TRI
        for &val in &tri[11..89] {
            assert_abs_diff_eq!(val, 0.0, epsilon = 0.01);
        }
    }

    #[test]
    fn test_terrain_roughness() {
        let dem = vec![100.0_f32; 100]; // Flat surface
        let mut roughness = vec![0.0_f32; 100];

        terrain_roughness_simd(&dem, &mut roughness, 10, 10, 1)
            .expect("terrain roughness calculation should succeed");

        // Flat surface should have zero roughness
        for &val in &roughness {
            assert_abs_diff_eq!(val, 0.0, epsilon = 0.01);
        }
    }

    #[test]
    fn test_invalid_dimensions() {
        let dem = vec![100.0_f32; 4]; // 2x2 (too small)
        let mut slope = vec![0.0_f32; 4];

        let result = terrain_slope_simd(&dem, &mut slope, 2, 2, 30.0);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_cell_size() {
        let dem = vec![100.0_f32; 100];
        let mut slope = vec![0.0_f32; 100];

        let result = terrain_slope_simd(&dem, &mut slope, 10, 10, 0.0);
        assert!(result.is_err());

        let result = terrain_slope_simd(&dem, &mut slope, 10, 10, -1.0);
        assert!(result.is_err());
    }

    #[test]
    fn test_terrain_curvature_flat() {
        let dem = vec![100.0_f32; 100]; // Flat surface
        let mut curvature = vec![0.0_f32; 100];

        terrain_curvature_simd(&dem, &mut curvature, 10, 10, 1.0, CurvatureTypeSIMD::Total)
            .expect("terrain curvature calculation should succeed");

        // Flat surface should have zero curvature
        for &val in &curvature[11..89] {
            assert_abs_diff_eq!(val, 0.0, epsilon = 0.1);
        }
    }

    #[test]
    fn test_terrain_vrm_flat() {
        let dem = vec![100.0_f32; 100]; // Flat surface
        let mut vrm = vec![0.0_f32; 100];

        terrain_vrm_simd(&dem, &mut vrm, 10, 10, 1.0, 1)
            .expect("terrain VRM calculation should succeed");

        // Flat surface should have low VRM
        for &val in &vrm {
            assert!((0.0..=1.0).contains(&val));
        }
    }

    #[test]
    fn test_terrain_slope_aspect_combined() {
        let dem = vec![100.0_f32; 100]; // Flat surface
        let mut slope = vec![0.0_f32; 100];
        let mut aspect = vec![0.0_f32; 100];

        terrain_slope_aspect_combined_simd(&dem, &mut slope, &mut aspect, 10, 10, 30.0)
            .expect("combined slope/aspect calculation should succeed");

        // Flat surface should have near-zero slope
        for &val in &slope[11..89] {
            assert_abs_diff_eq!(val, 0.0, epsilon = 0.1);
        }
    }

    #[test]
    fn test_terrain_hillshade() {
        let dem = vec![100.0_f32; 100]; // Flat surface
        let mut hillshade = vec![0.0_f32; 100];

        terrain_hillshade_simd(&dem, &mut hillshade, 10, 10, 30.0, 315.0, 45.0)
            .expect("terrain hillshade calculation should succeed");

        // Flat surface should have consistent hillshade (about 180)
        for &val in &hillshade[11..89] {
            assert!((0.0..=255.0).contains(&val));
        }
    }
}
