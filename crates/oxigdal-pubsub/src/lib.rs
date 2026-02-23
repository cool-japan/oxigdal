//! OxiGDAL Pub/Sub - Google Cloud Pub/Sub integration for OxiGDAL.
//!
//! This crate provides comprehensive support for Google Cloud Pub/Sub messaging,
//! including publishing, subscribing, schema validation, and monitoring capabilities.
//!
//! # Features
//!
//! - **Publisher**: Async message publishing with batching and ordering keys
//! - **Subscriber**: Pull and push subscriptions with flow control
//! - **Schema Support**: Avro and Protobuf schema validation (feature-gated)
//! - **Monitoring**: Cloud Monitoring integration for metrics and observability
//! - **Dead Letter Queues**: Automatic handling of failed messages
//! - **Flow Control**: Intelligent message throttling and backpressure
//!
//! # Example: Publishing Messages
//!
//! ```no_run
//! use oxigdal_pubsub::{Publisher, PublisherConfig, Message};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = PublisherConfig::new("my-project", "my-topic")
//!     .with_batching(true)
//!     .with_batch_size(100);
//!
//! let publisher = Publisher::new(config).await?;
//!
//! let message = Message::new(b"Hello, Pub/Sub!".to_vec())
//!     .with_attribute("source", "oxigdal")
//!     .with_ordering_key("order-1");
//!
//! let message_id = publisher.publish(message).await?;
//! println!("Published message: {}", message_id);
//! # Ok(())
//! # }
//! ```
//!
//! # Example: Subscribing to Messages
//!
//! ```no_run
//! use oxigdal_pubsub::{Subscriber, SubscriberConfig, HandlerResult};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = SubscriberConfig::new("my-project", "my-subscription")
//!     .with_ack_deadline(30);
//!
//! let subscriber = Subscriber::new(config).await?;
//!
//! let handle = subscriber.start(|message| {
//!     println!("Received: {:?}", message.data);
//!     HandlerResult::Ack
//! }).await?;
//!
//! // Wait for shutdown signal...
//! subscriber.stop();
//! # Ok(())
//! # }
//! ```
//!
//! # Feature Flags
//!
//! - `std` (default): Enable standard library support
//! - `async` (default): Enable async runtime support
//! - `publisher` (default): Enable publisher functionality
//! - `subscriber` (default): Enable subscriber functionality
//! - `schema`: Enable schema support
//! - `avro`: Enable Apache Avro schema support
//! - `protobuf`: Enable Protocol Buffers schema support
//! - `monitoring`: Enable Cloud Monitoring integration
//! - `batching`: Enable message batching
//! - `ordering`: Enable message ordering
//! - `flow-control`: Enable flow control
//! - `dead-letter`: Enable dead letter queue support
//!
//! # Pure Rust Implementation
//!
//! This crate uses Pure Rust implementations for all functionality:
//! - `google-cloud-pubsub` for Pub/Sub operations
//! - `google-cloud-auth` for authentication
//! - `google-cloud-monitoring` for monitoring (optional)
//! - `apache-avro` for Avro schema support (optional)
//! - `prost` for Protocol Buffers support (optional)
//!
//! # COOLJAPAN Policy Compliance
//!
//! - ✅ Pure Rust (no C/Fortran dependencies)
//! - ✅ No `unwrap()` usage (proper error handling)
//! - ✅ Files under 2000 lines (modular design)
//! - ✅ Workspace dependencies

#![deny(missing_docs)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::panic)]
#![warn(clippy::expect_used)]
#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod error;

#[cfg(feature = "publisher")]
pub mod publisher;

#[cfg(feature = "subscriber")]
pub mod subscriber;

#[cfg(feature = "schema")]
pub mod schema;

#[cfg(feature = "monitoring")]
pub mod monitoring;

pub mod topic;

pub mod subscription;

// Re-exports for convenience
pub use error::{PubSubError, Result};

#[cfg(feature = "publisher")]
pub use publisher::{
    DEFAULT_BATCH_SIZE, DEFAULT_BATCH_TIMEOUT_MS, DEFAULT_MAX_OUTSTANDING_PUBLISHES,
    MAX_MESSAGE_SIZE, Message, Publisher, PublisherConfig, PublisherStats, RetryConfig,
};

#[cfg(feature = "subscriber")]
pub use subscriber::{
    DEFAULT_ACK_DEADLINE_SECONDS, DEFAULT_HANDLER_CONCURRENCY, DEFAULT_MAX_OUTSTANDING_BYTES,
    DEFAULT_MAX_OUTSTANDING_MESSAGES, DeadLetterConfig, FlowControlSettings, HandlerResult,
    ReceivedMessage, Subscriber, SubscriberConfig, SubscriberStats, SubscriptionType,
};

#[cfg(feature = "schema")]
pub use schema::{Schema, SchemaEncoding, SchemaRegistry, SchemaValidator};

#[cfg(all(feature = "schema", feature = "avro"))]
pub use schema::AvroSchema;

#[cfg(all(feature = "schema", feature = "protobuf"))]
pub use schema::ProtobufSchema;

#[cfg(feature = "monitoring")]
pub use monitoring::{
    LatencyTracker, MetricPoint, MetricType, MetricValue, MetricsCollector, MetricsExporter,
    OperationTimer, PublisherMetrics, SubscriberMetrics,
};

pub use topic::{TopicBuilder, TopicConfig, TopicManager, TopicMetadata, TopicStats};

#[cfg(feature = "schema")]
pub use topic::SchemaSettings;

pub use subscription::{
    DeadLetterPolicy, ExpirationPolicy, RetryPolicy, SubscriptionBuilder, SubscriptionCreateConfig,
    SubscriptionManager, SubscriptionMetadata, SubscriptionStats,
};

/// Crate version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Crate name.
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");

/// Gets the crate version.
pub fn version() -> &'static str {
    VERSION
}

/// Gets the crate name.
pub fn crate_name() -> &'static str {
    CRATE_NAME
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!version().is_empty());
    }

    #[test]
    fn test_crate_name() {
        assert_eq!(crate_name(), "oxigdal-pubsub");
    }
}
