//! Caching storage wrapper for Zarr arrays
//!
//! This module provides a caching layer that wraps any storage backend,
//! improving read performance for frequently accessed chunks.

use super::{Store, StoreKey};
use crate::error::Result;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// LRU cache entry
struct CacheEntry {
    value: Vec<u8>,
    /// Access count for simple LRU approximation
    access_count: u64,
}

/// Caching storage wrapper
///
/// Wraps any storage backend and caches read values in memory.
pub struct CachingStorage<S: Store> {
    /// Underlying storage backend
    inner: S,
    /// In-memory cache
    cache: Arc<RwLock<HashMap<String, CacheEntry>>>,
    /// Maximum cache size in bytes
    max_size: usize,
    /// Current cache size in bytes
    current_size: Arc<RwLock<usize>>,
}

impl<S: Store> CachingStorage<S> {
    /// Creates a new caching storage wrapper
    ///
    /// # Arguments
    /// * `inner` - The underlying storage backend
    /// * `max_size` - Maximum cache size in bytes
    #[must_use]
    pub fn new(inner: S, max_size: usize) -> Self {
        Self {
            inner,
            cache: Arc::new(RwLock::new(HashMap::new())),
            max_size,
            current_size: Arc::new(RwLock::new(0)),
        }
    }

    /// Clears the cache
    pub fn clear(&self) {
        if let Ok(mut cache) = self.cache.write() {
            cache.clear();
        }
        if let Ok(mut size) = self.current_size.write() {
            *size = 0;
        }
    }

    /// Returns the current cache size in bytes
    #[must_use]
    pub fn cache_size(&self) -> usize {
        self.current_size.read().map(|s| *s).unwrap_or(0)
    }

    /// Returns the number of cached entries
    #[must_use]
    pub fn cache_entries(&self) -> usize {
        self.cache.read().map(|c| c.len()).unwrap_or(0)
    }

    fn evict_if_needed(&self, new_entry_size: usize) {
        let current = self.cache_size();
        if current + new_entry_size <= self.max_size {
            return;
        }

        // Simple eviction: remove entries until we have space
        if let Ok(mut cache) = self.cache.write() {
            let mut to_remove = Vec::new();
            let mut freed = 0usize;
            let needed = (current + new_entry_size).saturating_sub(self.max_size);

            // Find entries to remove (simple FIFO for now)
            for (key, entry) in cache.iter() {
                if freed >= needed {
                    break;
                }
                to_remove.push(key.clone());
                freed += entry.value.len();
            }

            for key in to_remove {
                if let Some(entry) = cache.remove(&key) {
                    if let Ok(mut size) = self.current_size.write() {
                        *size = size.saturating_sub(entry.value.len());
                    }
                }
            }
        }
    }
}

impl<S: Store> Store for CachingStorage<S> {
    fn get(&self, key: &StoreKey) -> Result<Vec<u8>> {
        let cache_key = key.as_str().to_string();

        // Check cache first
        if let Ok(mut cache) = self.cache.write() {
            if let Some(entry) = cache.get_mut(&cache_key) {
                entry.access_count += 1;
                return Ok(entry.value.clone());
            }
        }

        // Fetch from underlying storage
        let result = self.inner.get(key)?;

        // Cache the result
        self.evict_if_needed(result.len());

        if let Ok(mut cache) = self.cache.write() {
            let entry = CacheEntry {
                value: result.clone(),
                access_count: 1,
            };
            let size = entry.value.len();
            cache.insert(cache_key, entry);

            if let Ok(mut current) = self.current_size.write() {
                *current += size;
            }
        }

        Ok(result)
    }

    fn set(&mut self, key: &StoreKey, value: &[u8]) -> Result<()> {
        // Invalidate cache entry
        let cache_key = key.as_str().to_string();
        if let Ok(mut cache) = self.cache.write() {
            if let Some(entry) = cache.remove(&cache_key) {
                if let Ok(mut size) = self.current_size.write() {
                    *size = size.saturating_sub(entry.value.len());
                }
            }
        }

        self.inner.set(key, value)
    }

    fn delete(&mut self, key: &StoreKey) -> Result<()> {
        // Invalidate cache entry
        let cache_key = key.as_str().to_string();
        if let Ok(mut cache) = self.cache.write() {
            if let Some(entry) = cache.remove(&cache_key) {
                if let Ok(mut size) = self.current_size.write() {
                    *size = size.saturating_sub(entry.value.len());
                }
            }
        }

        self.inner.delete(key)
    }

    fn exists(&self, key: &StoreKey) -> Result<bool> {
        // Check cache first
        let cache_key = key.as_str().to_string();
        if let Ok(cache) = self.cache.read() {
            if cache.contains_key(&cache_key) {
                return Ok(true);
            }
        }

        self.inner.exists(key)
    }

    fn list_prefix(&self, prefix: &StoreKey) -> Result<Vec<StoreKey>> {
        // Don't cache list results, delegate to inner
        self.inner.list_prefix(prefix)
    }
}

#[cfg(test)]
mod tests {
    use super::super::memory::MemoryStore;
    use super::*;

    #[test]
    fn test_caching_storage() {
        let inner = MemoryStore::new();
        let cache = CachingStorage::new(inner, 1024 * 1024);

        assert_eq!(cache.cache_size(), 0);
        assert_eq!(cache.cache_entries(), 0);
    }

    #[test]
    fn test_cache_clear() {
        let inner = MemoryStore::new();
        let cache = CachingStorage::new(inner, 1024 * 1024);

        cache.clear();
        assert_eq!(cache.cache_size(), 0);
    }
}
