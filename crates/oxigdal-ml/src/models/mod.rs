//! Model management for OxiGDAL ML
//!
//! This module provides model loading, management, and configuration.

pub mod onnx;
pub mod tflite;

#[cfg(feature = "coreml")]
pub mod coreml;

pub use onnx::{ExecutionProvider, ModelMetadata, OnnxModel, SessionConfig};

#[cfg(not(feature = "tflite"))]
pub use tflite::{Delegate, QuantizationParams, TensorDataType, TensorInfo, TfLiteConfig};

#[cfg(feature = "tflite")]
pub use tflite::{
    Delegate, QuantizationParams, TensorDataType, TensorInfo, TfLiteConfig, TfLiteModel,
};

use crate::error::Result;
use oxigdal_core::buffer::RasterBuffer;

/// Trait for ML models
pub trait Model: Send + Sync {
    /// Returns the model metadata
    fn metadata(&self) -> &ModelMetadata;

    /// Predicts on a single raster buffer
    ///
    /// # Errors
    /// Returns an error if prediction fails
    fn predict(&mut self, input: &RasterBuffer) -> Result<RasterBuffer>;

    /// Predicts on multiple raster buffers (batch prediction)
    ///
    /// # Errors
    /// Returns an error if prediction fails
    fn predict_batch(&mut self, inputs: &[RasterBuffer]) -> Result<Vec<RasterBuffer>>;

    /// Returns the expected input shape (channels, height, width)
    fn input_shape(&self) -> (usize, usize, usize);

    /// Returns the expected output shape (channels, height, width)
    fn output_shape(&self) -> (usize, usize, usize);
}
