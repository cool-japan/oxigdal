//! Error types for OxiGDAL Pub/Sub operations.
//!
//! This module provides comprehensive error handling for Google Cloud Pub/Sub
//! operations including publishing, subscribing, schema validation, and monitoring.

#[cfg(feature = "schema")]
use std::fmt;

/// Result type for Pub/Sub operations.
pub type Result<T> = std::result::Result<T, PubSubError>;

/// Errors that can occur during Pub/Sub operations.
#[derive(Debug, thiserror::Error)]
pub enum PubSubError {
    /// Error during message publishing.
    #[error("Publishing error: {message}")]
    PublishError {
        /// Error message
        message: String,
        /// Optional source error
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Error during message subscription.
    #[error("Subscription error: {message}")]
    SubscriptionError {
        /// Error message
        message: String,
        /// Optional source error
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Error during message acknowledgment.
    #[error("Acknowledgment error: {message}")]
    AcknowledgmentError {
        /// Error message
        message: String,
        /// Optional source error
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Schema validation error.
    #[cfg(feature = "schema")]
    #[error("Schema validation error: {message}")]
    SchemaValidationError {
        /// Error message
        message: String,
        /// Schema ID that failed validation
        schema_id: Option<String>,
    },

    /// Schema encoding error.
    #[cfg(feature = "schema")]
    #[error("Schema encoding error: {message}")]
    SchemaEncodingError {
        /// Error message
        message: String,
        /// Schema format
        format: SchemaFormat,
    },

    /// Schema decoding error.
    #[cfg(feature = "schema")]
    #[error("Schema decoding error: {message}")]
    SchemaDecodingError {
        /// Error message
        message: String,
        /// Schema format
        format: SchemaFormat,
    },

    /// Batching error.
    #[error("Batching error: {message}")]
    BatchingError {
        /// Error message
        message: String,
        /// Number of messages in failed batch
        batch_size: usize,
    },

    /// Flow control error.
    #[error("Flow control error: {message}")]
    FlowControlError {
        /// Error message
        message: String,
        /// Current message count
        current_count: usize,
        /// Maximum allowed count
        max_count: usize,
    },

    /// Dead letter queue error.
    #[error("Dead letter queue error: {message}")]
    DeadLetterQueueError {
        /// Error message
        message: String,
        /// Message ID that failed
        message_id: String,
    },

    /// Ordering key error.
    #[error("Ordering key error: {message}")]
    OrderingKeyError {
        /// Error message
        message: String,
        /// Ordering key that caused the error
        ordering_key: String,
    },

    /// Monitoring error.
    #[cfg(feature = "monitoring")]
    #[error("Monitoring error: {message}")]
    MonitoringError {
        /// Error message
        message: String,
        /// Optional source error
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Authentication error.
    #[error("Authentication error: {message}")]
    AuthenticationError {
        /// Error message
        message: String,
        /// Optional source error
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Configuration error.
    #[error("Configuration error: {message}")]
    ConfigurationError {
        /// Error message
        message: String,
        /// Parameter name
        parameter: String,
    },

    /// Topic not found error.
    #[error("Topic not found: {topic_name}")]
    TopicNotFound {
        /// Topic name
        topic_name: String,
    },

    /// Subscription not found error.
    #[error("Subscription not found: {subscription_name}")]
    SubscriptionNotFound {
        /// Subscription name
        subscription_name: String,
    },

    /// Message too large error.
    #[error("Message too large: {size} bytes (max: {max_size} bytes)")]
    MessageTooLarge {
        /// Actual message size
        size: usize,
        /// Maximum allowed size
        max_size: usize,
    },

    /// Invalid message format.
    #[error("Invalid message format: {message}")]
    InvalidMessageFormat {
        /// Error message
        message: String,
    },

    /// Timeout error.
    #[error("Operation timed out after {duration_ms}ms")]
    Timeout {
        /// Timeout duration in milliseconds
        duration_ms: u64,
    },

    /// Network error.
    #[error("Network error: {message}")]
    NetworkError {
        /// Error message
        message: String,
        /// Optional source error
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Resource exhausted error.
    #[error("Resource exhausted: {resource}")]
    ResourceExhausted {
        /// Resource name
        resource: String,
        /// Optional retry after duration in seconds
        retry_after: Option<u64>,
    },

    /// Permission denied error.
    #[error("Permission denied: {operation}")]
    PermissionDenied {
        /// Operation that was denied
        operation: String,
    },

    /// Internal error.
    #[error("Internal error: {message}")]
    InternalError {
        /// Error message
        message: String,
    },

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization/deserialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Google Cloud Pub/Sub client error.
    #[error("Pub/Sub client error: {0}")]
    ClientError(String),
}

/// Schema format types.
#[cfg(feature = "schema")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaFormat {
    /// Apache Avro format
    Avro,
    /// Protocol Buffers format
    Protobuf,
}

#[cfg(feature = "schema")]
impl fmt::Display for SchemaFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SchemaFormat::Avro => write!(f, "Avro"),
            SchemaFormat::Protobuf => write!(f, "Protobuf"),
        }
    }
}

impl PubSubError {
    /// Creates a publish error from a message.
    pub fn publish<S: Into<String>>(message: S) -> Self {
        Self::PublishError {
            message: message.into(),
            source: None,
        }
    }

    /// Creates a publish error with a source error.
    pub fn publish_with_source<S: Into<String>>(
        message: S,
        source: Box<dyn std::error::Error + Send + Sync>,
    ) -> Self {
        Self::PublishError {
            message: message.into(),
            source: Some(source),
        }
    }

    /// Creates a subscription error from a message.
    pub fn subscription<S: Into<String>>(message: S) -> Self {
        Self::SubscriptionError {
            message: message.into(),
            source: None,
        }
    }

    /// Creates a subscription error with a source error.
    pub fn subscription_with_source<S: Into<String>>(
        message: S,
        source: Box<dyn std::error::Error + Send + Sync>,
    ) -> Self {
        Self::SubscriptionError {
            message: message.into(),
            source: Some(source),
        }
    }

    /// Creates an acknowledgment error from a message.
    pub fn acknowledgment<S: Into<String>>(message: S) -> Self {
        Self::AcknowledgmentError {
            message: message.into(),
            source: None,
        }
    }

    /// Creates a configuration error.
    pub fn configuration<S: Into<String>, P: Into<String>>(message: S, parameter: P) -> Self {
        Self::ConfigurationError {
            message: message.into(),
            parameter: parameter.into(),
        }
    }

    /// Creates a batching error.
    pub fn batching<S: Into<String>>(message: S, batch_size: usize) -> Self {
        Self::BatchingError {
            message: message.into(),
            batch_size,
        }
    }

    /// Creates a flow control error.
    pub fn flow_control<S: Into<String>>(
        message: S,
        current_count: usize,
        max_count: usize,
    ) -> Self {
        Self::FlowControlError {
            message: message.into(),
            current_count,
            max_count,
        }
    }

    /// Creates a dead letter queue error.
    pub fn dead_letter<S: Into<String>, M: Into<String>>(message: S, message_id: M) -> Self {
        Self::DeadLetterQueueError {
            message: message.into(),
            message_id: message_id.into(),
        }
    }

    /// Creates an ordering key error.
    pub fn ordering_key<S: Into<String>, K: Into<String>>(message: S, ordering_key: K) -> Self {
        Self::OrderingKeyError {
            message: message.into(),
            ordering_key: ordering_key.into(),
        }
    }

    /// Creates a topic not found error.
    pub fn topic_not_found<S: Into<String>>(topic_name: S) -> Self {
        Self::TopicNotFound {
            topic_name: topic_name.into(),
        }
    }

    /// Creates a subscription not found error.
    pub fn subscription_not_found<S: Into<String>>(subscription_name: S) -> Self {
        Self::SubscriptionNotFound {
            subscription_name: subscription_name.into(),
        }
    }

    /// Creates a message too large error.
    pub fn message_too_large(size: usize, max_size: usize) -> Self {
        Self::MessageTooLarge { size, max_size }
    }

    /// Creates a timeout error.
    pub fn timeout(duration_ms: u64) -> Self {
        Self::Timeout { duration_ms }
    }

    /// Creates a resource exhausted error.
    pub fn resource_exhausted<S: Into<String>>(resource: S, retry_after: Option<u64>) -> Self {
        Self::ResourceExhausted {
            resource: resource.into(),
            retry_after,
        }
    }

    /// Creates a permission denied error.
    pub fn permission_denied<S: Into<String>>(operation: S) -> Self {
        Self::PermissionDenied {
            operation: operation.into(),
        }
    }

    /// Checks if the error is retryable.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::NetworkError { .. }
                | Self::ResourceExhausted { .. }
                | Self::Timeout { .. }
                | Self::InternalError { .. }
        )
    }

    /// Gets the retry after duration if available.
    pub fn retry_after(&self) -> Option<u64> {
        match self {
            Self::ResourceExhausted { retry_after, .. } => *retry_after,
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_publish_error_creation() {
        let error = PubSubError::publish("test error");
        assert!(matches!(error, PubSubError::PublishError { .. }));
        assert!(error.to_string().contains("test error"));
    }

    #[test]
    fn test_subscription_error_creation() {
        let error = PubSubError::subscription("test error");
        assert!(matches!(error, PubSubError::SubscriptionError { .. }));
        assert!(error.to_string().contains("test error"));
    }

    #[test]
    fn test_configuration_error() {
        let error = PubSubError::configuration("invalid value", "timeout");
        assert!(matches!(error, PubSubError::ConfigurationError { .. }));
        assert!(error.to_string().contains("invalid value"));
    }

    #[test]
    fn test_message_too_large_error() {
        let error = PubSubError::message_too_large(11000000, 10000000);
        assert!(matches!(error, PubSubError::MessageTooLarge { .. }));
        assert!(error.to_string().contains("11000000"));
        assert!(error.to_string().contains("10000000"));
    }

    #[test]
    fn test_retryable_errors() {
        let network_error = PubSubError::NetworkError {
            message: "connection reset".to_string(),
            source: None,
        };
        assert!(network_error.is_retryable());

        let timeout_error = PubSubError::timeout(5000);
        assert!(timeout_error.is_retryable());

        let config_error = PubSubError::configuration("bad value", "param");
        assert!(!config_error.is_retryable());
    }

    #[test]
    fn test_retry_after() {
        let error = PubSubError::resource_exhausted("quota", Some(60));
        assert_eq!(error.retry_after(), Some(60));

        let error = PubSubError::timeout(1000);
        assert_eq!(error.retry_after(), None);
    }

    #[test]
    fn test_topic_not_found() {
        let error = PubSubError::topic_not_found("my-topic");
        assert!(matches!(error, PubSubError::TopicNotFound { .. }));
        assert!(error.to_string().contains("my-topic"));
    }

    #[test]
    fn test_flow_control_error() {
        let error = PubSubError::flow_control("limit exceeded", 1000, 500);
        assert!(matches!(error, PubSubError::FlowControlError { .. }));
        assert!(error.to_string().contains("limit exceeded"));
    }
}
