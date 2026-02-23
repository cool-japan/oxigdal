//! Error types for offline data management

use thiserror::Error;

/// Result type for offline operations
pub type Result<T> = core::result::Result<T, Error>;

/// Errors that can occur during offline operations
#[derive(Debug, Error)]
pub enum Error {
    /// Storage backend error
    #[error("Storage error: {0}")]
    Storage(String),

    /// Sync queue error
    #[error("Sync queue error: {0}")]
    SyncQueue(String),

    /// Conflict detected
    #[error("Conflict detected: {0}")]
    Conflict(String),

    /// Merge error
    #[error("Merge error: {0}")]
    Merge(String),

    /// Retry exhausted
    #[error("Retry exhausted after {attempts} attempts: {message}")]
    RetryExhausted {
        /// Number of attempts made
        attempts: usize,
        /// Error message
        message: String,
    },

    /// Network error
    #[error("Network error: {0}")]
    Network(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Deserialization error
    #[error("Deserialization error: {0}")]
    Deserialization(String),

    /// Invalid operation
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),

    /// Record not found
    #[error("Record not found: {0}")]
    NotFound(String),

    /// Version mismatch
    #[error("Version mismatch: expected {expected}, got {actual}")]
    VersionMismatch {
        /// Expected version
        expected: u64,
        /// Actual version
        actual: u64,
    },

    /// Capacity exceeded
    #[error("Capacity exceeded: {0}")]
    CapacityExceeded(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    Config(String),

    /// Database error
    #[error("Database error: {0}")]
    Database(String),

    /// Lock error
    #[error("Lock error: {0}")]
    Lock(String),

    /// Timeout error
    #[error("Timeout error: {0}")]
    Timeout(String),

    /// Internal error
    #[error("Internal error: {0}")]
    Internal(String),
}

#[cfg(feature = "native")]
impl From<rusqlite::Error> for Error {
    fn from(err: rusqlite::Error) -> Self {
        Error::Database(err.to_string())
    }
}

#[cfg(feature = "wasm")]
impl From<wasm_bindgen::JsValue> for Error {
    fn from(err: wasm_bindgen::JsValue) -> Self {
        Error::Storage(format!("WASM error: {err:?}"))
    }
}

impl Error {
    /// Create a storage error
    pub fn storage(message: impl Into<String>) -> Self {
        Self::Storage(message.into())
    }

    /// Create a sync queue error
    pub fn sync_queue(message: impl Into<String>) -> Self {
        Self::SyncQueue(message.into())
    }

    /// Create a conflict error
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::Conflict(message.into())
    }

    /// Create a merge error
    pub fn merge(message: impl Into<String>) -> Self {
        Self::Merge(message.into())
    }

    /// Create a network error
    pub fn network(message: impl Into<String>) -> Self {
        Self::Network(message.into())
    }

    /// Create a serialization error
    pub fn serialization(message: impl Into<String>) -> Self {
        Self::Serialization(message.into())
    }

    /// Create a deserialization error
    pub fn deserialization(message: impl Into<String>) -> Self {
        Self::Deserialization(message.into())
    }

    /// Create an invalid operation error
    pub fn invalid_operation(message: impl Into<String>) -> Self {
        Self::InvalidOperation(message.into())
    }

    /// Create a not found error
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }

    /// Create a version mismatch error
    pub fn version_mismatch(expected: u64, actual: u64) -> Self {
        Self::VersionMismatch { expected, actual }
    }

    /// Create a capacity exceeded error
    pub fn capacity_exceeded(message: impl Into<String>) -> Self {
        Self::CapacityExceeded(message.into())
    }

    /// Create a configuration error
    pub fn config(message: impl Into<String>) -> Self {
        Self::Config(message.into())
    }

    /// Create an internal error
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }

    /// Create a not supported error
    pub fn not_supported(message: impl Into<String>) -> Self {
        Self::InvalidOperation(format!("Not supported: {}", message.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = Error::Conflict("concurrent modification".to_string());
        assert!(err.to_string().contains("Conflict detected"));
        assert!(err.to_string().contains("concurrent modification"));
    }

    #[test]
    fn test_version_mismatch() {
        let err = Error::version_mismatch(5, 3);
        assert!(matches!(
            err,
            Error::VersionMismatch {
                expected: 5,
                actual: 3
            }
        ));
    }

    #[test]
    fn test_retry_exhausted() {
        let err = Error::RetryExhausted {
            attempts: 5,
            message: "network timeout".to_string(),
        };
        assert!(err.to_string().contains("5 attempts"));
    }
}
