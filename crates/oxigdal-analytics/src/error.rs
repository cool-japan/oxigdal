//! Error types for OxiGDAL Analytics
//!
//! This module provides comprehensive error handling for all analytics operations.
//! All errors implement `std::error::Error` and are designed for production use.

use thiserror::Error;

/// Result type alias for analytics operations
pub type Result<T> = std::result::Result<T, AnalyticsError>;

/// Comprehensive error types for analytics operations
#[derive(Debug, Error)]
pub enum AnalyticsError {
    /// Invalid input parameters
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// Insufficient data for computation
    #[error("Insufficient data: {0}")]
    InsufficientData(String),

    /// Computation failed to converge
    #[error("Convergence failed: {0}")]
    ConvergenceError(String),

    /// Numerical instability detected
    #[error("Numerical instability: {0}")]
    NumericalInstability(String),

    /// Matrix operation failed
    #[error("Matrix operation failed: {0}")]
    MatrixError(String),

    /// Dimension mismatch in arrays
    #[error("Dimension mismatch: expected {expected}, got {actual}")]
    DimensionMismatch { expected: String, actual: String },

    /// Invalid parameter value
    #[error("Invalid parameter '{param}': {reason}")]
    InvalidParameter { param: String, reason: String },

    /// Statistical test failed
    #[error("Statistical test failed: {0}")]
    StatisticalTestError(String),

    /// Clustering operation failed
    #[error("Clustering failed: {0}")]
    ClusteringError(String),

    /// Interpolation failed
    #[error("Interpolation failed: {0}")]
    InterpolationError(String),

    /// Time series analysis failed
    #[error("Time series analysis failed: {0}")]
    TimeSeriesError(String),

    /// Hotspot analysis failed
    #[error("Hotspot analysis failed: {0}")]
    HotspotError(String),

    /// Change detection failed
    #[error("Change detection failed: {0}")]
    ChangeDetectionError(String),

    /// Zonal statistics failed
    #[error("Zonal statistics failed: {0}")]
    ZonalStatsError(String),

    /// Core library error
    #[error("Core error: {0}")]
    CoreError(#[from] oxigdal_core::error::OxiGdalError),

    /// SciRS2 error
    #[error("SciRS2 error: {0}")]
    SciRS2Error(String),

    /// I/O error
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
}

impl AnalyticsError {
    /// Create an invalid input error
    pub fn invalid_input(msg: impl Into<String>) -> Self {
        Self::InvalidInput(msg.into())
    }

    /// Create an insufficient data error
    pub fn insufficient_data(msg: impl Into<String>) -> Self {
        Self::InsufficientData(msg.into())
    }

    /// Create a convergence error
    pub fn convergence_error(msg: impl Into<String>) -> Self {
        Self::ConvergenceError(msg.into())
    }

    /// Create a numerical instability error
    pub fn numerical_instability(msg: impl Into<String>) -> Self {
        Self::NumericalInstability(msg.into())
    }

    /// Create a matrix error
    pub fn matrix_error(msg: impl Into<String>) -> Self {
        Self::MatrixError(msg.into())
    }

    /// Create a dimension mismatch error
    pub fn dimension_mismatch(expected: impl Into<String>, actual: impl Into<String>) -> Self {
        Self::DimensionMismatch {
            expected: expected.into(),
            actual: actual.into(),
        }
    }

    /// Create an invalid parameter error
    pub fn invalid_parameter(param: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::InvalidParameter {
            param: param.into(),
            reason: reason.into(),
        }
    }

    /// Create a statistical test error
    pub fn statistical_test_error(msg: impl Into<String>) -> Self {
        Self::StatisticalTestError(msg.into())
    }

    /// Create a clustering error
    pub fn clustering_error(msg: impl Into<String>) -> Self {
        Self::ClusteringError(msg.into())
    }

    /// Create an interpolation error
    pub fn interpolation_error(msg: impl Into<String>) -> Self {
        Self::InterpolationError(msg.into())
    }

    /// Create a time series error
    pub fn time_series_error(msg: impl Into<String>) -> Self {
        Self::TimeSeriesError(msg.into())
    }

    /// Create a hotspot analysis error
    pub fn hotspot_error(msg: impl Into<String>) -> Self {
        Self::HotspotError(msg.into())
    }

    /// Create a change detection error
    pub fn change_detection_error(msg: impl Into<String>) -> Self {
        Self::ChangeDetectionError(msg.into())
    }

    /// Create a zonal statistics error
    pub fn zonal_stats_error(msg: impl Into<String>) -> Self {
        Self::ZonalStatsError(msg.into())
    }

    /// Create a SciRS2 error
    pub fn scirs2_error(msg: impl Into<String>) -> Self {
        Self::SciRS2Error(msg.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let err = AnalyticsError::invalid_input("test");
        assert!(matches!(err, AnalyticsError::InvalidInput(_)));

        let err = AnalyticsError::dimension_mismatch("3x3", "4x4");
        assert!(matches!(err, AnalyticsError::DimensionMismatch { .. }));
    }

    #[test]
    fn test_error_display() {
        let err = AnalyticsError::invalid_input("invalid parameter value");
        assert_eq!(format!("{}", err), "Invalid input: invalid parameter value");

        let err = AnalyticsError::dimension_mismatch("3x3", "4x4");
        assert_eq!(
            format!("{}", err),
            "Dimension mismatch: expected 3x3, got 4x4"
        );
    }
}
