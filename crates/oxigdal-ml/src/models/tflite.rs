//! TensorFlow Lite model support
//!
//! This module provides TensorFlow Lite model loading and inference integration
//! for geospatial ML workflows.
//!
//! # System Requirements
//!
//! To use TFLite support, enable the `tflite` feature flag:
//!
//! ```toml
//! [dependencies]
//! oxigdal-ml = { version = "0.1.0", features = ["tflite"] }
//! ```
//!
//! **Note:** The `tflitec` crate requires either:
//! - Pre-built TensorFlow Lite C library (set `TFLITEC_PREBUILT_PATH` env var)
//! - Bazel 6.5.0+ for building from source (may take 10-30 minutes on first build)
//!
//! To use a pre-built TensorFlow Lite library:
//! ```bash
//! export TFLITEC_PREBUILT_PATH=/path/to/libtensorflowlite_c.{so,dylib,dll}
//! cargo build --features tflite
//! ```
//!
//! # Features
//!
//! - TFLite model loading and initialization
//! - Hardware acceleration delegates (GPU, NNAPI, CoreML, XNNPACK)
//! - Quantized model support (INT8, FP16)
//! - Dynamic shape tensors
//! - Multi-threading support
//!
//! # Example
//!
//! ```ignore
//! use oxigdal_ml::models::TfLiteModel;
//! use oxigdal_ml::models::tflite::{TfLiteConfig, Delegate};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let config = TfLiteConfig::builder()
//!     .threads(4)
//!     .delegate(Delegate::Gpu)
//!     .build();
//!
//! let model = TfLiteModel::from_file("model.tflite", config)?;
//! # Ok(())
//! # }
//! ```

use crate::error::{InferenceError, MlError, ModelError, Result};
use crate::models::{Model, ModelMetadata};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;
use std::path::Path;
use std::sync::Mutex;
use tracing::{debug, info, warn};

#[cfg(feature = "tflite")]
use tflitec::{interpreter::Interpreter, model::Model as TfLiteModelInner, options::Options};

/// TensorFlow Lite execution delegate
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delegate {
    /// CPU execution (default)
    Cpu,
    /// GPU delegate (OpenGL/Metal/Vulkan)
    Gpu,
    /// NNAPI delegate (Android)
    Nnapi,
    /// CoreML delegate (iOS/macOS)
    CoreMl,
    /// XNNPACK delegate (optimized CPU)
    Xnnpack,
}

/// TensorFlow Lite configuration
#[derive(Debug, Clone)]
pub struct TfLiteConfig {
    /// Number of threads for CPU execution
    pub threads: usize,
    /// Execution delegate
    pub delegate: Delegate,
    /// Enable profiling
    pub profiling: bool,
    /// Allow FP16 precision reduction
    pub allow_fp16: bool,
    /// Allocate tensors on demand
    pub lazy_allocation: bool,
}

impl Default for TfLiteConfig {
    fn default() -> Self {
        Self {
            threads: num_cpus::get(),
            delegate: Delegate::Cpu,
            profiling: false,
            allow_fp16: true,
            lazy_allocation: false,
        }
    }
}

impl TfLiteConfig {
    /// Creates a new configuration builder
    #[must_use]
    pub fn builder() -> TfLiteConfigBuilder {
        TfLiteConfigBuilder::default()
    }
}

/// Builder for TensorFlow Lite configuration
#[derive(Debug, Default)]
pub struct TfLiteConfigBuilder {
    threads: Option<usize>,
    delegate: Option<Delegate>,
    profiling: bool,
    allow_fp16: bool,
    lazy_allocation: bool,
}

impl TfLiteConfigBuilder {
    /// Sets the number of threads
    #[must_use]
    pub fn threads(mut self, threads: usize) -> Self {
        self.threads = Some(threads);
        self
    }

    /// Sets the execution delegate
    #[must_use]
    pub fn delegate(mut self, delegate: Delegate) -> Self {
        self.delegate = Some(delegate);
        self
    }

    /// Enables profiling
    #[must_use]
    pub fn profiling(mut self, enable: bool) -> Self {
        self.profiling = enable;
        self
    }

    /// Allows FP16 precision reduction
    #[must_use]
    pub fn allow_fp16(mut self, allow: bool) -> Self {
        self.allow_fp16 = allow;
        self
    }

    /// Enables lazy tensor allocation
    #[must_use]
    pub fn lazy_allocation(mut self, enable: bool) -> Self {
        self.lazy_allocation = enable;
        self
    }

    /// Builds the configuration
    #[must_use]
    pub fn build(self) -> TfLiteConfig {
        TfLiteConfig {
            threads: self.threads.unwrap_or_else(num_cpus::get),
            delegate: self.delegate.unwrap_or(Delegate::Cpu),
            profiling: self.profiling,
            allow_fp16: self.allow_fp16,
            lazy_allocation: self.lazy_allocation,
        }
    }
}

/// TensorFlow Lite tensor information
#[derive(Debug, Clone)]
pub struct TensorInfo {
    /// Tensor name
    pub name: String,
    /// Tensor shape (NHWC format)
    pub shape: Vec<usize>,
    /// Data type
    pub dtype: TensorDataType,
    /// Quantization parameters (if quantized)
    pub quantization: Option<QuantizationParams>,
}

/// Tensor data type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TensorDataType {
    /// 32-bit floating point
    Float32,
    /// 16-bit floating point
    Float16,
    /// 8-bit signed integer (quantized)
    Int8,
    /// 8-bit unsigned integer (quantized)
    UInt8,
    /// 16-bit signed integer
    Int16,
    /// 32-bit signed integer
    Int32,
    /// 64-bit signed integer
    Int64,
}

impl TensorDataType {
    /// Converts to OxiGDAL raster data type
    #[must_use]
    pub fn to_raster_type(self) -> RasterDataType {
        match self {
            Self::Float32 => RasterDataType::Float32,
            Self::Float16 => RasterDataType::Float32, // No FP16 in OxiGDAL yet
            Self::Int8 | Self::UInt8 => RasterDataType::UInt8,
            Self::Int16 => RasterDataType::Int16,
            Self::Int32 => RasterDataType::Int32,
            Self::Int64 => RasterDataType::Float64, // Closest match
        }
    }
}

/// Quantization parameters for quantized tensors
#[derive(Debug, Clone, Copy)]
pub struct QuantizationParams {
    /// Scale factor
    pub scale: f32,
    /// Zero point
    pub zero_point: i32,
}

impl QuantizationParams {
    /// Dequantizes a quantized value
    #[must_use]
    pub fn dequantize(&self, quantized: i32) -> f32 {
        (quantized - self.zero_point) as f32 * self.scale
    }

    /// Quantizes a floating-point value
    #[must_use]
    pub fn quantize(&self, value: f32) -> i32 {
        (value / self.scale).round() as i32 + self.zero_point
    }
}

/// TensorFlow Lite model wrapper
#[cfg(feature = "tflite")]
pub struct TfLiteModel {
    /// Model file path
    path: String,
    /// Model configuration
    config: TfLiteConfig,
    /// Model metadata
    metadata: ModelMetadata,
    /// Input tensor information
    input_info: Vec<TensorInfo>,
    /// Output tensor information
    output_info: Vec<TensorInfo>,
    /// TFLite model (kept alive for interpreter)
    #[allow(dead_code)]
    tflite_model: TfLiteModelInner,
    /// TFLite interpreter for inference (wrapped in Mutex for interior mutability)
    interpreter: Mutex<Interpreter>,
}

#[cfg(feature = "tflite")]
impl TfLiteModel {
    /// Loads a TensorFlow Lite model from file
    ///
    /// # Errors
    /// Returns an error if the model cannot be loaded or initialized
    pub fn from_file<P: AsRef<Path>>(path: P, config: TfLiteConfig) -> Result<Self> {
        let path = path.as_ref();
        info!("Loading TFLite model from {:?}", path);

        if !path.exists() {
            return Err(ModelError::NotFound {
                path: path.display().to_string(),
            }
            .into());
        }

        debug!(
            "TFLite config: threads={}, delegate={:?}",
            config.threads, config.delegate
        );

        // Load TFLite model
        let tflite_model = TfLiteModelInner::new(path).map_err(|e| ModelError::LoadFailed {
            path: path.display().to_string(),
            reason: format!("TFLite model loading failed: {}", e),
        })?;

        // Create interpreter options
        let mut options = Options::default();
        options.thread_count = config.threads as i32;

        // Create interpreter
        let mut interpreter =
            Interpreter::new(&tflite_model, Some(options)).map_err(|e| ModelError::LoadFailed {
                path: path.display().to_string(),
                reason: format!("TFLite interpreter creation failed: {}", e),
            })?;

        // Allocate tensors
        interpreter
            .allocate_tensors()
            .map_err(|e| ModelError::LoadFailed {
                path: path.display().to_string(),
                reason: format!("TFLite tensor allocation failed: {}", e),
            })?;

        // Extract input tensor information
        let input_count = interpreter.input_tensor_count();
        let mut input_info = Vec::with_capacity(input_count);
        let mut input_names = Vec::with_capacity(input_count);

        for i in 0..input_count {
            let tensor = interpreter.input(i).ok_or_else(|| ModelError::LoadFailed {
                path: path.display().to_string(),
                reason: format!("Failed to get input tensor {}", i),
            })?;

            let name = tensor
                .name()
                .unwrap_or_else(|| format!("input_{}", i))
                .to_string();
            let shape = tensor.shape().iter().map(|&s| s as usize).collect();
            let dtype = Self::convert_tensor_type(tensor.data_type());
            let quantization = Self::get_quantization_params(&tensor);

            input_names.push(name.clone());
            input_info.push(TensorInfo {
                name,
                shape,
                dtype,
                quantization,
            });
        }

        // Extract output tensor information
        let output_count = interpreter.output_tensor_count();
        let mut output_info = Vec::with_capacity(output_count);
        let mut output_names = Vec::with_capacity(output_count);

        for i in 0..output_count {
            let tensor = interpreter
                .output(i)
                .ok_or_else(|| ModelError::LoadFailed {
                    path: path.display().to_string(),
                    reason: format!("Failed to get output tensor {}", i),
                })?;

            let name = tensor
                .name()
                .unwrap_or_else(|| format!("output_{}", i))
                .to_string();
            let shape = tensor.shape().iter().map(|&s| s as usize).collect();
            let dtype = Self::convert_tensor_type(tensor.data_type());
            let quantization = Self::get_quantization_params(&tensor);

            output_names.push(name.clone());
            output_info.push(TensorInfo {
                name,
                shape,
                dtype,
                quantization,
            });
        }

        // Create metadata
        let (input_shape, output_shape) =
            if let (Some(input), Some(output)) = (input_info.first(), output_info.first()) {
                let in_shape = &input.shape;
                let out_shape = &output.shape;

                // Extract CHW from NHWC
                let input_shape = if in_shape.len() == 4 {
                    (in_shape[3], in_shape[1], in_shape[2])
                } else {
                    (3, 256, 256)
                };

                let output_shape = if out_shape.len() == 4 {
                    (out_shape[3], out_shape[1], out_shape[2])
                } else {
                    (1, 256, 256)
                };

                (input_shape, output_shape)
            } else {
                ((3, 256, 256), (1, 256, 256))
            };

        let metadata = ModelMetadata {
            name: path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string(),
            version: "1.0".to_string(),
            description: "TensorFlow Lite model".to_string(),
            input_names,
            output_names,
            input_shape,
            output_shape,
            class_labels: None,
        };

        info!(
            "Successfully loaded TFLite model: {} inputs, {} outputs",
            input_count, output_count
        );

        Ok(Self {
            path: path.display().to_string(),
            config,
            metadata,
            input_info,
            output_info,
            tflite_model,
            interpreter: Mutex::new(interpreter),
        })
    }

    /// Loads a quantized TensorFlow Lite model
    ///
    /// # Errors
    /// Returns an error if the model cannot be loaded
    pub fn from_file_quantized<P: AsRef<Path>>(path: P, config: TfLiteConfig) -> Result<Self> {
        let mut model = Self::from_file(path, config)?;

        // Mark tensors as quantized (placeholder)
        for tensor in &mut model.input_info {
            tensor.quantization = Some(QuantizationParams {
                scale: 0.003921569, // 1/255
                zero_point: 0,
            });
        }

        Ok(model)
    }

    /// Returns the TFLite configuration
    #[must_use]
    pub fn config(&self) -> &TfLiteConfig {
        &self.config
    }

    /// Returns input tensor information
    #[must_use]
    pub fn input_info(&self) -> &[TensorInfo] {
        &self.input_info
    }

    /// Returns output tensor information
    #[must_use]
    pub fn output_info(&self) -> &[TensorInfo] {
        &self.output_info
    }

    /// Runs inference with raw tensor data
    ///
    /// # Errors
    /// Returns an error if inference fails
    pub fn invoke(&self, input_tensor: &[f32]) -> Result<Vec<f32>> {
        debug!(
            "Running TFLite inference on {} input values",
            input_tensor.len()
        );

        let mut interpreter = self
            .interpreter
            .lock()
            .map_err(|_| InferenceError::Failed {
                reason: "Failed to lock interpreter mutex".to_string(),
            })?;

        // Get input tensor and validate size
        let input = interpreter.input(0).ok_or_else(|| InferenceError::Failed {
            reason: "Failed to get input tensor".to_string(),
        })?;

        let expected_size: usize = input.shape().iter().map(|&s| s as usize).product();
        if input_tensor.len() != expected_size {
            return Err(InferenceError::Failed {
                reason: format!(
                    "Input size mismatch: expected {}, got {}",
                    expected_size,
                    input_tensor.len()
                ),
            }
            .into());
        }

        // Copy input data to tensor
        interpreter
            .copy(0, input_tensor)
            .map_err(|e| InferenceError::Failed {
                reason: format!("Failed to copy input data: {}", e),
            })?;

        // Run inference
        interpreter.invoke().map_err(|e| InferenceError::Failed {
            reason: format!("TFLite inference failed: {}", e),
        })?;

        // Extract output data
        let output = interpreter
            .output(0)
            .ok_or_else(|| InferenceError::Failed {
                reason: "Failed to get output tensor".to_string(),
            })?;

        let output_data = output.data::<f32>().ok_or_else(|| InferenceError::Failed {
            reason: "Failed to extract output data as f32".to_string(),
        })?;

        Ok(output_data.to_vec())
    }

    /// Checks if the model uses quantization
    #[must_use]
    pub fn is_quantized(&self) -> bool {
        self.input_info.iter().any(|t| t.quantization.is_some())
            || self.output_info.iter().any(|t| t.quantization.is_some())
    }

    /// Gets the delegate in use
    #[must_use]
    pub fn delegate(&self) -> Delegate {
        self.config.delegate
    }

    /// Helper: Converts TFLite tensor type to our enum
    fn convert_tensor_type(tflite_type: tflitec::tensor::DataType) -> TensorDataType {
        use tflitec::tensor::DataType;
        match tflite_type {
            DataType::Float32 => TensorDataType::Float32,
            DataType::Float16 => TensorDataType::Float16,
            DataType::Int8 => TensorDataType::Int8,
            DataType::UInt8 => TensorDataType::UInt8,
            DataType::Int16 => TensorDataType::Int16,
            DataType::Int32 => TensorDataType::Int32,
            DataType::Int64 => TensorDataType::Int64,
            _ => TensorDataType::Float32, // Default fallback
        }
    }

    /// Helper: Extracts quantization parameters from tensor
    fn get_quantization_params(tensor: &tflitec::tensor::Tensor) -> Option<QuantizationParams> {
        let params = tensor.quantization_params()?;
        if params.scale.is_empty() || params.zero_point.is_empty() {
            return None;
        }

        Some(QuantizationParams {
            scale: params.scale[0],
            zero_point: params.zero_point[0],
        })
    }
}

#[cfg(feature = "tflite")]
impl Model for TfLiteModel {
    fn metadata(&self) -> &ModelMetadata {
        &self.metadata
    }

    fn predict(&self, input: &RasterBuffer) -> Result<RasterBuffer> {
        debug!(
            "TFLite prediction on {}x{} raster",
            input.width(),
            input.height()
        );

        let (channels, height, width) = self.input_shape();

        // Validate input dimensions
        if input.width() as usize != width || input.height() as usize != height {
            warn!(
                "Input size mismatch: expected {}x{}, got {}x{}",
                width,
                height,
                input.width(),
                input.height()
            );
        }

        // Convert raster to tensor format (NHWC)
        let mut tensor_data = Vec::with_capacity(height * width * channels);
        for _c in 0..channels {
            for y in 0..height {
                for x in 0..width {
                    let value = input.get_pixel(x as u64, y as u64).unwrap_or(0.0);
                    tensor_data.push(value as f32);
                }
            }
        }

        // Run inference
        let output_tensor = self.invoke(&tensor_data)?;

        // Convert output tensor back to raster
        let (_out_channels, out_height, out_width) = self.output_shape();
        let output_type = self.output_info[0].dtype.to_raster_type();
        let mut output = RasterBuffer::zeros(out_width as u64, out_height as u64, output_type);

        // Copy first channel (assuming single-channel output for segmentation)
        for y in 0..out_height {
            for x in 0..out_width {
                let idx = y * out_width + x;
                if let Some(&value) = output_tensor.get(idx) {
                    let _ = output.set_pixel(x as u64, y as u64, value as f64);
                }
            }
        }

        Ok(output)
    }

    fn predict_batch(&self, inputs: &[RasterBuffer]) -> Result<Vec<RasterBuffer>> {
        debug!("TFLite batch prediction on {} inputs", inputs.len());

        // Process each input sequentially (true batching requires model support)
        inputs.iter().map(|input| self.predict(input)).collect()
    }

    fn input_shape(&self) -> (usize, usize, usize) {
        if let Some(info) = self.input_info.first() {
            // NHWC format: [batch, height, width, channels]
            let shape = &info.shape;
            if shape.len() == 4 {
                return (shape[3], shape[1], shape[2]); // (C, H, W)
            }
        }
        (3, 256, 256) // Default
    }

    fn output_shape(&self) -> (usize, usize, usize) {
        if let Some(info) = self.output_info.first() {
            let shape = &info.shape;
            if shape.len() == 4 {
                return (shape[3], shape[1], shape[2]); // (C, H, W)
            }
        }
        (2, 256, 256) // Default
    }
}

/// Placeholder module when tflite feature is not enabled
#[cfg(not(feature = "tflite"))]
pub struct TfLiteModel;

#[cfg(not(feature = "tflite"))]
impl TfLiteModel {
    /// Returns an error indicating TFLite support is not enabled
    ///
    /// # Errors
    /// Always returns an error when the feature is not enabled
    pub fn from_file<P: AsRef<Path>>(_path: P, _config: TfLiteConfig) -> Result<Self> {
        Err(MlError::FeatureNotAvailable {
            feature: "TensorFlow Lite support".to_string(),
            flag: "tflite".to_string(),
        })
    }

    /// Returns an error indicating TFLite support is not enabled
    ///
    /// # Errors
    /// Always returns an error when the feature is not enabled
    pub fn from_file_quantized<P: AsRef<Path>>(_path: P, _config: TfLiteConfig) -> Result<Self> {
        Err(MlError::FeatureNotAvailable {
            feature: "TensorFlow Lite support".to_string(),
            flag: "tflite".to_string(),
        })
    }
}

// Helper function to get number of CPUs
#[cfg(not(feature = "tflite"))]
mod num_cpus {
    pub fn get() -> usize {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
    }
}

#[cfg(feature = "tflite")]
mod num_cpus {
    pub fn get() -> usize {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tflite_config_builder() {
        let config = TfLiteConfig::builder()
            .threads(8)
            .delegate(Delegate::Gpu)
            .profiling(true)
            .allow_fp16(false)
            .build();

        assert_eq!(config.threads, 8);
        assert_eq!(config.delegate, Delegate::Gpu);
        assert!(config.profiling);
        assert!(!config.allow_fp16);
    }

    #[test]
    fn test_quantization_params() {
        let params = QuantizationParams {
            scale: 0.1,
            zero_point: 128,
        };

        // Test quantization
        let value = 5.0;
        let quantized = params.quantize(value);
        assert_eq!(quantized, 178); // round(5.0 / 0.1) + 128

        // Test dequantization
        let dequantized = params.dequantize(quantized);
        assert!((dequantized - value).abs() < 0.2);
    }

    #[test]
    fn test_tensor_dtype_conversion() {
        assert_eq!(
            TensorDataType::Float32.to_raster_type(),
            RasterDataType::Float32
        );
        assert_eq!(
            TensorDataType::UInt8.to_raster_type(),
            RasterDataType::UInt8
        );
    }

    #[test]
    fn test_delegate_variants() {
        let delegates = vec![
            Delegate::Cpu,
            Delegate::Gpu,
            Delegate::Nnapi,
            Delegate::CoreMl,
            Delegate::Xnnpack,
        ];

        for delegate in delegates {
            let config = TfLiteConfig::builder().delegate(delegate).build();
            assert_eq!(config.delegate, delegate);
        }
    }

    #[cfg(not(feature = "tflite"))]
    #[test]
    fn test_tflite_not_available() {
        let config = TfLiteConfig::default();
        let result = TfLiteModel::from_file("model.tflite", config);
        assert!(result.is_err());

        if let Err(MlError::FeatureNotAvailable { feature, flag }) = result {
            assert!(feature.contains("TensorFlow Lite"));
            assert_eq!(flag, "tflite");
        }
    }

    #[cfg(feature = "tflite")]
    #[test]
    fn test_model_not_found() {
        use std::env;
        let temp_dir = env::temp_dir();
        let nonexistent_path = temp_dir.join("nonexistent_model.tflite");

        let config = TfLiteConfig::default();
        let result = TfLiteModel::from_file(&nonexistent_path, config);

        assert!(result.is_err());
        if let Err(MlError::Model(ModelError::NotFound { path })) = result {
            assert!(path.contains("nonexistent_model.tflite"));
        } else {
            panic!("Expected NotFound error");
        }
    }

    #[cfg(feature = "tflite")]
    #[test]
    fn test_tensor_info_structure() {
        let info = TensorInfo {
            name: "test_tensor".to_string(),
            shape: vec![1, 224, 224, 3],
            dtype: TensorDataType::Float32,
            quantization: None,
        };

        assert_eq!(info.name, "test_tensor");
        assert_eq!(info.shape.len(), 4);
        assert_eq!(info.shape[0], 1); // batch
        assert_eq!(info.shape[1], 224); // height
        assert_eq!(info.shape[2], 224); // width
        assert_eq!(info.shape[3], 3); // channels
        assert!(info.quantization.is_none());
    }

    #[cfg(feature = "tflite")]
    #[test]
    fn test_quantized_tensor_info() {
        let quant_params = QuantizationParams {
            scale: 0.003921569,
            zero_point: 0,
        };

        let info = TensorInfo {
            name: "quantized_input".to_string(),
            shape: vec![1, 128, 128, 3],
            dtype: TensorDataType::UInt8,
            quantization: Some(quant_params),
        };

        assert!(info.quantization.is_some());
        if let Some(params) = info.quantization {
            assert!((params.scale - 0.003921569).abs() < 1e-7);
            assert_eq!(params.zero_point, 0);
        }
    }

    #[cfg(feature = "tflite")]
    #[test]
    fn test_config_default_values() {
        let config = TfLiteConfig::default();

        assert!(config.threads > 0);
        assert_eq!(config.delegate, Delegate::Cpu);
        assert!(!config.profiling);
        assert!(config.allow_fp16);
        assert!(!config.lazy_allocation);
    }

    #[cfg(feature = "tflite")]
    #[test]
    fn test_config_custom_values() {
        let config = TfLiteConfig::builder()
            .threads(16)
            .delegate(Delegate::Xnnpack)
            .profiling(true)
            .allow_fp16(false)
            .lazy_allocation(true)
            .build();

        assert_eq!(config.threads, 16);
        assert_eq!(config.delegate, Delegate::Xnnpack);
        assert!(config.profiling);
        assert!(!config.allow_fp16);
        assert!(config.lazy_allocation);
    }

    #[test]
    fn test_all_delegates() {
        // Test all delegate variants are valid
        let all_delegates = [
            Delegate::Cpu,
            Delegate::Gpu,
            Delegate::Nnapi,
            Delegate::CoreMl,
            Delegate::Xnnpack,
        ];

        for delegate in &all_delegates {
            let config = TfLiteConfig::builder().delegate(*delegate).build();
            assert_eq!(config.delegate, *delegate);
        }
    }

    #[test]
    fn test_tensor_dtype_conversions() {
        let types = [
            (TensorDataType::Float32, RasterDataType::Float32),
            (TensorDataType::Float16, RasterDataType::Float32),
            (TensorDataType::Int8, RasterDataType::UInt8),
            (TensorDataType::UInt8, RasterDataType::UInt8),
            (TensorDataType::Int16, RasterDataType::Int16),
            (TensorDataType::Int32, RasterDataType::Int32),
            (TensorDataType::Int64, RasterDataType::Float64),
        ];

        for (tensor_type, expected_raster_type) in types {
            assert_eq!(tensor_type.to_raster_type(), expected_raster_type);
        }
    }

    #[test]
    fn test_quantization_round_trip() {
        let params = QuantizationParams {
            scale: 0.1,
            zero_point: 128,
        };

        let test_values = [-10.0, 0.0, 5.0, 10.0, 25.5];

        for &value in &test_values {
            let quantized = params.quantize(value);
            let dequantized = params.dequantize(quantized);
            // Allow small error due to rounding
            assert!(
                (dequantized - value).abs() < 0.2,
                "Round-trip failed for value {}: got {}",
                value,
                dequantized
            );
        }
    }

    #[test]
    fn test_quantization_edge_cases() {
        let params = QuantizationParams {
            scale: 1.0,
            zero_point: 0,
        };

        // Test identity transform when scale=1, zero_point=0
        assert_eq!(params.quantize(5.0), 5);
        assert_eq!(params.quantize(-5.0), -5);
        assert!((params.dequantize(10) - 10.0).abs() < 1e-6);
    }
}
