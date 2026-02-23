//! Watershed delineation algorithms
//!
//! Implements:
//!   - Pour-point watershed delineation (upstream tracing from outlets)
//!   - Sub-basin delineation (automatic basins from stream junctions)
//!   - Watershed hierarchy (nesting relationships between sub-basins)
//!   - Snap pour points to high-accumulation cells

use crate::error::{AlgorithmError, Result};
use crate::raster::hydrology::flow_direction::{
    D8_DX, D8_DY, D8Direction, compute_d8_flow_direction,
};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;
use std::collections::VecDeque;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

#[inline]
fn in_bounds(x: i64, y: i64, w: u64, h: u64) -> bool {
    x >= 0 && y >= 0 && (x as u64) < w && (y as u64) < h
}

/// Checks if cell at (nx, ny) flows into cell (x, y) according to `flow_dir`.
fn flows_into(
    nx: u64,
    ny: u64,
    x: u64,
    y: u64,
    flow_dir: &RasterBuffer,
    w: u64,
    h: u64,
) -> Result<bool> {
    let code = flow_dir.get_pixel(nx, ny).map_err(AlgorithmError::Core)? as u8;
    if let Some(dir) = D8Direction::from_code(code) {
        let (dx, dy) = dir.offset();
        let target_x = nx as i64 + dx;
        let target_y = ny as i64 + dy;
        if in_bounds(target_x, target_y, w, h) {
            return Ok(target_x as u64 == x && target_y as u64 == y);
        }
    }
    Ok(false)
}

/// Returns all cells that flow into (x, y).
fn find_upstream(
    x: u64,
    y: u64,
    flow_dir: &RasterBuffer,
    w: u64,
    h: u64,
) -> Result<Vec<(u64, u64)>> {
    let mut ups = Vec::new();
    for i in 0..8 {
        let nx = x as i64 + D8_DX[i];
        let ny = y as i64 + D8_DY[i];
        if !in_bounds(nx, ny, w, h) {
            continue;
        }
        let nxu = nx as u64;
        let nyu = ny as u64;
        if flows_into(nxu, nyu, x, y, flow_dir, w, h)? {
            ups.push((nxu, nyu));
        }
    }
    Ok(ups)
}

// ---------------------------------------------------------------------------
// Pour-point watershed delineation
// ---------------------------------------------------------------------------

/// Delineates watersheds from pour points.
///
/// Each pour point receives a unique watershed ID. The algorithm traces
/// upstream from each pour point through the D8 flow direction grid to find
/// all contributing cells.
///
/// # Arguments
///
/// * `dem` - Digital elevation model
/// * `pour_points` - Raster with pour point locations (non-zero = pour point)
/// * `cell_size` - Cell size in map units
///
/// # Errors
///
/// Returns an error if pixel access fails.
pub fn delineate_watersheds(
    dem: &RasterBuffer,
    pour_points: &RasterBuffer,
    cell_size: f64,
) -> Result<RasterBuffer> {
    let flow_dir = compute_d8_flow_direction(dem, cell_size)?;
    delineate_watersheds_from_fdir(&flow_dir, pour_points)
}

/// Delineates watersheds from a precomputed D8 flow direction grid.
///
/// # Errors
///
/// Returns an error if pixel access fails.
pub fn delineate_watersheds_from_fdir(
    flow_dir: &RasterBuffer,
    pour_points: &RasterBuffer,
) -> Result<RasterBuffer> {
    let w = flow_dir.width();
    let h = flow_dir.height();
    let mut watersheds = RasterBuffer::zeros(w, h, RasterDataType::Int32);

    // Collect pour points and assign IDs
    let mut pps: Vec<(u64, u64, i32)> = Vec::new();
    let mut wid = 1i32;
    for y in 0..h {
        for x in 0..w {
            let v = pour_points.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            if v > 0.0 {
                pps.push((x, y, wid));
                watersheds
                    .set_pixel(x, y, f64::from(wid))
                    .map_err(AlgorithmError::Core)?;
                wid += 1;
            }
        }
    }

    // BFS upstream from each pour point
    for &(px, py, watershed_id) in &pps {
        trace_upstream_bfs(flow_dir, &mut watersheds, px, py, watershed_id, w, h)?;
    }

    Ok(watersheds)
}

/// BFS upstream from a single seed cell, labeling all unlabeled contributing
/// cells with `watershed_id`.
fn trace_upstream_bfs(
    flow_dir: &RasterBuffer,
    watersheds: &mut RasterBuffer,
    start_x: u64,
    start_y: u64,
    watershed_id: i32,
    w: u64,
    h: u64,
) -> Result<()> {
    let mut queue = VecDeque::new();
    queue.push_back((start_x, start_y));

    while let Some((cx, cy)) = queue.pop_front() {
        let ups = find_upstream(cx, cy, flow_dir, w, h)?;
        for (ux, uy) in ups {
            let current = watersheds.get_pixel(ux, uy).map_err(AlgorithmError::Core)? as i32;
            if current == 0 {
                watersheds
                    .set_pixel(ux, uy, f64::from(watershed_id))
                    .map_err(AlgorithmError::Core)?;
                queue.push_back((ux, uy));
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Snap pour points to high-accumulation cells
// ---------------------------------------------------------------------------

/// Snaps pour point locations to the nearest cell with the highest flow
/// accumulation within a search radius.
///
/// This is useful when pour point coordinates don't exactly fall on the
/// stream channel.
///
/// # Arguments
///
/// * `pour_points` - Original pour point raster
/// * `flow_accumulation` - Flow accumulation raster
/// * `snap_radius` - Search radius in cells
///
/// # Errors
///
/// Returns an error if pixel access fails.
pub fn snap_pour_points(
    pour_points: &RasterBuffer,
    flow_accumulation: &RasterBuffer,
    snap_radius: u64,
) -> Result<RasterBuffer> {
    let w = pour_points.width();
    let h = pour_points.height();
    let mut snapped = RasterBuffer::zeros(w, h, RasterDataType::Float64);

    for y in 0..h {
        for x in 0..w {
            let v = pour_points.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            if v <= 0.0 {
                continue;
            }

            // Search within radius for highest accumulation
            let mut best_accum = f64::NEG_INFINITY;
            let mut best_x = x;
            let mut best_y = y;

            let y_min = y.saturating_sub(snap_radius);
            let y_max = (y + snap_radius).min(h - 1);
            let x_min = x.saturating_sub(snap_radius);
            let x_max = (x + snap_radius).min(w - 1);

            for sy in y_min..=y_max {
                for sx in x_min..=x_max {
                    let dx = (sx as f64 - x as f64).abs();
                    let dy = (sy as f64 - y as f64).abs();
                    let dist = (dx * dx + dy * dy).sqrt();
                    if dist > snap_radius as f64 {
                        continue;
                    }

                    let acc = flow_accumulation
                        .get_pixel(sx, sy)
                        .map_err(AlgorithmError::Core)?;
                    if acc > best_accum {
                        best_accum = acc;
                        best_x = sx;
                        best_y = sy;
                    }
                }
            }

            snapped
                .set_pixel(best_x, best_y, v)
                .map_err(AlgorithmError::Core)?;
        }
    }

    Ok(snapped)
}

// ---------------------------------------------------------------------------
// Sub-basin delineation
// ---------------------------------------------------------------------------

/// Automatically delineates sub-basins from a stream network.
///
/// Each stream junction becomes a pour point, and the upstream contributing
/// area to each junction becomes a sub-basin.
///
/// # Arguments
///
/// * `flow_dir` - D8 flow direction raster
/// * `streams` - Binary stream raster (1 = stream, 0 = non-stream)
/// * `flow_accumulation` - Flow accumulation raster
///
/// # Errors
///
/// Returns an error if pixel access fails.
pub fn delineate_sub_basins(
    flow_dir: &RasterBuffer,
    streams: &RasterBuffer,
    flow_accumulation: &RasterBuffer,
) -> Result<RasterBuffer> {
    let w = flow_dir.width();
    let h = flow_dir.height();

    // Identify junction cells and outlet cells as pour points
    let mut pour_points = RasterBuffer::zeros(w, h, RasterDataType::Float64);
    let mut wid = 1i32;

    for y in 0..h {
        for x in 0..w {
            let is_stream = streams.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            if is_stream < 0.5 {
                continue;
            }

            // Count upstream stream neighbours
            let ups = find_upstream(x, y, flow_dir, w, h)?;
            let stream_ups: Vec<_> = ups
                .into_iter()
                .filter(|&(ux, uy)| streams.get_pixel(ux, uy).map(|v| v >= 0.5).unwrap_or(false))
                .collect();

            let is_junction = stream_ups.len() >= 2;

            // Also check if this is an outlet (flows off grid or into non-stream)
            let code = flow_dir.get_pixel(x, y).map_err(AlgorithmError::Core)? as u8;
            let is_outlet = if let Some(dir) = D8Direction::from_code(code) {
                let (dx, dy) = dir.offset();
                let nx = x as i64 + dx;
                let ny = y as i64 + dy;
                if !in_bounds(nx, ny, w, h) {
                    true
                } else {
                    let ds = streams
                        .get_pixel(nx as u64, ny as u64)
                        .map_err(AlgorithmError::Core)?;
                    ds < 0.5
                }
            } else {
                true // no valid direction = outlet
            };

            if is_junction || is_outlet {
                pour_points
                    .set_pixel(x, y, f64::from(wid))
                    .map_err(AlgorithmError::Core)?;
                wid += 1;
            }
        }
    }

    delineate_watersheds_from_fdir(flow_dir, &pour_points)
}

// ---------------------------------------------------------------------------
// Watershed hierarchy
// ---------------------------------------------------------------------------

/// A node in the watershed hierarchy tree.
#[derive(Debug, Clone)]
pub struct WatershedNode {
    /// Watershed ID
    pub id: i32,
    /// Pour point location (x, y)
    pub pour_point: (u64, u64),
    /// Flow accumulation at the pour point
    pub accumulation: f64,
    /// ID of the parent (downstream) watershed, or None for the root
    pub parent_id: Option<i32>,
    /// IDs of child (upstream) sub-basins
    pub children: Vec<i32>,
}

/// Builds a watershed hierarchy from sub-basin delineation.
///
/// Each sub-basin has a pour point; by tracing downstream from each pour
/// point, we find which sub-basin it drains into (the parent).
///
/// # Errors
///
/// Returns an error if pixel access fails.
pub fn build_watershed_hierarchy(
    flow_dir: &RasterBuffer,
    sub_basins: &RasterBuffer,
    flow_accumulation: &RasterBuffer,
) -> Result<Vec<WatershedNode>> {
    let w = flow_dir.width();
    let h = flow_dir.height();

    // Collect pour points: find the cell with highest accumulation in each sub-basin
    let mut basin_pour: std::collections::HashMap<i32, (u64, u64, f64)> =
        std::collections::HashMap::new();

    for y in 0..h {
        for x in 0..w {
            let bid = sub_basins.get_pixel(x, y).map_err(AlgorithmError::Core)? as i32;
            if bid <= 0 {
                continue;
            }
            let acc = flow_accumulation
                .get_pixel(x, y)
                .map_err(AlgorithmError::Core)?;
            let entry = basin_pour.entry(bid).or_insert((x, y, acc));
            if acc > entry.2 {
                *entry = (x, y, acc);
            }
        }
    }

    // Trace from each pour point downstream to find parent basin
    let mut nodes: Vec<WatershedNode> = Vec::new();

    for (&bid, &(px, py, acc)) in &basin_pour {
        let parent_id = find_downstream_basin(px, py, bid, flow_dir, sub_basins, w, h)?;

        nodes.push(WatershedNode {
            id: bid,
            pour_point: (px, py),
            accumulation: acc,
            parent_id,
            children: Vec::new(),
        });
    }

    // Populate children
    let node_ids: Vec<(i32, Option<i32>)> = nodes.iter().map(|n| (n.id, n.parent_id)).collect();
    for (child_id, parent_id) in &node_ids {
        if let Some(pid) = parent_id {
            if let Some(parent_node) = nodes.iter_mut().find(|n| n.id == *pid) {
                parent_node.children.push(*child_id);
            }
        }
    }

    // Sort by ID
    nodes.sort_by_key(|n| n.id);
    Ok(nodes)
}

/// Traces downstream from a pour point until we enter a different basin.
fn find_downstream_basin(
    start_x: u64,
    start_y: u64,
    own_basin: i32,
    flow_dir: &RasterBuffer,
    sub_basins: &RasterBuffer,
    w: u64,
    h: u64,
) -> Result<Option<i32>> {
    let mut cx = start_x;
    let mut cy = start_y;
    let max_steps = (w * h) as usize; // safety bound

    for _ in 0..max_steps {
        let code = flow_dir.get_pixel(cx, cy).map_err(AlgorithmError::Core)? as u8;
        let dir = match D8Direction::from_code(code) {
            Some(d) => d,
            None => return Ok(None),
        };
        let (dx, dy) = dir.offset();
        let nx = cx as i64 + dx;
        let ny = cy as i64 + dy;
        if !in_bounds(nx, ny, w, h) {
            return Ok(None); // Drains off grid
        }
        let nxu = nx as u64;
        let nyu = ny as u64;
        let nbid = sub_basins
            .get_pixel(nxu, nyu)
            .map_err(AlgorithmError::Core)? as i32;
        if nbid > 0 && nbid != own_basin {
            return Ok(Some(nbid));
        }
        cx = nxu;
        cy = nyu;
    }

    Ok(None)
}

// ---------------------------------------------------------------------------
// Utility: whole-grid watershed labeling (no pour points needed)
// ---------------------------------------------------------------------------

/// Labels every cell with a watershed ID by assigning unique IDs to all
/// outlet cells (cells that drain off the grid or into pits) and then
/// tracing upstream.
///
/// This is equivalent to a full drainage basin decomposition of the DEM.
///
/// # Errors
///
/// Returns an error if pixel access fails.
pub fn label_all_watersheds(flow_dir: &RasterBuffer) -> Result<RasterBuffer> {
    let w = flow_dir.width();
    let h = flow_dir.height();
    let mut watersheds = RasterBuffer::zeros(w, h, RasterDataType::Int32);
    let mut wid = 1i32;

    // Find outlet cells
    for y in 0..h {
        for x in 0..w {
            let code = flow_dir.get_pixel(x, y).map_err(AlgorithmError::Core)? as u8;
            let is_outlet = match D8Direction::from_code(code) {
                Some(dir) => {
                    let (dx, dy) = dir.offset();
                    let nx = x as i64 + dx;
                    let ny = y as i64 + dy;
                    !in_bounds(nx, ny, w, h)
                }
                None => true, // pit or flat with no direction
            };

            if is_outlet {
                let current = watersheds.get_pixel(x, y).map_err(AlgorithmError::Core)? as i32;
                if current == 0 {
                    watersheds
                        .set_pixel(x, y, f64::from(wid))
                        .map_err(AlgorithmError::Core)?;
                    trace_upstream_bfs(flow_dir, &mut watersheds, x, y, wid, w, h)?;
                    wid += 1;
                }
            }
        }
    }

    Ok(watersheds)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raster::hydrology::flow_accumulation::compute_flow_accumulation;

    fn make_east_slope(size: u64) -> RasterBuffer {
        let mut dem = RasterBuffer::zeros(size, size, RasterDataType::Float32);
        for y in 0..size {
            for x in 0..size {
                let _ = dem.set_pixel(x, y, (size - 1 - x) as f64);
            }
        }
        dem
    }

    fn make_v_valley(size: u64) -> RasterBuffer {
        let mut dem = RasterBuffer::zeros(size, size, RasterDataType::Float32);
        let center = size / 2;
        for y in 0..size {
            for x in 0..size {
                let dx = (x as f64 - center as f64).abs();
                let dy = (size - 1 - y) as f64;
                let _ = dem.set_pixel(x, y, dx + dy);
            }
        }
        dem
    }

    fn make_two_basins(size: u64) -> RasterBuffer {
        // Two valleys side by side
        let mut dem = RasterBuffer::zeros(size, size, RasterDataType::Float32);
        let q1 = size / 4;
        let q3 = 3 * size / 4;
        for y in 0..size {
            for x in 0..size {
                let d1 = (x as f64 - q1 as f64).abs();
                let d2 = (x as f64 - q3 as f64).abs();
                let min_d = d1.min(d2);
                let dy = (size - 1 - y) as f64;
                let _ = dem.set_pixel(x, y, min_d + dy);
            }
        }
        dem
    }

    #[test]
    fn test_delineate_watersheds_simple() {
        let dem = make_east_slope(7);
        let mut pour_points = RasterBuffer::zeros(7, 7, RasterDataType::Float32);
        let _ = pour_points.set_pixel(6, 3, 1.0);

        let ws = delineate_watersheds(&dem, &pour_points, 1.0);
        assert!(ws.is_ok());
        let ws = ws.expect("should succeed");

        // Interior cells should be in watershed 1
        let center = ws.get_pixel(3, 3).expect("should succeed") as i32;
        assert!(center > 0, "Center cell should be in a watershed");
    }

    #[test]
    fn test_delineate_from_fdir() {
        let dem = make_east_slope(7);
        let flow_dir = compute_d8_flow_direction(&dem, 1.0).expect("should succeed");
        let mut pour_points = RasterBuffer::zeros(7, 7, RasterDataType::Float32);
        let _ = pour_points.set_pixel(6, 3, 1.0);

        let ws = delineate_watersheds_from_fdir(&flow_dir, &pour_points);
        assert!(ws.is_ok());
    }

    #[test]
    fn test_snap_pour_points() {
        let dem = make_v_valley(11);
        let accum = compute_flow_accumulation(&dem, 1.0).expect("should succeed");

        let mut pp = RasterBuffer::zeros(11, 11, RasterDataType::Float64);
        // Place pour point slightly off channel
        let _ = pp.set_pixel(4, 8, 1.0);

        let snapped = snap_pour_points(&pp, &accum, 3);
        assert!(snapped.is_ok());
        let snapped = snapped.expect("should succeed");

        // The snapped point should be at a cell with higher accumulation
        let mut found = false;
        for y in 0..11u64 {
            for x in 0..11u64 {
                let v = snapped.get_pixel(x, y).expect("should succeed");
                if v > 0.0 {
                    found = true;
                }
            }
        }
        assert!(found, "Should have a snapped pour point");
    }

    #[test]
    fn test_label_all_watersheds() {
        let dem = make_east_slope(7);
        let flow_dir = compute_d8_flow_direction(&dem, 1.0).expect("should succeed");
        let ws = label_all_watersheds(&flow_dir);
        assert!(ws.is_ok());
        let ws = ws.expect("should succeed");

        // All cells should be labeled
        for y in 0..7u64 {
            for x in 0..7u64 {
                let v = ws.get_pixel(x, y).expect("should succeed") as i32;
                assert!(v > 0, "Cell ({x},{y}) should be in a watershed, got {v}");
            }
        }
    }

    #[test]
    fn test_sub_basin_delineation() {
        let dem = make_v_valley(11);
        let flow_dir = compute_d8_flow_direction(&dem, 1.0).expect("should succeed");
        let accum = compute_flow_accumulation(&dem, 1.0).expect("should succeed");
        let streams = crate::raster::hydrology::stream_network::extract_stream_network(&accum, 3.0)
            .expect("should succeed");

        let sub = delineate_sub_basins(&flow_dir, &streams, &accum);
        assert!(sub.is_ok());
    }

    #[test]
    fn test_watershed_hierarchy() {
        let dem = make_v_valley(11);
        let flow_dir = compute_d8_flow_direction(&dem, 1.0).expect("should succeed");
        let accum = compute_flow_accumulation(&dem, 1.0).expect("should succeed");
        let streams = crate::raster::hydrology::stream_network::extract_stream_network(&accum, 3.0)
            .expect("should succeed");

        let sub = delineate_sub_basins(&flow_dir, &streams, &accum).expect("should succeed");
        let hierarchy = build_watershed_hierarchy(&flow_dir, &sub, &accum);
        assert!(hierarchy.is_ok());
        let hierarchy = hierarchy.expect("should succeed");

        // Should have at least one node
        assert!(
            !hierarchy.is_empty(),
            "Watershed hierarchy should have at least one node"
        );
    }

    #[test]
    fn test_two_separate_basins() {
        let dem = make_two_basins(13);
        let flow_dir = compute_d8_flow_direction(&dem, 1.0).expect("should succeed");
        let ws = label_all_watersheds(&flow_dir).expect("should succeed");

        // Should have at least 2 distinct watershed IDs
        let mut ids = std::collections::HashSet::new();
        for y in 0..13u64 {
            for x in 0..13u64 {
                let v = ws.get_pixel(x, y).expect("should succeed") as i32;
                if v > 0 {
                    ids.insert(v);
                }
            }
        }
        assert!(
            ids.len() >= 2,
            "Should have at least 2 watersheds, found {}",
            ids.len()
        );
    }
}
