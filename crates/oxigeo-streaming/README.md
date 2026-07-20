# oxigeo-streaming

Real-time data processing and streaming pipelines for OxiGeo.

## Overview

`oxigeo-streaming` provides a comprehensive framework for processing geospatial data in real-time. It includes robust stream processing capabilities with event-time processing, windowing, stateful operations, and fault tolerance.

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
- **State Backends**: In-memory and persistent Pure-Rust LSM (OxiStore/fjall) backends
- **Recovery**: Automatic state recovery from checkpoints

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
oxigeo-streaming = "0.2.0"
```

For the persistent LSM key-value state backend (Pure-Rust, via OxiStore/fjall):

```toml
[dependencies]
oxigeo-streaming = { version = "0.2.0", features = ["kv-store"] }
```

## Usage

### Basic Stream Processing

```rust
use oxigeo_streaming::core::stream::{Stream, StreamElement, StreamMessage};
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
use oxigeo_streaming::windowing::tumbling::TumblingAssigner;
use oxigeo_streaming::windowing::window::WindowAssigner;
use chrono::Duration;

let assigner = TumblingAssigner::new(Duration::seconds(60));
let windows = assigner.assign_windows(&element)?;
```

### Aggregation

```rust
use oxigeo_streaming::transformations::aggregate::{AggregateOperator, CountAggregate};

let operator = AggregateOperator::new(CountAggregate);

for elem in elements {
    operator.process(elem).await?;
}

let result = operator.get_result(None).await;
```

### Join Operations

```rust
use oxigeo_streaming::transformations::join::{JoinOperator, JoinConfig};

let config = JoinConfig::default();
let join = JoinOperator::new(config);

join.process_left(left_element).await?;
let results = join.process_right(right_element).await?;
```

### Stateful Processing

```rust
use oxigeo_streaming::state::backend::MemoryStateBackend;
use oxigeo_streaming::state::keyed_state::ValueState;
use std::sync::Arc;

let backend = Arc::new(MemoryStateBackend::new());
let state = ValueState::new(backend, "namespace".to_string(), vec![1]);

state.set(vec![42]).await?;
let value = state.get().await?;
```

### Checkpointing

```rust
use std::sync::Arc;
use oxigeo_streaming::state::{
    BroadcastState, CheckpointConfig, CheckpointCoordinator, DynOperatorState,
    FileCheckpointStorage,
};

// Durable, file-backed checkpoint storage (Pure Rust).
let storage = Arc::new(FileCheckpointStorage::new("/var/lib/app/checkpoints")?);
let coordinator = CheckpointCoordinator::with_storage(CheckpointConfig::default(), storage);

// Register operator state to be captured on every checkpoint.
let operator_state = Arc::new(BroadcastState::new());
operator_state.put(vec![1], vec![42]).await;
coordinator
    .register_operator("operator-1", operator_state.clone() as Arc<dyn DynOperatorState>)
    .await;

// Capture + persist + complete in one durable step. Only reported as
// successful once the state has actually been written to storage.
let checkpoint_id = coordinator.checkpoint().await?;

// Later, recover state from a persisted checkpoint.
coordinator.restore_checkpoint(checkpoint_id).await?;
```

The lower-level `trigger_checkpoint()` / `complete_checkpoint(id, success)` pair
is still available for manual coordination; `checkpoint()` combines them with the
persistence step.

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
- Pure-Rust LSM (OxiStore/fjall) backend for persistent state with minimal overhead

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
