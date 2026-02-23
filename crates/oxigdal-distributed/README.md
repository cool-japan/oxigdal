# OxiGDAL Distributed

Distributed processing capabilities for large-scale geospatial workflows using Apache Arrow Flight.

## Features

- **Apache Arrow Flight RPC**: Zero-copy data transfer between nodes using gRPC
- **Multi-node Processing**: Distribute workloads across multiple worker nodes
- **Fault Tolerance**: Automatic task retry and failure recovery
- **Dynamic Scaling**: Add or remove workers at runtime
- **Progress Monitoring**: Real-time tracking of distributed execution
- **Resource Management**: Memory and CPU limits per worker
- **Data Partitioning**: Multiple strategies (spatial tiles, strips, hash, range, load-balanced)
- **Shuffle Operations**: Efficient data redistribution for group-by, sort, and joins
- **Pure Rust**: No C/C++ dependencies

## Architecture

```text
┌─────────────┐
│ Coordinator │ ──── Schedules tasks and manages workers
└──────┬──────┘
       │
  ┌────┴────┐
  │  Flight │
  │  Server │ ──── Zero-copy data transfer
  └────┬────┘
       │
  ┌────┴────────────────┐
  │                     │
┌─▼──────┐         ┌───▼─────┐
│ Worker │         │ Worker  │ ──── Execute tasks
│ Node 1 │         │ Node 2  │
└────────┘         └─────────┘
```

## Example: Distributed NDVI Calculation

```rust
use oxigdal_distributed::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create coordinator
    let config = CoordinatorConfig::new("localhost:50051".to_string());
    let coordinator = Coordinator::new(config);

    // Add workers
    coordinator.add_worker("worker-1".to_string(), "localhost:50052".to_string())?;
    coordinator.add_worker("worker-2".to_string(), "localhost:50053".to_string())?;

    // Partition data spatially into 4x4 tiles
    let extent = SpatialExtent::new(0.0, 0.0, 1000.0, 1000.0)?;
    let partitioner = TilePartitioner::new(extent, 4, 4)?;
    let partitions = partitioner.partition();

    // Submit tasks for each partition
    for partition in partitions {
        coordinator.submit_task(
            partition.id,
            TaskOperation::CalculateIndex {
                index_type: "NDVI".to_string(),
                bands: vec![3, 4], // Red and NIR bands
            },
        )?;
    }

    // Monitor progress
    while !coordinator.is_complete() {
        let progress = coordinator.get_progress()?;
        println!(
            "Progress: {}/{} completed ({:.1}%)",
            progress.completed_tasks,
            progress.total_tasks(),
            progress.completion_percentage()
        );
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }

    // Collect results
    let results = coordinator.collect_results()?;
    println!("Processing complete: {} results", results.len());

    Ok(())
}
```

## Example: Worker Node

```rust
use oxigdal_distributed::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Configure worker
    let config = WorkerConfig::new("worker-1".to_string())
        .with_max_concurrent_tasks(4)
        .with_memory_limit(8 * 1024 * 1024 * 1024) // 8 GB
        .with_num_cores(4);

    let worker = Worker::new(config);

    // Start heartbeat
    let (tx, rx) = tokio::sync::mpsc::channel(100);
    worker.start_heartbeat(tx).await?;

    // Worker would receive and execute tasks from coordinator
    // (Implementation would connect to Flight server)

    Ok(())
}
```

## Example: Data Shuffle

```rust
use oxigdal_distributed::*;
use arrow::array::{Int32Array, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create test data
    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int32, false),
        Field::new("name", DataType::Utf8, false),
    ]));

    let batch = RecordBatch::try_new(
        schema,
        vec![
            Arc::new(Int32Array::from(vec![1, 2, 3, 4, 5])),
            Arc::new(StringArray::from(vec!["a", "b", "c", "d", "e"])),
        ],
    )?;

    // Hash shuffle by ID column into 2 partitions
    let shuffle = HashShuffle::new("id".to_string(), 2)?;
    let partitions = shuffle.shuffle(&batch)?;

    println!("Data shuffled into {} partitions", partitions.len());
    for (partition_id, partition_batch) in partitions {
        println!(
            "Partition {:?}: {} rows",
            partition_id,
            partition_batch.num_rows()
        );
    }

    Ok(())
}
```

## Partitioning Strategies

### Tile Partitioning

Divide spatial data into regular tiles:

```rust
let extent = SpatialExtent::new(0.0, 0.0, 1000.0, 1000.0)?;
let partitioner = TilePartitioner::new(extent, 4, 4)?; // 4x4 grid
let partitions = partitioner.partition();
```

### Strip Partitioning

Divide data into horizontal strips:

```rust
let extent = SpatialExtent::new(0.0, 0.0, 1000.0, 1000.0)?;
let partitioner = StripPartitioner::new(extent, 8)?; // 8 strips
let partitions = partitioner.partition();
```

### Hash Partitioning

Distribute data based on hash of a key:

```rust
let partitioner = HashPartitioner::new(16)?; // 16 partitions
let partition_id = partitioner.partition_for_key(b"my_key");
```

### Range Partitioning

Partition based on value ranges:

```rust
let boundaries = vec![100.0, 200.0, 300.0, 400.0];
let partitioner = RangePartitioner::new(boundaries)?;
let partition_id = partitioner.partition_for_value(250.0);
```

### Load Balanced Partitioning

Balance load based on data size:

```rust
let total_size = 1000 * 1024 * 1024; // 1 GB
let num_workers = 8;
let partitioner = LoadBalancedPartitioner::new(total_size, num_workers)?;
```

## Shuffle Operations

### Hash Shuffle

Group data by key:

```rust
let shuffle = HashShuffle::new("column_name".to_string(), 4)?;
let partitions = shuffle.shuffle(&batch)?;
```

### Range Shuffle

Sort data by value ranges:

```rust
let boundaries = vec![10.0, 20.0, 30.0];
let shuffle = RangeShuffle::new("value_column".to_string(), boundaries)?;
let partitions = shuffle.shuffle(&batch)?;
```

### Broadcast Shuffle

Replicate data to all partitions:

```rust
let shuffle = BroadcastShuffle::new(num_workers)?;
let partitions = shuffle.shuffle(&batch);
```

## Performance

Benchmarks show excellent performance for large-scale operations:

- **Tile Partitioning**: 16x16 grid in <1ms
- **Hash Shuffle**: 100K rows in ~50ms
- **Range Shuffle**: 100K rows in ~60ms
- **Task Scheduling**: 10K tasks in ~100ms

Run benchmarks:

```bash
cargo bench --package oxigdal-distributed
```

## Safety

This crate follows OxiGDAL's strict safety policies:

- No `unwrap()` usage
- No `panic!()` calls
- Comprehensive error handling
- Pure Rust implementation

## License

Apache-2.0

## Authors

COOLJAPAN OU (Team Kitasan)
