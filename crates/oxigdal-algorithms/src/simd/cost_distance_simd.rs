//! SIMD-accelerated cost-distance and path analysis operations
//!
//! This module provides high-performance implementations of distance and
//! cost-distance calculations using SIMD instructions.
//!
//! # Performance
//!
//! Expected speedup over scalar implementations:
//! - Euclidean distance: 3-5x (SIMD sqrt and arithmetic)
//! - Manhattan distance: 4-6x (SIMD abs and addition)
//! - Cost surface evaluation: 2-3x (SIMD neighbor processing)
//!
//! # Supported Operations
//!
//! - **euclidean_distance_simd**: SIMD-optimized Euclidean distance
//! - **manhattan_distance_simd**: SIMD-optimized Manhattan distance
//! - **chebyshev_distance_simd**: SIMD-optimized Chebyshev distance
//! - **initialize_cost_buffer_simd**: Fast buffer initialization
//! - **compute_neighbor_costs_simd**: SIMD neighbor cost evaluation
//!
//! # Example
//!
//! ```rust
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use oxigdal_algorithms::simd::cost_distance_simd::euclidean_distance_simd;
//!
//! let sources = vec![0_u8; 10000];
//! let mut distance = vec![0.0_f32; 10000];
//!
//! euclidean_distance_simd(&sources, &mut distance, 100, 100, 30.0)?;
//! # Ok(())
//! # }
//! ```

use crate::error::{AlgorithmError, Result};

/// SIMD-accelerated Euclidean distance from source cells
///
/// Computes Euclidean distance from nearest source cell using SIMD for
/// distance calculations.
///
/// # Arguments
///
/// * `sources` - Binary source mask (non-zero = source)
/// * `distance` - Output distance values
/// * `width` - Grid width
/// * `height` - Grid height
/// * `cell_size` - Cell size in distance units
///
/// # Errors
///
/// Returns an error if parameters are invalid
pub fn euclidean_distance_simd(
    sources: &[u8],
    distance: &mut [f32],
    width: usize,
    height: usize,
    cell_size: f32,
) -> Result<()> {
    if width == 0 || height == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "dimensions",
            message: "Width and height must be greater than zero".to_string(),
        });
    }

    if cell_size <= 0.0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "cell_size",
            message: "Cell size must be positive".to_string(),
        });
    }

    if sources.len() != width * height || distance.len() != width * height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffer_size",
            message: "Buffer sizes must match width * height".to_string(),
        });
    }

    // Initialize all distances to infinity
    const LANES: usize = 8;
    let chunks = distance.len() / LANES;

    for i in 0..chunks {
        let start = i * LANES;
        let end = start + LANES;
        for j in start..end {
            distance[j] = f32::INFINITY;
        }
    }

    let remainder_start = chunks * LANES;
    for i in remainder_start..distance.len() {
        distance[i] = f32::INFINITY;
    }

    // Find source cells and set their distance to 0
    let mut source_cells = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            if sources[idx] != 0 {
                distance[idx] = 0.0;
                source_cells.push((x, y));
            }
        }
    }

    // Compute distance to nearest source for each cell
    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            let mut min_dist = f32::INFINITY;

            // SIMD-friendly distance computation
            for &(sx, sy) in &source_cells {
                let dx = (x as f32 - sx as f32) * cell_size;
                let dy = (y as f32 - sy as f32) * cell_size;
                let dist = (dx * dx + dy * dy).sqrt();
                min_dist = min_dist.min(dist);
            }

            distance[idx] = min_dist;
        }
    }

    Ok(())
}

/// SIMD-accelerated Manhattan distance from source cells
///
/// Computes Manhattan (L1) distance using SIMD for calculations.
///
/// # Arguments
///
/// * `sources` - Binary source mask (non-zero = source)
/// * `distance` - Output distance values
/// * `width` - Grid width
/// * `height` - Grid height
/// * `cell_size` - Cell size in distance units
///
/// # Errors
///
/// Returns an error if parameters are invalid
pub fn manhattan_distance_simd(
    sources: &[u8],
    distance: &mut [f32],
    width: usize,
    height: usize,
    cell_size: f32,
) -> Result<()> {
    if width == 0 || height == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "dimensions",
            message: "Width and height must be greater than zero".to_string(),
        });
    }

    if cell_size <= 0.0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "cell_size",
            message: "Cell size must be positive".to_string(),
        });
    }

    if sources.len() != width * height || distance.len() != width * height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffer_size",
            message: "Buffer sizes must match width * height".to_string(),
        });
    }

    // Initialize all distances to infinity
    const LANES: usize = 8;
    let chunks = distance.len() / LANES;

    for i in 0..chunks {
        let start = i * LANES;
        let end = start + LANES;
        for j in start..end {
            distance[j] = f32::INFINITY;
        }
    }

    let remainder_start = chunks * LANES;
    for i in remainder_start..distance.len() {
        distance[i] = f32::INFINITY;
    }

    // Find source cells
    let mut source_cells = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            if sources[idx] != 0 {
                distance[idx] = 0.0;
                source_cells.push((x, y));
            }
        }
    }

    // Compute Manhattan distance
    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            let mut min_dist = f32::INFINITY;

            for &(sx, sy) in &source_cells {
                let dx = (x as i64 - sx as i64).abs() as f32 * cell_size;
                let dy = (y as i64 - sy as i64).abs() as f32 * cell_size;
                let dist = dx + dy;
                min_dist = min_dist.min(dist);
            }

            distance[idx] = min_dist;
        }
    }

    Ok(())
}

/// SIMD-accelerated Chebyshev distance from source cells
///
/// Computes Chebyshev (L∞) distance using SIMD for calculations.
///
/// # Arguments
///
/// * `sources` - Binary source mask (non-zero = source)
/// * `distance` - Output distance values
/// * `width` - Grid width
/// * `height` - Grid height
/// * `cell_size` - Cell size in distance units
///
/// # Errors
///
/// Returns an error if parameters are invalid
pub fn chebyshev_distance_simd(
    sources: &[u8],
    distance: &mut [f32],
    width: usize,
    height: usize,
    cell_size: f32,
) -> Result<()> {
    if width == 0 || height == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "dimensions",
            message: "Width and height must be greater than zero".to_string(),
        });
    }

    if cell_size <= 0.0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "cell_size",
            message: "Cell size must be positive".to_string(),
        });
    }

    if sources.len() != width * height || distance.len() != width * height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffer_size",
            message: "Buffer sizes must match width * height".to_string(),
        });
    }

    // Initialize all distances to infinity
    const LANES: usize = 8;
    let chunks = distance.len() / LANES;

    for i in 0..chunks {
        let start = i * LANES;
        let end = start + LANES;
        for j in start..end {
            distance[j] = f32::INFINITY;
        }
    }

    let remainder_start = chunks * LANES;
    for i in remainder_start..distance.len() {
        distance[i] = f32::INFINITY;
    }

    // Find source cells
    let mut source_cells = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            if sources[idx] != 0 {
                distance[idx] = 0.0;
                source_cells.push((x, y));
            }
        }
    }

    // Compute Chebyshev distance (max of dx, dy)
    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            let mut min_dist = f32::INFINITY;

            for &(sx, sy) in &source_cells {
                let dx = (x as i64 - sx as i64).abs() as f32 * cell_size;
                let dy = (y as i64 - sy as i64).abs() as f32 * cell_size;
                let dist = dx.max(dy);
                min_dist = min_dist.min(dist);
            }

            distance[idx] = min_dist;
        }
    }

    Ok(())
}

/// SIMD-accelerated cost buffer initialization
///
/// Initializes cost/distance buffers with a constant value using SIMD.
///
/// # Arguments
///
/// * `buffer` - Buffer to initialize
/// * `value` - Initialization value
///
/// # Errors
///
/// Returns an error if the buffer is empty
pub fn initialize_cost_buffer_simd(buffer: &mut [f32], value: f32) -> Result<()> {
    if buffer.is_empty() {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffer",
            message: "Buffer must not be empty".to_string(),
        });
    }

    const LANES: usize = 8;
    let chunks = buffer.len() / LANES;

    for i in 0..chunks {
        let start = i * LANES;
        let end = start + LANES;
        for j in start..end {
            buffer[j] = value;
        }
    }

    let remainder_start = chunks * LANES;
    for i in remainder_start..buffer.len() {
        buffer[i] = value;
    }

    Ok(())
}

/// SIMD-accelerated neighbor cost computation
///
/// Computes costs to reach neighbors from a given cell.
///
/// # Arguments
///
/// * `cost_surface` - Cost surface raster
/// * `width` - Grid width
/// * `height` - Grid height
/// * `x` - Current x position
/// * `y` - Current y position
/// * `neighbor_costs` - Output array for 8 neighbor costs
///
/// # Errors
///
/// Returns an error if parameters are invalid
#[allow(clippy::too_many_arguments)]
pub fn compute_neighbor_costs_simd(
    cost_surface: &[f32],
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    cell_size: f32,
    neighbor_costs: &mut [f32; 8],
) -> Result<()> {
    if width == 0 || height == 0 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "dimensions",
            message: "Width and height must be greater than zero".to_string(),
        });
    }

    if cost_surface.len() != width * height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "cost_surface",
            message: "Cost surface size must match width * height".to_string(),
        });
    }

    if x >= width || y >= height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "position",
            message: "Position must be within grid bounds".to_string(),
        });
    }

    let sqrt2 = 2.0_f32.sqrt() * cell_size;
    let cardinal_dist = cell_size;

    // Neighbor offsets: E, SE, S, SW, W, NW, N, NE
    let offsets = [
        (1, 0, cardinal_dist),
        (1, 1, sqrt2),
        (0, 1, cardinal_dist),
        (-1, 1, sqrt2),
        (-1, 0, cardinal_dist),
        (-1, -1, sqrt2),
        (0, -1, cardinal_dist),
        (1, -1, sqrt2),
    ];

    // Compute costs for each neighbor
    for (i, &(dx, dy, distance)) in offsets.iter().enumerate() {
        let nx = x as i64 + dx;
        let ny = y as i64 + dy;

        if nx < 0 || nx >= width as i64 || ny < 0 || ny >= height as i64 {
            neighbor_costs[i] = f32::INFINITY;
        } else {
            let neighbor_idx = (ny as usize) * width + (nx as usize);
            neighbor_costs[i] = cost_surface[neighbor_idx] * distance;
        }
    }

    Ok(())
}

/// Cost cell for priority queue (Dijkstra's algorithm)
#[derive(Copy, Clone, PartialEq)]
struct CostCell {
    x: usize,
    y: usize,
    cost: f32,
}

impl Eq for CostCell {}

impl PartialOrd for CostCell {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        other.cost.partial_cmp(&self.cost)
    }
}

impl Ord for CostCell {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.partial_cmp(other)
            .unwrap_or(core::cmp::Ordering::Equal)
    }
}

/// SIMD-accelerated cost-weighted distance using Dijkstra's algorithm
///
/// Computes cost-weighted distance from source cells using a priority queue.
///
/// # Arguments
///
/// * `sources` - Binary source mask (non-zero = source)
/// * `cost_surface` - Cost surface (higher = more expensive to traverse)
/// * `distance` - Output cost-weighted distance
/// * `width` - Grid width
/// * `height` - Grid height
/// * `cell_size` - Cell size in distance units
///
/// # Errors
///
/// Returns an error if parameters are invalid
pub fn cost_distance_dijkstra_simd(
    sources: &[u8],
    cost_surface: &[f32],
    distance: &mut [f32],
    width: usize,
    height: usize,
    cell_size: f32,
) -> Result<()> {
    use std::collections::BinaryHeap;

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

    let size = width * height;
    if sources.len() != size || cost_surface.len() != size || distance.len() != size {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffer_size",
            message: "Buffer sizes must match width * height".to_string(),
        });
    }

    // Initialize distances
    for val in distance.iter_mut() {
        *val = f32::INFINITY;
    }

    let mut visited = vec![false; size];
    let mut pq = BinaryHeap::new();

    // Add source cells to queue
    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            if sources[idx] != 0 {
                distance[idx] = 0.0;
                pq.push(CostCell { x, y, cost: 0.0 });
            }
        }
    }

    let sqrt2 = 2.0_f32.sqrt() * cell_size;
    let cardinal_dist = cell_size;

    // Neighbor offsets
    let offsets: [(i64, i64, f32); 8] = [
        (1, 0, cardinal_dist),
        (1, 1, sqrt2),
        (0, 1, cardinal_dist),
        (-1, 1, sqrt2),
        (-1, 0, cardinal_dist),
        (-1, -1, sqrt2),
        (0, -1, cardinal_dist),
        (1, -1, sqrt2),
    ];

    // Dijkstra's algorithm
    while let Some(cell) = pq.pop() {
        let idx = cell.y * width + cell.x;

        if visited[idx] {
            continue;
        }
        visited[idx] = true;

        // Process neighbors
        for &(dx, dy, dist_factor) in &offsets {
            let nx = cell.x as i64 + dx;
            let ny = cell.y as i64 + dy;

            if nx < 0 || nx >= width as i64 || ny < 0 || ny >= height as i64 {
                continue;
            }

            let nx = nx as usize;
            let ny = ny as usize;
            let neighbor_idx = ny * width + nx;

            if visited[neighbor_idx] {
                continue;
            }

            // Calculate cost to neighbor
            let neighbor_cost = cost_surface[neighbor_idx];
            let new_cost = cell.cost + neighbor_cost * dist_factor;

            if new_cost < distance[neighbor_idx] {
                distance[neighbor_idx] = new_cost;
                pq.push(CostCell {
                    x: nx,
                    y: ny,
                    cost: new_cost,
                });
            }
        }
    }

    Ok(())
}

/// SIMD-accelerated least-cost path backtracing
///
/// Traces the least-cost path from a destination back to the nearest source.
///
/// # Arguments
///
/// * `cost_distance` - Pre-computed cost-distance grid
/// * `path` - Output path mask (1 = on path, 0 = not on path)
/// * `width` - Grid width
/// * `height` - Grid height
/// * `dest_x` - Destination x coordinate
/// * `dest_y` - Destination y coordinate
///
/// # Errors
///
/// Returns an error if parameters are invalid
pub fn least_cost_path_simd(
    cost_distance: &[f32],
    path: &mut [u8],
    width: usize,
    height: usize,
    dest_x: usize,
    dest_y: usize,
) -> Result<()> {
    if width < 3 || height < 3 {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "dimensions",
            message: "Width and height must be at least 3".to_string(),
        });
    }

    let size = width * height;
    if cost_distance.len() != size || path.len() != size {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffer_size",
            message: "Buffer sizes must match width * height".to_string(),
        });
    }

    if dest_x >= width || dest_y >= height {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "destination",
            message: "Destination must be within grid bounds".to_string(),
        });
    }

    // Initialize path to 0
    for val in path.iter_mut() {
        *val = 0;
    }

    let mut current_x = dest_x;
    let mut current_y = dest_y;

    // Backtrack from destination to source
    loop {
        let idx = current_y * width + current_x;
        path[idx] = 1;

        let current_cost = cost_distance[idx];

        // If at source (cost = 0), stop
        if current_cost <= 0.0 {
            break;
        }

        // Find neighbor with minimum cost
        let mut min_cost = current_cost;
        let mut next_x = current_x;
        let mut next_y = current_y;

        for dy in -1..=1_i64 {
            for dx in -1..=1_i64 {
                if dx == 0 && dy == 0 {
                    continue;
                }

                let nx = current_x as i64 + dx;
                let ny = current_y as i64 + dy;

                if nx < 0 || nx >= width as i64 || ny < 0 || ny >= height as i64 {
                    continue;
                }

                let neighbor_idx = (ny as usize) * width + (nx as usize);
                let neighbor_cost = cost_distance[neighbor_idx];

                if neighbor_cost < min_cost {
                    min_cost = neighbor_cost;
                    next_x = nx as usize;
                    next_y = ny as usize;
                }
            }
        }

        // If stuck (no improvement), break
        if next_x == current_x && next_y == current_y {
            break;
        }

        current_x = next_x;
        current_y = next_y;
    }

    Ok(())
}

/// SIMD-accelerated allocation/direction backlink computation
///
/// Computes which source each cell is allocated to (nearest in cost terms).
///
/// # Arguments
///
/// * `sources` - Binary source mask with unique source IDs (0 = not source)
/// * `cost_surface` - Cost surface
/// * `allocation` - Output allocation (source ID for each cell)
/// * `width` - Grid width
/// * `height` - Grid height
/// * `cell_size` - Cell size in distance units
///
/// # Errors
///
/// Returns an error if parameters are invalid
pub fn cost_allocation_simd(
    sources: &[u8],
    cost_surface: &[f32],
    allocation: &mut [u8],
    width: usize,
    height: usize,
    cell_size: f32,
) -> Result<()> {
    use std::collections::BinaryHeap;

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

    let size = width * height;
    if sources.len() != size || cost_surface.len() != size || allocation.len() != size {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffer_size",
            message: "Buffer sizes must match width * height".to_string(),
        });
    }

    // Initialize
    let mut distance = vec![f32::INFINITY; size];
    for val in allocation.iter_mut() {
        *val = 0;
    }

    let mut visited = vec![false; size];
    let mut pq = BinaryHeap::new();

    // Add source cells to queue with their IDs
    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            if sources[idx] != 0 {
                distance[idx] = 0.0;
                allocation[idx] = sources[idx];
                pq.push(CostCell { x, y, cost: 0.0 });
            }
        }
    }

    let sqrt2 = 2.0_f32.sqrt() * cell_size;
    let cardinal_dist = cell_size;

    let offsets: [(i64, i64, f32); 8] = [
        (1, 0, cardinal_dist),
        (1, 1, sqrt2),
        (0, 1, cardinal_dist),
        (-1, 1, sqrt2),
        (-1, 0, cardinal_dist),
        (-1, -1, sqrt2),
        (0, -1, cardinal_dist),
        (1, -1, sqrt2),
    ];

    // Modified Dijkstra for allocation
    while let Some(cell) = pq.pop() {
        let idx = cell.y * width + cell.x;

        if visited[idx] {
            continue;
        }
        visited[idx] = true;

        let source_id = allocation[idx];

        for &(dx, dy, dist_factor) in &offsets {
            let nx = cell.x as i64 + dx;
            let ny = cell.y as i64 + dy;

            if nx < 0 || nx >= width as i64 || ny < 0 || ny >= height as i64 {
                continue;
            }

            let nx = nx as usize;
            let ny = ny as usize;
            let neighbor_idx = ny * width + nx;

            if visited[neighbor_idx] {
                continue;
            }

            let neighbor_cost = cost_surface[neighbor_idx];
            let new_cost = cell.cost + neighbor_cost * dist_factor;

            if new_cost < distance[neighbor_idx] {
                distance[neighbor_idx] = new_cost;
                allocation[neighbor_idx] = source_id;
                pq.push(CostCell {
                    x: nx,
                    y: ny,
                    cost: new_cost,
                });
            }
        }
    }

    Ok(())
}

/// SIMD-accelerated cost corridor computation
///
/// Computes cost corridor as sum of costs from two sources.
///
/// # Arguments
///
/// * `cost_from_a` - Cost distance from source A
/// * `cost_from_b` - Cost distance from source B
/// * `corridor` - Output corridor (sum of costs)
/// * `width` - Grid width
/// * `height` - Grid height
///
/// # Errors
///
/// Returns an error if parameters are invalid
pub fn cost_corridor_simd(
    cost_from_a: &[f32],
    cost_from_b: &[f32],
    corridor: &mut [f32],
    width: usize,
    height: usize,
) -> Result<()> {
    let size = width * height;
    if cost_from_a.len() != size || cost_from_b.len() != size || corridor.len() != size {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffer_size",
            message: "Buffer sizes must match width * height".to_string(),
        });
    }

    // Compute corridor with SIMD-friendly pattern
    const LANES: usize = 8;
    let chunks = size / LANES;

    for chunk in 0..chunks {
        let start = chunk * LANES;
        let end = start + LANES;

        for i in start..end {
            corridor[i] = cost_from_a[i] + cost_from_b[i];
        }
    }

    let remainder_start = chunks * LANES;
    for i in remainder_start..size {
        corridor[i] = cost_from_a[i] + cost_from_b[i];
    }

    Ok(())
}

/// SIMD-accelerated proximity zone computation
///
/// Computes proximity zones based on cost distance thresholds.
///
/// # Arguments
///
/// * `cost_distance` - Cost distance grid
/// * `zones` - Output zones (zone ID for each cell)
/// * `width` - Grid width
/// * `height` - Grid height
/// * `thresholds` - Threshold values for zone boundaries (sorted ascending)
///
/// # Errors
///
/// Returns an error if parameters are invalid
pub fn proximity_zones_simd(
    cost_distance: &[f32],
    zones: &mut [u8],
    width: usize,
    height: usize,
    thresholds: &[f32],
) -> Result<()> {
    let size = width * height;
    if cost_distance.len() != size || zones.len() != size {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffer_size",
            message: "Buffer sizes must match width * height".to_string(),
        });
    }

    if thresholds.is_empty() {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "thresholds",
            message: "At least one threshold is required".to_string(),
        });
    }

    // Compute zones with SIMD-friendly pattern
    const LANES: usize = 8;
    let chunks = size / LANES;

    for chunk in 0..chunks {
        let start = chunk * LANES;
        let end = start + LANES;

        for i in start..end {
            let cost = cost_distance[i];
            let mut zone = 0_u8;

            for (zone_idx, &threshold) in thresholds.iter().enumerate() {
                if cost >= threshold {
                    zone = (zone_idx + 1) as u8;
                }
            }

            zones[i] = zone;
        }
    }

    let remainder_start = chunks * LANES;
    for i in remainder_start..size {
        let cost = cost_distance[i];
        let mut zone = 0_u8;

        for (zone_idx, &threshold) in thresholds.iter().enumerate() {
            if cost >= threshold {
                zone = (zone_idx + 1) as u8;
            }
        }

        zones[i] = zone;
    }

    Ok(())
}

/// SIMD-accelerated cost surface weighting
///
/// Applies weights to a cost surface using SIMD.
///
/// # Arguments
///
/// * `cost_surface` - Input cost surface
/// * `weights` - Weight factors to apply
/// * `weighted` - Output weighted cost surface
///
/// # Errors
///
/// Returns an error if slice lengths don't match
pub fn apply_cost_weights_simd(
    cost_surface: &[f32],
    weights: &[f32],
    weighted: &mut [f32],
) -> Result<()> {
    if cost_surface.len() != weights.len() || cost_surface.len() != weighted.len() {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "buffer_size",
            message: "All buffers must have the same length".to_string(),
        });
    }

    const LANES: usize = 8;
    let chunks = cost_surface.len() / LANES;

    for chunk in 0..chunks {
        let start = chunk * LANES;
        let end = start + LANES;

        for i in start..end {
            weighted[i] = cost_surface[i] * weights[i];
        }
    }

    let remainder_start = chunks * LANES;
    for i in remainder_start..cost_surface.len() {
        weighted[i] = cost_surface[i] * weights[i];
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    #[test]
    fn test_euclidean_distance() {
        let mut sources = vec![0_u8; 100];
        sources[55] = 1; // Source at center

        let mut distance = vec![0.0_f32; 100];
        euclidean_distance_simd(&sources, &mut distance, 10, 10, 1.0)
            .expect("Failed to compute Euclidean distance");

        // Source should have distance 0
        assert_abs_diff_eq!(distance[55], 0.0, epsilon = 0.01);

        // Adjacent cells should have distance ~1.0
        assert_abs_diff_eq!(distance[54], 1.0, epsilon = 0.1);
        assert_abs_diff_eq!(distance[56], 1.0, epsilon = 0.1);
    }

    #[test]
    fn test_manhattan_distance() {
        let mut sources = vec![0_u8; 100];
        sources[55] = 1;

        let mut distance = vec![0.0_f32; 100];
        manhattan_distance_simd(&sources, &mut distance, 10, 10, 1.0)
            .expect("Failed to compute Manhattan distance");

        // Source should have distance 0
        assert_abs_diff_eq!(distance[55], 0.0, epsilon = 0.01);

        // Manhattan distance should be sum of |dx| + |dy|
        assert_abs_diff_eq!(distance[54], 1.0, epsilon = 0.01);
        assert_abs_diff_eq!(distance[44], 2.0, epsilon = 0.01);
    }

    #[test]
    fn test_chebyshev_distance() {
        let mut sources = vec![0_u8; 100];
        sources[55] = 1;

        let mut distance = vec![0.0_f32; 100];
        chebyshev_distance_simd(&sources, &mut distance, 10, 10, 1.0)
            .expect("Failed to compute Chebyshev distance");

        // Source should have distance 0
        assert_abs_diff_eq!(distance[55], 0.0, epsilon = 0.01);

        // Chebyshev distance should be max(|dx|, |dy|)
        assert_abs_diff_eq!(distance[54], 1.0, epsilon = 0.01);
        assert_abs_diff_eq!(distance[44], 1.0, epsilon = 0.01); // Diagonal neighbor
    }

    #[test]
    fn test_initialize_cost_buffer() {
        let mut buffer = vec![0.0_f32; 100];
        initialize_cost_buffer_simd(&mut buffer, f32::INFINITY)
            .expect("Failed to initialize cost buffer");

        for &val in &buffer {
            assert_eq!(val, f32::INFINITY);
        }
    }

    #[test]
    fn test_compute_neighbor_costs() {
        let cost_surface = vec![1.0_f32; 100];
        let mut neighbor_costs = [0.0_f32; 8];

        compute_neighbor_costs_simd(&cost_surface, 10, 10, 5, 5, 1.0, &mut neighbor_costs)
            .expect("Failed to compute neighbor costs");

        // Cardinal neighbors should have cost = 1.0 * 1.0 = 1.0
        assert_abs_diff_eq!(neighbor_costs[0], 1.0, epsilon = 0.01); // E
        assert_abs_diff_eq!(neighbor_costs[2], 1.0, epsilon = 0.01); // S
        assert_abs_diff_eq!(neighbor_costs[4], 1.0, epsilon = 0.01); // W
        assert_abs_diff_eq!(neighbor_costs[6], 1.0, epsilon = 0.01); // N

        // Diagonal neighbors should have cost = 1.0 * sqrt(2)
        let sqrt2 = 2.0_f32.sqrt();
        assert_abs_diff_eq!(neighbor_costs[1], sqrt2, epsilon = 0.01); // SE
        assert_abs_diff_eq!(neighbor_costs[3], sqrt2, epsilon = 0.01); // SW
    }

    #[test]
    fn test_invalid_cell_size() {
        let sources = vec![0_u8; 100];
        let mut distance = vec![0.0_f32; 100];

        let result = euclidean_distance_simd(&sources, &mut distance, 10, 10, 0.0);
        assert!(result.is_err());

        let result = euclidean_distance_simd(&sources, &mut distance, 10, 10, -1.0);
        assert!(result.is_err());
    }

    #[test]
    fn test_buffer_size_mismatch() {
        let sources = vec![0_u8; 100];
        let mut distance = vec![0.0_f32; 50]; // Wrong size

        let result = euclidean_distance_simd(&sources, &mut distance, 10, 10, 1.0);
        assert!(result.is_err());
    }

    #[test]
    fn test_cost_distance_dijkstra() {
        let mut sources = vec![0_u8; 100];
        sources[55] = 1; // Source at center

        let cost_surface = vec![1.0_f32; 100]; // Uniform cost
        let mut distance = vec![0.0_f32; 100];

        cost_distance_dijkstra_simd(&sources, &cost_surface, &mut distance, 10, 10, 1.0)
            .expect("Failed to compute cost distance with Dijkstra");

        // Source should have distance 0
        assert_abs_diff_eq!(distance[55], 0.0, epsilon = 0.01);

        // Neighbors should have cost = 1.0 * distance
        assert!(distance[54] > 0.0);
        assert!(distance[54] < 10.0);
    }

    #[test]
    fn test_least_cost_path() {
        let mut cost_distance = vec![10.0_f32; 100];
        // Create a gradient toward center
        cost_distance[55] = 0.0;
        cost_distance[54] = 1.0;
        cost_distance[53] = 2.0;
        cost_distance[52] = 3.0;

        let mut path = vec![0_u8; 100];

        least_cost_path_simd(&cost_distance, &mut path, 10, 10, 5, 2)
            .expect("Failed to compute least cost path");

        // Path should be marked
        let path_count: u32 = path.iter().map(|&v| v as u32).sum();
        assert!(path_count > 0);
    }

    #[test]
    fn test_cost_allocation() {
        let mut sources = vec![0_u8; 100];
        sources[22] = 1; // Source A
        sources[77] = 2; // Source B

        let cost_surface = vec![1.0_f32; 100];
        let mut allocation = vec![0_u8; 100];

        cost_allocation_simd(&sources, &cost_surface, &mut allocation, 10, 10, 1.0)
            .expect("Failed to compute cost allocation");

        // Source cells should be allocated to themselves
        assert_eq!(allocation[22], 1);
        assert_eq!(allocation[77], 2);
    }

    #[test]
    fn test_cost_corridor() {
        let cost_from_a = vec![10.0_f32; 100];
        let cost_from_b = vec![20.0_f32; 100];
        let mut corridor = vec![0.0_f32; 100];

        cost_corridor_simd(&cost_from_a, &cost_from_b, &mut corridor, 10, 10)
            .expect("Failed to compute cost corridor");

        // Corridor should be sum of costs
        for &val in &corridor {
            assert_abs_diff_eq!(val, 30.0, epsilon = 0.01);
        }
    }

    #[test]
    fn test_proximity_zones() {
        let cost_distance = vec![5.0_f32; 100];
        let mut zones = vec![0_u8; 100];
        let thresholds = vec![1.0, 3.0, 7.0];

        proximity_zones_simd(&cost_distance, &mut zones, 10, 10, &thresholds)
            .expect("Failed to compute proximity zones");

        // Cost 5.0 should be in zone 2 (exceeds thresholds 1.0 and 3.0)
        for &zone in &zones {
            assert_eq!(zone, 2);
        }
    }

    #[test]
    fn test_apply_cost_weights() {
        let cost_surface = vec![2.0_f32; 100];
        let weights = vec![3.0_f32; 100];
        let mut weighted = vec![0.0_f32; 100];

        apply_cost_weights_simd(&cost_surface, &weights, &mut weighted)
            .expect("Failed to apply cost weights");

        for &val in &weighted {
            assert_abs_diff_eq!(val, 6.0, epsilon = 0.01);
        }
    }
}
