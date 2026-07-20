//! Data preprocessing for ML workflows
//!
//! This module provides preprocessing operations for geospatial data
//! before ML inference.

use oxigeo_core::buffer::RasterBuffer;
// use oxigeo_core::types::RasterDataType;
// use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::error::{PreprocessingError, Result};

/// Normalization parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizationParams {
    /// Per-channel mean values
    pub mean: Vec<f64>,
    /// Per-channel standard deviation values
    pub std: Vec<f64>,
}

impl NormalizationParams {
    /// Creates ImageNet normalization parameters
    #[must_use]
    pub fn imagenet() -> Self {
        Self {
            mean: vec![0.485, 0.456, 0.406],
            std: vec![0.229, 0.224, 0.225],
        }
    }

    /// Creates normalization parameters for a given range
    #[must_use]
    pub fn from_range(min: f64, max: f64) -> Self {
        let mean = (min + max) / 2.0;
        let std = (max - min) / 2.0;
        Self {
            mean: vec![mean],
            std: vec![std],
        }
    }

    /// Creates zero-mean unit-variance normalization
    #[must_use]
    pub fn zero_mean_unit_variance() -> Self {
        Self {
            mean: vec![0.0],
            std: vec![1.0],
        }
    }
}

/// Padding strategy for tiles
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaddingStrategy {
    /// Zero padding
    Zero,
    /// Replicate edge values
    Replicate,
    /// Reflect values at boundaries
    Reflect,
    /// Wrap around to opposite edge
    Wrap,
}

/// Tile configuration
#[derive(Debug, Clone)]
pub struct TileConfig {
    /// Tile width
    pub tile_width: usize,
    /// Tile height
    pub tile_height: usize,
    /// Overlap between tiles (in pixels)
    pub overlap: usize,
    /// Padding strategy
    pub padding: PaddingStrategy,
}

impl Default for TileConfig {
    fn default() -> Self {
        Self {
            tile_width: 256,
            tile_height: 256,
            overlap: 32,
            padding: PaddingStrategy::Replicate,
        }
    }
}

/// A single tile from a raster
#[derive(Debug, Clone)]
pub struct Tile {
    /// The tile buffer
    pub buffer: RasterBuffer,
    /// X offset in the original raster
    pub x_offset: u64,
    /// Y offset in the original raster
    pub y_offset: u64,
    /// Original raster width
    pub original_width: u64,
    /// Original raster height
    pub original_height: u64,
}

/// Normalizes a single-band raster buffer using the statistics of one channel.
///
/// [`RasterBuffer`] is architecturally single-band, so a single call normalizes
/// exactly one channel. When `params` carries per-channel statistics (e.g.
/// [`NormalizationParams::imagenet`] with three RGB entries), `channel_idx`
/// selects which channel's mean/std to apply. Callers that hold multi-band data
/// must split it into per-band buffers and invoke `normalize` once per band with
/// the matching `channel_idx` (see `cloud::detection::preprocess` for the
/// per-band pattern).
///
/// # Errors
/// Returns [`PreprocessingError::InvalidNormalization`] if the mean/std vectors
/// are empty or the selected standard deviation is zero, and
/// [`PreprocessingError::ChannelMismatch`] if `channel_idx` is out of range for
/// the supplied statistics.
pub fn normalize(
    buffer: &RasterBuffer,
    params: &NormalizationParams,
    channel_idx: usize,
) -> Result<RasterBuffer> {
    if params.mean.is_empty() || params.std.is_empty() {
        return Err(PreprocessingError::InvalidNormalization {
            message: "Mean and std must not be empty".to_string(),
        }
        .into());
    }

    // Select the statistics for the requested channel. Using `get` (rather than
    // indexing) makes the bounds check explicit and panic-free, and surfaces a
    // typed error instead of silently defaulting to channel 0.
    let mean =
        *params
            .mean
            .get(channel_idx)
            .ok_or_else(|| PreprocessingError::ChannelMismatch {
                expected: channel_idx + 1,
                actual: params.mean.len(),
            })?;
    let std = *params
        .std
        .get(channel_idx)
        .ok_or_else(|| PreprocessingError::ChannelMismatch {
            expected: channel_idx + 1,
            actual: params.std.len(),
        })?;

    if std == 0.0 {
        return Err(PreprocessingError::InvalidNormalization {
            message: "Standard deviation cannot be zero".to_string(),
        }
        .into());
    }

    let mut result = buffer.clone();

    // Normalize each pixel using the selected channel's statistics.
    for y in 0..buffer.height() {
        for x in 0..buffer.width() {
            let pixel =
                buffer
                    .get_pixel(x, y)
                    .map_err(|e| PreprocessingError::InvalidNormalization {
                        message: format!("Failed to get pixel: {}", e),
                    })?;

            let normalized = (pixel - mean) / std;

            result.set_pixel(x, y, normalized).map_err(|e| {
                PreprocessingError::InvalidNormalization {
                    message: format!("Failed to set pixel: {}", e),
                }
            })?;
        }
    }

    Ok(result)
}

/// Tiles a raster buffer into smaller tiles.
///
/// This delegates to [`crate::tiling::compute_tile_grid`] for the tile
/// layout, then extracts pixel data for each tile.
///
/// # Errors
/// Returns an error if tiling fails
pub fn tile_raster(buffer: &RasterBuffer, config: &TileConfig) -> Result<Vec<Tile>> {
    let width = buffer.width() as usize;
    let height = buffer.height() as usize;

    debug!(
        "Tiling {}x{} raster into {}x{} tiles with {} overlap",
        width, height, config.tile_width, config.tile_height, config.overlap
    );

    // Compute tile layout via the shared tiling module
    let specs = crate::tiling::compute_tile_grid(
        width,
        height,
        config.tile_width,
        config.tile_height,
        config.overlap,
    )?;

    let mut tiles = Vec::with_capacity(specs.len());

    for spec in &specs {
        let x = spec.x_offset as u64;
        let y = spec.y_offset as u64;
        let tw = spec.width as u64;
        let th = spec.height as u64;

        let tile_buffer = extract_tile(buffer, x, y, tw, th, config)?;

        tiles.push(Tile {
            buffer: tile_buffer,
            x_offset: x,
            y_offset: y,
            original_width: buffer.width(),
            original_height: buffer.height(),
        });
    }

    debug!("Created {} tiles", tiles.len());

    Ok(tiles)
}

/// Extracts a tile from a raster buffer
fn extract_tile(
    buffer: &RasterBuffer,
    x: u64,
    y: u64,
    width: u64,
    height: u64,
    config: &TileConfig,
) -> Result<RasterBuffer> {
    let mut tile = RasterBuffer::zeros(
        config.tile_width as u64,
        config.tile_height as u64,
        buffer.data_type(),
    );

    // Copy pixels from source to tile
    for ty in 0..height {
        for tx in 0..width {
            let src_x = x + tx;
            let src_y = y + ty;

            let pixel =
                buffer
                    .get_pixel(src_x, src_y)
                    .map_err(|e| PreprocessingError::TilingFailed {
                        reason: format!("Failed to get pixel: {}", e),
                    })?;

            tile.set_pixel(tx, ty, pixel)
                .map_err(|e| PreprocessingError::TilingFailed {
                    reason: format!("Failed to set pixel: {}", e),
                })?;
        }
    }

    // Apply padding if tile is smaller than requested size
    if width < config.tile_width as u64 || height < config.tile_height as u64 {
        apply_padding(&mut tile, width, height, config.padding)?;
    }

    Ok(tile)
}

/// Applies padding to a tile
fn apply_padding(
    tile: &mut RasterBuffer,
    valid_width: u64,
    valid_height: u64,
    strategy: PaddingStrategy,
) -> Result<()> {
    let tile_width = tile.width();
    let tile_height = tile.height();

    match strategy {
        PaddingStrategy::Zero => {
            // Zeros are already filled by RasterBuffer::zeros
            Ok(())
        }
        PaddingStrategy::Replicate => {
            // Replicate right edge
            if valid_width < tile_width {
                let edge_x = valid_width.saturating_sub(1);
                for y in 0..valid_height {
                    let edge_value = tile.get_pixel(edge_x, y).map_err(|e| {
                        PreprocessingError::PaddingFailed {
                            reason: format!("Failed to get edge pixel: {}", e),
                        }
                    })?;
                    for x in valid_width..tile_width {
                        tile.set_pixel(x, y, edge_value).map_err(|e| {
                            PreprocessingError::PaddingFailed {
                                reason: format!("Failed to set pixel: {}", e),
                            }
                        })?;
                    }
                }
            }

            // Replicate bottom edge
            if valid_height < tile_height {
                let edge_y = valid_height.saturating_sub(1);
                for x in 0..tile_width {
                    let edge_value = tile.get_pixel(x, edge_y).map_err(|e| {
                        PreprocessingError::PaddingFailed {
                            reason: format!("Failed to get edge pixel: {}", e),
                        }
                    })?;
                    for y in valid_height..tile_height {
                        tile.set_pixel(x, y, edge_value).map_err(|e| {
                            PreprocessingError::PaddingFailed {
                                reason: format!("Failed to set pixel: {}", e),
                            }
                        })?;
                    }
                }
            }

            Ok(())
        }
        PaddingStrategy::Reflect => {
            // Simplified reflection padding
            if valid_width < tile_width {
                for y in 0..valid_height {
                    for x in valid_width..tile_width {
                        let reflect_x =
                            valid_width.saturating_sub((x - valid_width + 1).min(valid_width));
                        let value = tile.get_pixel(reflect_x, y).map_err(|e| {
                            PreprocessingError::PaddingFailed {
                                reason: format!("Failed to get reflected pixel: {}", e),
                            }
                        })?;
                        tile.set_pixel(x, y, value).map_err(|e| {
                            PreprocessingError::PaddingFailed {
                                reason: format!("Failed to set pixel: {}", e),
                            }
                        })?;
                    }
                }
            }

            if valid_height < tile_height {
                for x in 0..tile_width {
                    for y in valid_height..tile_height {
                        let reflect_y =
                            valid_height.saturating_sub((y - valid_height + 1).min(valid_height));
                        let value = tile.get_pixel(x, reflect_y).map_err(|e| {
                            PreprocessingError::PaddingFailed {
                                reason: format!("Failed to get reflected pixel: {}", e),
                            }
                        })?;
                        tile.set_pixel(x, y, value).map_err(|e| {
                            PreprocessingError::PaddingFailed {
                                reason: format!("Failed to set pixel: {}", e),
                            }
                        })?;
                    }
                }
            }

            Ok(())
        }
        PaddingStrategy::Wrap => {
            // Wrap around to opposite edge
            if valid_width < tile_width && valid_width > 0 {
                for y in 0..valid_height {
                    for x in valid_width..tile_width {
                        let wrap_x = (x - valid_width) % valid_width;
                        let value = tile.get_pixel(wrap_x, y).map_err(|e| {
                            PreprocessingError::PaddingFailed {
                                reason: format!("Failed to get wrapped pixel: {}", e),
                            }
                        })?;
                        tile.set_pixel(x, y, value).map_err(|e| {
                            PreprocessingError::PaddingFailed {
                                reason: format!("Failed to set pixel: {}", e),
                            }
                        })?;
                    }
                }
            }

            if valid_height < tile_height && valid_height > 0 {
                for x in 0..tile_width {
                    for y in valid_height..tile_height {
                        let wrap_y = (y - valid_height) % valid_height;
                        let value = tile.get_pixel(x, wrap_y).map_err(|e| {
                            PreprocessingError::PaddingFailed {
                                reason: format!("Failed to get wrapped pixel: {}", e),
                            }
                        })?;
                        tile.set_pixel(x, y, value).map_err(|e| {
                            PreprocessingError::PaddingFailed {
                                reason: format!("Failed to set pixel: {}", e),
                            }
                        })?;
                    }
                }
            }

            Ok(())
        }
    }
}

/// Resizes a raster buffer using nearest neighbor interpolation
///
/// # Errors
/// Returns an error if resizing fails
pub fn resize_nearest(
    buffer: &RasterBuffer,
    new_width: u64,
    new_height: u64,
) -> Result<RasterBuffer> {
    let mut result = RasterBuffer::zeros(new_width, new_height, buffer.data_type());

    let x_ratio = buffer.width() as f64 / new_width as f64;
    let y_ratio = buffer.height() as f64 / new_height as f64;

    for y in 0..new_height {
        for x in 0..new_width {
            let src_x = (x as f64 * x_ratio) as u64;
            let src_y = (y as f64 * y_ratio) as u64;

            let pixel = buffer.get_pixel(src_x, src_y).map_err(|e| {
                PreprocessingError::InvalidNormalization {
                    message: format!("Failed to get pixel during resize: {}", e),
                }
            })?;

            result.set_pixel(x, y, pixel).map_err(|e| {
                PreprocessingError::InvalidNormalization {
                    message: format!("Failed to set pixel during resize: {}", e),
                }
            })?;
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigeo_core::types::RasterDataType;

    #[test]
    fn test_normalization_params() {
        let params = NormalizationParams::imagenet();
        assert_eq!(params.mean.len(), 3);
        assert_eq!(params.std.len(), 3);

        let params = NormalizationParams::from_range(0.0, 255.0);
        assert!((params.mean[0] - 127.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_normalize() {
        let buffer = RasterBuffer::zeros(10, 10, RasterDataType::Float32);
        let params = NormalizationParams::zero_mean_unit_variance();

        let result = normalize(&buffer, &params, 0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_normalize_uses_selected_channel_stats() {
        // A single-band buffer filled with a known constant, normalized against
        // ImageNet's 3-channel statistics, must use the requested channel's
        // mean/std -- not always channel 0.
        // Float64 buffer so the assertions can use a tight tolerance.
        let mut buffer = RasterBuffer::zeros(2, 2, RasterDataType::Float64);
        for y in 0..2 {
            for x in 0..2 {
                let _ = buffer.set_pixel(x, y, 1.0);
            }
        }
        let params = NormalizationParams::imagenet();

        // Channel 1 (G): (1.0 - 0.456) / 0.224
        let g = normalize(&buffer, &params, 1).expect("normalize channel 1");
        let expected_g = (1.0 - 0.456) / 0.224;
        assert!((g.get_pixel(0, 0).unwrap_or(0.0) - expected_g).abs() < 1e-9);

        // Channel 2 (B): (1.0 - 0.406) / 0.225
        let b = normalize(&buffer, &params, 2).expect("normalize channel 2");
        let expected_b = (1.0 - 0.406) / 0.225;
        assert!((b.get_pixel(0, 0).unwrap_or(0.0) - expected_b).abs() < 1e-9);

        // The channel-0 (R) result must differ from G and B, proving no silent
        // fallback to index 0.
        let r = normalize(&buffer, &params, 0).expect("normalize channel 0");
        let expected_r = (1.0 - 0.485) / 0.229;
        assert!((r.get_pixel(0, 0).unwrap_or(0.0) - expected_r).abs() < 1e-9);
        assert!((expected_r - expected_g).abs() > 1e-6);
        assert!((expected_r - expected_b).abs() > 1e-6);
    }

    #[test]
    fn test_normalize_channel_out_of_range() {
        let buffer = RasterBuffer::zeros(2, 2, RasterDataType::Float32);
        let params = NormalizationParams::zero_mean_unit_variance(); // len == 1
        // channel_idx 1 is out of range for single-channel params.
        let result = normalize(&buffer, &params, 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_tile_config_default() {
        let config = TileConfig::default();
        assert_eq!(config.tile_width, 256);
        assert_eq!(config.tile_height, 256);
        assert_eq!(config.overlap, 32);
    }

    #[test]
    fn test_tiling() {
        let buffer = RasterBuffer::zeros(512, 512, RasterDataType::Float32);
        let config = TileConfig::default();

        let tiles = tile_raster(&buffer, &config);
        assert!(tiles.is_ok());
        let tiles = tiles.ok().unwrap_or_default();
        assert!(!tiles.is_empty());
    }

    #[test]
    fn test_resize_nearest() {
        let buffer = RasterBuffer::zeros(100, 100, RasterDataType::Float32);
        let resized = resize_nearest(&buffer, 50, 50);
        assert!(resized.is_ok());
        let resized = resized
            .ok()
            .unwrap_or_else(|| RasterBuffer::zeros(1, 1, RasterDataType::Float32));
        assert_eq!(resized.width(), 50);
        assert_eq!(resized.height(), 50);
    }
}
