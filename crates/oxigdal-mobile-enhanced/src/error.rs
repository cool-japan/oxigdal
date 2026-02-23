//! Error types for mobile-enhanced operations

use thiserror::Error;

/// Result type alias using MobileError
pub type Result<T> = core::result::Result<T, MobileError>;

/// Errors that can occur in mobile-enhanced operations
#[derive(Debug, Error)]
pub enum MobileError {
    /// Battery monitoring is not supported on this platform
    #[error("Battery monitoring is not supported on this platform")]
    BatteryMonitoringNotSupported,

    /// Failed to read battery information
    #[error("Failed to read battery information: {0}")]
    BatteryReadError(String),

    /// Network optimization failed
    #[error("Network optimization failed: {0}")]
    NetworkOptimizationError(String),

    /// Compression failed
    #[error("Compression failed: {0}")]
    CompressionError(String),

    /// Decompression failed
    #[error("Decompression failed: {0}")]
    DecompressionError(String),

    /// Background task scheduling failed
    #[error("Background task scheduling failed: {0}")]
    BackgroundTaskError(String),

    /// Storage operation failed
    #[error("Storage operation failed: {0}")]
    StorageError(String),

    /// Cache operation failed
    #[error("Cache operation failed: {0}")]
    CacheError(String),

    /// Memory pressure handling failed
    #[error("Memory pressure handling failed: {0}")]
    MemoryPressureError(String),

    /// Platform-specific operation failed
    #[error("Platform-specific operation failed: {0}")]
    PlatformError(String),

    /// Invalid configuration
    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),

    /// Feature not available
    #[error("Feature not available: {0}")]
    FeatureNotAvailable(String),

    /// I/O error
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// Serialization error
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    /// Core library error
    #[error("Core library error: {0}")]
    CoreError(String),
}

impl From<oxigdal_core::error::OxiGdalError> for MobileError {
    fn from(err: oxigdal_core::error::OxiGdalError) -> Self {
        Self::CoreError(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = MobileError::BatteryMonitoringNotSupported;
        assert!(err.to_string().contains("Battery monitoring"));

        let err = MobileError::NetworkOptimizationError("test".to_string());
        assert!(err.to_string().contains("Network optimization"));
        assert!(err.to_string().contains("test"));
    }

    #[test]
    fn test_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let mobile_err: MobileError = io_err.into();
        assert!(mobile_err.to_string().contains("I/O error"));
    }

    #[test]
    fn test_result_type() {
        let ok_result: Result<i32> = Ok(42);
        assert_eq!(ok_result.ok(), Some(42));

        let err_result: Result<i32> = Err(MobileError::BatteryMonitoringNotSupported);
        assert!(err_result.is_err());
    }
}
