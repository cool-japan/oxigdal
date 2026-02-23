//! Local-first caching for edge devices
//!
//! Provides efficient caching mechanisms optimized for resource-constrained
//! edge devices with offline-first architecture.

use crate::error::{EdgeError, Result};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use lru::LruCache;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Arc;

/// Cache eviction policy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CachePolicy {
    /// Least Recently Used
    Lru,
    /// Least Frequently Used
    Lfu,
    /// Time To Live
    Ttl,
    /// Size-based eviction
    SizeBased,
}

/// Cache configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// Maximum cache size in bytes
    pub max_size: usize,
    /// Cache eviction policy
    pub policy: CachePolicy,
    /// Time to live in seconds
    pub ttl_secs: Option<u64>,
    /// Enable persistent cache
    pub persistent: bool,
    /// Cache directory for persistent storage
    pub cache_dir: Option<PathBuf>,
    /// Maximum number of entries
    pub max_entries: usize,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_size: crate::DEFAULT_CACHE_SIZE,
            policy: CachePolicy::Lru,
            ttl_secs: Some(3600), // 1 hour
            persistent: false,
            cache_dir: None,
            max_entries: 1000,
        }
    }
}

impl CacheConfig {
    /// Create minimal cache config for embedded devices
    pub fn minimal() -> Self {
        Self {
            max_size: 1024 * 1024, // 1 MB
            policy: CachePolicy::Lru,
            ttl_secs: Some(1800), // 30 minutes
            persistent: false,
            cache_dir: None,
            max_entries: 100,
        }
    }

    /// Create config for offline-first mode
    pub fn offline_first() -> Self {
        Self {
            max_size: 50 * 1024 * 1024, // 50 MB
            policy: CachePolicy::Lru,
            ttl_secs: None, // No expiration
            persistent: true,
            cache_dir: Some(PathBuf::from(".oxigdal_cache")),
            max_entries: 5000,
        }
    }
}

/// Cache entry with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    /// Entry key
    pub key: String,
    /// Cached data
    pub data: Bytes,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last access timestamp
    pub accessed_at: DateTime<Utc>,
    /// Access count
    pub access_count: u64,
    /// Entry size in bytes
    pub size: usize,
    /// Optional expiration time
    pub expires_at: Option<DateTime<Utc>>,
}

impl CacheEntry {
    /// Create new cache entry
    pub fn new(key: String, data: Bytes) -> Self {
        let now = Utc::now();
        let size = data.len();
        Self {
            key,
            data,
            created_at: now,
            accessed_at: now,
            access_count: 0,
            size,
            expires_at: None,
        }
    }

    /// Create entry with TTL
    pub fn with_ttl(key: String, data: Bytes, ttl_secs: u64) -> Self {
        let mut entry = Self::new(key, data);
        entry.expires_at = Some(Utc::now() + chrono::Duration::seconds(ttl_secs as i64));
        entry
    }

    /// Check if entry is expired
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            Utc::now() > expires_at
        } else {
            false
        }
    }

    /// Mark entry as accessed
    pub fn mark_accessed(&mut self) {
        self.accessed_at = Utc::now();
        self.access_count = self.access_count.saturating_add(1);
    }
}

/// Edge cache implementation
pub struct Cache {
    config: CacheConfig,
    lru_cache: Arc<RwLock<LruCache<String, CacheEntry>>>,
    metadata: Arc<RwLock<HashMap<String, CacheMetadata>>>,
    current_size: Arc<RwLock<usize>>,
    persistent_storage: Option<sled::Db>,
}

/// Cache metadata for tracking entry statistics
#[derive(Debug, Clone)]
struct CacheMetadata {
    /// Size of the cached entry in bytes
    size: usize,
    /// Number of times this entry has been accessed
    access_count: u64,
}

impl Cache {
    /// Create new cache with configuration
    pub fn new(config: CacheConfig) -> Result<Self> {
        let max_entries = NonZeroUsize::new(config.max_entries)
            .ok_or_else(|| EdgeError::invalid_config("max_entries must be greater than 0"))?;

        let lru_cache = Arc::new(RwLock::new(LruCache::new(max_entries)));
        let metadata = Arc::new(RwLock::new(HashMap::new()));
        let current_size = Arc::new(RwLock::new(0));

        let persistent_storage = if config.persistent {
            if let Some(cache_dir) = &config.cache_dir {
                let db = sled::open(cache_dir).map_err(|e| EdgeError::storage(e.to_string()))?;
                Some(db)
            } else {
                None
            }
        } else {
            None
        };

        Ok(Self {
            config,
            lru_cache,
            metadata,
            current_size,
            persistent_storage,
        })
    }

    /// Get entry from cache
    pub fn get(&self, key: &str) -> Result<Option<Bytes>> {
        // Try memory cache first
        let mut cache = self.lru_cache.write();
        if let Some(entry) = cache.get_mut(key) {
            if !entry.is_expired() {
                entry.mark_accessed();
                return Ok(Some(entry.data.clone()));
            } else {
                // Remove expired entry
                cache.pop(key);
                let mut meta = self.metadata.write();
                meta.remove(key);
            }
        }
        drop(cache);

        // Try persistent storage if enabled
        if let Some(db) = &self.persistent_storage {
            if let Some(value) = db.get(key).map_err(|e| EdgeError::storage(e.to_string()))? {
                let entry: CacheEntry = serde_json::from_slice(&value)
                    .map_err(|e| EdgeError::deserialization(e.to_string()))?;

                if !entry.is_expired() {
                    // Restore to memory cache
                    let mut cache = self.lru_cache.write();
                    cache.put(key.to_string(), entry.clone());
                    return Ok(Some(entry.data));
                }
            }
        }

        Ok(None)
    }

    /// Put entry into cache
    pub fn put(&self, key: String, data: Bytes) -> Result<()> {
        let entry_size = data.len();

        // Check size constraint
        if entry_size > self.config.max_size {
            return Err(EdgeError::cache(format!(
                "Entry size {} exceeds max cache size {}",
                entry_size, self.config.max_size
            )));
        }

        // Create cache entry
        let entry = if let Some(ttl) = self.config.ttl_secs {
            CacheEntry::with_ttl(key.clone(), data, ttl)
        } else {
            CacheEntry::new(key.clone(), data)
        };

        // Evict entries if necessary
        self.evict_if_needed(entry_size)?;

        // Insert into memory cache
        let mut cache = self.lru_cache.write();
        cache.put(key.clone(), entry.clone());
        drop(cache);

        // Update metadata
        let mut meta = self.metadata.write();
        meta.insert(
            key.clone(),
            CacheMetadata {
                size: entry_size,
                access_count: 0,
            },
        );
        drop(meta);

        // Update size
        let mut current_size = self.current_size.write();
        *current_size = current_size.saturating_add(entry_size);
        drop(current_size);

        // Persist if enabled
        if let Some(db) = &self.persistent_storage {
            let serialized =
                serde_json::to_vec(&entry).map_err(|e| EdgeError::serialization(e.to_string()))?;
            db.insert(key.as_bytes(), serialized)
                .map_err(|e| EdgeError::storage(e.to_string()))?;
        }

        Ok(())
    }

    /// Remove entry from cache
    pub fn remove(&self, key: &str) -> Result<Option<Bytes>> {
        let mut cache = self.lru_cache.write();
        let entry = cache.pop(key);
        drop(cache);

        if let Some(ref e) = entry {
            // Update metadata
            let mut meta = self.metadata.write();
            meta.remove(key);
            drop(meta);

            // Update size
            let mut current_size = self.current_size.write();
            *current_size = current_size.saturating_sub(e.size);
            drop(current_size);

            // Remove from persistent storage
            if let Some(db) = &self.persistent_storage {
                db.remove(key.as_bytes())
                    .map_err(|e| EdgeError::storage(e.to_string()))?;
            }
        }

        Ok(entry.map(|e| e.data))
    }

    /// Clear all cache entries
    pub fn clear(&self) -> Result<()> {
        let mut cache = self.lru_cache.write();
        cache.clear();
        drop(cache);

        let mut meta = self.metadata.write();
        meta.clear();
        drop(meta);

        let mut current_size = self.current_size.write();
        *current_size = 0;
        drop(current_size);

        if let Some(db) = &self.persistent_storage {
            db.clear().map_err(|e| EdgeError::storage(e.to_string()))?;
        }

        Ok(())
    }

    /// Get current cache size
    pub fn size(&self) -> usize {
        *self.current_size.read()
    }

    /// Get number of cached entries
    pub fn len(&self) -> usize {
        self.lru_cache.read().len()
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Evict entries based on policy
    fn evict_if_needed(&self, new_entry_size: usize) -> Result<()> {
        let current_size = *self.current_size.read();
        let target_size = self.config.max_size.saturating_sub(new_entry_size);

        if current_size <= target_size {
            return Ok(());
        }

        let mut to_evict = Vec::new();
        let mut freed_size = 0;

        match self.config.policy {
            CachePolicy::Lru => {
                // LRU eviction is handled by LruCache automatically
                let mut cache = self.lru_cache.write();
                while freed_size < current_size.saturating_sub(target_size) && !cache.is_empty() {
                    if let Some((key, entry)) = cache.pop_lru() {
                        freed_size = freed_size.saturating_add(entry.size);
                        to_evict.push(key);
                    }
                }
            }
            CachePolicy::Lfu => {
                // Evict least frequently used
                let meta = self.metadata.read();
                let mut entries: Vec<_> = meta.iter().collect();
                entries.sort_by_key(|(_, m)| m.access_count);

                for (key, metadata) in entries {
                    if freed_size >= current_size.saturating_sub(target_size) {
                        break;
                    }
                    freed_size = freed_size.saturating_add(metadata.size);
                    to_evict.push(key.clone());
                }
            }
            CachePolicy::Ttl => {
                // Evict expired entries first
                let cache = self.lru_cache.read();
                for (key, entry) in cache.iter() {
                    if entry.is_expired() {
                        freed_size = freed_size.saturating_add(entry.size);
                        to_evict.push(key.clone());
                    }
                }
            }
            CachePolicy::SizeBased => {
                // Evict largest entries first
                let meta = self.metadata.read();
                let mut entries: Vec<_> = meta.iter().collect();
                entries.sort_by_key(|(_, m)| std::cmp::Reverse(m.size));

                for (key, metadata) in entries {
                    if freed_size >= current_size.saturating_sub(target_size) {
                        break;
                    }
                    freed_size = freed_size.saturating_add(metadata.size);
                    to_evict.push(key.clone());
                }
            }
        }

        // Remove evicted entries
        for key in to_evict {
            self.remove(&key)?;
        }

        Ok(())
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            entries: self.len(),
            size_bytes: self.size(),
            max_size_bytes: self.config.max_size,
            max_entries: self.config.max_entries,
            policy: self.config.policy,
        }
    }
}

/// Cache statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStats {
    /// Number of entries
    pub entries: usize,
    /// Current size in bytes
    pub size_bytes: usize,
    /// Maximum size in bytes
    pub max_size_bytes: usize,
    /// Maximum number of entries
    pub max_entries: usize,
    /// Cache policy
    pub policy: CachePolicy,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_creation() {
        let config = CacheConfig::default();
        let cache = Cache::new(config);
        assert!(cache.is_ok());
    }

    #[test]
    fn test_cache_put_get() -> Result<()> {
        let config = CacheConfig::minimal();
        let cache = Cache::new(config)?;

        let key = "test_key".to_string();
        let data = Bytes::from("test_data");

        cache.put(key.clone(), data.clone())?;
        let retrieved = cache.get(&key)?;

        assert_eq!(retrieved, Some(data));
        Ok(())
    }

    #[test]
    fn test_cache_eviction() -> Result<()> {
        let mut config = CacheConfig::minimal();
        config.max_size = 100;
        config.max_entries = 10;

        let cache = Cache::new(config)?;

        // Fill cache
        for i in 0..5 {
            let key = format!("key_{}", i);
            let data = Bytes::from(vec![0u8; 25]);
            cache.put(key, data)?;
        }

        // Should trigger eviction
        let key = "new_key".to_string();
        let data = Bytes::from(vec![0u8; 25]);
        cache.put(key.clone(), data.clone())?;

        let retrieved = cache.get(&key)?;
        assert_eq!(retrieved, Some(data));

        Ok(())
    }

    #[test]
    fn test_cache_remove() -> Result<()> {
        let config = CacheConfig::minimal();
        let cache = Cache::new(config)?;

        let key = "test_key".to_string();
        let data = Bytes::from("test_data");

        cache.put(key.clone(), data.clone())?;
        let removed = cache.remove(&key)?;

        assert_eq!(removed, Some(data));
        assert_eq!(cache.get(&key)?, None);

        Ok(())
    }

    #[test]
    fn test_cache_clear() -> Result<()> {
        let config = CacheConfig::minimal();
        let cache = Cache::new(config)?;

        for i in 0..5 {
            let key = format!("key_{}", i);
            let data = Bytes::from(format!("data_{}", i));
            cache.put(key, data)?;
        }

        assert_eq!(cache.len(), 5);

        cache.clear()?;
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());

        Ok(())
    }

    #[test]
    fn test_entry_expiration() {
        let key = "test".to_string();
        let data = Bytes::from("data");

        let entry = CacheEntry::with_ttl(key, data, 0);
        std::thread::sleep(std::time::Duration::from_millis(10));
        assert!(entry.is_expired());
    }
}
