//! Error types for WebSocket operations.

/// Result type for WebSocket operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can occur during WebSocket operations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// WebSocket connection error
    #[error("WebSocket connection error: {0}")]
    Connection(String),

    /// WebSocket send error
    #[error("Failed to send WebSocket message: {0}")]
    Send(String),

    /// WebSocket receive error
    #[error("Failed to receive WebSocket message: {0}")]
    Receive(String),

    /// Protocol error
    #[error("Protocol error: {0}")]
    Protocol(String),

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

    /// Subscription error
    #[error("Subscription error: {0}")]
    Subscription(String),

    /// Authentication error
    #[error("Authentication failed: {0}")]
    Authentication(String),

    /// Authorization error
    #[error("Authorization failed: {0}")]
    Authorization(String),

    /// Rate limit exceeded
    #[error("Rate limit exceeded: {0}")]
    RateLimit(String),

    /// Invalid message
    #[error("Invalid message: {0}")]
    InvalidMessage(String),

    /// Invalid parameter
    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),

    /// Resource not found
    #[error("Resource not found: {0}")]
    NotFound(String),

    /// Server error
    #[error("Server error: {0}")]
    Server(String),

    /// Client error
    #[error("Client error: {0}")]
    Client(String),

    /// Timeout error
    #[error("Operation timed out: {0}")]
    Timeout(String),

    /// Channel send error
    #[error("Channel send error")]
    ChannelSend,

    /// Channel receive error
    #[error("Channel receive error")]
    ChannelReceive,

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// OxiGDAL core error
    #[error("OxiGDAL error: {0}")]
    Core(String),

    /// JSON error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// MessagePack error
    #[error("MessagePack error: {0}")]
    MessagePack(String),

    /// Axum error
    #[error("Axum error: {0}")]
    Axum(String),

    /// Other error
    #[error("Other error: {0}")]
    Other(String),
}

impl From<axum::Error> for Error {
    fn from(err: axum::Error) -> Self {
        Error::Axum(err.to_string())
    }
}

impl From<rmp_serde::encode::Error> for Error {
    fn from(err: rmp_serde::encode::Error) -> Self {
        Error::MessagePack(err.to_string())
    }
}

impl From<rmp_serde::decode::Error> for Error {
    fn from(err: rmp_serde::decode::Error) -> Self {
        Error::MessagePack(err.to_string())
    }
}

impl<T> From<tokio::sync::mpsc::error::SendError<T>> for Error {
    fn from(_: tokio::sync::mpsc::error::SendError<T>) -> Self {
        Error::ChannelSend
    }
}

impl From<tokio::sync::oneshot::error::RecvError> for Error {
    fn from(_: tokio::sync::oneshot::error::RecvError) -> Self {
        Error::ChannelReceive
    }
}
