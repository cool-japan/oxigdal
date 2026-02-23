//! Error types for the streaming module.

/// Result type for streaming operations.
pub type Result<T> = std::result::Result<T, StreamingError>;

/// Errors that can occur during streaming operations.
#[derive(Debug, thiserror::Error)]
pub enum StreamingError {
    /// Core OxiGDAL error
    #[error("OxiGDAL error: {0}")]
    Core(#[from] oxigdal_core::error::OxiGdalError),

    /// Stream is closed
    #[error("Stream is closed")]
    StreamClosed,

    /// Stream buffer full
    #[error("Stream buffer is full")]
    BufferFull,

    /// Invalid window configuration
    #[error("Invalid window configuration: {0}")]
    InvalidWindow(String),

    /// Watermark error
    #[error("Watermark error: {0}")]
    WatermarkError(String),

    /// State error
    #[error("State error: {0}")]
    StateError(String),

    /// Checkpoint error
    #[error("Checkpoint error: {0}")]
    CheckpointError(String),

    /// Partition error
    #[error("Partition error: {0}")]
    PartitionError(String),

    /// Join error
    #[error("Join error: {0}")]
    JoinError(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Deserialization error
    #[error("Deserialization error: {0}")]
    DeserializationError(String),

    /// RocksDB error
    #[cfg(feature = "rocksdb-backend")]
    #[error("RocksDB error: {0}")]
    RocksDB(#[from] rocksdb::Error),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Arrow error
    #[error("Arrow error: {0}")]
    Arrow(#[from] arrow::error::ArrowError),

    /// Send error
    #[error("Channel send error")]
    SendError,

    /// Receive error
    #[error("Channel receive error")]
    RecvError,

    /// Timeout error
    #[error("Operation timed out")]
    Timeout,

    /// Invalid state
    #[error("Invalid state: {0}")]
    InvalidState(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// Invalid operation
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),

    /// Not implemented
    #[error("Not implemented: {0}")]
    NotImplemented(String),

    /// Other error
    #[error("Other error: {0}")]
    Other(String),
}

impl<T> From<crossbeam_channel::SendError<T>> for StreamingError {
    fn from(_: crossbeam_channel::SendError<T>) -> Self {
        StreamingError::SendError
    }
}

impl From<crossbeam_channel::RecvError> for StreamingError {
    fn from(_: crossbeam_channel::RecvError) -> Self {
        StreamingError::RecvError
    }
}

impl From<serde_json::Error> for StreamingError {
    fn from(e: serde_json::Error) -> Self {
        StreamingError::SerializationError(e.to_string())
    }
}
