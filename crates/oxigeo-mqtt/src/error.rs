//! Error types for MQTT operations

use thiserror::Error;

/// Result type for MQTT operations
pub type Result<T> = std::result::Result<T, MqttError>;

/// Main error type for MQTT operations
#[derive(Debug, Error)]
pub enum MqttError {
    /// Connection error
    #[error("Connection error: {0}")]
    Connection(#[from] ConnectionError),

    /// Protocol error
    #[error("Protocol error: {0}")]
    Protocol(#[from] ProtocolError),

    /// Subscription error
    #[error("Subscription error: {0}")]
    Subscription(#[from] SubscriptionError),

    /// Publication error
    #[error("Publication error: {0}")]
    Publication(#[from] PublicationError),

    /// Persistence error
    #[error("Persistence error: {0}")]
    Persistence(#[from] PersistenceError),

    /// IoT error
    #[error("IoT error: {0}")]
    Iot(#[from] IotError),

    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Invalid parameter
    #[error("Invalid parameter '{parameter}': {message}")]
    InvalidParameter {
        /// Parameter name
        parameter: &'static str,
        /// Error message
        message: String,
    },

    /// Timeout error
    #[error("Operation timed out after {timeout_ms}ms")]
    Timeout {
        /// Timeout duration in milliseconds
        timeout_ms: u64,
    },

    /// Internal error
    #[error("Internal error: {0}")]
    Internal(String),
}

/// Connection-related errors
#[derive(Debug, Error)]
pub enum ConnectionError {
    /// Failed to connect to broker
    #[error("Failed to connect to broker at {broker}: {reason}")]
    ConnectFailed {
        /// Broker address
        broker: String,
        /// Failure reason
        reason: String,
    },

    /// Connection lost
    #[error("Connection lost: {reason}")]
    ConnectionLost {
        /// Reason for connection loss
        reason: String,
    },

    /// Authentication failed
    #[error("Authentication failed for client '{client_id}': {reason}")]
    AuthenticationFailed {
        /// Client ID
        client_id: String,
        /// Failure reason
        reason: String,
    },

    /// TLS error
    #[error("TLS error: {0}")]
    Tls(String),

    /// Invalid broker URL
    #[error("Invalid broker URL: {0}")]
    InvalidBrokerUrl(String),

    /// Keep-alive timeout
    #[error("Keep-alive timeout")]
    KeepAliveTimeout,

    /// Maximum reconnect attempts reached
    #[error("Maximum reconnect attempts ({max_attempts}) reached")]
    MaxReconnectAttemptsReached {
        /// Maximum attempts
        max_attempts: usize,
    },
}

/// Protocol-related errors
#[derive(Debug, Error)]
pub enum ProtocolError {
    /// Unsupported protocol version
    #[error("Unsupported protocol version: {version}")]
    UnsupportedVersion {
        /// Protocol version
        version: String,
    },

    /// Invalid QoS level
    #[error("Invalid QoS level: {qos}")]
    InvalidQos {
        /// QoS value
        qos: u8,
    },

    /// Invalid topic
    #[error("Invalid topic: {topic}")]
    InvalidTopic {
        /// Topic string
        topic: String,
    },

    /// Packet too large
    #[error("Packet too large: {size} bytes exceeds maximum {max_size} bytes")]
    PacketTooLarge {
        /// Packet size
        size: usize,
        /// Maximum size
        max_size: usize,
    },

    /// Protocol violation
    #[error("Protocol violation: {0}")]
    ProtocolViolation(String),

    /// Invalid packet
    #[error("Invalid packet: {0}")]
    InvalidPacket(String),
}

/// Subscription-related errors
#[derive(Debug, Error)]
pub enum SubscriptionError {
    /// Subscription failed
    #[error("Failed to subscribe to topic '{topic}': {reason}")]
    SubscribeFailed {
        /// Topic
        topic: String,
        /// Failure reason
        reason: String,
    },

    /// Unsubscribe failed
    #[error("Failed to unsubscribe from topic '{topic}': {reason}")]
    UnsubscribeFailed {
        /// Topic
        topic: String,
        /// Failure reason
        reason: String,
    },

    /// Invalid wildcard
    #[error("Invalid wildcard in topic '{topic}': {reason}")]
    InvalidWildcard {
        /// Topic
        topic: String,
        /// Reason
        reason: String,
    },

    /// Subscription not found
    #[error("No active subscription for topic: {topic}")]
    NotFound {
        /// Topic
        topic: String,
    },

    /// Maximum subscriptions reached
    #[error("Maximum subscriptions ({max_subscriptions}) reached")]
    MaxSubscriptionsReached {
        /// Maximum subscriptions
        max_subscriptions: usize,
    },
}

/// Publication-related errors
#[derive(Debug, Error)]
pub enum PublicationError {
    /// Publish failed
    #[error("Failed to publish to topic '{topic}': {reason}")]
    PublishFailed {
        /// Topic
        topic: String,
        /// Failure reason
        reason: String,
    },

    /// QoS not acknowledged
    #[error("QoS {qos} publish not acknowledged for topic '{topic}'")]
    NotAcknowledged {
        /// Topic
        topic: String,
        /// QoS level
        qos: u8,
    },

    /// Payload too large
    #[error("Payload too large: {size} bytes exceeds maximum {max_size} bytes")]
    PayloadTooLarge {
        /// Payload size
        size: usize,
        /// Maximum size
        max_size: usize,
    },

    /// Duplicate packet ID
    #[error("Duplicate packet ID: {packet_id}")]
    DuplicatePacketId {
        /// Packet ID
        packet_id: u16,
    },
}

/// Persistence-related errors
#[derive(Debug, Error)]
pub enum PersistenceError {
    /// Failed to open persistence store
    #[error("Failed to open persistence store at '{path}': {reason}")]
    OpenFailed {
        /// Store path
        path: String,
        /// Failure reason
        reason: String,
    },

    /// Failed to write to store
    #[error("Failed to write to persistence store: {0}")]
    WriteFailed(String),

    /// Failed to read from store
    #[error("Failed to read from persistence store: {0}")]
    ReadFailed(String),

    /// Failed to delete from store
    #[error("Failed to delete from persistence store: {0}")]
    DeleteFailed(String),

    /// Store corruption
    #[error("Persistence store corrupted: {0}")]
    Corrupted(String),
}

/// IoT-related errors
#[derive(Debug, Error)]
pub enum IotError {
    /// Invalid sensor data
    #[error("Invalid sensor data: {0}")]
    InvalidSensorData(String),

    /// Geospatial error
    #[error("Geospatial error: {0}")]
    Geospatial(String),

    /// Time-series error
    #[error("Time-series error: {0}")]
    TimeSeries(String),

    /// Message format error
    #[error("Message format error: {0}")]
    MessageFormat(String),

    /// Encoding error
    #[error("Encoding error: {0}")]
    Encoding(String),

    /// Decoding error
    #[error("Decoding error: {0}")]
    Decoding(String),
}

impl From<serde_json::Error> for MqttError {
    fn from(err: serde_json::Error) -> Self {
        Self::Serialization(err.to_string())
    }
}

impl From<rumqttc::ClientError> for MqttError {
    fn from(err: rumqttc::ClientError) -> Self {
        Self::Connection(ConnectionError::ConnectFailed {
            broker: "unknown".to_string(),
            reason: err.to_string(),
        })
    }
}

impl From<rumqttc::ConnectionError> for MqttError {
    fn from(err: rumqttc::ConnectionError) -> Self {
        Self::Connection(ConnectionError::ConnectionLost {
            reason: err.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = MqttError::InvalidParameter {
            parameter: "qos",
            message: "must be 0, 1, or 2".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("qos"));
        assert!(display.contains("must be 0, 1, or 2"));
    }

    #[test]
    fn test_connection_error() {
        let err = ConnectionError::ConnectFailed {
            broker: "mqtt://localhost:1883".to_string(),
            reason: "connection refused".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("localhost:1883"));
        assert!(display.contains("connection refused"));
    }

    #[test]
    fn test_protocol_error() {
        let err = ProtocolError::InvalidQos { qos: 3 };
        let display = format!("{}", err);
        assert!(display.contains("3"));
    }

    #[test]
    fn test_subscription_error() {
        let err = SubscriptionError::SubscribeFailed {
            topic: "sensor/+/temperature".to_string(),
            reason: "not authorized".to_string(),
        };
        let display = format!("{}", err);
        assert!(display.contains("sensor/+/temperature"));
        assert!(display.contains("not authorized"));
    }
}
