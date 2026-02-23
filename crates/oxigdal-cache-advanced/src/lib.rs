//! Advanced Multi-Tier Caching for OxiGDAL
//!
//! This crate provides intelligent caching with:
//! - Multi-tier cache (L1: memory, L2: SSD, L3: network)
//! - Predictive prefetching with ML
//! - Adaptive compression
//! - Distributed cache protocol
//! - Cache analytics and warming
//! - Advanced eviction policies

#![warn(missing_docs)]
#![deny(unsafe_code)]

pub mod analytics;
pub mod coherency;
pub mod compression;
pub mod distributed;
pub mod error;
pub mod eviction;
pub mod multi_tier;
pub mod observability;
pub mod partitioning;
pub mod predictive;
pub mod tiering;
pub mod warming;
pub mod write_policy;

pub use error::{CacheError, Result};
pub use multi_tier::{CacheKey, CacheTier, CacheValue, MultiTierCache};

/// Cache statistics
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CacheStats {
    /// Total number of cache hits
    pub hits: u64,
    /// Total number of cache misses
    pub misses: u64,
    /// Number of evictions
    pub evictions: u64,
    /// Total bytes stored
    pub bytes_stored: u64,
    /// Number of items in cache
    pub item_count: usize,
}

impl CacheStats {
    /// Create new cache statistics
    pub fn new() -> Self {
        Self {
            hits: 0,
            misses: 0,
            evictions: 0,
            bytes_stored: 0,
            item_count: 0,
        }
    }

    /// Calculate hit rate as a percentage
    pub fn hit_rate(&self) -> f64 {
        if self.hits + self.misses == 0 {
            0.0
        } else {
            (self.hits as f64) / ((self.hits + self.misses) as f64) * 100.0
        }
    }

    /// Calculate average item size in bytes
    pub fn avg_item_size(&self) -> f64 {
        if self.item_count == 0 {
            0.0
        } else {
            (self.bytes_stored as f64) / (self.item_count as f64)
        }
    }
}

impl Default for CacheStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Cache configuration
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CacheConfig {
    /// L1 cache size in bytes
    pub l1_size: usize,
    /// L2 cache size in bytes
    pub l2_size: usize,
    /// L3 cache size in bytes
    pub l3_size: usize,
    /// Enable compression
    pub enable_compression: bool,
    /// Enable predictive prefetching
    pub enable_prefetch: bool,
    /// Enable distributed cache
    pub enable_distributed: bool,
    /// Cache directory for disk-based tiers
    pub cache_dir: Option<std::path::PathBuf>,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            l1_size: 128 * 1024 * 1024,       // 128 MB
            l2_size: 1024 * 1024 * 1024,      // 1 GB
            l3_size: 10 * 1024 * 1024 * 1024, // 10 GB
            enable_compression: true,
            enable_prefetch: true,
            enable_distributed: false,
            cache_dir: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_stats_hit_rate() {
        let mut stats = CacheStats::new();
        stats.hits = 80;
        stats.misses = 20;

        approx::assert_relative_eq!(stats.hit_rate(), 80.0, epsilon = 0.01);
    }

    #[test]
    fn test_cache_stats_avg_size() {
        let mut stats = CacheStats::new();
        stats.bytes_stored = 1000;
        stats.item_count = 10;

        approx::assert_relative_eq!(stats.avg_item_size(), 100.0, epsilon = 0.01);
    }

    #[test]
    fn test_default_config() {
        let config = CacheConfig::default();
        assert!(config.enable_compression);
        assert!(config.enable_prefetch);
        assert!(!config.enable_distributed);
    }
}
