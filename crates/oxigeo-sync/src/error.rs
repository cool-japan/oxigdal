//! Error types for synchronization operations

use thiserror::Error;

/// Result type for synchronization operations
pub type SyncResult<T> = Result<T, SyncError>;

/// Synchronization error types
#[derive(Error, Debug)]
pub enum SyncError {
    /// Vector clock comparison resulted in concurrent/conflicting events
    #[error("Concurrent events detected: {0}")]
    ConcurrentEvents(String),

    /// Invalid operation in operational transformation
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),

    /// Merkle tree verification failed
    #[error("Merkle tree verification failed: {0}")]
    MerkleVerificationFailed(String),

    /// Delta encoding/decoding error
    #[error("Delta encoding error: {0}")]
    DeltaEncodingError(String),

    /// Device coordination error
    #[error("Device coordination error: {0}")]
    CoordinationError(String),

    /// CRDT merge conflict
    #[error("CRDT merge conflict: {0}")]
    MergeConflict(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// I/O error
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// JSON error
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    /// Invalid device ID
    #[error("Invalid device ID: {0}")]
    InvalidDeviceId(String),

    /// Operation timeout
    #[error("Operation timeout: {0}")]
    Timeout(String),

    /// Network error
    #[error("Network error: {0}")]
    NetworkError(String),

    /// State inconsistency detected
    #[error("State inconsistency: {0}")]
    StateInconsistency(String),
}
