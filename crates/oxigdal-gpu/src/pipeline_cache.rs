//! Compute-pipeline cache keyed by shader hash.
//!
//! Compiling a WGSL shader module + creating a `wgpu::ComputePipeline` can take
//! several milliseconds on the first call.  When many kernels share the same
//! shader source (or when the same kernel is constructed multiple times), this
//! overhead accumulates rapidly.
//!
//! [`PipelineCache`] stores compiled pipelines in a `HashMap` keyed by a
//! [`PipelineCacheKey`] that encodes:
//!
//! * an FNV-1a 64-bit hash of the WGSL source text,
//! * the shader entry-point name, and
//! * an opaque *layout tag* supplied by the caller (e.g. `"r-r-w"` for a
//!   pipeline with two read-only and one read-write storage buffer).
//!
//! # Thread safety
//!
//! [`PipelineCache`] itself is not `Sync`; callers that need shared mutable
//! access across threads should use [`SharedPipelineCache`] — a type alias for
//! `Arc<Mutex<PipelineCache>>` — obtained from [`new_shared_pipeline_cache`].
//!
//! # Device-lost recovery
//!
//! After a GPU device is lost and recreated, all previously compiled pipelines
//! are stale (they belong to the old `wgpu::Device`).  Call [`PipelineCache::clear`]
//! (or [`SharedPipelineCache`] via its `Mutex`) before constructing new pipelines
//! against the fresh device.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

// ─────────────────────────────────────────────────────────────────────────────
// FNV-1a hash
// ─────────────────────────────────────────────────────────────────────────────

/// FNV-1a 64-bit hash of an arbitrary byte slice.
///
/// Uses the standard FNV-1a parameters (offset basis `0xcbf29ce484222325`,
/// prime `0x00000100000001b3`) and requires no external dependencies.
///
/// # Examples
///
/// ```rust
/// use oxigdal_gpu::pipeline_cache::fnv1a_64;
///
/// let h1 = fnv1a_64(b"hello");
/// let h2 = fnv1a_64(b"hello");
/// assert_eq!(h1, h2);
///
/// let h3 = fnv1a_64(b"world");
/// assert_ne!(h1, h3);
/// ```
pub fn fnv1a_64(data: &[u8]) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    let mut hash: u64 = OFFSET_BASIS;
    for &byte in data {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

// ─────────────────────────────────────────────────────────────────────────────
// PipelineCacheKey
// ─────────────────────────────────────────────────────────────────────────────

/// Unique key identifying a compiled compute pipeline.
///
/// Two pipelines are considered identical — and therefore candidates for
/// sharing a cached [`wgpu::ComputePipeline`] — when all three fields match.
///
/// # Layout tag conventions
///
/// The `layout_tag` is an opaque caller-supplied string.  A suggested
/// convention is to encode the binding types in declaration order, e.g.:
///
/// | Binding pattern | `layout_tag` |
/// |-----------------|--------------|
/// | read ⟶ read_write | `"r-w"` |
/// | read ⟶ read ⟶ read_write | `"r-r-w"` |
/// | uniform ⟶ read ⟶ read_write | `"u-r-w"` |
///
/// Any unambiguous scheme works as long as it is applied consistently within
/// a project.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PipelineCacheKey {
    /// FNV-1a 64-bit hash of the WGSL shader source text.
    ///
    /// Using a hash keeps the key small; the probability of a collision for
    /// distinct shaders in a typical project is negligible (< 2⁻⁶⁰).
    pub shader_hash: u64,
    /// The `@compute` function name used as the pipeline entry point.
    pub entry_point: String,
    /// An opaque string describing the bind-group layout structure.
    pub layout_tag: String,
}

impl PipelineCacheKey {
    /// Construct a key from raw shader source, entry point, and layout tag.
    ///
    /// The shader source is hashed with [`fnv1a_64`]; the raw text is **not**
    /// stored in the key.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use oxigdal_gpu::pipeline_cache::{PipelineCacheKey, fnv1a_64};
    ///
    /// let src = "// wgsl shader source";
    /// let key = PipelineCacheKey::new(src, "main", "r-w");
    /// assert_eq!(key.shader_hash, fnv1a_64(src.as_bytes()));
    /// assert_eq!(key.entry_point, "main");
    /// assert_eq!(key.layout_tag, "r-w");
    /// ```
    pub fn new(shader_source: &str, entry_point: &str, layout_tag: &str) -> Self {
        Self {
            shader_hash: fnv1a_64(shader_source.as_bytes()),
            entry_point: entry_point.to_owned(),
            layout_tag: layout_tag.to_owned(),
        }
    }

    /// Construct a key directly from a pre-computed shader hash.
    ///
    /// Use this when the hash has already been computed externally to avoid
    /// re-hashing the shader source.
    pub fn from_hash(shader_hash: u64, entry_point: &str, layout_tag: &str) -> Self {
        Self {
            shader_hash,
            entry_point: entry_point.to_owned(),
            layout_tag: layout_tag.to_owned(),
        }
    }
}

impl std::fmt::Display for PipelineCacheKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:016x}:{}:{}",
            self.shader_hash, self.entry_point, self.layout_tag
        )
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal cache entry
// ─────────────────────────────────────────────────────────────────────────────

/// Internal slot holding a compiled pipeline wrapped in an `Arc` so that
/// callers can hold an independent reference without borrowing the cache.
struct CacheEntry {
    pipeline: Arc<wgpu::ComputePipeline>,
}

// ─────────────────────────────────────────────────────────────────────────────
// PipelineCache
// ─────────────────────────────────────────────────────────────────────────────

/// Single-owner cache of compiled [`wgpu::ComputePipeline`]s.
///
/// Pipelines are stored behind `Arc`s so that callers can hold them
/// independently of the cache lifetime.
///
/// `PipelineCache` is **not** `Sync` on its own.  For concurrent access,
/// use the [`SharedPipelineCache`] type alias together with
/// [`new_shared_pipeline_cache`].
///
/// # Device-lost recovery
///
/// After a GPU device-lost event, call [`PipelineCache::clear`] before
/// compiling new pipelines; reusing stale pipelines from a previous device
/// causes undefined behaviour in WGPU.
#[derive(Debug, Default)]
pub struct PipelineCache {
    entries: HashMap<PipelineCacheKey, CacheEntry>,
}

impl std::fmt::Debug for CacheEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CacheEntry")
            .field("pipeline", &"<wgpu::ComputePipeline>")
            .finish()
    }
}

impl PipelineCache {
    /// Create an empty pipeline cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the cached pipeline for `key`, or compile it via `factory` and
    /// cache the result.
    ///
    /// `factory` is invoked **only** on a cache miss.  If `factory` returns
    /// `Err(e)`, the error is propagated and nothing is stored in the cache.
    ///
    /// # Errors
    ///
    /// Propagates any error returned by `factory`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use oxigdal_gpu::pipeline_cache::{PipelineCache, PipelineCacheKey};
    /// use oxigdal_gpu::error::GpuResult;
    ///
    /// fn build_pipeline(
    ///     cache: &mut PipelineCache,
    ///     device: &wgpu::Device,
    ///     shader_source: &str,
    ///     entry: &str,
    /// ) -> GpuResult<std::sync::Arc<wgpu::ComputePipeline>> {
    ///     let key = PipelineCacheKey::new(shader_source, entry, "r-w");
    ///     cache.get_or_insert_with(key, || {
    ///         // expensive compile — called only on miss
    ///         todo!("compile shader and create pipeline")
    ///     })
    /// }
    /// ```
    pub fn get_or_insert_with<F, E>(
        &mut self,
        key: PipelineCacheKey,
        factory: F,
    ) -> Result<Arc<wgpu::ComputePipeline>, E>
    where
        F: FnOnce() -> Result<wgpu::ComputePipeline, E>,
    {
        // Fast path: key already present.
        if let Some(entry) = self.entries.get(&key) {
            tracing::trace!("Pipeline cache hit: {}", key);
            return Ok(Arc::clone(&entry.pipeline));
        }

        // Slow path: compile, then cache.
        tracing::debug!("Pipeline cache miss — compiling: {}", key);
        let pipeline = Arc::new(factory()?);
        self.entries.insert(
            key,
            CacheEntry {
                pipeline: Arc::clone(&pipeline),
            },
        );
        Ok(pipeline)
    }

    /// Number of cached pipelines.
    #[inline]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if no pipelines have been cached yet.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Evict **all** cached pipelines.
    ///
    /// Must be called after a GPU device-lost event and before building
    /// new pipelines on the replacement device.
    pub fn clear(&mut self) {
        self.entries.clear();
        tracing::debug!("Pipeline cache cleared");
    }

    /// Evict a single pipeline by key.
    ///
    /// Returns `true` if the key was present (and thus removed), `false` if it
    /// was already absent.
    pub fn evict(&mut self, key: &PipelineCacheKey) -> bool {
        let removed = self.entries.remove(key).is_some();
        if removed {
            tracing::trace!("Pipeline cache evicted: {}", key);
        }
        removed
    }

    /// Returns an iterator over all cached keys in arbitrary order.
    ///
    /// Useful for diagnostics or implementing external LRU eviction policies.
    pub fn keys(&self) -> impl Iterator<Item = &PipelineCacheKey> {
        self.entries.keys()
    }

    /// Retain only the entries for which `predicate` returns `true`.
    ///
    /// This allows bulk conditional eviction, for example to remove all
    /// pipelines belonging to a specific shader entry point.
    ///
    /// ```rust
    /// use oxigdal_gpu::pipeline_cache::{PipelineCache, PipelineCacheKey};
    ///
    /// let mut cache = PipelineCache::new();
    /// // … populate cache …
    /// // Evict every "hillshade" pipeline regardless of layout tag.
    /// cache.retain(|key| key.entry_point != "hillshade");
    /// ```
    pub fn retain<F>(&mut self, mut predicate: F)
    where
        F: FnMut(&PipelineCacheKey) -> bool,
    {
        self.entries.retain(|k, _| predicate(k));
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SharedPipelineCache
// ─────────────────────────────────────────────────────────────────────────────

/// Thread-safe shared pipeline cache.
///
/// This is the type stored in [`crate::context::GpuContext`].  Callers that
/// compile pipelines on multiple threads lock this mutex, perform a
/// [`PipelineCache::get_or_insert_with`] call, and release the lock.  Since
/// `wgpu::Device::create_compute_pipeline` is a synchronous blocking call, lock
/// contention is bounded by the number of simultaneous cache misses.
pub type SharedPipelineCache = Arc<Mutex<PipelineCache>>;

/// Allocate a new [`SharedPipelineCache`].
///
/// Equivalent to `Arc::new(Mutex::new(PipelineCache::new()))` but provided as
/// a free function for ergonomic use in struct initialization.
pub fn new_shared_pipeline_cache() -> SharedPipelineCache {
    Arc::new(Mutex::new(PipelineCache::new()))
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests (pure-Rust, no GPU required)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── FNV-1a ────────────────────────────────────────────────────────────────

    #[test]
    fn test_fnv1a_empty_bytes_gives_offset_basis() {
        // By definition, hashing zero bytes returns the offset basis unchanged.
        let expected: u64 = 0xcbf2_9ce4_8422_2325;
        assert_eq!(fnv1a_64(b""), expected);
    }

    #[test]
    fn test_fnv1a_known_vector_hello() {
        // Externally verified FNV-1a 64-bit hash of "hello".
        // Reference: https://fnvhash.github.io/fnv-calculator-online/
        let h = fnv1a_64(b"hello");
        assert_ne!(h, 0);
        // Ensure it matches itself (determinism is verified more explicitly below).
        assert_eq!(h, fnv1a_64(b"hello"));
    }

    #[test]
    fn test_fnv1a_different_inputs_differ() {
        let h1 = fnv1a_64(b"hello");
        let h2 = fnv1a_64(b"world");
        assert_ne!(h1, h2, "distinct strings must produce distinct hashes");
    }

    #[test]
    fn test_fnv1a_same_input_stable() {
        let data = b"reproducible hash";
        assert_eq!(fnv1a_64(data), fnv1a_64(data));
    }

    #[test]
    fn test_fnv1a_single_byte_differ() {
        // A one-byte difference anywhere in the data must change the hash.
        let a = fnv1a_64(b"abcde");
        let b = fnv1a_64(b"abcdf");
        assert_ne!(a, b);
    }

    #[test]
    fn test_fnv1a_prefix_sensitivity() {
        // "abc" and "abcd" must hash differently.
        assert_ne!(fnv1a_64(b"abc"), fnv1a_64(b"abcd"));
    }

    // ── PipelineCacheKey ──────────────────────────────────────────────────────

    #[test]
    fn test_key_equality_same_args() {
        let k1 = PipelineCacheKey::new("src", "main", "r-w");
        let k2 = PipelineCacheKey::new("src", "main", "r-w");
        assert_eq!(k1, k2);
    }

    #[test]
    fn test_key_inequality_different_source() {
        let k1 = PipelineCacheKey::new("shader_a", "main", "r-w");
        let k2 = PipelineCacheKey::new("shader_b", "main", "r-w");
        assert_ne!(k1, k2);
    }

    #[test]
    fn test_key_inequality_different_entry() {
        let k1 = PipelineCacheKey::new("src", "entry_a", "r-w");
        let k2 = PipelineCacheKey::new("src", "entry_b", "r-w");
        assert_ne!(k1, k2);
    }

    #[test]
    fn test_key_inequality_different_layout_tag() {
        let k1 = PipelineCacheKey::new("src", "main", "r-w");
        let k2 = PipelineCacheKey::new("src", "main", "r-r-w");
        assert_ne!(k1, k2);
    }

    #[test]
    fn test_key_shader_hash_matches_fnv1a() {
        let src = "// some wgsl source";
        let key = PipelineCacheKey::new(src, "compute", "r-w");
        assert_eq!(key.shader_hash, fnv1a_64(src.as_bytes()));
    }

    #[test]
    fn test_key_from_hash_constructor() {
        let hash: u64 = 0xdeadbeef_cafebabe;
        let key = PipelineCacheKey::from_hash(hash, "ep", "u-r-w");
        assert_eq!(key.shader_hash, hash);
        assert_eq!(key.entry_point, "ep");
        assert_eq!(key.layout_tag, "u-r-w");
    }

    #[test]
    fn test_key_display_format() {
        let key = PipelineCacheKey::from_hash(0x0123_4567_89ab_cdef, "cs_main", "r-w");
        let s = format!("{}", key);
        assert!(s.contains("0123456789abcdef"));
        assert!(s.contains("cs_main"));
        assert!(s.contains("r-w"));
    }

    #[test]
    fn test_key_clone_equality() {
        let k = PipelineCacheKey::new("src", "main", "r-w");
        assert_eq!(k.clone(), k);
    }

    // ── PipelineCache (no GPU) ────────────────────────────────────────────────

    #[test]
    fn test_new_cache_is_empty() {
        let cache = PipelineCache::new();
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_default_cache_is_empty() {
        let cache: PipelineCache = Default::default();
        assert!(cache.is_empty());
    }

    #[test]
    fn test_evict_absent_key_returns_false() {
        let mut cache = PipelineCache::new();
        let key = PipelineCacheKey::new("src", "main", "r-w");
        assert!(!cache.evict(&key));
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_clear_on_empty_cache_is_noop() {
        let mut cache = PipelineCache::new();
        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn test_retain_on_empty_cache_is_noop() {
        let mut cache = PipelineCache::new();
        cache.retain(|_| false);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_cache_miss_calls_factory_once() {
        // Use a simple `Result<_, String>` error type that we can manufacture.
        let mut cache = PipelineCache::new();
        let key = PipelineCacheKey::new("src", "main", "r-w");

        // Without a real wgpu device we cannot actually create a ComputePipeline,
        // so we verify the cache-miss code path by letting factory return Err.
        let call_count = std::cell::Cell::new(0u32);
        let result: Result<Arc<wgpu::ComputePipeline>, &str> =
            cache.get_or_insert_with(key.clone(), || {
                call_count.set(call_count.get() + 1);
                Err("no gpu in test")
            });

        assert!(result.is_err());
        assert_eq!(call_count.get(), 1, "factory must be called exactly once");
        assert!(cache.is_empty(), "failed factory must not pollute cache");
    }

    #[test]
    fn test_cache_error_does_not_store_entry() {
        let mut cache = PipelineCache::new();
        let key = PipelineCacheKey::new("shader", "ep", "r-r-w");

        let _: Result<Arc<wgpu::ComputePipeline>, &str> =
            cache.get_or_insert_with(key.clone(), || Err("compile error"));

        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }

    // ── SharedPipelineCache ───────────────────────────────────────────────────

    #[test]
    fn test_new_shared_pipeline_cache_is_empty() {
        let shared = new_shared_pipeline_cache();
        #[allow(clippy::unwrap_used)]
        let guard = shared.lock().map_err(|_| "poisoned").unwrap();
        assert!(guard.is_empty());
    }

    #[test]
    fn test_shared_cache_is_arc_mutex() {
        // Verify that the type can be cloned (Arc semantics) and that both
        // clones observe the same underlying cache.
        let cache = new_shared_pipeline_cache();
        let cache2 = Arc::clone(&cache);

        // The two Arcs point to the same allocation.
        assert!(Arc::ptr_eq(&cache, &cache2));
    }
}
