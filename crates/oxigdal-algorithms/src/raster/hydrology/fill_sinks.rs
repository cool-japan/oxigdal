//! Depression / sink filling algorithms for hydrological conditioning
//!
//! Implements:
//!   - Wang & Liu (2006) priority-flood algorithm (fast, O(N log N))
//!   - Planchon & Darboux (2001) iterative algorithm (simple, O(N^2) worst)
//!   - Breach depressions (Lindsay & Dhun, 2015 approach)
//!   - Hybrid: fill then optionally breach
//!
//! All algorithms produce a hydrologically conditioned DEM where every cell
//! can drain to the grid boundary.

use crate::error::{AlgorithmError, Result};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::collections::VecDeque;

// ---------------------------------------------------------------------------
// Neighbour offsets (8-connected)
// ---------------------------------------------------------------------------

const DX: [i64; 8] = [1, 1, 0, -1, -1, -1, 0, 1];
const DY: [i64; 8] = [0, 1, 1, 1, 0, -1, -1, -1];

#[inline]
fn in_bounds(x: i64, y: i64, w: u64, h: u64) -> bool {
    x >= 0 && y >= 0 && (x as u64) < w && (y as u64) < h
}

fn get_neighbors(x: u64, y: u64, w: u64, h: u64) -> Vec<(u64, u64)> {
    let mut out = Vec::with_capacity(8);
    for i in 0..8 {
        let nx = x as i64 + DX[i];
        let ny = y as i64 + DY[i];
        if in_bounds(nx, ny, w, h) {
            out.push((nx as u64, ny as u64));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Priority-queue cell for Wang & Liu
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, PartialEq)]
struct PqCell {
    x: u64,
    y: u64,
    elevation: f64,
}

impl Eq for PqCell {}

impl PartialOrd for PqCell {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PqCell {
    fn cmp(&self, other: &Self) -> Ordering {
        // Min-heap: lower elevation first, break ties by (y, x)
        match other.elevation.partial_cmp(&self.elevation) {
            Some(Ordering::Equal) | None => {}
            Some(ord) => return ord,
        }
        match self.y.cmp(&other.y) {
            Ordering::Equal => self.x.cmp(&other.x),
            ord => ord,
        }
    }
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Sink filling method
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillMethod {
    /// Wang & Liu (2006) priority-flood
    WangLiu,
    /// Planchon & Darboux (2001) iterative
    PlanchonDarboux,
}

/// Configuration for sink filling
#[derive(Debug, Clone)]
pub struct FillSinksConfig {
    /// Algorithm to use
    pub method: FillMethod,
    /// Small elevation increment to impose drainage on flat surfaces.
    /// Set to 0.0 for exact filling (creates flats).
    pub epsilon: f64,
    /// Maximum fill depth. Depressions deeper than this are left unfilled.
    /// Use `f64::INFINITY` for no limit.
    pub max_fill_depth: f64,
    /// If true, attempt to breach (carve) depressions before filling.
    pub breach_first: bool,
    /// Maximum breach path length (cells). Only used when `breach_first = true`.
    pub max_breach_length: usize,
}

impl Default for FillSinksConfig {
    fn default() -> Self {
        Self {
            method: FillMethod::WangLiu,
            epsilon: 1e-5,
            max_fill_depth: f64::INFINITY,
            breach_first: false,
            max_breach_length: 50,
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Fills sinks in a DEM using the Wang & Liu (2006) priority-flood algorithm.
///
/// This is the recommended default. `epsilon` controls the small elevation
/// increment that ensures drainage across filled flats.
///
/// # Errors
///
/// Returns an error if pixel access fails.
pub fn fill_sinks(dem: &RasterBuffer, epsilon: f64) -> Result<RasterBuffer> {
    let cfg = FillSinksConfig {
        epsilon,
        ..Default::default()
    };
    fill_sinks_cfg(dem, &cfg)
}

/// Fills sinks with full configuration.
///
/// # Errors
///
/// Returns an error if pixel access fails.
pub fn fill_sinks_cfg(dem: &RasterBuffer, cfg: &FillSinksConfig) -> Result<RasterBuffer> {
    let mut working = dem.clone();

    // Optional breach step
    if cfg.breach_first {
        working = breach_depressions(&working, cfg.max_breach_length)?;
    }

    match cfg.method {
        FillMethod::WangLiu => wang_liu_fill(&working, cfg.epsilon, cfg.max_fill_depth),
        FillMethod::PlanchonDarboux => {
            planchon_darboux_fill(&working, cfg.epsilon, cfg.max_fill_depth)
        }
    }
}

/// Identifies sinks (local minima) in a DEM.
///
/// Returns a binary mask: 1.0 = sink cell, 0.0 = non-sink.
///
/// A cell is a sink if it is lower than or equal to all of its neighbours
/// (and not on the grid boundary).
///
/// # Errors
///
/// Returns an error if pixel access fails.
pub fn identify_sinks(dem: &RasterBuffer) -> Result<RasterBuffer> {
    let w = dem.width();
    let h = dem.height();
    let mut sinks = RasterBuffer::zeros(w, h, RasterDataType::UInt8);

    for y in 1..(h.saturating_sub(1)) {
        for x in 1..(w.saturating_sub(1)) {
            let center = dem.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            let neighbors = get_neighbors(x, y, w, h);
            let is_sink = neighbors
                .iter()
                .all(|&(nx, ny)| dem.get_pixel(nx, ny).map(|n| center <= n).unwrap_or(false));
            if is_sink {
                sinks.set_pixel(x, y, 1.0).map_err(AlgorithmError::Core)?;
            }
        }
    }

    Ok(sinks)
}

/// Identifies sinks and returns the fill depth at each cell.
///
/// This computes `filled_dem - original_dem` so you can see how much each
/// cell was raised.
///
/// # Errors
///
/// Returns an error if pixel access fails.
pub fn compute_fill_depth(dem: &RasterBuffer, epsilon: f64) -> Result<RasterBuffer> {
    let filled = fill_sinks(dem, epsilon)?;
    let w = dem.width();
    let h = dem.height();
    let mut depth = RasterBuffer::zeros(w, h, RasterDataType::Float64);
    for y in 0..h {
        for x in 0..w {
            let orig = dem.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            let fill = filled.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            let d = (fill - orig).max(0.0);
            depth.set_pixel(x, y, d).map_err(AlgorithmError::Core)?;
        }
    }
    Ok(depth)
}

// ---------------------------------------------------------------------------
// Wang & Liu (2006) priority-flood
// ---------------------------------------------------------------------------

/// Wang & Liu (2006) priority-flood sink-filling algorithm.
///
/// 1. Seed the priority queue with boundary cells.
/// 2. Pop the lowest-elevation cell, examine unvisited neighbours.
/// 3. If a neighbour is lower, raise it to current + epsilon.
/// 4. Push the neighbour (with possibly adjusted elevation) onto the queue.
fn wang_liu_fill(dem: &RasterBuffer, epsilon: f64, max_depth: f64) -> Result<RasterBuffer> {
    let w = dem.width();
    let h = dem.height();
    let mut filled = dem.clone();
    let n = (w * h) as usize;
    let mut visited = vec![false; n];
    let mut pq = BinaryHeap::new();

    // Seed boundary
    for y in 0..h {
        for x in 0..w {
            if x == 0 || x == w - 1 || y == 0 || y == h - 1 {
                let elev = dem.get_pixel(x, y).map_err(AlgorithmError::Core)?;
                pq.push(PqCell {
                    x,
                    y,
                    elevation: elev,
                });
                visited[(y * w + x) as usize] = true;
            }
        }
    }

    while let Some(cell) = pq.pop() {
        for i in 0..8 {
            let nx = cell.x as i64 + DX[i];
            let ny = cell.y as i64 + DY[i];
            if !in_bounds(nx, ny, w, h) {
                continue;
            }
            let nxu = nx as u64;
            let nyu = ny as u64;
            let nidx = (nyu * w + nxu) as usize;

            if visited[nidx] {
                continue;
            }
            visited[nidx] = true;

            let original = dem.get_pixel(nxu, nyu).map_err(AlgorithmError::Core)?;
            let neighbour_elev = filled.get_pixel(nxu, nyu).map_err(AlgorithmError::Core)?;
            let min_drain = cell.elevation + epsilon;

            if neighbour_elev < min_drain {
                let fill_amount = min_drain - original;
                let new_elev = if fill_amount <= max_depth {
                    min_drain
                } else {
                    // Exceeds max fill depth: leave original
                    neighbour_elev
                };
                filled
                    .set_pixel(nxu, nyu, new_elev)
                    .map_err(AlgorithmError::Core)?;
                pq.push(PqCell {
                    x: nxu,
                    y: nyu,
                    elevation: new_elev,
                });
            } else {
                pq.push(PqCell {
                    x: nxu,
                    y: nyu,
                    elevation: neighbour_elev,
                });
            }
        }
    }

    Ok(filled)
}

// ---------------------------------------------------------------------------
// Planchon & Darboux (2001)
// ---------------------------------------------------------------------------

/// Planchon & Darboux (2001) iterative sink-filling algorithm.
///
/// 1. Set all interior cells to a very high value.
/// 2. Iteratively lower cells toward the original DEM while maintaining
///    drainage connectivity.
/// 3. Repeat until no cell changes.
fn planchon_darboux_fill(dem: &RasterBuffer, epsilon: f64, max_depth: f64) -> Result<RasterBuffer> {
    let w = dem.width();
    let h = dem.height();
    let n = (w * h) as usize;

    // Read original elevations
    let mut orig = vec![0.0_f64; n];
    let mut filled = vec![0.0_f64; n];

    let big = f64::MAX / 2.0;

    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) as usize;
            let elev = dem.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            orig[idx] = elev;

            if x == 0 || x == w - 1 || y == 0 || y == h - 1 {
                filled[idx] = elev; // boundary stays
            } else {
                filled[idx] = big; // interior starts high
            }
        }
    }

    // Iterative lowering
    let mut changed = true;
    while changed {
        changed = false;
        for y in 1..(h - 1) {
            for x in 1..(w - 1) {
                let idx = (y * w + x) as usize;
                if (filled[idx] - orig[idx]).abs() < f64::EPSILON {
                    continue; // already at original
                }

                for i in 0..8 {
                    let nx = x as i64 + DX[i];
                    let ny = y as i64 + DY[i];
                    if !in_bounds(nx, ny, w, h) {
                        continue;
                    }
                    let nidx = (ny as u64 * w + nx as u64) as usize;

                    // If original is already above any neighbour + epsilon, use original
                    if orig[idx] >= filled[nidx] + epsilon {
                        if filled[idx] > orig[idx] {
                            filled[idx] = orig[idx];
                            changed = true;
                        }
                    }
                    // Otherwise lower to neighbour + epsilon if that helps
                    else {
                        let candidate = filled[nidx] + epsilon;
                        if candidate < filled[idx] {
                            let depth = candidate - orig[idx];
                            if depth <= max_depth {
                                filled[idx] = candidate;
                                changed = true;
                            }
                        }
                    }
                }
            }
        }
    }

    // Write result
    let mut result = dem.clone();
    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) as usize;
            result
                .set_pixel(x, y, filled[idx])
                .map_err(AlgorithmError::Core)?;
        }
    }
    Ok(result)
}

// ---------------------------------------------------------------------------
// Breach depressions (Lindsay & Dhun, 2015 style)
// ---------------------------------------------------------------------------

/// Breaches (carves through) depressions by finding the shortest path from
/// each sink to a lower cell on the grid, then lowering elevations along
/// the path.
///
/// `max_path_length` limits how far to search.
///
/// # Errors
///
/// Returns an error if pixel access fails.
pub fn breach_depressions(dem: &RasterBuffer, max_path_length: usize) -> Result<RasterBuffer> {
    let w = dem.width();
    let h = dem.height();
    let mut result = dem.clone();

    // Find sinks
    let sink_mask = identify_sinks(dem)?;

    for y in 1..(h.saturating_sub(1)) {
        for x in 1..(w.saturating_sub(1)) {
            let is_sink = sink_mask.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            if is_sink < 0.5 {
                continue;
            }

            // BFS from sink to find nearest lower cell or boundary
            if let Some(path) = find_breach_path(&result, x, y, max_path_length)? {
                // Carve the path
                apply_breach_path(&mut result, &path)?;
            }
        }
    }

    Ok(result)
}

/// BFS from a sink cell to find the shortest path to a cell that is lower
/// than the sink. Returns the path as a list of (x, y) coordinates.
fn find_breach_path(
    dem: &RasterBuffer,
    sx: u64,
    sy: u64,
    max_len: usize,
) -> Result<Option<Vec<(u64, u64)>>> {
    let w = dem.width();
    let h = dem.height();
    let n = (w * h) as usize;
    let sink_elev = dem.get_pixel(sx, sy).map_err(AlgorithmError::Core)?;

    let mut visited = vec![false; n];
    let mut parent: Vec<Option<usize>> = vec![None; n];
    let mut dist = vec![0usize; n];

    let start = (sy * w + sx) as usize;
    visited[start] = true;

    let mut queue = VecDeque::new();
    queue.push_back((sx, sy));

    while let Some((cx, cy)) = queue.pop_front() {
        let cidx = (cy * w + cx) as usize;
        if dist[cidx] >= max_len {
            continue;
        }

        for i in 0..8 {
            let nx = cx as i64 + DX[i];
            let ny = cy as i64 + DY[i];
            if !in_bounds(nx, ny, w, h) {
                continue;
            }
            let nxu = nx as u64;
            let nyu = ny as u64;
            let nidx = (nyu * w + nxu) as usize;

            if visited[nidx] {
                continue;
            }
            visited[nidx] = true;
            parent[nidx] = Some(cidx);
            dist[nidx] = dist[cidx] + 1;

            let ne = dem.get_pixel(nxu, nyu).map_err(AlgorithmError::Core)?;

            // Found a lower cell or boundary
            if ne < sink_elev || nxu == 0 || nxu == w - 1 || nyu == 0 || nyu == h - 1 {
                // Reconstruct path
                let mut path = Vec::new();
                let mut cur = nidx;
                path.push((nxu, nyu));
                while let Some(p) = parent[cur] {
                    let px = (p as u64) % w;
                    let py = (p as u64) / w;
                    path.push((px, py));
                    if cur == start {
                        break;
                    }
                    cur = p;
                }
                path.reverse();
                return Ok(Some(path));
            }

            queue.push_back((nxu, nyu));
        }
    }

    Ok(None) // No path found within max_len
}

/// Lowers elevations along a breach path to ensure monotonic drainage.
fn apply_breach_path(dem: &mut RasterBuffer, path: &[(u64, u64)]) -> Result<()> {
    if path.len() < 2 {
        return Ok(());
    }

    // Start elevation (at the path source)
    let start_elev = dem
        .get_pixel(path[0].0, path[0].1)
        .map_err(AlgorithmError::Core)?;
    let end_elev = dem
        .get_pixel(path[path.len() - 1].0, path[path.len() - 1].1)
        .map_err(AlgorithmError::Core)?;

    let steps = path.len() as f64;

    for (i, &(px, py)) in path.iter().enumerate() {
        let frac = i as f64 / (steps - 1.0);
        let target = start_elev + frac * (end_elev - start_elev);
        let current = dem.get_pixel(px, py).map_err(AlgorithmError::Core)?;

        if current > target {
            dem.set_pixel(px, py, target)
                .map_err(AlgorithmError::Core)?;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_flat_with_pit(size: u64, pit_depth: f64) -> RasterBuffer {
        let mut dem = RasterBuffer::zeros(size, size, RasterDataType::Float32);
        for y in 0..size {
            for x in 0..size {
                let _ = dem.set_pixel(x, y, 10.0);
            }
        }
        let mid = size / 2;
        let _ = dem.set_pixel(mid, mid, 10.0 - pit_depth);
        dem
    }

    fn make_bowl(size: u64) -> RasterBuffer {
        let mut dem = RasterBuffer::zeros(size, size, RasterDataType::Float32);
        let center = size as f64 / 2.0;
        for y in 0..size {
            for x in 0..size {
                let dx = x as f64 - center;
                let dy = y as f64 - center;
                let dist = (dx * dx + dy * dy).sqrt();
                let _ = dem.set_pixel(x, y, dist);
            }
        }
        dem
    }

    // --- Wang & Liu tests ---

    #[test]
    fn test_wang_liu_fills_pit() {
        let dem = make_flat_with_pit(7, 5.0);
        let filled = fill_sinks(&dem, 0.001);
        assert!(filled.is_ok());
        let filled = filled.expect("should succeed");

        let orig = dem.get_pixel(3, 3).expect("should succeed");
        let after = filled.get_pixel(3, 3).expect("should succeed");
        assert!(
            after > orig,
            "Pit should be filled: orig={orig}, after={after}"
        );
    }

    #[test]
    fn test_wang_liu_preserves_slope() {
        // Simple slope: elevation = x. No sinks.
        let mut dem = RasterBuffer::zeros(7, 7, RasterDataType::Float32);
        for y in 0..7u64 {
            for x in 0..7u64 {
                let _ = dem.set_pixel(x, y, x as f64);
            }
        }
        let filled = fill_sinks(&dem, 0.001).expect("should succeed");
        // Interior cells should be largely unchanged
        for y in 1..6u64 {
            for x in 1..6u64 {
                let orig = dem.get_pixel(x, y).expect("should succeed");
                let f = filled.get_pixel(x, y).expect("should succeed");
                assert!(
                    (f - orig).abs() < 0.1,
                    "Cell ({x},{y}): orig={orig}, filled={f}"
                );
            }
        }
    }

    #[test]
    fn test_wang_liu_max_depth() {
        let dem = make_flat_with_pit(7, 20.0);
        let cfg = FillSinksConfig {
            method: FillMethod::WangLiu,
            epsilon: 0.001,
            max_fill_depth: 5.0,
            ..Default::default()
        };
        let filled = fill_sinks_cfg(&dem, &cfg).expect("should succeed");
        let after = filled.get_pixel(3, 3).expect("should succeed");
        let orig = dem.get_pixel(3, 3).expect("should succeed");
        // Fill depth should not exceed 5 from original
        let depth = after - orig;
        assert!(
            depth <= 5.1,
            "Fill depth {depth} should not exceed max of 5.0 + epsilon"
        );
    }

    // --- Planchon & Darboux tests ---

    #[test]
    fn test_planchon_darboux_fills_pit() {
        let dem = make_flat_with_pit(7, 5.0);
        let cfg = FillSinksConfig {
            method: FillMethod::PlanchonDarboux,
            epsilon: 0.001,
            ..Default::default()
        };
        let filled = fill_sinks_cfg(&dem, &cfg);
        assert!(filled.is_ok());
        let filled = filled.expect("should succeed");

        let orig = dem.get_pixel(3, 3).expect("should succeed");
        let after = filled.get_pixel(3, 3).expect("should succeed");
        assert!(after > orig, "Planchon-Darboux should fill pit");
    }

    #[test]
    fn test_planchon_darboux_bowl() {
        let dem = make_bowl(9);
        let cfg = FillSinksConfig {
            method: FillMethod::PlanchonDarboux,
            epsilon: 0.001,
            ..Default::default()
        };
        let filled = fill_sinks_cfg(&dem, &cfg);
        assert!(filled.is_ok());
    }

    // --- Sink identification tests ---

    #[test]
    fn test_identify_sinks() {
        let dem = make_flat_with_pit(7, 5.0);
        let sinks = identify_sinks(&dem).expect("should succeed");
        let center = sinks.get_pixel(3, 3).expect("should succeed");
        assert!(center > 0.0, "Center pit should be identified as a sink");
    }

    #[test]
    fn test_no_sinks_on_slope() {
        let mut dem = RasterBuffer::zeros(7, 7, RasterDataType::Float32);
        for y in 0..7u64 {
            for x in 0..7u64 {
                let _ = dem.set_pixel(x, y, (6 - x) as f64);
            }
        }
        let sinks = identify_sinks(&dem).expect("should succeed");
        // No interior sinks on a monotonic slope
        for y in 1..6u64 {
            for x in 1..6u64 {
                let v = sinks.get_pixel(x, y).expect("should succeed");
                assert!(v < 0.5, "Cell ({x},{y}) should not be a sink on a slope");
            }
        }
    }

    // --- Breach depressions tests ---

    #[test]
    fn test_breach_simple_pit() {
        let dem = make_flat_with_pit(9, 3.0);
        let breached = breach_depressions(&dem, 20);
        assert!(breached.is_ok());
    }

    #[test]
    fn test_breach_max_path_limit() {
        let dem = make_flat_with_pit(9, 3.0);
        // Very short max path -- may not reach boundary
        let breached = breach_depressions(&dem, 1);
        assert!(breached.is_ok());
    }

    // --- Fill depth tests ---

    #[test]
    fn test_compute_fill_depth() {
        let dem = make_flat_with_pit(7, 5.0);
        let depth = compute_fill_depth(&dem, 0.001);
        assert!(depth.is_ok());
        let depth = depth.expect("should succeed");

        let center_depth = depth.get_pixel(3, 3).expect("should succeed");
        assert!(
            center_depth > 0.0,
            "Pit cell should have positive fill depth"
        );

        // Boundary cells should have zero depth
        let boundary_depth = depth.get_pixel(0, 0).expect("should succeed");
        assert!(
            boundary_depth < 0.01,
            "Boundary cell should have ~zero fill depth"
        );
    }

    // --- Hybrid test ---

    #[test]
    fn test_breach_then_fill() {
        let dem = make_flat_with_pit(9, 5.0);
        let cfg = FillSinksConfig {
            method: FillMethod::WangLiu,
            epsilon: 0.001,
            breach_first: true,
            max_breach_length: 20,
            ..Default::default()
        };
        let filled = fill_sinks_cfg(&dem, &cfg);
        assert!(filled.is_ok());
    }
}
