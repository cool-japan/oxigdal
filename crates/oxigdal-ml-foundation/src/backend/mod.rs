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

// Temporarily disabled until full backend implementation
// #[cfg(feature = "ml")]
// pub mod scirs2_backend;

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
        _config: &crate::models::unet::UNetConfig,
        _backend_config: &BackendConfig,
    ) -> Result<Box<dyn MLBackend>> {
        Err(crate::Error::NotImplemented(
            "UNet backend awaiting scirs2 API stabilization".to_string(),
        ))
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
        _config: &crate::models::resnet::ResNetConfig,
        _backend_config: &BackendConfig,
    ) -> Result<Box<dyn MLBackend>> {
        Err(crate::Error::NotImplemented(
            "ResNet backend awaiting scirs2 API stabilization".to_string(),
        ))
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
