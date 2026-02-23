# oxigdal-streaming

Real-time data processing and streaming pipelines for OxiGDAL.

## Overview

`oxigdal-streaming` provides a comprehensive framework for processing geospatial data in real-time. It includes robust stream processing capabilities with event-time processing, windowing, stateful operations, and fault tolerance.

## Features

### Streaming Core
- **Stream Traits and Abstractions**: Flexible stream processing with sources, sinks, and operators
- **Backpressure Handling**: Adaptive backpressure management to prevent buffer overflow
- **Flow Control**: Rate limiting and flow control mechanisms
- **Error Recovery**: Configurable recovery strategies with exponential backoff

### Windowing & Watermarking
- **Tumbling Windows**: Fixed, non-overlapping time windows
- **Sliding Windows**: Overlapping time windows with configurable slide intervals
- **Session Windows**: Dynamic windows based on activity gaps
- **Event Time Processing**: Watermark generation for handling out-of-order events
- **Late Data Handling**: Configurable policies for late-arriving data

### Transformations
- **Basic Operations**: Map, filter, flatMap
- **Aggregations**: Count, sum, average, min, max
- **Reduce Operations**: Reduce, fold, scan
- **Join Operations**: Inner, left, right, full outer joins
- **Partitioning**: Hash, range, round-robin partitioning strategies

### State Management
- **Keyed State**: Value, list, map, reducing, and aggregating state
- **Operator State**: Broadcast and union list state
- **Checkpointing**: Periodic checkpointing for fault tolerance
- **State Backends**: In-memory and RocksDB backends
- **Recovery**: Automatic state recovery from checkpoints

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
oxigdal-streaming = "0.1.0"
```

For RocksDB backend support:

```toml
[dependencies]
oxigdal-streaming = { version = "0.1.0", features = ["rocksdb-backend"] }
```

## Usage

### Basic Stream Processing

```rust
use oxigdal_streaming::core::stream::{Stream, StreamElement, StreamMessage};
use chrono::Utc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let stream = Stream::new();

    // Send elements
    for i in 0..10 {
        let elem = StreamElement::new(vec![i], Utc::now());
        stream.send(StreamMessage::Data(elem)).await?;
    }

    // Receive elements
    for _ in 0..10 {
        match stream.recv().await? {
            StreamMessage::Data(elem) => {
                println!("Received: {:?}", elem.data);
            }
            _ => {}
        }
    }

    Ok(())
}
```

### Windowing

```rust
use oxigdal_streaming::windowing::tumbling::TumblingAssigner;
use oxigdal_streaming::windowing::window::WindowAssigner;
use chrono::Duration;

let assigner = TumblingAssigner::new(Duration::seconds(60));
let windows = assigner.assign_windows(&element)?;
```

### Aggregation

```rust
use oxigdal_streaming::transformations::aggregate::{AggregateOperator, CountAggregate};

let operator = AggregateOperator::new(CountAggregate);

for elem in elements {
    operator.process(elem).await?;
}

let result = operator.get_result(None).await;
```

### Join Operations

```rust
use oxigdal_streaming::transformations::join::{JoinOperator, JoinConfig};

let config = JoinConfig::default();
let join = JoinOperator::new(config);

join.process_left(left_element).await?;
let results = join.process_right(right_element).await?;
```

### Stateful Processing

```rust
use oxigdal_streaming::state::backend::MemoryStateBackend;
use oxigdal_streaming::state::keyed_state::ValueState;
use std::sync::Arc;

let backend = Arc::new(MemoryStateBackend::new());
let state = ValueState::new(backend, "namespace".to_string(), vec![1]);

state.set(vec![42]).await?;
let value = state.get().await?;
```

### Checkpointing

```rust
use oxigdal_streaming::state::checkpoint::{CheckpointCoordinator, CheckpointConfig};

let config = CheckpointConfig::default();
let coordinator = CheckpointCoordinator::new(config);

let checkpoint_id = coordinator.trigger_checkpoint().await?;
// Process...
coordinator.complete_checkpoint(checkpoint_id, true).await?;
```

## Architecture

The crate is organized into several modules:

- **core**: Stream abstractions, backpressure, flow control, operators, and recovery
- **windowing**: Window types, assigners, and watermark generation
- **transformations**: Stream transformations, aggregations, joins, and partitioning
- **state**: State backends, checkpointing, keyed state, and operator state

## Performance

The streaming framework is designed for high performance:

- Lock-free data structures where possible
- Efficient buffer management with adaptive backpressure
- Configurable parallelism for distributed processing
- RocksDB backend for persistent state with minimal overhead

## COOLJAPAN Compliance

This crate follows all COOLJAPAN policies:

- ✅ 100% Pure Rust (no C/Fortran dependencies)
- ✅ No `unwrap()` or `panic!()` in production code
- ✅ All files under 2000 lines
- ✅ Workspace dependencies
- ✅ Comprehensive tests and benchmarks

## License

Licensed under Apache-2.0.

## Contributing

Contributions are welcome! Please ensure all tests pass and follow the COOLJAPAN policies.
