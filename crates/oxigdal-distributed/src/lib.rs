//! OxiGDAL Distributed Processing
//!
//! This crate provides distributed processing capabilities for large-scale geospatial
//! workflows using Apache Arrow Flight for zero-copy data transfer.
//!
//! # Features
//!
//! - **Arrow Flight RPC**: Zero-copy data transfer between nodes
//! - **Worker Nodes**: Execute processing tasks with resource management
//! - **Coordinator**: Schedule and manage distributed execution
//! - **Data Partitioning**: Spatial, hash, range, and load-balanced partitioning
//! - **Shuffle Operations**: Efficient data redistribution for group-by and joins
//! - **Fault Tolerance**: Automatic retry and failure recovery
//! - **Progress Monitoring**: Real-time tracking of distributed execution
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────┐
//! │ Coordinator │ ──── Schedules tasks
//! └──────┬──────┘
//!        │
//!   ┌────┴────┐
//!   │  Flight │
//!   │  Server │
//!   └────┬────┘
//!        │
//!   ┌────┴────────────────┐
//!   │                     │
//! ┌─▼──────┐         ┌───▼─────┐
//! │ Worker │         │ Worker  │
//! │ Node 1 │         │ Node 2  │
//! └────────┘         └─────────┘
//! ```
//!
//! # Example: Distributed NDVI Calculation
//!
//! ```rust,no_run
//! use oxigdal_distributed::*;
//! # async fn example() -> std::result::Result<(), Box<dyn std::error::Error>> {
//!
//! // Create coordinator
//! let config = CoordinatorConfig::new("localhost:50051".to_string());
//! let coordinator = Coordinator::new(config);
//!
//! // Add workers
//! coordinator.add_worker("worker-1".to_string(), "localhost:50052".to_string())?;
//! coordinator.add_worker("worker-2".to_string(), "localhost:50053".to_string())?;
//!
//! // Partition data spatially
//! let extent = SpatialExtent::new(0.0, 0.0, 1000.0, 1000.0)?;
//! let partitioner = TilePartitioner::new(extent, 4, 4)?;
//! let partitions = partitioner.partition();
//!
//! // Submit tasks for each partition
//! for partition in partitions {
//!     coordinator.submit_task(
//!         partition.id,
//!         TaskOperation::CalculateIndex {
//!             index_type: "NDVI".to_string(),
//!             bands: vec![3, 4], // Red and NIR
//!         },
//!     )?;
//! }
//!
//! // Monitor progress
//! while !coordinator.is_complete() {
//!     let progress = coordinator.get_progress()?;
//!     println!(
//!         "Progress: {}/{} completed",
//!         progress.completed_tasks,
//!         progress.total_tasks()
//!     );
//!     tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
//! }
//!
//! // Collect results
//! let results = coordinator.collect_results()?;
//! println!("Processing complete: {} results", results.len());
//! # Ok(())
//! # }
//! ```
//!
//! # Example: Custom Processing with Workers
//!
//! ```rust,no_run
//! use oxigdal_distributed::*;
//! # async fn example() -> std::result::Result<(), Box<dyn std::error::Error>> {
//!
//! // Create worker
//! let config = WorkerConfig::new("worker-1".to_string())
//!     .with_max_concurrent_tasks(4)
//!     .with_memory_limit(8 * 1024 * 1024 * 1024); // 8 GB
//!
//! let worker = Worker::new(config);
//!
//! // Execute tasks
//! // (Tasks would be received from coordinator in real implementation)
//! # Ok(())
//! # }
//! ```
//!
//! # Example: Data Shuffle
//!
//! ```rust,no_run
//! use oxigdal_distributed::*;
//! use arrow::array::{Int32Array, StringArray};
//! use arrow::datatypes::{DataType, Field, Schema};
//! use arrow::record_batch::RecordBatch;
//! use std::sync::Arc;
//!
//! # fn example() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! // Create test data
//! let schema = Arc::new(Schema::new(vec![
//!     Field::new("id", DataType::Int32, false),
//!     Field::new("name", DataType::Utf8, false),
//! ]));
//!
//! let batch = RecordBatch::try_new(
//!     schema,
//!     vec![
//!         Arc::new(Int32Array::from(vec![1, 2, 3, 4, 5])),
//!         Arc::new(StringArray::from(vec!["a", "b", "c", "d", "e"])),
//!     ],
//! )?;
//!
//! // Hash shuffle by ID column
//! let shuffle = HashShuffle::new("id".to_string(), 2)?;
//! let partitions = shuffle.shuffle(&batch)?;
//!
//! println!("Data shuffled into {} partitions", partitions.len());
//! # Ok(())
//! # }
//! ```

#![deny(clippy::unwrap_used)]
#![deny(clippy::panic)]
#![warn(missing_docs)]
#![warn(clippy::expect_used)]

pub mod coordinator;
pub mod error;
pub mod flight;
pub mod partition;
pub mod shuffle;
pub mod task;
pub mod worker;

// Re-export main types
pub use coordinator::{Coordinator, CoordinatorConfig, CoordinatorProgress, WorkerInfo};
pub use error::{DistributedError, Result};
pub use flight::{FlightClient, FlightServer};
pub use partition::{
    HashPartitioner, LoadBalancedPartitioner, Partition, PartitionStrategy, RangePartitioner,
    SpatialExtent, StripPartitioner, TilePartitioner,
};
pub use shuffle::{
    BroadcastShuffle, HashShuffle, RangeShuffle, ShuffleConfig, ShuffleKey, ShuffleResult,
    ShuffleStats, ShuffleType,
};
pub use task::{
    PartitionId, Task, TaskContext, TaskId, TaskOperation, TaskResult, TaskScheduler, TaskStatus,
};
pub use worker::{Worker, WorkerConfig, WorkerHealthCheck, WorkerMetrics, WorkerStatus};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exports() {
        // Verify that main types are exported
        let _config: CoordinatorConfig;
        let _worker_config: WorkerConfig;
        let _task_id: TaskId;
        let _partition_id: PartitionId;
    }
}
