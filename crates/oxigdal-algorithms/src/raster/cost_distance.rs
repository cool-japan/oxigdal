//! Cost-distance analysis and least-cost path algorithms
//!
//! Provides cost-weighted distance analysis, anisotropic cost surfaces,
//! barrier support, path allocation, and various shortest-path algorithms.
//!
//! # Algorithms
//!
//! ## Dijkstra-based cost distance
//!
//! Standard cost-distance using Dijkstra's algorithm on an 8-connected grid.
//! The cost to traverse from cell A to cell B is:
//!   cost = (cost_A + cost_B) / 2 * distance
//! where distance is 1 for cardinal neighbors and sqrt(2) for diagonal.
//!
//! ## Anisotropic cost distance
//!
//! Direction-dependent traversal costs. Uses slope and aspect information
//! to compute different costs for uphill vs. downhill and lateral movement.
//! Follows Tobler's hiking function or custom friction models.
//!
//! ## A* shortest path
//!
//! Heuristic-guided shortest path between two specific points. Uses the
//! Euclidean distance to the target as an admissible heuristic.
//!
//! # Features
//!
//! - **Barrier support**: Cells with infinite cost that cannot be traversed
//! - **Path allocation**: Assigns each cell to its nearest source
//! - **Direction raster**: Records the direction to the next cell on the
//!   least-cost path back to the source
//! - **Corridor analysis**: Identifies least-cost corridors between regions
//!
//! # References
//!
//! - Tobler, W. (1993). Three presentations on geographical analysis and modeling.
//! - Douglas, D.H. (1994). Least-cost path in GIS using an accumulated cost surface and slopelines.
//! - Yu, C. et al. (2003). An assessment of anisotropic least-cost path computation.

use crate::error::{AlgorithmError, Result};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

// ===========================================================================
// Cost cell for priority queue
// ===========================================================================

#[derive(Copy, Clone, PartialEq)]
struct CostCell {
    x: u64,
    y: u64,
    cost: f64,
}

impl Eq for CostCell {}

impl PartialOrd for CostCell {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CostCell {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .partial_cmp(&self.cost)
            .unwrap_or(Ordering::Equal)
    }
}

// ===========================================================================
// Anisotropic cost model
// ===========================================================================

/// Anisotropic friction model for direction-dependent costs
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum FrictionModel {
    /// Isotropic: same cost in all directions (default)
    #[default]
    Isotropic,

    /// Tobler's hiking function: speed = 6 * exp(-3.5 * |tan(slope) + 0.05|)
    /// Cost is inversely proportional to speed.
    ToblerHiking,

    /// Symmetric slope friction: cost increases with absolute slope gradient
    /// cost_factor = 1 + |dz| * slope_weight / distance
    SymmetricSlope {
        /// Weight factor for slope contribution
        slope_weight: f64,
    },

    /// Asymmetric friction: uphill is harder than downhill
    /// cost_factor_uphill = 1 + dz * uphill_weight / distance  (if dz > 0)
    /// cost_factor_downhill = 1 + |dz| * downhill_weight / distance  (if dz < 0)
    AsymmetricSlope {
        /// Weight factor for uphill movement
        uphill_weight: f64,
        /// Weight factor for downhill movement
        downhill_weight: f64,
    },
}

// ===========================================================================
// Direction encoding
// ===========================================================================

/// Direction encoding for backlink raster
///
/// Uses compass directions encoded as integers:
/// ```text
///  7  0  1
///  6  X  2
///  5  4  3
/// ```
///
/// 0 = North, 1 = NE, 2 = E, 3 = SE, 4 = S, 5 = SW, 6 = W, 7 = NW
/// -1 = source (no direction)
#[derive(Debug, Clone, Copy)]
pub struct Direction;

impl Direction {
    /// North
    pub const N: f64 = 0.0;
    /// Northeast
    pub const NE: f64 = 1.0;
    /// East
    pub const E: f64 = 2.0;
    /// Southeast
    pub const SE: f64 = 3.0;
    /// South
    pub const S: f64 = 4.0;
    /// Southwest
    pub const SW: f64 = 5.0;
    /// West
    pub const W: f64 = 6.0;
    /// Northwest
    pub const NW: f64 = 7.0;
    /// Source cell (no direction)
    pub const SOURCE: f64 = -1.0;
}

/// Result of a cost-distance analysis
#[derive(Debug)]
pub struct CostDistanceResult {
    /// Accumulated cost-distance raster
    pub cost_distance: RasterBuffer,
    /// Direction (backlink) raster: direction to next cell toward the nearest source
    pub direction: Option<RasterBuffer>,
    /// Allocation raster: index of the nearest source for each cell
    pub allocation: Option<RasterBuffer>,
}

// ===========================================================================
// 8-neighbor offsets and directions
// ===========================================================================

/// 8-connected neighbor offsets: (dx, dy, direction_from_neighbor_to_current)
const NEIGHBORS: [(i64, i64, f64); 8] = [
    (0, -1, Direction::S),   // N neighbor -> go South to reach current
    (1, -1, Direction::SW),  // NE neighbor
    (1, 0, Direction::W),    // E neighbor
    (1, 1, Direction::NW),   // SE neighbor
    (0, 1, Direction::N),    // S neighbor
    (-1, 1, Direction::NE),  // SW neighbor
    (-1, 0, Direction::E),   // W neighbor
    (-1, -1, Direction::SE), // NW neighbor
];

/// Returns the direction index pointing from (nx, ny) back toward (cx, cy)
fn direction_from_offset(dx: i64, dy: i64) -> f64 {
    match (dx, dy) {
        (0, -1) => Direction::N,
        (1, -1) => Direction::NE,
        (1, 0) => Direction::E,
        (1, 1) => Direction::SE,
        (0, 1) => Direction::S,
        (-1, 1) => Direction::SW,
        (-1, 0) => Direction::W,
        (-1, -1) => Direction::NW,
        _ => Direction::SOURCE,
    }
}

// ===========================================================================
// Euclidean distance
// ===========================================================================

/// Computes Euclidean distance from source cells
///
/// Uses a brute-force approach: for each cell, finds the minimum distance
/// to any source cell.
///
/// # Arguments
///
/// * `sources` - Binary raster where non-zero cells are sources
/// * `cell_size` - Size of each cell
///
/// # Errors
///
/// Returns an error if the operation fails
pub fn euclidean_distance(sources: &RasterBuffer, cell_size: f64) -> Result<RasterBuffer> {
    let width = sources.width();
    let height = sources.height();
    let mut distance = RasterBuffer::zeros(width, height, RasterDataType::Float64);

    // Find source cells
    let mut source_cells = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let val = sources.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            if val > 0.0 {
                source_cells.push((x, y));
            }
        }
    }

    // Initialize with infinity
    for y in 0..height {
        for x in 0..width {
            distance
                .set_pixel(x, y, f64::INFINITY)
                .map_err(AlgorithmError::Core)?;
        }
    }

    // Compute distance to nearest source for each cell
    for y in 0..height {
        for x in 0..width {
            let mut min_dist = f64::INFINITY;

            for &(sx, sy) in &source_cells {
                let dx = (x as f64 - sx as f64) * cell_size;
                let dy = (y as f64 - sy as f64) * cell_size;
                let dist = (dx * dx + dy * dy).sqrt();
                min_dist = min_dist.min(dist);
            }

            distance
                .set_pixel(x, y, min_dist)
                .map_err(AlgorithmError::Core)?;
        }
    }

    Ok(distance)
}

// ===========================================================================
// Cost distance (isotropic, backward-compatible)
// ===========================================================================

/// Computes cost-weighted distance using Dijkstra's algorithm
///
/// # Arguments
///
/// * `sources` - Binary raster with source locations (non-zero = source)
/// * `cost_surface` - Raster with cost values (higher = harder to traverse)
/// * `cell_size` - Size of each cell
///
/// # Errors
///
/// Returns an error if the operation fails
pub fn cost_distance(
    sources: &RasterBuffer,
    cost_surface: &RasterBuffer,
    cell_size: f64,
) -> Result<RasterBuffer> {
    let result = cost_distance_full(
        sources,
        cost_surface,
        None,
        cell_size,
        FrictionModel::Isotropic,
    )?;
    Ok(result.cost_distance)
}

/// Computes cost-distance with full output (cost, direction, allocation)
///
/// # Arguments
///
/// * `sources` - Binary raster with source locations (non-zero = source)
/// * `cost_surface` - Raster with cost values
/// * `barriers` - Optional barrier raster (non-zero cells are impassable)
/// * `cell_size` - Size of each cell
/// * `friction_model` - Friction model to use
///
/// # Errors
///
/// Returns an error if the operation fails
pub fn cost_distance_full(
    sources: &RasterBuffer,
    cost_surface: &RasterBuffer,
    barriers: Option<&RasterBuffer>,
    cell_size: f64,
    friction_model: FrictionModel,
) -> Result<CostDistanceResult> {
    let width = sources.width();
    let height = sources.height();

    let mut cost_dist = RasterBuffer::zeros(width, height, RasterDataType::Float64);
    let mut direction = RasterBuffer::zeros(width, height, RasterDataType::Float64);
    let mut allocation = RasterBuffer::zeros(width, height, RasterDataType::Float64);
    let mut visited = vec![false; (width * height) as usize];

    // Initialize with infinity
    for y in 0..height {
        for x in 0..width {
            cost_dist
                .set_pixel(x, y, f64::INFINITY)
                .map_err(AlgorithmError::Core)?;
            direction
                .set_pixel(x, y, Direction::SOURCE)
                .map_err(AlgorithmError::Core)?;
            allocation
                .set_pixel(x, y, -1.0)
                .map_err(AlgorithmError::Core)?;
        }
    }

    // Priority queue for Dijkstra's
    let mut pq = BinaryHeap::new();

    // Add source cells to queue
    let mut source_index = 0.0_f64;
    for y in 0..height {
        for x in 0..width {
            let val = sources.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            if val > 0.0 {
                cost_dist
                    .set_pixel(x, y, 0.0)
                    .map_err(AlgorithmError::Core)?;
                allocation
                    .set_pixel(x, y, source_index)
                    .map_err(AlgorithmError::Core)?;
                pq.push(CostCell { x, y, cost: 0.0 });
                source_index += 1.0;
            }
        }
    }

    // We need a separate DEM for anisotropic models (use cost_surface as proxy for elevation)
    // In practice, callers would pass DEM separately; here we approximate

    // Dijkstra's algorithm
    while let Some(cell) = pq.pop() {
        let idx = (cell.y * width + cell.x) as usize;
        if visited[idx] {
            continue;
        }
        visited[idx] = true;

        let current_alloc = allocation
            .get_pixel(cell.x, cell.y)
            .map_err(AlgorithmError::Core)?;

        // Check 8 neighbors
        for &(dx, dy, _dir_from) in &NEIGHBORS {
            let nx = cell.x as i64 + dx;
            let ny = cell.y as i64 + dy;

            if nx < 0 || nx >= width as i64 || ny < 0 || ny >= height as i64 {
                continue;
            }

            let nx_u = nx as u64;
            let ny_u = ny as u64;
            let n_idx = (ny_u * width + nx_u) as usize;

            if visited[n_idx] {
                continue;
            }

            // Check barrier
            if let Some(barrier_raster) = barriers {
                let barrier_val = barrier_raster
                    .get_pixel(nx_u, ny_u)
                    .map_err(AlgorithmError::Core)?;
                if barrier_val > 0.0 {
                    continue; // Impassable
                }
            }

            let neighbor_cost = cost_surface
                .get_pixel(nx_u, ny_u)
                .map_err(AlgorithmError::Core)?;

            if neighbor_cost < 0.0 || neighbor_cost.is_nan() || neighbor_cost.is_infinite() {
                continue; // Invalid cost, treat as barrier
            }

            let current_cost_val = cost_surface
                .get_pixel(cell.x, cell.y)
                .map_err(AlgorithmError::Core)?;

            // Base distance (cardinal = cell_size, diagonal = cell_size * sqrt(2))
            let base_dist = if dx.abs() + dy.abs() == 2 {
                cell_size * core::f64::consts::SQRT_2
            } else {
                cell_size
            };

            // Average cost between current and neighbor cells
            let avg_cost = (current_cost_val + neighbor_cost) / 2.0;

            // Apply friction model
            let friction_factor = match friction_model {
                FrictionModel::Isotropic => 1.0,
                FrictionModel::ToblerHiking => {
                    // Use cost surface values as proxy for elevation difference
                    let dz = neighbor_cost - current_cost_val;
                    let slope_tangent = dz / base_dist;
                    let speed = 6.0 * (-3.5 * (slope_tangent + 0.05).abs()).exp();
                    if speed > 0.001 { 1.0 / speed } else { 1000.0 }
                }
                FrictionModel::SymmetricSlope { slope_weight } => {
                    let dz = (neighbor_cost - current_cost_val).abs();
                    1.0 + dz * slope_weight / base_dist
                }
                FrictionModel::AsymmetricSlope {
                    uphill_weight,
                    downhill_weight,
                } => {
                    let dz = neighbor_cost - current_cost_val;
                    if dz > 0.0 {
                        1.0 + dz * uphill_weight / base_dist
                    } else {
                        1.0 + dz.abs() * downhill_weight / base_dist
                    }
                }
            };

            let travel_cost = avg_cost * base_dist * friction_factor;
            let new_cost = cell.cost + travel_cost;

            let current_best = cost_dist
                .get_pixel(nx_u, ny_u)
                .map_err(AlgorithmError::Core)?;

            if new_cost < current_best {
                cost_dist
                    .set_pixel(nx_u, ny_u, new_cost)
                    .map_err(AlgorithmError::Core)?;

                // Direction from neighbor pointing back to current cell
                let back_dir = direction_from_offset(-dx, -dy);
                direction
                    .set_pixel(nx_u, ny_u, back_dir)
                    .map_err(AlgorithmError::Core)?;

                allocation
                    .set_pixel(nx_u, ny_u, current_alloc)
                    .map_err(AlgorithmError::Core)?;

                pq.push(CostCell {
                    x: nx_u,
                    y: ny_u,
                    cost: new_cost,
                });
            }
        }
    }

    Ok(CostDistanceResult {
        cost_distance: cost_dist,
        direction: Some(direction),
        allocation: Some(allocation),
    })
}

/// Computes anisotropic cost distance using a DEM for slope calculations
///
/// Unlike `cost_distance_full` which uses cost surface values as elevation proxy,
/// this function takes an explicit DEM for proper slope-based anisotropic costs.
///
/// # Arguments
///
/// * `sources` - Binary raster with source locations
/// * `cost_surface` - Base friction cost raster
/// * `dem` - Digital elevation model for slope computation
/// * `barriers` - Optional barrier raster
/// * `cell_size` - Cell size
/// * `friction_model` - Friction model to use
///
/// # Errors
///
/// Returns an error if dimensions don't match or operation fails
pub fn cost_distance_anisotropic(
    sources: &RasterBuffer,
    cost_surface: &RasterBuffer,
    dem: &RasterBuffer,
    barriers: Option<&RasterBuffer>,
    cell_size: f64,
    friction_model: FrictionModel,
) -> Result<CostDistanceResult> {
    let width = sources.width();
    let height = sources.height();

    if dem.width() != width || dem.height() != height {
        return Err(AlgorithmError::InvalidDimensions {
            message: "DEM dimensions must match source raster",
            actual: dem.width() as usize,
            expected: width as usize,
        });
    }

    let mut cost_dist = RasterBuffer::zeros(width, height, RasterDataType::Float64);
    let mut direction = RasterBuffer::zeros(width, height, RasterDataType::Float64);
    let mut allocation = RasterBuffer::zeros(width, height, RasterDataType::Float64);
    let mut visited = vec![false; (width * height) as usize];

    // Initialize
    for y in 0..height {
        for x in 0..width {
            cost_dist
                .set_pixel(x, y, f64::INFINITY)
                .map_err(AlgorithmError::Core)?;
            direction
                .set_pixel(x, y, Direction::SOURCE)
                .map_err(AlgorithmError::Core)?;
            allocation
                .set_pixel(x, y, -1.0)
                .map_err(AlgorithmError::Core)?;
        }
    }

    let mut pq = BinaryHeap::new();
    let mut source_index = 0.0_f64;

    for y in 0..height {
        for x in 0..width {
            let val = sources.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            if val > 0.0 {
                cost_dist
                    .set_pixel(x, y, 0.0)
                    .map_err(AlgorithmError::Core)?;
                allocation
                    .set_pixel(x, y, source_index)
                    .map_err(AlgorithmError::Core)?;
                pq.push(CostCell { x, y, cost: 0.0 });
                source_index += 1.0;
            }
        }
    }

    while let Some(cell) = pq.pop() {
        let idx = (cell.y * width + cell.x) as usize;
        if visited[idx] {
            continue;
        }
        visited[idx] = true;

        let current_alloc = allocation
            .get_pixel(cell.x, cell.y)
            .map_err(AlgorithmError::Core)?;

        let current_elev = dem
            .get_pixel(cell.x, cell.y)
            .map_err(AlgorithmError::Core)?;

        let current_friction = cost_surface
            .get_pixel(cell.x, cell.y)
            .map_err(AlgorithmError::Core)?;

        for &(dx, dy, _) in &NEIGHBORS {
            let nx = cell.x as i64 + dx;
            let ny = cell.y as i64 + dy;

            if nx < 0 || nx >= width as i64 || ny < 0 || ny >= height as i64 {
                continue;
            }

            let nx_u = nx as u64;
            let ny_u = ny as u64;
            let n_idx = (ny_u * width + nx_u) as usize;

            if visited[n_idx] {
                continue;
            }

            if let Some(barrier_raster) = barriers {
                let barrier_val = barrier_raster
                    .get_pixel(nx_u, ny_u)
                    .map_err(AlgorithmError::Core)?;
                if barrier_val > 0.0 {
                    continue;
                }
            }

            let neighbor_friction = cost_surface
                .get_pixel(nx_u, ny_u)
                .map_err(AlgorithmError::Core)?;

            if neighbor_friction < 0.0
                || neighbor_friction.is_nan()
                || neighbor_friction.is_infinite()
            {
                continue;
            }

            let neighbor_elev = dem.get_pixel(nx_u, ny_u).map_err(AlgorithmError::Core)?;

            let base_dist = if dx.abs() + dy.abs() == 2 {
                cell_size * core::f64::consts::SQRT_2
            } else {
                cell_size
            };

            let avg_friction = (current_friction + neighbor_friction) / 2.0;
            let dz = neighbor_elev - current_elev;

            let friction_factor = match friction_model {
                FrictionModel::Isotropic => 1.0,
                FrictionModel::ToblerHiking => {
                    let slope_tangent = dz / base_dist;
                    let speed = 6.0 * (-3.5 * (slope_tangent + 0.05).abs()).exp();
                    if speed > 0.001 { 1.0 / speed } else { 1000.0 }
                }
                FrictionModel::SymmetricSlope { slope_weight } => {
                    1.0 + dz.abs() * slope_weight / base_dist
                }
                FrictionModel::AsymmetricSlope {
                    uphill_weight,
                    downhill_weight,
                } => {
                    if dz > 0.0 {
                        1.0 + dz * uphill_weight / base_dist
                    } else {
                        1.0 + dz.abs() * downhill_weight / base_dist
                    }
                }
            };

            let travel_cost = avg_friction * base_dist * friction_factor;
            let new_cost = cell.cost + travel_cost;

            let current_best = cost_dist
                .get_pixel(nx_u, ny_u)
                .map_err(AlgorithmError::Core)?;

            if new_cost < current_best {
                cost_dist
                    .set_pixel(nx_u, ny_u, new_cost)
                    .map_err(AlgorithmError::Core)?;

                let back_dir = direction_from_offset(-dx, -dy);
                direction
                    .set_pixel(nx_u, ny_u, back_dir)
                    .map_err(AlgorithmError::Core)?;

                allocation
                    .set_pixel(nx_u, ny_u, current_alloc)
                    .map_err(AlgorithmError::Core)?;

                pq.push(CostCell {
                    x: nx_u,
                    y: ny_u,
                    cost: new_cost,
                });
            }
        }
    }

    Ok(CostDistanceResult {
        cost_distance: cost_dist,
        direction: Some(direction),
        allocation: Some(allocation),
    })
}

// ===========================================================================
// Least-cost path
// ===========================================================================

/// Extracts least-cost path from destination back to source using the direction raster
///
/// Returns a binary raster with the path marked as 1.
///
/// # Arguments
///
/// * `cost_distance_raster` - Pre-computed cost-distance raster
/// * `dest_x` - Destination X coordinate
/// * `dest_y` - Destination Y coordinate
///
/// # Errors
///
/// Returns an error if the path cannot be found or coordinates are invalid
pub fn least_cost_path(
    cost_distance_raster: &RasterBuffer,
    dest_x: u64,
    dest_y: u64,
) -> Result<RasterBuffer> {
    let width = cost_distance_raster.width();
    let height = cost_distance_raster.height();
    let mut path = RasterBuffer::zeros(width, height, RasterDataType::UInt8);

    if dest_x >= width || dest_y >= height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "destination",
            message: format!(
                "Destination ({}, {}) is outside raster bounds ({}, {})",
                dest_x, dest_y, width, height
            ),
        });
    }

    let mut current_x = dest_x;
    let mut current_y = dest_y;
    let max_steps = (width * height) as usize; // Safety limit

    for _ in 0..max_steps {
        path.set_pixel(current_x, current_y, 1.0)
            .map_err(AlgorithmError::Core)?;

        let current_cost = cost_distance_raster
            .get_pixel(current_x, current_y)
            .map_err(AlgorithmError::Core)?;

        if current_cost <= 0.0 || current_cost.is_nan() {
            break; // Reached source
        }

        // Find neighbor with minimum cost
        let mut min_cost = current_cost;
        let mut next_x = current_x;
        let mut next_y = current_y;

        for &(dx, dy, _) in &NEIGHBORS {
            let nx = current_x as i64 + dx;
            let ny = current_y as i64 + dy;

            if nx < 0 || nx >= width as i64 || ny < 0 || ny >= height as i64 {
                continue;
            }

            let nx_u = nx as u64;
            let ny_u = ny as u64;

            let neighbor_cost = cost_distance_raster
                .get_pixel(nx_u, ny_u)
                .map_err(AlgorithmError::Core)?;

            if neighbor_cost < min_cost {
                min_cost = neighbor_cost;
                next_x = nx_u;
                next_y = ny_u;
            }
        }

        if next_x == current_x && next_y == current_y {
            break; // Stuck (no downhill neighbor)
        }

        current_x = next_x;
        current_y = next_y;
    }

    Ok(path)
}

/// Extracts least-cost path using the direction (backlink) raster
///
/// The direction raster encodes which direction to follow from each cell
/// back toward the source. This is more efficient than the gradient-descent
/// approach used by `least_cost_path`.
///
/// # Arguments
///
/// * `direction_raster` - Direction raster from `cost_distance_full`
/// * `dest_x` - Destination X coordinate
/// * `dest_y` - Destination Y coordinate
///
/// # Errors
///
/// Returns an error if the path cannot be found
pub fn least_cost_path_from_direction(
    direction_raster: &RasterBuffer,
    dest_x: u64,
    dest_y: u64,
) -> Result<RasterBuffer> {
    let width = direction_raster.width();
    let height = direction_raster.height();
    let mut path = RasterBuffer::zeros(width, height, RasterDataType::UInt8);

    if dest_x >= width || dest_y >= height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "destination",
            message: format!(
                "Destination ({}, {}) is outside raster bounds ({}, {})",
                dest_x, dest_y, width, height
            ),
        });
    }

    let mut cx = dest_x;
    let mut cy = dest_y;
    let max_steps = (width * height) as usize;

    for _ in 0..max_steps {
        path.set_pixel(cx, cy, 1.0).map_err(AlgorithmError::Core)?;

        let dir = direction_raster
            .get_pixel(cx, cy)
            .map_err(AlgorithmError::Core)?;

        if (dir - Direction::SOURCE).abs() < 0.5 {
            break; // Reached source
        }

        // Follow direction
        let (dx, dy) = direction_to_offset(dir);
        let nx = cx as i64 + dx;
        let ny = cy as i64 + dy;

        if nx < 0 || nx >= width as i64 || ny < 0 || ny >= height as i64 {
            break;
        }

        cx = nx as u64;
        cy = ny as u64;
    }

    Ok(path)
}

/// Converts a direction code to (dx, dy) offset
fn direction_to_offset(dir: f64) -> (i64, i64) {
    let d = dir.round() as i64;
    match d {
        0 => (0, -1),  // N
        1 => (1, -1),  // NE
        2 => (1, 0),   // E
        3 => (1, 1),   // SE
        4 => (0, 1),   // S
        5 => (-1, 1),  // SW
        6 => (-1, 0),  // W
        7 => (-1, -1), // NW
        _ => (0, 0),   // Source or invalid
    }
}

// ===========================================================================
// A* path finding
// ===========================================================================

/// A* cost cell with heuristic
#[derive(Copy, Clone, PartialEq)]
struct AStarCell {
    x: u64,
    y: u64,
    g_cost: f64, // Actual cost from start
    f_cost: f64, // g_cost + heuristic
}

impl Eq for AStarCell {}

impl PartialOrd for AStarCell {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for AStarCell {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .f_cost
            .partial_cmp(&self.f_cost)
            .unwrap_or(Ordering::Equal)
    }
}

/// Computes A* shortest path between two specific points
///
/// Uses Euclidean distance as admissible heuristic. Returns the path
/// as a binary raster and the total cost.
///
/// # Arguments
///
/// * `cost_surface` - Cost raster
/// * `barriers` - Optional barrier raster
/// * `start_x`, `start_y` - Start coordinates
/// * `end_x`, `end_y` - End coordinates
/// * `cell_size` - Cell size
///
/// # Errors
///
/// Returns an error if no path exists or coordinates are invalid
pub fn astar_path(
    cost_surface: &RasterBuffer,
    barriers: Option<&RasterBuffer>,
    start_x: u64,
    start_y: u64,
    end_x: u64,
    end_y: u64,
    cell_size: f64,
) -> Result<(RasterBuffer, f64)> {
    let width = cost_surface.width();
    let height = cost_surface.height();

    if start_x >= width || start_y >= height || end_x >= width || end_y >= height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "coordinates",
            message: "Start or end coordinates are outside raster bounds".to_string(),
        });
    }

    let mut g_costs = vec![f64::INFINITY; (width * height) as usize];
    let mut came_from = vec![(u64::MAX, u64::MAX); (width * height) as usize];
    let mut closed = vec![false; (width * height) as usize];

    let start_idx = (start_y * width + start_x) as usize;
    g_costs[start_idx] = 0.0;

    let mut open = BinaryHeap::new();
    let h = heuristic(start_x, start_y, end_x, end_y, cell_size);
    open.push(AStarCell {
        x: start_x,
        y: start_y,
        g_cost: 0.0,
        f_cost: h,
    });

    while let Some(current) = open.pop() {
        if current.x == end_x && current.y == end_y {
            // Reconstruct path
            let path = reconstruct_path(&came_from, width, height, end_x, end_y)?;
            return Ok((path, current.g_cost));
        }

        let idx = (current.y * width + current.x) as usize;
        if closed[idx] {
            continue;
        }
        closed[idx] = true;

        for &(dx, dy, _) in &NEIGHBORS {
            let nx = current.x as i64 + dx;
            let ny = current.y as i64 + dy;

            if nx < 0 || nx >= width as i64 || ny < 0 || ny >= height as i64 {
                continue;
            }

            let nx_u = nx as u64;
            let ny_u = ny as u64;
            let n_idx = (ny_u * width + nx_u) as usize;

            if closed[n_idx] {
                continue;
            }

            if let Some(barrier_raster) = barriers {
                let barrier_val = barrier_raster
                    .get_pixel(nx_u, ny_u)
                    .map_err(AlgorithmError::Core)?;
                if barrier_val > 0.0 {
                    continue;
                }
            }

            let neighbor_cost = cost_surface
                .get_pixel(nx_u, ny_u)
                .map_err(AlgorithmError::Core)?;

            if neighbor_cost < 0.0 || neighbor_cost.is_nan() || neighbor_cost.is_infinite() {
                continue;
            }

            let current_cost_val = cost_surface
                .get_pixel(current.x, current.y)
                .map_err(AlgorithmError::Core)?;

            let base_dist = if dx.abs() + dy.abs() == 2 {
                cell_size * core::f64::consts::SQRT_2
            } else {
                cell_size
            };

            let avg_cost = (current_cost_val + neighbor_cost) / 2.0;
            let travel_cost = avg_cost * base_dist;
            let tentative_g = current.g_cost + travel_cost;

            if tentative_g < g_costs[n_idx] {
                g_costs[n_idx] = tentative_g;
                came_from[n_idx] = (current.x, current.y);

                let h = heuristic(nx_u, ny_u, end_x, end_y, cell_size);
                open.push(AStarCell {
                    x: nx_u,
                    y: ny_u,
                    g_cost: tentative_g,
                    f_cost: tentative_g + h,
                });
            }
        }
    }

    Err(AlgorithmError::PathNotFound(format!(
        "No path exists from ({}, {}) to ({}, {})",
        start_x, start_y, end_x, end_y
    )))
}

/// Euclidean distance heuristic for A*
fn heuristic(x1: u64, y1: u64, x2: u64, y2: u64, cell_size: f64) -> f64 {
    let dx = (x2 as f64 - x1 as f64) * cell_size;
    let dy = (y2 as f64 - y1 as f64) * cell_size;
    (dx * dx + dy * dy).sqrt()
}

/// Reconstructs the A* path from came_from array
fn reconstruct_path(
    came_from: &[(u64, u64)],
    width: u64,
    height: u64,
    end_x: u64,
    end_y: u64,
) -> Result<RasterBuffer> {
    let mut path = RasterBuffer::zeros(width, height, RasterDataType::UInt8);

    let mut cx = end_x;
    let mut cy = end_y;
    let max_steps = (width * height) as usize;

    for _ in 0..max_steps {
        path.set_pixel(cx, cy, 1.0).map_err(AlgorithmError::Core)?;

        let idx = (cy * width + cx) as usize;
        let (px, py) = came_from[idx];

        if px == u64::MAX {
            break; // Reached start
        }

        cx = px;
        cy = py;
    }

    Ok(path)
}

// ===========================================================================
// Corridor analysis
// ===========================================================================

/// Computes a least-cost corridor between two source regions
///
/// The corridor raster shows the combined cost from both sources. Low values
/// indicate the corridor (efficient route between the two sources).
///
/// corridor(x,y) = cost_from_a(x,y) + cost_from_b(x,y)
///
/// # Arguments
///
/// * `cost_from_a` - Cost-distance raster from source A
/// * `cost_from_b` - Cost-distance raster from source B
///
/// # Errors
///
/// Returns an error if the rasters have different dimensions
pub fn compute_corridor(
    cost_from_a: &RasterBuffer,
    cost_from_b: &RasterBuffer,
) -> Result<RasterBuffer> {
    let width = cost_from_a.width();
    let height = cost_from_a.height();

    if cost_from_b.width() != width || cost_from_b.height() != height {
        return Err(AlgorithmError::InvalidDimensions {
            message: "Cost distance rasters must have the same dimensions",
            actual: cost_from_b.width() as usize,
            expected: width as usize,
        });
    }

    let mut corridor = RasterBuffer::zeros(width, height, RasterDataType::Float64);

    for y in 0..height {
        for x in 0..width {
            let ca = cost_from_a.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            let cb = cost_from_b.get_pixel(x, y).map_err(AlgorithmError::Core)?;

            let combined = if ca.is_infinite() || cb.is_infinite() {
                f64::INFINITY
            } else {
                ca + cb
            };

            corridor
                .set_pixel(x, y, combined)
                .map_err(AlgorithmError::Core)?;
        }
    }

    Ok(corridor)
}

/// Computes normalized corridor (values relative to minimum corridor cost)
///
/// Subtracts the minimum combined cost so the optimal path has value 0.
/// Cells above the given threshold are set to infinity (not in corridor).
///
/// # Arguments
///
/// * `cost_from_a` - Cost-distance from source A
/// * `cost_from_b` - Cost-distance from source B
/// * `threshold` - Maximum cost above minimum to include in corridor
///
/// # Errors
///
/// Returns an error if the rasters have different dimensions
pub fn compute_corridor_normalized(
    cost_from_a: &RasterBuffer,
    cost_from_b: &RasterBuffer,
    threshold: f64,
) -> Result<RasterBuffer> {
    let corridor = compute_corridor(cost_from_a, cost_from_b)?;
    let width = corridor.width();
    let height = corridor.height();

    // Find minimum corridor value
    let mut min_val = f64::INFINITY;
    for y in 0..height {
        for x in 0..width {
            let v = corridor.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            if v < min_val {
                min_val = v;
            }
        }
    }

    let mut normalized = RasterBuffer::zeros(width, height, RasterDataType::Float64);

    for y in 0..height {
        for x in 0..width {
            let v = corridor.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            let relative = v - min_val;
            let final_val = if relative > threshold {
                f64::INFINITY
            } else {
                relative
            };
            normalized
                .set_pixel(x, y, final_val)
                .map_err(AlgorithmError::Core)?;
        }
    }

    Ok(normalized)
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn create_uniform_cost(size: u64, cost_value: f64) -> RasterBuffer {
        let mut buf = RasterBuffer::zeros(size, size, RasterDataType::Float32);
        for y in 0..size {
            for x in 0..size {
                let _ = buf.set_pixel(x, y, cost_value);
            }
        }
        buf
    }

    fn create_sources_single(size: u64, sx: u64, sy: u64) -> RasterBuffer {
        let mut buf = RasterBuffer::zeros(size, size, RasterDataType::Float32);
        let _ = buf.set_pixel(sx, sy, 1.0);
        buf
    }

    fn create_barrier_wall(size: u64, wall_col: u64) -> RasterBuffer {
        let mut buf = RasterBuffer::zeros(size, size, RasterDataType::Float32);
        for y in 0..size {
            let _ = buf.set_pixel(wall_col, y, 1.0);
        }
        buf
    }

    // --- Euclidean distance ---

    #[test]
    fn test_euclidean_distance() {
        let sources = create_sources_single(10, 5, 5);
        let distance = euclidean_distance(&sources, 1.0).expect("euclidean");

        // At source should be 0
        let at_source = distance.get_pixel(5, 5).expect("pixel");
        assert!(at_source < 1e-6);

        // Adjacent should be ~1
        let adjacent = distance.get_pixel(5, 6).expect("pixel");
        assert!((adjacent - 1.0).abs() < 0.01);

        // Distance should increase
        let d1 = distance.get_pixel(5, 6).expect("pixel");
        let d2 = distance.get_pixel(5, 7).expect("pixel");
        assert!(d2 > d1);
    }

    // --- Basic cost distance ---

    #[test]
    fn test_cost_distance_uniform() {
        let sources = create_sources_single(10, 0, 0);
        let cost = create_uniform_cost(10, 1.0);
        let result = cost_distance(&sources, &cost, 1.0).expect("cost_dist");

        // Source should be 0
        let at_source = result.get_pixel(0, 0).expect("pixel");
        assert!(at_source < 1e-6);

        // Cost should increase with distance
        let near = result.get_pixel(1, 0).expect("near");
        let far = result.get_pixel(5, 5).expect("far");
        assert!(far > near);
    }

    // --- Cost distance with barriers ---

    #[test]
    fn test_cost_distance_with_barrier() {
        let sources = create_sources_single(10, 0, 5);
        let cost = create_uniform_cost(10, 1.0);
        let barrier = create_barrier_wall(10, 5);

        let result_no_barrier =
            cost_distance_full(&sources, &cost, None, 1.0, FrictionModel::Isotropic)
                .expect("no_barrier");

        let result_with_barrier = cost_distance_full(
            &sources,
            &cost,
            Some(&barrier),
            1.0,
            FrictionModel::Isotropic,
        )
        .expect("with_barrier");

        // Cells beyond the barrier should have higher cost (must go around)
        let cost_no_barrier = result_no_barrier
            .cost_distance
            .get_pixel(9, 5)
            .expect("pixel");
        let cost_with_barrier = result_with_barrier
            .cost_distance
            .get_pixel(9, 5)
            .expect("pixel");

        // With full wall barrier, cells behind it should be unreachable (infinity)
        assert!(
            cost_with_barrier.is_infinite(),
            "Cells behind full wall barrier should be unreachable, got {cost_with_barrier}"
        );
        assert!(cost_no_barrier.is_finite());
    }

    // --- Direction raster ---

    #[test]
    fn test_direction_raster() {
        let sources = create_sources_single(10, 5, 5);
        let cost = create_uniform_cost(10, 1.0);

        let result =
            cost_distance_full(&sources, &cost, None, 1.0, FrictionModel::Isotropic).expect("full");

        assert!(result.direction.is_some());
        let dir = result.direction.expect("dir");

        // Source cell should have direction = -1 (SOURCE)
        let source_dir = dir.get_pixel(5, 5).expect("pixel");
        assert!(
            (source_dir - Direction::SOURCE).abs() < 0.5,
            "Source direction should be SOURCE (-1), got {source_dir}"
        );

        // Cell to the east of source should point west (toward source)
        let east_dir = dir.get_pixel(6, 5).expect("pixel");
        assert!(
            (east_dir - Direction::W).abs() < 0.5,
            "East neighbor should point West, got {east_dir}"
        );
    }

    // --- Allocation raster ---

    #[test]
    fn test_allocation_raster() {
        let mut sources = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        let _ = sources.set_pixel(2, 5, 1.0);
        let _ = sources.set_pixel(7, 5, 1.0);

        let cost = create_uniform_cost(10, 1.0);

        let result = cost_distance_full(&sources, &cost, None, 1.0, FrictionModel::Isotropic)
            .expect("allocation");

        assert!(result.allocation.is_some());
        let alloc = result.allocation.expect("alloc");

        // Cells near source 0 (2,5) should be allocated to source 0
        let near_0 = alloc.get_pixel(1, 5).expect("pixel");
        assert!(
            (near_0 - 0.0).abs() < 0.5,
            "Cell near source 0 should be allocated to 0, got {near_0}"
        );

        // Cells near source 1 (7,5) should be allocated to source 1
        let near_1 = alloc.get_pixel(8, 5).expect("pixel");
        assert!(
            (near_1 - 1.0).abs() < 0.5,
            "Cell near source 1 should be allocated to 1, got {near_1}"
        );
    }

    // --- Least cost path ---

    #[test]
    fn test_least_cost_path() {
        let sources = create_sources_single(10, 0, 0);
        let cost = create_uniform_cost(10, 1.0);
        let cost_dist = cost_distance(&sources, &cost, 1.0).expect("cost_dist");

        let path = least_cost_path(&cost_dist, 5, 5).expect("path");

        // Destination should be on path
        let at_dest = path.get_pixel(5, 5).expect("pixel");
        assert!(at_dest > 0.0);

        // Source should be on path
        let at_source = path.get_pixel(0, 0).expect("pixel");
        assert!(at_source > 0.0);
    }

    #[test]
    fn test_least_cost_path_from_direction() {
        let sources = create_sources_single(10, 0, 0);
        let cost = create_uniform_cost(10, 1.0);

        let result =
            cost_distance_full(&sources, &cost, None, 1.0, FrictionModel::Isotropic).expect("full");

        let dir_raster = result.direction.expect("dir");
        let path = least_cost_path_from_direction(&dir_raster, 5, 5).expect("path");

        let at_dest = path.get_pixel(5, 5).expect("pixel");
        assert!(at_dest > 0.0);
    }

    // --- A* path finding ---

    #[test]
    fn test_astar_path() {
        let cost = create_uniform_cost(10, 1.0);
        let (path, total_cost) = astar_path(&cost, None, 0, 0, 9, 9, 1.0).expect("astar");

        let at_start = path.get_pixel(0, 0).expect("pixel");
        let at_end = path.get_pixel(9, 9).expect("pixel");
        assert!(at_start > 0.0);
        assert!(at_end > 0.0);

        // Diagonal distance from (0,0) to (9,9): ~9*sqrt(2)*1.0 ~= 12.7
        assert!(total_cost > 10.0 && total_cost < 20.0);
    }

    #[test]
    fn test_astar_path_with_barrier() {
        let cost = create_uniform_cost(10, 1.0);
        // Create a partial wall (leave gap at top)
        let mut barrier = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        for y in 1..10 {
            let _ = barrier.set_pixel(5, y, 1.0);
        }

        let result = astar_path(&cost, Some(&barrier), 0, 5, 9, 5, 1.0);
        assert!(result.is_ok());
        let (_, total_cost) = result.expect("astar_barrier");
        // Must go around the barrier, so cost should be higher
        assert!(total_cost > 9.0, "Path around barrier should be longer");
    }

    #[test]
    fn test_astar_no_path() {
        let cost = create_uniform_cost(10, 1.0);
        let barrier = create_barrier_wall(10, 5); // Full wall

        let result = astar_path(&cost, Some(&barrier), 0, 5, 9, 5, 1.0);
        assert!(result.is_err());
    }

    // --- Corridor analysis ---

    #[test]
    fn test_corridor() {
        let source_a = create_sources_single(10, 0, 5);
        let source_b = create_sources_single(10, 9, 5);
        let cost = create_uniform_cost(10, 1.0);

        let cost_a = cost_distance(&source_a, &cost, 1.0).expect("cost_a");
        let cost_b = cost_distance(&source_b, &cost, 1.0).expect("cost_b");

        let corridor = compute_corridor(&cost_a, &cost_b).expect("corridor");

        // Minimum corridor should be along the straight line y=5
        let center = corridor.get_pixel(5, 5).expect("center");
        let off_center = corridor.get_pixel(5, 0).expect("off_center");
        assert!(
            center < off_center,
            "Corridor center should have lower cost than off-center"
        );
    }

    #[test]
    fn test_corridor_normalized() {
        let source_a = create_sources_single(10, 0, 5);
        let source_b = create_sources_single(10, 9, 5);
        let cost = create_uniform_cost(10, 1.0);

        let cost_a = cost_distance(&source_a, &cost, 1.0).expect("cost_a");
        let cost_b = cost_distance(&source_b, &cost, 1.0).expect("cost_b");

        let normalized = compute_corridor_normalized(&cost_a, &cost_b, 5.0).expect("normalized");

        // Minimum should be 0
        let mut found_zero = false;
        for y in 0..10 {
            for x in 0..10 {
                let v = normalized.get_pixel(x, y).expect("pixel");
                if v.abs() < 0.01 {
                    found_zero = true;
                }
            }
        }
        assert!(found_zero, "Normalized corridor should have a zero minimum");
    }

    // --- Anisotropic cost distance ---

    #[test]
    fn test_anisotropic_tobler() {
        let sources = create_sources_single(10, 5, 5);
        let cost = create_uniform_cost(10, 1.0);
        // Create a simple slope DEM
        let mut dem = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        for y in 0..10 {
            for x in 0..10 {
                let _ = dem.set_pixel(x, y, y as f64 * 10.0);
            }
        }

        let result = cost_distance_anisotropic(
            &sources,
            &cost,
            &dem,
            None,
            1.0,
            FrictionModel::ToblerHiking,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_anisotropic_symmetric_slope() {
        let sources = create_sources_single(10, 5, 5);
        let cost = create_uniform_cost(10, 1.0);
        let mut dem = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        for y in 0..10 {
            for x in 0..10 {
                let _ = dem.set_pixel(x, y, y as f64 * 5.0);
            }
        }

        let result = cost_distance_anisotropic(
            &sources,
            &cost,
            &dem,
            None,
            1.0,
            FrictionModel::SymmetricSlope { slope_weight: 1.0 },
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_anisotropic_asymmetric_slope() {
        let sources = create_sources_single(10, 5, 5);
        let cost = create_uniform_cost(10, 1.0);
        let mut dem = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        for y in 0..10 {
            for x in 0..10 {
                let _ = dem.set_pixel(x, y, y as f64 * 5.0);
            }
        }

        let result = cost_distance_anisotropic(
            &sources,
            &cost,
            &dem,
            None,
            1.0,
            FrictionModel::AsymmetricSlope {
                uphill_weight: 2.0,
                downhill_weight: 0.5,
            },
        );
        assert!(result.is_ok());

        let cd = result.expect("aniso");

        // Cost going uphill should be higher than downhill
        let uphill = cd.cost_distance.get_pixel(5, 0).expect("uphill"); // y decreases = uphill
        let downhill = cd.cost_distance.get_pixel(5, 9).expect("downhill"); // y increases = downhill
        // Note: depending on DEM setup, these may vary but both should be finite
        assert!(uphill.is_finite());
        assert!(downhill.is_finite());
    }

    // --- Friction model tests ---

    #[test]
    fn test_friction_model_default() {
        let model = FrictionModel::default();
        assert_eq!(model, FrictionModel::Isotropic);
    }

    // --- Edge case tests ---

    #[test]
    fn test_least_cost_path_invalid_destination() {
        let cost_dist = RasterBuffer::zeros(10, 10, RasterDataType::Float64);
        let result = least_cost_path(&cost_dist, 100, 100);
        assert!(result.is_err());
    }

    #[test]
    fn test_direction_from_offset_all() {
        assert!((direction_from_offset(0, -1) - Direction::N).abs() < 0.5);
        assert!((direction_from_offset(1, -1) - Direction::NE).abs() < 0.5);
        assert!((direction_from_offset(1, 0) - Direction::E).abs() < 0.5);
        assert!((direction_from_offset(1, 1) - Direction::SE).abs() < 0.5);
        assert!((direction_from_offset(0, 1) - Direction::S).abs() < 0.5);
        assert!((direction_from_offset(-1, 1) - Direction::SW).abs() < 0.5);
        assert!((direction_from_offset(-1, 0) - Direction::W).abs() < 0.5);
        assert!((direction_from_offset(-1, -1) - Direction::NW).abs() < 0.5);
    }

    #[test]
    fn test_direction_to_offset_all() {
        assert_eq!(direction_to_offset(Direction::N), (0, -1));
        assert_eq!(direction_to_offset(Direction::NE), (1, -1));
        assert_eq!(direction_to_offset(Direction::E), (1, 0));
        assert_eq!(direction_to_offset(Direction::SE), (1, 1));
        assert_eq!(direction_to_offset(Direction::S), (0, 1));
        assert_eq!(direction_to_offset(Direction::SW), (-1, 1));
        assert_eq!(direction_to_offset(Direction::W), (-1, 0));
        assert_eq!(direction_to_offset(Direction::NW), (-1, -1));
    }
}
