//! Stream network extraction and analysis
//!
//! Implements:
//!   - Stream extraction from flow accumulation (threshold-based)
//!   - Strahler stream order (Strahler, 1957)
//!   - Shreve stream magnitude (Shreve, 1966)
//!   - Stream head identification
//!   - Stream link identification (unique ID per reach)
//!   - Stream vectorisation (raster to polyline coordinates)

use crate::error::{AlgorithmError, Result};
use crate::raster::hydrology::flow_direction::{D8_DX, D8_DY, D8Direction};
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

/// Resolves a D8 code to the downstream cell coordinates.
fn downstream_cell(x: u64, y: u64, code: u8, w: u64, h: u64) -> Option<(u64, u64)> {
    let dir = D8Direction::from_code(code)?;
    let (dx, dy) = dir.offset();
    let nx = x as i64 + dx;
    let ny = y as i64 + dy;
    if in_bounds(nx, ny, w, h) {
        Some((nx as u64, ny as u64))
    } else {
        None
    }
}

/// Returns indices of all neighbours that flow **into** cell (x, y).
fn upstream_cells(
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
        let code = flow_dir.get_pixel(nxu, nyu).map_err(AlgorithmError::Core)? as u8;
        if let Some((dx, dy)) = downstream_cell(nxu, nyu, code, w, h) {
            if dx == x && dy == y {
                ups.push((nxu, nyu));
            }
        }
    }
    Ok(ups)
}

// ---------------------------------------------------------------------------
// Stream extraction
// ---------------------------------------------------------------------------

/// Extracts a binary stream network from flow accumulation using a threshold.
///
/// Returns a raster: 1.0 where `accumulation >= threshold`, 0.0 otherwise.
///
/// # Errors
///
/// Returns an error if pixel access fails.
pub fn extract_stream_network(
    flow_accumulation: &RasterBuffer,
    threshold: f64,
) -> Result<RasterBuffer> {
    let w = flow_accumulation.width();
    let h = flow_accumulation.height();
    let mut streams = RasterBuffer::zeros(w, h, RasterDataType::UInt8);

    for y in 0..h {
        for x in 0..w {
            let v = flow_accumulation
                .get_pixel(x, y)
                .map_err(AlgorithmError::Core)?;
            if v >= threshold {
                streams.set_pixel(x, y, 1.0).map_err(AlgorithmError::Core)?;
            }
        }
    }

    Ok(streams)
}

// ---------------------------------------------------------------------------
// Stream head identification
// ---------------------------------------------------------------------------

/// Identifies stream head cells.
///
/// A stream head is a stream cell that has no upstream stream neighbours
/// (i.e. no neighbour flows into it and is also a stream cell).
///
/// Returns a binary raster: 1.0 = stream head, 0.0 otherwise.
///
/// # Errors
///
/// Returns an error if pixel access fails.
pub fn identify_stream_heads(
    streams: &RasterBuffer,
    flow_dir: &RasterBuffer,
) -> Result<RasterBuffer> {
    let w = streams.width();
    let h = streams.height();
    let mut heads = RasterBuffer::zeros(w, h, RasterDataType::UInt8);

    for y in 0..h {
        for x in 0..w {
            let is_stream = streams.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            if is_stream < 0.5 {
                continue;
            }

            let ups = upstream_cells(x, y, flow_dir, w, h)?;
            let has_upstream_stream = ups
                .iter()
                .any(|&(ux, uy)| streams.get_pixel(ux, uy).map(|v| v >= 0.5).unwrap_or(false));

            if !has_upstream_stream {
                heads.set_pixel(x, y, 1.0).map_err(AlgorithmError::Core)?;
            }
        }
    }

    Ok(heads)
}

// ---------------------------------------------------------------------------
// Strahler stream order
// ---------------------------------------------------------------------------

/// Computes Strahler stream order (Strahler, 1957).
///
/// Rules:
///   - Stream heads (no upstream tributaries) get order 1.
///   - When two streams of the *same* order join, the downstream order is
///     incremented by 1.
///   - When two streams of *different* orders join, the downstream gets
///     the higher order.
///
/// Requires a D8 flow direction grid and a binary stream mask.
///
/// # Errors
///
/// Returns an error if pixel access fails.
pub fn compute_stream_order(
    _dem: &RasterBuffer,
    flow_accumulation: &RasterBuffer,
    threshold: f64,
    _cell_size: f64,
) -> Result<RasterBuffer> {
    let w = flow_accumulation.width();
    let h = flow_accumulation.height();

    // We need a flow direction; re-derive it from the DEM passed through _dem
    // but actually we need it from _dem. Use the provided DEM.
    let flow_dir =
        crate::raster::hydrology::flow_direction::compute_d8_flow_direction(_dem, _cell_size)?;
    let streams = extract_stream_network(flow_accumulation, threshold)?;

    compute_strahler_order(&streams, &flow_dir)
}

/// Computes Strahler order from precomputed stream and flow direction rasters.
///
/// # Errors
///
/// Returns an error if pixel access fails.
pub fn compute_strahler_order(
    streams: &RasterBuffer,
    flow_dir: &RasterBuffer,
) -> Result<RasterBuffer> {
    let w = streams.width();
    let h = streams.height();
    let n = (w * h) as usize;

    let mut order = RasterBuffer::zeros(w, h, RasterDataType::Int32);
    let mut computed = vec![false; n];

    // Count upstream stream neighbours for each stream cell
    let mut upstream_count = vec![0u32; n];
    for y in 0..h {
        for x in 0..w {
            let is_stream = streams.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            if is_stream < 0.5 {
                continue;
            }
            let ups = upstream_cells(x, y, flow_dir, w, h)?;
            let count = ups
                .iter()
                .filter(|&&(ux, uy)| streams.get_pixel(ux, uy).map(|v| v >= 0.5).unwrap_or(false))
                .count();
            upstream_count[(y * w + x) as usize] = count as u32;
        }
    }

    // BFS from stream heads (upstream_count == 0 and is_stream)
    let mut queue = VecDeque::new();
    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) as usize;
            let is_stream = streams.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            if is_stream >= 0.5 && upstream_count[idx] == 0 {
                order.set_pixel(x, y, 1.0).map_err(AlgorithmError::Core)?;
                computed[idx] = true;
                queue.push_back((x, y));
            }
        }
    }

    while let Some((x, y)) = queue.pop_front() {
        let code = flow_dir.get_pixel(x, y).map_err(AlgorithmError::Core)? as u8;
        if let Some((dx, dy)) = downstream_cell(x, y, code, w, h) {
            let didx = (dy * w + dx) as usize;
            let ds_stream = streams.get_pixel(dx, dy).map_err(AlgorithmError::Core)?;
            if ds_stream < 0.5 {
                continue;
            }

            upstream_count[didx] = upstream_count[didx].saturating_sub(1);

            if upstream_count[didx] == 0 && !computed[didx] {
                // All upstream tributaries computed -- determine Strahler order
                let ups = upstream_cells(dx, dy, flow_dir, w, h)?;
                let mut max_order = 0i32;
                let mut max_count = 0u32;

                for &(ux, uy) in &ups {
                    let us = streams.get_pixel(ux, uy).map_err(AlgorithmError::Core)?;
                    if us < 0.5 {
                        continue;
                    }
                    let o = order.get_pixel(ux, uy).map_err(AlgorithmError::Core)? as i32;
                    if o > max_order {
                        max_order = o;
                        max_count = 1;
                    } else if o == max_order {
                        max_count += 1;
                    }
                }

                let result_order = if max_count >= 2 {
                    max_order + 1
                } else {
                    max_order.max(1)
                };

                order
                    .set_pixel(dx, dy, f64::from(result_order))
                    .map_err(AlgorithmError::Core)?;
                computed[didx] = true;
                queue.push_back((dx, dy));
            }
        }
    }

    Ok(order)
}

// ---------------------------------------------------------------------------
// Shreve stream magnitude
// ---------------------------------------------------------------------------

/// Computes Shreve stream magnitude (Shreve, 1966).
///
/// Rules:
///   - Stream heads get magnitude 1.
///   - At confluences, magnitudes are summed.
///
/// # Errors
///
/// Returns an error if pixel access fails.
pub fn compute_shreve_magnitude(
    streams: &RasterBuffer,
    flow_dir: &RasterBuffer,
) -> Result<RasterBuffer> {
    let w = streams.width();
    let h = streams.height();
    let n = (w * h) as usize;

    let mut magnitude = RasterBuffer::zeros(w, h, RasterDataType::Float64);
    let mut computed = vec![false; n];

    // Count upstream stream neighbours
    let mut upstream_count = vec![0u32; n];
    for y in 0..h {
        for x in 0..w {
            let is_stream = streams.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            if is_stream < 0.5 {
                continue;
            }
            let ups = upstream_cells(x, y, flow_dir, w, h)?;
            let count = ups
                .iter()
                .filter(|&&(ux, uy)| streams.get_pixel(ux, uy).map(|v| v >= 0.5).unwrap_or(false))
                .count();
            upstream_count[(y * w + x) as usize] = count as u32;
        }
    }

    // BFS from stream heads
    let mut queue = VecDeque::new();
    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) as usize;
            let is_stream = streams.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            if is_stream >= 0.5 && upstream_count[idx] == 0 {
                magnitude
                    .set_pixel(x, y, 1.0)
                    .map_err(AlgorithmError::Core)?;
                computed[idx] = true;
                queue.push_back((x, y));
            }
        }
    }

    while let Some((x, y)) = queue.pop_front() {
        let code = flow_dir.get_pixel(x, y).map_err(AlgorithmError::Core)? as u8;
        if let Some((dx, dy)) = downstream_cell(x, y, code, w, h) {
            let didx = (dy * w + dx) as usize;
            let ds_stream = streams.get_pixel(dx, dy).map_err(AlgorithmError::Core)?;
            if ds_stream < 0.5 {
                continue;
            }

            upstream_count[didx] = upstream_count[didx].saturating_sub(1);

            if upstream_count[didx] == 0 && !computed[didx] {
                // Sum magnitudes of all upstream stream neighbours
                let ups = upstream_cells(dx, dy, flow_dir, w, h)?;
                let mut total_mag = 0.0_f64;
                for &(ux, uy) in &ups {
                    let us = streams.get_pixel(ux, uy).map_err(AlgorithmError::Core)?;
                    if us < 0.5 {
                        continue;
                    }
                    total_mag += magnitude.get_pixel(ux, uy).map_err(AlgorithmError::Core)?;
                }

                magnitude
                    .set_pixel(dx, dy, total_mag.max(1.0))
                    .map_err(AlgorithmError::Core)?;
                computed[didx] = true;
                queue.push_back((dx, dy));
            }
        }
    }

    Ok(magnitude)
}

// ---------------------------------------------------------------------------
// Stream link identification
// ---------------------------------------------------------------------------

/// Assigns a unique integer ID to each stream link (reach between junctions
/// or between a stream head and the next junction/outlet).
///
/// Returns a raster where stream cells have their link ID (>= 1) and
/// non-stream cells are 0.
///
/// # Errors
///
/// Returns an error if pixel access fails.
pub fn compute_stream_links(
    streams: &RasterBuffer,
    flow_dir: &RasterBuffer,
) -> Result<RasterBuffer> {
    let w = streams.width();
    let h = streams.height();
    let mut links = RasterBuffer::zeros(w, h, RasterDataType::Int32);
    let mut link_id = 1i32;

    // Identify stream heads and junctions
    let heads = identify_stream_heads(streams, flow_dir)?;

    // Also identify junction cells (stream cells with >= 2 upstream stream neighbours)
    let mut is_junction = vec![false; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let is_stream = streams.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            if is_stream < 0.5 {
                continue;
            }
            let ups = upstream_cells(x, y, flow_dir, w, h)?;
            let stream_ups = ups
                .iter()
                .filter(|&&(ux, uy)| streams.get_pixel(ux, uy).map(|v| v >= 0.5).unwrap_or(false))
                .count();
            if stream_ups >= 2 {
                is_junction[(y * w + x) as usize] = true;
            }
        }
    }

    // Trace downstream from each stream head, assigning link IDs
    for y in 0..h {
        for x in 0..w {
            let is_head = heads.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            if is_head < 0.5 {
                continue;
            }

            let mut cx = x;
            let mut cy = y;
            let current_link = link_id;
            link_id += 1;

            loop {
                let idx = (cy * w + cx) as usize;
                let already = links.get_pixel(cx, cy).map_err(AlgorithmError::Core)? as i32;
                if already > 0 {
                    break; // Already assigned
                }

                links
                    .set_pixel(cx, cy, f64::from(current_link))
                    .map_err(AlgorithmError::Core)?;

                let code = flow_dir.get_pixel(cx, cy).map_err(AlgorithmError::Core)? as u8;
                match downstream_cell(cx, cy, code, w, h) {
                    Some((dx, dy)) => {
                        let ds_stream = streams.get_pixel(dx, dy).map_err(AlgorithmError::Core)?;
                        if ds_stream < 0.5 {
                            break; // Left the stream
                        }
                        // Check if we hit a junction: start a new link ID there
                        let didx = (dy * w + dx) as usize;
                        if is_junction[didx] {
                            // The junction itself will be labeled when traced from
                            // another head or will get its own link
                            break;
                        }
                        cx = dx;
                        cy = dy;
                    }
                    None => break, // Edge of grid
                }
            }
        }
    }

    // Label junction cells that might not have been visited yet
    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) as usize;
            if !is_junction[idx] {
                continue;
            }
            let already = links.get_pixel(x, y).map_err(AlgorithmError::Core)? as i32;
            if already > 0 {
                continue;
            }
            // Assign a new link ID and trace downstream
            let current_link = link_id;
            link_id += 1;

            let mut cx = x;
            let mut cy = y;
            loop {
                let a = links.get_pixel(cx, cy).map_err(AlgorithmError::Core)? as i32;
                if a > 0 {
                    break;
                }
                links
                    .set_pixel(cx, cy, f64::from(current_link))
                    .map_err(AlgorithmError::Core)?;

                let code = flow_dir.get_pixel(cx, cy).map_err(AlgorithmError::Core)? as u8;
                match downstream_cell(cx, cy, code, w, h) {
                    Some((dx, dy)) => {
                        let ds = streams.get_pixel(dx, dy).map_err(AlgorithmError::Core)?;
                        if ds < 0.5 {
                            break;
                        }
                        let didx = (dy * w + dx) as usize;
                        if is_junction[didx] && (dx != x || dy != y) {
                            break;
                        }
                        cx = dx;
                        cy = dy;
                    }
                    None => break,
                }
            }
        }
    }

    Ok(links)
}

// ---------------------------------------------------------------------------
// Stream vectorisation
// ---------------------------------------------------------------------------

/// A polyline segment representing a stream reach.
#[derive(Debug, Clone)]
pub struct StreamSegment {
    /// Unique segment ID (corresponds to stream link ID)
    pub id: i32,
    /// Strahler order of this segment (if computed)
    pub strahler_order: Option<i32>,
    /// Shreve magnitude (if computed)
    pub shreve_magnitude: Option<f64>,
    /// Coordinates of the segment: (col, row) pairs from upstream to downstream
    pub coordinates: Vec<(u64, u64)>,
}

/// Vectorises the stream network into polyline segments.
///
/// Each segment corresponds to a stream link (reach between junctions).
///
/// # Arguments
///
/// * `streams` - Binary stream raster
/// * `flow_dir` - D8 flow direction raster
/// * `strahler` - Optional Strahler order raster
/// * `shreve` - Optional Shreve magnitude raster
///
/// # Errors
///
/// Returns an error if pixel access fails.
pub fn vectorize_streams(
    streams: &RasterBuffer,
    flow_dir: &RasterBuffer,
    strahler: Option<&RasterBuffer>,
    shreve: Option<&RasterBuffer>,
) -> Result<Vec<StreamSegment>> {
    let w = streams.width();
    let h = streams.height();
    let mut segments = Vec::new();

    let heads = identify_stream_heads(streams, flow_dir)?;
    let links = compute_stream_links(streams, flow_dir)?;

    // Find unique link IDs and collect their cells
    let mut link_cells: std::collections::HashMap<i32, Vec<(u64, u64)>> =
        std::collections::HashMap::new();

    for y in 0..h {
        for x in 0..w {
            let lid = links.get_pixel(x, y).map_err(AlgorithmError::Core)? as i32;
            if lid > 0 {
                link_cells.entry(lid).or_default().push((x, y));
            }
        }
    }

    // For each link, trace from the head to build ordered coordinates
    for (lid, cells) in &link_cells {
        // Find the head cell of this link (cell with no upstream stream in the same link)
        let mut head_cell = None;
        for &(cx, cy) in cells {
            let is_head = heads.get_pixel(cx, cy).map_err(AlgorithmError::Core)?;
            if is_head >= 0.5 {
                head_cell = Some((cx, cy));
                break;
            }
            // Also check if this cell has no upstream stream cell with the same link ID
            let ups = upstream_cells(cx, cy, flow_dir, w, h)?;
            let has_same_link_upstream = ups.iter().any(|&(ux, uy)| {
                links
                    .get_pixel(ux, uy)
                    .map(|v| v as i32 == *lid)
                    .unwrap_or(false)
            });
            if !has_same_link_upstream {
                head_cell = Some((cx, cy));
                break;
            }
        }

        let start = match head_cell {
            Some(c) => c,
            None => {
                if let Some(&c) = cells.first() {
                    c
                } else {
                    continue;
                }
            }
        };

        // Trace downstream
        let mut coords = Vec::new();
        let mut cx = start.0;
        let mut cy = start.1;
        let mut visited_count = 0usize;

        loop {
            let current_lid = links.get_pixel(cx, cy).map_err(AlgorithmError::Core)? as i32;
            if current_lid != *lid && !coords.is_empty() {
                break;
            }
            coords.push((cx, cy));
            visited_count += 1;
            if visited_count > cells.len() + 1 {
                break; // Safety: avoid infinite loop
            }

            let code = flow_dir.get_pixel(cx, cy).map_err(AlgorithmError::Core)? as u8;
            match downstream_cell(cx, cy, code, w, h) {
                Some((dx, dy)) => {
                    let dl = links.get_pixel(dx, dy).map_err(AlgorithmError::Core)? as i32;
                    if dl != *lid {
                        break;
                    }
                    cx = dx;
                    cy = dy;
                }
                None => break,
            }
        }

        // Get Strahler order and Shreve magnitude from the first cell
        let so = strahler
            .and_then(|s| s.get_pixel(start.0, start.1).ok())
            .map(|v| v as i32);
        let sm = shreve.and_then(|s| s.get_pixel(start.0, start.1).ok());

        segments.push(StreamSegment {
            id: *lid,
            strahler_order: so,
            shreve_magnitude: sm,
            coordinates: coords,
        });
    }

    // Sort by segment ID
    segments.sort_by_key(|s| s.id);
    Ok(segments)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raster::hydrology::flow_direction::compute_d8_flow_direction;

    fn make_v_shaped_dem(size: u64) -> RasterBuffer {
        // V-shaped valley: elevation = |x - center| + y_distance_from_outlet
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

    fn make_simple_accum(size: u64) -> RasterBuffer {
        // Synthetic accumulation: high in center column, increasing downstream
        let mut accum = RasterBuffer::zeros(size, size, RasterDataType::Float32);
        let center = size / 2;
        for y in 0..size {
            let _ = accum.set_pixel(center, y, (y + 1) as f64 * 10.0);
        }
        accum
    }

    #[test]
    fn test_extract_stream_network() {
        let accum = make_simple_accum(7);
        let streams = extract_stream_network(&accum, 20.0);
        assert!(streams.is_ok());
        let streams = streams.expect("should succeed");

        // Center column with accum >= 20 should be stream
        let v = streams.get_pixel(3, 2).expect("should succeed");
        assert!(v >= 0.5, "Cell with high accumulation should be stream");

        let v2 = streams.get_pixel(0, 0).expect("should succeed");
        assert!(v2 < 0.5, "Off-stream cell should be 0");
    }

    #[test]
    fn test_identify_stream_heads() {
        let dem = make_v_shaped_dem(9);
        let flow_dir = compute_d8_flow_direction(&dem, 1.0).expect("should succeed");
        let accum =
            crate::raster::hydrology::flow_accumulation::compute_flow_accumulation(&dem, 1.0)
                .expect("should succeed");
        let streams = extract_stream_network(&accum, 5.0).expect("should succeed");

        let heads = identify_stream_heads(&streams, &flow_dir);
        assert!(heads.is_ok());
        let heads = heads.expect("should succeed");

        // At least one stream head should exist
        let mut head_count = 0;
        for y in 0..9u64 {
            for x in 0..9u64 {
                let v = heads.get_pixel(x, y).expect("should succeed");
                if v >= 0.5 {
                    head_count += 1;
                }
            }
        }
        assert!(head_count > 0, "Should find at least one stream head");
    }

    #[test]
    fn test_strahler_order_basic() {
        let dem = make_v_shaped_dem(11);
        let flow_dir = compute_d8_flow_direction(&dem, 1.0).expect("should succeed");
        let accum =
            crate::raster::hydrology::flow_accumulation::compute_flow_accumulation(&dem, 1.0)
                .expect("should succeed");
        let streams = extract_stream_network(&accum, 3.0).expect("should succeed");

        let order = compute_strahler_order(&streams, &flow_dir);
        assert!(order.is_ok());
        let order = order.expect("should succeed");

        // At least some cells should have order >= 1
        let mut has_order = false;
        for y in 0..11u64 {
            for x in 0..11u64 {
                let o = order.get_pixel(x, y).expect("should succeed");
                if o >= 1.0 {
                    has_order = true;
                    break;
                }
            }
            if has_order {
                break;
            }
        }
        assert!(has_order, "Some stream cells should have order >= 1");
    }

    #[test]
    fn test_shreve_magnitude_basic() {
        let dem = make_v_shaped_dem(11);
        let flow_dir = compute_d8_flow_direction(&dem, 1.0).expect("should succeed");
        let accum =
            crate::raster::hydrology::flow_accumulation::compute_flow_accumulation(&dem, 1.0)
                .expect("should succeed");
        let streams = extract_stream_network(&accum, 3.0).expect("should succeed");

        let mag = compute_shreve_magnitude(&streams, &flow_dir);
        assert!(mag.is_ok());
        let mag = mag.expect("should succeed");

        // Downstream should have higher magnitude than upstream
        let mut has_mag = false;
        for y in 0..11u64 {
            for x in 0..11u64 {
                let m = mag.get_pixel(x, y).expect("should succeed");
                if m >= 1.0 {
                    has_mag = true;
                    break;
                }
            }
            if has_mag {
                break;
            }
        }
        assert!(has_mag, "Some stream cells should have magnitude >= 1");
    }

    #[test]
    fn test_stream_links() {
        let dem = make_v_shaped_dem(11);
        let flow_dir = compute_d8_flow_direction(&dem, 1.0).expect("should succeed");
        let accum =
            crate::raster::hydrology::flow_accumulation::compute_flow_accumulation(&dem, 1.0)
                .expect("should succeed");
        let streams = extract_stream_network(&accum, 3.0).expect("should succeed");

        let links = compute_stream_links(&streams, &flow_dir);
        assert!(links.is_ok());
        let links = links.expect("should succeed");

        // At least one link should exist
        let mut has_link = false;
        for y in 0..11u64 {
            for x in 0..11u64 {
                let v = links.get_pixel(x, y).expect("should succeed") as i32;
                if v > 0 {
                    has_link = true;
                    break;
                }
            }
            if has_link {
                break;
            }
        }
        assert!(has_link, "Should find at least one stream link");
    }

    #[test]
    fn test_vectorize_streams() {
        let dem = make_v_shaped_dem(11);
        let flow_dir = compute_d8_flow_direction(&dem, 1.0).expect("should succeed");
        let accum =
            crate::raster::hydrology::flow_accumulation::compute_flow_accumulation(&dem, 1.0)
                .expect("should succeed");
        let streams = extract_stream_network(&accum, 3.0).expect("should succeed");

        let segments = vectorize_streams(&streams, &flow_dir, None, None);
        assert!(segments.is_ok());
        let segments = segments.expect("should succeed");

        assert!(
            !segments.is_empty(),
            "Should produce at least one stream segment"
        );
        for seg in &segments {
            assert!(
                !seg.coordinates.is_empty(),
                "Segment {} should have coordinates",
                seg.id
            );
        }
    }

    #[test]
    fn test_compute_stream_order_legacy_api() {
        // Test the legacy API that takes DEM + accum
        let dem = make_v_shaped_dem(11);
        let accum =
            crate::raster::hydrology::flow_accumulation::compute_flow_accumulation(&dem, 1.0)
                .expect("should succeed");

        let order = compute_stream_order(&dem, &accum, 3.0, 1.0);
        assert!(order.is_ok());
    }
}
