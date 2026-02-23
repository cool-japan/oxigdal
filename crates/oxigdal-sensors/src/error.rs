//! Error types for OxiGDAL Sensors
//!
//! This module provides comprehensive error handling for all sensor operations.
//! All errors implement `std::error::Error` and are designed for production use.

use thiserror::Error;

/// Result type alias for sensor operations
pub type Result<T> = std::result::Result<T, SensorError>;

/// Comprehensive error types for sensor operations
#[derive(Debug, Error)]
pub enum SensorError {
    /// Invalid sensor type or configuration
    #[error("Invalid sensor: {0}")]
    InvalidSensor(String),

    /// Invalid band specification
    #[error("Invalid band: {0}")]
    InvalidBand(String),

    /// Band not found for sensor
    #[error("Band '{band}' not found for sensor '{sensor}'")]
    BandNotFound {
        /// The sensor name
        sensor: String,
        /// The band name that was not found
        band: String,
    },

    /// Invalid spectral index
    #[error("Invalid spectral index: {0}")]
    InvalidIndex(String),

    /// Missing required band for operation
    #[error("Missing required band '{band}' for operation '{operation}'")]
    MissingBand {
        /// The missing band name
        band: String,
        /// The operation that required the band
        operation: String,
    },

    /// Invalid metadata
    #[error("Invalid metadata: {0}")]
    InvalidMetadata(String),

    /// Radiometric calibration failed
    #[error("Radiometric calibration failed: {0}")]
    CalibrationError(String),

    /// Atmospheric correction failed
    #[error("Atmospheric correction failed: {0}")]
    AtmosphericCorrectionError(String),

    /// BRDF normalization failed
    #[error("BRDF normalization failed: {0}")]
    BrdfError(String),

    /// Spectral index calculation failed
    #[error("Spectral index calculation failed: {0}")]
    IndexError(String),

    /// Pan-sharpening failed
    #[error("Pan-sharpening failed: {0}")]
    PanSharpeningError(String),

    /// Classification failed
    #[error("Classification failed: {0}")]
    ClassificationError(String),

    /// Invalid DN (Digital Number) value
    #[error("Invalid DN value: {0}")]
    InvalidDN(String),

    /// Invalid radiance value
    #[error("Invalid radiance value: {0}")]
    InvalidRadiance(String),

    /// Invalid reflectance value
    #[error("Invalid reflectance value: {0}")]
    InvalidReflectance(String),

    /// Invalid solar angle
    #[error("Invalid solar angle: {0}")]
    InvalidSolarAngle(String),

    /// Invalid date/time
    #[error("Invalid date/time: {0}")]
    InvalidDateTime(String),

    /// Dimension mismatch in arrays
    #[error("Dimension mismatch: expected {expected}, got {actual}")]
    DimensionMismatch {
        /// The expected dimension
        expected: String,
        /// The actual dimension
        actual: String,
    },

    /// Invalid parameter value
    #[error("Invalid parameter '{param}': {reason}")]
    InvalidParameter {
        /// The invalid parameter name
        param: String,
        /// The reason why it is invalid
        reason: String,
    },

    /// Numerical instability detected
    #[error("Numerical instability: {0}")]
    NumericalInstability(String),

    /// Division by zero
    #[error("Division by zero in operation: {0}")]
    DivisionByZero(String),

    /// Core library error
    #[error("Core error: {0}")]
    CoreError(#[from] oxigdal_core::error::OxiGdalError),

    /// SciRS2 error
    #[error("SciRS2 error: {0}")]
    SciRS2Error(String),

    /// I/O error
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// Serialization error
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

impl SensorError {
    /// Create an invalid sensor error
    pub fn invalid_sensor(msg: impl Into<String>) -> Self {
        Self::InvalidSensor(msg.into())
    }

    /// Create an invalid band error
    pub fn invalid_band(msg: impl Into<String>) -> Self {
        Self::InvalidBand(msg.into())
    }

    /// Create a band not found error
    pub fn band_not_found(sensor: impl Into<String>, band: impl Into<String>) -> Self {
        Self::BandNotFound {
            sensor: sensor.into(),
            band: band.into(),
        }
    }

    /// Create an invalid index error
    pub fn invalid_index(msg: impl Into<String>) -> Self {
        Self::InvalidIndex(msg.into())
    }

    /// Create a missing band error
    pub fn missing_band(band: impl Into<String>, operation: impl Into<String>) -> Self {
        Self::MissingBand {
            band: band.into(),
            operation: operation.into(),
        }
    }

    /// Create an invalid metadata error
    pub fn invalid_metadata(msg: impl Into<String>) -> Self {
        Self::InvalidMetadata(msg.into())
    }

    /// Create a calibration error
    pub fn calibration_error(msg: impl Into<String>) -> Self {
        Self::CalibrationError(msg.into())
    }

    /// Create an atmospheric correction error
    pub fn atmospheric_correction_error(msg: impl Into<String>) -> Self {
        Self::AtmosphericCorrectionError(msg.into())
    }

    /// Create a BRDF error
    pub fn brdf_error(msg: impl Into<String>) -> Self {
        Self::BrdfError(msg.into())
    }

    /// Create an index error
    pub fn index_error(msg: impl Into<String>) -> Self {
        Self::IndexError(msg.into())
    }

    /// Create a pan-sharpening error
    pub fn pan_sharpening_error(msg: impl Into<String>) -> Self {
        Self::PanSharpeningError(msg.into())
    }

    /// Create a classification error
    pub fn classification_error(msg: impl Into<String>) -> Self {
        Self::ClassificationError(msg.into())
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

    /// Create a numerical instability error
    pub fn numerical_instability(msg: impl Into<String>) -> Self {
        Self::NumericalInstability(msg.into())
    }

    /// Create a division by zero error
    pub fn division_by_zero(msg: impl Into<String>) -> Self {
        Self::DivisionByZero(msg.into())
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
        let err = SensorError::invalid_sensor("Unknown sensor");
        assert!(matches!(err, SensorError::InvalidSensor(_)));

        let err = SensorError::band_not_found("Landsat8", "B10");
        assert!(matches!(err, SensorError::BandNotFound { .. }));

        let err = SensorError::missing_band("NIR", "NDVI");
        assert!(matches!(err, SensorError::MissingBand { .. }));
    }

    #[test]
    fn test_error_display() {
        let err = SensorError::invalid_sensor("Unknown sensor type");
        assert_eq!(format!("{}", err), "Invalid sensor: Unknown sensor type");

        let err = SensorError::band_not_found("Landsat8", "B10");
        assert_eq!(
            format!("{}", err),
            "Band 'B10' not found for sensor 'Landsat8'"
        );

        let err = SensorError::dimension_mismatch("100x100", "200x200");
        assert_eq!(
            format!("{}", err),
            "Dimension mismatch: expected 100x100, got 200x200"
        );
    }
}
