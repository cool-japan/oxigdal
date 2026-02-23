//! Cloud detection using ONNX models

use std::path::Path;

use ndarray::{Array3, Array4, s};
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::TensorRef;
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;
use rayon::prelude::*;
use tracing::{debug, instrument};

use super::CloudConfig;
use crate::error::{InferenceError, MlError, ModelError, Result};

/// Cloud mask with confidence scores
#[derive(Debug, Clone)]
pub struct CloudMask {
    /// Binary mask (0 = clear, 1 = cloud)
    pub mask: RasterBuffer,
    /// Confidence scores (0.0 to 1.0)
    pub confidence: RasterBuffer,
    /// Mask width
    pub width: u64,
    /// Mask height
    pub height: u64,
}

impl CloudMask {
    /// Create a new cloud mask
    ///
    /// # Errors
    /// Returns error if buffer creation fails
    pub fn new(mask: RasterBuffer, confidence: RasterBuffer) -> Result<Self> {
        let width = mask.width();
        let height = mask.height();

        if confidence.width() != width || confidence.height() != height {
            return Err(MlError::Inference(InferenceError::InvalidInputShape {
                expected: vec![height as usize, width as usize],
                actual: vec![confidence.height() as usize, confidence.width() as usize],
            }));
        }

        Ok(Self {
            mask,
            confidence,
            width,
            height,
        })
    }

    /// Apply morphological dilation to expand cloud regions
    ///
    /// # Errors
    /// Returns error if operation fails
    pub fn dilate(&mut self, radius: usize) -> Result<()> {
        let dilated = morphological_dilate(&self.mask, radius)?;
        self.mask = dilated;
        Ok(())
    }

    /// Apply morphological erosion to shrink cloud regions
    ///
    /// # Errors
    /// Returns error if operation fails
    pub fn erode(&mut self, radius: usize) -> Result<()> {
        let eroded = morphological_erode(&self.mask, radius)?;
        self.mask = eroded;
        Ok(())
    }
}

/// Cloud detector using ONNX models
pub struct CloudDetector {
    /// ONNX Runtime session (wrapped in Mutex for interior mutability)
    session: Option<std::sync::Mutex<Session>>,
    /// Model path
    model_path: std::path::PathBuf,
    /// Configuration
    config: CloudConfig,
}

impl CloudDetector {
    /// Create detector from ONNX model file
    ///
    /// # Errors
    /// Returns error if model loading fails
    #[instrument(skip(model_path, config))]
    pub fn from_file<P: AsRef<Path>>(model_path: P, config: CloudConfig) -> Result<Self> {
        let path = model_path.as_ref();
        debug!("Loading cloud detection model from {:?}", path);

        if !path.exists() {
            return Err(ModelError::NotFound {
                path: path.display().to_string(),
            }
            .into());
        }

        // Load ONNX model using ONNX Runtime 2.0 API
        let session = Session::builder()
            .map_err(|e| ModelError::LoadFailed {
                reason: format!("Failed to create session builder: {}", e),
            })?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| ModelError::LoadFailed {
                reason: format!("Failed to set optimization level: {}", e),
            })?
            .commit_from_file(path)
            .map_err(|e| ModelError::LoadFailed {
                reason: format!("Failed to load cloud detection model: {}", e),
            })?;

        debug!("Cloud detection model loaded successfully");

        Ok(Self {
            session: Some(std::sync::Mutex::new(session)),
            model_path: path.to_path_buf(),
            config,
        })
    }

    /// Detect clouds in multi-spectral imagery
    ///
    /// # Arguments
    /// * `image` - Multi-band raster buffer (bands in channel dimension)
    ///
    /// # Returns
    /// Cloud mask with confidence scores
    ///
    /// # Errors
    /// Returns error if inference fails or invalid input
    #[instrument(skip(self, image))]
    pub fn detect(&self, image: &RasterBuffer) -> Result<CloudMask> {
        debug!(
            "Detecting clouds in {}x{} image",
            image.width(),
            image.height()
        );

        // Extract and normalize bands
        let normalized = self.preprocess(image)?;

        // Use ONNX model if available, otherwise fall back to rule-based detection
        let (mask, confidence) = if let Some(ref session_mutex) = self.session {
            let mut session = session_mutex.lock().map_err(|e| {
                crate::error::MlError::InvalidConfig(format!("Failed to lock session: {}", e))
            })?;
            self.detect_onnx(&mut session, &normalized, image.width(), image.height())?
        } else {
            debug!("No ONNX session available, using rule-based detection");
            self.detect_rule_based(&normalized, image.width(), image.height())?
        };

        let mut cloud_mask = CloudMask::new(mask, confidence)?;

        // Apply morphological operations
        if self.config.erosion_radius > 0 {
            cloud_mask.erode(self.config.erosion_radius)?;
        }
        if self.config.dilation_radius > 0 {
            cloud_mask.dilate(self.config.dilation_radius)?;
        }

        Ok(cloud_mask)
    }

    /// Preprocess image: extract bands and normalize
    fn preprocess(&self, image: &RasterBuffer) -> Result<Array4<f32>> {
        let height = image.height() as usize;
        let width = image.width() as usize;
        let num_bands = self.config.band_indices.len();

        // Convert to f32 array
        let image_data = buffer_to_array3(image)?;

        // Extract selected bands
        let mut normalized = Array4::<f32>::zeros((1, num_bands, height, width));

        for (i, &band_idx) in self.config.band_indices.iter().enumerate() {
            if band_idx >= image_data.shape()[0] {
                return Err(MlError::Inference(InferenceError::InvalidInputShape {
                    expected: vec![num_bands],
                    actual: vec![image_data.shape()[0]],
                }));
            }

            let band = image_data.slice(s![band_idx, .., ..]);
            let mean = self
                .config
                .normalization_mean
                .get(i)
                .copied()
                .unwrap_or(0.0);
            let std = self.config.normalization_std.get(i).copied().unwrap_or(1.0);

            // Normalize: (x - mean) / std
            let mut norm_slice = normalized.slice_mut(s![0, i, .., ..]);
            for y in 0..height {
                for x in 0..width {
                    let val = band[[y, x]];
                    norm_slice[[y, x]] = (val - mean) / std;
                }
            }
        }

        Ok(normalized)
    }

    /// ONNX-based cloud detection using trained model
    fn detect_onnx(
        &self,
        session: &mut Session,
        normalized: &Array4<f32>,
        width: u64,
        height: u64,
    ) -> Result<(RasterBuffer, RasterBuffer)> {
        debug!("Running ONNX inference for cloud detection");

        // Get input and output names from session metadata
        let input_name = session
            .inputs()
            .first()
            .ok_or_else(|| InferenceError::Failed {
                reason: "No input tensor found in model".to_string(),
            })?
            .name()
            .to_string();

        let output_name = session
            .outputs()
            .first()
            .ok_or_else(|| InferenceError::Failed {
                reason: "No output tensor found in model".to_string(),
            })?
            .name()
            .to_string();

        // Create TensorRef from ndarray view
        let input_tensor =
            TensorRef::from_array_view(normalized.view()).map_err(|e| InferenceError::Failed {
                reason: format!("Failed to create input tensor: {}", e),
            })?;

        // Run inference using ort 2.0 API
        let outputs = session
            .run(ort::inputs![input_name.as_str() => input_tensor])
            .map_err(|e| InferenceError::Failed {
                reason: format!("Cloud detection inference failed: {}", e),
            })?;

        // Extract output tensor
        let output_tensor = outputs.get(output_name.as_str()).ok_or_else(|| {
            InferenceError::OutputParsingFailed {
                reason: format!("Output tensor '{}' not found", output_name),
            }
        })?;

        // Extract array from tensor (ort 2.0 API)
        // try_extract_array directly returns ArrayViewD
        let output_array = output_tensor.try_extract_array::<f32>().map_err(|e| {
            InferenceError::OutputParsingFailed {
                reason: format!("Failed to extract output tensor: {}", e),
            }
        })?;

        // Convert to owned array to avoid borrow checker issues
        let output_owned = output_array.to_owned();

        // Drop outputs to release the borrow of session
        drop(outputs);

        let h = height as usize;
        let w = width as usize;

        // Apply threshold for binary mask
        let threshold = self.config.confidence_threshold;
        let mut mask_data = Vec::with_capacity(h * w);
        let mut conf_data = Vec::with_capacity(h * w);

        // Extract confidence scores and create binary mask
        for y in 0..h {
            for x in 0..w {
                // Get probability from output (assuming single channel output [B, 1, H, W])
                let prob = if output_owned.shape().len() == 4 {
                    output_owned[[0, 0, y, x]]
                } else if output_owned.shape().len() == 3 {
                    output_owned[[0, y, x]]
                } else {
                    output_owned[[y, x]]
                };

                let prob = prob.clamp(0.0, 1.0);
                conf_data.push(prob);
                mask_data.push(if prob >= threshold { 1.0_f32 } else { 0.0_f32 });
            }
        }

        // Convert to bytes
        let mask_bytes = float_vec_to_bytes(&mask_data);
        let conf_bytes = float_vec_to_bytes(&conf_data);

        let mask = RasterBuffer::new(
            mask_bytes,
            width,
            height,
            RasterDataType::Float32,
            oxigdal_core::types::NoDataValue::None,
        )?;

        let confidence = RasterBuffer::new(
            conf_bytes,
            width,
            height,
            RasterDataType::Float32,
            oxigdal_core::types::NoDataValue::None,
        )?;

        Ok((mask, confidence))
    }

    /// Rule-based cloud detection (fallback when ONNX model is not available)
    /// Uses brightness and spectral ratios for cloud detection
    fn detect_rule_based(
        &self,
        normalized: &Array4<f32>,
        width: u64,
        height: u64,
    ) -> Result<(RasterBuffer, RasterBuffer)> {
        let h = height as usize;
        let w = width as usize;
        let num_bands = normalized.shape()[1];

        // Apply threshold for binary mask
        let threshold = self.config.confidence_threshold;
        let mut mask_data = Vec::with_capacity(h * w);
        let mut conf_data = Vec::with_capacity(h * w);

        for y in 0..h {
            for x in 0..w {
                // Simple rule-based detection using brightness
                // In real implementation, this would use ONNX model
                let mut brightness = 0.0_f32;
                for band in 0..num_bands.min(3) {
                    brightness += normalized[[0, band, y, x]];
                }
                brightness /= num_bands.min(3) as f32;

                // Cloud probability based on brightness (clouds are bright)
                let prob = if brightness > 0.5 {
                    (brightness - 0.5) * 2.0
                } else {
                    0.0
                };
                let prob = prob.clamp(0.0, 1.0);

                conf_data.push(prob);
                mask_data.push(if prob >= threshold { 1.0_f32 } else { 0.0_f32 });
            }
        }

        // Convert to bytes
        let mask_bytes = float_vec_to_bytes(&mask_data);
        let conf_bytes = float_vec_to_bytes(&conf_data);

        let mask = RasterBuffer::new(
            mask_bytes,
            width,
            height,
            RasterDataType::Float32,
            oxigdal_core::types::NoDataValue::None,
        )?;

        let confidence = RasterBuffer::new(
            conf_bytes,
            width,
            height,
            RasterDataType::Float32,
            oxigdal_core::types::NoDataValue::None,
        )?;

        Ok((mask, confidence))
    }
}

/// Convert RasterBuffer to ndarray Array3 (C, H, W)
fn buffer_to_array3(buffer: &RasterBuffer) -> Result<Array3<f32>> {
    let height = buffer.height() as usize;
    let width = buffer.width() as usize;

    // Assume single band for now, or multiple bands stacked
    // For multi-band, we need to know the band count
    let data = buffer.as_slice::<f32>().map_err(MlError::OxiGdal)?;

    let total_pixels = height * width;
    let num_bands = data.len() / total_pixels;

    let array = Array3::from_shape_vec((num_bands, height, width), data.to_vec()).map_err(|e| {
        MlError::Inference(InferenceError::OutputParsingFailed {
            reason: format!("Failed to create array: {}", e),
        })
    })?;

    Ok(array)
}

/// Convert f32 vec to bytes
fn float_vec_to_bytes(data: &[f32]) -> Vec<u8> {
    data.iter().flat_map(|&f| f.to_le_bytes()).collect()
}

/// Morphological dilation
fn morphological_dilate(mask: &RasterBuffer, radius: usize) -> Result<RasterBuffer> {
    let height = mask.height() as usize;
    let width = mask.width() as usize;

    let input = mask.as_slice::<f32>().map_err(MlError::OxiGdal)?;

    let output: Vec<f32> = (0..height * width)
        .into_par_iter()
        .map(|idx| {
            let y = idx / width;
            let x = idx % width;

            // Check neighborhood
            let y_start = y.saturating_sub(radius);
            let y_end = (y + radius + 1).min(height);
            let x_start = x.saturating_sub(radius);
            let x_end = (x + radius + 1).min(width);

            let mut max_val = 0.0_f32;
            for ny in y_start..y_end {
                for nx in x_start..x_end {
                    let val = input[ny * width + nx];
                    if val > max_val {
                        max_val = val;
                    }
                }
            }
            max_val
        })
        .collect();

    let bytes = float_vec_to_bytes(&output);
    Ok(RasterBuffer::new(
        bytes,
        width as u64,
        height as u64,
        RasterDataType::Float32,
        oxigdal_core::types::NoDataValue::None,
    )?)
}

/// Morphological erosion
fn morphological_erode(mask: &RasterBuffer, radius: usize) -> Result<RasterBuffer> {
    let height = mask.height() as usize;
    let width = mask.width() as usize;

    let input = mask.as_slice::<f32>().map_err(MlError::OxiGdal)?;

    let output: Vec<f32> = (0..height * width)
        .into_par_iter()
        .map(|idx| {
            let y = idx / width;
            let x = idx % width;

            // Check neighborhood
            let y_start = y.saturating_sub(radius);
            let y_end = (y + radius + 1).min(height);
            let x_start = x.saturating_sub(radius);
            let x_end = (x + radius + 1).min(width);

            let mut min_val = 1.0_f32;
            for ny in y_start..y_end {
                for nx in x_start..x_end {
                    let val = input[ny * width + nx];
                    if val < min_val {
                        min_val = val;
                    }
                }
            }
            min_val
        })
        .collect();

    let bytes = float_vec_to_bytes(&output);
    Ok(RasterBuffer::new(
        bytes,
        width as u64,
        height as u64,
        RasterDataType::Float32,
        oxigdal_core::types::NoDataValue::None,
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cloud_mask_creation() {
        let mask = RasterBuffer::zeros(100, 100, RasterDataType::Float32);
        let confidence = RasterBuffer::zeros(100, 100, RasterDataType::Float32);

        let cloud_mask = CloudMask::new(mask, confidence);
        assert!(cloud_mask.is_ok());
    }

    #[test]
    fn test_morphological_operations() {
        let mut data = vec![0.0_f32; 100 * 100];
        // Set center pixel to 1.0
        data[50 * 100 + 50] = 1.0;

        let bytes = float_vec_to_bytes(&data);
        let mask = RasterBuffer::new(
            bytes,
            100,
            100,
            RasterDataType::Float32,
            oxigdal_core::types::NoDataValue::None,
        )
        .expect("Failed to create buffer");

        // Dilate should expand the region
        let dilated = morphological_dilate(&mask, 3).expect("Dilation failed");
        assert!(dilated.width() == 100);
    }
}
