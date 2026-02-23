//! ONNX Runtime integration for OxiGDAL
//!
//! This module provides integration with ONNX Runtime for running ML models
//! on geospatial data.

use std::path::Path;

use ndarray::{Array, ArrayD, ArrayView, IxDyn};
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::TensorRef;
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::error::{InferenceError, ModelError, Result};
use crate::models::Model;

/// ONNX model with ONNX Runtime backend
pub struct OnnxModel {
    session: Session,
    metadata: ModelMetadata,
    config: SessionConfig,
}

/// Model metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelMetadata {
    /// Model name
    pub name: String,
    /// Model version
    pub version: String,
    /// Model description
    pub description: String,
    /// Input tensor names
    pub input_names: Vec<String>,
    /// Output tensor names
    pub output_names: Vec<String>,
    /// Input shape (channels, height, width)
    pub input_shape: (usize, usize, usize),
    /// Output shape (channels, height, width)
    pub output_shape: (usize, usize, usize),
    /// Class labels (if classification model)
    pub class_labels: Option<Vec<String>>,
}

/// Session configuration for ONNX Runtime
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Execution provider
    pub execution_provider: ExecutionProvider,
    /// Number of threads for CPU inference
    pub num_threads: usize,
    /// Enable graph optimization
    pub graph_optimization: bool,
    /// Batch size
    pub batch_size: usize,
}

/// Execution provider for ONNX Runtime
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionProvider {
    /// CPU execution
    Cpu,
    /// CUDA GPU execution (requires 'gpu' feature)
    #[cfg(feature = "gpu")]
    Cuda,
    /// TensorRT execution (requires 'gpu' feature)
    #[cfg(feature = "gpu")]
    TensorRt,
    /// DirectML execution (requires 'directml' feature, Windows only)
    #[cfg(feature = "directml")]
    DirectMl,
    /// CoreML execution (requires 'coreml' feature, macOS/iOS only)
    #[cfg(feature = "coreml")]
    CoreMl,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            execution_provider: ExecutionProvider::Cpu,
            num_threads: num_cpus(),
            graph_optimization: true,
            batch_size: 1,
        }
    }
}

impl OnnxModel {
    /// Loads an ONNX model from a file
    ///
    /// # Errors
    /// Returns an error if the model cannot be loaded
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::from_file_with_config(path, SessionConfig::default())
    }

    /// Loads an ONNX model from a file with custom configuration
    ///
    /// # Errors
    /// Returns an error if the model cannot be loaded
    pub fn from_file_with_config<P: AsRef<Path>>(path: P, config: SessionConfig) -> Result<Self> {
        let path = path.as_ref();
        info!("Loading ONNX model from: {}", path.display());

        if !path.exists() {
            return Err(ModelError::NotFound {
                path: path.display().to_string(),
            }
            .into());
        }

        // Create SessionBuilder with configuration
        let mut builder = Session::builder().map_err(|e| ModelError::LoadFailed {
            reason: format!("Failed to create session builder: {}", e),
        })?;

        // Configure number of threads
        builder = builder
            .with_intra_threads(config.num_threads)
            .map_err(|e| ModelError::LoadFailed {
                reason: format!("Failed to set intra threads: {}", e),
            })?;

        // Configure graph optimization
        if config.graph_optimization {
            builder = builder
                .with_optimization_level(GraphOptimizationLevel::Level3)
                .map_err(|e| ModelError::LoadFailed {
                    reason: format!("Failed to set optimization level: {}", e),
                })?;
        }

        // Configure execution provider
        #[cfg(feature = "gpu")]
        {
            use ort::execution_providers::CUDAExecutionProvider;
            if matches!(config.execution_provider, ExecutionProvider::Cuda) {
                builder = builder
                    .with_execution_providers([CUDAExecutionProvider::default().build()])
                    .map_err(|e| ModelError::LoadFailed {
                        reason: format!("Failed to set CUDA execution provider: {}", e),
                    })?;
            }
        }

        #[cfg(feature = "directml")]
        {
            use ort::execution_providers::DirectMLExecutionProvider;
            if matches!(config.execution_provider, ExecutionProvider::DirectMl) {
                builder = builder
                    .with_execution_providers([DirectMLExecutionProvider::default().build()])
                    .map_err(|e| ModelError::LoadFailed {
                        reason: format!("Failed to set DirectML execution provider: {}", e),
                    })?;
            }
        }

        #[cfg(feature = "coreml")]
        {
            use ort::execution_providers::CoreMLExecutionProvider;
            if matches!(config.execution_provider, ExecutionProvider::CoreMl) {
                builder = builder
                    .with_execution_providers([CoreMLExecutionProvider::default().build()])
                    .map_err(|e| ModelError::LoadFailed {
                        reason: format!("Failed to set CoreML execution provider: {}", e),
                    })?;
            }
        }

        // Load the model
        let session = builder
            .commit_from_file(path)
            .map_err(|e| ModelError::LoadFailed {
                reason: format!("Failed to load ONNX model: {}", e),
            })?;

        info!("ONNX model loaded successfully");

        // Extract metadata from the loaded session
        let metadata = Self::extract_metadata(&session)?;

        Ok(Self {
            session,
            metadata,
            config,
        })
    }

    /// Extracts metadata from an ONNX session
    fn extract_metadata(session: &Session) -> Result<ModelMetadata> {
        // Get input metadata using accessor methods
        let inputs = session.inputs();
        let outputs = session.outputs();

        debug!(
            "Extracting metadata: {} inputs, {} outputs",
            inputs.len(),
            outputs.len()
        );

        // Extract input names and shape
        let input_names: Vec<String> = inputs.iter().map(|i| i.name().to_string()).collect();

        // Get first input shape (assuming batch, channels, height, width)
        let input_shape = if let Some(first_input) = inputs.first() {
            if let Some(shape) = first_input.dtype().tensor_shape() {
                // Assume NCHW format: [batch, channels, height, width]
                // Extract C, H, W (skip batch dimension)
                // shape derefs to &[i64]
                if shape.len() >= 4 {
                    let c = if shape[1] < 0 { 3 } else { shape[1] as usize };
                    let h = if shape[2] < 0 { 256 } else { shape[2] as usize };
                    let w = if shape[3] < 0 { 256 } else { shape[3] as usize };
                    (c, h, w)
                } else if shape.len() == 3 {
                    let c = if shape[0] < 0 { 3 } else { shape[0] as usize };
                    let h = if shape[1] < 0 { 256 } else { shape[1] as usize };
                    let w = if shape[2] < 0 { 256 } else { shape[2] as usize };
                    (c, h, w)
                } else {
                    (3, 256, 256) // Default fallback
                }
            } else {
                (3, 256, 256) // Default fallback
            }
        } else {
            return Err(ModelError::LoadFailed {
                reason: "No input tensors found in model".to_string(),
            }
            .into());
        };

        // Extract output names and shape
        let output_names: Vec<String> = outputs.iter().map(|o| o.name().to_string()).collect();

        let output_shape = if let Some(first_output) = outputs.first() {
            if let Some(shape) = first_output.dtype().tensor_shape() {
                // Assume NCHW format: [batch, channels, height, width]
                if shape.len() >= 4 {
                    let c = if shape[1] < 0 { 1 } else { shape[1] as usize };
                    let h = if shape[2] < 0 { 256 } else { shape[2] as usize };
                    let w = if shape[3] < 0 { 256 } else { shape[3] as usize };
                    (c, h, w)
                } else if shape.len() == 3 {
                    let c = if shape[0] < 0 { 1 } else { shape[0] as usize };
                    let h = if shape[1] < 0 { 256 } else { shape[1] as usize };
                    let w = if shape[2] < 0 { 256 } else { shape[2] as usize };
                    (c, h, w)
                } else {
                    (1, 256, 256) // Default fallback
                }
            } else {
                (1, 256, 256) // Default fallback
            }
        } else {
            return Err(ModelError::LoadFailed {
                reason: "No output tensors found in model".to_string(),
            }
            .into());
        };

        Ok(ModelMetadata {
            name: "onnx_model".to_string(),
            version: "1.0.0".to_string(),
            description: "ONNX Runtime model".to_string(),
            input_names,
            output_names,
            input_shape,
            output_shape,
            class_labels: None,
        })
    }

    /// Runs inference on a raster buffer
    ///
    /// # Errors
    /// Returns an error if inference fails
    pub fn infer(&mut self, input: &RasterBuffer) -> Result<RasterBuffer> {
        debug!(
            "Running inference on {}x{} buffer",
            input.width(),
            input.height()
        );

        // Convert RasterBuffer to ndarray
        let input_array = self.buffer_to_ndarray(input)?;

        // Get input name
        let input_name =
            self.metadata
                .input_names
                .first()
                .ok_or_else(|| InferenceError::Failed {
                    reason: "No input tensor name available".to_string(),
                })?;

        // Create TensorRef from ndarray view
        let input_tensor =
            TensorRef::from_array_view(input_array.view()).map_err(|e| InferenceError::Failed {
                reason: format!("Failed to create input tensor: {}", e),
            })?;

        // Run inference using ort 2.0 API with inputs! macro
        let outputs = self
            .session
            .run(ort::inputs![input_name.as_str() => input_tensor])
            .map_err(|e| InferenceError::Failed {
                reason: format!("ONNX inference failed: {}", e),
            })?;

        // Get output name
        let output_name =
            self.metadata
                .output_names
                .first()
                .ok_or_else(|| InferenceError::Failed {
                    reason: "No output tensor name available".to_string(),
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

        // Drop outputs to release the borrow of self.session
        drop(outputs);

        // Convert back to RasterBuffer
        let output_view = output_owned.view().into_dyn();
        self.ndarray_to_buffer(&output_view)
    }

    /// Runs batch inference
    ///
    /// # Errors
    /// Returns an error if inference fails
    pub fn infer_batch(&mut self, inputs: &[RasterBuffer]) -> Result<Vec<RasterBuffer>> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }

        debug!("Running batch inference on {} inputs", inputs.len());

        // Process each input individually (ONNX Runtime handles batching internally)
        let mut results = Vec::with_capacity(inputs.len());
        for input in inputs {
            let output = self.infer(input)?;
            results.push(output);
        }

        Ok(results)
    }

    /// Converts RasterBuffer to ndarray
    fn buffer_to_ndarray(&self, buffer: &RasterBuffer) -> Result<ArrayD<f32>> {
        let width = buffer.width() as usize;
        let height = buffer.height() as usize;

        // Get expected input shape from metadata
        let (channels, expected_height, expected_width) = self.metadata.input_shape;

        // Validate dimensions
        if width != expected_width || height != expected_height {
            return Err(InferenceError::InvalidInputShape {
                expected: vec![channels, expected_height, expected_width],
                actual: vec![channels, height, width],
            }
            .into());
        }

        // Convert buffer data to f32
        let data = match buffer.data_type() {
            RasterDataType::Float32 => {
                let slice = buffer
                    .as_slice::<f32>()
                    .map_err(crate::error::MlError::OxiGdal)?;
                slice.to_vec()
            }
            RasterDataType::UInt8 => {
                let slice = buffer
                    .as_slice::<u8>()
                    .map_err(crate::error::MlError::OxiGdal)?;
                slice.iter().map(|&v| f32::from(v) / 255.0).collect()
            }
            RasterDataType::Int16 => {
                let slice = buffer
                    .as_slice::<i16>()
                    .map_err(crate::error::MlError::OxiGdal)?;
                slice.iter().map(|&v| v as f32).collect()
            }
            RasterDataType::UInt16 => {
                let slice = buffer
                    .as_slice::<u16>()
                    .map_err(crate::error::MlError::OxiGdal)?;
                slice.iter().map(|&v| f32::from(v) / 65535.0).collect()
            }
            RasterDataType::Float64 => {
                let slice = buffer
                    .as_slice::<f64>()
                    .map_err(crate::error::MlError::OxiGdal)?;
                slice.iter().map(|&v| v as f32).collect()
            }
            _ => {
                return Err(InferenceError::Failed {
                    reason: format!("Unsupported data type: {:?}", buffer.data_type()),
                }
                .into());
            }
        };

        // Calculate expected total size
        let total_pixels = height * width;
        let num_bands = data.len() / total_pixels;

        // Create array with shape [batch=1, channels, height, width]
        let shape = IxDyn(&[1, num_bands, height, width]);

        Array::from_shape_vec(shape, data).map_err(|e| {
            InferenceError::Failed {
                reason: format!("Failed to create ndarray from buffer: {}", e),
            }
            .into()
        })
    }

    /// Converts ndarray to RasterBuffer
    fn ndarray_to_buffer(&self, array: &ArrayView<f32, IxDyn>) -> Result<RasterBuffer> {
        let shape = array.shape();
        debug!("Converting ndarray with shape {:?} to RasterBuffer", shape);

        // Expect shape [batch, channels, height, width] or [channels, height, width]
        let (height, width) = if shape.len() == 4 {
            // Shape: [batch, channels, height, width]
            (shape[2], shape[3])
        } else if shape.len() == 3 {
            // Shape: [channels, height, width]
            (shape[1], shape[2])
        } else if shape.len() == 2 {
            // Shape: [height, width]
            (shape[0], shape[1])
        } else {
            return Err(InferenceError::OutputParsingFailed {
                reason: format!("Unexpected output shape: {:?}", shape),
            }
            .into());
        };

        // Convert to contiguous vec
        let data: Vec<f32> = array.iter().copied().collect();

        // Convert to bytes
        let bytes: Vec<u8> = data.iter().flat_map(|&f: &f32| f.to_le_bytes()).collect();

        // Create RasterBuffer
        RasterBuffer::new(
            bytes,
            width as u64,
            height as u64,
            RasterDataType::Float32,
            oxigdal_core::types::NoDataValue::None,
        )
        .map_err(crate::error::MlError::OxiGdal)
    }
}

impl Model for OnnxModel {
    fn metadata(&self) -> &ModelMetadata {
        &self.metadata
    }

    fn predict(&mut self, input: &RasterBuffer) -> Result<RasterBuffer> {
        self.infer(input)
    }

    fn predict_batch(&mut self, inputs: &[RasterBuffer]) -> Result<Vec<RasterBuffer>> {
        self.infer_batch(inputs)
    }

    fn input_shape(&self) -> (usize, usize, usize) {
        self.metadata.input_shape
    }

    fn output_shape(&self) -> (usize, usize, usize) {
        self.metadata.output_shape
    }
}

/// Returns the number of CPUs
fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_config_default() {
        let config = SessionConfig::default();
        assert_eq!(config.execution_provider, ExecutionProvider::Cpu);
        assert!(config.graph_optimization);
        assert_eq!(config.batch_size, 1);
    }

    #[test]
    fn test_metadata_serialization() {
        let metadata = ModelMetadata {
            name: "test_model".to_string(),
            version: "1.0.0".to_string(),
            description: "Test model".to_string(),
            input_names: vec!["input".to_string()],
            output_names: vec!["output".to_string()],
            input_shape: (3, 256, 256),
            output_shape: (1, 256, 256),
            class_labels: None,
        };

        let json = serde_json::to_string(&metadata);
        assert!(json.is_ok());
    }

    #[test]
    fn test_num_cpus() {
        let cpus = num_cpus();
        assert!(cpus > 0);
        assert!(cpus <= 256); // Reasonable upper bound
    }
}
