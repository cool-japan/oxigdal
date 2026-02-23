//! Real-time data processing and streaming pipelines for OxiGDAL.
//!
//! This crate provides a comprehensive streaming framework for processing
//! geospatial data in real-time. It includes:
//!
//! - Stream traits and abstractions with backpressure handling
//! - Windowing and watermarking for event-time processing
//! - Rich set of transformations (map, filter, join, etc.)
//! - Stateful operations with checkpointing
//! - State backends (RocksDB) for fault tolerance
//!
//! # Example
//!
//! ```no_run
//! use oxigdal_streaming::core::Stream;
//! use oxigdal_streaming::transformations::MapTransform;
//!
//! # async fn example() -> oxigdal_streaming::error::Result<()> {
//! // Create a stream and apply transformations
//! // let stream = Stream::new();
//! // let transformed = stream.map(|x| x * 2).filter(|x| x > 10);
//! # Ok(())
//! # }
//! ```

#![warn(missing_docs)]
#![deny(unsafe_code)]

pub mod core;
pub mod error;
pub mod metrics;
pub mod state;
pub mod transformations;
pub mod windowing;

pub use error::{Result, StreamingError};
