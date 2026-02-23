//! Error types for ML foundation operations.

use thiserror::Error;

/// Result type for ML foundation operations.
pub type Result<T> = core::result::Result<T, Error>;

/// Error types for ML foundation operations.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// Invalid input dimensions
    #[error("Invalid input dimensions: expected {expected}, got {actual}")]
    InvalidDimensions {
        /// Expected dimension description
        expected: String,
        /// Actual dimension found
        actual: String,
    },

    /// Invalid parameter value
    #[error("Invalid parameter: {name} = {value}, reason: {reason}")]
    InvalidParameter {
        /// Parameter name
        name: String,
        /// Parameter value
        value: String,
        /// Reason for invalidity
        reason: String,
    },

    /// Model architecture error
    #[error("Model architecture error: {0}")]
    ModelArchitecture(String),

    /// Training error
    #[error("Training error: {0}")]
    Training(String),

    /// Optimizer error
    #[error("Optimizer error: {0}")]
    Optimizer(String),

    /// Loss function error
    #[error("Loss function error: {0}")]
    LossFunction(String),

    /// Data augmentation error
    #[error("Data augmentation error: {0}")]
    Augmentation(String),

    /// Checkpoint I/O error
    #[error("Checkpoint I/O error: {0}")]
    Checkpoint(String),

    /// Transfer learning error
    #[error("Transfer learning error: {0}")]
    TransferLearning(String),

    /// Metric computation error
    #[error("Metric computation error: {0}")]
    Metric(String),

    /// Feature not available
    #[error(
        "Feature not available: {feature}. Enable the '{cargo_feature}' feature to use this functionality"
    )]
    FeatureNotAvailable {
        /// Feature name that is not available
        feature: String,
        /// Cargo feature required to enable this functionality
        cargo_feature: String,
    },

    /// OxiGDAL core error
    #[error("OxiGDAL core error: {0}")]
    Core(#[from] oxigdal_core::error::OxiGdalError),

    /// Image processing error
    #[error("Image processing error: {0}")]
    Image(String),

    /// Serialization/deserialization error
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Numerical error (overflow, underflow, NaN, etc.)
    #[error("Numerical error: {0}")]
    Numerical(String),

    /// Early stopping triggered
    #[error("Early stopping triggered: {reason}")]
    EarlyStopping {
        /// Reason for early stopping
        reason: String,
    },

    /// Invalid model state
    #[error("Invalid model state: {0}")]
    InvalidState(String),

    /// Backend error
    #[error("Backend error: {0}")]
    Backend(String),

    /// Not implemented
    #[error("Not implemented: {0}")]
    NotImplemented(String),
}

impl Error {
    /// Creates an invalid dimensions error.
    pub fn invalid_dimensions(expected: impl Into<String>, actual: impl Into<String>) -> Self {
        Self::InvalidDimensions {
            expected: expected.into(),
            actual: actual.into(),
        }
    }

    /// Creates an invalid parameter error.
    pub fn invalid_parameter(
        name: impl Into<String>,
        value: impl std::fmt::Display,
        reason: impl Into<String>,
    ) -> Self {
        Self::InvalidParameter {
            name: name.into(),
            value: value.to_string(),
            reason: reason.into(),
        }
    }

    /// Creates a feature not available error.
    pub fn feature_not_available(
        feature: impl Into<String>,
        cargo_feature: impl Into<String>,
    ) -> Self {
        Self::FeatureNotAvailable {
            feature: feature.into(),
            cargo_feature: cargo_feature.into(),
        }
    }

    /// Creates a numerical error.
    pub fn numerical(msg: impl Into<String>) -> Self {
        Self::Numerical(msg.into())
    }

    /// Creates an early stopping error.
    pub fn early_stopping(reason: impl Into<String>) -> Self {
        Self::EarlyStopping {
            reason: reason.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_messages() {
        let err = Error::invalid_dimensions("[32, 3, 224, 224]", "[32, 3, 256, 256]");
        assert!(err.to_string().contains("expected"));

        let err = Error::invalid_parameter("learning_rate", 0.0, "must be positive");
        assert!(err.to_string().contains("learning_rate"));

        let err = Error::feature_not_available("PyTorch backend", "pytorch");
        assert!(err.to_string().contains("pytorch"));
    }
}
