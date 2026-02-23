//! Flow accumulation algorithms for hydrological analysis
//!
//! Supports D8, D-Infinity, and MFD (Multiple Flow Direction) accumulation,
//! with optional weight grids (e.g. rainfall, runoff coefficients).
//!
//! Algorithms:
//!   - D8 accumulation (topological sort, Jenson & Domingue 1988)
//!   - D-Infinity weighted accumulation (Tarboton 1997)
//!   - MFD accumulation (Freeman 1991)
//!   - Stream threshold extraction

use crate::error::{AlgorithmError, Result};
use crate::raster::hydrology::flow_direction::{
    D8_DX, D8_DY, D8Direction, MfdConfig, MfdResult, compute_d8_flow_direction,
    compute_dinf_flow_direction, compute_mfd_flow_direction,
};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;
use std::collections::VecDeque;

// ---------------------------------------------------------------------------
// Helper: direction code -> D8Direction
// ---------------------------------------------------------------------------

fn direction_from_code(code: u8) -> Option<D8Direction> {
    D8Direction::from_code(code)
}

// ---------------------------------------------------------------------------
// D8 flow accumulation
// ---------------------------------------------------------------------------

/// Computes D8 flow accumulation.
///
/// Each cell accumulates a count of all upstream cells that drain through it
/// (including itself, which contributes 1).
///
/// Uses a topological-sort approach: cells with zero incoming flows are
/// processed first, propagating accumulated values downstream.
///
/// # Errors
///
/// Returns an error if raster pixel access fails.
pub fn compute_flow_accumulation(dem: &RasterBuffer, cell_size: f64) -> Result<RasterBuffer> {
    let flow_dir = compute_d8_flow_direction(dem, cell_size)?;
    accumulate_d8(&flow_dir, None)
}

/// Computes D8 weighted flow accumulation.
///
/// Like `compute_flow_accumulation` but each cell contributes its weight
/// (e.g. precipitation depth, runoff coefficient) instead of 1.
///
/// # Arguments
///
/// * `dem` - Digital elevation model
/// * `weights` - Weight raster (must match DEM dimensions)
/// * `cell_size` - Cell size in map units
///
/// # Errors
///
/// Returns an error if dimensions mismatch or pixel access fails.
pub fn compute_weighted_flow_accumulation(
    dem: &RasterBuffer,
    weights: &RasterBuffer,
    cell_size: f64,
) -> Result<RasterBuffer> {
    if dem.width() != weights.width() || dem.height() != weights.height() {
        return Err(AlgorithmError::InvalidParameter {
            parameter: "weights",
            message: "Weight raster must have the same dimensions as the DEM".to_string(),
        });
    }
    let flow_dir = compute_d8_flow_direction(dem, cell_size)?;
    accumulate_d8(&flow_dir, Some(weights))
}

/// Core D8 accumulation on a precomputed flow direction grid.
///
/// If `weights` is `None`, each cell contributes 1. Otherwise each cell
/// contributes its weight value.
fn accumulate_d8(flow_dir: &RasterBuffer, weights: Option<&RasterBuffer>) -> Result<RasterBuffer> {
    let w = flow_dir.width();
    let h = flow_dir.height();
    let n = (w * h) as usize;

    let mut accum = RasterBuffer::zeros(w, h, RasterDataType::Float64);

    // Initialise with weight (or 1)
    for y in 0..h {
        for x in 0..w {
            let val = match weights {
                Some(wt) => wt.get_pixel(x, y).map_err(AlgorithmError::Core)?,
                None => 1.0,
            };
            accum.set_pixel(x, y, val).map_err(AlgorithmError::Core)?;
        }
    }

    // Count incoming edges
    let mut incoming = vec![0u32; n];

    for y in 0..h {
        for x in 0..w {
            let code = flow_dir.get_pixel(x, y).map_err(AlgorithmError::Core)? as u8;
            if let Some(dir) = direction_from_code(code) {
                let (dx, dy) = dir.offset();
                let nx = x as i64 + dx;
                let ny = y as i64 + dy;
                if nx >= 0 && (nx as u64) < w && ny >= 0 && (ny as u64) < h {
                    incoming[(ny as u64 * w + nx as u64) as usize] += 1;
                }
            }
        }
    }

    // Topological BFS
    let mut queue = VecDeque::with_capacity(n / 4);
    for idx in 0..n {
        if incoming[idx] == 0 {
            let x = (idx as u64) % w;
            let y = (idx as u64) / w;
            queue.push_back((x, y));
        }
    }

    while let Some((x, y)) = queue.pop_front() {
        let code = flow_dir.get_pixel(x, y).map_err(AlgorithmError::Core)? as u8;
        if let Some(dir) = direction_from_code(code) {
            let (dx, dy) = dir.offset();
            let nx = x as i64 + dx;
            let ny = y as i64 + dy;
            if nx >= 0 && (nx as u64) < w && ny >= 0 && (ny as u64) < h {
                let nxu = nx as u64;
                let nyu = ny as u64;
                let cur = accum.get_pixel(x, y).map_err(AlgorithmError::Core)?;
                let downstream = accum.get_pixel(nxu, nyu).map_err(AlgorithmError::Core)?;
                accum
                    .set_pixel(nxu, nyu, downstream + cur)
                    .map_err(AlgorithmError::Core)?;

                let nidx = (nyu * w + nxu) as usize;
                incoming[nidx] -= 1;
                if incoming[nidx] == 0 {
                    queue.push_back((nxu, nyu));
                }
            }
        }
    }

    Ok(accum)
}

// ---------------------------------------------------------------------------
// D-Infinity flow accumulation  (Tarboton, 1997)
// ---------------------------------------------------------------------------

/// Computes D-Infinity flow accumulation.
///
/// Uses the angle/proportion rasters from `compute_dinf_flow_direction` to
/// distribute upstream area to the two downslope cells in proportion.
///
/// # Errors
///
/// Returns an error if pixel access fails.
pub fn compute_dinf_flow_accumulation(dem: &RasterBuffer, cell_size: f64) -> Result<RasterBuffer> {
    compute_dinf_flow_accumulation_weighted(dem, None, cell_size)
}

/// Computes D-Infinity weighted flow accumulation.
///
/// # Errors
///
/// Returns an error if pixel access fails or dimensions mismatch.
pub fn compute_dinf_flow_accumulation_weighted(
    dem: &RasterBuffer,
    weights: Option<&RasterBuffer>,
    cell_size: f64,
) -> Result<RasterBuffer> {
    if let Some(wt) = weights {
        if dem.width() != wt.width() || dem.height() != wt.height() {
            return Err(AlgorithmError::InvalidParameter {
                parameter: "weights",
                message: "Weight raster must match DEM dimensions".to_string(),
            });
        }
    }

    let w = dem.width();
    let h = dem.height();
    let n = (w * h) as usize;

    let (angle_raster, prop_raster) = compute_dinf_flow_direction(dem, cell_size)?;

    // Initialise accumulation
    let mut accum = vec![0.0_f64; n];
    for y in 0..h {
        for x in 0..w {
            let val = match weights {
                Some(wt) => wt.get_pixel(x, y).map_err(AlgorithmError::Core)?,
                None => 1.0,
            };
            accum[(y * w + x) as usize] = val;
        }
    }

    // Build dependency graph: for each cell, find its two downstream cells
    // and compute incoming edge count
    let mut incoming = vec![0u32; n];
    let mut targets: Vec<[(u64, u64, f64); 2]> = Vec::with_capacity(n);

    for y in 0..h {
        for x in 0..w {
            let angle_deg = angle_raster.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            let prop = prop_raster.get_pixel(x, y).map_err(AlgorithmError::Core)?;

            let (t1, t2) = dinf_target_cells(x, y, angle_deg, prop, w, h);
            targets.push([t1, t2]);

            // Count incoming
            if t1.2 > 0.0 {
                let idx = (t1.1 * w + t1.0) as usize;
                if idx < n {
                    incoming[idx] += 1;
                }
            }
            if t2.2 > 0.0 {
                let idx = (t2.1 * w + t2.0) as usize;
                if idx < n {
                    incoming[idx] += 1;
                }
            }
        }
    }

    // Topological BFS
    let mut queue = VecDeque::with_capacity(n / 4);
    for idx in 0..n {
        if incoming[idx] == 0 {
            queue.push_back(idx);
        }
    }

    while let Some(idx) = queue.pop_front() {
        let cur_accum = accum[idx];
        let [t1, t2] = targets[idx];

        for &(tx, ty, frac) in &[t1, t2] {
            if frac <= 0.0 {
                continue;
            }
            let tidx = (ty * w + tx) as usize;
            if tidx >= n {
                continue;
            }
            accum[tidx] += cur_accum * frac;
            incoming[tidx] -= 1;
            if incoming[tidx] == 0 {
                queue.push_back(tidx);
            }
        }
    }

    // Copy results to RasterBuffer
    let mut result = RasterBuffer::zeros(w, h, RasterDataType::Float64);
    for y in 0..h {
        for x in 0..w {
            result
                .set_pixel(x, y, accum[(y * w + x) as usize])
                .map_err(AlgorithmError::Core)?;
        }
    }
    Ok(result)
}

/// Determines the two downstream target cells for D-Infinity.
///
/// Returns `[(x1, y1, frac1), (x2, y2, frac2)]`.
fn dinf_target_cells(
    x: u64,
    y: u64,
    angle_deg: f64,
    proportion: f64,
    w: u64,
    h: u64,
) -> ((u64, u64, f64), (u64, u64, f64)) {
    let facet = ((angle_deg / 45.0).floor() as usize) % 8;
    let next_facet = (facet + 1) % 8;

    let nx1 = x as i64 + D8_DX[facet];
    let ny1 = y as i64 + D8_DY[facet];
    let nx2 = x as i64 + D8_DX[next_facet];
    let ny2 = y as i64 + D8_DY[next_facet];

    let in_bounds1 = nx1 >= 0 && (nx1 as u64) < w && ny1 >= 0 && (ny1 as u64) < h;
    let in_bounds2 = nx2 >= 0 && (nx2 as u64) < w && ny2 >= 0 && (ny2 as u64) < h;

    let t1 = if in_bounds1 {
        (nx1 as u64, ny1 as u64, proportion)
    } else {
        (x, y, 0.0) // dummy
    };

    let t2 = if in_bounds2 {
        (nx2 as u64, ny2 as u64, 1.0 - proportion)
    } else {
        (x, y, 0.0) // dummy
    };

    (t1, t2)
}

// ---------------------------------------------------------------------------
// MFD flow accumulation (Freeman, 1991)
// ---------------------------------------------------------------------------

/// Computes MFD flow accumulation.
///
/// Distributes flow from each cell to all downslope neighbours in proportion
/// to their weighted slope.
///
/// # Errors
///
/// Returns an error if pixel access fails.
pub fn compute_mfd_flow_accumulation(dem: &RasterBuffer, cell_size: f64) -> Result<RasterBuffer> {
    compute_mfd_flow_accumulation_weighted(dem, None, cell_size)
}

/// Computes MFD weighted flow accumulation.
///
/// # Errors
///
/// Returns an error if pixel access fails or dimensions mismatch.
pub fn compute_mfd_flow_accumulation_weighted(
    dem: &RasterBuffer,
    weights: Option<&RasterBuffer>,
    cell_size: f64,
) -> Result<RasterBuffer> {
    if let Some(wt) = weights {
        if dem.width() != wt.width() || dem.height() != wt.height() {
            return Err(AlgorithmError::InvalidParameter {
                parameter: "weights",
                message: "Weight raster must match DEM dimensions".to_string(),
            });
        }
    }

    let w = dem.width();
    let h = dem.height();
    let n = (w * h) as usize;

    let cfg = MfdConfig {
        cell_size,
        ..MfdConfig::default()
    };
    let mfd = compute_mfd_flow_direction(dem, &cfg)?;

    // Initialise accumulation
    let mut accum = vec![0.0_f64; n];
    for y in 0..h {
        for x in 0..w {
            let val = match weights {
                Some(wt) => wt.get_pixel(x, y).map_err(AlgorithmError::Core)?,
                None => 1.0,
            };
            accum[(y * w + x) as usize] = val;
        }
    }

    // Build incoming count from MFD proportions
    let mut incoming = vec![0u32; n];
    for y in 0..h {
        for x in 0..w {
            let props = mfd.get_proportions(x, y);
            for i in 0..8 {
                if props[i] > 0.0 {
                    let nx = x as i64 + D8_DX[i];
                    let ny = y as i64 + D8_DY[i];
                    if nx >= 0 && (nx as u64) < w && ny >= 0 && (ny as u64) < h {
                        incoming[(ny as u64 * w + nx as u64) as usize] += 1;
                    }
                }
            }
        }
    }

    // Topological BFS
    let mut queue = VecDeque::with_capacity(n / 4);
    for idx in 0..n {
        if incoming[idx] == 0 {
            queue.push_back(idx);
        }
    }

    while let Some(idx) = queue.pop_front() {
        let x = (idx as u64) % w;
        let y = (idx as u64) / w;
        let cur_accum = accum[idx];
        let props = mfd.get_proportions(x, y);

        for i in 0..8 {
            if props[i] <= 0.0 {
                continue;
            }
            let nx = x as i64 + D8_DX[i];
            let ny = y as i64 + D8_DY[i];
            if nx < 0 || (nx as u64) >= w || ny < 0 || (ny as u64) >= h {
                continue;
            }
            let nidx = (ny as u64 * w + nx as u64) as usize;
            accum[nidx] += cur_accum * props[i];
            incoming[nidx] -= 1;
            if incoming[nidx] == 0 {
                queue.push_back(nidx);
            }
        }
    }

    // Copy to RasterBuffer
    let mut result = RasterBuffer::zeros(w, h, RasterDataType::Float64);
    for y in 0..h {
        for x in 0..w {
            result
                .set_pixel(x, y, accum[(y * w + x) as usize])
                .map_err(AlgorithmError::Core)?;
        }
    }
    Ok(result)
}

// ---------------------------------------------------------------------------
// Stream threshold
// ---------------------------------------------------------------------------

/// Configuration for stream threshold extraction
#[derive(Debug, Clone)]
pub struct StreamThresholdConfig {
    /// Minimum flow accumulation to define a stream cell
    pub threshold: f64,
    /// Whether to use percentage of maximum accumulation instead of absolute
    pub use_percentage: bool,
}

/// Extracts stream cells from a flow accumulation raster using a threshold.
///
/// Returns a binary raster (1 = stream, 0 = non-stream).
///
/// If `use_percentage` is true, the threshold is interpreted as a fraction
/// (0.0 .. 1.0) of the maximum accumulation in the grid.
///
/// # Errors
///
/// Returns an error if pixel access fails.
pub fn extract_streams_by_threshold(
    accumulation: &RasterBuffer,
    config: &StreamThresholdConfig,
) -> Result<RasterBuffer> {
    let w = accumulation.width();
    let h = accumulation.height();

    let actual_threshold = if config.use_percentage {
        // Find maximum accumulation
        let mut max_val = 0.0_f64;
        for y in 0..h {
            for x in 0..w {
                let v = accumulation.get_pixel(x, y).map_err(AlgorithmError::Core)?;
                if v > max_val {
                    max_val = v;
                }
            }
        }
        max_val * config.threshold.clamp(0.0, 1.0)
    } else {
        config.threshold
    };

    let mut streams = RasterBuffer::zeros(w, h, RasterDataType::UInt8);
    for y in 0..h {
        for x in 0..w {
            let v = accumulation.get_pixel(x, y).map_err(AlgorithmError::Core)?;
            if v >= actual_threshold {
                streams.set_pixel(x, y, 1.0).map_err(AlgorithmError::Core)?;
            }
        }
    }

    Ok(streams)
}

/// Computes flow accumulation from a precomputed D8 flow direction grid.
///
/// This is useful when the flow direction has already been computed elsewhere
/// (e.g. after sink filling).
///
/// # Errors
///
/// Returns an error if pixel access fails.
pub fn compute_d8_accumulation_from_fdir(
    flow_dir: &RasterBuffer,
    weights: Option<&RasterBuffer>,
) -> Result<RasterBuffer> {
    accumulate_d8(flow_dir, weights)
}

/// Computes MFD accumulation from a precomputed MFD result.
///
/// # Errors
///
/// Returns an error if pixel access fails.
pub fn compute_mfd_accumulation_from_result(
    mfd: &MfdResult,
    weights: Option<&RasterBuffer>,
) -> Result<RasterBuffer> {
    let w = mfd.width;
    let h = mfd.height;
    let n = (w * h) as usize;

    let mut accum = vec![0.0_f64; n];

    // Initialise
    for y in 0..h {
        for x in 0..w {
            let val = match weights {
                Some(wt) => wt.get_pixel(x, y).map_err(AlgorithmError::Core)?,
                None => 1.0,
            };
            accum[(y * w + x) as usize] = val;
        }
    }

    // Incoming count
    let mut incoming = vec![0u32; n];
    for y in 0..h {
        for x in 0..w {
            let props = mfd.get_proportions(x, y);
            for i in 0..8 {
                if props[i] > 0.0 {
                    let nx = x as i64 + D8_DX[i];
                    let ny = y as i64 + D8_DY[i];
                    if nx >= 0 && (nx as u64) < w && ny >= 0 && (ny as u64) < h {
                        incoming[(ny as u64 * w + nx as u64) as usize] += 1;
                    }
                }
            }
        }
    }

    let mut queue = VecDeque::with_capacity(n / 4);
    for idx in 0..n {
        if incoming[idx] == 0 {
            queue.push_back(idx);
        }
    }

    while let Some(idx) = queue.pop_front() {
        let x = (idx as u64) % w;
        let y = (idx as u64) / w;
        let cur = accum[idx];
        let props = mfd.get_proportions(x, y);

        for i in 0..8 {
            if props[i] <= 0.0 {
                continue;
            }
            let nx = x as i64 + D8_DX[i];
            let ny = y as i64 + D8_DY[i];
            if nx < 0 || (nx as u64) >= w || ny < 0 || (ny as u64) >= h {
                continue;
            }
            let nidx = (ny as u64 * w + nx as u64) as usize;
            accum[nidx] += cur * props[i];
            incoming[nidx] -= 1;
            if incoming[nidx] == 0 {
                queue.push_back(nidx);
            }
        }
    }

    let mut result = RasterBuffer::zeros(w, h, RasterDataType::Float64);
    for y in 0..h {
        for x in 0..w {
            result
                .set_pixel(x, y, accum[(y * w + x) as usize])
                .map_err(AlgorithmError::Core)?;
        }
    }
    Ok(result)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    fn make_east_slope(w: u64, h: u64) -> RasterBuffer {
        let mut dem = RasterBuffer::zeros(w, h, RasterDataType::Float32);
        for y in 0..h {
            for x in 0..w {
                let _ = dem.set_pixel(x, y, (w - 1 - x) as f64);
            }
        }
        dem
    }

    #[test]
    fn test_d8_flow_accumulation() {
        let dem = make_east_slope(7, 7);
        let accum = compute_flow_accumulation(&dem, 1.0);
        assert!(accum.is_ok());
        let accum = accum.expect("should succeed");

        // Eastern column should have high accumulation
        let east = accum.get_pixel(6, 3).expect("should succeed");
        assert!(east > 1.0, "East edge accumulation {east} should be > 1");
    }

    #[test]
    fn test_weighted_accumulation() {
        let dem = make_east_slope(7, 7);
        let mut weights = RasterBuffer::zeros(7, 7, RasterDataType::Float32);
        for y in 0..7u64 {
            for x in 0..7u64 {
                let _ = weights.set_pixel(x, y, 2.0);
            }
        }

        let accum = compute_weighted_flow_accumulation(&dem, &weights, 1.0);
        assert!(accum.is_ok());
        let accum = accum.expect("should succeed");
        let east = accum.get_pixel(6, 3).expect("should succeed");
        assert!(
            east > 2.0,
            "Weighted accumulation at east edge should be > 2"
        );
    }

    #[test]
    fn test_weighted_dimension_mismatch() {
        let dem = make_east_slope(7, 7);
        let weights = RasterBuffer::zeros(5, 5, RasterDataType::Float32);
        let result = compute_weighted_flow_accumulation(&dem, &weights, 1.0);
        assert!(result.is_err());
    }

    #[test]
    fn test_dinf_accumulation() {
        let dem = make_east_slope(7, 7);
        let accum = compute_dinf_flow_accumulation(&dem, 1.0);
        assert!(accum.is_ok());
        let accum = accum.expect("should succeed");
        let east = accum.get_pixel(6, 3).expect("should succeed");
        assert!(east >= 1.0, "D-Inf accumulation at east should be >= 1");
    }

    #[test]
    fn test_mfd_accumulation() {
        let dem = make_east_slope(7, 7);
        let accum = compute_mfd_flow_accumulation(&dem, 1.0);
        assert!(accum.is_ok());
        let accum = accum.expect("should succeed");
        let east = accum.get_pixel(6, 3).expect("should succeed");
        assert!(east >= 1.0, "MFD accumulation at east should be >= 1");
    }

    #[test]
    fn test_stream_threshold_absolute() {
        let dem = make_east_slope(7, 7);
        let accum = compute_flow_accumulation(&dem, 1.0).expect("should succeed");

        let cfg = StreamThresholdConfig {
            threshold: 3.0,
            use_percentage: false,
        };
        let streams = extract_streams_by_threshold(&accum, &cfg);
        assert!(streams.is_ok());
        let streams = streams.expect("should succeed");

        // Cells near western edge should not be streams
        let west = streams.get_pixel(0, 3).expect("should succeed");
        assert_abs_diff_eq!(west, 0.0, epsilon = 0.1);
    }

    #[test]
    fn test_stream_threshold_percentage() {
        let dem = make_east_slope(7, 7);
        let accum = compute_flow_accumulation(&dem, 1.0).expect("should succeed");

        let cfg = StreamThresholdConfig {
            threshold: 0.5,
            use_percentage: true,
        };
        let streams = extract_streams_by_threshold(&accum, &cfg);
        assert!(streams.is_ok());
    }

    #[test]
    fn test_accumulation_conservation() {
        // For a simple slope, total accumulation at outlets should equal total input
        let dem = make_east_slope(5, 5);
        let accum = compute_flow_accumulation(&dem, 1.0).expect("should succeed");

        // The rightmost column cells are the outlets -- sum should be close to 25
        let mut outlet_sum = 0.0;
        for y in 0..5u64 {
            outlet_sum += accum.get_pixel(4, y).expect("should succeed");
        }
        // Each of the 25 cells contributes 1, and all flow east
        assert!(
            outlet_sum >= 20.0,
            "Outlet sum {outlet_sum} should capture most of the 25 cells"
        );
    }
}
