//! Distributed cache with coherency protocol.
//!
//! This module implements a distributed cache system with cache coherency,
//! distributed LRU eviction, cache warming, and compression.

use crate::error::{ClusterError, Result};
use crate::worker_pool::WorkerId;
use dashmap::DashMap;
use lru::LruCache;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Distributed cache manager.
#[derive(Clone)]
pub struct DistributedCache {
    inner: Arc<DistributedCacheInner>,
}

struct DistributedCacheInner {
    /// Local cache (per node)
    local_cache: Arc<RwLock<LruCache<CacheKey, CacheEntry>>>,

    /// Cache directory (key -> locations)
    cache_directory: DashMap<CacheKey, HashSet<WorkerId>>,

    /// Invalidation queue
    invalidations: DashMap<CacheKey, InvalidationRecord>,

    /// Configuration
    config: CacheConfig,

    /// Statistics
    stats: Arc<CacheStatistics>,
}

/// Cache configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// Maximum local cache size (number of entries)
    pub max_local_entries: usize,

    /// Maximum entry size (bytes)
    pub max_entry_size: usize,

    /// Enable compression
    pub enable_compression: bool,

    /// Compression threshold (bytes)
    pub compression_threshold: usize,

    /// Cache entry TTL
    pub entry_ttl: Duration,

    /// Coherency protocol
    pub coherency_protocol: CoherencyProtocol,

    /// Enable cache warming
    pub enable_warming: bool,

    /// Warming prefetch size
    pub warming_prefetch_size: usize,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_local_entries: 10000,
            max_entry_size: 100 * 1024 * 1024, // 100 MB
            enable_compression: true,
            compression_threshold: 1024, // 1 KB
            entry_ttl: Duration::from_secs(3600),
            coherency_protocol: CoherencyProtocol::Invalidation,
            enable_warming: true,
            warming_prefetch_size: 100,
        }
    }
}

/// Cache coherency protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CoherencyProtocol {
    /// Invalidation-based (invalidate copies on write)
    Invalidation,

    /// Update-based (update all copies on write)
    Update,
}

/// Cache key.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CacheKey {
    /// Namespace
    pub namespace: String,

    /// Key
    pub key: String,
}

impl CacheKey {
    /// Create a new cache key.
    pub fn new(namespace: String, key: String) -> Self {
        Self { namespace, key }
    }
}

/// Cache entry.
#[derive(Debug, Clone)]
pub struct CacheEntry {
    /// Entry data
    pub data: Vec<u8>,

    /// Compressed flag
    pub compressed: bool,

    /// Version
    pub version: u64,

    /// Creation time
    pub created_at: Instant,

    /// Last accessed time
    pub last_accessed: Instant,

    /// Access count
    pub access_count: u64,

    /// Size in bytes (uncompressed)
    pub size_bytes: usize,
}

/// Invalidation record.
#[derive(Debug, Clone)]
pub struct InvalidationRecord {
    /// Cache key
    pub key: CacheKey,

    /// Invalidation version
    pub version: u64,

    /// Timestamp
    pub timestamp: Instant,

    /// Invalidated workers
    pub workers: HashSet<WorkerId>,
}

/// Cache statistics.
#[derive(Debug, Default)]
struct CacheStatistics {
    /// Cache hits
    hits: AtomicU64,

    /// Cache misses
    misses: AtomicU64,

    /// Evictions
    evictions: AtomicU64,

    /// Invalidations
    invalidations: AtomicU64,

    /// Compressions
    compressions: AtomicU64,

    /// Decompressions
    decompressions: AtomicU64,

    /// Bytes stored (compressed)
    bytes_stored: AtomicU64,

    /// Bytes saved by compression
    bytes_saved: AtomicU64,
}

impl DistributedCache {
    /// Default cache capacity when configured value is zero.
    const DEFAULT_CAPACITY: usize = 1000;

    /// Create a new distributed cache.
    pub fn new(config: CacheConfig) -> Self {
        // NonZeroUsize::new returns None for 0, so we use a default capacity in that case
        // Using MIN (which is 1) as the fallback ensures we always have a valid NonZeroUsize
        let capacity = NonZeroUsize::new(config.max_local_entries)
            .unwrap_or(NonZeroUsize::new(Self::DEFAULT_CAPACITY).unwrap_or(NonZeroUsize::MIN));

        Self {
            inner: Arc::new(DistributedCacheInner {
                local_cache: Arc::new(RwLock::new(LruCache::new(capacity))),
                cache_directory: DashMap::new(),
                invalidations: DashMap::new(),
                config,
                stats: Arc::new(CacheStatistics::default()),
            }),
        }
    }

    /// Create with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(CacheConfig::default())
    }

    /// Put entry in cache.
    pub fn put(&self, key: CacheKey, data: Vec<u8>, worker_id: WorkerId) -> Result<()> {
        if data.len() > self.inner.config.max_entry_size {
            return Err(ClusterError::CacheError(
                "Entry size exceeds maximum".to_string(),
            ));
        }

        let original_size = data.len();

        // Compress if enabled and above threshold
        let (data, compressed) = if self.inner.config.enable_compression
            && data.len() > self.inner.config.compression_threshold
        {
            match self.compress_data(&data) {
                Ok(compressed_data) => {
                    let saved = original_size.saturating_sub(compressed_data.len());
                    self.inner
                        .stats
                        .bytes_saved
                        .fetch_add(saved as u64, Ordering::Relaxed);
                    self.inner
                        .stats
                        .compressions
                        .fetch_add(1, Ordering::Relaxed);
                    (compressed_data, true)
                }
                Err(_) => (data, false),
            }
        } else {
            (data, false)
        };

        let entry = CacheEntry {
            data: data.clone(),
            compressed,
            version: 1,
            created_at: Instant::now(),
            last_accessed: Instant::now(),
            access_count: 0,
            size_bytes: original_size,
        };

        // Store in local cache
        let mut cache = self.inner.local_cache.write();
        if let Some((evicted_key, _)) = cache.push(key.clone(), entry) {
            self.inner.stats.evictions.fetch_add(1, Ordering::Relaxed);

            // Remove from directory
            self.inner.cache_directory.remove(&evicted_key);
        }
        drop(cache);

        // Update directory
        self.inner
            .cache_directory
            .entry(key)
            .or_default()
            .insert(worker_id);

        self.inner
            .stats
            .bytes_stored
            .fetch_add(data.len() as u64, Ordering::Relaxed);

        Ok(())
    }

    /// Get entry from cache.
    pub fn get(&self, key: &CacheKey) -> Result<Option<Vec<u8>>> {
        let mut cache = self.inner.local_cache.write();

        if let Some(entry) = cache.get_mut(key) {
            entry.last_accessed = Instant::now();
            entry.access_count += 1;

            self.inner.stats.hits.fetch_add(1, Ordering::Relaxed);

            // Decompress if needed
            let data = if entry.compressed {
                self.inner
                    .stats
                    .decompressions
                    .fetch_add(1, Ordering::Relaxed);
                self.decompress_data(&entry.data)?
            } else {
                entry.data.clone()
            };

            Ok(Some(data))
        } else {
            self.inner.stats.misses.fetch_add(1, Ordering::Relaxed);
            Ok(None)
        }
    }

    /// Remove entry from cache.
    pub fn remove(&self, key: &CacheKey, worker_id: WorkerId) -> Result<()> {
        // Remove from local cache
        let mut cache = self.inner.local_cache.write();
        cache.pop(key);
        drop(cache);

        // Update directory
        if let Some(mut locations) = self.inner.cache_directory.get_mut(key) {
            locations.remove(&worker_id);
            if locations.is_empty() {
                drop(locations);
                self.inner.cache_directory.remove(key);
            }
        }

        Ok(())
    }

    /// Invalidate cache entry (coherency protocol).
    pub fn invalidate(&self, key: CacheKey, version: u64) -> Result<Vec<WorkerId>> {
        // Get all workers with this entry
        let workers = self
            .inner
            .cache_directory
            .get(&key)
            .map(|locs| locs.iter().copied().collect::<Vec<_>>())
            .unwrap_or_default();

        // Record invalidation
        let invalidation = InvalidationRecord {
            key: key.clone(),
            version,
            timestamp: Instant::now(),
            workers: workers.iter().copied().collect(),
        };

        self.inner.invalidations.insert(key.clone(), invalidation);

        // Remove from local cache
        let mut cache = self.inner.local_cache.write();
        cache.pop(&key);
        drop(cache);

        // Clear directory
        self.inner.cache_directory.remove(&key);

        self.inner
            .stats
            .invalidations
            .fetch_add(1, Ordering::Relaxed);

        Ok(workers)
    }

    /// Check if entry exists in cache.
    pub fn contains(&self, key: &CacheKey) -> bool {
        self.inner.local_cache.write().contains(key)
    }

    /// Get cache entry locations.
    pub fn get_locations(&self, key: &CacheKey) -> Vec<WorkerId> {
        self.inner
            .cache_directory
            .get(key)
            .map(|locs| locs.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Warm cache with entries.
    pub fn warm_cache(&self, keys: Vec<CacheKey>, worker_id: WorkerId) -> Result<usize> {
        if !self.inner.config.enable_warming {
            return Ok(0);
        }

        let mut warmed = 0;

        for key in keys
            .into_iter()
            .take(self.inner.config.warming_prefetch_size)
        {
            // Check if already in cache
            if self.contains(&key) {
                continue;
            }

            // Mark as available on this worker (would need to fetch in real impl)
            self.inner
                .cache_directory
                .entry(key)
                .or_default()
                .insert(worker_id);

            warmed += 1;
        }

        Ok(warmed)
    }

    /// Compress data using zstd.
    fn compress_data(&self, data: &[u8]) -> Result<Vec<u8>> {
        oxiarc_zstd::encode_all(data, 3)
            .map_err(|e| ClusterError::CacheError(format!("Compression error: {}", e)))
    }

    /// Decompress data using zstd.
    fn decompress_data(&self, data: &[u8]) -> Result<Vec<u8>> {
        oxiarc_zstd::decode_all(data)
            .map_err(|e| ClusterError::CacheError(format!("Decompression error: {}", e)))
    }

    /// Evict expired entries.
    pub fn evict_expired(&self) -> usize {
        let mut cache = self.inner.local_cache.write();
        let now = Instant::now();
        let ttl = self.inner.config.entry_ttl;

        let expired_keys: Vec<_> = cache
            .iter()
            .filter(|(_, entry)| now.duration_since(entry.created_at) > ttl)
            .map(|(key, _)| key.clone())
            .collect();

        let count = expired_keys.len();

        for key in expired_keys {
            cache.pop(&key);
            self.inner.cache_directory.remove(&key);
        }

        self.inner
            .stats
            .evictions
            .fetch_add(count as u64, Ordering::Relaxed);

        count
    }

    /// Get cache statistics.
    pub fn get_statistics(&self) -> CacheStats {
        let hits = self.inner.stats.hits.load(Ordering::Relaxed);
        let misses = self.inner.stats.misses.load(Ordering::Relaxed);

        let total_requests = hits + misses;
        let hit_rate = if total_requests > 0 {
            hits as f64 / total_requests as f64
        } else {
            0.0
        };

        let bytes_stored = self.inner.stats.bytes_stored.load(Ordering::Relaxed);
        let bytes_saved = self.inner.stats.bytes_saved.load(Ordering::Relaxed);

        let compression_ratio = if bytes_stored > 0 {
            1.0 - (bytes_saved as f64 / bytes_stored as f64)
        } else {
            1.0
        };

        CacheStats {
            hits,
            misses,
            hit_rate,
            evictions: self.inner.stats.evictions.load(Ordering::Relaxed),
            invalidations: self.inner.stats.invalidations.load(Ordering::Relaxed),
            compressions: self.inner.stats.compressions.load(Ordering::Relaxed),
            decompressions: self.inner.stats.decompressions.load(Ordering::Relaxed),
            bytes_stored,
            bytes_saved,
            compression_ratio,
            total_entries: self.inner.local_cache.read().len(),
            directory_entries: self.inner.cache_directory.len(),
        }
    }

    /// Clear cache.
    pub fn clear(&self) {
        self.inner.local_cache.write().clear();
        self.inner.cache_directory.clear();
        self.inner.invalidations.clear();
    }
}

/// Cache statistics snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStats {
    /// Cache hits
    pub hits: u64,

    /// Cache misses
    pub misses: u64,

    /// Hit rate (0.0-1.0)
    pub hit_rate: f64,

    /// Evictions
    pub evictions: u64,

    /// Invalidations
    pub invalidations: u64,

    /// Compressions performed
    pub compressions: u64,

    /// Decompressions performed
    pub decompressions: u64,

    /// Bytes stored (compressed)
    pub bytes_stored: u64,

    /// Bytes saved by compression
    pub bytes_saved: u64,

    /// Compression ratio
    pub compression_ratio: f64,

    /// Total entries in local cache
    pub total_entries: usize,

    /// Total entries in directory
    pub directory_entries: usize,
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_creation() {
        let cache = DistributedCache::with_defaults();
        let stats = cache.get_statistics();
        assert_eq!(stats.hits, 0);
    }

    #[test]
    fn test_cache_put_get() {
        let cache = DistributedCache::with_defaults();
        let worker_id = WorkerId::new();
        let key = CacheKey::new("test".to_string(), "key1".to_string());
        let data = vec![1, 2, 3, 4, 5];

        cache.put(key.clone(), data.clone(), worker_id).ok();

        let result = cache.get(&key);
        assert!(result.is_ok());
        if let Ok(Some(retrieved)) = result {
            assert_eq!(retrieved, data);
        }
    }

    #[test]
    fn test_cache_invalidation() {
        let cache = DistributedCache::with_defaults();
        let worker_id = WorkerId::new();
        let key = CacheKey::new("test".to_string(), "key1".to_string());
        let data = vec![1, 2, 3, 4, 5];

        cache.put(key.clone(), data, worker_id).ok();
        assert!(cache.contains(&key));

        cache.invalidate(key.clone(), 2).ok();
        assert!(!cache.contains(&key));
    }

    #[test]
    fn test_cache_compression() {
        let config = CacheConfig {
            compression_threshold: 10,
            ..Default::default()
        };

        let cache = DistributedCache::new(config);
        let worker_id = WorkerId::new();
        let key = CacheKey::new("test".to_string(), "key1".to_string());
        let data = vec![1; 1000]; // Repeated data compresses well

        cache.put(key.clone(), data.clone(), worker_id).ok();

        let stats = cache.get_statistics();
        assert!(stats.compressions > 0);

        let result = cache.get(&key);
        assert!(result.is_ok());
        if let Ok(Some(retrieved)) = result {
            assert_eq!(retrieved, data);
        }
    }

    #[test]
    fn test_cache_hit_rate() {
        let cache = DistributedCache::with_defaults();
        let worker_id = WorkerId::new();

        let key1 = CacheKey::new("test".to_string(), "key1".to_string());
        cache.put(key1.clone(), vec![1, 2, 3], worker_id).ok();

        // Hit
        cache.get(&key1).ok();

        // Miss
        let key2 = CacheKey::new("test".to_string(), "key2".to_string());
        cache.get(&key2).ok();

        let stats = cache.get_statistics();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
        assert!((stats.hit_rate - 0.5).abs() < 0.01);
    }
}
