//! Chunk cache management for HDF5 datasets.
//!
//! This module provides LRU-based caching for chunked dataset chunks
//! to improve read and write performance.

use crate::chunking::ChunkIndex;
use crate::error::{Hdf5Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};

/// Cached chunk data
#[derive(Debug, Clone)]
pub struct CachedChunk {
    /// Chunk index
    index: ChunkIndex,
    /// Chunk data
    data: Vec<u8>,
    /// Whether chunk has been modified
    dirty: bool,
    /// Last access time (for LRU)
    last_access: u64,
    /// Access count
    access_count: u64,
}

impl CachedChunk {
    /// Create a new cached chunk
    pub fn new(index: ChunkIndex, data: Vec<u8>) -> Self {
        Self {
            index,
            data,
            dirty: false,
            last_access: 0,
            access_count: 0,
        }
    }

    /// Get the chunk index
    pub fn index(&self) -> &ChunkIndex {
        &self.index
    }

    /// Get the chunk data
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Get mutable chunk data and mark as dirty
    pub fn data_mut(&mut self) -> &mut Vec<u8> {
        self.dirty = true;
        &mut self.data
    }

    /// Check if chunk is dirty
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Mark chunk as clean
    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }

    /// Mark chunk as dirty
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Update access statistics
    pub fn update_access(&mut self, time: u64) {
        self.last_access = time;
        self.access_count += 1;
    }

    /// Get last access time
    pub fn last_access(&self) -> u64 {
        self.last_access
    }

    /// Get access count
    pub fn access_count(&self) -> u64 {
        self.access_count
    }
}

/// Cache eviction policy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvictionPolicy {
    /// Least Recently Used
    LRU,
    /// Least Frequently Used
    LFU,
    /// First In First Out
    FIFO,
}

/// Chunk cache configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// Maximum number of chunks to cache
    max_chunks: usize,
    /// Maximum cache size in bytes
    max_size_bytes: usize,
    /// Eviction policy
    eviction_policy: EvictionPolicy,
    /// Preload neighbors
    preload_neighbors: bool,
    /// Write-through (immediately write dirty chunks) vs write-back
    write_through: bool,
}

impl CacheConfig {
    /// Create a new cache configuration
    pub fn new(max_chunks: usize, max_size_bytes: usize) -> Self {
        Self {
            max_chunks,
            max_size_bytes,
            eviction_policy: EvictionPolicy::LRU,
            preload_neighbors: false,
            write_through: false,
        }
    }

    /// Set eviction policy
    pub fn with_eviction_policy(mut self, policy: EvictionPolicy) -> Self {
        self.eviction_policy = policy;
        self
    }

    /// Enable preloading of neighbor chunks
    pub fn with_preload_neighbors(mut self, enable: bool) -> Self {
        self.preload_neighbors = enable;
        self
    }

    /// Set write-through mode
    pub fn with_write_through(mut self, enable: bool) -> Self {
        self.write_through = enable;
        self
    }

    /// Get max chunks
    pub fn max_chunks(&self) -> usize {
        self.max_chunks
    }

    /// Get max size in bytes
    pub fn max_size_bytes(&self) -> usize {
        self.max_size_bytes
    }

    /// Get eviction policy
    pub fn eviction_policy(&self) -> EvictionPolicy {
        self.eviction_policy
    }

    /// Check if preload neighbors is enabled
    pub fn preload_neighbors_enabled(&self) -> bool {
        self.preload_neighbors
    }

    /// Check if write-through is enabled
    pub fn write_through_enabled(&self) -> bool {
        self.write_through
    }
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self::new(1000, 100 * 1024 * 1024) // 1000 chunks, 100 MB
    }
}

/// Chunk cache statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CacheStatistics {
    /// Total number of cache hits
    hits: u64,
    /// Total number of cache misses
    misses: u64,
    /// Total number of evictions
    evictions: u64,
    /// Total number of writes
    writes: u64,
    /// Total number of dirty chunks flushed
    flushes: u64,
    /// Current number of cached chunks
    num_cached: usize,
    /// Current cache size in bytes
    size_bytes: usize,
}

impl CacheStatistics {
    /// Create new statistics
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a cache hit
    pub fn record_hit(&mut self) {
        self.hits += 1;
    }

    /// Record a cache miss
    pub fn record_miss(&mut self) {
        self.misses += 1;
    }

    /// Record an eviction
    pub fn record_eviction(&mut self) {
        self.evictions += 1;
    }

    /// Record a write
    pub fn record_write(&mut self) {
        self.writes += 1;
    }

    /// Record a flush
    pub fn record_flush(&mut self) {
        self.flushes += 1;
    }

    /// Update cache state
    pub fn update_state(&mut self, num_cached: usize, size_bytes: usize) {
        self.num_cached = num_cached;
        self.size_bytes = size_bytes;
    }

    /// Calculate hit rate
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }

    /// Get statistics
    pub fn hits(&self) -> u64 {
        self.hits
    }

    pub fn misses(&self) -> u64 {
        self.misses
    }

    pub fn evictions(&self) -> u64 {
        self.evictions
    }

    pub fn writes(&self) -> u64 {
        self.writes
    }

    pub fn flushes(&self) -> u64 {
        self.flushes
    }

    pub fn num_cached(&self) -> usize {
        self.num_cached
    }

    pub fn size_bytes(&self) -> usize {
        self.size_bytes
    }
}

/// Chunk cache implementation
pub struct ChunkCache {
    /// Configuration
    config: CacheConfig,
    /// Cached chunks
    chunks: HashMap<String, CachedChunk>,
    /// LRU queue (for LRU policy)
    lru_queue: VecDeque<String>,
    /// Current cache size in bytes
    current_size: usize,
    /// Access counter (for timestamping)
    access_counter: u64,
    /// Statistics
    statistics: CacheStatistics,
}

impl ChunkCache {
    /// Create a new chunk cache
    pub fn new(config: CacheConfig) -> Self {
        Self {
            config,
            chunks: HashMap::new(),
            lru_queue: VecDeque::new(),
            current_size: 0,
            access_counter: 0,
            statistics: CacheStatistics::new(),
        }
    }

    /// Get a chunk from cache
    pub fn get(&mut self, index: &ChunkIndex) -> Option<&CachedChunk> {
        let key = Self::chunk_key(index);

        if self.chunks.contains_key(&key) {
            self.access_counter += 1;
            self.update_lru(&key);
            self.statistics.record_hit();

            // Update access time
            if let Some(chunk) = self.chunks.get_mut(&key) {
                chunk.update_access(self.access_counter);
            }

            self.chunks.get(&key)
        } else {
            self.statistics.record_miss();
            None
        }
    }

    /// Get a mutable chunk from cache
    pub fn get_mut(&mut self, index: &ChunkIndex) -> Option<&mut CachedChunk> {
        let key = Self::chunk_key(index);

        if self.chunks.contains_key(&key) {
            self.access_counter += 1;
            self.update_lru(&key);
            self.statistics.record_hit();
            self.chunks.get_mut(&key)
        } else {
            self.statistics.record_miss();
            None
        }
    }

    /// Put a chunk in cache
    pub fn put(&mut self, chunk: CachedChunk) -> Result<()> {
        let key = Self::chunk_key(chunk.index());
        let chunk_size = chunk.data.len();

        // Check if we need to evict chunks
        while self.needs_eviction(chunk_size) {
            self.evict_one()?;
        }

        // Insert chunk
        self.lru_queue.push_back(key.clone());
        self.current_size += chunk_size;
        self.chunks.insert(key, chunk);

        self.statistics
            .update_state(self.chunks.len(), self.current_size);

        Ok(())
    }

    /// Remove a chunk from cache
    pub fn remove(&mut self, index: &ChunkIndex) -> Option<CachedChunk> {
        let key = Self::chunk_key(index);

        if let Some(chunk) = self.chunks.remove(&key) {
            self.current_size -= chunk.data.len();
            self.lru_queue.retain(|k| k != &key);

            self.statistics
                .update_state(self.chunks.len(), self.current_size);

            Some(chunk)
        } else {
            None
        }
    }

    /// Check if cache contains a chunk
    pub fn contains(&self, index: &ChunkIndex) -> bool {
        self.chunks.contains_key(&Self::chunk_key(index))
    }

    /// Flush all dirty chunks
    pub fn flush_all(&mut self) -> Result<Vec<CachedChunk>> {
        let mut dirty_chunks = Vec::new();

        for chunk in self.chunks.values_mut() {
            if chunk.is_dirty() {
                dirty_chunks.push(chunk.clone());
                chunk.mark_clean();
                self.statistics.record_flush();
            }
        }

        Ok(dirty_chunks)
    }

    /// Flush a specific chunk
    pub fn flush(&mut self, index: &ChunkIndex) -> Result<Option<CachedChunk>> {
        let key = Self::chunk_key(index);

        if let Some(chunk) = self.chunks.get_mut(&key) {
            if chunk.is_dirty() {
                let flushed = chunk.clone();
                chunk.mark_clean();
                self.statistics.record_flush();
                return Ok(Some(flushed));
            }
        }

        Ok(None)
    }

    /// Clear all chunks from cache
    pub fn clear(&mut self) {
        self.chunks.clear();
        self.lru_queue.clear();
        self.current_size = 0;
        self.statistics
            .update_state(self.chunks.len(), self.current_size);
    }

    /// Get cache statistics
    pub fn statistics(&self) -> &CacheStatistics {
        &self.statistics
    }

    /// Get configuration
    pub fn config(&self) -> &CacheConfig {
        &self.config
    }

    /// Check if eviction is needed
    fn needs_eviction(&self, new_chunk_size: usize) -> bool {
        self.chunks.len() >= self.config.max_chunks
            || self.current_size + new_chunk_size > self.config.max_size_bytes
    }

    /// Evict one chunk
    fn evict_one(&mut self) -> Result<()> {
        let key_to_evict = match self.config.eviction_policy {
            EvictionPolicy::LRU => self.find_lru_victim(),
            EvictionPolicy::LFU => self.find_lfu_victim(),
            EvictionPolicy::FIFO => self.find_fifo_victim(),
        };

        if let Some(key) = key_to_evict {
            if let Some(chunk) = self.chunks.remove(&key) {
                if chunk.is_dirty() {
                    return Err(Hdf5Error::InvalidOperation(
                        "Cannot evict dirty chunk without flushing".to_string(),
                    ));
                }

                self.current_size -= chunk.data.len();
                self.lru_queue.retain(|k| k != &key);
                self.statistics.record_eviction();

                self.statistics
                    .update_state(self.chunks.len(), self.current_size);
            }
        }

        Ok(())
    }

    /// Find LRU victim
    fn find_lru_victim(&self) -> Option<String> {
        self.lru_queue.front().cloned()
    }

    /// Find LFU victim
    fn find_lfu_victim(&self) -> Option<String> {
        self.chunks
            .iter()
            .min_by_key(|(_, chunk)| chunk.access_count)
            .map(|(key, _)| key.clone())
    }

    /// Find FIFO victim
    fn find_fifo_victim(&self) -> Option<String> {
        self.lru_queue.front().cloned()
    }

    /// Update LRU queue
    fn update_lru(&mut self, key: &str) {
        if matches!(self.config.eviction_policy, EvictionPolicy::LRU) {
            self.lru_queue.retain(|k| k != key);
            self.lru_queue.push_back(key.to_string());
        }
    }

    /// Generate cache key from chunk index
    fn chunk_key(index: &ChunkIndex) -> String {
        index
            .coords()
            .iter()
            .map(|c| c.to_string())
            .collect::<Vec<_>>()
            .join("_")
    }
}

/// Thread-safe chunk cache
pub struct SharedChunkCache {
    cache: Arc<RwLock<ChunkCache>>,
}

impl SharedChunkCache {
    /// Create a new shared cache
    pub fn new(config: CacheConfig) -> Self {
        Self {
            cache: Arc::new(RwLock::new(ChunkCache::new(config))),
        }
    }

    /// Get a chunk from cache
    pub fn get(&self, index: &ChunkIndex) -> Result<Option<Vec<u8>>> {
        let mut cache = self
            .cache
            .write()
            .map_err(|e| Hdf5Error::ConcurrencyError(e.to_string()))?;

        Ok(cache.get(index).map(|chunk| chunk.data().to_vec()))
    }

    /// Put a chunk in cache
    pub fn put(&self, chunk: CachedChunk) -> Result<()> {
        let mut cache = self
            .cache
            .write()
            .map_err(|e| Hdf5Error::ConcurrencyError(e.to_string()))?;

        cache.put(chunk)
    }

    /// Flush all dirty chunks
    pub fn flush_all(&self) -> Result<Vec<CachedChunk>> {
        let mut cache = self
            .cache
            .write()
            .map_err(|e| Hdf5Error::ConcurrencyError(e.to_string()))?;

        cache.flush_all()
    }

    /// Get statistics
    pub fn statistics(&self) -> Result<CacheStatistics> {
        let cache = self
            .cache
            .read()
            .map_err(|e| Hdf5Error::ConcurrencyError(e.to_string()))?;

        Ok(cache.statistics().clone())
    }

    /// Clone the inner Arc
    pub fn clone_inner(&self) -> Self {
        Self {
            cache: Arc::clone(&self.cache),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cached_chunk() {
        let index = ChunkIndex::new(vec![1, 2, 3]);
        let data = vec![1, 2, 3, 4, 5];
        let mut chunk = CachedChunk::new(index, data);

        assert!(!chunk.is_dirty());
        chunk.mark_dirty();
        assert!(chunk.is_dirty());

        chunk.update_access(1);
        assert_eq!(chunk.last_access(), 1);
        assert_eq!(chunk.access_count(), 1);

        chunk.update_access(2);
        assert_eq!(chunk.last_access(), 2);
        assert_eq!(chunk.access_count(), 2);
    }

    #[test]
    fn test_cache_config() {
        let config = CacheConfig::new(100, 1024 * 1024)
            .with_eviction_policy(EvictionPolicy::LFU)
            .with_preload_neighbors(true)
            .with_write_through(true);

        assert_eq!(config.max_chunks(), 100);
        assert_eq!(config.max_size_bytes(), 1024 * 1024);
        assert_eq!(config.eviction_policy(), EvictionPolicy::LFU);
        assert!(config.preload_neighbors_enabled());
        assert!(config.write_through_enabled());
    }

    #[test]
    fn test_cache_statistics() {
        let mut stats = CacheStatistics::new();

        stats.record_hit();
        stats.record_hit();
        stats.record_miss();

        assert_eq!(stats.hits(), 2);
        assert_eq!(stats.misses(), 1);
        assert_eq!(stats.hit_rate(), 2.0 / 3.0);
    }

    #[test]
    fn test_chunk_cache() {
        let config = CacheConfig::new(2, 1000);
        let mut cache = ChunkCache::new(config);

        let chunk1 = CachedChunk::new(ChunkIndex::new(vec![0, 0]), vec![1, 2, 3, 4]);
        let chunk2 = CachedChunk::new(ChunkIndex::new(vec![0, 1]), vec![5, 6, 7, 8]);

        cache.put(chunk1).expect("Failed to put chunk1");
        cache.put(chunk2).expect("Failed to put chunk2");

        assert_eq!(cache.statistics().num_cached(), 2);

        let retrieved = cache.get(&ChunkIndex::new(vec![0, 0]));
        assert!(retrieved.is_some());
        assert_eq!(cache.statistics().hits(), 1);

        let missing = cache.get(&ChunkIndex::new(vec![1, 1]));
        assert!(missing.is_none());
        assert_eq!(cache.statistics().misses(), 1);
    }

    #[test]
    fn test_cache_eviction() {
        let config = CacheConfig::new(2, 1000);
        let mut cache = ChunkCache::new(config);

        let chunk1 = CachedChunk::new(ChunkIndex::new(vec![0, 0]), vec![1; 100]);
        let chunk2 = CachedChunk::new(ChunkIndex::new(vec![0, 1]), vec![2; 100]);
        let chunk3 = CachedChunk::new(ChunkIndex::new(vec![0, 2]), vec![3; 100]);

        cache.put(chunk1).expect("Failed to put chunk1");
        cache.put(chunk2).expect("Failed to put chunk2");

        // This should trigger eviction
        cache.put(chunk3).expect("Failed to put chunk3");

        assert_eq!(cache.statistics().evictions(), 1);
        assert_eq!(cache.statistics().num_cached(), 2);
    }

    #[test]
    fn test_cache_flush() {
        let config = CacheConfig::new(10, 1000);
        let mut cache = ChunkCache::new(config);

        let mut chunk = CachedChunk::new(ChunkIndex::new(vec![0, 0]), vec![1, 2, 3, 4]);
        chunk.mark_dirty();

        cache.put(chunk).expect("Failed to put chunk");

        let dirty = cache.flush_all().expect("Failed to flush");
        assert_eq!(dirty.len(), 1);
        assert_eq!(cache.statistics().flushes(), 1);
    }

    #[test]
    fn test_shared_cache() {
        let config = CacheConfig::new(10, 1000);
        let cache = SharedChunkCache::new(config);

        let chunk = CachedChunk::new(ChunkIndex::new(vec![0, 0]), vec![1, 2, 3, 4]);
        cache.put(chunk).expect("Failed to put chunk");

        let data = cache
            .get(&ChunkIndex::new(vec![0, 0]))
            .expect("Failed to get chunk");
        assert!(data.is_some());
    }
}
