//! Error types for OxiGDAL Temporal Analysis
//!
//! This module provides comprehensive error handling for all temporal operations.
//! All errors implement `std::error::Error` and are designed for production use.

use thiserror::Error;

/// Result type alias for temporal operations
pub type Result<T> = std::result::Result<T, TemporalError>;

/// Comprehensive error types for temporal analysis operations
#[derive(Debug, Error)]
pub enum TemporalError {
    /// Invalid temporal input
    #[error("Invalid temporal input: {0}")]
    InvalidInput(String),

    /// Insufficient temporal data for computation
    #[error("Insufficient temporal data: {0}")]
    InsufficientData(String),

    /// Time index out of bounds
    #[error("Time index out of bounds: {index} (valid range: {min}..{max})")]
    TimeIndexOutOfBounds {
        /// The invalid index that was provided
        index: usize,
        /// Minimum valid index
        min: usize,
        /// Maximum valid index (exclusive)
        max: usize,
    },

    /// Invalid time range
    #[error("Invalid time range: start={start}, end={end}")]
    InvalidTimeRange {
        /// Start of the invalid range
        start: String,
        /// End of the invalid range
        end: String,
    },

    /// Gap in time series data
    #[error("Gap detected in time series at position {position}")]
    GapDetected {
        /// Position where the gap was detected
        position: usize,
    },

    /// Temporal dimension mismatch
    #[error("Temporal dimension mismatch: expected {expected}, got {actual}")]
    DimensionMismatch {
        /// Expected dimension specification
        expected: String,
        /// Actual dimension specification
        actual: String,
    },

    /// Invalid temporal parameter
    #[error("Invalid temporal parameter '{param}': {reason}")]
    InvalidParameter {
        /// Name of the invalid parameter
        param: String,
        /// Reason why the parameter is invalid
        reason: String,
    },

    /// Compositing operation failed
    #[error("Compositing failed: {0}")]
    CompositingError(String),

    /// Interpolation failed
    #[error("Temporal interpolation failed: {0}")]
    InterpolationError(String),

    /// Aggregation failed
    #[error("Temporal aggregation failed: {0}")]
    AggregationError(String),

    /// Change detection failed
    #[error("Change detection failed: {0}")]
    ChangeDetectionError(String),

    /// Trend analysis failed
    #[error("Trend analysis failed: {0}")]
    TrendAnalysisError(String),

    /// Phenology analysis failed
    #[error("Phenology analysis failed: {0}")]
    PhenologyError(String),

    /// Data cube operation failed
    #[error("Data cube operation failed: {0}")]
    DataCubeError(String),

    /// Temporal metadata error
    #[error("Temporal metadata error: {0}")]
    MetadataError(String),

    /// Storage backend error
    #[error("Storage error: {0}")]
    StorageError(String),

    /// Zarr driver error
    #[cfg(feature = "zarr")]
    #[error("Zarr error: {0}")]
    ZarrError(String),

    /// Analytics library error
    #[error("Analytics error: {0}")]
    AnalyticsError(#[from] oxigdal_analytics::error::AnalyticsError),

    /// Core library error
    #[error("Core error: {0}")]
    CoreError(#[from] oxigdal_core::error::OxiGdalError),

    /// Date/time parsing error
    #[error("DateTime parse error: {0}")]
    DateTimeParseError(String),

    /// I/O error
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// Serialization error
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

impl TemporalError {
    /// Create an invalid input error
    pub fn invalid_input(msg: impl Into<String>) -> Self {
        Self::InvalidInput(msg.into())
    }

    /// Create an insufficient data error
    pub fn insufficient_data(msg: impl Into<String>) -> Self {
        Self::InsufficientData(msg.into())
    }

    /// Create a time index out of bounds error
    pub fn time_index_out_of_bounds(index: usize, min: usize, max: usize) -> Self {
        Self::TimeIndexOutOfBounds { index, min, max }
    }

    /// Create an invalid time range error
    pub fn invalid_time_range(start: impl Into<String>, end: impl Into<String>) -> Self {
        Self::InvalidTimeRange {
            start: start.into(),
            end: end.into(),
        }
    }

    /// Create a gap detected error
    pub fn gap_detected(position: usize) -> Self {
        Self::GapDetected { position }
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

    /// Create a compositing error
    pub fn compositing_error(msg: impl Into<String>) -> Self {
        Self::CompositingError(msg.into())
    }

    /// Create an interpolation error
    pub fn interpolation_error(msg: impl Into<String>) -> Self {
        Self::InterpolationError(msg.into())
    }

    /// Create an aggregation error
    pub fn aggregation_error(msg: impl Into<String>) -> Self {
        Self::AggregationError(msg.into())
    }

    /// Create a change detection error
    pub fn change_detection_error(msg: impl Into<String>) -> Self {
        Self::ChangeDetectionError(msg.into())
    }

    /// Create a trend analysis error
    pub fn trend_analysis_error(msg: impl Into<String>) -> Self {
        Self::TrendAnalysisError(msg.into())
    }

    /// Create a phenology error
    pub fn phenology_error(msg: impl Into<String>) -> Self {
        Self::PhenologyError(msg.into())
    }

    /// Create a data cube error
    pub fn datacube_error(msg: impl Into<String>) -> Self {
        Self::DataCubeError(msg.into())
    }

    /// Create a metadata error
    pub fn metadata_error(msg: impl Into<String>) -> Self {
        Self::MetadataError(msg.into())
    }

    /// Create a storage error
    pub fn storage_error(msg: impl Into<String>) -> Self {
        Self::StorageError(msg.into())
    }

    /// Create a Zarr error
    #[cfg(feature = "zarr")]
    pub fn zarr_error(msg: impl Into<String>) -> Self {
        Self::ZarrError(msg.into())
    }

    /// Create a date/time parse error
    pub fn datetime_parse_error(msg: impl Into<String>) -> Self {
        Self::DateTimeParseError(msg.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let err = TemporalError::invalid_input("test");
        assert!(matches!(err, TemporalError::InvalidInput(_)));

        let err = TemporalError::dimension_mismatch("100x100x10", "100x100x5");
        assert!(matches!(err, TemporalError::DimensionMismatch { .. }));

        let err = TemporalError::time_index_out_of_bounds(15, 0, 10);
        assert!(matches!(err, TemporalError::TimeIndexOutOfBounds { .. }));
    }

    #[test]
    fn test_error_display() {
        let err = TemporalError::invalid_input("invalid timestamp format");
        assert_eq!(
            format!("{}", err),
            "Invalid temporal input: invalid timestamp format"
        );

        let err = TemporalError::dimension_mismatch("100x100x10", "100x100x5");
        assert_eq!(
            format!("{}", err),
            "Temporal dimension mismatch: expected 100x100x10, got 100x100x5"
        );

        let err = TemporalError::time_index_out_of_bounds(15, 0, 10);
        assert_eq!(
            format!("{}", err),
            "Time index out of bounds: 15 (valid range: 0..10)"
        );
    }

    #[test]
    fn test_invalid_time_range() {
        let err = TemporalError::invalid_time_range("2023-12-31", "2023-01-01");
        assert!(matches!(err, TemporalError::InvalidTimeRange { .. }));
        assert!(format!("{}", err).contains("2023-12-31"));
        assert!(format!("{}", err).contains("2023-01-01"));
    }

    #[test]
    fn test_gap_detected() {
        let err = TemporalError::gap_detected(5);
        assert!(matches!(err, TemporalError::GapDetected { position: 5 }));
    }
}
