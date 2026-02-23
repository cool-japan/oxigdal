//! OxiGDAL ETL - Streaming ETL framework for geospatial data processing
//!
//! This crate provides a comprehensive ETL (Extract, Transform, Load) framework
//! for continuous geospatial data processing with OxiGDAL.
//!
//! # Features
//!
//! - **Async Streaming**: Built on tokio for high-performance async I/O
//! - **Backpressure Handling**: Automatic backpressure management
//! - **Error Recovery**: Configurable error handling and retry logic
//! - **Checkpointing**: State persistence for fault tolerance
//! - **Monitoring**: Built-in metrics and logging
//! - **Resource Limits**: Control parallelism and memory usage
//!
//! # Architecture
//!
//! The ETL framework consists of several key components:
//!
//! - **Sources**: Data inputs (files, HTTP, S3, STAC, Kafka, PostGIS)
//! - **Transforms**: Data transformations (map, filter, window, join, etc.)
//! - **Sinks**: Data outputs (files, S3, PostGIS, Kafka)
//! - **Pipeline**: Fluent API for composing ETL workflows
//! - **Stream**: Async stream processing with backpressure
//! - **Scheduler**: Task scheduling and execution
//!
//! # Example
//!
//! ```rust,no_run
//! use oxigdal_etl::*;
//! use oxigdal_etl::source::FileSource;
//! use oxigdal_etl::sink::FileSink;
//! use oxigdal_etl::error::TransformError;
//! use std::path::PathBuf;
//!
//! # async fn example() -> Result<()> {
//! // Build ETL pipeline
//! let pipeline = Pipeline::builder()
//!     .source(Box::new(FileSource::new(PathBuf::from("input.json"))))
//!     .map("uppercase".to_string(), |item| {
//!         Box::pin(async move {
//!             let s = String::from_utf8(item).map_err(|e| {
//!                 TransformError::InvalidInput {
//!                     message: e.to_string(),
//!                 }
//!             })?;
//!             Ok(s.to_uppercase().into_bytes())
//!         })
//!     })
//!     .filter("non_empty".to_string(), |item| {
//!         let is_empty = item.is_empty();
//!         Box::pin(async move { Ok(!is_empty) })
//!     })
//!     .sink(Box::new(FileSink::new(PathBuf::from("output.json"))))
//!     .with_checkpointing()
//!     .buffer_size(1000)
//!     .build()?;
//!
//! // Execute pipeline
//! let stats = pipeline.run().await?;
//! println!("Processed {} items", stats.items_processed());
//! # Ok(())
//! # }
//! ```
//!
//! # Feature Flags
//!
//! - `std` (default): Enable standard library support
//! - `kafka`: Enable Kafka source and sink
//! - `postgres`: Enable PostgreSQL/PostGIS support
//! - `s3`: Enable Amazon S3 support
//! - `stac`: Enable STAC catalog support
//! - `http`: Enable HTTP source support
//! - `scheduler`: Enable cron-based scheduling
//! - `all`: Enable all optional features

#![warn(missing_docs)]
#![cfg_attr(not(feature = "std"), no_std)]

// Re-export key types
pub use error::{EtlError, Result};
pub use pipeline::{ExecutionMode, Pipeline, PipelineBuilder, PipelineConfig, PipelineStats};
pub use scheduler::{Schedule, Scheduler, TaskConfig, TaskResult};
pub use sink::Sink;
pub use source::Source;
pub use stream::{
    BoxStream, BufferedStream, ParallelProcessor, StateManager, StreamConfig, StreamItem,
    StreamProcessor,
};
pub use transform::Transform;

// Modules
pub mod error;
pub mod operators;
pub mod pipeline;
pub mod scheduler;
pub mod sink;
pub mod source;
pub mod stream;
pub mod transform;

/// Prelude module for convenient imports
pub mod prelude {
    pub use crate::error::{EtlError, Result};
    pub use crate::operators::{aggregate::*, filter::*, join::*, map::*, window::*};
    pub use crate::pipeline::{ExecutionMode, Pipeline, PipelineBuilder};
    pub use crate::scheduler::{Schedule, Scheduler, TaskConfig};
    pub use crate::sink::{FileSink, Sink};
    pub use crate::source::{FileSource, Source};
    pub use crate::stream::{StreamConfig, StreamItem, StreamProcessor};
    pub use crate::transform::Transform;

    #[cfg(feature = "kafka")]
    pub use crate::sink::KafkaSink;
    #[cfg(feature = "kafka")]
    pub use crate::source::KafkaSource;

    #[cfg(feature = "postgres")]
    pub use crate::sink::PostGisSink;

    #[cfg(feature = "s3")]
    pub use crate::sink::S3Sink;

    #[cfg(feature = "stac")]
    pub use crate::source::StacSource;

    #[cfg(feature = "http")]
    pub use crate::source::HttpSource;
}

/// Version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Crate name
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }

    #[test]
    fn test_crate_name() {
        assert_eq!(CRATE_NAME, "oxigdal-etl");
    }
}
