//! Shared tile-based inference for large rasters
//!
//! This module provides a unified tiling infrastructure used by both
//! the preprocessing pipeline (`tile_raster()`) and the super-resolution
//! model (`extract_tiles()`/`merge_tiles()`). It also supports detection-
//! specific tile merging (NMS across tile boundaries) and a streaming
//! mode for memory-efficient processing of large rasters.
//!
//! # Architecture
//!
//! - [`TileSpec`]: Pure geometry — pixel offsets and dimensions, no pixel data.
//! - [`compute_tile_grid`]: Stride-based tile layout over an image.
//! - [`compute_blend_weight`]: Distance-from-edge linear weight for overlap blending.
//! - [`BlendStrategy`]: Controls how overlapping tile results are combined.
//! - [`merge_tile_detections`]: Offsets per-tile detection bboxes to full-image space,
//!   then runs NMS on the merged set.
//! - [`TileSource`] / [`TileIterator`]: Streaming mode — read tiles on demand
//!   from an arbitrary backing store without loading the full raster into memory.
//!
//! # Example
//!
//! ```
//! use oxigeo_ml::tiling::{compute_tile_grid, compute_blend_weight, BlendStrategy};
//!
//! // Compute tile layout for a 1024x768 image with 256x256 tiles and 32px overlap
//! let tiles = compute_tile_grid(1024, 768, 256, 256, 32).ok().unwrap_or_default();
//! assert!(!tiles.is_empty());
//!
//! // Compute blend weight at the center of a tile
//! let w = compute_blend_weight(128, 128, 256, 256, 32);
//! assert!((w - 1.0).abs() < f32::EPSILON);
//! ```

use oxigeo_core::buffer::RasterBuffer;
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::detection::{BoundingBox, Detection, NmsConfig, non_maximum_suppression};
use crate::error::{PreprocessingError, Result};

// ─── TileSpec ────────────────────────────────────────────────────────────────

/// Pure-geometry description of a single tile within a larger image.
///
/// `TileSpec` carries no pixel data; it describes *where* a tile lives
/// within the original image and how large it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TileSpec {
    /// Horizontal offset of the tile's top-left corner (pixels).
    pub x_offset: usize,
    /// Vertical offset of the tile's top-left corner (pixels).
    pub y_offset: usize,
    /// Width of this tile (may be smaller than the requested tile size
    /// for edge tiles).
    pub width: usize,
    /// Height of this tile (may be smaller than the requested tile size
    /// for edge tiles).
    pub height: usize,
    /// Width of the original (full) image.
    pub original_width: usize,
    /// Height of the original (full) image.
    pub original_height: usize,
}

impl TileSpec {
    /// Creates a new tile specification.
    #[must_use]
    pub fn new(
        x_offset: usize,
        y_offset: usize,
        width: usize,
        height: usize,
        original_width: usize,
        original_height: usize,
    ) -> Self {
        Self {
            x_offset,
            y_offset,
            width,
            height,
            original_width,
            original_height,
        }
    }

    /// Returns `true` if the point `(px, py)` in image coordinates falls
    /// within this tile.
    #[must_use]
    pub fn contains_point(&self, px: usize, py: usize) -> bool {
        px >= self.x_offset
            && px < self.x_offset + self.width
            && py >= self.y_offset
            && py < self.y_offset + self.height
    }

    /// Returns the tile area in pixels.
    #[must_use]
    pub fn area(&self) -> usize {
        self.width * self.height
    }

    /// Returns `true` if this tile overlaps with another tile.
    #[must_use]
    pub fn overlaps(&self, other: &Self) -> bool {
        let self_x2 = self.x_offset + self.width;
        let self_y2 = self.y_offset + self.height;
        let other_x2 = other.x_offset + other.width;
        let other_y2 = other.y_offset + other.height;

        self.x_offset < other_x2
            && other.x_offset < self_x2
            && self.y_offset < other_y2
            && other.y_offset < self_y2
    }
}

// ─── Tile Grid ───────────────────────────────────────────────────────────────

/// Computes a stride-based tile grid for the given image dimensions.
///
/// The grid covers the entire image. Edge tiles are clipped to the image
/// boundary so their `width`/`height` may be smaller than `tile_width`/
/// `tile_height`.
///
/// # Arguments
///
/// * `image_width`  — Width of the full image in pixels.
/// * `image_height` — Height of the full image in pixels.
/// * `tile_width`   — Requested tile width in pixels.
/// * `tile_height`  — Requested tile height in pixels.
/// * `overlap`      — Overlap between adjacent tiles in pixels.
///
/// # Errors
///
/// Returns an error if the tile dimensions are zero or if the overlap
/// is larger than the tile size (which would cause zero or negative
/// stride).
pub fn compute_tile_grid(
    image_width: usize,
    image_height: usize,
    tile_width: usize,
    tile_height: usize,
    overlap: usize,
) -> Result<Vec<TileSpec>> {
    if tile_width == 0 || tile_height == 0 {
        return Err(PreprocessingError::InvalidTileSize {
            width: tile_width,
            height: tile_height,
        }
        .into());
    }

    if image_width == 0 || image_height == 0 {
        return Ok(Vec::new());
    }

    let stride_x = tile_width.saturating_sub(overlap);
    let stride_y = tile_height.saturating_sub(overlap);

    if stride_x == 0 || stride_y == 0 {
        return Err(PreprocessingError::TilingFailed {
            reason: "Overlap is too large for the tile size".to_string(),
        }
        .into());
    }

    let mut tiles = Vec::new();

    let mut y = 0_usize;
    while y < image_height {
        let mut x = 0_usize;
        while x < image_width {
            let tw = tile_width.min(image_width - x);
            let th = tile_height.min(image_height - y);

            tiles.push(TileSpec {
                x_offset: x,
                y_offset: y,
                width: tw,
                height: th,
                original_width: image_width,
                original_height: image_height,
            });

            x = x.saturating_add(stride_x);
            if x >= image_width {
                break;
            }
        }

        y = y.saturating_add(stride_y);
        if y >= image_height {
            break;
        }
    }

    debug!(
        "Computed tile grid: {}x{} image, {}x{} tiles, {} overlap -> {} tiles",
        image_width,
        image_height,
        tile_width,
        tile_height,
        overlap,
        tiles.len()
    );

    Ok(tiles)
}

// ─── Blend Weights ───────────────────────────────────────────────────────────

/// Strategy for combining overlapping tile results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BlendStrategy {
    /// Weighted average using distance-from-edge weights (default).
    /// Best for continuous outputs like segmentation probability maps
    /// and super-resolution pixel values.
    #[default]
    WeightedAverage,
    /// Take the pixel value from the tile with the highest weight
    /// (i.e. the tile where the pixel is furthest from any edge).
    /// Good for hard segmentation masks.
    MaxConfidence,
    /// No blending — last-writer-wins. Suitable for detection pipelines
    /// where tile outputs are bounding boxes, not pixel grids.
    None,
}

/// Computes a blend weight for a pixel at `(x, y)` within a tile
/// of size `tile_w x tile_h` with the given overlap.
///
/// The weight is 1.0 at the tile center and decreases linearly
/// toward the edges within the overlap band. This function does
/// **not** apply a minimum floor — callers that need a floor
/// (e.g. `inference.rs`) should apply it themselves.
///
/// # Arguments
///
/// * `x`        — X coordinate within the tile (0-based).
/// * `y`        — Y coordinate within the tile (0-based).
/// * `tile_w`   — Width of the tile.
/// * `tile_h`   — Height of the tile.
/// * `overlap`  — Overlap band in pixels.
///
/// # Returns
///
/// A weight in the range `[0.0, 1.0]`.
#[must_use]
pub fn compute_blend_weight(
    x: usize,
    y: usize,
    tile_w: usize,
    tile_h: usize,
    overlap: usize,
) -> f32 {
    if overlap == 0 {
        return 1.0;
    }

    let dist_left = x;
    let dist_right = tile_w.saturating_sub(x + 1);
    let dist_top = y;
    let dist_bottom = tile_h.saturating_sub(y + 1);

    let min_dist = dist_left.min(dist_right).min(dist_top).min(dist_bottom);

    if min_dist >= overlap {
        1.0
    } else {
        min_dist as f32 / overlap as f32
    }
}

// ─── Detection Tile Merging ──────────────────────────────────────────────────

/// Merges per-tile detection results into a single set of detections
/// in full-image coordinates, then applies NMS to eliminate duplicates
/// in overlap regions.
///
/// Each detection's bounding box is offset by the tile's position so
/// that coordinates are in full-image pixel space. After merging, NMS
/// is applied to the combined set.
///
/// # Arguments
///
/// * `tiles`                — Tile specifications (one per tile).
/// * `per_tile_detections`  — Detections for each tile (parallel to `tiles`).
/// * `nms_config`           — NMS configuration for the final pass.
///
/// # Errors
///
/// Returns an error if the tile/detection slice lengths do not match
/// or if NMS itself fails.
pub fn merge_tile_detections(
    tiles: &[TileSpec],
    per_tile_detections: &[Vec<Detection>],
    nms_config: &NmsConfig,
) -> Result<Vec<Detection>> {
    if tiles.len() != per_tile_detections.len() {
        return Err(PreprocessingError::TilingFailed {
            reason: format!(
                "Tile count ({}) does not match detection count ({})",
                tiles.len(),
                per_tile_detections.len()
            ),
        }
        .into());
    }

    // Offset each detection's bbox to full-image coordinates
    let mut all_detections = Vec::new();
    for (tile, dets) in tiles.iter().zip(per_tile_detections.iter()) {
        for det in dets {
            let offset_bbox = BoundingBox::new(
                det.bbox.x + tile.x_offset as f32,
                det.bbox.y + tile.y_offset as f32,
                det.bbox.width,
                det.bbox.height,
            );
            all_detections.push(Detection {
                bbox: offset_bbox,
                class_id: det.class_id,
                class_label: det.class_label.clone(),
                confidence: det.confidence,
                attributes: det.attributes.clone(),
            });
        }
    }

    debug!(
        "Merged {} detections from {} tiles, running NMS",
        all_detections.len(),
        tiles.len()
    );

    // Run NMS on the merged set to remove duplicates in overlap regions
    non_maximum_suppression(&all_detections, nms_config)
}

// ─── Streaming: TileSource / TileIterator ────────────────────────────────────

/// A source of raster tile data that can read arbitrary regions.
///
/// Implementations can back this with in-memory buffers, memory-mapped
/// files, cloud object stores, etc. This allows tile-based inference
/// to proceed without loading the entire raster into memory.
pub trait TileSource: Send + Sync {
    /// Reads a rectangular region from the source raster.
    ///
    /// # Arguments
    ///
    /// * `x` — Left edge of the region (pixels).
    /// * `y` — Top edge of the region (pixels).
    /// * `w` — Width of the region (pixels).
    /// * `h` — Height of the region (pixels).
    ///
    /// # Errors
    ///
    /// Returns an error if the region is out of bounds or the underlying
    /// I/O fails.
    fn read_region(&self, x: usize, y: usize, w: usize, h: usize) -> Result<RasterBuffer>;

    /// Returns the full width of the source raster.
    fn width(&self) -> usize;

    /// Returns the full height of the source raster.
    fn height(&self) -> usize;
}

/// An in-memory implementation of [`TileSource`] backed by a
/// [`RasterBuffer`].
pub struct InMemoryTileSource {
    buffer: RasterBuffer,
}

impl InMemoryTileSource {
    /// Creates a new in-memory tile source from an existing raster buffer.
    #[must_use]
    pub fn new(buffer: RasterBuffer) -> Self {
        Self { buffer }
    }
}

impl TileSource for InMemoryTileSource {
    fn read_region(&self, x: usize, y: usize, w: usize, h: usize) -> Result<RasterBuffer> {
        let mut tile = RasterBuffer::zeros(w as u64, h as u64, self.buffer.data_type());

        for ty in 0..h {
            for tx in 0..w {
                let src_x = (x + tx) as u64;
                let src_y = (y + ty) as u64;
                if src_x < self.buffer.width() && src_y < self.buffer.height() {
                    let pixel = self.buffer.get_pixel(src_x, src_y).map_err(|e| {
                        PreprocessingError::TilingFailed {
                            reason: format!("Failed to read pixel ({}, {}): {}", src_x, src_y, e),
                        }
                    })?;
                    tile.set_pixel(tx as u64, ty as u64, pixel).map_err(|e| {
                        PreprocessingError::TilingFailed {
                            reason: format!("Failed to write pixel ({}, {}): {}", tx, ty, e),
                        }
                    })?;
                }
            }
        }

        Ok(tile)
    }

    fn width(&self) -> usize {
        self.buffer.width() as usize
    }

    fn height(&self) -> usize {
        self.buffer.height() as usize
    }
}

/// An iterator that yields `(TileSpec, RasterBuffer)` pairs from a
/// [`TileSource`].
///
/// This allows streaming tile-based inference: tiles are read from the
/// source one at a time (or in small batches) so only a few tiles
/// need to be in memory at once.
pub struct TileIterator {
    source: Box<dyn TileSource>,
    specs: Vec<TileSpec>,
    index: usize,
}

impl TileIterator {
    /// Creates a new tile iterator over the given source.
    ///
    /// The tile grid is computed from the source dimensions plus the
    /// provided tile size and overlap.
    ///
    /// # Errors
    ///
    /// Returns an error if the tile grid computation fails (e.g. zero
    /// tile size or overlap too large).
    pub fn new(
        source: Box<dyn TileSource>,
        tile_width: usize,
        tile_height: usize,
        overlap: usize,
    ) -> Result<Self> {
        let w = source.width();
        let h = source.height();
        let specs = compute_tile_grid(w, h, tile_width, tile_height, overlap)?;
        Ok(Self {
            source,
            specs,
            index: 0,
        })
    }

    /// Returns the total number of tiles in the grid.
    #[must_use]
    pub fn tile_count(&self) -> usize {
        self.specs.len()
    }

    /// Returns a slice of all tile specifications.
    #[must_use]
    pub fn specs(&self) -> &[TileSpec] {
        &self.specs
    }

    /// Returns the number of tiles that have already been yielded.
    #[must_use]
    pub fn tiles_yielded(&self) -> usize {
        self.index
    }

    /// Reads and returns the next tile, or `None` if all tiles have
    /// been yielded.
    ///
    /// # Errors
    ///
    /// Returns an error if reading from the source fails.
    pub fn next_tile(&mut self) -> Result<Option<(TileSpec, RasterBuffer)>> {
        if self.index >= self.specs.len() {
            return Ok(None);
        }

        let spec = self.specs[self.index];
        let buf = self
            .source
            .read_region(spec.x_offset, spec.y_offset, spec.width, spec.height)?;
        self.index += 1;

        Ok(Some((spec, buf)))
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detection::BoundingBox;
    use oxigeo_core::types::RasterDataType;
    use std::collections::HashMap;

    // ─── compute_tile_grid ───────────────────────────────────────────────

    #[test]
    fn test_compute_tile_grid_no_overlap() {
        // 512x512 image, 256x256 tiles, 0 overlap -> 4 tiles (2x2)
        let tiles = compute_tile_grid(512, 512, 256, 256, 0)
            .ok()
            .unwrap_or_default();
        assert_eq!(tiles.len(), 4);

        // Check positions
        assert_eq!(tiles[0].x_offset, 0);
        assert_eq!(tiles[0].y_offset, 0);
        assert_eq!(tiles[0].width, 256);
        assert_eq!(tiles[0].height, 256);

        assert_eq!(tiles[1].x_offset, 256);
        assert_eq!(tiles[1].y_offset, 0);

        assert_eq!(tiles[2].x_offset, 0);
        assert_eq!(tiles[2].y_offset, 256);

        assert_eq!(tiles[3].x_offset, 256);
        assert_eq!(tiles[3].y_offset, 256);
    }

    #[test]
    fn test_compute_tile_grid_with_overlap() {
        // 512x512 image, 256x256 tiles, 32 overlap
        // stride = 256 - 32 = 224
        // x positions: 0, 224, 448 (448+256=704 > 512, so tile_w = 512-448 = 64)
        // y positions: same
        // -> 3 x 3 = 9 tiles
        let tiles = compute_tile_grid(512, 512, 256, 256, 32)
            .ok()
            .unwrap_or_default();
        assert_eq!(tiles.len(), 9);

        // Verify first tile
        assert_eq!(tiles[0].x_offset, 0);
        assert_eq!(tiles[0].y_offset, 0);
        assert_eq!(tiles[0].width, 256);
        assert_eq!(tiles[0].height, 256);

        // Verify last tile (edge tile) is clipped
        let last = &tiles[tiles.len() - 1];
        assert_eq!(last.x_offset, 448);
        assert_eq!(last.y_offset, 448);
        assert_eq!(last.width, 64);
        assert_eq!(last.height, 64);
    }

    #[test]
    fn test_compute_tile_grid_non_divisible() {
        // 500x300 image, 256x256 tiles, 0 overlap
        // x: 0, 256 (256+256=512 > 500, so tile_w = 500-256 = 244)
        // y: 0, 256 (256+256=512 > 300, so tile_h = 300-256 = 44)
        // -> 2 x 2 = 4 tiles
        let tiles = compute_tile_grid(500, 300, 256, 256, 0)
            .ok()
            .unwrap_or_default();
        assert_eq!(tiles.len(), 4);

        // First tile: full size
        assert_eq!(tiles[0].width, 256);
        assert_eq!(tiles[0].height, 256);

        // Last tile: clipped
        let last = &tiles[tiles.len() - 1];
        assert_eq!(last.x_offset, 256);
        assert_eq!(last.y_offset, 256);
        assert_eq!(last.width, 244);
        assert_eq!(last.height, 44);
    }

    #[test]
    fn test_compute_tile_grid_zero_tile_size() {
        let result = compute_tile_grid(512, 512, 0, 256, 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_compute_tile_grid_overlap_too_large() {
        let result = compute_tile_grid(512, 512, 256, 256, 256);
        assert!(result.is_err());
    }

    #[test]
    fn test_compute_tile_grid_zero_image() {
        let tiles = compute_tile_grid(0, 0, 256, 256, 0)
            .ok()
            .unwrap_or_default();
        assert!(tiles.is_empty());
    }

    #[test]
    fn test_compute_tile_grid_single_tile() {
        // Image smaller than tile -> 1 tile
        let tiles = compute_tile_grid(100, 100, 256, 256, 0)
            .ok()
            .unwrap_or_default();
        assert_eq!(tiles.len(), 1);
        assert_eq!(tiles[0].width, 100);
        assert_eq!(tiles[0].height, 100);
    }

    // ─── compute_blend_weight ────────────────────────────────────────────

    #[test]
    fn test_blend_weight_center_is_one() {
        // Center pixel of a 256x256 tile with 32 overlap
        let w = compute_blend_weight(128, 128, 256, 256, 32);
        assert!(
            (w - 1.0).abs() < f32::EPSILON,
            "Center weight should be 1.0, got {}",
            w
        );
    }

    #[test]
    fn test_blend_weight_edge_is_low() {
        // Top-left corner with overlap
        let w = compute_blend_weight(0, 0, 256, 256, 32);
        assert!(w < 1.0, "Edge weight should be < 1.0, got {}", w);
        assert!(w >= 0.0, "Edge weight should be >= 0.0, got {}", w);
        // (0, 0) -> min_dist = 0 -> weight = 0.0
        assert!(
            w.abs() < f32::EPSILON,
            "Corner weight should be ~0.0, got {}",
            w
        );
    }

    #[test]
    fn test_blend_weight_no_overlap() {
        // With 0 overlap, all weights should be 1.0
        let w = compute_blend_weight(0, 0, 256, 256, 0);
        assert!((w - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_blend_weight_linear_ramp() {
        // Weight should increase linearly within the overlap band
        let w1 = compute_blend_weight(5, 128, 256, 256, 32);
        let w2 = compute_blend_weight(10, 128, 256, 256, 32);
        let w3 = compute_blend_weight(20, 128, 256, 256, 32);
        assert!(w1 < w2);
        assert!(w2 < w3);
        assert!(w3 < 1.0);
    }

    // ─── BlendStrategy ───────────────────────────────────────────────────

    #[test]
    fn test_blend_strategy_default() {
        let strategy = BlendStrategy::default();
        assert_eq!(strategy, BlendStrategy::WeightedAverage);
    }

    // ─── TileSpec ────────────────────────────────────────────────────────

    #[test]
    fn test_tile_spec_contains_point() {
        let spec = TileSpec::new(100, 200, 256, 256, 1024, 768);

        // Inside
        assert!(spec.contains_point(100, 200));
        assert!(spec.contains_point(200, 300));
        assert!(spec.contains_point(355, 455)); // boundary: 100+255, 200+255

        // Outside
        assert!(!spec.contains_point(99, 200));
        assert!(!spec.contains_point(100, 199));
        assert!(!spec.contains_point(356, 200)); // 100 + 256 = 356 is out
        assert!(!spec.contains_point(100, 456)); // 200 + 256 = 456 is out
    }

    #[test]
    fn test_tile_spec_area() {
        let spec = TileSpec::new(0, 0, 100, 200, 512, 512);
        assert_eq!(spec.area(), 20_000);
    }

    #[test]
    fn test_tile_spec_overlaps() {
        let a = TileSpec::new(0, 0, 100, 100, 512, 512);
        let b = TileSpec::new(50, 50, 100, 100, 512, 512);
        let c = TileSpec::new(200, 200, 100, 100, 512, 512);

        assert!(a.overlaps(&b));
        assert!(b.overlaps(&a));
        assert!(!a.overlaps(&c));
        assert!(!c.overlaps(&a));
    }

    // ─── merge_tile_detections ───────────────────────────────────────────

    fn make_detection(x: f32, y: f32, w: f32, h: f32, conf: f32, class_id: usize) -> Detection {
        Detection {
            bbox: BoundingBox::new(x, y, w, h),
            class_id,
            class_label: None,
            confidence: conf,
            attributes: HashMap::new(),
        }
    }

    #[test]
    fn test_merge_tile_detections() {
        // Two adjacent tiles with overlap. Each tile detects the same object
        // in the overlap zone (duplicate detection that NMS should remove).
        let tiles = vec![
            TileSpec::new(0, 0, 256, 256, 512, 256),
            TileSpec::new(224, 0, 256, 256, 512, 256),
        ];

        let per_tile = vec![
            // Tile 0: detection near right edge (in overlap zone)
            vec![make_detection(210.0, 50.0, 30.0, 30.0, 0.9, 0)],
            // Tile 1: same object, at local coords (210-224=-14 -> but detector sees
            // it at a slightly different position, e.g. x=0)
            // After offset: 224 + 0 = 224, which overlaps with 210..240 from tile 0
            vec![make_detection(0.0, 50.0, 30.0, 30.0, 0.85, 0)],
        ];

        let nms_config = NmsConfig {
            iou_threshold: 0.3,
            confidence_threshold: 0.5,
            max_detections: None,
            ..NmsConfig::default()
        };

        let result = merge_tile_detections(&tiles, &per_tile, &nms_config)
            .ok()
            .unwrap_or_default();

        // The two detections overlap significantly (IoU > 0.3), NMS should keep 1
        assert_eq!(result.len(), 1);
        // The higher-confidence detection is kept
        assert!((result[0].confidence - 0.9).abs() < f32::EPSILON);
    }

    #[test]
    fn test_merge_tile_detections_preserves_non_overlapping() {
        // Two tiles with detections far apart — both should survive NMS.
        let tiles = vec![
            TileSpec::new(0, 0, 256, 256, 512, 256),
            TileSpec::new(256, 0, 256, 256, 512, 256),
        ];

        let per_tile = vec![
            vec![make_detection(10.0, 10.0, 20.0, 20.0, 0.9, 0)],
            vec![make_detection(10.0, 10.0, 20.0, 20.0, 0.8, 0)],
        ];

        let nms_config = NmsConfig {
            iou_threshold: 0.5,
            confidence_threshold: 0.5,
            max_detections: None,
            ..NmsConfig::default()
        };

        let result = merge_tile_detections(&tiles, &per_tile, &nms_config)
            .ok()
            .unwrap_or_default();

        // Detections are far apart (10 vs 266 in image coords), both kept
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_merge_tile_detections_mismatched_lengths() {
        let tiles = vec![TileSpec::new(0, 0, 256, 256, 512, 512)];
        let per_tile: Vec<Vec<Detection>> = vec![vec![], vec![]];

        let result = merge_tile_detections(&tiles, &per_tile, &NmsConfig::default());
        assert!(result.is_err());
    }

    #[test]
    fn test_merge_tile_detections_empty() {
        let tiles: Vec<TileSpec> = vec![];
        let per_tile: Vec<Vec<Detection>> = vec![];

        let result = merge_tile_detections(&tiles, &per_tile, &NmsConfig::default())
            .ok()
            .unwrap_or_default();
        assert!(result.is_empty());
    }

    #[test]
    fn test_merge_tile_detections_offsets_bbox() {
        // Verify that bbox coordinates are offset by tile position
        let tiles = vec![TileSpec::new(100, 200, 256, 256, 512, 512)];
        let per_tile = vec![vec![make_detection(10.0, 20.0, 30.0, 30.0, 0.9, 0)]];

        let nms_config = NmsConfig {
            confidence_threshold: 0.1,
            ..NmsConfig::default()
        };

        let result = merge_tile_detections(&tiles, &per_tile, &nms_config)
            .ok()
            .unwrap_or_default();

        assert_eq!(result.len(), 1);
        assert!((result[0].bbox.x - 110.0).abs() < f32::EPSILON);
        assert!((result[0].bbox.y - 220.0).abs() < f32::EPSILON);
    }

    // ─── Streaming: TileSource / TileIterator ────────────────────────────

    #[test]
    fn test_in_memory_tile_source() {
        let buf = RasterBuffer::zeros(512, 512, RasterDataType::Float32);
        let source = InMemoryTileSource::new(buf);
        assert_eq!(source.width(), 512);
        assert_eq!(source.height(), 512);

        let region = source.read_region(100, 100, 50, 50);
        assert!(region.is_ok());
        let region = region
            .ok()
            .unwrap_or_else(|| RasterBuffer::zeros(1, 1, RasterDataType::Float32));
        assert_eq!(region.width(), 50);
        assert_eq!(region.height(), 50);
    }

    #[test]
    fn test_streaming_iterator_yields_all_tiles() {
        let buf = RasterBuffer::zeros(512, 512, RasterDataType::Float32);
        let source = InMemoryTileSource::new(buf);

        let mut iter = TileIterator::new(Box::new(source), 256, 256, 0)
            .ok()
            .unwrap_or_else(|| {
                // Fallback: create with a tiny source so the test can at least run
                let tiny = RasterBuffer::zeros(1, 1, RasterDataType::Float32);
                TileIterator::new(Box::new(InMemoryTileSource::new(tiny)), 1, 1, 0)
                    .ok()
                    .unwrap_or_else(|| TileIterator {
                        source: Box::new(InMemoryTileSource::new(RasterBuffer::zeros(
                            1,
                            1,
                            RasterDataType::Float32,
                        ))),
                        specs: Vec::new(),
                        index: 0,
                    })
            });

        assert_eq!(iter.tile_count(), 4);
        assert_eq!(iter.tiles_yielded(), 0);

        let mut count = 0;
        loop {
            match iter.next_tile() {
                Ok(Some((spec, buf))) => {
                    count += 1;
                    assert_eq!(spec.width as u64, buf.width());
                    assert_eq!(spec.height as u64, buf.height());
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }

        assert_eq!(count, 4);
        assert_eq!(iter.tiles_yielded(), 4);
    }

    #[test]
    fn test_streaming_iterator_with_overlap() {
        let buf = RasterBuffer::zeros(512, 512, RasterDataType::Float32);
        let source = InMemoryTileSource::new(buf);

        let iter = TileIterator::new(Box::new(source), 256, 256, 32)
            .ok()
            .unwrap_or_else(|| {
                let tiny = RasterBuffer::zeros(1, 1, RasterDataType::Float32);
                TileIterator::new(Box::new(InMemoryTileSource::new(tiny)), 1, 1, 0)
                    .ok()
                    .unwrap_or_else(|| TileIterator {
                        source: Box::new(InMemoryTileSource::new(RasterBuffer::zeros(
                            1,
                            1,
                            RasterDataType::Float32,
                        ))),
                        specs: Vec::new(),
                        index: 0,
                    })
            });

        // 512x512 with stride 224 -> 3x3 = 9 tiles
        assert_eq!(iter.tile_count(), 9);
    }

    #[test]
    fn test_tile_iterator_specs() {
        let buf = RasterBuffer::zeros(300, 200, RasterDataType::Float32);
        let source = InMemoryTileSource::new(buf);

        let iter = TileIterator::new(Box::new(source), 256, 256, 0)
            .ok()
            .unwrap_or_else(|| TileIterator {
                source: Box::new(InMemoryTileSource::new(RasterBuffer::zeros(
                    1,
                    1,
                    RasterDataType::Float32,
                ))),
                specs: Vec::new(),
                index: 0,
            });

        let specs = iter.specs();
        // 300x200, 256x256, no overlap -> 2x1 = 2 tiles
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].width, 256);
        assert_eq!(specs[0].height, 200);
        assert_eq!(specs[1].width, 44);
        assert_eq!(specs[1].height, 200);
    }

    // ─── Round-trip: grid equivalence with preprocessing.rs ──────────────

    #[test]
    fn test_grid_equivalence_with_preprocessing() {
        // The tile grid from compute_tile_grid should produce the same
        // offsets and dimensions as the loop in preprocessing::tile_raster.
        let img_w = 500;
        let img_h = 300;
        let tile_w = 256;
        let tile_h = 256;
        let overlap = 32;
        let stride_x = tile_w - overlap;
        let stride_y = tile_h - overlap;

        let specs = compute_tile_grid(img_w, img_h, tile_w, tile_h, overlap)
            .ok()
            .unwrap_or_default();

        // Manually compute expected tiles (same algorithm as preprocessing.rs)
        let mut expected = Vec::new();
        let mut y = 0usize;
        while y < img_h {
            let mut x = 0usize;
            while x < img_w {
                let tw = tile_w.min(img_w - x);
                let th = tile_h.min(img_h - y);
                expected.push((x, y, tw, th));
                x += stride_x;
                if x >= img_w {
                    break;
                }
            }
            y += stride_y;
            if y >= img_h {
                break;
            }
        }

        assert_eq!(specs.len(), expected.len());
        for (spec, (ex, ey, ew, eh)) in specs.iter().zip(expected.iter()) {
            assert_eq!(spec.x_offset, *ex);
            assert_eq!(spec.y_offset, *ey);
            assert_eq!(spec.width, *ew);
            assert_eq!(spec.height, *eh);
        }
    }
}
