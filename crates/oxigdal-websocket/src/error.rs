//! Error types for oxigdal-websocket

use thiserror::Error;

/// Result type for oxigdal-websocket operations
pub type Result<T> = std::result::Result<T, Error>;

/// Error types for WebSocket operations
#[derive(Error, Debug)]
pub enum Error {
    /// WebSocket error
    #[error("WebSocket error: {0}")]
    WebSocket(String),

    /// Connection error
    #[error("Connection error: {0}")]
    Connection(String),

    /// Protocol error
    #[error("Protocol error: {0}")]
    Protocol(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Compression error
    #[error("Compression error: {0}")]
    Compression(String),

    /// Broadcast error
    #[error("Broadcast error: {0}")]
    Broadcast(String),

    /// Room error
    #[error("Room error: {0}")]
    Room(String),

    /// Subscription error
    #[error("Subscription error: {0}")]
    Subscription(String),

    /// Channel error
    #[error("Channel error: {0}")]
    Channel(String),

    /// Timeout error
    #[error("Timeout error: {0}")]
    Timeout(String),

    /// Invalid message error
    #[error("Invalid message: {0}")]
    InvalidMessage(String),

    /// Invalid state error
    #[error("Invalid state: {0}")]
    InvalidState(String),

    /// Resource exhausted error
    #[error("Resource exhausted: {0}")]
    ResourceExhausted(String),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// MessagePack error
    #[error("MessagePack error: {0}")]
    MessagePack(String),

    /// OxiGDAL core error
    #[error("OxiGDAL core error: {0}")]
    Core(#[from] oxigdal_core::error::OxiGdalError),
}

impl From<tokio_tungstenite::tungstenite::Error> for Error {
    fn from(err: tokio_tungstenite::tungstenite::Error) -> Self {
        Error::WebSocket(err.to_string())
    }
}

impl<T> From<tokio::sync::mpsc::error::SendError<T>> for Error {
    fn from(err: tokio::sync::mpsc::error::SendError<T>) -> Self {
        Error::Channel(err.to_string())
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
