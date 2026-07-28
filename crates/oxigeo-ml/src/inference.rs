//! Inference engine for ML workflows
//!
//! This module coordinates model loading, preprocessing, prediction, and postprocessing.

use std::time::SystemTime;

use oxigeo_core::buffer::RasterBuffer;
use oxigeo_core::types::RasterDataType;
use tracing::{debug, info};

use crate::error::Result;
use crate::inference_cache::{CacheEntry, InferenceCache};
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
    /// Optional GPU device-selection preference.
    ///
    /// This records *which* detected GPU backend/device the caller would like to
    /// use (see [`crate::gpu`]). Actual GPU execution still depends on the ONNX
    /// backend being compiled with the `gpu` feature; when it is not, inference
    /// runs on CPU regardless of this setting. Use
    /// [`InferenceEngine::selected_gpu_device`] to resolve the concrete device
    /// this config points at.
    pub gpu_config: Option<crate::gpu::GpuConfig>,
}

impl Default for InferenceConfig {
    fn default() -> Self {
        Self {
            normalization: Some(NormalizationParams::imagenet()),
            tiling: None,
            confidence_threshold: 0.5,
            gpu_config: None,
        }
    }
}

/// Inference engine
pub struct InferenceEngine<M: Model> {
    model: M,
    config: InferenceConfig,
    /// Optional content-addressed result cache. When present, single-tile
    /// predictions are looked up by `SHA-256(model_hash || normalized input)`
    /// before running the model, and inserted afterwards.
    cache: Option<InferenceCache>,
    /// Model identity hash mixed into every cache key so that swapping the model
    /// invalidates cached results.
    model_hash: Vec<u8>,
}

impl<M: Model> InferenceEngine<M> {
    /// Creates a new inference engine
    #[must_use]
    pub fn new(model: M, config: InferenceConfig) -> Self {
        Self {
            model,
            config,
            cache: None,
            model_hash: Vec::new(),
        }
    }

    /// Enables content-addressed result caching for single-tile predictions.
    ///
    /// `capacity` is the maximum number of cached results (LRU eviction);
    /// `model_hash` uniquely identifies the loaded model so cache entries are
    /// invalidated when the model changes (e.g. a file hash or version bytes).
    #[must_use]
    pub fn with_cache(mut self, capacity: usize, model_hash: Vec<u8>) -> Self {
        self.cache = Some(InferenceCache::new(capacity));
        self.model_hash = model_hash;
        self
    }

    /// Returns cache statistics if caching is enabled.
    #[must_use]
    pub fn cache_stats(&self) -> Option<crate::inference_cache::CacheStats> {
        self.cache.as_ref().map(|c| c.stats().clone())
    }

    /// Resolves the concrete GPU device the [`InferenceConfig::gpu_config`]
    /// points at, if GPU selection was requested and a matching device is
    /// present.
    ///
    /// This is device *discovery/selection* only: it does not change how
    /// [`predict`](Self::predict) executes (see [`crate::gpu`] for why). Returns
    /// `Ok(None)` when no GPU preference was set, and an error when a preference
    /// was set but no matching device is available.
    ///
    /// # Errors
    /// Returns an error if a GPU was requested but device enumeration fails or no
    /// matching device exists.
    pub fn selected_gpu_device(&self) -> Result<Option<crate::gpu::GpuDevice>> {
        match self.config.gpu_config {
            Some(ref gpu_config) => Ok(Some(crate::gpu::select_device(gpu_config)?)),
            None => Ok(None),
        }
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
    ///
    /// `input` is a single-band [`RasterBuffer`], so normalization applies the
    /// first channel's statistics (`channel_idx = 0`). Callers with multi-band
    /// imagery should split it into per-band buffers and normalize each band
    /// with its matching channel index before invoking the engine.
    fn predict_single(&mut self, input: &RasterBuffer) -> Result<RasterBuffer> {
        // Normalize if configured
        let normalized = if let Some(ref params) = self.config.normalization {
            debug!("Applying normalization");
            normalize(input, params, 0)?
        } else {
            input.clone()
        };

        // Without a cache, run the model directly.
        if self.cache.is_none() {
            return self.model.predict(&normalized);
        }

        // Content-addressed lookup on the normalized input.
        let input_floats = flatten_buffer(&normalized)?;
        let key = InferenceCache::compute_key(&self.model_hash, &input_floats);

        // Cache probe (scoped so the mutable borrow is released before predict).
        let hit = if let Some(cache) = self.cache.as_mut() {
            cache
                .get(&key)
                .map(|entry| (entry.outputs.clone(), entry.output_shapes.clone()))
        } else {
            None
        };
        if let Some((outputs, shapes)) = hit {
            debug!("Inference cache hit");
            return reconstruct_buffer(&outputs, &shapes);
        }

        // Miss: run the model and cache the result.
        let result = self.model.predict(&normalized)?;
        let (flat, shape) = encode_buffer(&result)?;
        if let Some(cache) = self.cache.as_mut() {
            let entry = CacheEntry {
                outputs: vec![flat],
                output_shapes: vec![shape],
                created_at: SystemTime::now(),
                hit_count: 0,
                input_size_bytes: input_floats.len() * 4,
            };
            // A rejected insert (e.g. oversized) is not fatal to inference.
            let _ = cache.insert(key, entry);
        }
        Ok(result)
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
                // Tiles are single-band buffers; normalize with the first channel.
                normalize(&tile.buffer, params, 0)?
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

/// Flattens a raster buffer's pixels into a row-major `f32` vector (used for
/// cache-key computation on the model input).
fn flatten_buffer(buffer: &RasterBuffer) -> Result<Vec<f32>> {
    let width = buffer.width();
    let height = buffer.height();
    let mut out = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            let value =
                buffer
                    .get_pixel(x, y)
                    .map_err(|e| crate::error::InferenceError::Failed {
                        reason: format!("Failed to read input pixel for cache key: {e}"),
                    })?;
            out.push(value as f32);
        }
    }
    Ok(out)
}

/// Encodes an output raster buffer as `(row-major f32 pixels, [height, width,
/// dtype_code])` for storage in the cache.
fn encode_buffer(buffer: &RasterBuffer) -> Result<(Vec<f32>, Vec<usize>)> {
    let flat = flatten_buffer(buffer)?;
    let shape = vec![
        buffer.height() as usize,
        buffer.width() as usize,
        buffer.data_type() as u8 as usize,
    ];
    Ok((flat, shape))
}

/// Reconstructs a raster buffer from a cached `(pixels, [height, width,
/// dtype_code])` pair.
fn reconstruct_buffer(outputs: &[Vec<f32>], shapes: &[Vec<usize>]) -> Result<RasterBuffer> {
    let flat = outputs
        .first()
        .ok_or_else(|| crate::error::InferenceError::Failed {
            reason: "cached entry has no output tensor".to_string(),
        })?;
    let shape = shapes
        .first()
        .ok_or_else(|| crate::error::InferenceError::Failed {
            reason: "cached entry has no output shape".to_string(),
        })?;
    if shape.len() != 3 {
        return Err(crate::error::InferenceError::Failed {
            reason: format!("cached raster shape must be [h, w, dtype], got {shape:?}"),
        }
        .into());
    }
    let height = shape[0] as u64;
    let width = shape[1] as u64;
    let dtype = raster_dtype_from_code(shape[2] as u8);

    let mut buffer = RasterBuffer::zeros(width, height, dtype);
    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) as usize;
            if let Some(&value) = flat.get(idx) {
                buffer.set_pixel(x, y, value as f64).map_err(|e| {
                    crate::error::InferenceError::Failed {
                        reason: format!("Failed to write cached pixel: {e}"),
                    }
                })?;
            }
        }
    }
    Ok(buffer)
}

/// Maps a [`RasterDataType`] `#[repr(u8)]` discriminant back to the enum,
/// defaulting to `Float32` for an unrecognized code.
fn raster_dtype_from_code(code: u8) -> RasterDataType {
    match code {
        1 => RasterDataType::UInt8,
        2 => RasterDataType::Int8,
        3 => RasterDataType::UInt16,
        4 => RasterDataType::Int16,
        5 => RasterDataType::UInt32,
        6 => RasterDataType::Int32,
        7 => RasterDataType::UInt64,
        8 => RasterDataType::Int64,
        9 => RasterDataType::Float32,
        10 => RasterDataType::Float64,
        11 => RasterDataType::CFloat32,
        12 => RasterDataType::CFloat64,
        _ => RasterDataType::Float32,
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

/// Computes weight for a pixel in a tile based on distance from edges.
///
/// Delegates to [`crate::tiling::compute_blend_weight`] and applies a
/// minimum floor of 0.1 to prevent zero-weight contributions.
fn compute_tile_weight(x: u64, y: u64, width: u64, height: u64, overlap: u64) -> f32 {
    let w = crate::tiling::compute_blend_weight(
        x as usize,
        y as usize,
        width as usize,
        height as usize,
        overlap as usize,
    );
    // Apply minimum floor to preserve original behavior
    w.max(0.1)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::models::{Model, ModelMetadata};
    use oxigeo_core::types::RasterDataType;

    /// A model that records how many times `predict` was invoked and returns a
    /// deterministic single-pixel-doubling output, so cache hits (which skip
    /// `predict`) are observable.
    struct CountingModel {
        calls: usize,
        metadata: ModelMetadata,
    }

    impl CountingModel {
        fn new() -> Self {
            Self {
                calls: 0,
                metadata: ModelMetadata {
                    name: "counting".to_string(),
                    version: "1".to_string(),
                    description: String::new(),
                    input_names: vec!["in".to_string()],
                    output_names: vec!["out".to_string()],
                    input_shape: (1, 4, 4),
                    output_shape: (1, 4, 4),
                    class_labels: None,
                },
            }
        }
    }

    impl Model for CountingModel {
        fn metadata(&self) -> &ModelMetadata {
            &self.metadata
        }
        fn predict(&mut self, input: &RasterBuffer) -> Result<RasterBuffer> {
            self.calls += 1;
            let mut out =
                RasterBuffer::zeros(input.width(), input.height(), RasterDataType::Float32);
            for y in 0..input.height() {
                for x in 0..input.width() {
                    let v = input.get_pixel(x, y).unwrap_or(0.0);
                    out.set_pixel(x, y, v * 2.0).unwrap();
                }
            }
            Ok(out)
        }
        fn predict_batch(&mut self, inputs: &[RasterBuffer]) -> Result<Vec<RasterBuffer>> {
            inputs.iter().map(|i| self.predict(i)).collect()
        }
        fn input_shape(&self) -> (usize, usize, usize) {
            (1, 4, 4)
        }
        fn output_shape(&self) -> (usize, usize, usize) {
            (1, 4, 4)
        }
    }

    fn ramp_buffer() -> RasterBuffer {
        let mut b = RasterBuffer::zeros(4, 4, RasterDataType::Float32);
        for y in 0..4u64 {
            for x in 0..4u64 {
                b.set_pixel(x, y, (y * 4 + x) as f64).unwrap();
            }
        }
        b
    }

    #[test]
    fn test_inference_cache_hit_skips_model() {
        let config = InferenceConfig {
            normalization: None,
            tiling: None,
            confidence_threshold: 0.5,
            gpu_config: None,
        };
        let mut engine =
            InferenceEngine::new(CountingModel::new(), config).with_cache(16, b"model-v1".to_vec());

        let input = ramp_buffer();

        // First call: miss -> model runs.
        let out1 = engine.predict(&input).expect("predict 1");
        assert_eq!(engine.model().calls, 1);
        // (2,3) pixel = value 4*... let's check a known doubling.
        assert!((out1.get_pixel(1, 1).unwrap() - 10.0).abs() < 1e-6); // (y=1,x=1)=5 -> 10

        // Second identical call: hit -> model must NOT run again.
        let out2 = engine.predict(&input).expect("predict 2");
        assert_eq!(engine.model().calls, 1, "cache hit must skip the model");
        assert!((out2.get_pixel(1, 1).unwrap() - 10.0).abs() < 1e-6);

        let stats = engine.cache_stats().expect("stats");
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
    }

    #[test]
    fn test_inference_cache_distinct_inputs_miss() {
        let config = InferenceConfig {
            normalization: None,
            tiling: None,
            confidence_threshold: 0.5,
            gpu_config: None,
        };
        let mut engine =
            InferenceEngine::new(CountingModel::new(), config).with_cache(16, b"m".to_vec());

        let a = ramp_buffer();
        let mut b = ramp_buffer();
        b.set_pixel(0, 0, 99.0).unwrap();

        engine.predict(&a).expect("a");
        engine.predict(&b).expect("b");
        // Two different inputs => two model runs.
        assert_eq!(engine.model().calls, 2);
    }

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
