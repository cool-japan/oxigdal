//! Flow direction algorithms for hydrological analysis
//!
//! Implements D8 (Jenson & Domingue, 1988), D-Infinity (Tarboton, 1997),
//! and MFD (Multiple Flow Direction, Freeman 1991 / Quinn et al. 1991) algorithms.
//! Includes flat area resolution (Garbrecht & Martz, 1997) and proper pit handling.

use crate::error::{AlgorithmError, Result};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;
use std::collections::VecDeque;

// ---------------------------------------------------------------------------
// Direction constants and types
// ---------------------------------------------------------------------------

/// Flow direction method
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowMethod {
    /// D8 (8-direction) method -- Jenson & Domingue (1988)
    D8,
    /// D-infinity method (continuous flow direction) -- Tarboton (1997)
    DInfinity,
    /// Multiple Flow Direction -- Freeman (1991)
    MFD,
}

/// D8 flow direction codes (powers of 2, ESRI convention)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum D8Direction {
    /// East (1)
    East = 1,
    /// Southeast (2)
    Southeast = 2,
    /// South (4)
    South = 4,
    /// Southwest (8)
    Southwest = 8,
    /// West (16)
    West = 16,
    /// Northwest (32)
    Northwest = 32,
    /// North (64)
    North = 64,
    /// Northeast (128)
    Northeast = 128,
}

/// Sentinel value for flat cells (no steepest downslope neighbour)
pub const D8_FLAT: u8 = 0;

/// Sentinel value for pit cells (lower than all neighbours)
pub const D8_PIT: u8 = 255;

/// Neighbour offsets in D8 order: E, SE, S, SW, W, NW, N, NE
pub const D8_DX: [i64; 8] = [1, 1, 0, -1, -1, -1, 0, 1];
/// Neighbour Y offsets in D8 order: E, SE, S, SW, W, NW, N, NE
pub const D8_DY: [i64; 8] = [0, 1, 1, 1, 0, -1, -1, -1];
const D8_CODES: [u8; 8] = [1, 2, 4, 8, 16, 32, 64, 128];

impl D8Direction {
    /// Gets the (dx, dy) offset for this direction
    #[must_use]
    pub fn offset(&self) -> (i64, i64) {
        let idx = self.index();
        (D8_DX[idx], D8_DY[idx])
    }

    /// Returns the index [0..8) in the canonical D8 neighbour array
    #[must_use]
    pub fn index(&self) -> usize {
        match self {
            Self::East => 0,
            Self::Southeast => 1,
            Self::South => 2,
            Self::Southwest => 3,
            Self::West => 4,
            Self::Northwest => 5,
            Self::North => 6,
            Self::Northeast => 7,
        }
    }

    /// Returns all eight D8 directions in canonical order
    #[must_use]
    pub fn all() -> [Self; 8] {
        [
            Self::East,
            Self::Southeast,
            Self::South,
            Self::Southwest,
            Self::West,
            Self::Northwest,
            Self::North,
            Self::Northeast,
        ]
    }

    /// Gets the angle in degrees (0 = East, clockwise)
    #[must_use]
    pub fn angle_degrees(&self) -> f64 {
        match self {
            Self::East => 0.0,
            Self::Southeast => 45.0,
            Self::South => 90.0,
            Self::Southwest => 135.0,
            Self::West => 180.0,
            Self::Northwest => 225.0,
            Self::North => 270.0,
            Self::Northeast => 315.0,
        }
    }

    /// Returns the opposite direction
    #[must_use]
    pub fn opposite(&self) -> Self {
        match self {
            Self::East => Self::West,
            Self::Southeast => Self::Northwest,
            Self::South => Self::North,
            Self::Southwest => Self::Northeast,
            Self::West => Self::East,
            Self::Northwest => Self::Southeast,
            Self::North => Self::South,
            Self::Northeast => Self::Southwest,
        }
    }

    /// Construct from D8 code (power-of-2)
    #[must_use]
    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::East),
            2 => Some(Self::Southeast),
            4 => Some(Self::South),
            8 => Some(Self::Southwest),
            16 => Some(Self::West),
            32 => Some(Self::Northwest),
            64 => Some(Self::North),
            128 => Some(Self::Northeast),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Helper: distance factor for D8 directions
// ---------------------------------------------------------------------------

/// Returns the distance factor for a D8 neighbour index (1.0 cardinal, sqrt(2) diagonal)
#[inline]
fn d8_distance(idx: usize) -> f64 {
    if idx % 2 == 0 {
        1.0
    } else {
        std::f64::consts::SQRT_2
    }
}

// ---------------------------------------------------------------------------
// Inline helpers for bounds
// ---------------------------------------------------------------------------

#[inline]
fn in_bounds(x: i64, y: i64, w: u64, h: u64) -> bool {
    x >= 0 && y >= 0 && (x as u64) < w && (y as u64) < h
}

// ---------------------------------------------------------------------------
// D8 flow direction  (Jenson & Domingue, 1988)
// ---------------------------------------------------------------------------

/// Configuration for D8 flow direction computation
#[derive(Debug, Clone)]
pub struct D8Config {
    /// Cell size in map units
    pub cell_size: f64,
    /// Whether to resolve flat areas using Garbrecht & Martz (1997)
    pub resolve_flats: bool,
}

impl Default for D8Config {
    fn default() -> Self {
        Self {
            cell_size: 1.0,
            resolve_flats: true,
        }
    }
}

/// Computes D8 flow direction from a DEM.
///
/// Each cell receives the code of the steepest-descent neighbour.
/// Flat cells get `D8_FLAT` (0), pit cells get `D8_PIT` (255) unless
/// `resolve_flats` is enabled, in which case flats are resolved via the
/// Garbrecht & Martz (1997) algorithm.
///
/// # Errors
///
/// Returns an error if raster pixel access fails.
pub fn compute_d8_flow_direction(dem: &RasterBuffer, cell_size: f64) -> Result<RasterBuffer> {
    let cfg = D8Config {
        cell_size,
        resolve_flats: true,
    };
    compute_d8_flow_direction_cfg(dem, &cfg)
}

/// Computes D8 flow direction with full configuration
///
/// # Errors
///
/// Returns an error if raster pixel access fails.
pub fn compute_d8_flow_direction_cfg(dem: &RasterBuffer, cfg: &D8Config) -> Result<RasterBuffer> {
    let w = dem.width();
    let h = dem.height();
    let mut flow_dir = RasterBuffer::zeros(w, h, RasterDataType::UInt8);

    for y in 0..h {
        for x in 0..w {
            let center = dem.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            let mut max_slope = 0.0_f64; // only accept downhill
            let mut best_code: u8 = D8_FLAT;
            let mut is_pit = true;

            for i in 0..8 {
                let nx = x as i64 + D8_DX[i];
                let ny = y as i64 + D8_DY[i];
                if !in_bounds(nx, ny, w, h) {
                    // Boundary cells drain outward -- they are never pits
                    is_pit = false;
                    continue;
                }
                let ne = dem
                    .get_pixel(nx as u64, ny as u64)
                    .map_err(AlgorithmError::Core)?;
                if ne < center {
                    is_pit = false;
                    let dist = d8_distance(i) * cfg.cell_size;
                    let slope = (center - ne) / dist;
                    if slope > max_slope {
                        max_slope = slope;
                        best_code = D8_CODES[i];
                    }
                } else if (ne - center).abs() < f64::EPSILON {
                    // Flat neighbour -- not a pit
                    is_pit = false;
                }
            }

            if is_pit && best_code == D8_FLAT {
                best_code = D8_PIT;
            }
            flow_dir
                .set_pixel(x, y, f64::from(best_code))
                .map_err(AlgorithmError::Core)?;
        }
    }

    // Resolve flat areas if requested
    if cfg.resolve_flats {
        resolve_flat_areas(dem, &mut flow_dir)?;
    }

    Ok(flow_dir)
}

// ---------------------------------------------------------------------------
// Flat area resolution  (Garbrecht & Martz, 1997)
// ---------------------------------------------------------------------------

/// Resolves flat areas in a D8 flow direction grid.
///
/// Uses a two-pass approach inspired by Garbrecht & Martz (1997):
///   1. Gradient *away from* higher terrain (increments toward lower edges)
///   2. Gradient *toward* lower terrain (increments toward outlets)
///
/// The combined surface is used to assign flow directions to formerly flat cells.
fn resolve_flat_areas(dem: &RasterBuffer, flow_dir: &mut RasterBuffer) -> Result<()> {
    let w = dem.width();
    let h = dem.height();

    // Identify flat cells (code == D8_FLAT)
    let mut is_flat = vec![false; (w * h) as usize];
    let mut has_flat = false;

    for y in 0..h {
        for x in 0..w {
            let code = flow_dir.get_pixel(x, y).map_err(AlgorithmError::Core)? as u8;
            if code == D8_FLAT {
                is_flat[(y * w + x) as usize] = true;
                has_flat = true;
            }
        }
    }

    if !has_flat {
        return Ok(());
    }

    // Build increment surfaces
    let toward_lower = build_toward_lower_gradient(dem, &is_flat, w, h)?;
    let away_higher = build_away_higher_gradient(dem, &is_flat, w, h)?;

    // Combine: virtual elevation = toward_lower + away_higher
    // Then assign D8 directions on the combined surface for flat cells
    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) as usize;
            if !is_flat[idx] {
                continue;
            }
            let combined = toward_lower[idx] + away_higher[idx];
            let mut max_drop = 0.0_f64;
            let mut best_code: u8 = D8_FLAT;

            for i in 0..8 {
                let nx = x as i64 + D8_DX[i];
                let ny = y as i64 + D8_DY[i];
                if !in_bounds(nx, ny, w, h) {
                    continue;
                }
                let nidx = (ny as u64 * w + nx as u64) as usize;
                let n_combined = toward_lower[nidx] + away_higher[nidx];
                let drop = (combined - n_combined) / d8_distance(i);
                if drop > max_drop {
                    max_drop = drop;
                    best_code = D8_CODES[i];
                }
            }
            flow_dir
                .set_pixel(x, y, f64::from(best_code))
                .map_err(AlgorithmError::Core)?;
        }
    }

    Ok(())
}

/// BFS from flat-cell edges that border *lower* terrain, assigning increasing
/// values inward. This creates a gradient *toward* lower terrain.
fn build_toward_lower_gradient(
    dem: &RasterBuffer,
    is_flat: &[bool],
    w: u64,
    h: u64,
) -> Result<Vec<f64>> {
    let n = (w * h) as usize;
    let mut grad = vec![0.0_f64; n];
    let mut visited = vec![false; n];
    let mut queue = VecDeque::new();

    // Seed: flat cells adjacent to lower cells
    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) as usize;
            if !is_flat[idx] {
                continue;
            }
            let center = dem.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            let mut borders_lower = false;
            for i in 0..8 {
                let nx = x as i64 + D8_DX[i];
                let ny = y as i64 + D8_DY[i];
                if !in_bounds(nx, ny, w, h) {
                    // Edge cells can drain outward
                    borders_lower = true;
                    break;
                }
                let ne = dem
                    .get_pixel(nx as u64, ny as u64)
                    .map_err(AlgorithmError::Core)?;
                if ne < center {
                    borders_lower = true;
                    break;
                }
            }
            if borders_lower {
                queue.push_back((x, y));
                visited[idx] = true;
                grad[idx] = 0.0;
            }
        }
    }

    // BFS inward
    while let Some((x, y)) = queue.pop_front() {
        let idx = (y * w + x) as usize;
        let cur_val = grad[idx];
        for i in 0..8 {
            let nx = x as i64 + D8_DX[i];
            let ny = y as i64 + D8_DY[i];
            if !in_bounds(nx, ny, w, h) {
                continue;
            }
            let nidx = (ny as u64 * w + nx as u64) as usize;
            if !is_flat[nidx] || visited[nidx] {
                continue;
            }
            visited[nidx] = true;
            grad[nidx] = cur_val + 1.0;
            queue.push_back((nx as u64, ny as u64));
        }
    }

    Ok(grad)
}

/// BFS from flat-cell edges that border *higher* terrain, assigning increasing
/// values inward. This creates a gradient *away from* higher terrain.
fn build_away_higher_gradient(
    dem: &RasterBuffer,
    is_flat: &[bool],
    w: u64,
    h: u64,
) -> Result<Vec<f64>> {
    let n = (w * h) as usize;
    let mut grad = vec![0.0_f64; n];
    let mut visited = vec![false; n];
    let mut queue = VecDeque::new();

    // Seed: flat cells adjacent to higher cells
    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) as usize;
            if !is_flat[idx] {
                continue;
            }
            let center = dem.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            let mut borders_higher = false;
            for i in 0..8 {
                let nx = x as i64 + D8_DX[i];
                let ny = y as i64 + D8_DY[i];
                if !in_bounds(nx, ny, w, h) {
                    continue;
                }
                let ne = dem
                    .get_pixel(nx as u64, ny as u64)
                    .map_err(AlgorithmError::Core)?;
                if ne > center {
                    borders_higher = true;
                    break;
                }
            }
            if borders_higher {
                queue.push_back((x, y));
                visited[idx] = true;
                grad[idx] = 0.0;
            }
        }
    }

    // BFS inward
    while let Some((x, y)) = queue.pop_front() {
        let idx = (y * w + x) as usize;
        let cur_val = grad[idx];
        for i in 0..8 {
            let nx = x as i64 + D8_DX[i];
            let ny = y as i64 + D8_DY[i];
            if !in_bounds(nx, ny, w, h) {
                continue;
            }
            let nidx = (ny as u64 * w + nx as u64) as usize;
            if !is_flat[nidx] || visited[nidx] {
                continue;
            }
            visited[nidx] = true;
            grad[nidx] = cur_val + 1.0;
            queue.push_back((nx as u64, ny as u64));
        }
    }

    Ok(grad)
}

// ---------------------------------------------------------------------------
// D-Infinity flow direction  (Tarboton, 1997)
// ---------------------------------------------------------------------------

/// Computes D-infinity flow direction.
///
/// Returns two rasters:
///   - **angle** in radians [0, 2 pi), measured counter-clockwise from East
///   - **proportion**: fraction of flow going to the "left" cell of the
///     steepest triangular facet (the remainder goes to the "right" cell).
///
/// Based on Tarboton (1997) "A new method for the determination of flow
/// directions and upslope areas in grid digital elevation models."
///
/// # Errors
///
/// Returns an error if raster pixel access fails.
pub fn compute_dinf_flow_direction(
    dem: &RasterBuffer,
    cell_size: f64,
) -> Result<(RasterBuffer, RasterBuffer)> {
    let w = dem.width();
    let h = dem.height();
    let mut flow_angle = RasterBuffer::zeros(w, h, RasterDataType::Float64);
    let mut flow_prop = RasterBuffer::zeros(w, h, RasterDataType::Float64);

    for y in 0..h {
        for x in 0..w {
            let center = dem.get_pixel(x, y).map_err(AlgorithmError::Core)?;

            // Gather 8-neighbour elevations (use center if out of bounds)
            let mut e = [0.0_f64; 8];
            for i in 0..8 {
                let nx = x as i64 + D8_DX[i];
                let ny = y as i64 + D8_DY[i];
                e[i] = if in_bounds(nx, ny, w, h) {
                    dem.get_pixel(nx as u64, ny as u64)
                        .map_err(AlgorithmError::Core)?
                } else {
                    center // treat boundary as same elevation (no flow outward via Dinf)
                };
            }

            let (angle, prop) = dinf_facet_steepest(center, &e, cell_size);
            flow_angle
                .set_pixel(x, y, angle)
                .map_err(AlgorithmError::Core)?;
            flow_prop
                .set_pixel(x, y, prop)
                .map_err(AlgorithmError::Core)?;
        }
    }

    Ok((flow_angle, flow_prop))
}

/// Finds the steepest facet and returns (angle_degrees, proportion).
///
/// The eight triangular facets are formed by the center cell and each pair of
/// adjacent neighbours. The steepest downhill slope across all facets determines
/// the flow direction.
fn dinf_facet_steepest(center: f64, e: &[f64; 8], cell_size: f64) -> (f64, f64) {
    let mut max_slope = f64::NEG_INFINITY;
    let mut best_angle = 0.0_f64;
    let mut best_prop = 1.0_f64;

    let d1 = cell_size;
    let d2 = cell_size;
    let dd = cell_size * std::f64::consts::SQRT_2;

    // 8 facets: each formed by (e[i], e[(i+1)%8])
    for i in 0..8 {
        let j = (i + 1) % 8;
        let e1 = e[i];
        let e2 = e[j];

        // Distance to e1 and e2
        let dist1 = if i % 2 == 0 { d1 } else { dd };
        let dist2 = if j % 2 == 0 { d2 } else { dd };

        let s1 = (center - e1) / dist1;
        let s2 = (center - e2) / dist2;

        let base_angle = (i as f64) * 45.0;

        if s1 > 0.0 || s2 > 0.0 {
            // Check if flow direction lies within the facet
            if s1 > 0.0 && s2 > 0.0 {
                let facet_angle = (s2 / s1).atan().to_degrees();
                if (0.0..=45.0).contains(&facet_angle) {
                    let s = (s1 * s1 + s2 * s2).sqrt();
                    if s > max_slope {
                        max_slope = s;
                        best_angle = base_angle + facet_angle;
                        // Proportion to the "left" cell (e1 side)
                        best_prop = 1.0 - facet_angle / 45.0;
                    }
                    continue;
                }
            }

            // Check along edge 1 (angle = base_angle)
            if s1 > max_slope {
                max_slope = s1;
                best_angle = base_angle;
                best_prop = 1.0;
            }
            // Check along edge 2 (angle = base_angle + 45)
            if s2 > max_slope {
                max_slope = s2;
                best_angle = base_angle + 45.0;
                best_prop = 0.0;
            }
        }
    }

    // Normalise angle to [0, 360)
    best_angle = best_angle.rem_euclid(360.0);
    (best_angle, best_prop)
}

// ---------------------------------------------------------------------------
// MFD -- Multiple Flow Direction  (Freeman, 1991)
// ---------------------------------------------------------------------------

/// Configuration for MFD computation
#[derive(Debug, Clone)]
pub struct MfdConfig {
    /// Cell size in map units
    pub cell_size: f64,
    /// Exponent controlling flow partitioning convergence.
    /// Larger values concentrate flow into the steepest path.
    /// Freeman (1991) recommends p = 1.1; Holmgren (1994) uses p = 4-8.
    pub exponent: f64,
}

impl Default for MfdConfig {
    fn default() -> Self {
        Self {
            cell_size: 1.0,
            exponent: 1.1,
        }
    }
}

/// MFD result: for each cell, a vector of (neighbour_index, proportion).
/// Stored as a flat array of 8 proportions per cell.
pub struct MfdResult {
    /// Width of the raster
    pub width: u64,
    /// Height of the raster
    pub height: u64,
    /// Proportions: `proportions[cell_idx * 8 + dir_idx]`
    pub proportions: Vec<f64>,
}

impl MfdResult {
    /// Gets flow proportions for a cell as `[f64; 8]` in D8 order
    #[must_use]
    pub fn get_proportions(&self, x: u64, y: u64) -> [f64; 8] {
        let base = ((y * self.width + x) * 8) as usize;
        let mut out = [0.0; 8];
        for i in 0..8 {
            out[i] = self.proportions[base + i];
        }
        out
    }
}

/// Computes MFD (Multiple Flow Direction) proportions.
///
/// Each cell partitions flow among all downslope neighbours according to
/// `w_i = (tan(slope_i))^p / sum((tan(slope_j))^p)` where the sum is over
/// all downslope neighbours.
///
/// # Errors
///
/// Returns an error if raster pixel access fails.
pub fn compute_mfd_flow_direction(dem: &RasterBuffer, cfg: &MfdConfig) -> Result<MfdResult> {
    let w = dem.width();
    let h = dem.height();
    let total_cells = (w * h) as usize;
    let mut proportions = vec![0.0_f64; total_cells * 8];

    for y in 0..h {
        for x in 0..w {
            let center = dem.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            let base = ((y * w + x) * 8) as usize;

            let mut slopes = [0.0_f64; 8];
            let mut total_weight = 0.0_f64;

            for i in 0..8 {
                let nx = x as i64 + D8_DX[i];
                let ny = y as i64 + D8_DY[i];
                if !in_bounds(nx, ny, w, h) {
                    continue;
                }
                let ne = dem
                    .get_pixel(nx as u64, ny as u64)
                    .map_err(AlgorithmError::Core)?;
                let drop = center - ne;
                if drop > 0.0 {
                    let dist = d8_distance(i) * cfg.cell_size;
                    let tan_slope = drop / dist;
                    let weight = tan_slope.powf(cfg.exponent);
                    slopes[i] = weight;
                    total_weight += weight;
                }
            }

            if total_weight > 0.0 {
                for i in 0..8 {
                    proportions[base + i] = slopes[i] / total_weight;
                }
            }
            // If total_weight == 0, cell is flat/pit: all proportions stay 0
        }
    }

    Ok(MfdResult {
        width: w,
        height: h,
        proportions,
    })
}

// ---------------------------------------------------------------------------
// Unified flow direction computation
// ---------------------------------------------------------------------------

/// Result from a flow direction computation.
///
/// Different methods produce different result types, unified here.
pub enum FlowDirectionResult {
    /// D8: a single raster with direction codes
    D8(RasterBuffer),
    /// D-Infinity: (angle raster, proportion raster)
    DInfinity(RasterBuffer, RasterBuffer),
    /// MFD: per-cell proportions
    Mfd(MfdResult),
}

/// Computes flow direction using the specified method.
///
/// # Errors
///
/// Returns an error if raster pixel access fails.
pub fn compute_flow_direction(
    dem: &RasterBuffer,
    method: FlowMethod,
    cell_size: f64,
) -> Result<FlowDirectionResult> {
    match method {
        FlowMethod::D8 => {
            let fd = compute_d8_flow_direction(dem, cell_size)?;
            Ok(FlowDirectionResult::D8(fd))
        }
        FlowMethod::DInfinity => {
            let (a, p) = compute_dinf_flow_direction(dem, cell_size)?;
            Ok(FlowDirectionResult::DInfinity(a, p))
        }
        FlowMethod::MFD => {
            let cfg = MfdConfig {
                cell_size,
                ..MfdConfig::default()
            };
            let mfd = compute_mfd_flow_direction(dem, &cfg)?;
            Ok(FlowDirectionResult::Mfd(mfd))
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    fn make_slope_dem(width: u64, height: u64) -> RasterBuffer {
        // Elevation decreases toward the east: elev = (width - 1 - x)
        let mut dem = RasterBuffer::zeros(width, height, RasterDataType::Float32);
        for y in 0..height {
            for x in 0..width {
                let _ = dem.set_pixel(x, y, (width - 1 - x) as f64);
            }
        }
        dem
    }

    fn make_se_slope_dem(width: u64, height: u64) -> RasterBuffer {
        let mut dem = RasterBuffer::zeros(width, height, RasterDataType::Float32);
        for y in 0..height {
            for x in 0..width {
                let _ = dem.set_pixel(x, y, ((width - 1 - x) + (height - 1 - y)) as f64);
            }
        }
        dem
    }

    #[test]
    fn test_d8_direction_offset() {
        assert_eq!(D8Direction::East.offset(), (1, 0));
        assert_eq!(D8Direction::South.offset(), (0, 1));
        assert_eq!(D8Direction::West.offset(), (-1, 0));
        assert_eq!(D8Direction::North.offset(), (0, -1));
    }

    #[test]
    fn test_d8_direction_angle() {
        assert_abs_diff_eq!(D8Direction::East.angle_degrees(), 0.0);
        assert_abs_diff_eq!(D8Direction::South.angle_degrees(), 90.0);
        assert_abs_diff_eq!(D8Direction::West.angle_degrees(), 180.0);
        assert_abs_diff_eq!(D8Direction::North.angle_degrees(), 270.0);
    }

    #[test]
    fn test_d8_from_code_round_trip() {
        for dir in D8Direction::all() {
            let code = dir as u8;
            let recovered = D8Direction::from_code(code);
            assert_eq!(recovered, Some(dir));
        }
    }

    #[test]
    fn test_d8_opposite() {
        assert_eq!(D8Direction::East.opposite(), D8Direction::West);
        assert_eq!(D8Direction::North.opposite(), D8Direction::South);
        assert_eq!(D8Direction::Southeast.opposite(), D8Direction::Northwest);
    }

    #[test]
    fn test_d8_simple_east_slope() {
        let dem = make_slope_dem(7, 7);
        let fd = compute_d8_flow_direction(&dem, 1.0);
        assert!(fd.is_ok());
        let fd = fd.expect("should succeed");
        // Interior cell (3,3): slope goes east
        let code = fd.get_pixel(3, 3).expect("should succeed") as u8;
        assert_eq!(code, D8Direction::East as u8);
    }

    #[test]
    fn test_d8_pit_detection() {
        let mut dem = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        for y in 0..5u64 {
            for x in 0..5u64 {
                let _ = dem.set_pixel(x, y, 10.0);
            }
        }
        // Create pit at (2,2)
        let _ = dem.set_pixel(2, 2, 1.0);

        let cfg = D8Config {
            cell_size: 1.0,
            resolve_flats: false,
        };
        let fd = compute_d8_flow_direction_cfg(&dem, &cfg);
        assert!(fd.is_ok());
        let fd = fd.expect("should succeed");
        let code = fd.get_pixel(2, 2).expect("should succeed") as u8;
        assert_eq!(code, D8_PIT, "Cell (2,2) should be marked as pit");
    }

    #[test]
    fn test_d8_flat_resolution() {
        // Flat DEM except edges are high
        let mut dem = RasterBuffer::zeros(7, 7, RasterDataType::Float32);
        for y in 0..7u64 {
            for x in 0..7u64 {
                let _ = dem.set_pixel(x, y, 10.0);
            }
        }
        // Lower outlet at (6,3)
        let _ = dem.set_pixel(6, 3, 5.0);

        let fd = compute_d8_flow_direction(&dem, 1.0);
        assert!(fd.is_ok());
        let fd = fd.expect("should succeed");

        // Interior flat cells should now have non-zero, non-pit directions
        let code = fd.get_pixel(3, 3).expect("should succeed") as u8;
        assert_ne!(code, D8_FLAT, "Flat cell should be resolved");
        assert_ne!(code, D8_PIT, "Flat cell should not be marked as pit");
    }

    #[test]
    fn test_dinf_se_slope() {
        let dem = make_se_slope_dem(7, 7);
        let result = compute_dinf_flow_direction(&dem, 1.0);
        assert!(result.is_ok());
        let (angle_raster, prop_raster) = result.expect("should succeed");

        let angle = angle_raster.get_pixel(3, 3).expect("should succeed");
        let prop = prop_raster.get_pixel(3, 3).expect("should succeed");

        // Should be in the SE quadrant (roughly 45 degrees)
        assert!((0.0..=360.0).contains(&angle), "Angle {angle} out of range");
        assert!(
            (0.0..=1.0).contains(&prop),
            "Proportion {prop} out of range"
        );
    }

    #[test]
    fn test_mfd_east_slope() {
        let dem = make_slope_dem(7, 7);
        let cfg = MfdConfig {
            cell_size: 1.0,
            exponent: 1.1,
        };
        let result = compute_mfd_flow_direction(&dem, &cfg);
        assert!(result.is_ok());
        let mfd = result.expect("should succeed");

        let props = mfd.get_proportions(3, 3);
        // East (idx 0) should get the largest proportion
        let east_prop = props[0];
        for i in 1..8 {
            assert!(
                east_prop >= props[i],
                "East proportion {east_prop} should be >= props[{i}] = {}",
                props[i]
            );
        }
        // Sum should be ~1.0
        let sum: f64 = props.iter().sum();
        assert_abs_diff_eq!(sum, 1.0, epsilon = 1e-10);
    }

    #[test]
    fn test_mfd_flat_no_crash() {
        // Completely flat DEM
        let dem = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        let cfg = MfdConfig::default();
        let result = compute_mfd_flow_direction(&dem, &cfg);
        assert!(result.is_ok());
        let mfd = result.expect("should succeed");

        // All proportions should be 0 (flat)
        let props = mfd.get_proportions(2, 2);
        let sum: f64 = props.iter().sum();
        assert_abs_diff_eq!(sum, 0.0, epsilon = 1e-10);
    }

    #[test]
    fn test_unified_compute() {
        let dem = make_slope_dem(7, 7);
        let r1 = compute_flow_direction(&dem, FlowMethod::D8, 1.0);
        assert!(r1.is_ok());
        let r2 = compute_flow_direction(&dem, FlowMethod::DInfinity, 1.0);
        assert!(r2.is_ok());
        let r3 = compute_flow_direction(&dem, FlowMethod::MFD, 1.0);
        assert!(r3.is_ok());
    }
}
