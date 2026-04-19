//! ONNX Runtime integration for OxiGDAL
//!
//! This module provides integration with ONNX Runtime for running ML models
//! on geospatial data.

use std::path::Path;

use ndarray::{Array, ArrayD, ArrayView, IxDyn};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;
use oxionnx::{GraphOptimizationLevel, Session, SessionBuilder, Tensor};
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

        // Create SessionBuilder with configuration — builder methods return Self (no Result)
        let mut builder = SessionBuilder::new();

        // Configure number of threads — sets per-session rayon thread pool and enables parallel execution
        builder = builder.with_intra_threads(config.num_threads);

        // Configure graph optimization
        if config.graph_optimization {
            builder = builder.with_optimization_level(GraphOptimizationLevel::All);
        }

        // Configure execution provider
        #[cfg(feature = "gpu")]
        {
            use oxionnx::CUDAExecutionProvider;
            if matches!(config.execution_provider, ExecutionProvider::Cuda) {
                builder = builder.with_execution_providers([CUDAExecutionProvider.build()]);
            }
        }

        #[cfg(feature = "coreml")]
        {
            use oxionnx::CoreMLExecutionProvider;
            if matches!(config.execution_provider, ExecutionProvider::CoreMl) {
                builder = builder.with_execution_providers([CoreMLExecutionProvider.build()]);
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
        // Get input/output metadata — TensorInfo { name: String, dtype: DType, shape: Vec<Option<usize>> }
        let inputs = session.input_info();
        let outputs = session.output_info();

        debug!(
            "Extracting metadata: {} inputs, {} outputs",
            inputs.len(),
            outputs.len()
        );

        // Extract input names
        let input_names: Vec<String> = inputs.iter().map(|i| i.name.clone()).collect();

        // Get first input shape (assuming batch, channels, height, width)
        // shape elements are Option<usize>: None means dynamic dimension
        let input_shape = if let Some(first_input) = inputs.first() {
            let shape = &first_input.shape;
            if shape.len() >= 4 {
                // NCHW format: [batch, channels, height, width]
                let c = shape[1].unwrap_or(3);
                let h = shape[2].unwrap_or(256);
                let w = shape[3].unwrap_or(256);
                (c, h, w)
            } else if shape.len() == 3 {
                let c = shape[0].unwrap_or(3);
                let h = shape[1].unwrap_or(256);
                let w = shape[2].unwrap_or(256);
                (c, h, w)
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
        let output_names: Vec<String> = outputs.iter().map(|o| o.name.clone()).collect();

        let output_shape = if let Some(first_output) = outputs.first() {
            let shape = &first_output.shape;
            if shape.len() >= 4 {
                let c = shape[1].unwrap_or(1);
                let h = shape[2].unwrap_or(256);
                let w = shape[3].unwrap_or(256);
                (c, h, w)
            } else if shape.len() == 3 {
                let c = shape[0].unwrap_or(1);
                let h = shape[1].unwrap_or(256);
                let w = shape[2].unwrap_or(256);
                (c, h, w)
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
        let input_name = self
            .metadata
            .input_names
            .first()
            .ok_or_else(|| InferenceError::Failed {
                reason: "No input tensor name available".to_string(),
            })?
            .clone();

        // Create Tensor from ndarray (from_ndarray_view returns Tensor directly, no Result)
        let input_tensor = Tensor::from_ndarray_view(input_array.view());

        // Build inputs map using oxionnx::inputs! macro (returns Result<HashMap<&str, Tensor>>)
        let inputs_map = oxionnx::inputs![input_name.as_str() => input_tensor].map_err(|e| {
            InferenceError::Failed {
                reason: format!("Failed to build inputs map: {}", e),
            }
        })?;

        // Run inference — session.run takes &HashMap<&str, Tensor>
        let outputs = self
            .session
            .run(&inputs_map)
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

        // Extract output tensor from HashMap<String, Tensor>
        let output_tensor = outputs.get(output_name.as_str()).ok_or_else(|| {
            InferenceError::OutputParsingFailed {
                reason: format!("Output tensor '{}' not found", output_name),
            }
        })?;

        // Extract ndarray view from Tensor
        let output_array = output_tensor.try_extract_array::<f32>().map_err(|e| {
            InferenceError::OutputParsingFailed {
                reason: format!("Failed to extract output tensor: {}", e),
            }
        })?;

        // Convert to owned array to avoid borrow checker issues
        let output_owned = output_array.to_owned();

        // Drop outputs to release the borrow
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

    // ── End-to-end ONNX inference tests using in-memory graph construction ──

    /// Helper: builds a minimal ONNX graph with the given op, input/output names, shape,
    /// and pre-loaded weights, then returns a ready-to-run Session.
    fn build_session_from_graph(
        graph: oxionnx::Graph,
        weights: std::collections::HashMap<String, Tensor>,
    ) -> Result<Session> {
        let session = SessionBuilder::new()
            .build_from_graph(graph, weights)
            .map_err(|e| ModelError::LoadFailed {
                reason: format!("Failed to build session from graph: {}", e),
            })?;
        Ok(session)
    }

    /// Builds an Identity-op graph: output = Identity(input)
    fn build_identity_graph(
        input_name: &str,
        output_name: &str,
        shape: &[Option<usize>],
    ) -> oxionnx::Graph {
        use oxionnx::{Attributes, DType, Node, OpKind, TensorInfo};

        oxionnx::Graph {
            name: "identity_test".to_string(),
            nodes: vec![Node {
                op: OpKind::Identity,
                name: "identity_0".to_string(),
                inputs: vec![input_name.to_string()],
                outputs: vec![output_name.to_string()],
                attrs: Attributes::default(),
            }],
            input_names: vec![input_name.to_string()],
            output_names: vec![output_name.to_string()],
            input_infos: vec![TensorInfo {
                name: input_name.to_string(),
                dtype: DType::F32,
                shape: shape.to_vec(),
                dim_params: vec![],
            }],
            output_infos: vec![TensorInfo {
                name: output_name.to_string(),
                dtype: DType::F32,
                shape: shape.to_vec(),
                dim_params: vec![],
            }],
        }
    }

    /// Builds a Relu-op graph: output = Relu(input)
    fn build_relu_graph(
        input_name: &str,
        output_name: &str,
        shape: &[Option<usize>],
    ) -> oxionnx::Graph {
        use oxionnx::{Attributes, DType, Node, OpKind, TensorInfo};

        oxionnx::Graph {
            name: "relu_test".to_string(),
            nodes: vec![Node {
                op: OpKind::Relu,
                name: "relu_0".to_string(),
                inputs: vec![input_name.to_string()],
                outputs: vec![output_name.to_string()],
                attrs: Attributes::default(),
            }],
            input_names: vec![input_name.to_string()],
            output_names: vec![output_name.to_string()],
            input_infos: vec![TensorInfo {
                name: input_name.to_string(),
                dtype: DType::F32,
                shape: shape.to_vec(),
                dim_params: vec![],
            }],
            output_infos: vec![TensorInfo {
                name: output_name.to_string(),
                dtype: DType::F32,
                shape: shape.to_vec(),
                dim_params: vec![],
            }],
        }
    }

    /// Builds an Add-op graph with a constant weight: output = input + bias
    fn build_add_bias_graph(
        input_name: &str,
        bias_name: &str,
        output_name: &str,
        shape: &[Option<usize>],
    ) -> oxionnx::Graph {
        use oxionnx::{Attributes, DType, Node, OpKind, TensorInfo};

        oxionnx::Graph {
            name: "add_bias_test".to_string(),
            nodes: vec![Node {
                op: OpKind::Add,
                name: "add_0".to_string(),
                inputs: vec![input_name.to_string(), bias_name.to_string()],
                outputs: vec![output_name.to_string()],
                attrs: Attributes::default(),
            }],
            input_names: vec![input_name.to_string()],
            output_names: vec![output_name.to_string()],
            input_infos: vec![TensorInfo {
                name: input_name.to_string(),
                dtype: DType::F32,
                shape: shape.to_vec(),
                dim_params: vec![],
            }],
            output_infos: vec![TensorInfo {
                name: output_name.to_string(),
                dtype: DType::F32,
                shape: shape.to_vec(),
                dim_params: vec![],
            }],
        }
    }

    #[test]
    fn test_identity_inference_end_to_end() {
        // Build an Identity graph: output should equal input
        let shape = &[Some(1), Some(3), Some(4), Some(4)];
        let graph = build_identity_graph("X", "Y", shape);
        let session = build_session_from_graph(graph, std::collections::HashMap::new())
            .expect("build identity session");

        // Create input tensor: [1, 3, 4, 4] = 48 elements
        let input_data: Vec<f32> = (0..48).map(|i| i as f32 * 0.1).collect();
        let input_tensor = Tensor::new(input_data.clone(), vec![1, 3, 4, 4]);

        let inputs_map = oxionnx::inputs!["X" => input_tensor].expect("build inputs map");
        let outputs = session.run(&inputs_map).expect("run identity inference");

        let output = outputs.get("Y").expect("output Y not found");
        let (out_shape, out_data) = output
            .try_extract_tensor::<f32>()
            .expect("extract output tensor");

        assert_eq!(out_shape, &[1, 3, 4, 4]);
        assert_eq!(out_data.len(), 48);
        for (a, b) in input_data.iter().zip(out_data.iter()) {
            assert!((a - b).abs() < 1e-6, "identity mismatch: {} vs {}", a, b);
        }
    }

    #[test]
    fn test_relu_inference_end_to_end() {
        // Build a Relu graph: output = max(0, input)
        let shape = &[Some(1), Some(1), Some(2), Some(3)];
        let graph = build_relu_graph("input", "output", shape);
        let session = build_session_from_graph(graph, std::collections::HashMap::new())
            .expect("build relu session");

        // Input with negative values
        let input_data: Vec<f32> = vec![-3.0, -1.0, 0.0, 1.0, 2.5, -0.5];
        let expected: Vec<f32> = vec![0.0, 0.0, 0.0, 1.0, 2.5, 0.0];

        let input_tensor = Tensor::new(input_data, vec![1, 1, 2, 3]);
        let inputs_map = oxionnx::inputs!["input" => input_tensor].expect("build inputs map");
        let outputs = session.run(&inputs_map).expect("run relu inference");

        let output = outputs.get("output").expect("output not found");
        let (out_shape, out_data) = output.try_extract_tensor::<f32>().expect("extract output");

        assert_eq!(out_shape, &[1, 1, 2, 3]);
        for (a, b) in expected.iter().zip(out_data.iter()) {
            assert!(
                (a - b).abs() < 1e-6,
                "relu mismatch: expected {} got {}",
                a,
                b
            );
        }
    }

    #[test]
    fn test_add_bias_inference_end_to_end() {
        // Build an Add graph with a constant bias weight: output = input + bias
        let shape = &[Some(1), Some(1), Some(2), Some(2)];
        let graph = build_add_bias_graph("input", "bias", "output", shape);

        // Pre-load bias as a weight tensor
        let mut weights = std::collections::HashMap::new();
        weights.insert(
            "bias".to_string(),
            Tensor::new(vec![10.0, 20.0, 30.0, 40.0], vec![1, 1, 2, 2]),
        );

        let session = build_session_from_graph(graph, weights).expect("build add session");

        let input_data: Vec<f32> = vec![1.0, 2.0, 3.0, 4.0];
        let expected: Vec<f32> = vec![11.0, 22.0, 33.0, 44.0];

        let input_tensor = Tensor::new(input_data, vec![1, 1, 2, 2]);
        let inputs_map = oxionnx::inputs!["input" => input_tensor].expect("build inputs map");
        let outputs = session.run(&inputs_map).expect("run add inference");

        let output = outputs.get("output").expect("output not found");
        let (out_shape, out_data) = output.try_extract_tensor::<f32>().expect("extract output");

        assert_eq!(out_shape, &[1, 1, 2, 2]);
        for (a, b) in expected.iter().zip(out_data.iter()) {
            assert!(
                (a - b).abs() < 1e-6,
                "add mismatch: expected {} got {}",
                a,
                b
            );
        }
    }

    #[test]
    fn test_metadata_extraction_nchw_shape() {
        // Build a graph with NCHW shape [batch=1, C=3, H=64, W=64]
        let shape = &[Some(1), Some(3), Some(64), Some(64)];
        let graph = build_identity_graph("input", "output", shape);
        let session = build_session_from_graph(graph, std::collections::HashMap::new())
            .expect("build session for metadata extraction");

        let metadata = OnnxModel::extract_metadata(&session).expect("extract metadata");

        assert_eq!(metadata.input_names, vec!["input"]);
        assert_eq!(metadata.output_names, vec!["output"]);
        assert_eq!(metadata.input_shape, (3, 64, 64));
        assert_eq!(metadata.output_shape, (3, 64, 64));
    }

    #[test]
    fn test_metadata_extraction_dynamic_dims() {
        // Build a graph with dynamic batch dimension: [None, 3, 32, 32]
        let shape = &[None, Some(3), Some(32), Some(32)];
        let graph = build_identity_graph("x", "y", shape);
        let session = build_session_from_graph(graph, std::collections::HashMap::new())
            .expect("build session for dynamic dim test");

        let metadata =
            OnnxModel::extract_metadata(&session).expect("extract metadata with dynamic dims");

        // Dynamic batch dim should fall through to defaults or be ignored;
        // channels/height/width should be resolved from the static dims
        assert_eq!(metadata.input_shape, (3, 32, 32));
        assert_eq!(metadata.output_shape, (3, 32, 32));
    }

    #[test]
    fn test_metadata_extraction_3d_shape() {
        // Build a graph with 3D shape [C, H, W] (no batch dimension)
        let shape = &[Some(3), Some(128), Some(128)];
        let graph = build_identity_graph("img", "out", shape);
        let session = build_session_from_graph(graph, std::collections::HashMap::new())
            .expect("build session for 3D shape test");

        let metadata = OnnxModel::extract_metadata(&session).expect("extract 3D metadata");

        // With 3D input, should interpret as [C, H, W]
        assert_eq!(metadata.input_shape, (3, 128, 128));
    }

    #[test]
    fn test_session_builder_with_intra_threads() {
        // Verify that with_intra_threads produces a functional session
        let shape = &[Some(1), Some(1), Some(2), Some(2)];
        let graph = build_identity_graph("x", "y", shape);

        let session = SessionBuilder::new()
            .with_intra_threads(2)
            .build_from_graph(graph, std::collections::HashMap::new())
            .map_err(|e| ModelError::LoadFailed {
                reason: e.to_string(),
            })
            .expect("build session with intra_threads");

        let input_tensor = Tensor::new(vec![1.0, 2.0, 3.0, 4.0], vec![1, 1, 2, 2]);
        let inputs_map = oxionnx::inputs!["x" => input_tensor].expect("build inputs map");
        let outputs = session.run(&inputs_map).expect("run with intra_threads");
        assert!(outputs.contains_key("y"));
    }

    #[test]
    fn test_execution_provider_variants() {
        // Verify all execution provider enum variants compile and compare correctly
        let cpu = ExecutionProvider::Cpu;
        assert_eq!(cpu, ExecutionProvider::Cpu);

        #[cfg(feature = "gpu")]
        {
            let cuda = ExecutionProvider::Cuda;
            assert_eq!(cuda, ExecutionProvider::Cuda);
            assert_ne!(cuda, ExecutionProvider::Cpu);
        }
    }

    #[test]
    fn test_two_node_pipeline_relu_identity() {
        // Build a two-node graph: intermediate = Relu(input), output = Identity(intermediate)
        use oxionnx::{Attributes, DType, Node, OpKind, TensorInfo};

        let graph = oxionnx::Graph {
            name: "relu_identity_pipeline".to_string(),
            nodes: vec![
                Node {
                    op: OpKind::Relu,
                    name: "relu_0".to_string(),
                    inputs: vec!["input".to_string()],
                    outputs: vec!["intermediate".to_string()],
                    attrs: Attributes::default(),
                },
                Node {
                    op: OpKind::Identity,
                    name: "identity_0".to_string(),
                    inputs: vec!["intermediate".to_string()],
                    outputs: vec!["output".to_string()],
                    attrs: Attributes::default(),
                },
            ],
            input_names: vec!["input".to_string()],
            output_names: vec!["output".to_string()],
            input_infos: vec![TensorInfo {
                name: "input".to_string(),
                dtype: DType::F32,
                shape: vec![Some(1), Some(1), Some(2), Some(3)],
                dim_params: vec![],
            }],
            output_infos: vec![TensorInfo {
                name: "output".to_string(),
                dtype: DType::F32,
                shape: vec![Some(1), Some(1), Some(2), Some(3)],
                dim_params: vec![],
            }],
        };

        let session = build_session_from_graph(graph, std::collections::HashMap::new())
            .expect("build pipeline session");

        let input_data: Vec<f32> = vec![-5.0, -1.0, 0.0, 3.0, 7.0, -2.0];
        let expected: Vec<f32> = vec![0.0, 0.0, 0.0, 3.0, 7.0, 0.0];

        let input_tensor = Tensor::new(input_data, vec![1, 1, 2, 3]);
        let inputs_map = oxionnx::inputs!["input" => input_tensor].expect("build inputs map");
        let outputs = session.run(&inputs_map).expect("run pipeline");

        let output = outputs.get("output").expect("output not found");
        let (_shape, out_data) = output.try_extract_tensor::<f32>().expect("extract output");

        for (a, b) in expected.iter().zip(out_data.iter()) {
            assert!(
                (a - b).abs() < 1e-6,
                "pipeline mismatch: expected {} got {}",
                a,
                b,
            );
        }
    }

    #[test]
    fn test_ndarray_tensor_roundtrip() {
        // Verify that from_ndarray_view produces correct tensors and try_extract_array returns them
        let arr = ndarray::Array::from_shape_vec(
            ndarray::IxDyn(&[1, 2, 3]),
            vec![1.0_f32, 2.0, 3.0, 4.0, 5.0, 6.0],
        )
        .expect("create ndarray");

        let tensor = Tensor::from_ndarray_view(arr.view());
        let extracted = tensor
            .try_extract_array::<f32>()
            .expect("extract array from tensor");

        assert_eq!(extracted.shape(), &[1, 2, 3]);
        for (a, b) in arr.iter().zip(extracted.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn test_model_not_found_error() {
        let result = OnnxModel::from_file("/nonexistent/path/model.onnx");
        assert!(result.is_err());
        let err_msg = format!("{}", result.err().expect("should be error"));
        assert!(
            err_msg.contains("not found") || err_msg.contains("Not"),
            "error should mention 'not found', got: {}",
            err_msg,
        );
    }

    #[test]
    fn test_gpu_feature_compilation() {
        // This test verifies that the #[cfg(feature = "gpu")] blocks compile correctly.
        // When gpu feature is enabled, ExecutionProvider::Cuda should exist.
        // When not enabled, only Cpu is available. Either way, this test compiles.
        let config = SessionConfig {
            execution_provider: ExecutionProvider::Cpu,
            num_threads: 1,
            graph_optimization: false,
            batch_size: 2,
        };
        assert_eq!(config.batch_size, 2);
        assert!(!config.graph_optimization);
    }
}
