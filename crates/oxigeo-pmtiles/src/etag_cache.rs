//! ETag-based byte-range cache for the HTTP PMTiles reader.
//!
//! This module provides [`EtagCache`], a bounded LRU cache that stores raw
//! byte ranges keyed by `(offset, length)`.  PMTiles archives are immutable
//! by specification — once written, a tile or directory section never changes
//! — so a cached range is valid for the entire lifetime of the reader without
//! any revalidation.
//!
//! The stored ETag string is a synthetic stable key derived from
//! `"<offset>-<length>"` because [`oxigeo_streaming::cloud::HttpObjectStore`]
//! does not yet expose response headers (the upstream `If-None-Match`
//! conditional-GET path is documented here for future extension but is out of
//! scope for this workstream).
//!
//! # LRU eviction
//!
//! [`EtagCache`] tracks access order via a [`VecDeque<RangeKey>`] where the
//! *front* is the least-recently-used entry and the *back* is the
//! most-recently-used entry.  On every successful [`get`](EtagCache::get) the
//! accessed key is moved to the back.  On [`insert`](EtagCache::insert) at
//! capacity the front entry is evicted from both the deque and the backing
//! [`HashMap`].
//!
//! # Zero-capacity special case
//!
//! When `max_entries == 0` the cache is permanently disabled: every
//! [`get`](EtagCache::get) returns `None` and every
//! [`insert`](EtagCache::insert) is a no-op.  This allows callers to
//! semantically disable caching without a branching code path.

#![cfg(feature = "http-range")]

use std::collections::{HashMap, VecDeque};

// ─────────────────────────────────────────────────────────────────────────────
// RangeKey
// ─────────────────────────────────────────────────────────────────────────────

/// A composite cache key derived from a byte-range `(offset, length)` pair.
///
/// We store *inclusive-end* ranges internally in the reader but the cache
/// key always uses *(offset, length)* to avoid ambiguity around the endpoint
/// representation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RangeKey {
    /// Absolute byte offset into the remote archive.
    offset: u64,
    /// Number of bytes in the range.
    length: u64,
}

// ─────────────────────────────────────────────────────────────────────────────
// CachedEntry
// ─────────────────────────────────────────────────────────────────────────────

/// A single cached byte-range together with its synthetic ETag and the
/// monotonically-increasing sequence number at which it was last accessed.
///
/// The sequence number is used only for internal diagnostics; the actual LRU
/// ordering is maintained by [`EtagCache::lru_order`].
struct CachedEntry {
    /// The cached raw bytes for the range `[offset, offset + data.len())`.
    data: Vec<u8>,
    /// Synthetic ETag of the form `"<offset>-<length>"`.
    ///
    /// Stored so that future work can implement `If-None-Match` conditional
    /// GET requests without redesigning the cache schema.
    etag: String,
    /// Logical timestamp of the last access (monotonically increasing).
    last_used_seq: u64,
}

// ─────────────────────────────────────────────────────────────────────────────
// EtagCache
// ─────────────────────────────────────────────────────────────────────────────

/// Bounded LRU byte-range cache keyed by `(offset, length)`.
///
/// Designed for use inside [`HttpPmTilesReader`](crate::http_reader::HttpPmTilesReader)
/// to avoid redundant network round-trips when the same byte range is
/// requested more than once during tile resolution.
///
/// # Capacity
///
/// The maximum number of distinct byte ranges held simultaneously is
/// `max_entries`.  When a new entry would exceed that limit the least-recently
/// used entry is evicted synchronously inside [`insert`](Self::insert).
///
/// A `max_entries` of `0` permanently disables the cache: every
/// [`get`](Self::get) returns `None` and every [`insert`](Self::insert) is a
/// no-op.
///
/// # ETag storage
///
/// Each entry carries a synthetic ETag string (`"<offset>-<length>"`).
/// PMTiles archives are immutable, so the ETag is a stable identifier for the
/// range rather than a server-provided revalidation token.  The field is
/// retained in the struct to support a future `If-None-Match` HTTP header
/// path without breaking the public API.
///
/// # Examples
///
/// ```
/// # #[cfg(feature = "http-range")]
/// # {
/// use oxigeo_pmtiles::EtagCache;
///
/// let mut cache = EtagCache::new(4);
///
/// // Insert a byte range.
/// cache.insert(0, 127, "0-127".into(), vec![0u8; 127]);
/// assert_eq!(cache.len(), 1);
///
/// // Retrieve it — returns (data_clone, etag_clone).
/// let (data, etag) = cache.get(0, 127).unwrap();
/// assert_eq!(data.len(), 127);
/// assert_eq!(etag, "0-127");
/// # }
/// ```
pub struct EtagCache {
    /// Primary storage: `RangeKey → CachedEntry`.
    entries: HashMap<RangeKey, CachedEntry>,
    /// LRU ordering: front = least-recently-used, back = most-recently-used.
    lru_order: VecDeque<RangeKey>,
    /// Maximum number of entries before eviction kicks in.
    max_entries: usize,
    /// Monotonically increasing sequence number for last-access bookkeeping.
    seq: u64,
}

impl EtagCache {
    // ── Construction ─────────────────────────────────────────────────────────

    /// Create a new `EtagCache` with the given capacity.
    ///
    /// When `max_entries == 0` the cache is permanently disabled (every
    /// [`get`] returns `None`, every [`insert`] is a no-op).
    ///
    /// [`get`]: Self::get
    /// [`insert`]: Self::insert
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: HashMap::with_capacity(max_entries.min(256)),
            lru_order: VecDeque::with_capacity(max_entries.min(256)),
            max_entries,
            seq: 0,
        }
    }

    // ── Core cache operations ─────────────────────────────────────────────────

    /// Look up a cached byte range by `(offset, length)`.
    ///
    /// On a cache hit, returns `Some((data_clone, etag_clone))` and bumps the
    /// accessed entry to the most-recently-used position in the LRU order.
    ///
    /// Returns `None` on a miss *or* when the cache has zero capacity.
    pub fn get(&mut self, offset: u64, length: u64) -> Option<(Vec<u8>, String)> {
        if self.max_entries == 0 {
            return None;
        }

        let key = RangeKey { offset, length };

        // Advance the global sequence number to stamp this access.
        self.seq = self.seq.saturating_add(1);
        let current_seq = self.seq;

        let entry = self.entries.get_mut(&key)?;
        entry.last_used_seq = current_seq;

        // Clone the payload so we can return owned values to the caller while
        // separately updating the LRU deque.
        let data = entry.data.clone();
        let etag = entry.etag.clone();

        // Move key to back (most-recently-used).
        self.move_to_back(&key);

        Some((data, etag))
    }

    /// Look up a cached byte range without modifying LRU order.
    ///
    /// Returns a reference to the cached bytes on a hit, `None` on a miss.
    /// Unlike [`get`](Self::get) this does **not** bump the LRU position or
    /// update the last-used sequence number.
    pub fn peek(&self, offset: u64, length: u64) -> Option<&[u8]> {
        if self.max_entries == 0 {
            return None;
        }
        let key = RangeKey { offset, length };
        self.entries.get(&key).map(|e| e.data.as_slice())
    }

    /// Insert a new entry into the cache.
    ///
    /// If the cache is already at capacity, the least-recently-used entry is
    /// evicted *before* the new entry is inserted.  If `max_entries == 0` the
    /// call is a no-op.
    ///
    /// If an entry already exists for `(offset, length)` it is replaced and
    /// moved to the most-recently-used position.
    pub fn insert(&mut self, offset: u64, length: u64, etag: String, data: Vec<u8>) {
        if self.max_entries == 0 {
            return;
        }

        let key = RangeKey { offset, length };
        self.seq = self.seq.saturating_add(1);
        let current_seq = self.seq;

        if self.entries.contains_key(&key) {
            // Update existing entry in-place and refresh LRU position.
            if let Some(entry) = self.entries.get_mut(&key) {
                entry.data = data;
                entry.etag = etag;
                entry.last_used_seq = current_seq;
            }
            self.move_to_back(&key);
            return;
        }

        // Evict if at capacity.
        if self.entries.len() >= self.max_entries {
            self.evict_lru();
        }

        // Insert the new entry.
        self.entries.insert(
            key.clone(),
            CachedEntry {
                data,
                etag,
                last_used_seq: current_seq,
            },
        );
        self.lru_order.push_back(key);
    }

    // ── Maintenance ───────────────────────────────────────────────────────────

    /// Clear all cached entries, resetting the cache to its empty state.
    ///
    /// The `max_entries` capacity is unchanged.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.lru_order.clear();
        // Intentionally do NOT reset `seq` — it is a monotonic counter used
        // only for diagnostic purposes and resetting it could cause stale
        // timestamps to compare equal to fresh ones after a clear+refill.
    }

    /// Returns the number of distinct byte ranges currently held in the cache.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if the cache holds no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns the maximum number of entries this cache can hold before
    /// eviction begins.
    pub fn capacity(&self) -> usize {
        self.max_entries
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    /// Evict the least-recently-used entry from both `entries` and `lru_order`.
    ///
    /// Pops from the *front* of `lru_order` (oldest access) and removes the
    /// corresponding entry from `entries`.  If `lru_order` is empty (should
    /// not happen in well-formed usage) the method is a safe no-op.
    fn evict_lru(&mut self) {
        if let Some(lru_key) = self.lru_order.pop_front() {
            self.entries.remove(&lru_key);
        }
    }

    /// Move `key` to the back of `lru_order` (most-recently-used position).
    ///
    /// This is an O(n) scan of the deque.  For the expected cache sizes
    /// (≤ a few hundred entries) this is acceptable.  A more sophisticated
    /// implementation would use a doubly-linked list with O(1) removal, but
    /// that would require `unsafe` or an external crate.
    fn move_to_back(&mut self, key: &RangeKey) {
        if let Some(pos) = self.lru_order.iter().position(|k| k == key) {
            self.lru_order.remove(pos);
            self.lru_order.push_back(key.clone());
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Debug impl
// ─────────────────────────────────────────────────────────────────────────────

impl std::fmt::Debug for EtagCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EtagCache")
            .field("len", &self.entries.len())
            .field("max_entries", &self.max_entries)
            .field("seq", &self.seq)
            .finish()
    }
}
