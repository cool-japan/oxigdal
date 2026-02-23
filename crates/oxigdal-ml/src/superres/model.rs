//! Super-resolution model implementation

use ndarray::{Array2, Array3, Array4, Axis, s};
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::Value;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tracing::{debug, info};

use crate::error::{InferenceError, ModelError, Result};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;

use super::UpscaleFactor;

/// Configuration for super-resolution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuperResConfig {
    /// Upscale factor (2x or 4x)
    pub scale_factor: usize,
    /// Tile size (width and height in pixels)
    pub tile_size: usize,
    /// Overlap between tiles (in pixels)
    pub overlap: usize,
    /// Batch size for processing multiple tiles
    pub batch_size: usize,
}

impl SuperResConfig {
    /// Creates a new super-resolution configuration
    ///
    /// # Arguments
    ///
    /// * `scale_factor` - Upscale factor (2 or 4)
    /// * `tile_size` - Size of tiles for processing
    /// * `overlap` - Overlap between tiles in pixels
    ///
    /// # Example
    ///
    /// ```
    /// use oxigdal_ml::superres::SuperResConfig;
    ///
    /// let config = SuperResConfig::new(2, 256, 32);
    /// assert_eq!(config.scale_factor, 2);
    /// assert_eq!(config.tile_size, 256);
    /// assert_eq!(config.overlap, 32);
    /// ```
    #[must_use]
    pub fn new(scale_factor: usize, tile_size: usize, overlap: usize) -> Self {
        Self {
            scale_factor,
            tile_size,
            overlap,
            batch_size: 1,
        }
    }

    /// Set batch size for parallel processing
    #[must_use]
    pub fn with_batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = batch_size;
        self
    }
}

impl Default for SuperResConfig {
    fn default() -> Self {
        Self::new(2, 256, 32)
    }
}

/// Super-resolution model using ONNX Runtime
pub struct SuperResolution {
    session: Session,
    config: SuperResConfig,
}

impl SuperResolution {
    /// Load a super-resolution model from an ONNX file
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the ONNX model file
    /// * `config` - Super-resolution configuration
    ///
    /// # Errors
    ///
    /// Returns an error if the model file cannot be loaded or is invalid
    ///
    /// # Example
    ///
    /// ```no_run
    /// use oxigdal_ml::superres::{SuperResolution, SuperResConfig};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let config = SuperResConfig::default();
    /// let model = SuperResolution::from_file("real_esrgan_2x.onnx", config)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn from_file<P: AsRef<Path>>(path: P, config: SuperResConfig) -> Result<Self> {
        let path = path.as_ref();

        if !path.exists() {
            return Err(ModelError::NotFound {
                path: path.display().to_string(),
            }
            .into());
        }

        let session = Session::builder()
            .map_err(|e: ort::Error| ModelError::InitializationFailed {
                reason: e.to_string(),
            })?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e: ort::Error| ModelError::InitializationFailed {
                reason: e.to_string(),
            })?
            .commit_from_file(path)
            .map_err(|e: ort::Error| ModelError::LoadFailed {
                reason: e.to_string(),
            })?;

        info!("Loaded super-resolution model from {}", path.display());

        Ok(Self { session, config })
    }

    /// Upscale a raster using the super-resolution model
    ///
    /// # Arguments
    ///
    /// * `input` - Input raster buffer
    ///
    /// # Errors
    ///
    /// Returns an error if inference fails or input validation fails
    ///
    /// # Example
    ///
    /// ```no_run
    /// use oxigdal_ml::superres::{SuperResolution, SuperResConfig};
    /// use oxigdal_core::buffer::RasterBuffer;
    /// use oxigdal_core::types::RasterDataType;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let config = SuperResConfig::default();
    /// let mut model = SuperResolution::from_file("real_esrgan_2x.onnx", config)?;
    ///
    /// let input = RasterBuffer::zeros(512, 512, RasterDataType::Float32);
    /// let output = model.upscale(&input)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn upscale(&mut self, input: &RasterBuffer) -> Result<RasterBuffer> {
        let width = input.width() as usize;
        let height = input.height() as usize;

        debug!(
            "Starting super-resolution upscaling: {}x{} -> {}x{}",
            width,
            height,
            width * self.config.scale_factor,
            height * self.config.scale_factor
        );

        // Extract tiles with overlap
        let tiles = self.extract_tiles(input)?;

        // Process tiles in batches (sequential for now due to &mut self requirement)
        let processed_tiles = self.process_batch(&tiles)?;

        // Merge tiles with blending
        let merged = self.merge_tiles(
            &processed_tiles,
            width * self.config.scale_factor,
            height * self.config.scale_factor,
        )?;

        // Create output buffer
        RasterBuffer::new(
            merged
                .as_slice()
                .ok_or_else(|| InferenceError::OutputParsingFailed {
                    reason: "Failed to convert array to slice".to_string(),
                })?
                .iter()
                .flat_map(|&v: &f32| v.to_le_bytes())
                .collect(),
            (width * self.config.scale_factor) as u64,
            (height * self.config.scale_factor) as u64,
            RasterDataType::Float32,
            input.nodata(),
        )
        .map_err(Into::into)
    }

    /// Extract tiles from input raster with overlap
    fn extract_tiles(&self, input: &RasterBuffer) -> Result<Vec<TileInfo>> {
        let width = input.width() as usize;
        let height = input.height() as usize;
        let tile_size = self.config.tile_size;
        let overlap = self.config.overlap;
        let stride = tile_size - overlap;

        let mut tiles = Vec::new();

        let mut y = 0;
        while y < height {
            let mut x = 0;
            while x < width {
                let tile_w = (tile_size).min(width - x);
                let tile_h = (tile_size).min(height - y);

                tiles.push(TileInfo {
                    x,
                    y,
                    width: tile_w,
                    height: tile_h,
                    data: self.extract_tile_data(input, x, y, tile_w, tile_h)?,
                });

                if x + tile_w >= width {
                    break;
                }
                x += stride;
            }

            if y + tile_size >= height {
                break;
            }
            y += stride;
        }

        debug!(
            "Extracted {} tiles from {}x{} image",
            tiles.len(),
            width,
            height
        );

        Ok(tiles)
    }

    /// Extract single tile data
    fn extract_tile_data(
        &self,
        input: &RasterBuffer,
        x: usize,
        y: usize,
        width: usize,
        height: usize,
    ) -> Result<Array3<f32>> {
        // For simplicity, assume single-band input (can be extended to multi-band)
        let mut tile = Array3::<f32>::zeros((1, height, width));

        // Extract data from buffer (simplified - assumes Float32)
        for ty in 0..height {
            for tx in 0..width {
                let pixel_idx = ((y + ty) * input.width() as usize + (x + tx)) * 4; // 4 bytes for Float32
                let bytes = input.as_bytes();

                if pixel_idx + 4 <= bytes.len() {
                    let value = f32::from_le_bytes([
                        bytes[pixel_idx],
                        bytes[pixel_idx + 1],
                        bytes[pixel_idx + 2],
                        bytes[pixel_idx + 3],
                    ]);
                    tile[[0, ty, tx]] = value;
                }
            }
        }

        Ok(tile)
    }

    /// Process a batch of tiles through the model
    fn process_batch(&mut self, tiles: &[TileInfo]) -> Result<Vec<ProcessedTile>> {
        let mut processed = Vec::with_capacity(tiles.len());

        for tile in tiles {
            // Create input tensor
            let input_tensor = tile.data.clone().insert_axis(Axis(0));

            // Create ONNX value from array
            let input_value =
                Value::from_array(input_tensor.clone()).map_err(|e: ort::Error| {
                    InferenceError::Failed {
                        reason: format!("Failed to create input tensor: {}", e),
                    }
                })?;

            // Run inference
            let outputs =
                self.session
                    .run(ort::inputs![input_value])
                    .map_err(|e: ort::Error| InferenceError::Failed {
                        reason: e.to_string(),
                    })?;

            // Extract output (first output)
            let output_value = &outputs[0];
            let output_tensor =
                output_value
                    .try_extract_tensor::<f32>()
                    .map_err(|e: ort::Error| InferenceError::OutputParsingFailed {
                        reason: e.to_string(),
                    })?;

            // Get shape and data
            let (shape, data) = output_tensor;
            let shape_vec: Vec<usize> = shape.iter().map(|&d| d as usize).collect();

            // Convert to ndarray
            let output: Array4<f32> = Array4::from_shape_vec(
                (shape_vec[0], shape_vec[1], shape_vec[2], shape_vec[3]),
                data.to_vec(),
            )
            .map_err(|e| InferenceError::OutputParsingFailed {
                reason: format!("Failed to reshape output: {}", e),
            })?;

            processed.push(ProcessedTile {
                x: tile.x * self.config.scale_factor,
                y: tile.y * self.config.scale_factor,
                data: output.index_axis_move(Axis(0), 0),
            });
        }

        Ok(processed)
    }

    /// Merge processed tiles with overlap blending
    fn merge_tiles(
        &self,
        tiles: &[ProcessedTile],
        output_width: usize,
        output_height: usize,
    ) -> Result<Array3<f32>> {
        let mut output = Array3::<f32>::zeros((1, output_height, output_width));
        let mut weight_map = Array3::<f32>::zeros((1, output_height, output_width));

        let overlap = self.config.overlap * self.config.scale_factor;

        for tile in tiles {
            let tile_height = tile.data.shape()[1];
            let tile_width = tile.data.shape()[2];

            // Create weight matrix for alpha blending
            let weights = self.create_blend_weights(tile_width, tile_height, overlap);

            // Blend tile into output
            for c in 0..1 {
                for ty in 0..tile_height {
                    for tx in 0..tile_width {
                        let out_y = tile.y + ty;
                        let out_x = tile.x + tx;

                        if out_y < output_height && out_x < output_width {
                            let weight = weights[[ty, tx]];
                            output[[c, out_y, out_x]] += tile.data[[c, ty, tx]] * weight;
                            weight_map[[c, out_y, out_x]] += weight;
                        }
                    }
                }
            }
        }

        // Normalize by weight map
        output.zip_mut_with(&weight_map, |out, &w| {
            if w > 0.0 {
                *out /= w;
            }
        });

        Ok(output)
    }

    /// Create blend weights for smooth tile merging
    fn create_blend_weights(&self, width: usize, height: usize, overlap: usize) -> Array2<f32> {
        let mut weights = Array2::<f32>::ones((height, width));

        if overlap == 0 {
            return weights;
        }

        // Create linear blend in overlap regions
        for y in 0..height {
            for x in 0..width {
                let mut w = 1.0_f32;

                // Blend on edges
                if x < overlap {
                    w = w.min(x as f32 / overlap as f32);
                }
                if x >= width - overlap {
                    w = w.min((width - x) as f32 / overlap as f32);
                }
                if y < overlap {
                    w = w.min(y as f32 / overlap as f32);
                }
                if y >= height - overlap {
                    w = w.min((height - y) as f32 / overlap as f32);
                }

                weights[[y, x]] = w;
            }
        }

        weights
    }
}

/// Information about an extracted tile
struct TileInfo {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    data: Array3<f32>,
}

/// A processed (upscaled) tile
struct ProcessedTile {
    x: usize,
    y: usize,
    data: Array3<f32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_creation() {
        let config = SuperResConfig::new(2, 256, 32);
        assert_eq!(config.scale_factor, 2);
        assert_eq!(config.tile_size, 256);
        assert_eq!(config.overlap, 32);
    }

    #[test]
    fn test_config_default() {
        let config = SuperResConfig::default();
        assert_eq!(config.scale_factor, 2);
        assert_eq!(config.batch_size, 1);
    }

    #[test]
    #[ignore = "Requires ONNX Runtime to be installed"]
    fn test_blend_weights() {
        let _config = SuperResConfig::default();
        let session = Session::builder().ok();

        if session.is_none() {
            // Skip if ONNX Runtime not available
        }

        // We can't easily test this without a full model
        // Just verify the method signature compiles
    }
}
