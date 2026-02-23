//! Mobile caching strategies optimized for limited storage

use crate::error::{MobileError, Result};
use lru::LruCache;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Cache eviction policy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CachePolicy {
    /// Least Recently Used (LRU)
    LRU,
    /// Least Frequently Used (LFU)
    LFU,
    /// Time-based expiration
    TTL,
    /// Size-based with priority
    SizePriority,
}

/// Cache entry priority
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CachePriority {
    /// Low priority, evict first
    Low,
    /// Normal priority
    Normal,
    /// High priority, keep as long as possible
    High,
}

/// Cache entry metadata
#[derive(Debug, Clone)]
struct CacheEntry<T> {
    value: T,
    priority: CachePriority,
    size_bytes: usize,
    access_count: u64,
    last_accessed: Instant,
    expires_at: Option<Instant>,
}

impl<T> CacheEntry<T> {
    fn new(value: T, priority: CachePriority, size_bytes: usize, ttl: Option<Duration>) -> Self {
        Self {
            value,
            priority,
            size_bytes,
            access_count: 0,
            last_accessed: Instant::now(),
            expires_at: ttl.map(|d| Instant::now() + d),
        }
    }

    fn access(&mut self) {
        self.access_count = self.access_count.saturating_add(1);
        self.last_accessed = Instant::now();
    }

    fn is_expired(&self) -> bool {
        self.expires_at.is_some_and(|exp| Instant::now() >= exp)
    }
}

/// Mobile cache with size limits and eviction policies
pub struct MobileCache<K, V>
where
    K: std::hash::Hash + Eq + Clone,
    V: Clone,
{
    entries: Arc<RwLock<HashMap<K, CacheEntry<V>>>>,
    lru: Arc<RwLock<LruCache<K, ()>>>,
    policy: CachePolicy,
    max_size_bytes: usize,
    current_size_bytes: Arc<RwLock<usize>>,
    default_ttl: Option<Duration>,
}

impl<K, V> MobileCache<K, V>
where
    K: std::hash::Hash + Eq + Clone,
    V: Clone,
{
    /// Create a new mobile cache with specified policy and size limit
    pub fn new(policy: CachePolicy, max_size_bytes: usize) -> Self {
        // Safety: 1000 is non-zero
        let capacity = NonZeroUsize::new(1000).unwrap_or(NonZeroUsize::MIN);

        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
            lru: Arc::new(RwLock::new(LruCache::new(capacity))),
            policy,
            max_size_bytes,
            current_size_bytes: Arc::new(RwLock::new(0)),
            default_ttl: None,
        }
    }

    /// Create cache with TTL policy
    pub fn with_ttl(max_size_bytes: usize, ttl: Duration) -> Self {
        let mut cache = Self::new(CachePolicy::TTL, max_size_bytes);
        cache.default_ttl = Some(ttl);
        cache
    }

    /// Insert value into cache
    pub fn insert(
        &self,
        key: K,
        value: V,
        size_bytes: usize,
        priority: CachePriority,
    ) -> Result<()> {
        // Check if we need to evict entries
        while *self.current_size_bytes.read() + size_bytes > self.max_size_bytes {
            self.evict_one()?;
        }

        let entry = CacheEntry::new(value, priority, size_bytes, self.default_ttl);

        let mut entries = self.entries.write();
        if let Some(old_entry) = entries.insert(key.clone(), entry) {
            // Update size
            let mut current_size = self.current_size_bytes.write();
            *current_size = current_size.saturating_sub(old_entry.size_bytes);
        }

        // Update LRU
        self.lru.write().put(key.clone(), ());

        // Update size
        let mut current_size = self.current_size_bytes.write();
        *current_size = current_size.saturating_add(size_bytes);

        Ok(())
    }

    /// Get value from cache
    pub fn get(&self, key: &K) -> Option<V> {
        self.cleanup_expired();

        let mut entries = self.entries.write();
        if let Some(entry) = entries.get_mut(key) {
            if entry.is_expired() {
                return None;
            }

            entry.access();
            self.lru.write().get(key);
            Some(entry.value.clone())
        } else {
            None
        }
    }

    /// Check if key exists in cache
    pub fn contains_key(&self, key: &K) -> bool {
        self.cleanup_expired();

        let entries = self.entries.read();
        entries.get(key).is_some_and(|e| !e.is_expired())
    }

    /// Remove entry from cache
    pub fn remove(&self, key: &K) -> Option<V> {
        let mut entries = self.entries.write();
        if let Some(entry) = entries.remove(key) {
            // Update size
            let mut current_size = self.current_size_bytes.write();
            *current_size = current_size.saturating_sub(entry.size_bytes);

            // Remove from LRU
            self.lru.write().pop(key);

            Some(entry.value)
        } else {
            None
        }
    }

    /// Clear all entries
    pub fn clear(&self) {
        self.entries.write().clear();
        self.lru.write().clear();
        *self.current_size_bytes.write() = 0;
    }

    /// Evict one entry based on policy
    fn evict_one(&self) -> Result<()> {
        let key_to_evict = match self.policy {
            CachePolicy::LRU => self.find_lru_key(),
            CachePolicy::LFU => self.find_lfu_key(),
            CachePolicy::TTL => self.find_oldest_key(),
            CachePolicy::SizePriority => self.find_lowest_priority_key(),
        };

        if let Some(key) = key_to_evict {
            self.remove(&key);
            Ok(())
        } else {
            Err(MobileError::CacheError("No entries to evict".to_string()))
        }
    }

    /// Find LRU key
    fn find_lru_key(&self) -> Option<K> {
        self.lru.write().peek_lru().map(|(k, _)| k.clone())
    }

    /// Find LFU key (least frequently used)
    fn find_lfu_key(&self) -> Option<K> {
        let entries = self.entries.read();
        entries
            .iter()
            .filter(|(_, e)| !e.is_expired())
            .min_by_key(|(_, e)| e.access_count)
            .map(|(k, _)| k.clone())
    }

    /// Find oldest key
    fn find_oldest_key(&self) -> Option<K> {
        let entries = self.entries.read();
        entries
            .iter()
            .filter(|(_, e)| !e.is_expired())
            .min_by_key(|(_, e)| e.last_accessed)
            .map(|(k, _)| k.clone())
    }

    /// Find lowest priority key
    fn find_lowest_priority_key(&self) -> Option<K> {
        let entries = self.entries.read();
        entries
            .iter()
            .filter(|(_, e)| !e.is_expired())
            .min_by_key(|(_, e)| (e.priority, e.last_accessed))
            .map(|(k, _)| k.clone())
    }

    /// Clean up expired entries
    fn cleanup_expired(&self) {
        let mut entries = self.entries.write();
        let expired_keys: Vec<K> = entries
            .iter()
            .filter(|(_, e)| e.is_expired())
            .map(|(k, _)| k.clone())
            .collect();

        for key in expired_keys {
            if let Some(entry) = entries.remove(&key) {
                let mut current_size = self.current_size_bytes.write();
                *current_size = current_size.saturating_sub(entry.size_bytes);
                self.lru.write().pop(&key);
            }
        }
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        self.cleanup_expired();

        let entries = self.entries.read();
        let current_size = *self.current_size_bytes.read();

        CacheStats {
            entry_count: entries.len(),
            total_size_bytes: current_size,
            max_size_bytes: self.max_size_bytes,
            hit_rate: 0.0, // Would need to track hits/misses
        }
    }

    /// Get number of entries
    pub fn len(&self) -> usize {
        self.cleanup_expired();
        self.entries.read().len()
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    /// Number of entries in cache
    pub entry_count: usize,
    /// Total size of cached data in bytes
    pub total_size_bytes: usize,
    /// Maximum cache size in bytes
    pub max_size_bytes: usize,
    /// Cache hit rate (0.0 - 1.0)
    pub hit_rate: f64,
}

impl CacheStats {
    /// Get cache usage percentage (0.0 - 100.0)
    pub fn usage_percentage(&self) -> f64 {
        if self.max_size_bytes == 0 {
            return 0.0;
        }
        (self.total_size_bytes as f64 / self.max_size_bytes as f64) * 100.0
    }

    /// Check if cache is nearly full (> 90%)
    pub fn is_nearly_full(&self) -> bool {
        self.usage_percentage() > 90.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_insert_and_get() {
        let cache = MobileCache::new(CachePolicy::LRU, 1024);

        cache
            .insert(
                "key1".to_string(),
                "value1".to_string(),
                100,
                CachePriority::Normal,
            )
            .expect("Insert failed");

        assert_eq!(cache.get(&"key1".to_string()), Some("value1".to_string()));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_cache_eviction() {
        let cache = MobileCache::new(CachePolicy::LRU, 250);

        cache
            .insert(
                "key1".to_string(),
                "value1".to_string(),
                100,
                CachePriority::Normal,
            )
            .expect("Insert failed");
        cache
            .insert(
                "key2".to_string(),
                "value2".to_string(),
                100,
                CachePriority::Normal,
            )
            .expect("Insert failed");

        // This should trigger eviction
        cache
            .insert(
                "key3".to_string(),
                "value3".to_string(),
                100,
                CachePriority::Normal,
            )
            .expect("Insert failed");

        // key1 should be evicted (LRU)
        assert!(cache.len() <= 2);
    }

    #[test]
    fn test_cache_priority() {
        let cache = MobileCache::new(CachePolicy::SizePriority, 250);

        cache
            .insert(
                "low".to_string(),
                "value".to_string(),
                100,
                CachePriority::Low,
            )
            .expect("Insert failed");
        cache
            .insert(
                "high".to_string(),
                "value".to_string(),
                100,
                CachePriority::High,
            )
            .expect("Insert failed");

        // Trigger eviction
        cache
            .insert(
                "new".to_string(),
                "value".to_string(),
                100,
                CachePriority::Normal,
            )
            .expect("Insert failed");

        // Low priority should be evicted first
        assert!(cache.contains_key(&"high".to_string()));
    }

    #[test]
    fn test_cache_ttl() {
        let cache = MobileCache::with_ttl(1024, Duration::from_millis(100));

        cache
            .insert(
                "key".to_string(),
                "value".to_string(),
                100,
                CachePriority::Normal,
            )
            .expect("Insert failed");

        assert!(cache.contains_key(&"key".to_string()));

        // Wait for expiration
        std::thread::sleep(Duration::from_millis(150));

        assert!(!cache.contains_key(&"key".to_string()));
    }

    #[test]
    fn test_cache_stats() {
        let cache = MobileCache::new(CachePolicy::LRU, 1024);

        cache
            .insert(
                "key1".to_string(),
                "value".to_string(),
                100,
                CachePriority::Normal,
            )
            .expect("Insert failed");
        cache
            .insert(
                "key2".to_string(),
                "value".to_string(),
                200,
                CachePriority::Normal,
            )
            .expect("Insert failed");

        let stats = cache.stats();
        assert_eq!(stats.entry_count, 2);
        assert_eq!(stats.total_size_bytes, 300);
        assert_eq!(stats.max_size_bytes, 1024);
        assert!(!stats.is_nearly_full());
    }

    #[test]
    fn test_cache_clear() {
        let cache = MobileCache::new(CachePolicy::LRU, 1024);

        cache
            .insert(
                "key1".to_string(),
                "value1".to_string(),
                100,
                CachePriority::Normal,
            )
            .expect("Insert failed");
        cache
            .insert(
                "key2".to_string(),
                "value2".to_string(),
                100,
                CachePriority::Normal,
            )
            .expect("Insert failed");

        assert_eq!(cache.len(), 2);

        cache.clear();
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }
}
