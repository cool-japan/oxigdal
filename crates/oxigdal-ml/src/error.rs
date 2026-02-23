//! Error types for OxiGDAL ML operations
//!
//! This module provides ML-specific error types that integrate with
//! the core OxiGDAL error hierarchy.

use thiserror::Error;

/// The main result type for ML operations
pub type Result<T> = core::result::Result<T, MlError>;

/// ML-specific errors
#[derive(Debug, Error)]
pub enum MlError {
    /// Model loading error
    #[error("Model error: {0}")]
    Model(#[from] ModelError),

    /// Inference error
    #[error("Inference error: {0}")]
    Inference(#[from] InferenceError),

    /// Preprocessing error
    #[error("Preprocessing error: {0}")]
    Preprocessing(#[from] PreprocessingError),

    /// Postprocessing error
    #[error("Postprocessing error: {0}")]
    Postprocessing(#[from] PostprocessingError),

    /// OxiGDAL core error
    #[error("OxiGDAL error: {0}")]
    OxiGdal(#[from] oxigdal_core::OxiGdalError),

    /// ONNX Runtime error
    #[error("ONNX Runtime error: {0}")]
    Ort(String),

    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Invalid configuration
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    /// Feature not available
    #[error("Feature not available: {feature}. Enable with feature flag: {flag}")]
    FeatureNotAvailable {
        /// The feature name
        feature: String,
        /// The required feature flag
        flag: String,
    },
}

/// Model-related errors
#[derive(Debug, Error)]
pub enum ModelError {
    /// Model file not found
    #[error("Model file not found: {path}")]
    NotFound {
        /// The model file path
        path: String,
    },

    /// Model loading failed
    #[error("Failed to load model: {reason}")]
    LoadFailed {
        /// The reason for failure
        reason: String,
    },

    /// Invalid model format
    #[error("Invalid model format: {message}")]
    InvalidFormat {
        /// Error message
        message: String,
    },

    /// Model initialization failed
    #[error("Model initialization failed: {reason}")]
    InitializationFailed {
        /// The reason for failure
        reason: String,
    },

    /// Incompatible model version
    #[error("Incompatible model version: expected {expected}, got {actual}")]
    IncompatibleVersion {
        /// Expected version
        expected: String,
        /// Actual version
        actual: String,
    },

    /// Missing required input
    #[error("Missing required model input: {input_name}")]
    MissingInput {
        /// Input name
        input_name: String,
    },

    /// Missing required output
    #[error("Missing required model output: {output_name}")]
    MissingOutput {
        /// Output name
        output_name: String,
    },
}

/// Inference-related errors
#[derive(Debug, Error)]
pub enum InferenceError {
    /// Invalid input shape
    #[error("Invalid input shape: expected {expected:?}, got {actual:?}")]
    InvalidInputShape {
        /// Expected shape
        expected: Vec<usize>,
        /// Actual shape
        actual: Vec<usize>,
    },

    /// Invalid input type
    #[error("Invalid input type: expected {expected}, got {actual}")]
    InvalidInputType {
        /// Expected type
        expected: String,
        /// Actual type
        actual: String,
    },

    /// Batch size mismatch
    #[error("Batch size mismatch: expected {expected}, got {actual}")]
    BatchSizeMismatch {
        /// Expected batch size
        expected: usize,
        /// Actual batch size
        actual: usize,
    },

    /// Inference failed
    #[error("Inference failed: {reason}")]
    Failed {
        /// The reason for failure
        reason: String,
    },

    /// Output parsing failed
    #[error("Failed to parse output: {reason}")]
    OutputParsingFailed {
        /// The reason for failure
        reason: String,
    },

    /// GPU not available
    #[error("GPU acceleration requested but not available: {message}")]
    GpuNotAvailable {
        /// Error message
        message: String,
    },
}

/// Preprocessing-related errors
#[derive(Debug, Error)]
pub enum PreprocessingError {
    /// Invalid normalization parameters
    #[error("Invalid normalization parameters: {message}")]
    InvalidNormalization {
        /// Error message
        message: String,
    },

    /// Tiling failed
    #[error("Tiling failed: {reason}")]
    TilingFailed {
        /// The reason for failure
        reason: String,
    },

    /// Padding failed
    #[error("Padding failed: {reason}")]
    PaddingFailed {
        /// The reason for failure
        reason: String,
    },

    /// Invalid tile size
    #[error("Invalid tile size: width={width}, height={height}")]
    InvalidTileSize {
        /// Tile width
        width: usize,
        /// Tile height
        height: usize,
    },

    /// Channel mismatch
    #[error("Channel mismatch: expected {expected}, got {actual}")]
    ChannelMismatch {
        /// Expected channels
        expected: usize,
        /// Actual channels
        actual: usize,
    },

    /// Augmentation failed
    #[error("Data augmentation failed: {reason}")]
    AugmentationFailed {
        /// The reason for failure
        reason: String,
    },
}

/// Postprocessing-related errors
#[derive(Debug, Error)]
pub enum PostprocessingError {
    /// Tile merging failed
    #[error("Tile merging failed: {reason}")]
    MergingFailed {
        /// The reason for failure
        reason: String,
    },

    /// Threshold out of range
    #[error("Threshold out of range: must be between 0.0 and 1.0, got {value}")]
    InvalidThreshold {
        /// The invalid threshold value
        value: f32,
    },

    /// Polygon conversion failed
    #[error("Polygon conversion failed: {reason}")]
    PolygonConversionFailed {
        /// The reason for failure
        reason: String,
    },

    /// NMS failed
    #[error("Non-maximum suppression failed: {reason}")]
    NmsFailed {
        /// The reason for failure
        reason: String,
    },

    /// Export failed
    #[error("Export failed: {reason}")]
    ExportFailed {
        /// The reason for failure
        reason: String,
    },

    /// Invalid class ID
    #[error("Invalid class ID: {class_id}")]
    InvalidClassId {
        /// The invalid class ID
        class_id: usize,
    },
}

impl From<ort::Error> for MlError {
    fn from(err: ort::Error) -> Self {
        MlError::Ort(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = ModelError::NotFound {
            path: "/path/to/model.onnx".to_string(),
        };
        assert!(err.to_string().contains("Model file not found"));
        assert!(err.to_string().contains("/path/to/model.onnx"));
    }

    #[test]
    fn test_error_conversion() {
        let model_err = ModelError::LoadFailed {
            reason: "test".to_string(),
        };
        let ml_err: MlError = model_err.into();
        assert!(matches!(ml_err, MlError::Model(_)));
    }

    #[test]
    fn test_invalid_threshold() {
        let err = PostprocessingError::InvalidThreshold { value: 1.5 };
        assert!(err.to_string().contains("1.5"));
    }
}
