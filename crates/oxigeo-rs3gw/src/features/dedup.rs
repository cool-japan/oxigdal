//! Zarr-optimized deduplication configuration
//!
//! This module provides deduplication strategies optimized for Zarr arrays,
//! which often contain redundant chunks (e.g., nodata chunks, repeated patterns).

use rs3gw::storage::{ChunkingAlgorithm, DedupConfig as Rs3gwDedupConfig};

/// Zarr-optimized deduplication configuration
///
/// Zarr arrays are chunked into fixed-size blocks, making them ideal for
/// content-based deduplication. This can achieve 30-70% storage savings for
/// datasets with nodata regions or repeated patterns.
#[derive(Debug, Clone)]
pub struct ZarrDedupConfig {
    /// Block size for deduplication (should match Zarr chunk size)
    ///
    /// For best results, this should match your Zarr array's chunk size.
    /// Common values: 64KB, 256KB, 1MB
    pub block_size: usize,

    /// Use content-defined chunking
    ///
    /// For Zarr, this should typically be `false` since Zarr chunks are
    /// already fixed-size. Set to `true` only for very large Zarr chunks.
    pub content_defined: bool,

    /// Minimum object size for deduplication in bytes
    ///
    /// Objects smaller than this won't be deduplicated. Default: 128KB
    pub min_object_size: usize,

    /// Enable aggressive nodata detection
    ///
    /// If true, chunks that are all zeros or all nodata values will be
    /// detected and stored only once. Highly recommended for geospatial data.
    pub aggressive_nodata: bool,
}

impl Default for ZarrDedupConfig {
    fn default() -> Self {
        Self {
            block_size: 256 * 1024,      // 256 KB (typical Zarr chunk)
            content_defined: false,      // Fixed-size for Zarr
            min_object_size: 128 * 1024, // 128 KB
            aggressive_nodata: true,     // Optimize for geospatial data
        }
    }
}

impl ZarrDedupConfig {
    /// Creates a new Zarr dedup configuration with default values
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a configuration optimized for a specific Zarr chunk size
    ///
    /// # Arguments
    /// * `chunk_bytes` - The size of Zarr chunks in bytes
    #[must_use]
    pub fn for_chunk_size(chunk_bytes: usize) -> Self {
        Self {
            block_size: chunk_bytes,
            content_defined: false,
            min_object_size: chunk_bytes / 2,
            aggressive_nodata: true,
        }
    }

    /// Sets the block size
    #[must_use]
    pub fn with_block_size(mut self, block_size: usize) -> Self {
        self.block_size = block_size;
        self
    }

    /// Sets whether to use content-defined chunking
    #[must_use]
    pub fn with_content_defined(mut self, enabled: bool) -> Self {
        self.content_defined = enabled;
        self
    }

    /// Sets the minimum object size for deduplication
    #[must_use]
    pub fn with_min_object_size(mut self, size: usize) -> Self {
        self.min_object_size = size;
        self
    }

    /// Enables or disables aggressive nodata detection
    #[must_use]
    pub fn with_aggressive_nodata(mut self, enabled: bool) -> Self {
        self.aggressive_nodata = enabled;
        self
    }

    /// Converts to rs3gw's DedupConfig
    ///
    /// # Errors
    /// Returns an error if the configuration is invalid
    pub fn to_rs3gw_config(&self) -> Result<Rs3gwDedupConfig, String> {
        let algorithm = if self.content_defined {
            ChunkingAlgorithm::ContentDefined
        } else {
            ChunkingAlgorithm::FixedSize
        };

        Rs3gwDedupConfig::new(self.block_size)
            .map(|config| {
                config
                    .with_algorithm(algorithm)
                    .with_min_size(self.min_object_size)
            })
            .map_err(|e| format!("Invalid dedup configuration: {e}"))
    }

    /// Disables deduplication
    #[must_use]
    pub fn disabled() -> Rs3gwDedupConfig {
        Rs3gwDedupConfig::disabled()
    }
}

/// Predefined configurations for common Zarr chunk sizes
pub struct ZarrDedupPresets;

impl ZarrDedupPresets {
    /// Configuration for small chunks (64 KB)
    ///
    /// Suitable for high-resolution imagery or detailed datasets
    #[must_use]
    pub fn small_chunks() -> ZarrDedupConfig {
        ZarrDedupConfig::for_chunk_size(64 * 1024)
    }

    /// Configuration for medium chunks (256 KB)
    ///
    /// Suitable for general-purpose geospatial data
    #[must_use]
    pub fn medium_chunks() -> ZarrDedupConfig {
        ZarrDedupConfig::for_chunk_size(256 * 1024)
    }

    /// Configuration for large chunks (1 MB)
    ///
    /// Suitable for large-scale climate or remote sensing data
    #[must_use]
    pub fn large_chunks() -> ZarrDedupConfig {
        ZarrDedupConfig::for_chunk_size(1024 * 1024)
    }

    /// Configuration for very large chunks (4 MB)
    ///
    /// Suitable for low-resolution global datasets
    #[must_use]
    pub fn xlarge_chunks() -> ZarrDedupConfig {
        ZarrDedupConfig::for_chunk_size(4 * 1024 * 1024).with_content_defined(true) // Use CDC for very large chunks
    }
}

/// Estimates potential storage savings from deduplication
///
/// # Arguments
/// * `total_chunks` - Total number of chunks in the Zarr array
/// * `unique_chunks` - Estimated number of unique chunks (use sampling)
///
/// # Returns
/// Estimated storage savings as a percentage (0.0 to 1.0)
#[must_use]
pub fn estimate_savings(total_chunks: usize, unique_chunks: usize) -> f64 {
    if total_chunks == 0 {
        return 0.0;
    }

    let duplicate_chunks = total_chunks.saturating_sub(unique_chunks);
    duplicate_chunks as f64 / total_chunks as f64
}

/// Calculates optimal chunk size for deduplication
///
/// # Arguments
/// * `array_shape` - Shape of the Zarr array (e.g., [time, z, y, x])
/// * `dtype_size` - Size of each element in bytes
/// * `target_chunk_mb` - Target chunk size in MB
///
/// # Returns
/// Recommended chunk shape
#[must_use]
pub fn calculate_optimal_chunk_shape(
    array_shape: &[usize],
    dtype_size: usize,
    target_chunk_mb: f64,
) -> Vec<usize> {
    let target_elements = ((target_chunk_mb * 1024.0 * 1024.0) / dtype_size as f64) as usize;

    let ndims = array_shape.len();
    let elements_per_dim = (target_elements as f64).powf(1.0 / ndims as f64) as usize;

    array_shape
        .iter()
        .map(|&dim_size| elements_per_dim.min(dim_size).max(1))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ZarrDedupConfig::default();
        assert_eq!(config.block_size, 256 * 1024);
        assert!(!config.content_defined);
        assert!(config.aggressive_nodata);
    }

    #[test]
    fn test_for_chunk_size() {
        let config = ZarrDedupConfig::for_chunk_size(512 * 1024);
        assert_eq!(config.block_size, 512 * 1024);
        assert_eq!(config.min_object_size, 256 * 1024);
    }

    #[test]
    fn test_presets() {
        let small = ZarrDedupPresets::small_chunks();
        assert_eq!(small.block_size, 64 * 1024);

        let medium = ZarrDedupPresets::medium_chunks();
        assert_eq!(medium.block_size, 256 * 1024);

        let large = ZarrDedupPresets::large_chunks();
        assert_eq!(large.block_size, 1024 * 1024);

        let xlarge = ZarrDedupPresets::xlarge_chunks();
        assert_eq!(xlarge.block_size, 4 * 1024 * 1024);
        assert!(xlarge.content_defined);
    }

    #[test]
    fn test_estimate_savings() {
        // 50% duplicate chunks
        let savings = estimate_savings(1000, 500);
        assert!((savings - 0.5).abs() < 0.01);

        // 70% duplicate chunks (typical for nodata-heavy datasets)
        let savings = estimate_savings(10000, 3000);
        assert!((savings - 0.7).abs() < 0.01);

        // No duplicates
        let savings = estimate_savings(1000, 1000);
        assert_eq!(savings, 0.0);

        // Empty array
        let savings = estimate_savings(0, 0);
        assert_eq!(savings, 0.0);
    }

    #[test]
    fn test_calculate_optimal_chunk_shape() {
        // 3D array (100, 1000, 1000) with float32 (4 bytes)
        // Target: 1 MB chunks
        let shape = vec![100, 1000, 1000];
        let chunk_shape = calculate_optimal_chunk_shape(&shape, 4, 1.0);

        assert_eq!(chunk_shape.len(), 3);
        // Should be approximately cubic root of (1MB / 4 bytes) elements per dimension
        let total_elements: usize = chunk_shape.iter().product();
        let total_bytes = total_elements * 4;
        assert!((500_000..=2_000_000).contains(&total_bytes));
    }

    #[test]
    fn test_to_rs3gw_config() {
        let config = ZarrDedupConfig::new()
            .with_block_size(256 * 1024)
            .with_content_defined(false);

        let rs3gw_config = config.to_rs3gw_config().expect("should convert");
        assert!(rs3gw_config.enabled);
    }
}
