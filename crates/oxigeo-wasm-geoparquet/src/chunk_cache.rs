//! A byte-capacity-bounded, *true* LRU cache keyed by `(row_group,
//! leaf_column)`.
//!
//! Deliberately platform-independent (no `wasm_bindgen` / `web_sys`) so its
//! eviction behaviour can be unit-tested natively without a browser;
//! `session.rs` (wasm32-only) wraps a [`ChunkCache`] to memoise fetched
//! Parquet column chunks across queries in the same `RemoteGeoParquet`
//! session.
//!
//! Eviction picks the entry with the globally smallest access-sequence
//! number — i.e. the one least recently inserted *or* read — never the
//! numerically smallest `(row_group, leaf_column)` key. A naive
//! `BTreeMap::keys().next()`-style eviction (picking by key order) would
//! always target row group 0's earliest columns first regardless of how
//! recently or frequently they were used, thrashing exactly the chunks a
//! spatially-clustered query keeps re-requesting.

// This module's only production consumer is the wasm32-only `session`
// module, so on a native (non-wasm32) build — where `session` is not
// compiled — its API looks unused to the compiler even though it is
// covered by the native unit tests below.
#![allow(dead_code)]

use std::collections::BTreeMap;

use bytes::Bytes;

/// A cached column chunk plus the sequence number of its most recent access
/// (insertion or cache hit).
struct CacheEntry {
    data: Bytes,
    last_used: u64,
}

/// A byte-capacity-bounded cache keyed by `(row_group, leaf_column)`.
pub struct ChunkCache {
    entries: BTreeMap<(usize, usize), CacheEntry>,
    bytes: usize,
    cap_bytes: usize,
    /// Monotonic counter stamped onto a [`CacheEntry`] on every insert or
    /// hit; the entry with the smallest value is the least-recently-used one.
    next_seq: u64,
}

impl ChunkCache {
    /// Builds an empty cache with the given byte capacity.
    #[must_use]
    pub fn new(cap_bytes: usize) -> Self {
        Self {
            entries: BTreeMap::new(),
            bytes: 0,
            cap_bytes,
            next_seq: 0,
        }
    }

    /// Total bytes currently held.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Whether `key` is currently cached (does not affect recency).
    #[must_use]
    pub fn contains_key(&self, key: &(usize, usize)) -> bool {
        self.entries.contains_key(key)
    }

    /// Reads `key` if cached, marking it as the most-recently-used entry.
    pub fn get(&mut self, key: (usize, usize)) -> Option<Bytes> {
        let seq = self.next_seq();
        let entry = self.entries.get_mut(&key)?;
        entry.last_used = seq;
        Some(entry.data.clone())
    }

    /// Inserts a freshly fetched chunk, evicting the least-recently-used
    /// entry (by insertion or last access) — other than `key` itself — until
    /// back under the capacity.
    pub fn insert(&mut self, key: (usize, usize), data: Bytes) {
        let added = data.len();
        let seq = self.next_seq();
        if let Some(old) = self.entries.insert(
            key,
            CacheEntry {
                data,
                last_used: seq,
            },
        ) {
            self.bytes = self.bytes.saturating_sub(old.data.len());
        }
        self.bytes += added;
        while self.bytes > self.cap_bytes {
            let victim = self
                .entries
                .iter()
                .filter(|(k, _)| **k != key)
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(k, _)| *k);
            match victim {
                Some(v) => {
                    if let Some(removed) = self.entries.remove(&v) {
                        self.bytes = self.bytes.saturating_sub(removed.data.len());
                    }
                }
                None => break,
            }
        }
    }

    /// The next monotonic access-sequence number, for LRU bookkeeping.
    fn next_seq(&mut self) -> u64 {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        seq
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn bytes_of(len: usize) -> Bytes {
        Bytes::from(vec![0u8; len])
    }

    #[test]
    fn get_before_any_insert_is_a_miss() {
        let mut cache = ChunkCache::new(1024);
        assert!(cache.get((0, 0)).is_none());
        assert!(!cache.contains_key(&(0, 0)));
    }

    #[test]
    fn insert_then_get_is_a_hit_with_same_bytes() {
        let mut cache = ChunkCache::new(1024);
        cache.insert((2, 1), bytes_of(10));
        assert_eq!(cache.bytes(), 10);
        assert!(cache.contains_key(&(2, 1)));
        assert_eq!(cache.get((2, 1)).map(|b| b.len()), Some(10));
    }

    /// Reproduces the exact bug the finding describes: with 3 entries under a
    /// cap that only fits 2, `BTreeMap`-key-order eviction would always drop
    /// `(0, 0)` — the numerically smallest key — even when it is the most
    /// recently used. True LRU must instead drop whichever entry has gone
    /// longest untouched, regardless of its key.
    #[test]
    fn eviction_targets_least_recently_used_not_smallest_key() {
        let mut cache = ChunkCache::new(25); // fits at most 2 x 10-byte chunks, tightly.
        cache.insert((0, 0), bytes_of(10));
        cache.insert((5, 5), bytes_of(10));
        // Touch (0, 0) again so it is now the *most* recently used, while
        // (5, 5) becomes the least-recently used of the two resident entries.
        assert!(cache.get((0, 0)).is_some());
        // This insert pushes total bytes to 30 > 25, forcing one eviction.
        // Key-order eviction (the bug) would drop (0, 0) since it sorts
        // first; true LRU must drop (5, 5) instead, since it was untouched
        // since its insertion while (0, 0) was just read.
        cache.insert((9, 9), bytes_of(10));
        assert!(
            cache.contains_key(&(0, 0)),
            "the just-touched (recently used) entry must survive eviction"
        );
        assert!(
            !cache.contains_key(&(5, 5)),
            "the untouched (least-recently-used) entry must be evicted, not (0,0)"
        );
        assert!(
            cache.contains_key(&(9, 9)),
            "the just-inserted key always survives its own insert"
        );
    }

    #[test]
    fn eviction_never_drops_the_key_currently_being_inserted() {
        let mut cache = ChunkCache::new(15);
        cache.insert((1, 1), bytes_of(10));
        // Overwriting/growing the same key that is already at/over capacity
        // must not evict itself.
        cache.insert((1, 1), bytes_of(20));
        assert!(cache.contains_key(&(1, 1)));
        assert_eq!(cache.bytes(), 20);
    }

    #[test]
    fn repeated_inserts_of_same_key_replace_rather_than_double_count_bytes() {
        let mut cache = ChunkCache::new(1024);
        cache.insert((3, 3), bytes_of(10));
        cache.insert((3, 3), bytes_of(4));
        assert_eq!(cache.bytes(), 4);
    }

    #[test]
    fn many_small_insertions_never_exceed_capacity() {
        let mut cache = ChunkCache::new(50);
        for i in 0..20 {
            cache.insert((i, 0), bytes_of(10));
        }
        assert!(
            cache.bytes() <= 50,
            "cache grew past its capacity: {}",
            cache.bytes()
        );
    }
}
