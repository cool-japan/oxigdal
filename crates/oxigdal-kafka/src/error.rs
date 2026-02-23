//! Error types for Kafka operations

use thiserror::Error;

/// Result type for Kafka operations
pub type Result<T> = core::result::Result<T, Error>;

/// Errors that can occur during Kafka operations
#[derive(Error, Debug)]
pub enum Error {
    /// Kafka client error
    #[error("Kafka client error: {0}")]
    KafkaClient(String),

    /// Producer error
    #[error("Producer error: {0}")]
    Producer(String),

    /// Consumer error
    #[error("Consumer error: {0}")]
    Consumer(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Deserialization error
    #[error("Deserialization error: {0}")]
    Deserialization(String),

    /// Schema registry error
    #[error("Schema registry error: {0}")]
    SchemaRegistry(String),

    /// Schema not found
    #[error("Schema not found: {0}")]
    SchemaNotFound(String),

    /// Schema incompatible
    #[error("Schema incompatible: {0}")]
    SchemaIncompatible(String),

    /// Transaction error
    #[error("Transaction error: {0}")]
    Transaction(String),

    /// Transaction aborted
    #[error("Transaction aborted: {0}")]
    TransactionAborted(String),

    /// Offset commit error
    #[error("Offset commit error: {0}")]
    OffsetCommit(String),

    /// Partition error
    #[error("Partition error: {0}")]
    Partition(String),

    /// Rebalance error
    #[error("Rebalance error: {0}")]
    Rebalance(String),

    /// Timeout error
    #[error("Timeout error: {0}")]
    Timeout(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    Configuration(String),

    /// Invalid topic
    #[error("Invalid topic: {0}")]
    InvalidTopic(String),

    /// Invalid key
    #[error("Invalid key: {0}")]
    InvalidKey(String),

    /// Invalid value
    #[error("Invalid value: {0}")]
    InvalidValue(String),

    /// Connection error
    #[error("Connection error: {0}")]
    Connection(String),

    /// Broker error
    #[error("Broker error: {0}")]
    Broker(String),

    /// Network error
    #[error("Network error: {0}")]
    Network(String),

    /// Compression error
    #[error("Compression error: {0}")]
    Compression(String),

    /// Decompression error
    #[error("Decompression error: {0}")]
    Decompression(String),

    /// IO error
    #[error("IO error: {0}")]
    Io(String),

    /// HTTP error for schema registry
    #[error("HTTP error: {0}")]
    Http(String),

    /// Invalid state
    #[error("Invalid state: {0}")]
    InvalidState(String),

    /// Not implemented
    #[error("Not implemented: {0}")]
    NotImplemented(String),

    /// Other error
    #[error("Other error: {0}")]
    Other(String),
}

impl From<rdkafka::error::KafkaError> for Error {
    fn from(err: rdkafka::error::KafkaError) -> Self {
        Error::KafkaClient(err.to_string())
    }
}

#[cfg(feature = "std")]
impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::Io(err.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::Serialization(err.to_string())
    }
}

impl From<reqwest::Error> for Error {
    fn from(err: reqwest::Error) -> Self {
        Error::Http(err.to_string())
    }
}

#[cfg(feature = "schema-registry")]
impl From<apache_avro::Error> for Error {
    fn from(err: apache_avro::Error) -> Self {
        Error::Serialization(err.to_string())
    }
}
