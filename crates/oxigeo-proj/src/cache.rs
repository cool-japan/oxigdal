//! Thread-safe LRU cache for [`Transformer`] instances keyed by
//! `(src_epsg, dst_epsg)`.
//!
//! Building a [`Transformer`] from EPSG codes incurs non-trivial cost:
//! both source and target [`crate::crs::Crs`] are constructed from the
//! embedded EPSG database, each is serialised to a PROJ string, and the
//! underlying `proj4rs::Proj` is initialised twice (source + target).  For
//! workloads that repeatedly transform between the same EPSG pair —
//! tile pipelines, vector reprojection batches, geocoders — this cost
//! dominates.
//!
//! [`TransformerCache`] provides an opt-in, thread-safe cache with a
//! bounded number of entries and LRU eviction.  Internally it stores
//! `Arc<Transformer>` so callers may freely clone, share across threads,
//! or hold the handle for the lifetime of a single transform operation.
//!
//! # Concurrency model
//!
//! All shared state lives behind a single [`std::sync::Mutex`].  The
//! cache is designed for short critical sections:
//!
//! * **Hit path** locks the mutex, looks up the key, updates the MRU
//!   order, and returns a cheap `Arc` clone.
//! * **Miss path** drops the lock before invoking
//!   [`Transformer::from_epsg`] (which may perform allocation, PROJ
//!   string parsing, and possibly trigger I/O when geoid grids are
//!   later wired in), then re-acquires the lock to insert.  A second
//!   thread that races in and inserts the same key first wins; the
//!   loser's freshly-built `Transformer` is dropped and the cached
//!   instance is returned.  This keeps the cache strictly single-copy.
//!
//! # Eviction
//!
//! Eviction is least-recently-used: every successful `get_or_build`
//! call promotes the key to the front of the order queue, and inserts
//! beyond `capacity` evict from the back.  `capacity` is clamped to at
//! least 1 to keep the invariant "the most recently inserted entry is
//! always present" trivially true.
//!
//! # Example
//!
//! ```no_run
//! use oxigeo_proj::TransformerCache;
//!
//! let cache = TransformerCache::new(16);
//! let wgs84_to_web = cache
//!     .get_or_build(4326, 3857)
//!     .expect("EPSG:4326→3857 must build");
//!
//! // Subsequent calls return the same Arc (no rebuild):
//! let again = cache
//!     .get_or_build(4326, 3857)
//!     .expect("cached");
//! assert!(std::sync::Arc::ptr_eq(&wgs84_to_web, &again));
//! ```

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::error::Error;
use crate::transform::Transformer;

/// Acquires the inner mutex, transparently recovering from poisoning.
///
/// A poisoned lock means a previous holder panicked while mutating the
/// cache state.  Because the cache is purely a memoisation layer (its
/// state can be safely re-derived), continuing with the inner data is
/// always sound — there is no invariant that a panic could have
/// half-violated.  Returning the inner guard avoids both `expect`/
/// `unwrap` (forbidden by clippy lints) and surprise panics in user
/// code that simply needs to read a few cache stats.
fn lock_inner(mutex: &Mutex<LruInner>) -> MutexGuard<'_, LruInner> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Composite cache key identifying a `(src_epsg, dst_epsg)` transformer
/// pair.
///
/// The struct is `Copy` + `Eq` + `Hash` so it can be used directly in a
/// [`HashMap`] and trivially moved through the MRU queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TransformerKey {
    /// Source EPSG code (e.g. `4326` for WGS84).
    pub src_epsg: u32,
    /// Destination EPSG code (e.g. `3857` for Web Mercator).
    pub dst_epsg: u32,
}

impl TransformerKey {
    /// Convenience constructor.
    pub fn new(src_epsg: u32, dst_epsg: u32) -> Self {
        Self { src_epsg, dst_epsg }
    }
}

/// Inner LRU state guarded by [`TransformerCache`]'s mutex.
///
/// `map` owns the cached `Arc<Transformer>` instances; `order` records
/// the MRU sequence, with the **front** being most-recently-used and
/// the **back** being the next eviction candidate.  Both collections
/// are kept consistent: every key in `map` appears exactly once in
/// `order` and vice versa.
struct LruInner {
    /// Owning storage of cached transformer Arcs.
    map: HashMap<TransformerKey, Arc<Transformer>>,
    /// MRU-first key order.  `front` = newest, `back` = oldest.
    order: VecDeque<TransformerKey>,
}

impl LruInner {
    /// Promotes `key` to the front of `order`.  Assumes `key` already
    /// exists in the queue (it is removed, then re-pushed at the front).
    /// If the key is missing, this is a no-op (caller bug).
    fn touch(&mut self, key: &TransformerKey) {
        if let Some(pos) = self.order.iter().position(|k| k == key) {
            self.order.remove(pos);
        }
        self.order.push_front(*key);
    }
}

/// Thread-safe LRU cache for [`Transformer`] instances.
///
/// `TransformerCache` is cheap to clone — internally a single
/// `Arc<Mutex<…>>` is shared.  All clones see the same cache; entries
/// inserted through one handle are visible through every other.
///
/// Lookups, insertions, and eviction all run under a single mutex.
/// The mutex is **dropped while a fresh transformer is being built**,
/// so a single slow build does not block parallel hits for other
/// EPSG pairs.
#[derive(Clone)]
pub struct TransformerCache {
    inner: Arc<Mutex<LruInner>>,
    capacity: usize,
}

impl TransformerCache {
    /// Constructs a new cache with at most `capacity` cached entries.
    ///
    /// `capacity` is clamped to a minimum of 1: a zero-capacity cache
    /// would be useless (every call would have to rebuild from
    /// scratch), so the clamp gracefully handles invalid input rather
    /// than panicking.
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            inner: Arc::new(Mutex::new(LruInner {
                map: HashMap::with_capacity(capacity),
                order: VecDeque::with_capacity(capacity),
            })),
            capacity,
        }
    }

    /// Returns the effective capacity (post-clamp).
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Returns the number of entries currently cached.
    pub fn len(&self) -> usize {
        let guard = lock_inner(&self.inner);
        guard.map.len()
    }

    /// Returns `true` when the cache holds no entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns `true` when the given EPSG pair is currently cached.
    ///
    /// This does **not** promote the key in the MRU order — it is a
    /// pure read query intended for introspection / metrics / tests.
    pub fn contains(&self, src_epsg: u32, dst_epsg: u32) -> bool {
        let guard = lock_inner(&self.inner);
        guard
            .map
            .contains_key(&TransformerKey::new(src_epsg, dst_epsg))
    }

    /// Removes all cached entries.  Capacity is preserved.
    pub fn clear(&self) {
        let mut guard = lock_inner(&self.inner);
        guard.map.clear();
        guard.order.clear();
    }

    /// Returns a snapshot of cached keys in MRU order
    /// (front = newest, back = oldest).
    ///
    /// Intended for introspection and tests: the cache is locked only
    /// briefly to copy the queue, then released before the snapshot is
    /// returned, so the result is a point-in-time view that may go
    /// stale immediately.
    pub fn cached_keys(&self) -> Vec<TransformerKey> {
        let guard = lock_inner(&self.inner);
        guard.order.iter().copied().collect()
    }

    /// Returns a cached transformer for the `(src_epsg, dst_epsg)`
    /// pair, building and inserting one on miss.
    ///
    /// # Behaviour
    ///
    /// * **Hit** — the cached `Arc` is cloned and returned; the key is
    ///   promoted to the front of the MRU order.
    /// * **Miss** — the cache mutex is released, a fresh
    ///   [`Transformer::from_epsg`] is constructed, then the mutex is
    ///   re-acquired and the entry is inserted.  If another thread
    ///   raced in and inserted first, the freshly built transformer is
    ///   dropped and the already-cached `Arc` is returned, preserving
    ///   single-copy semantics.
    /// * **Eviction** — when at `capacity`, the back of the MRU queue
    ///   is removed before the new entry is inserted.
    ///
    /// # Errors
    ///
    /// Propagates any error from [`Transformer::from_epsg`] — typically
    /// [`Error::EpsgCodeNotFound`] for unknown codes, or
    /// [`Error::ProjectionInitError`] when the underlying `proj4rs`
    /// initialisation fails.
    pub fn get_or_build(&self, src_epsg: u32, dst_epsg: u32) -> Result<Arc<Transformer>, Error> {
        let key = TransformerKey::new(src_epsg, dst_epsg);

        // Fast path: hit.  Take the lock, look up the Arc, promote the
        // key to MRU front, and return.  No transformer construction
        // happens while the lock is held in this path.
        {
            let mut guard = lock_inner(&self.inner);
            if let Some(arc) = guard.map.get(&key).cloned() {
                guard.touch(&key);
                return Ok(arc);
            }
        }

        // Slow path: miss.  Build outside the lock so other threads
        // hitting unrelated keys are not blocked while proj4rs parses
        // PROJ strings and initialises projection objects.
        let transformer = Arc::new(Transformer::from_epsg(src_epsg, dst_epsg)?);

        // Insert under lock.  Another thread may have raced in and
        // inserted the same key while we were building — re-check
        // before committing our own instance so the cache never
        // contains two distinct Arcs for the same key.
        let mut guard = lock_inner(&self.inner);
        if let Some(arc) = guard.map.get(&key).cloned() {
            // Lost the race; promote MRU and discard our freshly built
            // transformer.  Returning the existing Arc preserves the
            // single-copy invariant — every concurrent caller for the
            // same key observes the **same** Transformer instance.
            guard.touch(&key);
            return Ok(arc);
        }

        // Evict until there is room for the new entry.  The loop
        // tolerates a degenerate empty `order` (which would only
        // happen under a programming error elsewhere) by breaking
        // cleanly rather than spinning forever.
        while guard.map.len() >= self.capacity {
            match guard.order.pop_back() {
                Some(oldest) => {
                    guard.map.remove(&oldest);
                }
                None => break,
            }
        }

        guard.map.insert(key, transformer.clone());
        guard.order.push_front(key);
        Ok(transformer)
    }
}

impl std::fmt::Debug for TransformerCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Take a short read-only snapshot to render len without holding
        // the lock across the formatter machinery itself.
        let len = {
            let guard = lock_inner(&self.inner);
            guard.map.len()
        };
        f.debug_struct("TransformerCache")
            .field("len", &len)
            .field("capacity", &self.capacity)
            .finish()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_key_equality_and_hash() {
        let k1 = TransformerKey::new(4326, 3857);
        let k2 = TransformerKey::new(4326, 3857);
        let k3 = TransformerKey::new(3857, 4326);
        assert_eq!(k1, k2);
        assert_ne!(k1, k3);

        // Verify hash-equality contract is satisfied for HashMap usage.
        let mut map: HashMap<TransformerKey, i32> = HashMap::new();
        map.insert(k1, 42);
        assert_eq!(map.get(&k2), Some(&42));
        assert_eq!(map.get(&k3), None);
    }

    #[test]
    fn test_capacity_clamp() {
        let c0 = TransformerCache::new(0);
        let c5 = TransformerCache::new(5);
        assert_eq!(c0.capacity(), 1);
        assert_eq!(c5.capacity(), 5);
    }

    #[test]
    fn test_debug_format() {
        let cache = TransformerCache::new(4);
        let s = format!("{:?}", cache);
        assert!(s.contains("TransformerCache"));
        assert!(s.contains("len"));
        assert!(s.contains("capacity"));
    }
}
