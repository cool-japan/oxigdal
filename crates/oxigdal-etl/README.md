# OxiGDAL ETL

Streaming ETL (Extract, Transform, Load) framework for continuous geospatial data processing.

## Features

- **Async Streaming**: Built on tokio for high-performance async I/O
- **Backpressure Handling**: Automatic backpressure management to prevent memory overflow
- **Error Recovery**: Configurable error handling and retry logic
- **Checkpointing**: State persistence for fault tolerance
- **Monitoring**: Built-in metrics and logging with tracing
- **Resource Limits**: Control parallelism and memory usage
- **Flexible Pipeline**: Fluent API for composing ETL workflows

## Architecture

The ETL framework consists of several key components:

### Sources
Data inputs from various sources:
- **File**: Local file system (GeoTIFF, GeoJSON, etc.)
- **HTTP/S3**: Streaming from HTTP endpoints and S3
- **STAC**: STAC catalog integration (optional feature)
- **Kafka**: Real-time streaming (optional feature)
- **PostGIS**: PostgreSQL/PostGIS database (optional feature)
- **Custom**: User-defined sources

### Transforms
Data transformations:
- **Map**: Element-wise transformations
- **Filter**: Conditional filtering
- **FlatMap**: One-to-many transformations
- **Reduce**: Aggregations
- **GroupBy**: Grouping operations
- **Window**: Sliding/tumbling windows
- **Join**: Spatial/attribute joins

### Sinks
Data outputs to various destinations:
- **File**: Local file system
- **S3/Azure/GCS**: Cloud storage (optional features)
- **PostGIS**: PostgreSQL/PostGIS database (optional feature)
- **Kafka**: Message streaming (optional feature)
- **Custom**: User-defined sinks

### Pipeline
Fluent API for composing ETL workflows with:
- Source-transform-sink composition
- Backpressure management
- Error recovery and retries
- Checkpointing for fault tolerance
- Parallel processing

### Scheduler
Task scheduling and execution:
- Cron-based scheduling (optional feature)
- Event-triggered execution
- Retry on failure
- Resource limits
- Monitoring

## Usage

### Basic Pipeline

```rust
use oxigdal_etl::prelude::*;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<()> {
    // Build ETL pipeline
    let pipeline = Pipeline::builder()
        .source(Box::new(FileSource::new(PathBuf::from("input.json"))))
        .map("uppercase".to_string(), |item| {
            Box::pin(async move {
                let s = String::from_utf8(item)?;
                Ok(s.to_uppercase().into_bytes())
            })
        })
        .filter("non_empty".to_string(), |item| {
            Box::pin(async move { Ok(!item.is_empty()) })
        })
        .sink(Box::new(FileSink::new(PathBuf::from("output.json"))))
        .with_checkpointing()
        .buffer_size(1000)
        .build()?;

    // Execute pipeline
    let stats = pipeline.run().await?;
    println!("Processed {} items in {:?}",
             stats.items_processed(),
             stats.elapsed());

    Ok(())
}
```

### Streaming Pipeline with STAC

```rust
use oxigdal_etl::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    let pipeline = Pipeline::builder()
        // Source: STAC catalog
        .source(Box::new(
            StacSource::new("https://earth-search.aws.element84.com/v1")
                .collection("sentinel-2-l2a")
                .bbox([-180.0, -90.0, 180.0, 90.0])
                .limit(100)
        ))

        // Transform: Extract metadata
        .map("extract_metadata".to_string(), |item| {
            Box::pin(async move {
                let json: serde_json::Value = serde_json::from_slice(&item)?;
                let metadata = extract_metadata(&json)?;
                Ok(serde_json::to_vec(&metadata)?)
            })
        })

        // Filter: Only recent data
        .filter("recent".to_string(), |item| {
            Box::pin(async move {
                let json: serde_json::Value = serde_json::from_slice(item)?;
                Ok(is_recent(&json))
            })
        })

        // Sink: Write to S3
        .sink(Box::new(
            S3Sink::new("my-bucket".to_string(), "processed/".to_string()).await?
        ))

        .build()?;

    pipeline.run().await?;
    Ok(())
}
```

### Window Operations

```rust
use oxigdal_etl::prelude::*;
use oxigdal_etl::operators::window::*;
use std::time::Duration;

let window = WindowOperator::tumbling_time(
    Duration::from_secs(60),
    WindowAggregator::stats("value".to_string())
);

// Process stream with windowing
let pipeline = Pipeline::builder()
    .source(source)
    .transform(Box::new(window))
    .sink(sink)
    .build()?;
```

### Scheduled Tasks

```rust
use oxigdal_etl::prelude::*;
use oxigdal_etl::scheduler::*;
use std::time::Duration;

let scheduler = Scheduler::new();

// Add scheduled task
let config = TaskConfig::new(
    "daily-processing".to_string(),
    "Daily Data Processing".to_string(),
    Schedule::Interval(Duration::from_secs(86400))
).max_retries(3);

scheduler.add_task(config, pipeline).await?;

// Start scheduler
scheduler.start().await?;
```

## Feature Flags

- `std` (default): Enable standard library support
- `kafka`: Enable Kafka source and sink
- `postgres`: Enable PostgreSQL/PostGIS support
- `s3`: Enable Amazon S3 support
- `stac`: Enable STAC catalog support
- `http`: Enable HTTP source support
- `scheduler`: Enable cron-based scheduling
- `all`: Enable all optional features

## Performance

The ETL framework is designed for high performance:

- Async I/O with tokio for maximum throughput
- Automatic backpressure to prevent memory overflow
- Parallel processing with configurable parallelism
- Zero-copy operations where possible
- Efficient buffering and batching

## Error Handling

Comprehensive error handling:

- Type-safe error types with thiserror
- Configurable error recovery
- Retry logic with exponential backoff
- Detailed error messages
- No `unwrap()` or `panic!()` in production code

## License

Apache-2.0

## Authors

COOLJAPAN OU (Team Kitasan)
