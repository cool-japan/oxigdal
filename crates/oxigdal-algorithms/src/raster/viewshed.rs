//! Viewshed analysis for visibility computation
//!
//! Provides multiple algorithms for determining visibility from one or more
//! observer points across a digital elevation model (DEM).
//!
//! # Algorithms
//!
//! ## R1: Line-of-sight sampling (basic)
//!
//! Samples elevation along rays from observer to each target cell.
//! Simple and correct, but O(n^3) for an n x n raster.
//!
//! ## R2: Reference plane method
//!
//! Uses a sweep-based approach that processes cells along radial lines from
//! the observer. Each line is traced outward, maintaining the maximum angle
//! seen so far. More efficient than R1 for large rasters.
//!
//! ## R3: Wang et al. (2000) sweep line
//!
//! An O(n^2 log n) algorithm that sweeps a rotating line around the observer.
//! Uses a balanced binary search tree (approximated here) to efficiently track
//! which cells are visible. Most efficient for very large rasters.
//!
//! # Features
//!
//! - **Earth curvature correction**: Adjusts effective elevations for Earth's
//!   curvature and atmospheric refraction (using coefficient of refraction k).
//! - **Multiple observer support**: Cumulative viewshed from many observers.
//! - **Height offsets**: Observer and target height above ground level.
//! - **Maximum distance**: Limit the analysis radius.
//! - **Fresnel zone analysis**: Approximate radio wave propagation line-of-sight.
//!
//! # References
//!
//! - Wang, J. et al. (2000). "Efficient viewshed algorithms on raster terrain."
//! - De Floriani, L. & Magillo, P. (2003). "Algorithms for visibility computation on terrains."
//! - Franklin, W.R. & Ray, C.K. (1994). "Higher isn't necessarily better."

use crate::error::{AlgorithmError, Result};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;

#[cfg(feature = "parallel")]
use rayon::prelude::*;

// ===========================================================================
// Configuration types
// ===========================================================================

/// Algorithm to use for viewshed computation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewshedAlgorithm {
    /// R1: Line-of-sight ray sampling (basic, accurate)
    #[default]
    R1LineOfSight,

    /// R2: Reference plane / radial sweep (faster for large rasters)
    R2ReferencePlane,

    /// R3: Sweep line with angular sorting (most efficient for huge rasters)
    R3SweepLine,
}

/// Earth curvature correction parameters
#[derive(Debug, Clone, Copy)]
pub struct CurvatureCorrection {
    /// Earth radius in same units as cell_size (e.g., 6371000.0 for meters)
    pub earth_radius: f64,

    /// Atmospheric refraction coefficient (typically 0.13 for standard atmosphere)
    /// Effective radius = earth_radius / (1 - k)
    pub refraction_coefficient: f64,
}

impl Default for CurvatureCorrection {
    fn default() -> Self {
        Self {
            earth_radius: 6_371_000.0,
            refraction_coefficient: 0.13,
        }
    }
}

/// Configuration for viewshed analysis
#[derive(Debug, Clone)]
pub struct ViewshedConfig {
    /// Observer X coordinate (column)
    pub observer_x: u64,
    /// Observer Y coordinate (row)
    pub observer_y: u64,
    /// Observer height above ground (meters)
    pub observer_height: f64,
    /// Target height above ground (meters)
    pub target_height: f64,
    /// Maximum viewing distance (None = unlimited)
    pub max_distance: Option<f64>,
    /// Cell size (ground distance per pixel)
    pub cell_size: f64,
    /// Algorithm to use
    pub algorithm: ViewshedAlgorithm,
    /// Earth curvature correction (None = no correction)
    pub curvature_correction: Option<CurvatureCorrection>,
}

/// An observer point with location and height
#[derive(Debug, Clone, Copy)]
pub struct ObserverPoint {
    /// X coordinate (column)
    pub x: u64,
    /// Y coordinate (row)
    pub y: u64,
    /// Height above ground
    pub height: f64,
}

/// Result of a viewshed analysis
#[derive(Debug)]
pub struct ViewshedResult {
    /// Binary visibility raster (1.0 = visible, 0.0 = not visible)
    pub visibility: RasterBuffer,
    /// Elevation angle raster (angle from horizontal at observer to each visible cell, in radians)
    /// Positive = above horizon, negative = below. Only set for visible cells.
    pub elevation_angle: Option<RasterBuffer>,
}

// ===========================================================================
// Main entry points
// ===========================================================================

/// Computes viewshed from a single observer point (backward-compatible)
///
/// Returns a binary raster where 1 = visible, 0 = not visible.
///
/// # Arguments
///
/// * `dem` - Digital elevation model
/// * `observer_x` - X coordinate of observer
/// * `observer_y` - Y coordinate of observer
/// * `observer_height` - Height of observer above ground
/// * `target_height` - Height of targets above ground
/// * `max_distance` - Maximum viewing distance (None = unlimited)
/// * `cell_size` - Size of each cell
///
/// # Errors
///
/// Returns an error if the observer is outside the DEM bounds
pub fn compute_viewshed(
    dem: &RasterBuffer,
    observer_x: u64,
    observer_y: u64,
    observer_height: f64,
    target_height: f64,
    max_distance: Option<f64>,
    cell_size: f64,
) -> Result<RasterBuffer> {
    let config = ViewshedConfig {
        observer_x,
        observer_y,
        observer_height,
        target_height,
        max_distance,
        cell_size,
        algorithm: ViewshedAlgorithm::R1LineOfSight,
        curvature_correction: None,
    };

    let result = compute_viewshed_advanced(dem, &config)?;
    Ok(result.visibility)
}

/// Computes viewshed with full configuration
///
/// # Arguments
///
/// * `dem` - Digital elevation model
/// * `config` - Full viewshed configuration
///
/// # Errors
///
/// Returns an error if the observer is outside the DEM bounds
pub fn compute_viewshed_advanced(
    dem: &RasterBuffer,
    config: &ViewshedConfig,
) -> Result<ViewshedResult> {
    // Validate observer position
    if config.observer_x >= dem.width() || config.observer_y >= dem.height() {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "observer position",
            message: format!(
                "Observer ({}, {}) is outside DEM bounds ({}, {})",
                config.observer_x,
                config.observer_y,
                dem.width(),
                dem.height()
            ),
        });
    }

    match config.algorithm {
        ViewshedAlgorithm::R1LineOfSight => viewshed_r1(dem, config),
        ViewshedAlgorithm::R2ReferencePlane => viewshed_r2(dem, config),
        ViewshedAlgorithm::R3SweepLine => viewshed_r3(dem, config),
    }
}

/// Computes cumulative viewshed from multiple observers
///
/// Returns a raster with count of observers that can see each cell.
///
/// # Arguments
///
/// * `dem` - Digital elevation model
/// * `observers` - List of (x, y, height) tuples
/// * `target_height` - Height of targets above ground
/// * `max_distance` - Maximum viewing distance
/// * `cell_size` - Cell size
///
/// # Errors
///
/// Returns an error if any observer is outside the DEM bounds
pub fn compute_cumulative_viewshed(
    dem: &RasterBuffer,
    observers: &[(u64, u64, f64)],
    target_height: f64,
    max_distance: Option<f64>,
    cell_size: f64,
) -> Result<RasterBuffer> {
    let observer_points: Vec<ObserverPoint> = observers
        .iter()
        .map(|&(x, y, height)| ObserverPoint { x, y, height })
        .collect();

    compute_cumulative_viewshed_advanced(
        dem,
        &observer_points,
        target_height,
        max_distance,
        cell_size,
        None,
    )
}

/// Computes cumulative viewshed with advanced options
///
/// # Arguments
///
/// * `dem` - Digital elevation model
/// * `observers` - List of observer points
/// * `target_height` - Target height above ground
/// * `max_distance` - Maximum distance
/// * `cell_size` - Cell size
/// * `curvature` - Optional earth curvature correction
///
/// # Errors
///
/// Returns an error if any observer is outside the DEM bounds
pub fn compute_cumulative_viewshed_advanced(
    dem: &RasterBuffer,
    observers: &[ObserverPoint],
    target_height: f64,
    max_distance: Option<f64>,
    cell_size: f64,
    curvature: Option<CurvatureCorrection>,
) -> Result<RasterBuffer> {
    let width = dem.width();
    let height = dem.height();
    let mut cumulative = RasterBuffer::zeros(width, height, RasterDataType::Float64);

    #[cfg(feature = "parallel")]
    {
        let viewsheds: Result<Vec<RasterBuffer>> = observers
            .par_iter()
            .map(|obs| {
                let config = ViewshedConfig {
                    observer_x: obs.x,
                    observer_y: obs.y,
                    observer_height: obs.height,
                    target_height,
                    max_distance,
                    cell_size,
                    algorithm: ViewshedAlgorithm::R2ReferencePlane,
                    curvature_correction: curvature,
                };
                let result = compute_viewshed_advanced(dem, &config)?;
                Ok(result.visibility)
            })
            .collect();

        for viewshed in viewsheds? {
            for y in 0..height {
                for x in 0..width {
                    let visible = viewshed.get_pixel(x, y).map_err(AlgorithmError::Core)?;
                    if visible > 0.0 {
                        let current = cumulative.get_pixel(x, y).map_err(AlgorithmError::Core)?;
                        cumulative
                            .set_pixel(x, y, current + 1.0)
                            .map_err(AlgorithmError::Core)?;
                    }
                }
            }
        }
    }

    #[cfg(not(feature = "parallel"))]
    {
        for obs in observers {
            let config = ViewshedConfig {
                observer_x: obs.x,
                observer_y: obs.y,
                observer_height: obs.height,
                target_height,
                max_distance,
                cell_size,
                algorithm: ViewshedAlgorithm::R2ReferencePlane,
                curvature_correction: curvature,
            };
            let result = compute_viewshed_advanced(dem, &config)?;

            for y in 0..height {
                for x in 0..width {
                    let visible = result
                        .visibility
                        .get_pixel(x, y)
                        .map_err(AlgorithmError::Core)?;
                    if visible > 0.0 {
                        let current = cumulative.get_pixel(x, y).map_err(AlgorithmError::Core)?;
                        cumulative
                            .set_pixel(x, y, current + 1.0)
                            .map_err(AlgorithmError::Core)?;
                    }
                }
            }
        }
    }

    Ok(cumulative)
}

/// Computes line-of-sight profile between two points
///
/// Returns a vector of (distance, terrain_elevation, los_elevation) tuples
/// showing the terrain profile and the line-of-sight line.
///
/// # Arguments
///
/// * `dem` - Digital elevation model
/// * `from_x`, `from_y` - Observer coordinates
/// * `from_height` - Observer height above ground
/// * `to_x`, `to_y` - Target coordinates
/// * `cell_size` - Cell size
/// * `curvature` - Optional earth curvature correction
///
/// # Errors
///
/// Returns an error if coordinates are out of bounds
pub fn compute_los_profile(
    dem: &RasterBuffer,
    from_x: u64,
    from_y: u64,
    from_height: f64,
    to_x: u64,
    to_y: u64,
    cell_size: f64,
    curvature: Option<CurvatureCorrection>,
) -> Result<Vec<(f64, f64, f64)>> {
    let observer_elev = dem
        .get_pixel(from_x, from_y)
        .map_err(AlgorithmError::Core)?
        + from_height;

    let target_elev = dem.get_pixel(to_x, to_y).map_err(AlgorithmError::Core)?;

    let dx = (to_x as f64 - from_x as f64) * cell_size;
    let dy = (to_y as f64 - from_y as f64) * cell_size;
    let total_distance = (dx * dx + dy * dy).sqrt();

    if total_distance < 1e-10 {
        return Ok(vec![(0.0, observer_elev, observer_elev)]);
    }

    let num_samples = (total_distance / (cell_size * 0.5)).ceil() as usize;
    let num_samples = num_samples.max(2);

    let mut profile = Vec::with_capacity(num_samples);

    for i in 0..num_samples {
        let t = i as f64 / (num_samples - 1) as f64;
        let px = from_x as f64 + (to_x as f64 - from_x as f64) * t;
        let py = from_y as f64 + (to_y as f64 - from_y as f64) * t;

        let ix = (px.round() as u64).min(dem.width() - 1);
        let iy = (py.round() as u64).min(dem.height() - 1);

        let dist = t * total_distance;
        let mut terrain_elev = dem.get_pixel(ix, iy).map_err(AlgorithmError::Core)?;

        // Apply curvature correction to terrain
        if let Some(ref curv) = curvature {
            terrain_elev -= earth_curvature_offset(dist, curv);
        }

        // Line-of-sight elevation at this distance
        let los_elev = observer_elev + (target_elev - observer_elev) * t;

        profile.push((dist, terrain_elev, los_elev));
    }

    Ok(profile)
}

/// Performs Fresnel zone clearance analysis
///
/// Checks whether a radio link between two points has adequate Fresnel zone clearance.
/// Returns the minimum clearance ratio (clearance / Fresnel radius) along the path.
/// Values >= 0.6 typically indicate adequate clearance.
///
/// # Arguments
///
/// * `dem` - Digital elevation model
/// * `from_x`, `from_y` - Transmitter coordinates
/// * `from_height` - Transmitter height above ground
/// * `to_x`, `to_y` - Receiver coordinates
/// * `to_height` - Receiver height above ground
/// * `cell_size` - Cell size
/// * `frequency_ghz` - Radio frequency in GHz
/// * `curvature` - Optional earth curvature correction
///
/// # Errors
///
/// Returns an error if coordinates are out of bounds
pub fn compute_fresnel_clearance(
    dem: &RasterBuffer,
    from_x: u64,
    from_y: u64,
    from_height: f64,
    to_x: u64,
    to_y: u64,
    to_height: f64,
    cell_size: f64,
    frequency_ghz: f64,
    curvature: Option<CurvatureCorrection>,
) -> Result<f64> {
    let obs_elev = dem
        .get_pixel(from_x, from_y)
        .map_err(AlgorithmError::Core)?
        + from_height;
    let tgt_elev = dem.get_pixel(to_x, to_y).map_err(AlgorithmError::Core)? + to_height;

    let dx = (to_x as f64 - from_x as f64) * cell_size;
    let dy = (to_y as f64 - from_y as f64) * cell_size;
    let total_distance = (dx * dx + dy * dy).sqrt();

    if total_distance < 1e-10 {
        return Ok(f64::INFINITY);
    }

    // Wavelength in meters
    let wavelength = 0.3 / frequency_ghz;

    let num_samples = (total_distance / (cell_size * 0.5)).ceil() as usize;
    let num_samples = num_samples.max(2);

    let mut min_clearance_ratio = f64::INFINITY;

    for i in 1..(num_samples - 1) {
        let t = i as f64 / (num_samples - 1) as f64;
        let px = from_x as f64 + (to_x as f64 - from_x as f64) * t;
        let py = from_y as f64 + (to_y as f64 - from_y as f64) * t;

        let ix = (px.round() as u64).min(dem.width() - 1);
        let iy = (py.round() as u64).min(dem.height() - 1);

        let dist_from = t * total_distance;
        let dist_to = (1.0 - t) * total_distance;

        let mut terrain_elev = dem.get_pixel(ix, iy).map_err(AlgorithmError::Core)?;

        if let Some(ref curv) = curvature {
            terrain_elev -= earth_curvature_offset(dist_from, curv);
        }

        // LOS elevation at this point
        let los_elev = obs_elev + (tgt_elev - obs_elev) * t;

        // First Fresnel zone radius at this point
        let fresnel_radius = (wavelength * dist_from * dist_to / total_distance).sqrt();

        // Clearance = LOS elevation - terrain elevation
        let clearance = los_elev - terrain_elev;
        let ratio = if fresnel_radius > 1e-10 {
            clearance / fresnel_radius
        } else {
            if clearance >= 0.0 {
                f64::INFINITY
            } else {
                f64::NEG_INFINITY
            }
        };

        if ratio < min_clearance_ratio {
            min_clearance_ratio = ratio;
        }
    }

    Ok(min_clearance_ratio)
}

// ===========================================================================
// R1: Line-of-sight sampling
// ===========================================================================

/// R1 algorithm: sample points along line from observer to each target cell
fn viewshed_r1(dem: &RasterBuffer, config: &ViewshedConfig) -> Result<ViewshedResult> {
    let width = dem.width();
    let height = dem.height();
    let mut visibility = RasterBuffer::zeros(width, height, RasterDataType::UInt8);
    let mut elev_angle = RasterBuffer::zeros(width, height, RasterDataType::Float64);

    let observer_elev = dem
        .get_pixel(config.observer_x, config.observer_y)
        .map_err(AlgorithmError::Core)?
        + config.observer_height;

    for y in 0..height {
        for x in 0..width {
            if x == config.observer_x && y == config.observer_y {
                visibility
                    .set_pixel(x, y, 1.0)
                    .map_err(AlgorithmError::Core)?;
                continue;
            }

            let dx = (x as f64 - config.observer_x as f64) * config.cell_size;
            let dy = (y as f64 - config.observer_y as f64) * config.cell_size;
            let distance = (dx * dx + dy * dy).sqrt();

            if let Some(max_dist) = config.max_distance {
                if distance > max_dist {
                    continue;
                }
            }

            let target_elev =
                dem.get_pixel(x, y).map_err(AlgorithmError::Core)? + config.target_height;

            let (is_vis, angle) = check_line_of_sight(
                dem,
                config.observer_x,
                config.observer_y,
                observer_elev,
                x,
                y,
                target_elev,
                config.cell_size,
                &config.curvature_correction,
            )?;

            if is_vis {
                visibility
                    .set_pixel(x, y, 1.0)
                    .map_err(AlgorithmError::Core)?;
                elev_angle
                    .set_pixel(x, y, angle)
                    .map_err(AlgorithmError::Core)?;
            }
        }
    }

    Ok(ViewshedResult {
        visibility,
        elevation_angle: Some(elev_angle),
    })
}

/// Checks line of sight between two points with curvature correction
fn check_line_of_sight(
    dem: &RasterBuffer,
    x0: u64,
    y0: u64,
    elev0: f64,
    x1: u64,
    y1: u64,
    elev1: f64,
    cell_size: f64,
    curvature: &Option<CurvatureCorrection>,
) -> Result<(bool, f64)> {
    let dx = (x1 as f64 - x0 as f64) * cell_size;
    let dy = (y1 as f64 - y0 as f64) * cell_size;
    let total_distance = (dx * dx + dy * dy).sqrt();

    if total_distance < 1e-10 {
        return Ok((true, 0.0));
    }

    // Angle from observer to target
    let target_angle = (elev1 - elev0).atan2(total_distance);

    let steps = ((total_distance / cell_size) * 2.0).max(2.0) as usize;
    let mut max_angle = f64::NEG_INFINITY;

    for i in 1..steps {
        let t = i as f64 / steps as f64;
        let px = x0 as f64 + (x1 as f64 - x0 as f64) * t;
        let py = y0 as f64 + (y1 as f64 - y0 as f64) * t;

        let ix = px.round() as u64;
        let iy = py.round() as u64;

        if ix >= dem.width() || iy >= dem.height() {
            continue;
        }

        let mut terrain_elev = dem.get_pixel(ix, iy).map_err(AlgorithmError::Core)?;
        let curr_dist = t * total_distance;

        // Apply curvature correction
        if let Some(curv) = curvature {
            terrain_elev -= earth_curvature_offset(curr_dist, curv);
        }

        let angle = (terrain_elev - elev0).atan2(curr_dist);

        if angle > max_angle {
            max_angle = angle;
        }

        if angle > target_angle + 1e-9 {
            return Ok((false, 0.0));
        }
    }

    Ok((true, target_angle))
}

// ===========================================================================
// R2: Reference plane / radial sweep
// ===========================================================================

/// R2 algorithm: sweep radial lines outward from observer
///
/// For each discrete angle from the observer, trace a ray outward through
/// the grid cells. Maintain the maximum elevation angle seen so far. A cell
/// is visible if its elevation angle exceeds the current maximum.
fn viewshed_r2(dem: &RasterBuffer, config: &ViewshedConfig) -> Result<ViewshedResult> {
    let width = dem.width();
    let height = dem.height();
    let mut visibility = RasterBuffer::zeros(width, height, RasterDataType::UInt8);

    let observer_elev = dem
        .get_pixel(config.observer_x, config.observer_y)
        .map_err(AlgorithmError::Core)?
        + config.observer_height;

    // Mark observer as visible
    visibility
        .set_pixel(config.observer_x, config.observer_y, 1.0)
        .map_err(AlgorithmError::Core)?;

    // Determine the maximum extent in cells
    let max_cells = if let Some(max_dist) = config.max_distance {
        (max_dist / config.cell_size).ceil() as i64
    } else {
        (width.max(height)) as i64
    };

    // Number of radial lines: use perimeter of bounding circle
    let num_rays = (2.0 * core::f64::consts::PI * max_cells as f64).ceil() as usize;
    let num_rays = num_rays.max(360);

    for ray_idx in 0..num_rays {
        let angle = 2.0 * core::f64::consts::PI * ray_idx as f64 / num_rays as f64;
        let cos_a = angle.cos();
        let sin_a = angle.sin();

        let mut max_elev_angle = f64::NEG_INFINITY;

        // Step outward along the ray
        for step in 1..=max_cells {
            let fx = config.observer_x as f64 + step as f64 * cos_a;
            let fy = config.observer_y as f64 + step as f64 * sin_a;

            let ix = fx.round() as i64;
            let iy = fy.round() as i64;

            if ix < 0 || ix >= width as i64 || iy < 0 || iy >= height as i64 {
                break;
            }

            let ix_u = ix as u64;
            let iy_u = iy as u64;

            let dist = step as f64 * config.cell_size;

            if let Some(max_dist) = config.max_distance {
                if dist > max_dist {
                    break;
                }
            }

            let mut terrain_elev =
                dem.get_pixel(ix_u, iy_u).map_err(AlgorithmError::Core)? + config.target_height;

            // Curvature correction
            if let Some(ref curv) = config.curvature_correction {
                terrain_elev -= earth_curvature_offset(dist, curv);
            }

            let elev_angle = (terrain_elev - observer_elev).atan2(dist);

            if elev_angle >= max_elev_angle {
                visibility
                    .set_pixel(ix_u, iy_u, 1.0)
                    .map_err(AlgorithmError::Core)?;
                max_elev_angle = elev_angle;
            }
        }
    }

    Ok(ViewshedResult {
        visibility,
        elevation_angle: None,
    })
}

// ===========================================================================
// R3: Sweep line with angular sorting
// ===========================================================================

/// R3 algorithm: angular sweep line approach
///
/// Processes cells in order of angle from the observer. For cells at the same
/// angle, processes nearer cells first. Uses maximum angle tracking similar to
/// R2 but processes in angular order rather than along fixed rays.
fn viewshed_r3(dem: &RasterBuffer, config: &ViewshedConfig) -> Result<ViewshedResult> {
    let width = dem.width();
    let height = dem.height();
    let mut visibility = RasterBuffer::zeros(width, height, RasterDataType::UInt8);

    let observer_elev = dem
        .get_pixel(config.observer_x, config.observer_y)
        .map_err(AlgorithmError::Core)?
        + config.observer_height;

    visibility
        .set_pixel(config.observer_x, config.observer_y, 1.0)
        .map_err(AlgorithmError::Core)?;

    // Build a list of all cells with their angle and distance from observer
    let mut cells: Vec<(u64, u64, f64, f64)> = Vec::new(); // (x, y, angle, distance)

    let max_dist_sq = config.max_distance.map(|d| d * d);

    for y in 0..height {
        for x in 0..width {
            if x == config.observer_x && y == config.observer_y {
                continue;
            }

            let dx = (x as f64 - config.observer_x as f64) * config.cell_size;
            let dy = (y as f64 - config.observer_y as f64) * config.cell_size;
            let dist_sq = dx * dx + dy * dy;

            if let Some(max_sq) = max_dist_sq {
                if dist_sq > max_sq {
                    continue;
                }
            }

            let angle = dy.atan2(dx);
            let distance = dist_sq.sqrt();
            cells.push((x, y, angle, distance));
        }
    }

    // Sort by angle, then by distance (nearest first)
    cells.sort_by(|a, b| {
        a.2.partial_cmp(&b.2)
            .unwrap_or(core::cmp::Ordering::Equal)
            .then_with(|| a.3.partial_cmp(&b.3).unwrap_or(core::cmp::Ordering::Equal))
    });

    // Process cells in angular sweep order
    // For each angular sector, maintain the maximum elevation angle
    let num_sectors = 3600usize; // 0.1 degree resolution
    let mut sector_max_angle = vec![f64::NEG_INFINITY; num_sectors];

    for (x, y, angle, distance) in &cells {
        let sector_idx = (((angle + core::f64::consts::PI) / (2.0 * core::f64::consts::PI)
            * num_sectors as f64)
            .floor() as usize)
            .min(num_sectors - 1);

        let mut terrain_elev =
            dem.get_pixel(*x, *y).map_err(AlgorithmError::Core)? + config.target_height;

        if let Some(ref curv) = config.curvature_correction {
            terrain_elev -= earth_curvature_offset(*distance, curv);
        }

        let elev_angle = (terrain_elev - observer_elev).atan2(*distance);

        if elev_angle >= sector_max_angle[sector_idx] {
            visibility
                .set_pixel(*x, *y, 1.0)
                .map_err(AlgorithmError::Core)?;
            sector_max_angle[sector_idx] = elev_angle;
        }
    }

    Ok(ViewshedResult {
        visibility,
        elevation_angle: None,
    })
}

// ===========================================================================
// Earth curvature correction
// ===========================================================================

/// Computes the elevation offset due to Earth curvature and atmospheric refraction
///
/// offset = distance^2 / (2 * R_eff)
///
/// where R_eff = R / (1 - k), R = earth radius, k = refraction coefficient
fn earth_curvature_offset(distance: f64, correction: &CurvatureCorrection) -> f64 {
    let effective_radius = correction.earth_radius / (1.0 - correction.refraction_coefficient);
    (distance * distance) / (2.0 * effective_radius)
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn create_flat_dem(size: u64) -> RasterBuffer {
        RasterBuffer::zeros(size, size, RasterDataType::Float32)
    }

    fn create_hill_dem() -> RasterBuffer {
        let mut dem = RasterBuffer::zeros(20, 20, RasterDataType::Float32);
        // Create a hill at (10, 10)
        for y in 0..20 {
            for x in 0..20 {
                let dx = x as f64 - 10.0;
                let dy = y as f64 - 10.0;
                let dist = (dx * dx + dy * dy).sqrt();
                let elev = (5.0 - dist).max(0.0);
                let _ = dem.set_pixel(x, y, elev);
            }
        }
        dem
    }

    fn create_wall_dem() -> RasterBuffer {
        let mut dem = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        // Create a wall at y=6
        for x in 0..10 {
            let _ = dem.set_pixel(x, 6, 50.0);
        }
        dem
    }

    // --- Basic viewshed tests ---

    #[test]
    fn test_viewshed_flat_terrain_r1() {
        let dem = create_flat_dem(10);
        let result = compute_viewshed(&dem, 5, 5, 10.0, 0.0, None, 1.0);
        assert!(result.is_ok());
        let viewshed = result.expect("viewshed");

        // All cells should be visible on flat terrain with elevated observer
        for y in 0..10 {
            for x in 0..10 {
                let val = viewshed.get_pixel(x, y).expect("pixel");
                assert!(
                    val > 0.0,
                    "Cell ({x},{y}) should be visible on flat terrain"
                );
            }
        }
    }

    #[test]
    fn test_viewshed_flat_terrain_r2() {
        let dem = create_flat_dem(10);
        let config = ViewshedConfig {
            observer_x: 5,
            observer_y: 5,
            observer_height: 10.0,
            target_height: 0.0,
            max_distance: None,
            cell_size: 1.0,
            algorithm: ViewshedAlgorithm::R2ReferencePlane,
            curvature_correction: None,
        };
        let result = compute_viewshed_advanced(&dem, &config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_viewshed_flat_terrain_r3() {
        let dem = create_flat_dem(10);
        let config = ViewshedConfig {
            observer_x: 5,
            observer_y: 5,
            observer_height: 10.0,
            target_height: 0.0,
            max_distance: None,
            cell_size: 1.0,
            algorithm: ViewshedAlgorithm::R3SweepLine,
            curvature_correction: None,
        };
        let result = compute_viewshed_advanced(&dem, &config);
        assert!(result.is_ok());
    }

    // --- Obstacle tests ---

    #[test]
    fn test_viewshed_with_wall() {
        let dem = create_wall_dem();
        let viewshed = compute_viewshed(&dem, 5, 5, 1.0, 0.0, None, 1.0).expect("viewshed");

        // Observer should be visible
        let obs = viewshed.get_pixel(5, 5).expect("obs");
        assert!(obs > 0.0);

        // Cells behind the wall should be hidden
        let behind = viewshed.get_pixel(5, 8).expect("behind");
        assert!(
            behind < 0.5,
            "Cell behind wall should not be visible, got {behind}"
        );
    }

    // --- Distance limit ---

    #[test]
    fn test_viewshed_max_distance() {
        let dem = create_flat_dem(20);
        let viewshed = compute_viewshed(&dem, 10, 10, 10.0, 0.0, Some(5.0), 1.0).expect("viewshed");

        // Cells within radius should be visible
        let near = viewshed.get_pixel(10, 12).expect("near");
        assert!(near > 0.0);

        // Cells beyond radius should not be visible
        let far = viewshed.get_pixel(0, 0).expect("far");
        assert!(
            far < 0.5,
            "Cell beyond max distance should not be visible, got {far}"
        );
    }

    // --- Earth curvature ---

    #[test]
    fn test_earth_curvature_offset() {
        let correction = CurvatureCorrection::default();

        // At 1km, the offset should be very small
        let offset_1km = earth_curvature_offset(1000.0, &correction);
        assert!(offset_1km > 0.0 && offset_1km < 0.1);

        // At 10km, offset should be larger
        let offset_10km = earth_curvature_offset(10_000.0, &correction);
        assert!(offset_10km > offset_1km);

        // Approximately: offset ~= d^2 / (2*R) for small d
        // At 10km: ~10000^2 / (2*6371000) ~= 7.85m (without refraction)
        // With refraction (k=0.13): effective R = 6371000/0.87 = 7322988
        // offset ~= 10000^2 / (2*7322988) ~= 6.83m
        assert!(offset_10km > 5.0 && offset_10km < 10.0);
    }

    #[test]
    fn test_viewshed_with_curvature() {
        let dem = create_flat_dem(10);
        let config = ViewshedConfig {
            observer_x: 5,
            observer_y: 5,
            observer_height: 10.0,
            target_height: 0.0,
            max_distance: None,
            cell_size: 1.0,
            algorithm: ViewshedAlgorithm::R1LineOfSight,
            curvature_correction: Some(CurvatureCorrection::default()),
        };
        let result = compute_viewshed_advanced(&dem, &config);
        assert!(result.is_ok());
    }

    // --- Cumulative viewshed ---

    #[test]
    fn test_cumulative_viewshed() {
        let dem = create_flat_dem(10);
        let observers = vec![(2, 2, 10.0), (7, 7, 10.0)];

        let cumulative =
            compute_cumulative_viewshed(&dem, &observers, 0.0, None, 1.0).expect("cumulative");

        let center = cumulative.get_pixel(5, 5).expect("center");
        assert!(
            center >= 2.0,
            "Center should be visible from both observers"
        );
    }

    #[test]
    fn test_cumulative_viewshed_advanced() {
        let dem = create_flat_dem(10);
        let observers = vec![
            ObserverPoint {
                x: 2,
                y: 2,
                height: 10.0,
            },
            ObserverPoint {
                x: 7,
                y: 7,
                height: 10.0,
            },
        ];

        let result = compute_cumulative_viewshed_advanced(
            &dem,
            &observers,
            0.0,
            None,
            1.0,
            Some(CurvatureCorrection::default()),
        );
        assert!(result.is_ok());
    }

    // --- LOS profile ---

    #[test]
    fn test_los_profile() {
        let dem = create_flat_dem(10);
        let profile = compute_los_profile(&dem, 0, 0, 10.0, 9, 9, 1.0, None);
        assert!(profile.is_ok());
        let prof = profile.expect("profile");
        assert!(prof.len() >= 2);

        // First point should be at distance 0
        assert!(prof[0].0.abs() < 1e-6);
    }

    #[test]
    fn test_los_profile_with_curvature() {
        let dem = create_flat_dem(10);
        let profile = compute_los_profile(
            &dem,
            0,
            0,
            10.0,
            9,
            9,
            1.0,
            Some(CurvatureCorrection::default()),
        );
        assert!(profile.is_ok());
    }

    // --- Fresnel zone ---

    #[test]
    fn test_fresnel_clearance_flat() {
        let dem = create_flat_dem(10);
        let clearance = compute_fresnel_clearance(&dem, 0, 0, 10.0, 9, 9, 10.0, 1.0, 2.4, None);
        assert!(clearance.is_ok());
        let c = clearance.expect("clearance");
        // Both antennas at 10m on flat terrain should have good clearance
        assert!(c > 0.0, "Fresnel clearance should be positive, got {c}");
    }

    #[test]
    fn test_fresnel_clearance_with_wall() {
        let dem = create_wall_dem();
        let clearance = compute_fresnel_clearance(&dem, 5, 3, 2.0, 5, 9, 2.0, 1.0, 2.4, None);
        assert!(clearance.is_ok());
        let c = clearance.expect("clearance");
        // Wall at y=6 should block the path
        assert!(
            c < 0.0,
            "Fresnel clearance should be negative with wall, got {c}"
        );
    }

    // --- Invalid inputs ---

    #[test]
    fn test_viewshed_invalid_observer() {
        let dem = create_flat_dem(10);
        let config = ViewshedConfig {
            observer_x: 100,
            observer_y: 100,
            observer_height: 10.0,
            target_height: 0.0,
            max_distance: None,
            cell_size: 1.0,
            algorithm: ViewshedAlgorithm::R1LineOfSight,
            curvature_correction: None,
        };
        let result = compute_viewshed_advanced(&dem, &config);
        assert!(result.is_err());
    }

    // --- ViewshedResult elevation angle ---

    #[test]
    fn test_viewshed_elevation_angle() {
        let dem = create_flat_dem(10);
        let config = ViewshedConfig {
            observer_x: 5,
            observer_y: 5,
            observer_height: 10.0,
            target_height: 0.0,
            max_distance: None,
            cell_size: 1.0,
            algorithm: ViewshedAlgorithm::R1LineOfSight,
            curvature_correction: None,
        };
        let result = compute_viewshed_advanced(&dem, &config).expect("result");
        assert!(result.elevation_angle.is_some());

        let elev_angle = result.elevation_angle.expect("elev_angle");
        let angle = elev_angle.get_pixel(5, 8).expect("angle");
        // Looking down from 10m height to ground 3 cells away
        // angle = atan(-10/3) ~= -1.28 rad (negative = looking down)
        assert!(
            angle < 0.0,
            "Elevation angle should be negative (looking down), got {angle}"
        );
    }
}
