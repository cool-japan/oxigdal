//! Jupyter integration for OxiGDAL
//!
//! This crate provides Jupyter kernel implementation with rich display capabilities,
//! interactive widgets, and magic commands for geospatial data analysis.
//!
//! # Features
//!
//! - Custom Jupyter kernel for OxiGDAL
//! - Rich display (images, maps, tables)
//! - Interactive widgets
//! - Magic commands (%load_raster, %plot, etc.)
//! - Integration with plotters for visualization
//!
//! # Example
//!
//! ```rust,no_run
//! use oxigdal_jupyter::kernel::OxiGdalKernel;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Create and start kernel
//! let kernel = OxiGdalKernel::new()?;
//! // kernel.run()?;
//! # Ok(())
//! # }
//! ```

#![warn(missing_docs)]
#![warn(clippy::unwrap_used)]
#![warn(clippy::panic)]

pub mod display;
pub mod kernel;
pub mod magic;
pub mod plotting;
pub mod widgets;

pub use display::{DisplayData, RichDisplay};
pub use kernel::OxiGdalKernel;
pub use magic::MagicCommand;
pub use widgets::{MapWidget, Widget};

use thiserror::Error;

/// Result type for Jupyter operations
pub type Result<T> = std::result::Result<T, JupyterError>;

/// Error types for Jupyter integration
#[derive(Error, Debug)]
pub enum JupyterError {
    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Kernel error
    #[error("Kernel error: {0}")]
    Kernel(String),

    /// Display error
    #[error("Display error: {0}")]
    Display(String),

    /// Widget error
    #[error("Widget error: {0}")]
    Widget(String),

    /// Magic command error
    #[error("Magic command error: {0}")]
    Magic(String),

    /// Plotting error
    #[error("Plotting error: {0}")]
    Plotting(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// OxiGDAL core error
    #[error("OxiGDAL error: {0}")]
    OxiGdal(#[from] oxigdal_core::error::OxiGdalError),
}
