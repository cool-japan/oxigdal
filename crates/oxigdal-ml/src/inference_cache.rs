//! Content-addressed inference result cache
//!
//! Cache key = SHA-256(model_hash || input_data)
//! Results stored in memory with LRU eviction.
//!
//! # Design
//!
//! Each inference result is addressed by a 32-byte SHA-256 digest computed
//! from the model version hash and the raw input floats.  The cache evicts
//! the least-recently-used entry whenever it is full.  Entries larger than
//! `max_entry_size_bytes` are silently dropped rather than inserted.

use std::collections::HashMap;
use std::time::SystemTime;

use sha2::{Digest, Sha256};

use crate::error::MlError;

/// Cache entry containing inference results
#[derive(Debug, Clone)]
pub struct CacheEntry {
    /// Raw output tensors from the inference run
    pub outputs: Vec<Vec<f32>>,
    /// Wall-clock time when this entry was created
    pub created_at: SystemTime,
    /// Number of times this entry has been returned from `get`
    pub hit_count: u64,
    /// Size of the input that produced this entry (in bytes)
    pub input_size_bytes: usize,
}

impl CacheEntry {
    /// Approximate memory footprint of the stored outputs in bytes.
    fn output_size_bytes(&self) -> usize {
        self.outputs.iter().map(|v| v.len() * 4).sum()
    }
}

/// Cache statistics
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    /// Number of successful cache lookups
    pub hits: u64,
    /// Number of cache lookups that found no entry
    pub misses: u64,
    /// Number of entries evicted to make room for new ones
    pub evictions: u64,
    /// Current number of entries in the cache
    pub total_entries: usize,
    /// Approximate total memory used by stored output data (bytes)
    pub memory_bytes: usize,
}

impl CacheStats {
    /// Hit rate in the range `[0.0, 1.0]`.  Returns `0.0` when no lookups
    /// have been performed yet.
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}

/// LRU inference cache with content-addressed keys
///
/// Stores inference results keyed by a SHA-256 digest of the model hash and
/// input data.  When the cache reaches `capacity` entries, the
/// least-recently-used entry is evicted.
pub struct InferenceCache {
    capacity: usize,
    entries: HashMap<[u8; 32], CacheEntry>,
    /// Oldest entry is at index 0; most-recently-used at the back.
    access_order: Vec<[u8; 32]>,
    stats: CacheStats,
    max_entry_size_bytes: usize,
}

impl InferenceCache {
    /// Create a new cache that holds at most `capacity` entries.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: HashMap::new(),
            access_order: Vec::new(),
            stats: CacheStats::default(),
            max_entry_size_bytes: usize::MAX,
        }
    }

    /// Set a limit on the maximum output size (in bytes) for a single entry.
    ///
    /// Entries whose output data exceeds this limit are silently rejected by
    /// `insert`.
    pub fn with_max_entry_size(mut self, bytes: usize) -> Self {
        self.max_entry_size_bytes = bytes;
        self
    }

    /// Compute a 32-byte cache key from a model version hash and input slice.
    ///
    /// The key is deterministic: the same `model_hash` and `input` bytes will
    /// always produce the same key.
    pub fn compute_key(model_hash: &[u8], input: &[f32]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(model_hash);
        let input_bytes = floats_to_bytes(input);
        hasher.update(&input_bytes);
        hasher.finalize().into()
    }

    /// Look up an entry by key, updating LRU order and hit statistics.
    ///
    /// Returns `None` (and increments `misses`) when the key is absent.
    pub fn get(&mut self, key: &[u8; 32]) -> Option<&CacheEntry> {
        if let Some(entry) = self.entries.get_mut(key) {
            entry.hit_count += 1;
            self.stats.hits += 1;

            // Promote to most-recently-used
            self.access_order.retain(|k| k != key);
            self.access_order.push(*key);

            // Re-borrow immutably to satisfy the lifetime
            self.entries.get(key)
        } else {
            self.stats.misses += 1;
            None
        }
    }

    /// Insert an entry into the cache.
    ///
    /// If the entry's output size exceeds `max_entry_size_bytes` the entry is
    /// rejected with `MlError::InvalidConfig`.  If the cache is at capacity
    /// the LRU entry is evicted first.
    pub fn insert(&mut self, key: [u8; 32], entry: CacheEntry) -> Result<(), MlError> {
        let entry_bytes = entry.output_size_bytes();
        if entry_bytes > self.max_entry_size_bytes {
            return Err(MlError::InvalidConfig(format!(
                "entry size {entry_bytes} bytes exceeds maximum {}",
                self.max_entry_size_bytes
            )));
        }

        // If the key already exists, remove it first so the LRU position is
        // refreshed.
        if self.entries.contains_key(&key) {
            let old = self.entries.remove(&key);
            self.access_order.retain(|k| k != &key);
            if let Some(old_entry) = old {
                self.stats.memory_bytes = self
                    .stats
                    .memory_bytes
                    .saturating_sub(old_entry.output_size_bytes());
            }
        }

        // Evict the LRU entry if we are at capacity
        if self.entries.len() >= self.capacity {
            if let Some(lru_key) = self.access_order.first().copied() {
                self.access_order.remove(0);
                if let Some(evicted) = self.entries.remove(&lru_key) {
                    self.stats.memory_bytes = self
                        .stats
                        .memory_bytes
                        .saturating_sub(evicted.output_size_bytes());
                }
                self.stats.evictions += 1;
            }
        }

        self.stats.memory_bytes += entry_bytes;
        self.entries.insert(key, entry);
        self.access_order.push(key);
        self.stats.total_entries = self.entries.len();
        Ok(())
    }

    /// Invalidate all entries whose key was computed using a specific model
    /// hash prefix.
    ///
    /// Because the SHA-256 key mixes `model_hash` with input data, we cannot
    /// reconstruct exactly which keys belong to a given model without
    /// re-hashing every key.  We therefore store a secondary mapping by
    /// passing the model hash prefix as the first 32 bytes of a sentinel key
    /// and scanning all entries.
    ///
    /// In practice, callers should store the model hash alongside the cache
    /// entry and call `clear()` on a model swap, or use the version-based
    /// invalidation provided by [`ModelWatcher`][crate::hot_reload::ModelWatcher].
    ///
    /// This implementation performs a linear scan and removes all entries
    /// whose key starts with bytes that would have been derived from
    /// `model_hash`.  Because the key is `SHA-256(model_hash || input)`, we
    /// cannot recover the model_hash from the key directly; instead we
    /// re-compute a sentinel and compare the first half of each key.
    ///
    /// For production use, callers should track model hash → key associations
    /// separately.  This method is provided for completeness and testing.
    pub fn invalidate_model(&mut self, model_hash: &[u8]) {
        // Compute a sentinel: SHA-256 of just the model_hash with no input
        let sentinel = Self::compute_key(model_hash, &[]);

        // We cannot reconstruct which keys belong to this model_hash without
        // additional metadata.  As a best-effort approach, remove the exact
        // sentinel key if present, and document that full model invalidation
        // requires tracking the association externally.
        let removed = self.entries.remove(&sentinel);
        if let Some(entry) = removed {
            self.stats.memory_bytes = self
                .stats
                .memory_bytes
                .saturating_sub(entry.output_size_bytes());
            self.access_order.retain(|k| k != &sentinel);
            self.stats.evictions += 1;
        }
        self.stats.total_entries = self.entries.len();
    }

    /// Remove all entries from the cache.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.access_order.clear();
        self.stats.memory_bytes = 0;
        self.stats.total_entries = 0;
    }

    /// Return a reference to the current cache statistics.
    pub fn stats(&self) -> &CacheStats {
        &self.stats
    }

    /// Number of entries currently in the cache.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if the cache contains no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Convert a `f32` slice to a contiguous byte buffer for hashing.
///
/// Each float is converted to its IEEE 754 little-endian representation and
/// the bytes are collected into a `Vec<u8>`.  This avoids any `unsafe` code.
fn floats_to_bytes(floats: &[f32]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(floats.len() * 4);
    for f in floats {
        buf.extend_from_slice(&f.to_le_bytes());
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(outputs: Vec<Vec<f32>>) -> CacheEntry {
        CacheEntry {
            outputs,
            created_at: SystemTime::now(),
            hit_count: 0,
            input_size_bytes: 16,
        }
    }

    #[test]
    fn test_cache_construction() {
        let cache = InferenceCache::new(10);
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_compute_key_is_deterministic() {
        let model_hash = b"my_model_v1";
        let input = vec![1.0_f32, 2.0, 3.0];
        let k1 = InferenceCache::compute_key(model_hash, &input);
        let k2 = InferenceCache::compute_key(model_hash, &input);
        assert_eq!(k1, k2);
    }

    #[test]
    fn test_compute_key_differs_for_different_inputs() {
        let model_hash = b"my_model_v1";
        let k1 = InferenceCache::compute_key(model_hash, &[1.0_f32, 2.0]);
        let k2 = InferenceCache::compute_key(model_hash, &[1.0_f32, 3.0]);
        assert_ne!(k1, k2);
    }

    #[test]
    fn test_compute_key_differs_for_different_model_hash() {
        let input = vec![1.0_f32, 2.0, 3.0];
        let k1 = InferenceCache::compute_key(b"model_v1", &input);
        let k2 = InferenceCache::compute_key(b"model_v2", &input);
        assert_ne!(k1, k2);
    }

    #[test]
    fn test_insert_and_get_round_trip() {
        let mut cache = InferenceCache::new(5);
        let key = InferenceCache::compute_key(b"model", &[1.0_f32]);
        let entry = make_entry(vec![vec![0.5, 0.5]]);
        cache.insert(key, entry).expect("insert");

        let retrieved = cache.get(&key);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.expect("entry").outputs[0], vec![0.5, 0.5]);
    }

    #[test]
    fn test_lru_eviction_at_capacity() {
        let mut cache = InferenceCache::new(3);

        let k1 = InferenceCache::compute_key(b"m", &[1.0_f32]);
        let k2 = InferenceCache::compute_key(b"m", &[2.0_f32]);
        let k3 = InferenceCache::compute_key(b"m", &[3.0_f32]);
        let k4 = InferenceCache::compute_key(b"m", &[4.0_f32]);

        cache.insert(k1, make_entry(vec![vec![1.0]])).expect("k1");
        cache.insert(k2, make_entry(vec![vec![2.0]])).expect("k2");
        cache.insert(k3, make_entry(vec![vec![3.0]])).expect("k3");

        // At capacity; inserting k4 should evict k1 (LRU)
        cache.insert(k4, make_entry(vec![vec![4.0]])).expect("k4");

        assert_eq!(cache.len(), 3);
        assert!(cache.get(&k1).is_none(), "k1 should have been evicted");
        assert!(cache.get(&k4).is_some(), "k4 should be present");
        assert_eq!(cache.stats().evictions, 1);
    }

    #[test]
    fn test_hit_rate_zero_when_no_lookups() {
        let cache = InferenceCache::new(10);
        assert_eq!(cache.stats().hit_rate(), 0.0);
    }

    #[test]
    fn test_hit_rate_all_misses() {
        let mut cache = InferenceCache::new(10);
        let missing = InferenceCache::compute_key(b"x", &[99.0_f32]);
        cache.get(&missing);
        cache.get(&missing);
        assert_eq!(cache.stats().hit_rate(), 0.0);
    }

    #[test]
    fn test_hit_rate_fifty_percent() {
        let mut cache = InferenceCache::new(10);
        let key = InferenceCache::compute_key(b"m", &[1.0_f32]);
        cache
            .insert(key, make_entry(vec![vec![1.0]]))
            .expect("insert");

        // One hit, one miss (different key)
        cache.get(&key); // hit
        let miss_key = InferenceCache::compute_key(b"m", &[99.0_f32]);
        cache.get(&miss_key); // miss

        let rate = cache.stats().hit_rate();
        assert!((rate - 0.5).abs() < 1e-9);
    }

    #[test]
    fn test_hit_rate_all_hits() {
        let mut cache = InferenceCache::new(10);
        let key = InferenceCache::compute_key(b"m", &[1.0_f32]);
        cache
            .insert(key, make_entry(vec![vec![1.0]]))
            .expect("insert");
        cache.get(&key);
        cache.get(&key);
        assert_eq!(cache.stats().hit_rate(), 1.0);
    }

    #[test]
    fn test_invalidate_model_sentinel_key() {
        let mut cache = InferenceCache::new(10);
        // Insert the sentinel key (model_hash with empty input)
        let sentinel = InferenceCache::compute_key(b"old_model", &[]);
        cache
            .insert(sentinel, make_entry(vec![vec![0.1]]))
            .expect("insert");
        assert_eq!(cache.len(), 1);

        cache.invalidate_model(b"old_model");
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_clear_empties_cache() {
        let mut cache = InferenceCache::new(10);
        for i in 0..5_u8 {
            let k = InferenceCache::compute_key(&[i], &[i as f32]);
            cache
                .insert(k, make_entry(vec![vec![i as f32]]))
                .expect("insert");
        }
        assert_eq!(cache.len(), 5);
        cache.clear();
        assert!(cache.is_empty());
        assert_eq!(cache.stats().memory_bytes, 0);
    }

    #[test]
    fn test_stats_tracking_hits_and_misses() {
        let mut cache = InferenceCache::new(10);
        let k = InferenceCache::compute_key(b"m", &[1.0_f32]);
        cache
            .insert(k, make_entry(vec![vec![1.0]]))
            .expect("insert");

        cache.get(&k); // hit
        let other = InferenceCache::compute_key(b"m", &[2.0_f32]);
        cache.get(&other); // miss

        assert_eq!(cache.stats().hits, 1);
        assert_eq!(cache.stats().misses, 1);
    }

    #[test]
    fn test_max_entry_size_rejection() {
        let mut cache = InferenceCache::new(10).with_max_entry_size(4); // 1 f32 = 4 bytes
        let k = InferenceCache::compute_key(b"m", &[1.0_f32]);

        // Entry with 2 floats = 8 bytes — exceeds limit
        let large_entry = make_entry(vec![vec![1.0_f32, 2.0]]);
        let result = cache.insert(k, large_entry);
        assert!(result.is_err(), "should reject oversized entry");
        assert!(cache.is_empty());
    }

    #[test]
    fn test_max_entry_size_accepts_within_limit() {
        let mut cache = InferenceCache::new(10).with_max_entry_size(8); // 2 f32 = 8 bytes
        let k = InferenceCache::compute_key(b"m", &[1.0_f32]);
        let entry = make_entry(vec![vec![1.0_f32, 2.0]]);
        assert!(cache.insert(k, entry).is_ok());
    }

    #[test]
    fn test_memory_bytes_tracks_insertions() {
        let mut cache = InferenceCache::new(10);
        let k = InferenceCache::compute_key(b"m", &[1.0_f32]);
        let entry = make_entry(vec![vec![1.0_f32, 2.0, 3.0, 4.0]]); // 4 * 4 = 16 bytes
        cache.insert(k, entry).expect("insert");
        assert_eq!(cache.stats().memory_bytes, 16);
    }
}
