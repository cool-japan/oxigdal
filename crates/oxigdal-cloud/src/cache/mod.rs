//! Advanced multi-level caching layer for cloud storage
//!
//! This module provides a comprehensive caching system with:
//! - LRU cache with TTL (Time-To-Live)
//! - LFU (Least Frequently Used) cache
//! - ARC (Adaptive Replacement Cache)
//! - Spatial-aware caching for geospatial data
//! - Tile-based caching for COG/tile pyramids
//! - Persistent disk cache with metadata
//! - Cache warming strategies
//! - Configurable eviction policies

use std::path::PathBuf;
use std::time::Duration;

pub mod backends;
pub mod eviction;
pub mod metadata;
pub mod multi;

#[cfg(test)]
mod tests;

// Re-export main types
pub use metadata::{
    CacheEntry, CacheKey, CacheStats, DiskCacheMetadata, LevelStats, SpatialInfo, TileCoord,
};

#[cfg(feature = "cache")]
pub use eviction::{ArcCache, LfuCache, LruTtlCache};

#[cfg(feature = "cache")]
pub use backends::{PersistentDiskCache, SpatialCache, TileCache};

#[cfg(feature = "cache")]
pub use multi::{CacheWarmer, DiskCache, MemoryCache, MultiLevelCache};

/// Cache eviction strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EvictionStrategy {
    /// Least Recently Used
    Lru,
    /// Least Frequently Used
    Lfu,
    /// Adaptive (combines LRU and LFU using ARC algorithm)
    #[default]
    Adaptive,
    /// Time-based eviction (oldest entries first)
    TimeToLive,
    /// Size-based eviction (largest entries first)
    LargestFirst,
    /// Random eviction
    Random,
}

/// Cache warming strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WarmingStrategy {
    /// No pre-warming
    #[default]
    None,
    /// Warm based on access patterns
    AccessPattern,
    /// Warm spatially adjacent tiles
    SpatialAdjacent,
    /// Warm pyramid levels (for tile caching)
    PyramidLevels,
    /// Custom warming function
    Custom,
}

/// Cache configuration
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// Maximum memory cache size in bytes
    pub max_memory_size: usize,
    /// Maximum disk cache size in bytes
    pub max_disk_size: usize,
    /// Whether to enable compression
    pub compress: bool,
    /// Compression threshold (compress if larger than this)
    pub compress_threshold: usize,
    /// Eviction strategy
    pub eviction_strategy: EvictionStrategy,
    /// Whether to enable persistent disk cache
    pub persistent: bool,
    /// Disk cache directory
    pub cache_dir: Option<PathBuf>,
    /// Default TTL for entries
    pub default_ttl: Option<Duration>,
    /// Maximum entry age before eviction
    pub max_age: Option<Duration>,
    /// Maximum number of entries
    pub max_entries: usize,
    /// Cache warming strategy
    pub warming_strategy: WarmingStrategy,
    /// Number of entries to warm at a time
    pub warm_batch_size: usize,
    /// Whether to track spatial metadata
    pub spatial_aware: bool,
    /// Target ARC adaptation rate (0.0-1.0)
    pub arc_adaptation_rate: f64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_memory_size: 100 * 1024 * 1024, // 100 MB
            max_disk_size: 1024 * 1024 * 1024,  // 1 GB
            compress: true,
            compress_threshold: 4096, // 4 KB
            eviction_strategy: EvictionStrategy::Adaptive,
            persistent: true,
            cache_dir: None,
            default_ttl: Some(Duration::from_secs(3600)), // 1 hour
            max_age: Some(Duration::from_secs(3600)),     // 1 hour
            max_entries: 10000,
            warming_strategy: WarmingStrategy::None,
            warm_batch_size: 10,
            spatial_aware: false,
            arc_adaptation_rate: 0.5,
        }
    }
}

impl CacheConfig {
    /// Creates a new cache configuration
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the maximum memory cache size
    #[must_use]
    pub fn with_max_memory_size(mut self, size: usize) -> Self {
        self.max_memory_size = size;
        self
    }

    /// Sets the maximum disk cache size
    #[must_use]
    pub fn with_max_disk_size(mut self, size: usize) -> Self {
        self.max_disk_size = size;
        self
    }

    /// Enables or disables compression
    #[must_use]
    pub fn with_compress(mut self, compress: bool) -> Self {
        self.compress = compress;
        self
    }

    /// Sets the eviction strategy
    #[must_use]
    pub fn with_eviction_strategy(mut self, strategy: EvictionStrategy) -> Self {
        self.eviction_strategy = strategy;
        self
    }

    /// Sets the cache directory
    #[must_use]
    pub fn with_cache_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.cache_dir = Some(dir.into());
        self
    }

    /// Sets the default TTL
    #[must_use]
    pub fn with_default_ttl(mut self, ttl: Duration) -> Self {
        self.default_ttl = Some(ttl);
        self
    }

    /// Sets the maximum entry age
    #[must_use]
    pub fn with_max_age(mut self, duration: Duration) -> Self {
        self.max_age = Some(duration);
        self
    }

    /// Sets the maximum number of entries
    #[must_use]
    pub fn with_max_entries(mut self, count: usize) -> Self {
        self.max_entries = count;
        self
    }

    /// Sets the warming strategy
    #[must_use]
    pub fn with_warming_strategy(mut self, strategy: WarmingStrategy) -> Self {
        self.warming_strategy = strategy;
        self
    }

    /// Enables spatial awareness
    #[must_use]
    pub fn with_spatial_aware(mut self, enabled: bool) -> Self {
        self.spatial_aware = enabled;
        self
    }
}
