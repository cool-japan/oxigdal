//! Shader compilation cache with LRU eviction.

use super::CompiledShader;
use blake3::Hash;
use lru::LruCache;
use parking_lot::Mutex;
use std::num::NonZeroUsize;

/// Default cache capacity (1000 shaders)
const DEFAULT_CACHE_CAPACITY: NonZeroUsize = match NonZeroUsize::new(1000) {
    Some(v) => v,
    None => unreachable!(),
};

/// Shader cache with LRU eviction policy
pub struct ShaderCache {
    /// LRU cache for compiled shaders
    cache: Mutex<LruCache<Hash, CompiledShader>>,
    /// Cache hits
    hits: Mutex<u64>,
    /// Cache misses
    misses: Mutex<u64>,
}

impl ShaderCache {
    /// Create a new shader cache with given capacity
    pub fn new(capacity: usize) -> Self {
        let cap = NonZeroUsize::new(capacity).unwrap_or(DEFAULT_CACHE_CAPACITY);
        Self {
            cache: Mutex::new(LruCache::new(cap)),
            hits: Mutex::new(0),
            misses: Mutex::new(0),
        }
    }

    /// Insert a compiled shader into cache
    pub fn insert(&self, hash: Hash, shader: CompiledShader) {
        let mut cache = self.cache.lock();
        cache.put(hash, shader);
    }

    /// Get a compiled shader from cache
    pub fn get(&self, hash: &Hash) -> Option<CompiledShader> {
        let mut cache = self.cache.lock();
        if let Some(shader) = cache.get(hash) {
            *self.hits.lock() += 1;
            Some(shader.clone())
        } else {
            *self.misses.lock() += 1;
            None
        }
    }

    /// Check if cache contains a shader
    pub fn contains(&self, hash: &Hash) -> bool {
        let cache = self.cache.lock();
        cache.contains(hash)
    }

    /// Clear the cache
    pub fn clear(&self) {
        let mut cache = self.cache.lock();
        cache.clear();
    }

    /// Get cache size
    pub fn len(&self) -> usize {
        let cache = self.cache.lock();
        cache.len()
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get cache statistics
    pub fn get_stats(&self) -> CacheStats {
        let hits = *self.hits.lock();
        let misses = *self.misses.lock();
        let size = self.len();

        CacheStats {
            hits,
            misses,
            size,
            hit_rate: if hits + misses > 0 {
                (hits as f64) / ((hits + misses) as f64)
            } else {
                0.0
            },
        }
    }

    /// Reset statistics
    pub fn reset_stats(&self) {
        *self.hits.lock() = 0;
        *self.misses.lock() = 0;
    }
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    /// Cache hits
    pub hits: u64,
    /// Cache misses
    pub misses: u64,
    /// Current cache size
    pub size: usize,
    /// Hit rate (0.0 to 1.0)
    pub hit_rate: f64,
}

impl CacheStats {
    /// Print statistics
    pub fn print(&self) {
        println!("\nShader Cache Statistics:");
        println!("  Hits: {}", self.hits);
        println!("  Misses: {}", self.misses);
        println!("  Hit rate: {:.1}%", self.hit_rate * 100.0);
        println!("  Cache size: {}", self.size);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_creation() {
        let cache = ShaderCache::new(100);
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_cache_stats() {
        let cache = ShaderCache::new(100);
        let stats = cache.get_stats();
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 0);
        assert_eq!(stats.hit_rate, 0.0);
    }
}
