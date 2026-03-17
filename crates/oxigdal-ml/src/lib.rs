//! OxiGDAL ML - Machine Learning for Geospatial Data
//!
//! This crate provides machine learning capabilities for the OxiGDAL ecosystem,
//! enabling geospatial ML workflows with ONNX Runtime integration.
//!
//! # Features
//!
//! - **ONNX Runtime Integration**: Pure Rust interface to ONNX Runtime for model inference
//! - **Image Segmentation**: Semantic, instance, and panoptic segmentation
//! - **Image Classification**: Scene classification, land cover classification, multi-label
//! - **Object Detection**: Bounding box detection with NMS and georeferencing
//! - **Preprocessing**: Normalization, tiling, padding, and augmentation
//! - **Postprocessing**: Tile merging, thresholding, polygon conversion, GeoJSON export
//!
//! # Optional Features
//!
//! - `gpu` - Enable CUDA and TensorRT GPU acceleration
//! - `directml` - Enable DirectML support (Windows)
//! - `coreml` - Enable CoreML support (macOS/iOS)
//!
//! # Example: Image Segmentation
//!
//! ```ignore
//! use oxigdal_ml::*;
//! use oxigdal_ml::models::OnnxModel;
//! use oxigdal_ml::inference::{InferenceEngine, InferenceConfig};
//! use oxigdal_ml::segmentation::probability_to_mask;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Load ONNX model
//! let model = OnnxModel::from_file("segmentation.onnx")?;
//!
//! // Create inference engine
//! let config = InferenceConfig::default();
//! let engine = InferenceEngine::new(model, config);
//!
//! // Load input raster (not shown)
//! # use oxigdal_core::buffer::RasterBuffer;
//! # use oxigdal_core::types::RasterDataType;
//! # let input = RasterBuffer::zeros(256, 256, RasterDataType::Float32);
//!
//! // Run inference
//! let predictions = engine.predict(&input)?;
//!
//! // Convert to segmentation mask
//! let mask = probability_to_mask(&predictions, 2, 0.5)?;
//! # Ok(())
//! # }
//! ```
//!
//! # Example: Object Detection
//!
//! ```ignore
//! use oxigdal_ml::*;
//! use oxigdal_ml::detection::{non_maximum_suppression, NmsConfig};
//! use oxigdal_ml::postprocessing::export_detections_geojson;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Assume we have detections from a model
//! # let detections = vec![];
//!
//! // Apply NMS to filter overlapping detections
//! let config = NmsConfig::default();
//! let filtered = non_maximum_suppression(&detections, &config)?;
//!
//! // Export to GeoJSON
//! # let geo_detections = vec![];
//! # export_detections_geojson(&geo_detections, "detections.geojson")?;
//! # Ok(())
//! # }
//! ```
//!
//! # Example: Batch Processing with Progress Tracking
//!
//! ```ignore
//! use oxigdal_ml::*;
//! use oxigdal_ml::batch::{BatchConfig, BatchProcessor};
//! use oxigdal_ml::models::OnnxModel;
//! # use oxigdal_core::buffer::RasterBuffer;
//! # use oxigdal_core::types::RasterDataType;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Load model
//! let model = OnnxModel::from_file("model.onnx")?;
//!
//! // Auto-tune batch size based on available memory
//! let sample_size = 3 * 256 * 256 * 4; // 3 channels, 256x256, float32
//! let batch_size = BatchConfig::auto_tune_batch_size(sample_size, 0.5);
//!
//! // Create batch processor
//! let config = BatchConfig::builder()
//!     .max_batch_size(batch_size)
//!     .parallel_batches(4)
//!     .build();
//!
//! let processor = BatchProcessor::new(model, config);
//!
//! // Process large batch with progress bar
//! # let inputs: Vec<RasterBuffer> = vec![];
//! let results = processor.infer_batch_with_progress(inputs, true)?;
//! # Ok(())
//! # }
//! ```
//!
//! # Example: Model Optimization Pipeline
//!
//! ```ignore
//! use oxigdal_ml::*;
//! use oxigdal_ml::optimization::{OptimizationPipeline, OptimizationProfile};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Create optimization pipeline for edge deployment
//! let pipeline = OptimizationPipeline::from_profile(OptimizationProfile::Size);
//!
//! // Optimize model (applies quantization + pruning)
//! let stats = pipeline.optimize("model.onnx", "model_optimized.onnx")?;
//!
//! println!("Size reduction: {:.1}%", stats.size_reduction_percent());
//! println!("Speedup: {:.2}x", stats.speedup);
//! println!("Accuracy delta: {:.2}%", stats.accuracy_delta);
//! # Ok(())
//! # }
//! ```
//!
//! # Example: Model Zoo - Download Pretrained Models
//!
//! ```ignore
//! use oxigdal_ml::*;
//! use oxigdal_ml::zoo::ModelZoo;
//! use oxigdal_ml::models::OnnxModel;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Create model zoo
//! let mut zoo = ModelZoo::new()?;
//!
//! // List available models
//! let models = zoo.list_models();
//! for model in models {
//!     println!("{}: {} - {:.1}% accuracy",
//!         model.name, model.description,
//!         model.accuracy.unwrap_or(0.0));
//! }
//!
//! // Download a model (with progress bar and checksum verification)
//! let model_path = zoo.get_model("unet_buildings")?;
//!
//! // Load and use the model
//! let model = OnnxModel::from_file(model_path)?;
//! # Ok(())
//! # }
//! ```
//!
//! # Example: Transfer Learning
//!
//! ```no_run
//! use oxigdal_ml::*;
//! # #[cfg(feature = "temporal")]
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // This example requires oxigdal-ml-foundation
//! // Load pretrained feature extractor
//! use oxigdal_ml_foundation::transfer::FeatureExtractor;
//!
//! // Extract features from pretrained model
//! let extractor = FeatureExtractor::from_pretrained("resnet50")?;
//!
//! // Use features for custom classification task
//! // (Training loop implementation would go here)
//! # Ok(())
//! # }
//! ```
//!
//! # Example: GPU Acceleration
//!
//! ```ignore
//! use oxigdal_ml::*;
//! use oxigdal_ml::gpu::{GpuBackend, GpuConfig};
//! use oxigdal_ml::inference::{InferenceEngine, InferenceConfig};
//! use oxigdal_ml::models::OnnxModel;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Detect available GPU backends
//! let backends = GpuBackend::detect_all();
//! for backend in &backends {
//!     println!("Available: {:?}", backend);
//! }
//!
//! // Configure GPU acceleration
//! let gpu_config = GpuConfig::builder()
//!     .preferred_backend(GpuBackend::Cuda)
//!     .device_id(0)
//!     .build();
//!
//! // Create inference engine with GPU support
//! let model = OnnxModel::from_file("model.onnx")?;
//! let mut inference_config = InferenceConfig::default();
//! inference_config.gpu_config = Some(gpu_config);
//!
//! let engine = InferenceEngine::new(model, inference_config);
//! # Ok(())
//! # }
//! ```
//!
//! # Example: Model Monitoring
//!
//! ```ignore
//! use oxigdal_ml::*;
//! use oxigdal_ml::monitoring::{ModelMonitor, MonitoringConfig};
//! use oxigdal_ml::models::OnnxModel;
//! # use oxigdal_core::buffer::RasterBuffer;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let model = OnnxModel::from_file("model.onnx")?;
//!
//! // Create monitor with custom config
//! let config = MonitoringConfig::builder()
//!     .track_latency(true)
//!     .track_memory(true)
//!     .alert_threshold_ms(100.0)
//!     .build();
//!
//! let mut monitor = ModelMonitor::new(model, config);
//!
//! // Run inference with monitoring
//! # let input = RasterBuffer::zeros(256, 256, oxigdal_core::types::RasterDataType::Float32);
//! let result = monitor.predict(&input)?;
//!
//! // Check metrics
//! let metrics = monitor.metrics();
//! println!("Avg latency: {:.2}ms", metrics.avg_latency_ms());
//! println!("Memory usage: {:.1}MB", metrics.peak_memory_mb);
//! # Ok(())
//! # }
//! ```
//!
//! # SciRS2 Integration Status
//!
//! This crate is being migrated to use the Pure Rust SciRS2 ecosystem following
//! COOLJAPAN policy. Current status:
//!
//! - **Random Number Generation**: Completed - using `scirs2-core::random` for RNG
//! - **Statistical Distributions**: Completed - using `NormalDistribution` for Gaussian noise
//! - **Data Augmentation**: Completed - integrated SciRS2-Core RNG
//! - **Linear Algebra**: Partial - `ndarray` still used in some modules, migration ongoing
//! - **Neural Network Training**: In Progress - `oxigdal-ml-foundation` implements custom
//!   Pure Rust backend with SciRS2 integration
//!
//! ## Usage Example with SciRS2
//!
//! ```no_run
//! use oxigdal_ml::augmentation::{add_gaussian_noise, random_crop};
//! # use oxigdal_core::buffer::RasterBuffer;
//! # use oxigdal_core::types::RasterDataType;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Create a raster buffer
//! let input = RasterBuffer::zeros(512, 512, RasterDataType::Float32);
//!
//! // Add Gaussian noise using SciRS2-Core
//! let noisy = add_gaussian_noise(&input, 0.01)?;
//!
//! // Random crop using SciRS2-Core RNG
//! let cropped = random_crop(&input, 256, 256)?;
//! # Ok(())
//! # }
//! ```
//!
//! For more information about SciRS2 integration, see the SCIRS2 POLICY in the
//! workspace documentation.

#![warn(missing_docs)]
#![warn(clippy::all)]
// Pedantic disabled to reduce noise - default clippy::all is sufficient
// #![warn(clippy::pedantic)]
#![deny(clippy::unwrap_used)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
// Allow unused imports in feature-gated modules
#![allow(unused_imports)]
// Allow dead code for ML model structures
#![allow(dead_code)]
// Allow collapsible match for ML result handling
#![allow(clippy::collapsible_match)]
#![allow(clippy::collapsible_if)]
// Allow expect() for model loading invariants
#![allow(clippy::expect_used)]
// Allow for loop with idx for tensor operations
#![allow(clippy::needless_range_loop)]
// Allow manual div_ceil for batch calculations
#![allow(clippy::manual_div_ceil)]
// Allow field assignment outside initializer for ML configs
#![allow(clippy::field_reassign_with_default)]
// Allow unexpected cfg for temporarily disabled features
#![allow(unexpected_cfgs)]

pub mod augmentation;
pub mod batch;
pub mod batch_predict;
pub mod classification;
#[cfg(feature = "cloud-removal")]
pub mod cloud;
pub mod detection;
pub mod error;
pub mod gpu;
pub mod hot_reload;
pub mod inference;
pub mod inference_cache;
pub mod model_versioning;
pub mod models;
pub mod monitoring;
pub mod optimization;
pub mod postprocessing;
pub mod preprocessing;
pub mod segmentation;
pub mod serving;
pub mod superres;
#[cfg(feature = "temporal")]
pub mod temporal;
pub mod zoo;

// Re-export commonly used items
pub use augmentation::AugmentationConfig;
pub use batch::{BatchConfig, BatchProcessor};
pub use error::{MlError, Result};
pub use gpu::{GpuBackend, GpuConfig, GpuDevice};
pub use models::{Model, OnnxModel};
pub use monitoring::{ModelMonitor, MonitoringConfig, PerformanceMetrics};
pub use optimization::{OptimizationPipeline, OptimizationProfile};
pub use serving::{DeploymentStrategy, ModelServer, ServerConfig};
pub use superres::{SuperResConfig, SuperResolution, UpscaleFactor};
#[cfg(feature = "temporal")]
pub use temporal::{ForecastConfig, ForecastResult, TemporalForecaster};
pub use zoo::ModelZoo;

/// Crate version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Crate name
pub const NAME: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
        assert_eq!(NAME, "oxigdal-ml");
    }
}
