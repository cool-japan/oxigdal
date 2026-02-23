//! Data pipeline for geospatial machine learning.
//!
//! This module provides dataset loaders, data loaders, and transformations
//! for training geospatial deep learning models.
//!
//! ## Core Components
//!
//! - [`Dataset`]: Trait for accessing samples from a dataset
//! - [`GeoTiffDataset`]: Dataset implementation for GeoTIFF files
//! - [`DataLoader`]: Parallel batch loading with prefetching
//! - [`transforms`]: Data transformation utilities
//!
//! ## Example
//!
//! ```rust,no_run
//! use oxigdal_ml_foundation::data::{GeoTiffDataset, DataLoader};
//! use std::path::PathBuf;
//! use std::sync::Arc;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Create a dataset from GeoTIFF files
//! let files = vec![PathBuf::from("image1.tif"), PathBuf::from("image2.tif")];
//! let dataset = Arc::new(GeoTiffDataset::new(files, (256, 256))?);
//!
//! // Create a data loader for batching
//! let loader = DataLoader::new(dataset, 16, true)?;
//!
//! // Iterate over batches
//! for batch in loader.iter() {
//!     let (inputs, targets) = batch?;
//!     // Train your model...
//! }
//! # Ok(())
//! # }
//! ```

pub mod dataloader;
pub mod dataset;
pub mod transforms;

// Re-export key types
pub use dataloader::{BatchIter, DataLoader};
pub use dataset::{Dataset, GeoTiffDataset};
pub use transforms::{apply_transforms_to_buffer, normalize_buffer};
