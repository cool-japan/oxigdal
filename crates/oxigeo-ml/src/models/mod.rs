//! Model management for OxiGeo ML
//!
//! This module provides model loading, management, and configuration.

pub mod onnx;
pub mod tflite;

pub use onnx::{ExecutionProvider, ModelMetadata, OnnxModel, SessionConfig};

#[cfg(not(feature = "tflite"))]
pub use tflite::{Delegate, QuantizationParams, TensorDataType, TensorInfo, TfLiteConfig};

#[cfg(feature = "tflite")]
pub use tflite::{
    Delegate, QuantizationParams, TensorDataType, TensorInfo, TfLiteConfig, TfLiteModel,
};

use crate::error::{InferenceError, Result};
use oxigeo_core::buffer::{MultiBandBuffer, RasterBuffer};

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

    /// Predicts on a multi-band buffer, feeding a real `[1, C, H, W]` tensor to
    /// the model (one channel per band).
    ///
    /// The default implementation fails loud with an
    /// [`InferenceError::Failed`]: implementors that support multi-channel input
    /// (e.g. [`OnnxModel`]) override this. This keeps existing single-band model
    /// implementations source-compatible.
    ///
    /// # Errors
    /// Returns an error if the model does not support multi-band input or if
    /// prediction fails.
    fn predict_multiband(&mut self, input: &MultiBandBuffer) -> Result<MultiBandBuffer> {
        let _ = input;
        Err(InferenceError::Failed {
            reason: "Multi-band prediction is not supported by this model".to_string(),
        }
        .into())
    }

    /// Predicts on multiple multi-band buffers (batch prediction).
    ///
    /// The default implementation fails loud with an
    /// [`InferenceError::Failed`]; implementors that support multi-channel input
    /// override this.
    ///
    /// # Errors
    /// Returns an error if the model does not support multi-band input or if
    /// prediction fails.
    fn predict_batch_multiband(
        &mut self,
        inputs: &[MultiBandBuffer],
    ) -> Result<Vec<MultiBandBuffer>> {
        let _ = inputs;
        Err(InferenceError::Failed {
            reason: "Multi-band batch prediction is not supported by this model".to_string(),
        }
        .into())
    }

    /// Returns the expected input shape (channels, height, width)
    fn input_shape(&self) -> (usize, usize, usize);

    /// Returns the expected output shape (channels, height, width)
    fn output_shape(&self) -> (usize, usize, usize);
}
