//! Development tools for OxiGDAL
//!
//! This crate provides various development utilities for working with OxiGDAL:
//!
//! - **Profiler**: Performance profiling and analysis
//! - **Debugger**: Debugging utilities and helpers
//! - **Validator**: Data validation and integrity checking
//! - **Inspector**: File format inspection and analysis
//! - **Generator**: Test data generation
//! - **Benchmarker**: Quick benchmarking tools
//!
//! # Example
//!
//! ```rust,no_run
//! use oxigdal_dev_tools::{profiler::Profiler, inspector::FileInspector};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Profile some operation
//! let mut profiler = Profiler::new("my_operation");
//! profiler.start();
//! // ... do work ...
//! profiler.stop();
//! println!("{}", profiler.report());
//!
//! // Inspect a file
//! let inspector = FileInspector::new("/path/to/file.tif")?;
//! println!("{}", inspector.summary());
//! # Ok(())
//! # }
//! ```

#![warn(missing_docs)]
#![warn(clippy::unwrap_used)]
#![warn(clippy::panic)]

pub mod benchmarker;
pub mod debugger;
pub mod generator;
pub mod inspector;
pub mod profiler;
pub mod validator;

use thiserror::Error;

/// Result type for dev tools operations
pub type Result<T> = std::result::Result<T, DevToolsError>;

/// Error types for dev tools
#[derive(Error, Debug)]
pub enum DevToolsError {
    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Profiler error
    #[error("Profiler error: {0}")]
    Profiler(String),

    /// Validator error
    #[error("Validation error: {0}")]
    Validation(String),

    /// Inspector error
    #[error("Inspector error: {0}")]
    Inspector(String),

    /// Generator error
    #[error("Generator error: {0}")]
    Generator(String),

    /// Benchmarker error
    #[error("Benchmarker error: {0}")]
    Benchmarker(String),

    /// OxiGDAL core error
    #[error("OxiGDAL error: {0}")]
    OxiGdal(#[from] oxigdal_core::error::OxiGdalError),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}
