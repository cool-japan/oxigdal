//! Cloud storage optimizations for Zarr arrays
//!
//! This module provides advanced cloud storage features for efficient reading
//! of Zarr arrays from cloud object stores like S3, Azure Blob, and GCS.
//!
//! # Features
//!
//! - **Parallel Chunk Fetching**: Concurrent retrieval of multiple chunks
//! - **Range Request Optimization**: Efficient partial object reads
//! - **Connection Pooling**: Reuse HTTP connections for performance
//! - **Request Batching**: Combine multiple small requests
//! - **Retry with Exponential Backoff**: Handle transient failures gracefully
//! - **Streaming Reads**: Memory-efficient processing of large chunks
//! - **Prefetch Hints**: Predictive loading based on access patterns

pub mod batch;
pub mod config;
pub mod parallel;
pub mod pool;
pub mod prefetch;
pub mod range;
pub mod retry;
pub mod streaming;

#[cfg(test)]
mod tests;

// Re-export commonly used types
pub use batch::{BatchedRequest, RequestBatch};
pub use config::CloudStorageConfig;
pub use parallel::{
    ChunkFetchResult, CloudStorageMetrics, CloudStorageMetricsSummary, ParallelFetchStats,
};
pub use pool::{ConnectionPoolStats, ConnectionPoolStatsSummary};
pub use prefetch::{AccessPattern, PrefetchHint, PrefetchManager, PrefetchStats};
pub use range::ByteRange;
pub use retry::{RetryContext, RetryPolicy};
pub use streaming::StreamingChunkReader;
