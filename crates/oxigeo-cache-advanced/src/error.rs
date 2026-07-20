//! Error types for cache operations

use std::io;

/// Result type for cache operations
pub type Result<T> = std::result::Result<T, CacheError>;

/// Errors that can occur during cache operations
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    /// I/O error occurred
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Deserialization error
    #[error("Deserialization error: {0}")]
    Deserialization(String),

    /// Compression error
    #[error("Compression error: {0}")]
    Compression(String),

    /// Decompression error
    #[error("Decompression error: {0}")]
    Decompression(String),

    /// Cache full error
    #[error("Cache tier is full: {0}")]
    CacheFull(String),

    /// Key not found
    #[error("Key not found: {0}")]
    KeyNotFound(String),

    /// Invalid configuration
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    /// Network error for distributed cache
    #[error("Network error: {0}")]
    Network(String),

    /// Timeout error
    #[error("Operation timed out")]
    Timeout,

    /// Prediction error
    #[error("Prediction error: {0}")]
    Prediction(String),

    /// Analytics error
    #[error("Analytics error: {0}")]
    Analytics(String),

    /// Lock error
    #[error("Lock acquisition failed")]
    LockError,

    /// Generic error
    #[error("Cache error: {0}")]
    Other(String),
}

impl From<serde_json::Error> for CacheError {
    fn from(err: serde_json::Error) -> Self {
        CacheError::Serialization(err.to_string())
    }
}

impl From<Box<dyn std::error::Error + Send + Sync>> for CacheError {
    fn from(err: Box<dyn std::error::Error + Send + Sync>) -> Self {
        CacheError::Other(err.to_string())
    }
}
