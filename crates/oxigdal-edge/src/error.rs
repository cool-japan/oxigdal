//! Error types for edge computing operations

use std::fmt;

/// Result type alias for edge operations
pub type Result<T> = std::result::Result<T, EdgeError>;

/// Edge computing errors
#[derive(Debug, thiserror::Error)]
pub enum EdgeError {
    /// I/O error occurred
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

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

    /// Cache error
    #[error("Cache error: {0}")]
    Cache(String),

    /// Synchronization error
    #[error("Sync error: {0}")]
    Sync(String),

    /// Conflict resolution error
    #[error("Conflict resolution error: {0}")]
    Conflict(String),

    /// Resource constraint error
    #[error("Resource constraint violated: {0}")]
    ResourceConstraint(String),

    /// Runtime error
    #[error("Runtime error: {0}")]
    Runtime(String),

    /// Invalid configuration
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    /// Network error
    #[error("Network error: {0}")]
    Network(String),

    /// Storage error
    #[error("Storage error: {0}")]
    Storage(String),

    /// Operation not supported in current mode
    #[error("Operation not supported: {0}")]
    NotSupported(String),

    /// Timeout error
    #[error("Operation timed out: {0}")]
    Timeout(String),

    /// Generic error
    #[error("{0}")]
    Other(String),
}

impl EdgeError {
    /// Create a new serialization error
    pub fn serialization<S: fmt::Display>(msg: S) -> Self {
        Self::Serialization(msg.to_string())
    }

    /// Create a new deserialization error
    pub fn deserialization<S: fmt::Display>(msg: S) -> Self {
        Self::Deserialization(msg.to_string())
    }

    /// Create a new compression error
    pub fn compression<S: fmt::Display>(msg: S) -> Self {
        Self::Compression(msg.to_string())
    }

    /// Create a new decompression error
    pub fn decompression<S: fmt::Display>(msg: S) -> Self {
        Self::Decompression(msg.to_string())
    }

    /// Create a new cache error
    pub fn cache<S: fmt::Display>(msg: S) -> Self {
        Self::Cache(msg.to_string())
    }

    /// Create a new sync error
    pub fn sync<S: fmt::Display>(msg: S) -> Self {
        Self::Sync(msg.to_string())
    }

    /// Create a new conflict error
    pub fn conflict<S: fmt::Display>(msg: S) -> Self {
        Self::Conflict(msg.to_string())
    }

    /// Create a new resource constraint error
    pub fn resource_constraint<S: fmt::Display>(msg: S) -> Self {
        Self::ResourceConstraint(msg.to_string())
    }

    /// Create a new runtime error
    pub fn runtime<S: fmt::Display>(msg: S) -> Self {
        Self::Runtime(msg.to_string())
    }

    /// Create a new invalid config error
    pub fn invalid_config<S: fmt::Display>(msg: S) -> Self {
        Self::InvalidConfig(msg.to_string())
    }

    /// Create a new network error
    pub fn network<S: fmt::Display>(msg: S) -> Self {
        Self::Network(msg.to_string())
    }

    /// Create a new storage error
    pub fn storage<S: fmt::Display>(msg: S) -> Self {
        Self::Storage(msg.to_string())
    }

    /// Create a new not supported error
    pub fn not_supported<S: fmt::Display>(msg: S) -> Self {
        Self::NotSupported(msg.to_string())
    }

    /// Create a new timeout error
    pub fn timeout<S: fmt::Display>(msg: S) -> Self {
        Self::Timeout(msg.to_string())
    }

    /// Create a new generic error
    pub fn other<S: fmt::Display>(msg: S) -> Self {
        Self::Other(msg.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let err = EdgeError::cache("test error");
        assert_eq!(err.to_string(), "Cache error: test error");

        let err = EdgeError::sync("sync failed");
        assert_eq!(err.to_string(), "Sync error: sync failed");

        let err = EdgeError::resource_constraint("memory limit exceeded");
        assert_eq!(
            err.to_string(),
            "Resource constraint violated: memory limit exceeded"
        );
    }

    #[test]
    fn test_error_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let edge_err: EdgeError = io_err.into();
        assert!(matches!(edge_err, EdgeError::Io(_)));
    }
}
