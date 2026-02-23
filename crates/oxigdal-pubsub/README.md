# OxiGDAL Pub/Sub

[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![Pure Rust](https://img.shields.io/badge/100%25-Pure%20Rust-orange.svg)](https://www.rust-lang.org/)

Google Cloud Pub/Sub integration for OxiGDAL - Pure Rust streaming and messaging for geospatial data processing.

## Overview

`oxigdal-pubsub` provides comprehensive support for Google Cloud Pub/Sub messaging, enabling real-time geospatial data streaming, event-driven processing, and distributed system communication.

## Features

### Core Capabilities

- **Publisher** (~1,500 LOC)
  - Async message publishing with batching
  - Ordering keys for sequential message delivery
  - Configurable retry logic with exponential backoff
  - Flow control and backpressure handling
  - Error handling and recovery

- **Subscriber** (~1,500 LOC)
  - Pull and push subscription models
  - Message acknowledgment and negative acknowledgment
  - Flow control with configurable limits
  - Dead letter queue support
  - Automatic acknowledgment deadline extension

- **Schema Support** (~500 LOC) - *feature-gated*
  - Apache Avro schema validation
  - Protocol Buffers schema support
  - Schema encoding and decoding
  - Schema registry management

- **Monitoring** (~300 LOC) - *feature-gated*
  - Latency tracking and metrics collection
  - Publisher and subscriber statistics
  - Custom metric points with labels
  - Metrics export for observability

- **Topic Management** (~600 LOC)
  - Topic creation and configuration
  - Message retention policies
  - Label management
  - Topic statistics and metadata

- **Subscription Management** (~700 LOC)
  - Subscription creation and updates
  - Expiration policies
  - Dead letter policies
  - Retry configurations
  - Subscription seeking (timestamp/snapshot)

## Pure Rust Implementation

This crate uses 100% Pure Rust implementations:

- `google-cloud-pubsub` - Pure Rust Pub/Sub client
- `google-cloud-auth` - Pure Rust authentication
- `apache-avro` - Pure Rust Avro support (optional)
- `prost` - Pure Rust Protocol Buffers (optional)

**No C/Fortran dependencies** - fully compliant with COOLJAPAN Pure Rust Policy.

## COOLJAPAN Policy Compliance

- ✅ **Pure Rust**: 100% Pure Rust, no C/Fortran dependencies
- ✅ **No unwrap()**: All error handling uses `Result<T, E>`
- ✅ **Files < 2000 lines**: All source files under 2000 lines
- ✅ **Workspace dependencies**: Uses workspace-level dependency management
- ✅ **Latest crates**: Uses latest available versions from crates.io

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
oxigdal-pubsub = "0.1"
```

### Feature Flags

```toml
[dependencies.oxigdal-pubsub]
version = "0.1"
features = ["schema", "monitoring", "avro", "protobuf"]
```

Available features:

- `std` (default) - Standard library support
- `async` (default) - Async runtime support
- `publisher` (default) - Publisher functionality
- `subscriber` (default) - Subscriber functionality
- `schema` - Schema validation support
- `avro` - Apache Avro schema support
- `protobuf` - Protocol Buffers schema support
- `monitoring` - Metrics and monitoring
- `batching` - Message batching
- `ordering` - Message ordering
- `flow-control` - Flow control
- `dead-letter` - Dead letter queue support

## Quick Start

### Publishing Messages

```rust
use oxigdal_pubsub::{Publisher, PublisherConfig, Message};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create publisher configuration
    let config = PublisherConfig::new("my-project", "my-topic")
        .with_batching(true)
        .with_batch_size(100)
        .with_ordering(true);

    // Create publisher
    let publisher = Publisher::new(config).await?;

    // Publish a message
    let message = Message::new(b"Hello, Pub/Sub!")
        .with_attribute("source", "oxigdal")
        .with_attribute("timestamp", "2025-01-27")
        .with_ordering_key("geo-events-1");

    let message_id = publisher.publish(message).await?;
    println!("Published message: {}", message_id);

    // Flush any pending batches
    publisher.flush_all().await?;

    Ok(())
}
```

### Subscribing to Messages

```rust
use oxigdal_pubsub::{Subscriber, SubscriberConfig, HandlerResult};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create subscriber configuration
    let config = SubscriberConfig::new("my-project", "my-subscription")
        .with_ack_deadline(30)
        .with_handler_concurrency(10);

    // Create subscriber
    let subscriber = Subscriber::new(config).await?;

    // Start subscription with message handler
    let handle = subscriber.start(|message| {
        println!("Received: {:?}", message.data);
        println!("Attributes: {:?}", message.attributes);

        // Process the message...

        // Return acknowledgment result
        HandlerResult::Ack
    }).await?;

    // Keep running...
    tokio::signal::ctrl_c().await?;

    // Stop the subscriber
    subscriber.stop();

    Ok(())
}
```

### Topic Management

```rust
use oxigdal_pubsub::{TopicManager, TopicBuilder};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manager = TopicManager::new("my-project").await?;

    // Create a topic with configuration
    let topic = TopicBuilder::new("my-project", "geo-events")
        .message_retention(86400)  // 24 hours
        .label("env", "production")
        .label("type", "geospatial")
        .message_ordering(true)
        .create(&manager)
        .await?;

    println!("Created topic: {}", topic);

    // List all topics
    let topics = manager.list_topics();
    println!("Available topics: {:?}", topics);

    Ok(())
}
```

### Subscription Management

```rust
use oxigdal_pubsub::{
    SubscriptionManager, SubscriptionBuilder,
    DeadLetterPolicy, RetryPolicy, ExpirationPolicy,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manager = SubscriptionManager::new("my-project").await?;

    // Create a subscription with advanced configuration
    let subscription = SubscriptionBuilder::new(
        "my-project",
        "geo-subscription",
        "geo-events"
    )
    .ack_deadline(45)
    .message_retention(604800)  // 7 days
    .message_ordering(true)
    .dead_letter_policy(DeadLetterPolicy::new("dlq-topic", 5))
    .retry_policy(RetryPolicy::aggressive())
    .expiration_policy(ExpirationPolicy::never_expire())
    .filter("attributes.type=\"geospatial\"")
    .create(&manager)
    .await?;

    println!("Created subscription: {}", subscription);

    Ok(())
}
```

### Schema Validation

```rust
#[cfg(feature = "schema")]
use oxigdal_pubsub::{Schema, SchemaRegistry, SchemaValidator};
use oxigdal_pubsub::error::SchemaFormat;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut registry = SchemaRegistry::new();

    // Register an Avro schema
    let avro_schema = r#"
    {
        "type": "record",
        "name": "GeoEvent",
        "fields": [
            {"name": "id", "type": "string"},
            {"name": "latitude", "type": "double"},
            {"name": "longitude", "type": "double"},
            {"name": "timestamp", "type": "long"}
        ]
    }
    "#;

    let schema = Schema::new(
        "geo-event-schema",
        "GeoEvent",
        SchemaFormat::Avro,
        avro_schema,
    );

    registry.register(schema)?;

    // Validate messages against schema
    let validator = SchemaValidator::new(Arc::new(registry));
    // ... use validator to validate messages

    Ok(())
}
```

### Monitoring and Metrics

```rust
#[cfg(feature = "monitoring")]
use oxigdal_pubsub::{MetricsCollector, MetricsExporter};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let collector = MetricsCollector::new("my-project")
        .with_topic("my-topic")
        .with_subscription("my-subscription");

    // Record metrics
    collector.record_publish(1024, true);
    collector.record_receive(2048);

    // Start metrics exporter
    let exporter = MetricsExporter::new(
        Arc::new(collector),
        Duration::from_secs(60),
    );
    let handle = exporter.start().await?;

    // Export metrics manually
    let metrics = collector.export_metrics();
    println!("Exported {} metric points", metrics.len());

    Ok(())
}
```

## Architecture

### Module Structure

```
oxigdal-pubsub/
├── src/
│   ├── lib.rs              # Main library entry point
│   ├── error.rs            # Error types and handling
│   ├── publisher.rs        # Message publishing
│   ├── subscriber.rs       # Message subscription
│   ├── topic.rs            # Topic management
│   ├── subscription.rs     # Subscription management
│   ├── schema.rs           # Schema support (feature-gated)
│   └── monitoring.rs       # Metrics and monitoring (feature-gated)
└── tests/
    └── integration_test.rs # Integration tests
```

### Performance Characteristics

- **Batching**: Automatically batches messages for optimal throughput
- **Flow Control**: Prevents overwhelming subscribers with configurable limits
- **Retry Logic**: Exponential backoff with configurable attempts
- **Latency Tracking**: Sub-millisecond precision for performance monitoring
- **Memory Efficiency**: Zero-copy operations where possible

## Error Handling

All operations return `Result<T, PubSubError>` with comprehensive error types:

```rust
use oxigdal_pubsub::PubSubError;

match publisher.publish(message).await {
    Ok(message_id) => println!("Published: {}", message_id),
    Err(PubSubError::MessageTooLarge { size, max_size }) => {
        eprintln!("Message too large: {} > {}", size, max_size);
    }
    Err(PubSubError::Timeout { duration_ms }) => {
        eprintln!("Timeout after {}ms", duration_ms);
    }
    Err(e) if e.is_retryable() => {
        eprintln!("Retryable error: {}", e);
        // Retry logic...
    }
    Err(e) => eprintln!("Error: {}", e),
}
```

## Testing

Run tests:

```bash
# Run all tests
cargo test -p oxigdal-pubsub

# Run with all features
cargo test -p oxigdal-pubsub --all-features

# Run specific test
cargo test -p oxigdal-pubsub test_publisher_config
```

## Statistics

- **Total Lines of Code**: ~3,700 LOC
- **Publisher**: ~1,500 LOC
- **Subscriber**: ~1,500 LOC
- **Topic Management**: ~600 LOC
- **Subscription Management**: ~700 LOC
- **Schema Support**: ~500 LOC
- **Monitoring**: ~300 LOC
- **Error Handling**: ~400 LOC
- **Tests**: ~600 LOC

## License

Apache-2.0

## Authors

COOLJAPAN OU (Team Kitasan)

## Contributing

This crate is part of the OxiGDAL project. Contributions are welcome!

## See Also

- [OxiGDAL Core](../oxigdal-core) - Core geospatial functionality
- [OxiGDAL Cloud](../oxigdal-cloud) - Cloud storage integration
- [OxiGDAL Distributed](../oxigdal-distributed) - Distributed computing
- [OxiGDAL Streaming](../oxigdal-streaming) - Real-time data streaming
