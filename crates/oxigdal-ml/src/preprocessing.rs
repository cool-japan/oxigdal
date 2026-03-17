//! Data preprocessing for ML workflows
//!
//! This module provides preprocessing operations for geospatial data
//! before ML inference.

use oxigdal_core::buffer::RasterBuffer;
// use oxigdal_core::types::RasterDataType;
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

/// Normalizes a raster buffer using the given parameters
///
/// # Errors
/// Returns an error if normalization fails
pub fn normalize(buffer: &RasterBuffer, params: &NormalizationParams) -> Result<RasterBuffer> {
    if params.mean.is_empty() || params.std.is_empty() {
        return Err(PreprocessingError::InvalidNormalization {
            message: "Mean and std must not be empty".to_string(),
        }
        .into());
    }

    if params.std.contains(&0.0) {
        return Err(PreprocessingError::InvalidNormalization {
            message: "Standard deviation cannot be zero".to_string(),
        }
        .into());
    }

    let mut result = buffer.clone();

    // Normalize each pixel
    for y in 0..buffer.height() {
        for x in 0..buffer.width() {
            let pixel =
                buffer
                    .get_pixel(x, y)
                    .map_err(|e| PreprocessingError::InvalidNormalization {
                        message: format!("Failed to get pixel: {}", e),
                    })?;

            // Use first channel params if only one set is provided
            let channel_idx = 0;
            let mean = params.mean[channel_idx];
            let std = params.std[channel_idx];

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

/// Tiles a raster buffer into smaller tiles
///
/// # Errors
/// Returns an error if tiling fails
pub fn tile_raster(buffer: &RasterBuffer, config: &TileConfig) -> Result<Vec<Tile>> {
    if config.tile_width == 0 || config.tile_height == 0 {
        return Err(PreprocessingError::InvalidTileSize {
            width: config.tile_width,
            height: config.tile_height,
        }
        .into());
    }

    let width = buffer.width();
    let height = buffer.height();

    debug!(
        "Tiling {}x{} raster into {}x{} tiles with {} overlap",
        width, height, config.tile_width, config.tile_height, config.overlap
    );

    let mut tiles = Vec::new();

    let stride_x = config.tile_width.saturating_sub(config.overlap);
    let stride_y = config.tile_height.saturating_sub(config.overlap);

    if stride_x == 0 || stride_y == 0 {
        return Err(PreprocessingError::TilingFailed {
            reason: "Overlap is too large for the tile size".to_string(),
        }
        .into());
    }

    let mut y = 0u64;
    while y < height {
        let mut x = 0u64;
        while x < width {
            let tile_width = (width - x).min(config.tile_width as u64);
            let tile_height = (height - y).min(config.tile_height as u64);

            let tile_buffer = extract_tile(buffer, x, y, tile_width, tile_height, config)?;

            tiles.push(Tile {
                buffer: tile_buffer,
                x_offset: x,
                y_offset: y,
                original_width: width,
                original_height: height,
            });

            x = x.saturating_add(stride_x as u64);
            if x >= width {
                break;
            }
        }

        y = y.saturating_add(stride_y as u64);
        if y >= height {
            break;
        }
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
    use oxigdal_core::types::RasterDataType;

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

        let result = normalize(&buffer, &params);
        assert!(result.is_ok());
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
