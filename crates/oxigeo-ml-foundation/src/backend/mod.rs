//! # ML Backend Abstraction
//!
//! This module provides a backend-agnostic interface for machine learning operations.
//! It allows switching between different tensor computation backends (scirs2, future alternatives)
//! while keeping the high-level API consistent.
//!
//! ## Design
//!
//! The backend abstraction separates model configuration (which can be serialized/deserialized)
//! from actual tensor operations (which require a specific backend). This enables:
//!
//! - Pure Rust training with scirs2
//! - ONNX export for inference
//! - Future backend alternatives without API breakage
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────┐
//! │ Model Configs   │  (UNetConfig, ResNetConfig, etc.)
//! │ (Serializable)  │
//! └────────┬────────┘
//!          │
//!          ▼
//! ┌─────────────────┐
//! │ MLBackend Trait │  (Backend abstraction)
//! └────────┬────────┘
//!          │
//!     ┌────┴────┐
//!     │         │
//!     ▼         ▼
//! ┌──────┐  ┌──────┐
//! │scirs2│  │future│
//! └──────┘  └──────┘
//! ```

use crate::error::Result;

// Re-enabled with scirs2-neural integration
#[cfg(feature = "ml")]
pub mod layers;

// Concrete trainable backend built from real scirs2-neural leaf layers.
#[cfg(feature = "ml")]
pub mod neural_backend;

#[cfg(feature = "ml")]
pub use neural_backend::NeuralBackend;

// Pure-Rust ONNX model export (protobuf writer) for UNet / ResNet architectures.
#[cfg(feature = "onnx")]
pub mod onnx_export;

// NOTE: the former `scirs2_backend` (scirs2-neural forward-only) and
// `autograd_backend` (scirs2-autograd trainable) modules were removed: they had
// drifted irrecoverably against the current scirs2 APIs. They are superseded by
// [`neural_backend::NeuralBackend`], which composes scirs2-neural leaf layers
// (whose `backward`/`update` implement real gradient computation) into a module
// tree with explicit gradient routing, so the full [`crate::training`] machinery
// can actually train a model. [`onnx_export`] serializes a model architecture
// from its config; [`layers`] provides forward-pass building blocks.

/// Trait for ML backend implementations
///
/// This trait defines the core operations that any ML backend must support.
/// Implementations handle actual tensor computations, forward/backward passes,
/// and parameter updates.
#[cfg(feature = "ml")]
pub trait MLBackend: Send + Sync {
    /// Forward pass through the network
    ///
    /// # Arguments
    ///
    /// * `input` - Input tensor with shape (batch, channels, height, width)
    ///
    /// # Returns
    ///
    /// Output tensor after forward pass
    fn forward(&self, input: &[f32], input_shape: &[usize]) -> Result<Vec<f32>>;

    /// Backward pass for gradient computation
    ///
    /// # Arguments
    ///
    /// * `grad_output` - Gradient with respect to output
    ///
    /// # Returns
    ///
    /// Gradient with respect to input
    fn backward(&self, grad_output: &[f32], grad_shape: &[usize]) -> Result<Vec<f32>>;

    /// Update parameters using optimizer
    ///
    /// # Arguments
    ///
    /// * `learning_rate` - Learning rate for parameter update
    fn optimizer_step(&mut self, learning_rate: f32) -> Result<()>;

    /// Number of independently addressable top-level layers, i.e. the
    /// granularity at which [`Self::optimizer_step_layerwise`] applies distinct
    /// learning rates. Backends without layer-wise support report `1`.
    fn num_layers(&self) -> usize {
        1
    }

    /// Update parameters using a distinct learning rate per top-level layer.
    ///
    /// `layer_lrs` must have exactly [`Self::num_layers`] entries. A learning
    /// rate of `0.0` freezes the corresponding layer for this step (its
    /// parameters are left unchanged), which is how transfer-learning layer
    /// freezing / gradual unfreezing is applied to a real model.
    ///
    /// The default implementation returns an error: a backend must opt in to
    /// layer-wise updates rather than silently collapsing them to a uniform
    /// step.
    fn optimizer_step_layerwise(&mut self, layer_lrs: &[f32]) -> Result<()> {
        let _ = layer_lrs;
        Err(crate::Error::NotImplemented(
            "this backend does not support layer-wise optimizer steps".to_string(),
        ))
    }

    /// Zero out all gradients
    fn zero_grad(&mut self) -> Result<()>;

    /// Get the number of trainable parameters
    fn num_parameters(&self) -> usize;

    /// Get current loss value (if available from last forward pass)
    fn last_loss(&self) -> Option<f32>;

    /// Set training mode (enables dropout, batch norm training, etc.)
    fn set_train_mode(&mut self, train: bool);

    /// Save model weights to binary format
    fn save_weights(&self, path: &std::path::Path) -> Result<()>;

    /// Load model weights from binary format
    fn load_weights(&mut self, path: &std::path::Path) -> Result<()>;

    /// Export to ONNX format (if supported)
    #[cfg(feature = "onnx")]
    fn export_onnx(&self, path: &std::path::Path) -> Result<()>;
}

/// Backend configuration
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BackendConfig {
    /// Device to use for computation ("cpu", "cuda:0", etc.)
    pub device: String,

    /// Enable mixed precision training (FP16)
    pub mixed_precision: bool,

    /// Enable gradient checkpointing to save memory
    pub gradient_checkpointing: bool,

    /// Number of threads for CPU backend
    pub num_threads: Option<usize>,
}

impl Default for BackendConfig {
    fn default() -> Self {
        Self {
            device: "cpu".to_string(),
            mixed_precision: false,
            gradient_checkpointing: false,
            num_threads: None,
        }
    }
}

/// Backend factory for creating backend instances from model configs
#[cfg(feature = "ml")]
pub struct BackendFactory;

#[cfg(feature = "ml")]
impl BackendFactory {
    /// Create scirs2 backend from UNet configuration
    ///
    /// # Arguments
    ///
    /// * `config` - UNet model configuration
    /// * `backend_config` - Backend-specific configuration
    ///
    /// # Returns
    ///
    /// Initialized scirs2 backend ready for training
    pub fn create_unet(
        config: &crate::models::unet::UNetConfig,
        backend_config: &BackendConfig,
    ) -> Result<Box<dyn MLBackend>> {
        let backend = NeuralBackend::unet(config, backend_config)?;
        Ok(Box::new(backend))
    }

    /// Create scirs2 backend from ResNet configuration
    ///
    /// # Arguments
    ///
    /// * `config` - ResNet model configuration
    /// * `backend_config` - Backend-specific configuration
    ///
    /// # Returns
    ///
    /// Initialized scirs2 backend ready for training
    pub fn create_resnet(
        config: &crate::models::resnet::ResNetConfig,
        backend_config: &BackendConfig,
    ) -> Result<Box<dyn MLBackend>> {
        let backend = NeuralBackend::resnet(config, backend_config)?;
        Ok(Box::new(backend))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_config_default() {
        let config = BackendConfig::default();
        assert_eq!(config.device, "cpu");
        assert!(!config.mixed_precision);
        assert!(!config.gradient_checkpointing);
        assert!(config.num_threads.is_none());
    }

    #[test]
    fn test_backend_config_serialization() {
        let config = BackendConfig {
            device: "cuda:0".to_string(),
            mixed_precision: true,
            gradient_checkpointing: true,
            num_threads: Some(8),
        };

        let json = serde_json::to_string(&config).expect("serialization failed");
        let deserialized: BackendConfig =
            serde_json::from_str(&json).expect("deserialization failed");

        assert_eq!(deserialized.device, config.device);
        assert_eq!(deserialized.mixed_precision, config.mixed_precision);
        assert_eq!(
            deserialized.gradient_checkpointing,
            config.gradient_checkpointing
        );
        assert_eq!(deserialized.num_threads, config.num_threads);
    }
}
