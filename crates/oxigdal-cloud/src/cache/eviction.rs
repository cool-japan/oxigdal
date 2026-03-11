//! LRU, LFU, and ARC eviction policy implementations

#[cfg(feature = "cache")]
use dashmap::DashMap;
#[cfg(feature = "cache")]
use lru::LruCache;
use std::collections::VecDeque;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

#[cfg(feature = "async")]
use tokio::sync::RwLock;

use bytes::Bytes;
use std::time::Duration;

use super::CacheConfig;
use super::metadata::{CacheEntry, CacheKey, CacheStats};
use crate::error::{CacheError, CloudError, Result};

/// LRU cache with TTL support
#[cfg(feature = "cache")]
pub struct LruTtlCache {
    /// LRU cache storage
    pub(crate) cache: Arc<RwLock<LruCache<CacheKey, CacheEntry>>>,
    /// Current size tracking
    pub(crate) current_size: Arc<AtomicUsize>,
    /// Configuration
    config: CacheConfig,
    /// Statistics
    stats: CacheStats,
}

#[cfg(feature = "cache")]
impl LruTtlCache {
    /// Creates a new LRU TTL cache
    pub fn new(config: CacheConfig) -> Result<Self> {
        let capacity = NonZeroUsize::new(config.max_entries.max(1)).ok_or_else(|| {
            CloudError::Cache(CacheError::Full {
                message: "Invalid cache capacity".to_string(),
            })
        })?;

        Ok(Self {
            cache: Arc::new(RwLock::new(LruCache::new(capacity))),
            current_size: Arc::new(AtomicUsize::new(0)),
            config,
            stats: CacheStats::default(),
        })
    }

    /// Gets an entry from the cache
    pub async fn get(&self, key: &CacheKey) -> Result<Bytes> {
        let mut cache = self.cache.write().await;

        if let Some(entry) = cache.get_mut(key) {
            // Check TTL expiration
            if entry.is_expired() {
                let size = entry.size;
                cache.pop(key);
                self.current_size.fetch_sub(size, Ordering::SeqCst);
                self.stats.evictions.fetch_add(1, Ordering::Relaxed);
                return Err(CloudError::Cache(CacheError::Miss { key: key.clone() }));
            }

            // Check max age
            if let Some(max_age) = self.config.max_age {
                if entry.age() > max_age {
                    let size = entry.size;
                    cache.pop(key);
                    self.current_size.fetch_sub(size, Ordering::SeqCst);
                    self.stats.evictions.fetch_add(1, Ordering::Relaxed);
                    return Err(CloudError::Cache(CacheError::Miss { key: key.clone() }));
                }
            }

            entry.record_access();
            self.stats.hits.fetch_add(1, Ordering::Relaxed);

            let data = if entry.compressed {
                self.decompress(&entry.data)?
            } else {
                entry.data.clone()
            };

            Ok(data)
        } else {
            self.stats.misses.fetch_add(1, Ordering::Relaxed);
            Err(CloudError::Cache(CacheError::Miss { key: key.clone() }))
        }
    }

    /// Puts an entry with optional TTL
    pub async fn put(&self, key: CacheKey, data: Bytes, ttl: Option<Duration>) -> Result<()> {
        let (final_data, is_compressed) =
            if self.config.compress && data.len() > self.config.compress_threshold {
                (self.compress(&data)?, true)
            } else {
                (data, false)
            };

        let entry = if let Some(ttl_duration) = ttl.or(self.config.default_ttl) {
            CacheEntry::with_ttl(final_data.clone(), is_compressed, ttl_duration)
        } else {
            CacheEntry::new(final_data.clone(), is_compressed)
        };

        let entry_size = entry.size;
        let mut cache = self.cache.write().await;

        // Evict expired entries first
        self.evict_expired(&mut cache).await;

        // Evict entries if necessary to make room
        while self.current_size.load(Ordering::SeqCst) + entry_size > self.config.max_memory_size
            && !cache.is_empty()
        {
            if let Some((_, evicted_entry)) = cache.pop_lru() {
                self.current_size
                    .fetch_sub(evicted_entry.size, Ordering::SeqCst);
                self.stats.evictions.fetch_add(1, Ordering::Relaxed);
            }
        }

        if let Some(old_entry) = cache.put(key, entry) {
            self.current_size
                .fetch_sub(old_entry.size, Ordering::SeqCst);
        }

        self.current_size.fetch_add(entry_size, Ordering::SeqCst);
        self.stats.writes.fetch_add(1, Ordering::Relaxed);

        Ok(())
    }

    /// Evicts expired entries
    async fn evict_expired(&self, cache: &mut LruCache<CacheKey, CacheEntry>) {
        let mut keys_to_remove = Vec::new();

        for (key, entry) in cache.iter() {
            if entry.is_expired() {
                keys_to_remove.push(key.clone());
            }
        }

        for key in keys_to_remove {
            if let Some(entry) = cache.pop(&key) {
                self.current_size.fetch_sub(entry.size, Ordering::SeqCst);
                self.stats.evictions.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Removes an entry
    pub async fn remove(&self, key: &CacheKey) -> Result<()> {
        let mut cache = self.cache.write().await;
        if let Some(entry) = cache.pop(key) {
            self.current_size.fetch_sub(entry.size, Ordering::SeqCst);
        }
        Ok(())
    }

    /// Clears the cache
    pub async fn clear(&self) -> Result<()> {
        let mut cache = self.cache.write().await;
        cache.clear();
        self.current_size.store(0, Ordering::SeqCst);
        Ok(())
    }

    /// Returns cache statistics
    #[must_use]
    pub fn stats(&self) -> &CacheStats {
        &self.stats
    }

    /// Compresses data using gzip
    fn compress(&self, data: &Bytes) -> Result<Bytes> {
        oxiarc_archive::gzip::compress(data, 6)
            .map(Bytes::from)
            .map_err(|e| {
                CloudError::Cache(CacheError::Compression {
                    message: format!("Compression failed: {e}"),
                })
            })
    }

    /// Decompresses data
    fn decompress(&self, data: &Bytes) -> Result<Bytes> {
        let mut reader = std::io::Cursor::new(data.as_ref());
        oxiarc_archive::gzip::decompress(&mut reader)
            .map(Bytes::from)
            .map_err(|e| {
                CloudError::Cache(CacheError::Decompression {
                    message: format!("Decompression failed: {e}"),
                })
            })
    }
}

/// LFU (Least Frequently Used) cache
#[cfg(feature = "cache")]
pub struct LfuCache {
    /// Storage map
    storage: Arc<DashMap<CacheKey, CacheEntry>>,
    /// Frequency tracking
    frequency_map: Arc<DashMap<CacheKey, u64>>,
    /// Minimum frequency tracker
    min_frequency: Arc<AtomicU64>,
    /// Current size
    current_size: Arc<AtomicUsize>,
    /// Configuration
    config: CacheConfig,
    /// Statistics
    stats: CacheStats,
}

#[cfg(feature = "cache")]
impl LfuCache {
    /// Creates a new LFU cache
    pub fn new(config: CacheConfig) -> Self {
        Self {
            storage: Arc::new(DashMap::new()),
            frequency_map: Arc::new(DashMap::new()),
            min_frequency: Arc::new(AtomicU64::new(0)),
            current_size: Arc::new(AtomicUsize::new(0)),
            config,
            stats: CacheStats::default(),
        }
    }

    /// Gets an entry
    pub async fn get(&self, key: &CacheKey) -> Result<Bytes> {
        if let Some(mut entry) = self.storage.get_mut(key) {
            // Check expiration
            if entry.is_expired() {
                drop(entry);
                self.remove(key).await?;
                return Err(CloudError::Cache(CacheError::Miss { key: key.clone() }));
            }

            entry.record_access();
            self.frequency_map
                .entry(key.clone())
                .and_modify(|f| *f += 1)
                .or_insert(1);

            self.stats.hits.fetch_add(1, Ordering::Relaxed);
            Ok(entry.data.clone())
        } else {
            self.stats.misses.fetch_add(1, Ordering::Relaxed);
            Err(CloudError::Cache(CacheError::Miss { key: key.clone() }))
        }
    }

    /// Puts an entry
    pub async fn put(&self, key: CacheKey, data: Bytes, ttl: Option<Duration>) -> Result<()> {
        let entry = if let Some(ttl_duration) = ttl.or(self.config.default_ttl) {
            CacheEntry::with_ttl(data, false, ttl_duration)
        } else {
            CacheEntry::new(data, false)
        };

        let entry_size = entry.size;

        // Evict entries if necessary
        while self.current_size.load(Ordering::SeqCst) + entry_size > self.config.max_memory_size
            && !self.storage.is_empty()
        {
            self.evict_lfu().await;
        }

        if let Some(old_entry) = self.storage.insert(key.clone(), entry) {
            self.current_size
                .fetch_sub(old_entry.size, Ordering::SeqCst);
        }

        self.current_size.fetch_add(entry_size, Ordering::SeqCst);
        self.frequency_map.insert(key, 1);
        self.min_frequency.store(1, Ordering::SeqCst);
        self.stats.writes.fetch_add(1, Ordering::Relaxed);

        Ok(())
    }

    /// Evicts the least frequently used entry
    async fn evict_lfu(&self) {
        // Find the entry with minimum frequency
        let mut min_freq = u64::MAX;
        let mut min_key: Option<String> = None;

        for entry in self.frequency_map.iter() {
            if *entry.value() < min_freq {
                min_freq = *entry.value();
                min_key = Some(entry.key().clone());
            }
        }

        if let Some(key) = min_key {
            if let Some((_, entry)) = self.storage.remove(&key) {
                self.current_size.fetch_sub(entry.size, Ordering::SeqCst);
                self.frequency_map.remove(&key);
                self.stats.evictions.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Removes an entry
    pub async fn remove(&self, key: &CacheKey) -> Result<()> {
        if let Some((_, entry)) = self.storage.remove(key) {
            self.current_size.fetch_sub(entry.size, Ordering::SeqCst);
            self.frequency_map.remove(key);
        }
        Ok(())
    }

    /// Clears the cache
    pub async fn clear(&self) -> Result<()> {
        self.storage.clear();
        self.frequency_map.clear();
        self.current_size.store(0, Ordering::SeqCst);
        self.min_frequency.store(0, Ordering::SeqCst);
        Ok(())
    }

    /// Returns cache statistics
    #[must_use]
    pub fn stats(&self) -> &CacheStats {
        &self.stats
    }
}

/// ARC (Adaptive Replacement Cache)
///
/// Combines LRU and LFU strategies adaptively based on access patterns.
#[cfg(feature = "cache")]
pub struct ArcCache {
    /// T1: Recent entries (LRU)
    t1: Arc<RwLock<VecDeque<CacheKey>>>,
    /// T2: Frequent entries (LFU)
    t2: Arc<RwLock<VecDeque<CacheKey>>>,
    /// B1: Ghost entries from T1
    b1: Arc<RwLock<VecDeque<CacheKey>>>,
    /// B2: Ghost entries from T2
    b2: Arc<RwLock<VecDeque<CacheKey>>>,
    /// Data storage
    storage: Arc<DashMap<CacheKey, CacheEntry>>,
    /// Adaptation parameter p
    p: Arc<RwLock<f64>>,
    /// Target cache size
    c: usize,
    /// Current size
    current_size: Arc<AtomicUsize>,
    /// Configuration
    config: CacheConfig,
    /// Statistics
    stats: CacheStats,
}

#[cfg(feature = "cache")]
impl ArcCache {
    /// Creates a new ARC cache
    pub fn new(config: CacheConfig) -> Self {
        let c = config.max_entries;
        Self {
            t1: Arc::new(RwLock::new(VecDeque::new())),
            t2: Arc::new(RwLock::new(VecDeque::new())),
            b1: Arc::new(RwLock::new(VecDeque::new())),
            b2: Arc::new(RwLock::new(VecDeque::new())),
            storage: Arc::new(DashMap::new()),
            p: Arc::new(RwLock::new(0.0)),
            c,
            current_size: Arc::new(AtomicUsize::new(0)),
            config,
            stats: CacheStats::default(),
        }
    }

    /// Gets an entry using ARC algorithm
    pub async fn get(&self, key: &CacheKey) -> Result<Bytes> {
        // Check if in T1 or T2
        if let Some(mut entry) = self.storage.get_mut(key) {
            if entry.is_expired() {
                drop(entry);
                self.remove(key).await?;
                return Err(CloudError::Cache(CacheError::Miss { key: key.clone() }));
            }

            entry.record_access();

            // Move from T1 to T2 (if in T1)
            let mut t1 = self.t1.write().await;
            if let Some(pos) = t1.iter().position(|k| k == key) {
                t1.remove(pos);
                let mut t2 = self.t2.write().await;
                t2.push_back(key.clone());
            }

            self.stats.hits.fetch_add(1, Ordering::Relaxed);
            Ok(entry.data.clone())
        } else {
            self.stats.misses.fetch_add(1, Ordering::Relaxed);
            Err(CloudError::Cache(CacheError::Miss { key: key.clone() }))
        }
    }

    /// Puts an entry using ARC algorithm
    pub async fn put(&self, key: CacheKey, data: Bytes, ttl: Option<Duration>) -> Result<()> {
        let entry = if let Some(ttl_duration) = ttl.or(self.config.default_ttl) {
            CacheEntry::with_ttl(data, false, ttl_duration)
        } else {
            CacheEntry::new(data, false)
        };

        let entry_size = entry.size;

        // Check ghost lists and adapt
        let in_b1 = {
            let b1 = self.b1.read().await;
            b1.contains(&key)
        };
        let in_b2 = {
            let b2 = self.b2.read().await;
            b2.contains(&key)
        };

        if in_b1 {
            // Case II: key in B1 (recently evicted from T1)
            // Adapt p: increase preference for T1
            let b1_len = self.b1.read().await.len();
            let b2_len = self.b2.read().await.len();
            let delta = if b1_len >= b2_len {
                1.0
            } else {
                b2_len as f64 / b1_len as f64
            };
            let mut p = self.p.write().await;
            *p = (*p + delta).min(self.c as f64);

            // Remove from B1
            let mut b1 = self.b1.write().await;
            if let Some(pos) = b1.iter().position(|k| k == &key) {
                b1.remove(pos);
            }
        } else if in_b2 {
            // Case III: key in B2 (recently evicted from T2)
            // Adapt p: decrease preference for T1
            let b1_len = self.b1.read().await.len();
            let b2_len = self.b2.read().await.len();
            let delta = if b2_len >= b1_len {
                1.0
            } else {
                b1_len as f64 / b2_len as f64
            };
            let mut p = self.p.write().await;
            *p = (*p - delta).max(0.0);

            // Remove from B2
            let mut b2 = self.b2.write().await;
            if let Some(pos) = b2.iter().position(|k| k == &key) {
                b2.remove(pos);
            }
        }

        // Ensure we have space
        while self.current_size.load(Ordering::SeqCst) + entry_size > self.config.max_memory_size {
            self.replace(&key).await;
        }

        // Add to T1 (new entries always go to T1)
        let mut t1 = self.t1.write().await;
        t1.push_back(key.clone());

        if let Some(old) = self.storage.insert(key, entry) {
            self.current_size.fetch_sub(old.size, Ordering::SeqCst);
        }
        self.current_size.fetch_add(entry_size, Ordering::SeqCst);
        self.stats.writes.fetch_add(1, Ordering::Relaxed);

        Ok(())
    }

    /// ARC replacement policy
    async fn replace(&self, _key: &CacheKey) {
        let t1_len = self.t1.read().await.len();
        let p = *self.p.read().await;

        if !self.storage.is_empty() {
            if t1_len > 0 && (t1_len as f64 > p || self.t2.read().await.is_empty()) {
                // Evict from T1
                let mut t1 = self.t1.write().await;
                if let Some(evict_key) = t1.pop_front() {
                    if let Some((_, entry)) = self.storage.remove(&evict_key) {
                        self.current_size.fetch_sub(entry.size, Ordering::SeqCst);
                    }
                    // Add to B1 ghost list
                    let mut b1 = self.b1.write().await;
                    b1.push_back(evict_key);
                    // Limit ghost list size
                    while b1.len() > self.c {
                        b1.pop_front();
                    }
                    self.stats.evictions.fetch_add(1, Ordering::Relaxed);
                }
            } else {
                // Evict from T2
                let mut t2 = self.t2.write().await;
                if let Some(evict_key) = t2.pop_front() {
                    if let Some((_, entry)) = self.storage.remove(&evict_key) {
                        self.current_size.fetch_sub(entry.size, Ordering::SeqCst);
                    }
                    // Add to B2 ghost list
                    let mut b2 = self.b2.write().await;
                    b2.push_back(evict_key);
                    // Limit ghost list size
                    while b2.len() > self.c {
                        b2.pop_front();
                    }
                    self.stats.evictions.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }

    /// Removes an entry
    pub async fn remove(&self, key: &CacheKey) -> Result<()> {
        if let Some((_, entry)) = self.storage.remove(key) {
            self.current_size.fetch_sub(entry.size, Ordering::SeqCst);
        }

        // Remove from T1 or T2
        {
            let mut t1 = self.t1.write().await;
            if let Some(pos) = t1.iter().position(|k| k == key) {
                t1.remove(pos);
            }
        }
        {
            let mut t2 = self.t2.write().await;
            if let Some(pos) = t2.iter().position(|k| k == key) {
                t2.remove(pos);
            }
        }

        Ok(())
    }

    /// Clears the cache
    pub async fn clear(&self) -> Result<()> {
        self.storage.clear();
        self.t1.write().await.clear();
        self.t2.write().await.clear();
        self.b1.write().await.clear();
        self.b2.write().await.clear();
        self.current_size.store(0, Ordering::SeqCst);
        *self.p.write().await = 0.0;
        Ok(())
    }

    /// Returns cache statistics
    #[must_use]
    pub fn stats(&self) -> &CacheStats {
        &self.stats
    }
}
