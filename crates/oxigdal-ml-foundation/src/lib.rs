//! # OxiGDAL ML Foundation
//!
//! Deep learning training infrastructure and model architectures for geospatial machine learning.
//!
//! This crate provides:
//! - Training infrastructure (loops, optimizers, schedulers, losses)
//! - Transfer learning and fine-tuning capabilities
//! - Model architectures (UNet, ResNet, custom layers)
//! - Data augmentation pipelines for geospatial imagery
//! - Evaluation metrics and monitoring
//!
//! ## Features
//!
//! - `std` (default): Standard library support
//! - `pytorch`: PyTorch backend for training (requires libtorch)
//! - `onnx`: ONNX export support for trained models
//! - `cuda`: GPU acceleration (requires CUDA)
//!
//! ## Architecture
//!
//! The crate is organized into several modules:
//!
//! - [`training`]: Training loops, optimizers, schedulers, and losses
//! - [`transfer`]: Transfer learning and fine-tuning
//! - [`models`]: Neural network architectures
//! - [`augmentation`]: Data augmentation pipelines
//! - [`data`]: Data pipeline with dataset loaders and batching
//! - [`metrics`]: Evaluation metrics
//!
//! ## COOLJAPAN Compliance
//!
//! - Pure Rust implementation (PyTorch bindings are feature-gated)
//! - No unwrap() calls in production code
//! - All files under 2000 lines
//! - Uses workspace dependencies
//! - Uses SciRS2-Core for numerical operations (Pure Rust Policy)
//!
//! ## Example
//!
//! ```rust,no_run
//! use oxigdal_ml_foundation::{
//!     training::TrainingConfig,
//!     training::training_loop::Trainer,
//!     models::unet::UNet,
//!     metrics::Metrics,
//! };
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Create a UNet model for segmentation (in_channels=3, num_classes=10, depth=4)
//! let model = UNet::new(3, 10, 4)?;
//!
//! // Configure training
//! let config = TrainingConfig {
//!     learning_rate: 0.001,
//!     batch_size: 16,
//!     num_epochs: 100,
//!     ..Default::default()
//! };
//!
//! // Train the model (requires PyTorch feature)
//! // let trainer = Trainer::new(model, config)?;
//! // let trained_model = trainer.train(train_data, val_data)?;
//! # Ok(())
//! # }
//! ```

#![cfg_attr(not(feature = "std"), no_std)]
#![warn(missing_docs)]
#![warn(clippy::unwrap_used)]
#![warn(clippy::panic)]

#[cfg(not(feature = "std"))]
extern crate alloc;

// Re-export key types
pub use error::{Error, Result};

// Core modules
pub mod error;
pub mod metrics;

// Backend abstraction for ML operations
#[cfg(feature = "ml")]
pub mod backend;

// Main functionality modules
pub mod augmentation;
pub mod data;
pub mod models;
pub mod training;
pub mod transfer;

/// Library version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Library name
pub const NAME: &str = env!("CARGO_PKG_NAME");

/// Checks if PyTorch backend is available
pub fn has_pytorch_backend() -> bool {
    cfg!(feature = "ml")
}

/// Checks if GPU support is available
pub fn has_gpu_support() -> bool {
    cfg!(feature = "gpu")
}

/// Checks if ONNX export is available
pub fn has_onnx_export() -> bool {
    cfg!(feature = "onnx")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
        assert_eq!(NAME, "oxigdal-ml-foundation");
    }

    #[test]
    fn test_feature_detection() {
        // These should return false in default test configuration
        // (unless features are explicitly enabled)
        let _ = has_pytorch_backend();
        let _ = has_gpu_support();
        let _ = has_onnx_export();
    }
}
