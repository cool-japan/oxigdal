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
//! # #[cfg(feature = "std")]
//! use oxigdal_streaming::core::Stream;
//!
//! # async fn example() -> oxigdal_streaming::error::Result<()> {
//! // Create a stream and apply transformations
//! // let stream = Stream::new();
//! // let transformed = stream.map(|x| x * 2).filter(|x| x > 10);
//! # Ok(())
//! # }
//! ```

#![cfg_attr(not(feature = "std"), no_std)]
#![warn(missing_docs)]
#![deny(unsafe_code)]

// When no_std is active, bring in alloc for heap allocation (Vec, String, etc.)
#[cfg(not(feature = "std"))]
extern crate alloc;

// All modules require std (async runtimes, I/O, Arrow, etc.)
#[cfg(feature = "std")]
pub mod arrow_ipc;
#[cfg(feature = "std")]
pub mod cloud;
#[cfg(feature = "std")]
pub mod core;
pub mod error;
#[cfg(feature = "std")]
pub mod io_coalescing;
#[cfg(feature = "std")]
pub mod metrics;
#[cfg(feature = "std")]
pub mod mmap;
#[cfg(feature = "std")]
pub mod state;
#[cfg(feature = "std")]
pub mod transformations;
#[cfg(feature = "std")]
pub mod v2;
#[cfg(feature = "std")]
pub mod windowing;

pub use error::{Result, StreamingError};
