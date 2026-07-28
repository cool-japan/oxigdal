//! Tile caching system
//!
//! Provides multi-level caching for rendered tiles:
//! - In-memory LRU cache for fast access
//! - Optional disk cache for persistence
//! - Cache statistics and monitoring

use bytes::Bytes;
use lru::LruCache;
use std::fmt;
use std::hash::Hash;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use thiserror::Error;
use tracing::{debug, trace, warn};

/// Cache errors
#[derive(Debug, Error)]
pub enum CacheError {
    /// I/O error
    #[error("Cache I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Invalid cache key
    #[error("Invalid cache key")]
    InvalidKey,

    /// Cache is full
    #[error("Cache is full")]
    Full,

    /// An internal lock was poisoned by a panic in another thread
    #[error("Cache lock poisoned (a panic occurred while a lock was held)")]
    Poisoned,
}

/// Result type for cache operations
pub type CacheResult<T> = Result<T, CacheError>;

/// Cache key for tiles
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey {
    /// Layer name
    pub layer: String,

    /// Zoom level
    pub z: u8,

    /// Tile X coordinate
    pub x: u32,

    /// Tile Y coordinate
    pub y: u32,

    /// Image format extension (png, jpg, webp)
    pub format: String,

    /// Optional style name
    pub style: Option<String>,
}

impl CacheKey {
    /// Create a new cache key
    pub fn new(layer: String, z: u8, x: u32, y: u32, format: String) -> Self {
        Self {
            layer,
            z,
            x,
            y,
            format,
            style: None,
        }
    }

    /// Create a cache key with style
    pub fn with_style(mut self, style: String) -> Self {
        self.style = Some(style);
        self
    }

    /// Get the file path for disk cache
    pub fn to_path(&self, base_dir: &Path) -> PathBuf {
        let mut path = base_dir.to_path_buf();
        path.push(&self.layer);

        if let Some(ref style) = self.style {
            path.push(style);
        }

        path.push(self.z.to_string());
        path.push(self.x.to_string());
        path.push(format!("{}.{}", self.y, self.format));
        path
    }
}

impl fmt::Display for CacheKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(ref style) = self.style {
            write!(
                f,
                "{}/{}/{}/{}/{}.{}",
                self.layer, style, self.z, self.x, self.y, self.format
            )
        } else {
            write!(
                f,
                "{}/{}/{}/{}.{}",
                self.layer, self.z, self.x, self.y, self.format
            )
        }
    }
}

/// Cached tile entry
#[derive(Debug, Clone)]
struct CacheEntry {
    /// Tile data
    data: Bytes,

    /// When this entry was created
    created_at: Instant,

    /// Entry size in bytes
    size: usize,

    /// Access count
    access_count: u64,
}

impl CacheEntry {
    /// Create a new cache entry
    fn new(data: Bytes) -> Self {
        let size = data.len();
        Self {
            data,
            created_at: Instant::now(),
            size,
            access_count: 0,
        }
    }

    /// Check if this entry has expired
    fn is_expired(&self, ttl: Duration) -> bool {
        self.created_at.elapsed() > ttl
    }

    /// Record an access
    fn record_access(&mut self) {
        self.access_count += 1;
    }
}

/// Cache statistics
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    /// Number of cache hits
    pub hits: u64,

    /// Number of cache misses
    pub misses: u64,

    /// Total number of entries
    pub entry_count: usize,

    /// Total size in bytes
    pub total_size: usize,

    /// Number of evictions
    pub evictions: u64,

    /// Number of expirations
    pub expirations: u64,

    /// Number of disk reads
    pub disk_reads: u64,

    /// Number of disk writes
    pub disk_writes: u64,

    /// Number of cache-insertion failures (memory-promotion or disk-write errors)
    pub put_failures: u64,
}

impl CacheStats {
    /// Calculate hit rate
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }

    /// Calculate average entry size
    pub fn avg_entry_size(&self) -> f64 {
        if self.entry_count == 0 {
            0.0
        } else {
            self.total_size as f64 / self.entry_count as f64
        }
    }
}

/// Tile cache configuration
#[derive(Debug, Clone)]
pub struct TileCacheConfig {
    /// Maximum memory size in bytes
    pub max_memory_bytes: usize,

    /// Optional disk cache directory
    pub disk_cache_dir: Option<PathBuf>,

    /// Time-to-live for cached entries
    pub ttl: Duration,

    /// Enable statistics tracking
    pub enable_stats: bool,

    /// Compress cached data
    pub compression: bool,
}

impl Default for TileCacheConfig {
    fn default() -> Self {
        Self {
            max_memory_bytes: 256 * 1024 * 1024, // 256 MB
            disk_cache_dir: None,
            ttl: Duration::from_secs(3600), // 1 hour
            enable_stats: true,
            compression: false,
        }
    }
}

/// Multi-level tile cache
pub struct TileCache {
    /// In-memory LRU cache
    memory_cache: Arc<Mutex<LruCache<CacheKey, CacheEntry>>>,

    /// Current memory usage
    memory_usage: Arc<Mutex<usize>>,

    /// Cache configuration
    config: TileCacheConfig,

    /// Cache statistics
    stats: Arc<Mutex<CacheStats>>,
}

/// Minimum cache capacity (100 entries)
const MIN_CACHE_CAPACITY: NonZeroUsize = match NonZeroUsize::new(100) {
    Some(n) => n,
    None => unreachable!(),
};

impl TileCache {
    /// Create a new tile cache
    pub fn new(config: TileCacheConfig) -> Self {
        // Calculate capacity based on assumed average tile size of 10KB
        let estimated_capacity = config.max_memory_bytes / (10 * 1024);
        // Use min capacity of 100, max of estimated_capacity
        let capacity = NonZeroUsize::new(estimated_capacity)
            .unwrap_or(MIN_CACHE_CAPACITY)
            .max(MIN_CACHE_CAPACITY);

        Self {
            memory_cache: Arc::new(Mutex::new(LruCache::new(capacity))),
            memory_usage: Arc::new(Mutex::new(0)),
            config,
            stats: Arc::new(Mutex::new(CacheStats::default())),
        }
    }

    /// Get a tile from the cache
    pub fn get(&self, key: &CacheKey) -> Option<Bytes> {
        trace!("Cache lookup: {}", key.to_string());

        // Try memory cache first
        if let Some(data) = self.get_from_memory(key) {
            self.record_hit();
            return Some(data);
        }

        // Try disk cache if enabled
        if self.config.disk_cache_dir.is_some()
            && let Some(data) = self.get_from_disk(key)
        {
            // Promote to memory cache. A failure here is non-fatal (the disk hit
            // is still returned) but must not be silently swallowed, or a broken
            // memory cache would degrade to perpetual disk-only reads unnoticed.
            if let Err(e) = self.put_in_memory(key.clone(), data.clone()) {
                warn!("Failed to promote disk-cache hit into memory cache: {}", e);
                self.record_put_failure();
            }
            self.record_hit();
            return Some(data);
        }

        self.record_miss();
        None
    }

    /// Put a tile in the cache
    pub fn put(&self, key: CacheKey, data: Bytes) -> CacheResult<()> {
        trace!("Caching tile: {}", key.to_string());

        // Store in memory cache
        self.put_in_memory(key.clone(), data.clone())?;

        // Store in disk cache if enabled. A disk write failure (e.g. disk full or
        // permission denied) must not fail the overall put — the tile is already
        // in memory — but it must be surfaced so operators can detect a broken
        // disk cache instead of silent perpetual disk-cache misses.
        if self.config.disk_cache_dir.is_some()
            && let Err(e) = self.put_on_disk(&key, &data)
        {
            warn!("Failed to write tile to disk cache ({}): {}", key, e);
            self.record_put_failure();
        }

        Ok(())
    }

    /// Get from memory cache
    fn get_from_memory(&self, key: &CacheKey) -> Option<Bytes> {
        let mut cache = self.memory_cache.lock().ok()?;

        // Check if entry exists and is not expired
        let is_expired = if let Some(entry) = cache.peek(key) {
            entry.is_expired(self.config.ttl)
        } else {
            return None;
        };

        if is_expired {
            trace!("Entry expired: {}", key.to_string());
            self.record_expiration();
            let entry = cache.pop(key)?;
            self.update_memory_usage(|usage| usage.saturating_sub(entry.size));
            return None;
        }

        // Entry exists and is not expired - get it and record access
        if let Some(entry) = cache.get_mut(key) {
            entry.record_access();
            Some(entry.data.clone())
        } else {
            None
        }
    }

    /// Put in memory cache
    fn put_in_memory(&self, key: CacheKey, data: Bytes) -> CacheResult<()> {
        let entry = CacheEntry::new(data);
        let entry_size = entry.size;

        let mut cache = self.memory_cache.lock().map_err(|_| CacheError::Full)?;

        // Evict entries if necessary
        while self.get_memory_usage() + entry_size > self.config.max_memory_bytes {
            if let Some((_, evicted)) = cache.pop_lru() {
                debug!("Evicting entry from memory cache");
                self.update_memory_usage(|usage| usage.saturating_sub(evicted.size));
                self.record_eviction();
            } else {
                break;
            }
        }

        // Insert new entry
        if let Some(old_entry) = cache.put(key, entry) {
            self.update_memory_usage(|usage| usage.saturating_sub(old_entry.size));
        }

        self.update_memory_usage(|usage| usage + entry_size);

        Ok(())
    }

    /// Get from disk cache
    fn get_from_disk(&self, key: &CacheKey) -> Option<Bytes> {
        let base_dir = self.config.disk_cache_dir.as_ref()?;
        let path = key.to_path(base_dir);

        match std::fs::read(&path) {
            Ok(data) => {
                trace!("Disk cache hit: {}", path.display());
                self.record_disk_read();
                Some(Bytes::from(data))
            }
            Err(_) => None,
        }
    }

    /// Put on disk cache
    fn put_on_disk(&self, key: &CacheKey, data: &Bytes) -> CacheResult<()> {
        let base_dir =
            self.config
                .disk_cache_dir
                .as_ref()
                .ok_or(CacheError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "No disk cache directory",
                )))?;

        let path = key.to_path(base_dir);

        // Create parent directories
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Write tile to disk
        std::fs::write(&path, data)?;
        self.record_disk_write();

        trace!("Wrote to disk cache: {}", path.display());
        Ok(())
    }

    /// Clear all cached entries
    ///
    /// If an internal lock is poisoned (a prior panic occurred while it was
    /// held), the poisoned guard is recovered via `into_inner()` so the clear
    /// still takes effect, and the poisoning is surfaced via a warning rather
    /// than silently leaving stale state behind.
    pub fn clear(&self) -> CacheResult<()> {
        // Clear memory cache, recovering from a poisoned lock.
        match self.memory_cache.lock() {
            Ok(mut cache) => cache.clear(),
            Err(poison) => {
                warn!("memory_cache lock poisoned during clear(); recovering and clearing");
                poison.into_inner().clear();
            }
        }

        self.update_memory_usage(|_| 0);

        // Clear disk cache if enabled
        if let Some(ref dir) = self.config.disk_cache_dir
            && dir.exists()
        {
            std::fs::remove_dir_all(dir)?;
            std::fs::create_dir_all(dir)?;
        }

        // Reset stats, recovering from a poisoned lock.
        match self.stats.lock() {
            Ok(mut stats) => *stats = CacheStats::default(),
            Err(poison) => {
                warn!("stats lock poisoned during clear(); recovering and resetting");
                *poison.into_inner() = CacheStats::default();
            }
        }

        debug!("Cache cleared");
        Ok(())
    }

    /// Get cache statistics
    ///
    /// On lock poisoning, the last-known stats are recovered (they remain valid
    /// after a panic) and a warning is emitted, instead of silently returning a
    /// zeroed default that would hide the underlying fault from operators.
    pub fn stats(&self) -> CacheStats {
        match self.stats.lock() {
            Ok(stats) => stats.clone(),
            Err(poison) => {
                warn!("stats lock poisoned; returning last-known stats");
                poison.into_inner().clone()
            }
        }
    }

    /// Get current memory usage
    fn get_memory_usage(&self) -> usize {
        match self.memory_usage.lock() {
            Ok(usage) => *usage,
            Err(poison) => {
                warn!("memory_usage lock poisoned; returning last-known usage");
                *poison.into_inner()
            }
        }
    }

    /// Update memory usage
    fn update_memory_usage<F>(&self, f: F)
    where
        F: FnOnce(usize) -> usize,
    {
        if let Ok(mut usage) = self.memory_usage.lock() {
            *usage = f(*usage);
        }

        if let Ok(mut stats) = self.stats.lock() {
            stats.total_size = self.get_memory_usage();
        }
    }

    /// Record a cache hit
    fn record_hit(&self) {
        if self.config.enable_stats
            && let Ok(mut stats) = self.stats.lock()
        {
            stats.hits += 1;
        }
    }

    /// Record a cache miss
    fn record_miss(&self) {
        if self.config.enable_stats
            && let Ok(mut stats) = self.stats.lock()
        {
            stats.misses += 1;
        }
    }

    /// Record an eviction
    fn record_eviction(&self) {
        if self.config.enable_stats
            && let Ok(mut stats) = self.stats.lock()
        {
            stats.evictions += 1;
        }
    }

    /// Record an expiration
    fn record_expiration(&self) {
        if self.config.enable_stats
            && let Ok(mut stats) = self.stats.lock()
        {
            stats.expirations += 1;
        }
    }

    /// Record a disk read
    fn record_disk_read(&self) {
        if self.config.enable_stats
            && let Ok(mut stats) = self.stats.lock()
        {
            stats.disk_reads += 1;
        }
    }

    /// Record a disk write
    fn record_disk_write(&self) {
        if self.config.enable_stats
            && let Ok(mut stats) = self.stats.lock()
        {
            stats.disk_writes += 1;
        }
    }

    /// Record a cache-insertion failure (memory promotion or disk write)
    fn record_put_failure(&self) {
        if self.config.enable_stats
            && let Ok(mut stats) = self.stats.lock()
        {
            stats.put_failures += 1;
        }
    }
}

impl Clone for TileCache {
    fn clone(&self) -> Self {
        Self {
            memory_cache: Arc::clone(&self.memory_cache),
            memory_usage: Arc::clone(&self.memory_usage),
            config: self.config.clone(),
            stats: Arc::clone(&self.stats),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_key_to_string() {
        let key = CacheKey::new("landsat".to_string(), 10, 512, 384, "png".to_string());
        assert_eq!(key.to_string(), "landsat/10/512/384.png");

        let key_with_style = key.with_style("default".to_string());
        assert_eq!(key_with_style.to_string(), "landsat/default/10/512/384.png");
    }

    #[test]
    fn test_cache_put_get() {
        let config = TileCacheConfig::default();
        let cache = TileCache::new(config);

        let key = CacheKey::new("test".to_string(), 0, 0, 0, "png".to_string());
        let data = Bytes::from(vec![1, 2, 3, 4, 5]);

        cache.put(key.clone(), data.clone()).expect("put failed");

        let retrieved = cache.get(&key).expect("get failed");
        assert_eq!(retrieved, data);
    }

    #[test]
    fn test_cache_miss() {
        let config = TileCacheConfig::default();
        let cache = TileCache::new(config);

        let key = CacheKey::new("test".to_string(), 0, 0, 0, "png".to_string());
        assert!(cache.get(&key).is_none());

        let stats = cache.stats();
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.hits, 0);
    }

    #[test]
    fn test_cache_stats() {
        let config = TileCacheConfig::default();
        let cache = TileCache::new(config);

        let key1 = CacheKey::new("test".to_string(), 0, 0, 0, "png".to_string());
        let key2 = CacheKey::new("test".to_string(), 0, 0, 1, "png".to_string());
        let data = Bytes::from(vec![1, 2, 3]);

        cache.put(key1.clone(), data.clone()).expect("put failed");
        cache.put(key2.clone(), data.clone()).expect("put failed");

        cache.get(&key1);
        cache.get(&key2);
        cache.get(&CacheKey::new(
            "nonexistent".to_string(),
            0,
            0,
            0,
            "png".to_string(),
        ));

        let stats = cache.stats();
        assert_eq!(stats.hits, 2);
        assert_eq!(stats.misses, 1);
        assert!(stats.hit_rate() > 0.6);
    }

    #[test]
    fn test_cache_clear() {
        let config = TileCacheConfig::default();
        let cache = TileCache::new(config);

        let key = CacheKey::new("test".to_string(), 0, 0, 0, "png".to_string());
        let data = Bytes::from(vec![1, 2, 3]);

        cache.put(key.clone(), data).expect("put failed");
        assert!(cache.get(&key).is_some());

        cache.clear().expect("clear failed");
        assert!(cache.get(&key).is_none());
    }
}
