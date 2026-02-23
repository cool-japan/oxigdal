//! Parallel processing support for multi-core performance
//!
//! This module provides parallel implementations of common geospatial operations
//! using Rayon for work-stealing and efficient multi-threaded execution.
//!
//! # Features
//!
//! This module is only available when the `parallel` feature is enabled.
//!
//! ## Parallel Raster Operations
//!
//! - `parallel_map_raster`: Apply a function to each pixel in parallel
//! - `parallel_reduce_raster`: Aggregate statistics from chunks
//! - `parallel_transform_raster`: Apply transformations using thread pool
//!
//! ## Parallel Tile Processing
//!
//! - Process multiple tiles concurrently
//! - COG pyramid generation in parallel
//! - Overview computation (multiple levels simultaneously)
//! - Tile compression in parallel
//!
//! ## Batch Processing
//!
//! - Process multiple files in parallel
//! - Configurable thread pool size
//! - Thread-safe result collection
//!
//! # Example
//!
//! ```no_run
//! # #[cfg(feature = "parallel")]
//! # {
//! use oxigdal_algorithms::parallel::*;
//! use oxigdal_core::buffer::RasterBuffer;
//! use oxigdal_core::types::RasterDataType;
//! # use oxigdal_algorithms::error::Result;
//!
//! # fn main() -> Result<()> {
//! // Create a raster buffer
//! let buffer = RasterBuffer::zeros(1000, 1000, RasterDataType::Float32);
//!
//! // Apply a parallel operation
//! let result = parallel_map_raster(&buffer, |pixel| pixel * 2.0)?;
//! # Ok(())
//! # }
//! # }
//! ```
//!
//! # Performance
//!
//! Parallel operations provide 2-16x speedup on multi-core systems, depending on:
//!
//! - Number of CPU cores
//! - Data size (larger datasets benefit more)
//! - Operation complexity (heavier operations benefit more)
//! - Memory bandwidth
//!
//! # Thread Safety
//!
//! All parallel operations are thread-safe and use proper synchronization.
//! Errors are collected and returned in a thread-safe manner.
//!
//! # COOLJAPAN Policy Compliance
//!
//! - Pure Rust (no C/Fortran dependencies)
//! - No `unwrap()` or `expect()` in production code
//! - Feature-gated (default OFF to keep dependencies minimal)
//! - Comprehensive error handling

pub mod batch;
pub mod raster;
pub mod tiles;

// Re-export commonly used items
pub use batch::{BatchConfig, BatchResult, parallel_batch_process, parallel_map};
pub use raster::{
    ChunkConfig, ReduceOp, parallel_focal_mean, parallel_focal_median, parallel_map_raster,
    parallel_map_raster_with_config, parallel_reduce_raster, parallel_transform_raster,
};
pub use tiles::{TileConfig, TileProcessor, parallel_generate_overviews, parallel_process_tiles};

/// Configuration for parallel processing
#[derive(Debug, Clone)]
pub struct ParallelConfig {
    /// Number of threads to use (None = default from Rayon)
    pub num_threads: Option<usize>,
    /// Chunk size for raster operations (None = automatic)
    pub chunk_size: Option<usize>,
    /// Enable progress reporting
    pub progress: bool,
}

impl Default for ParallelConfig {
    fn default() -> Self {
        Self {
            num_threads: None,
            chunk_size: None,
            progress: false,
        }
    }
}

impl ParallelConfig {
    /// Creates a new parallel configuration
    #[must_use]
    pub const fn new() -> Self {
        Self {
            num_threads: None,
            chunk_size: None,
            progress: false,
        }
    }

    /// Sets the number of threads
    #[must_use]
    pub const fn with_threads(mut self, num_threads: usize) -> Self {
        self.num_threads = Some(num_threads);
        self
    }

    /// Sets the chunk size
    #[must_use]
    pub const fn with_chunk_size(mut self, chunk_size: usize) -> Self {
        self.chunk_size = Some(chunk_size);
        self
    }

    /// Enables progress reporting
    #[must_use]
    pub const fn with_progress(mut self, progress: bool) -> Self {
        self.progress = progress;
        self
    }
}

/// Calculates optimal chunk size based on data size and CPU count
///
/// This function determines an appropriate chunk size for parallel operations
/// to maximize cache locality and minimize overhead.
///
/// # Arguments
///
/// * `total_elements` - Total number of elements to process
/// * `num_threads` - Number of threads (None = use CPU count)
///
/// # Returns
///
/// Optimal chunk size in elements
#[must_use]
pub fn calculate_chunk_size(total_elements: usize, num_threads: Option<usize>) -> usize {
    let threads = num_threads.unwrap_or_else(num_cpus);

    // Aim for 4-8 chunks per thread for good load balancing
    let target_chunks = threads * 6;

    // Minimum chunk size for cache efficiency (64 KB)
    const MIN_CHUNK_SIZE: usize = 64 * 1024 / core::mem::size_of::<f64>();

    // Maximum chunk size (4 MB)
    const MAX_CHUNK_SIZE: usize = 4 * 1024 * 1024 / core::mem::size_of::<f64>();

    let chunk_size = total_elements / target_chunks;

    // Clamp to reasonable bounds
    chunk_size.clamp(MIN_CHUNK_SIZE, MAX_CHUNK_SIZE)
}

/// Returns the number of available CPUs
#[must_use]
fn num_cpus() -> usize {
    rayon::current_num_threads()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parallel_config_default() {
        let config = ParallelConfig::default();
        assert!(config.num_threads.is_none());
        assert!(config.chunk_size.is_none());
        assert!(!config.progress);
    }

    #[test]
    fn test_parallel_config_builder() {
        let config = ParallelConfig::new()
            .with_threads(4)
            .with_chunk_size(1024)
            .with_progress(true);

        assert_eq!(config.num_threads, Some(4));
        assert_eq!(config.chunk_size, Some(1024));
        assert!(config.progress);
    }

    #[test]
    fn test_calculate_chunk_size() {
        let chunk_size = calculate_chunk_size(1_000_000, Some(4));
        assert!(chunk_size > 0);
        assert!(chunk_size < 1_000_000);
    }

    #[test]
    fn test_num_cpus() {
        let cpus = num_cpus();
        assert!(cpus > 0);
    }
}
