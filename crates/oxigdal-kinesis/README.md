# oxigdal-kinesis

[![Crates.io](https://img.shields.io/crates/v/oxigdal-kinesis.svg)](https://crates.io/crates/oxigdal-kinesis)
[![Documentation](https://docs.rs/oxigdal-kinesis/badge.svg)](https://docs.rs/oxigdal-kinesis)
[![License](https://img.shields.io/crates/l/oxigdal-kinesis.svg)](LICENSE)

AWS Kinesis streaming integration for OxiGDAL. Provides comprehensive support for real-time data streaming with Amazon Kinesis, including Data Streams, Firehose delivery, SQL analytics, and CloudWatch monitoring.

## Features

- **Kinesis Data Streams**: Producer with KPL-inspired patterns, enhanced fan-out consumer, shard management, and DynamoDB checkpointing for reliable stream processing
- **Kinesis Firehose**: Delivery streams with data transformation, multiple destination support (S3, Redshift, Elasticsearch), and batch buffering
- **Kinesis Analytics**: SQL query builder for real-time stream analytics with support for tumbling, sliding, and session windows
- **Monitoring**: CloudWatch metrics integration, stream performance monitoring, and alerting system
- **Data Compression**: Optional support for GZIP and Zstandard compression for efficient data transfer
- **Async-First**: Built on Tokio for high-performance, non-blocking stream processing
- **Pure Rust**: 100% Pure Rust implementation with no C/Fortran dependencies
- **Error Handling**: Comprehensive error types with proper Result types throughout

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
oxigdal-kinesis = "0.1"

# With all default features (streams, firehose, analytics, monitoring)
# oxigdal-kinesis = { version = "0.1", features = ["streams", "firehose", "analytics", "monitoring"] }

# With optional features
# oxigdal-kinesis = { version = "0.1", features = ["streams", "checkpointing", "enhanced-fanout", "compression"] }
```

### Minimum Supported Rust Version

Requires Rust 1.85 or later.

## Quick Start

### Kinesis Data Streams - Producer

```rust
use oxigdal_kinesis::streams::{Producer, ProducerConfig, Record};
use bytes::Bytes;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load AWS credentials from environment
    let config = aws_config::load_from_env().await;
    let client = aws_sdk_kinesis::Client::new(&config);

    // Configure producer with buffering and lingering
    let producer_config = ProducerConfig::new("my-stream")
        .with_buffer_size(1000)
        .with_linger_ms(100);

    let producer = Producer::new(client, producer_config).await?;

    // Send individual record
    let record = Record::new("partition-key-1", Bytes::from("sensor-data"));
    producer.send(record).await?;

    // Flush pending records to ensure delivery
    producer.flush().await?;

    Ok(())
}
```

### Kinesis Data Streams - Consumer

```rust
use oxigdal_kinesis::streams::{Consumer, ConsumerConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = aws_config::load_from_env().await;
    let client = aws_sdk_kinesis::Client::new(&config);

    let consumer_config = ConsumerConfig::new("my-stream")
        .with_max_records(100);

    let mut consumer = Consumer::new(client, consumer_config, "shard-0001").await?;

    // Poll stream for records
    loop {
        let records = consumer.poll().await?;
        for record in records {
            println!("Received: {:?}", String::from_utf8_lossy(&record.data));
        }
    }
}
```

### Kinesis Firehose

```rust
use oxigdal_kinesis::firehose::{DeliveryStream, DeliveryStreamConfig, FirehoseRecord};
use oxigdal_kinesis::firehose::destination::S3DestinationConfig;
use bytes::Bytes;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = aws_config::load_from_env().await;
    let client = aws_sdk_firehose::Client::new(&config);

    // Configure S3 destination
    let s3_config = S3DestinationConfig::new(
        "arn:aws:s3:::my-bucket",
        "arn:aws:iam::123456789012:role/firehose-role",
        "data/",
    );

    let stream_config = DeliveryStreamConfig::new("my-delivery-stream")
        .with_s3_destination(s3_config);

    let mut delivery_stream = DeliveryStream::new(client, stream_config);
    delivery_stream.start().await?;

    // Send records which are automatically batched and delivered to S3
    let record = FirehoseRecord::new(Bytes::from("log entry"));
    delivery_stream.send_record(record).await?;

    Ok(())
}
```

### Kinesis Analytics - SQL Queries

```rust
use oxigdal_kinesis::analytics::sql::QueryBuilder;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Build real-time analytics query
    let query = QueryBuilder::new()
        .select("userId")
        .select("COUNT(*) as event_count")
        .from("SOURCE_SQL_STREAM")
        .window("WINDOW TUMBLING (SIZE 1 MINUTE)")
        .group_by("userId")
        .build();

    println!("Query:\n{}", query.as_str());
    // Outputs: SELECT userId, COUNT(*) as event_count
    //          FROM SOURCE_SQL_STREAM
    //          WINDOW TUMBLING (SIZE 1 MINUTE)
    //          GROUP BY userId

    Ok(())
}
```

### Monitoring with CloudWatch

```rust
use oxigdal_kinesis::monitoring::{MetricsCollector, MetricConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = aws_config::load_from_env().await;
    let cloudwatch_client = aws_sdk_cloudwatch::Client::new(&config);

    let metric_config = MetricConfig::new("my-stream-metrics");
    let collector = MetricsCollector::new(cloudwatch_client, metric_config);

    // Track stream metrics
    collector.record_put_records(records_sent, bytes_sent).await?;
    collector.record_get_records(records_received, latency_ms).await?;

    Ok(())
}
```

## Usage

### Basic Producer Pattern

```rust
use oxigdal_kinesis::streams::{Producer, ProducerConfig, Record};
use bytes::Bytes;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = aws_config::load_from_env().await;
    let client = aws_sdk_kinesis::Client::new(&config);

    let producer = Producer::new(
        client,
        ProducerConfig::new("events-stream")
    ).await?;

    // Send with same partition key for ordering guarantee
    for i in 0..100 {
        let record = Record::new(
            "user-123",  // Partition key ensures ordering
            Bytes::from(format!("event {}", i))
        );
        producer.send(record).await?;
    }

    producer.flush().await?;
    Ok(())
}
```

### Enhanced Fan-Out Consumer

```rust
#[cfg(feature = "enhanced-fanout")]
use oxigdal_kinesis::streams::EnhancedFanOutConsumer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = aws_config::load_from_env().await;
    let client = aws_sdk_kinesis::Client::new(&config);

    let mut consumer = EnhancedFanOutConsumer::new(
        client,
        "my-stream",
        "shard-0001"
    ).await?;

    // Enhanced fan-out provides lower latency and higher throughput
    loop {
        let records = consumer.poll().await?;
        for record in records {
            println!("Record: {:?}", record);
        }
    }
}
```

### DynamoDB Checkpointing

```rust
#[cfg(feature = "checkpointing")]
use oxigdal_kinesis::streams::DynamoDbCheckpointStore;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = aws_config::load_from_env().await;
    let dynamodb_client = aws_sdk_dynamodb::Client::new(&config);

    let checkpoint_store = DynamoDbCheckpointStore::new(
        dynamodb_client,
        "kinesis-checkpoints"  // DynamoDB table name
    );

    // Save checkpoint after processing records
    checkpoint_store.save_checkpoint(
        "shard-0001",
        "49590338271490256608559692538361571095921575989136588898"
    ).await?;

    Ok(())
}
```

### Data Compression

```rust
#[cfg(feature = "compression")]
use oxigdal_kinesis::streams::ProducerConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = aws_config::load_from_env().await;
    let client = aws_sdk_kinesis::Client::new(&config);

    let producer = Producer::new(
        client,
        ProducerConfig::new("my-stream")
            .with_compression_enabled(true)
    ).await?;

    // Records are automatically compressed before sending
    Ok(())
}
```

### Firehose Data Transformation

```rust
use oxigdal_kinesis::firehose::{DeliveryStream, DeliveryStreamConfig, Transformer};
use oxigdal_kinesis::firehose::transform::TransformResult;

struct JsonTransformer;

#[async_trait::async_trait]
impl Transformer for JsonTransformer {
    async fn transform(&self, data: &[u8]) -> Result<TransformResult, Box<dyn std::error::Error>> {
        // Parse, validate, and transform data
        let json: serde_json::Value = serde_json::from_slice(data)?;
        let transformed = serde_json::json!({
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "data": json
        });

        Ok(TransformResult::success(
            serde_json::to_vec(&transformed)?.into()
        ))
    }
}
```

## API Overview

### Kinesis Data Streams

| Module | Type | Description |
|--------|------|-------------|
| `streams::Producer` | Struct | High-performance record producer with buffering |
| `streams::Consumer` | Struct | Stream consumer for reliable record retrieval |
| `streams::EnhancedFanOutConsumer` | Struct | Low-latency fan-out consumer (feature: `enhanced-fanout`) |
| `streams::ShardManager` | Struct | Shard discovery and management |
| `streams::Checkpointer` | Trait | Checkpoint persistence for fault tolerance |
| `streams::DynamoDbCheckpointStore` | Struct | DynamoDB-backed checkpoint storage (feature: `checkpointing`) |

### Kinesis Firehose

| Module | Type | Description |
|--------|------|-------------|
| `firehose::DeliveryStream` | Struct | Delivery stream management |
| `firehose::S3Destination` | Struct | S3 delivery destination |
| `firehose::Transformer` | Trait | Record transformation interface |
| `firehose::LambdaTransformer` | Struct | Lambda function-based transformation |

### Kinesis Analytics

| Module | Type | Description |
|--------|------|-------------|
| `analytics::QueryBuilder` | Struct | SQL query construction |
| `analytics::sql::Query` | Struct | Compiled analytics query |
| `analytics::window::TumblingWindow` | Struct | Fixed-time window |
| `analytics::window::SlidingWindow` | Struct | Overlapping window |
| `analytics::window::SessionWindow` | Struct | Event-driven window |

### Monitoring

| Module | Type | Description |
|--------|------|-------------|
| `monitoring::MetricsCollector` | Struct | CloudWatch metrics publisher |
| `monitoring::StreamMetrics` | Struct | Stream-level metrics |
| `monitoring::ShardMetrics` | Struct | Shard-level metrics |

## Features

### Default Features

- `std` - Standard library support
- `streams` - Kinesis Data Streams support
- `firehose` - Kinesis Firehose support
- `analytics` - Kinesis Analytics support
- `monitoring` - CloudWatch monitoring

### Optional Features

- `checkpointing` - DynamoDB checkpoint storage for consumers
- `enhanced-fanout` - Enhanced fan-out consumer support
- `compression` - Data compression (GZIP/Zstandard)
- `alloc` - Allocator support for no_std environments

## Performance Characteristics

### Producer

- **Throughput**: Efficient batching with configurable buffer sizes (default: 1000 records)
- **Latency**: Configurable linger time (default: 100ms) for batch optimization
- **Memory**: Bounded by buffer size configuration

### Consumer

- **Latency**: Enhanced fan-out provides sub-1s latency (vs 4-5s with standard consumer)
- **Throughput**: Scales with shard count
- **Checkpointing**: DynamoDB integration for stateful processing

### Firehose

- **Batching**: Automatic record batching for efficient delivery
- **Transformation**: Optional Lambda or inline transformation
- **Destinations**: S3, Redshift, Elasticsearch support

## Error Handling

This library follows the "no unwrap" policy. All fallible operations return `Result<T, E>` with descriptive error types:

```rust
use oxigdal_kinesis::Result;

// All operations return Result<T, KinesisError>
let producer: Result<Producer> = Producer::new(client, config).await;
let records: Result<Vec<Record>> = consumer.poll().await;
```

## Pure Rust Implementation

This library is 100% Pure Rust with no C/Fortran dependencies. All AWS interactions use the AWS SDK for Rust, and all data structures are implemented in pure Rust.

## Examples

For more comprehensive examples, see the [examples](examples/) directory:

- `producer_simple.rs` - Basic producer usage
- `consumer_simple.rs` - Basic consumer usage
- `firehose_delivery.rs` - Firehose delivery setup
- `analytics_query.rs` - Building analytics queries
- `monitoring_metrics.rs` - CloudWatch metrics integration
- `checkpoint_recovery.rs` - Stateful processing with checkpoints

## Documentation

Full API documentation is available at [docs.rs/oxigdal-kinesis](https://docs.rs/oxigdal-kinesis).

Additional resources:

- [AWS Kinesis Documentation](https://docs.aws.amazon.com/kinesis/)
- [OxiGDAL Documentation](https://docs.rs/oxigdal)
- [CONTRIBUTING.md](../../CONTRIBUTING.md) - Contribution guidelines

## Contributing

Contributions are welcome! Please read the [CONTRIBUTING.md](../../CONTRIBUTING.md) guidelines before submitting PRs.

This project follows COOLJAPAN ecosystem policies:

- Pure Rust implementation (no C/Fortran dependencies by default)
- No unwrap() in production code
- Comprehensive error handling
- Feature-gated optional dependencies

## License

Licensed under the Apache License, Version 2.0.

See [LICENSE](../../LICENSE) for details.

## Related Projects

- [OxiGDAL](https://github.com/cool-japan/oxigdal) - Geospatial data processing library
- [OxiGDAL Cloud](https://github.com/cool-japan/oxigdal/tree/main/crates/oxigdal-cloud) - Cloud service integration
- [OxiGDAL Analytics](https://github.com/cool-japan/oxigdal/tree/main/crates/oxigdal-analytics) - Analytics framework
- [AWS SDK for Rust](https://github.com/awslabs/aws-sdk-rust) - Official AWS SDK

---

Part of the [COOLJAPAN](https://github.com/cool-japan) ecosystem. Built by Team Kitasan.
