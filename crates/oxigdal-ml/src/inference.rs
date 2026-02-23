//! Inference engine for ML workflows
//!
//! This module coordinates model loading, preprocessing, prediction, and postprocessing.

use oxigdal_core::buffer::RasterBuffer;
use tracing::{debug, info};

use crate::error::Result;
use crate::models::{Model, OnnxModel};
use crate::preprocessing::{NormalizationParams, Tile, TileConfig, normalize, tile_raster};

/// Inference configuration
#[derive(Debug, Clone)]
pub struct InferenceConfig {
    /// Normalization parameters
    pub normalization: Option<NormalizationParams>,
    /// Tile configuration (for large images)
    pub tiling: Option<TileConfig>,
    /// Confidence threshold for filtering results
    pub confidence_threshold: f32,
}

impl Default for InferenceConfig {
    fn default() -> Self {
        Self {
            normalization: Some(NormalizationParams::imagenet()),
            tiling: None,
            confidence_threshold: 0.5,
        }
    }
}

/// Inference engine
pub struct InferenceEngine<M: Model> {
    model: M,
    config: InferenceConfig,
}

impl<M: Model> InferenceEngine<M> {
    /// Creates a new inference engine
    #[must_use]
    pub fn new(model: M, config: InferenceConfig) -> Self {
        Self { model, config }
    }

    /// Runs inference on a raster buffer
    ///
    /// # Errors
    /// Returns an error if inference fails
    pub fn predict(&mut self, input: &RasterBuffer) -> Result<RasterBuffer> {
        info!(
            "Running inference on {}x{} raster",
            input.width(),
            input.height()
        );

        // Check if tiling is needed
        let (_, input_h, input_w) = self.model.input_shape();
        let needs_tiling = self.config.tiling.is_some()
            || input.width() > input_w as u64
            || input.height() > input_h as u64;

        if needs_tiling {
            self.predict_tiled(input)
        } else {
            self.predict_single(input)
        }
    }

    /// Runs inference on a single (non-tiled) buffer
    fn predict_single(&mut self, input: &RasterBuffer) -> Result<RasterBuffer> {
        // Normalize if configured
        let normalized = if let Some(ref params) = self.config.normalization {
            debug!("Applying normalization");
            normalize(input, params)?
        } else {
            input.clone()
        };

        // Run inference
        self.model.predict(&normalized)
    }

    /// Runs inference on a tiled buffer
    fn predict_tiled(&mut self, input: &RasterBuffer) -> Result<RasterBuffer> {
        debug!("Using tiled inference");

        // Use provided tile config or create default
        let tile_config = self.config.tiling.clone().unwrap_or_default();

        // Create tiles
        let tiles = tile_raster(input, &tile_config)?;
        debug!("Created {} tiles", tiles.len());

        // Process each tile
        let mut tile_results = Vec::with_capacity(tiles.len());
        for tile in &tiles {
            let normalized = if let Some(ref params) = self.config.normalization {
                normalize(&tile.buffer, params)?
            } else {
                tile.buffer.clone()
            };

            let result = self.model.predict(&normalized)?;
            tile_results.push(result);
        }

        // Merge tiles back together
        merge_tiles(&tiles, &tile_results, &tile_config)
    }

    /// Returns the model metadata
    #[must_use]
    pub fn model(&self) -> &M {
        &self.model
    }

    /// Returns the inference configuration
    #[must_use]
    pub fn config(&self) -> &InferenceConfig {
        &self.config
    }
}

impl InferenceEngine<OnnxModel> {
    /// Creates an inference engine from an ONNX model file
    ///
    /// # Errors
    /// Returns an error if the model cannot be loaded
    pub fn from_onnx_file<P: AsRef<std::path::Path>>(
        path: P,
        config: InferenceConfig,
    ) -> Result<Self> {
        let model = OnnxModel::from_file(path)?;
        Ok(Self::new(model, config))
    }
}

/// Merges tiled inference results back into a single raster
fn merge_tiles(
    tiles: &[Tile],
    results: &[RasterBuffer],
    config: &TileConfig,
) -> Result<RasterBuffer> {
    if tiles.is_empty() || results.is_empty() {
        return Err(crate::error::PostprocessingError::MergingFailed {
            reason: "No tiles to merge".to_string(),
        }
        .into());
    }

    if tiles.len() != results.len() {
        return Err(crate::error::PostprocessingError::MergingFailed {
            reason: format!(
                "Tile count mismatch: {} tiles, {} results",
                tiles.len(),
                results.len()
            ),
        }
        .into());
    }

    let first_tile = &tiles[0];
    let width = first_tile.original_width;
    let height = first_tile.original_height;
    let data_type = results[0].data_type();

    debug!(
        "Merging {} tiles into {}x{} raster",
        tiles.len(),
        width,
        height
    );

    let mut output = RasterBuffer::zeros(width, height, data_type);
    let mut weight_map = vec![0.0f32; (width * height) as usize];

    // Merge tiles with weighted averaging in overlap regions
    for (tile, result) in tiles.iter().zip(results.iter()) {
        let x_start = tile.x_offset;
        let y_start = tile.y_offset;
        let tile_w = result.width().min(width - x_start);
        let tile_h = result.height().min(height - y_start);

        for ty in 0..tile_h {
            for tx in 0..tile_w {
                let out_x = x_start + tx;
                let out_y = y_start + ty;

                // Compute weight based on distance from tile center
                let weight = compute_tile_weight(tx, ty, tile_w, tile_h, config.overlap as u64);

                let pixel = result.get_pixel(tx, ty).map_err(|e| {
                    crate::error::PostprocessingError::MergingFailed {
                        reason: format!("Failed to get tile pixel: {}", e),
                    }
                })?;

                let idx = (out_y * width + out_x) as usize;
                let current_weight = weight_map[idx];
                let current_value = output.get_pixel(out_x, out_y).map_err(|e| {
                    crate::error::PostprocessingError::MergingFailed {
                        reason: format!("Failed to get output pixel: {}", e),
                    }
                })?;

                // Weighted average
                let new_value = if current_weight == 0.0 {
                    pixel
                } else {
                    (current_value * current_weight as f64 + pixel * weight as f64)
                        / (current_weight + weight) as f64
                };

                output.set_pixel(out_x, out_y, new_value).map_err(|e| {
                    crate::error::PostprocessingError::MergingFailed {
                        reason: format!("Failed to set output pixel: {}", e),
                    }
                })?;

                weight_map[idx] = current_weight + weight;
            }
        }
    }

    Ok(output)
}

/// Computes weight for a pixel in a tile based on distance from edges
fn compute_tile_weight(x: u64, y: u64, width: u64, height: u64, overlap: u64) -> f32 {
    if overlap == 0 {
        return 1.0;
    }

    // Distance from edges
    let dist_left = x;
    let dist_right = width.saturating_sub(x + 1);
    let dist_top = y;
    let dist_bottom = height.saturating_sub(y + 1);

    let min_dist = dist_left.min(dist_right).min(dist_top).min(dist_bottom);

    // Weight increases linearly from edge to center
    if min_dist >= overlap {
        1.0
    } else {
        (min_dist as f32 / overlap as f32).max(0.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigdal_core::types::RasterDataType;

    #[test]
    fn test_inference_config_default() {
        let config = InferenceConfig::default();
        assert!(config.normalization.is_some());
        assert!(config.tiling.is_none());
        assert!((config.confidence_threshold - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn test_compute_tile_weight() {
        // Center of tile should have weight 1.0
        let weight = compute_tile_weight(128, 128, 256, 256, 32);
        assert!((weight - 1.0).abs() < f32::EPSILON);

        // Edge should have lower weight
        let weight = compute_tile_weight(0, 0, 256, 256, 32);
        assert!(weight < 1.0);
        assert!(weight >= 0.1);
    }

    #[test]
    fn test_merge_tiles_validation() {
        let tiles = vec![];
        let results = vec![];
        let config = TileConfig::default();

        let result = merge_tiles(&tiles, &results, &config);
        assert!(result.is_err());
    }
}
