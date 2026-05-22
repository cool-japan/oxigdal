//! Tiled raster processing for datasets exceeding VRAM budget.
//!
//! This module provides automatic tile-based decomposition of large rasters
//! that would otherwise overflow GPU VRAM. Key concepts:
//!
//! - A **tile** is a rectangular sub-region of the full raster.
//! - An **overlap halo** (optional) extends each tile by `overlap_pixels` on
//!   every active edge, using edge-replication for boundary pixels.
//! - **Splitting** decomposes a flat f32 raster into `Vec<RasterTile>`.
//! - **Stitching** assembles processed tiles back into a full raster,
//!   discarding the halo and writing only the core interior.
//! - **`auto_tile_size`** finds a tile size that fits inside an available
//!   VRAM budget by halving the area until it fits.
//! - **`execute_tiled`** orchestrates split → per-tile `tile_fn` → stitch.

use crate::error::{GpuError, GpuResult};

// ─────────────────────────────────────────────────────────────────────────────
// TiledConfig
// ─────────────────────────────────────────────────────────────────────────────

/// Configuration for tiled raster processing.
///
/// The default tile size (512 × 512) fits comfortably within typical VRAM
/// budgets for f32 rasters.  Increase `overlap_pixels` when a kernel requires
/// neighbouring pixel context (e.g. convolution with radius `r` needs
/// `overlap_pixels = r`).
#[derive(Debug, Clone)]
pub struct TiledConfig {
    /// Tile width in pixels.  Default: 512.
    pub tile_width: usize,
    /// Tile height in pixels.  Default: 512.
    pub tile_height: usize,
    /// Overlap in pixels on every active edge (halo for kernels needing
    /// neighbours).  Default: 0.
    pub overlap_pixels: usize,
    /// Safety margin: fraction of the VRAM budget to keep free.
    /// Must be in `[0.0, 1.0)`.  Default: 0.1 (10 %).
    pub vram_safety_margin: f64,
}

impl Default for TiledConfig {
    fn default() -> Self {
        Self {
            tile_width: 512,
            tile_height: 512,
            overlap_pixels: 0,
            vram_safety_margin: 0.1,
        }
    }
}

impl TiledConfig {
    /// Set tile width and height.
    pub fn with_tile_size(mut self, w: usize, h: usize) -> Self {
        self.tile_width = w;
        self.tile_height = h;
        self
    }

    /// Set overlap (halo) width in pixels on each edge.
    pub fn with_overlap(mut self, pixels: usize) -> Self {
        self.overlap_pixels = pixels;
        self
    }

    /// Set the VRAM safety margin fraction.  Clamped to `[0.0, 0.99]`.
    pub fn with_vram_safety_margin(mut self, margin: f64) -> Self {
        self.vram_safety_margin = margin.clamp(0.0, 0.99);
        self
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// RasterTile
// ─────────────────────────────────────────────────────────────────────────────

/// A single tile extracted from a raster, optionally padded with an overlap halo.
///
/// `data` is stored row-major (top-left origin) and includes halo pixels if
/// `overlap_pixels > 0`.  The halo is filled using edge-replication (the value
/// at the nearest in-bounds pixel is copied).
#[derive(Debug, Clone)]
pub struct RasterTile {
    /// Pixel data (row-major, f32, single band).
    /// Length == `padded_width() * padded_height()`.
    pub data: Vec<f32>,

    /// Core tile width in pixels (without overlap).
    pub width: usize,
    /// Core tile height in pixels (without overlap).
    pub height: usize,

    /// Overlap rows added above the core region.
    pub overlap_top: usize,
    /// Overlap columns added to the right of the core region.
    pub overlap_right: usize,
    /// Overlap rows added below the core region.
    pub overlap_bottom: usize,
    /// Overlap columns added to the left of the core region.
    pub overlap_left: usize,

    /// X pixel coordinate of the tile's top-left corner in the full raster
    /// (excluding overlap extension, i.e. the origin of the core region).
    pub origin_x: usize,
    /// Y pixel coordinate of the tile's top-left corner in the full raster
    /// (excluding overlap extension).
    pub origin_y: usize,

    /// Full raster width in pixels.
    pub raster_width: usize,
    /// Full raster height in pixels.
    pub raster_height: usize,

    /// Flat tile index: `tile_row * tiles_per_row + tile_col`.
    pub tile_index: usize,
}

impl RasterTile {
    /// Width of `data` in pixels (core + left halo + right halo).
    #[inline]
    pub fn padded_width(&self) -> usize {
        self.width + self.overlap_left + self.overlap_right
    }

    /// Height of `data` in pixels (core + top halo + bottom halo).
    #[inline]
    pub fn padded_height(&self) -> usize {
        self.height + self.overlap_top + self.overlap_bottom
    }

    /// Total number of f32 elements in `data`.
    #[inline]
    pub fn padded_len(&self) -> usize {
        self.padded_width() * self.padded_height()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// split_into_tiles
// ─────────────────────────────────────────────────────────────────────────────

/// Split a flat f32 raster (row-major) into tiles with an optional overlap halo.
///
/// Edge handling: when a tile would extend beyond the raster boundary the halo
/// is filled with the value at the nearest valid pixel (edge-replication /
/// clamp-to-edge).
///
/// Tiles are returned in row-major tile order (left-to-right within each row,
/// top-to-bottom across rows).
///
/// If `raster_width == 0 || raster_height == 0` an empty `Vec` is returned.
/// If `config.tile_width == 0 || config.tile_height == 0` the entire raster is
/// returned as a single tile.
pub fn split_into_tiles(
    raster: &[f32],
    raster_width: usize,
    raster_height: usize,
    config: &TiledConfig,
) -> Vec<RasterTile> {
    if raster_width == 0 || raster_height == 0 {
        return Vec::new();
    }

    // Treat a zero tile dimension as "whole raster".
    let tile_w = if config.tile_width == 0 {
        raster_width
    } else {
        config.tile_width
    };
    let tile_h = if config.tile_height == 0 {
        raster_height
    } else {
        config.tile_height
    };
    let overlap = config.overlap_pixels;

    // Number of tiles along each axis.
    let tiles_x = raster_width.div_ceil(tile_w);
    let tiles_y = raster_height.div_ceil(tile_h);

    let mut result = Vec::with_capacity(tiles_x * tiles_y);

    for ty in 0..tiles_y {
        for tx in 0..tiles_x {
            // Core region (in raster coordinates).
            let core_x0 = tx * tile_w;
            let core_y0 = ty * tile_h;
            let core_w = tile_w.min(raster_width - core_x0);
            let core_h = tile_h.min(raster_height - core_y0);

            // Compute actual halo sizes — zero at raster boundary.
            let halo_top = overlap.min(core_y0);
            let halo_left = overlap.min(core_x0);
            let halo_bottom = overlap.min(raster_height.saturating_sub(core_y0 + core_h));
            let halo_right = overlap.min(raster_width.saturating_sub(core_x0 + core_w));

            // For corner/edge tiles we still want the overlap to contain valid
            // (replicated) data even if the raster has no pixels there.  The
            // padded_width / padded_height always include the full `overlap`
            // halo; pixels outside the raster are clamped to the edge row/col.
            let pad_top = overlap;
            let pad_left = overlap;
            let pad_bottom = overlap;
            let pad_right = overlap;

            let padded_w = core_w + pad_left + pad_right;
            let padded_h = core_h + pad_top + pad_bottom;

            let mut data = vec![0.0_f32; padded_w * padded_h];

            for row in 0..padded_h {
                // Row in raster space corresponding to this padded row.
                // Clamp to [0, raster_height-1] for edge replication.
                let raster_row = (core_y0 as isize - overlap as isize + row as isize)
                    .clamp(0, raster_height as isize - 1) as usize;

                for col in 0..padded_w {
                    // Column in raster space, clamped similarly.
                    let raster_col = (core_x0 as isize - overlap as isize + col as isize)
                        .clamp(0, raster_width as isize - 1)
                        as usize;

                    let src_idx = raster_row * raster_width + raster_col;
                    let dst_idx = row * padded_w + col;

                    data[dst_idx] = if src_idx < raster.len() {
                        raster[src_idx]
                    } else {
                        0.0
                    };
                }
            }

            result.push(RasterTile {
                data,
                width: core_w,
                height: core_h,
                overlap_top: pad_top,
                overlap_right: pad_right,
                overlap_bottom: pad_bottom,
                overlap_left: pad_left,
                // Actual halo extent (may be smaller at boundary).
                // We expose the configured overlap uniformly; stitching uses
                // `overlap_left/top` to locate the core interior.
                origin_x: core_x0,
                origin_y: core_y0,
                raster_width,
                raster_height,
                tile_index: ty * tiles_x + tx,
            });

            // Suppress unused variable warnings for the computed-but-not-stored
            // actual halo sizes (they were intermediate calculations).
            let _ = (halo_top, halo_left, halo_bottom, halo_right);
        }
    }

    result
}

// ─────────────────────────────────────────────────────────────────────────────
// stitch_tiles
// ─────────────────────────────────────────────────────────────────────────────

/// Stitch processed tiles back into a full raster.
///
/// Only the non-overlap core interior (`tile.width × tile.height`) of each tile
/// is written into the output at `(tile.origin_x, tile.origin_y)`.  Halo pixels
/// are discarded.  Pixels not covered by any tile are left as `0.0`.
///
/// Tiles may be supplied in any order; `tile_index` is not used for placement
/// (origin coordinates are used instead).
pub fn stitch_tiles(tiles: &[RasterTile], raster_width: usize, raster_height: usize) -> Vec<f32> {
    let mut output = vec![0.0_f32; raster_width * raster_height];

    for tile in tiles {
        let padded_w = tile.padded_width();
        let core_w = tile.width;
        let core_h = tile.height;
        let halo_left = tile.overlap_left;
        let halo_top = tile.overlap_top;

        for row in 0..core_h {
            for col in 0..core_w {
                let src_row = halo_top + row;
                let src_col = halo_left + col;
                let src_idx = src_row * padded_w + src_col;

                let dst_row = tile.origin_y + row;
                let dst_col = tile.origin_x + col;

                if dst_row < raster_height && dst_col < raster_width {
                    let dst_idx = dst_row * raster_width + dst_col;
                    if src_idx < tile.data.len() {
                        output[dst_idx] = tile.data[src_idx];
                    }
                }
            }
        }
    }

    output
}

// ─────────────────────────────────────────────────────────────────────────────
// vram_per_tile
// ─────────────────────────────────────────────────────────────────────────────

/// Estimate the VRAM required to process one tile.
///
/// Budget formula:
/// - input f32 buffer: `padded_len × 4` bytes
/// - output f32 buffer: `padded_len × 4` bytes
/// - uniform / metadata buffer: 256 bytes (rounded up to wgpu alignment)
///
/// Returns the total in bytes.
pub fn vram_per_tile(tile: &RasterTile) -> usize {
    tile.padded_len() * 4 * 2 + 256
}

// ─────────────────────────────────────────────────────────────────────────────
// auto_tile_size
// ─────────────────────────────────────────────────────────────────────────────

/// Compute an auto-selected tile size that fits within the effective VRAM budget.
///
/// Algorithm:
/// 1. Compute the effective budget: `vram_budget_bytes * (1 - safety_margin)`.
/// 2. Start with `(preferred_w, preferred_h)`.
/// 3. On each iteration check whether a synthetic tile of that size fits.
/// 4. If not, halve along the wider axis (alternating width / height).
/// 5. Stop when the tile fits or dimensions reach the floor `(16, 16)`.
///
/// Returns `(tile_width, tile_height)`.
pub fn auto_tile_size(
    preferred_w: usize,
    preferred_h: usize,
    overlap: usize,
    vram_budget_bytes: usize,
    safety_margin: f64,
) -> (usize, usize) {
    let effective_budget =
        (vram_budget_bytes as f64 * (1.0 - safety_margin.clamp(0.0, 0.99))) as usize;

    let mut w = preferred_w.max(16);
    let mut h = preferred_h.max(16);

    // Minimum floor — guarantee forward progress.
    const MIN: usize = 16;

    loop {
        // Synthesise a representative tile using the current dimensions.
        let synthetic = RasterTile {
            data: Vec::new(),
            width: w,
            height: h,
            overlap_top: overlap,
            overlap_right: overlap,
            overlap_bottom: overlap,
            overlap_left: overlap,
            origin_x: 0,
            origin_y: 0,
            raster_width: w,
            raster_height: h,
            tile_index: 0,
        };

        if vram_per_tile(&synthetic) <= effective_budget {
            return (w, h);
        }

        // Tile does not fit — halve the larger dimension.
        if w > MIN || h > MIN {
            // Halve whichever dimension is currently larger (prefer width first
            // when equal so splitting is deterministic).
            if w >= h {
                w = (w / 2).max(MIN);
            } else {
                h = (h / 2).max(MIN);
            }
        } else {
            // Already at minimum floor — return it regardless.
            return (MIN, MIN);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// execute_tiled
// ─────────────────────────────────────────────────────────────────────────────

/// High-level tiled executor: split → per-tile `tile_fn` → stitch.
///
/// `tile_fn(tile) -> GpuResult<Vec<f32>>` receives each tile and must return a
/// `Vec<f32>` whose length equals `tile.padded_len()` (the same layout as
/// `tile.data`).  GPU submission typically happens inside `tile_fn`.
///
/// The stitcher discards the halo and writes only the core interior into the
/// full-resolution output.
///
/// # Errors
///
/// Returns the first error propagated by `tile_fn`.
pub fn execute_tiled<F>(
    raster: &[f32],
    width: usize,
    height: usize,
    config: &TiledConfig,
    tile_fn: F,
) -> GpuResult<Vec<f32>>
where
    F: Fn(&RasterTile) -> GpuResult<Vec<f32>>,
{
    if width == 0 || height == 0 {
        return Ok(Vec::new());
    }

    let tiles = split_into_tiles(raster, width, height, config);

    let mut processed_tiles: Vec<RasterTile> = Vec::with_capacity(tiles.len());

    for tile in &tiles {
        let processed_data = tile_fn(tile).map_err(|e| {
            GpuError::execution_failed(format!(
                "tile {} (origin {},{}) failed: {}",
                tile.tile_index, tile.origin_x, tile.origin_y, e
            ))
        })?;

        // Validate returned length matches expectation.
        if processed_data.len() != tile.padded_len() {
            return Err(GpuError::execution_failed(format!(
                "tile_fn for tile {} returned {} elements but expected {} (padded_len)",
                tile.tile_index,
                processed_data.len(),
                tile.padded_len(),
            )));
        }

        processed_tiles.push(RasterTile {
            data: processed_data,
            width: tile.width,
            height: tile.height,
            overlap_top: tile.overlap_top,
            overlap_right: tile.overlap_right,
            overlap_bottom: tile.overlap_bottom,
            overlap_left: tile.overlap_left,
            origin_x: tile.origin_x,
            origin_y: tile.origin_y,
            raster_width: tile.raster_width,
            raster_height: tile.raster_height,
            tile_index: tile.tile_index,
        });
    }

    Ok(stitch_tiles(&processed_tiles, width, height))
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests (in-module — compile with lib)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tiled_config_default() {
        let cfg = TiledConfig::default();
        assert_eq!(cfg.tile_width, 512);
        assert_eq!(cfg.tile_height, 512);
        assert_eq!(cfg.overlap_pixels, 0);
        assert!((cfg.vram_safety_margin - 0.1).abs() < f64::EPSILON);
    }

    #[test]
    fn test_tiled_config_builder() {
        let cfg = TiledConfig::default()
            .with_tile_size(256, 128)
            .with_overlap(4)
            .with_vram_safety_margin(0.2);
        assert_eq!(cfg.tile_width, 256);
        assert_eq!(cfg.tile_height, 128);
        assert_eq!(cfg.overlap_pixels, 4);
        assert!((cfg.vram_safety_margin - 0.2).abs() < f64::EPSILON);
    }

    #[test]
    fn test_raster_tile_padded_dimensions() {
        let tile = RasterTile {
            data: vec![0.0; (10 + 2 + 2) * (8 + 3 + 3)],
            width: 10,
            height: 8,
            overlap_top: 3,
            overlap_right: 2,
            overlap_bottom: 3,
            overlap_left: 2,
            origin_x: 0,
            origin_y: 0,
            raster_width: 100,
            raster_height: 100,
            tile_index: 0,
        };
        assert_eq!(tile.padded_width(), 14);
        assert_eq!(tile.padded_height(), 14);
        assert_eq!(tile.padded_len(), 196);
    }
}
