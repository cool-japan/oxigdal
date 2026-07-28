//! COG-optimized caching configuration
//!
//! This module provides caching strategies optimized for Cloud-Optimized GeoTIFF (COG)
//! tile access patterns.
//!
//! # Prefetching
//!
//! The `ml_prefetch`/`prefetch_radius`/`ml_training_threshold` knobs enable
//! **real** background read-ahead prefetching (see
//! [`to_concurrent_read_config`](CogCacheConfig::to_concurrent_read_config) and
//! [`crate::datasource::Rs3gwDataSource`]): after a cache-miss read, the next
//! `prefetch_radius` contiguous same-sized chunks are eagerly fetched in the
//! background and stashed in the cache, once at least
//! `ml_training_threshold` reads have been observed on that data source.
//! Despite the field names (kept for API compatibility), this is a
//! deterministic contiguous-access heuristic, **not** a trained/learned
//! machine-learning model -- there is no model, no feature vector, and no
//! training step involved.

use crate::datasource::ConcurrentReadConfig;
use rs3gw::storage::CacheConfig as Rs3gwCacheConfig;

/// COG-optimized cache configuration
///
/// This configuration is tuned for efficient COG tile caching, with
/// heuristic (not ML-based) read-ahead prefetching to warm the cache for
/// tiles that are likely to be accessed next based on contiguous access
/// patterns. See [`to_concurrent_read_config`](Self::to_concurrent_read_config)
/// to turn this into a real, wired-up [`ConcurrentReadConfig`] for use with
/// [`crate::datasource::Rs3gwDataSource`].
#[derive(Debug, Clone)]
pub struct CogCacheConfig {
    /// Maximum cache size in MB (default: 512 MB)
    pub max_size_mb: u64,

    /// Maximum number of tiles to cache (default: 10,000)
    pub max_tiles: usize,

    /// TTL for cached tiles in seconds (default: 3600 = 1 hour)
    pub tile_ttl_secs: u64,

    /// Enable heuristic background read-ahead prefetch (default: true)
    ///
    /// Note: despite the name (kept for API compatibility), this is a
    /// deterministic contiguous-range look-ahead, not a machine-learned
    /// prediction.
    pub ml_prefetch: bool,

    /// Prefetch radius in tiles (default: 2)
    ///
    /// When a tile is accessed, prefetch surrounding tiles within this radius
    pub prefetch_radius: u32,

    /// Minimum access count on a data source before prefetching activates
    /// (default: 100). Acts as a warm-up gate so a data source that is only
    /// ever read once doesn't trigger background prefetch traffic. Despite
    /// the name (kept for API compatibility), no model training occurs.
    pub ml_training_threshold: usize,
}

impl Default for CogCacheConfig {
    fn default() -> Self {
        Self {
            max_size_mb: 512,
            max_tiles: 10_000,
            tile_ttl_secs: 3600, // 1 hour
            ml_prefetch: true,
            prefetch_radius: 2,
            ml_training_threshold: 100,
        }
    }
}

impl CogCacheConfig {
    /// Creates a new COG cache configuration with default values
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the maximum cache size in MB
    #[must_use]
    pub fn with_max_size_mb(mut self, max_size_mb: u64) -> Self {
        self.max_size_mb = max_size_mb;
        self
    }

    /// Sets the maximum number of tiles
    #[must_use]
    pub fn with_max_tiles(mut self, max_tiles: usize) -> Self {
        self.max_tiles = max_tiles;
        self
    }

    /// Sets the tile TTL in seconds
    #[must_use]
    pub fn with_tile_ttl(mut self, ttl_secs: u64) -> Self {
        self.tile_ttl_secs = ttl_secs;
        self
    }

    /// Enables or disables ML-based prefetching
    #[must_use]
    pub fn with_ml_prefetch(mut self, enabled: bool) -> Self {
        self.ml_prefetch = enabled;
        self
    }

    /// Sets the prefetch radius
    #[must_use]
    pub fn with_prefetch_radius(mut self, radius: u32) -> Self {
        self.prefetch_radius = radius;
        self
    }

    /// Sets the ML training threshold
    #[must_use]
    pub fn with_ml_training_threshold(mut self, threshold: usize) -> Self {
        self.ml_training_threshold = threshold;
        self
    }

    /// Converts to rs3gw's CacheConfig
    ///
    /// This only carries over the raw cache-sizing knobs (`max_size_mb`,
    /// `max_tiles`, `tile_ttl_secs`); rs3gw's own `CacheConfig` has no
    /// concept of prefetching at all. Use
    /// [`to_concurrent_read_config`](Self::to_concurrent_read_config) if you
    /// want the `ml_prefetch`/`prefetch_radius`/`ml_training_threshold`
    /// knobs to actually take effect.
    #[must_use]
    pub fn to_rs3gw_config(&self) -> Rs3gwCacheConfig {
        Rs3gwCacheConfig::default()
            .with_max_size_mb(self.max_size_mb)
            .with_max_objects(self.max_tiles)
            .with_ttl_secs(self.tile_ttl_secs)
    }

    /// Converts to a [`ConcurrentReadConfig`] that actually wires up this
    /// configuration's prefetch knobs to
    /// [`crate::datasource::Rs3gwDataSource`]'s real background read-ahead
    /// prefetcher. This is the config to pass to
    /// `Rs3gwDataSource::new_with_config`/`new_with_size_and_config` if you
    /// want `ml_prefetch`/`prefetch_radius`/`ml_training_threshold` to have
    /// any runtime effect.
    #[must_use]
    pub fn to_concurrent_read_config(&self) -> ConcurrentReadConfig {
        ConcurrentReadConfig::new()
            .with_cache(true)
            .with_cache_config(self.max_tiles as u64, self.tile_ttl_secs)
            .with_prefetch_radius(self.prefetch_radius as usize)
            .with_spatial_prefetch(self.ml_prefetch)
            .with_prefetch_warmup_reads(self.ml_training_threshold)
    }
}

/// Configuration for different COG access patterns
#[derive(Debug, Clone, Copy)]
pub enum CogAccessPattern {
    /// Sequential tile access (e.g., rendering from top-left to bottom-right)
    Sequential,

    /// Random tile access (e.g., user panning and zooming)
    Random,

    /// Regional access (e.g., viewing a specific area)
    Regional,

    /// Multi-scale pyramid access (e.g., zooming through overview levels)
    Pyramid,
}

impl CogAccessPattern {
    /// Returns recommended cache configuration for this access pattern
    #[must_use]
    pub fn recommended_config(&self) -> CogCacheConfig {
        match self {
            Self::Sequential => CogCacheConfig {
                max_size_mb: 256,
                max_tiles: 5_000,
                tile_ttl_secs: 1800, // 30 minutes
                ml_prefetch: true,
                prefetch_radius: 3, // Aggressive prefetch
                ml_training_threshold: 50,
            },

            Self::Random => CogCacheConfig {
                max_size_mb: 1024,
                max_tiles: 20_000,
                tile_ttl_secs: 7200, // 2 hours
                ml_prefetch: true,
                prefetch_radius: 1, // Conservative prefetch
                ml_training_threshold: 200,
            },

            Self::Regional => CogCacheConfig {
                max_size_mb: 512,
                max_tiles: 10_000,
                tile_ttl_secs: 3600, // 1 hour
                ml_prefetch: true,
                prefetch_radius: 2,
                ml_training_threshold: 100,
            },

            Self::Pyramid => CogCacheConfig {
                max_size_mb: 2048,
                max_tiles: 50_000,
                tile_ttl_secs: 10800, // 3 hours
                ml_prefetch: true,
                prefetch_radius: 2,
                ml_training_threshold: 150,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = CogCacheConfig::default();
        assert_eq!(config.max_size_mb, 512);
        assert_eq!(config.max_tiles, 10_000);
        assert_eq!(config.tile_ttl_secs, 3600);
        assert!(config.ml_prefetch);
    }

    #[test]
    fn test_builder() {
        let config = CogCacheConfig::new()
            .with_max_size_mb(1024)
            .with_max_tiles(20_000)
            .with_tile_ttl(7200)
            .with_ml_prefetch(false)
            .with_prefetch_radius(3);

        assert_eq!(config.max_size_mb, 1024);
        assert_eq!(config.max_tiles, 20_000);
        assert_eq!(config.tile_ttl_secs, 7200);
        assert!(!config.ml_prefetch);
        assert_eq!(config.prefetch_radius, 3);
    }

    #[test]
    fn test_access_pattern_configs() {
        let sequential = CogAccessPattern::Sequential.recommended_config();
        assert_eq!(sequential.prefetch_radius, 3);

        let random = CogAccessPattern::Random.recommended_config();
        assert_eq!(random.max_size_mb, 1024);

        let regional = CogAccessPattern::Regional.recommended_config();
        assert_eq!(regional.max_tiles, 10_000);

        let pyramid = CogAccessPattern::Pyramid.recommended_config();
        assert_eq!(pyramid.max_size_mb, 2048);
    }
}
