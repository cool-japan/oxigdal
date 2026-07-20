//! CPU-only integration tests for the compute-pipeline cache.
//!
//! These tests exercise the pure-Rust logic in `pipeline_cache` — hashing,
//! key equality, cache bookkeeping, etc. — without requiring a real GPU
//! device.  All `wgpu::ComputePipeline` interactions that would need a device
//! are tested indirectly by verifying that the factory closure is called (or
//! not) and by observing `cache.len()`.

// Permit the test-internal `unwrap()` idiom and relax doc requirements.
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use oxigdal_gpu::pipeline_cache::{
    PipelineCache, PipelineCacheKey, fnv1a_64, new_shared_pipeline_cache,
};
use std::sync::Arc;

// ─────────────────────────────────────────────────────────────────────────────
// FNV-1a hash tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_fnv1a_hash_different_strings_differ() {
    let h_hello = fnv1a_64(b"hello");
    let h_world = fnv1a_64(b"world");
    assert_ne!(
        h_hello, h_world,
        "distinct strings must produce distinct FNV-1a hashes"
    );
}

#[test]
fn test_fnv1a_hash_same_string_stable() {
    let data = b"reproducible test string";
    let first = fnv1a_64(data);
    let second = fnv1a_64(data);
    assert_eq!(first, second, "FNV-1a must be deterministic");
}

#[test]
fn test_fnv1a_hash_empty_slice() {
    // Hashing an empty slice must return the FNV-1a 64-bit offset basis.
    let expected: u64 = 0xcbf2_9ce4_8422_2325;
    assert_eq!(fnv1a_64(b""), expected);
}

#[test]
fn test_fnv1a_hash_single_byte_sensitivity() {
    assert_ne!(fnv1a_64(b"a"), fnv1a_64(b"b"));
}

#[test]
fn test_fnv1a_hash_prefix_sensitivity() {
    // Hashing "abc" and "abcd" must yield different results.
    assert_ne!(fnv1a_64(b"abc"), fnv1a_64(b"abcd"));
}

#[test]
fn test_fnv1a_hash_unicode_stability() {
    // UTF-8 encoded strings should hash consistently byte-by-byte.
    let src = "привет мир"; // "hello world" in Russian
    let h1 = fnv1a_64(src.as_bytes());
    let h2 = fnv1a_64(src.as_bytes());
    assert_eq!(h1, h2);
}

// ─────────────────────────────────────────────────────────────────────────────
// PipelineCacheKey tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_pipeline_cache_key_equality() {
    let k1 = PipelineCacheKey::new("shader source", "main", "r-w");
    let k2 = PipelineCacheKey::new("shader source", "main", "r-w");
    assert_eq!(k1, k2, "identical arguments must produce equal keys");
}

#[test]
fn test_pipeline_cache_key_different_source() {
    let k1 = PipelineCacheKey::new("shader_a", "main", "r-w");
    let k2 = PipelineCacheKey::new("shader_b", "main", "r-w");
    assert_ne!(k1, k2, "different source text must yield different keys");
}

#[test]
fn test_pipeline_cache_key_different_entry_point() {
    let k1 = PipelineCacheKey::new("src", "entry_alpha", "r-w");
    let k2 = PipelineCacheKey::new("src", "entry_beta", "r-w");
    assert_ne!(k1, k2);
}

#[test]
fn test_pipeline_cache_key_different_layout_tag() {
    let k1 = PipelineCacheKey::new("src", "main", "r-w");
    let k2 = PipelineCacheKey::new("src", "main", "r-r-w");
    assert_ne!(k1, k2);
}

#[test]
fn test_pipeline_cache_key_new_hashes_source() {
    let src = "// wgsl source for hashing test";
    let key = PipelineCacheKey::new(src, "cs_main", "r-w");
    assert_eq!(
        key.shader_hash,
        fnv1a_64(src.as_bytes()),
        "key.shader_hash must equal fnv1a_64 of the source bytes"
    );
}

#[test]
fn test_pipeline_cache_key_clone_is_equal() {
    let key = PipelineCacheKey::new("src", "main", "r-w");
    assert_eq!(key.clone(), key);
}

#[test]
fn test_pipeline_cache_key_from_hash_constructor() {
    let hash: u64 = 0xdead_beef_cafe_babe;
    let key = PipelineCacheKey::from_hash(hash, "cs", "u-r-w");
    assert_eq!(key.shader_hash, hash);
    assert_eq!(key.entry_point, "cs");
    assert_eq!(key.layout_tag, "u-r-w");
}

#[test]
fn test_pipeline_cache_key_display_contains_components() {
    let key = PipelineCacheKey::from_hash(0x0123_4567_89ab_cdef, "cs_main", "r-w");
    let s = format!("{}", key);
    assert!(
        s.contains("0123456789abcdef"),
        "display must contain hex hash"
    );
    assert!(s.contains("cs_main"), "display must contain entry point");
    assert!(s.contains("r-w"), "display must contain layout tag");
}

// ─────────────────────────────────────────────────────────────────────────────
// PipelineCache bookkeeping tests (no GPU required)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_pipeline_cache_len_empty() {
    let cache = PipelineCache::new();
    assert_eq!(cache.len(), 0, "new cache must report length 0");
    assert!(cache.is_empty(), "new cache must be empty");
}

#[test]
fn test_pipeline_cache_clear_empties() {
    // We cannot insert a real wgpu::ComputePipeline without a GPU, so we
    // verify clear() on an already-empty cache is a no-op, and that the
    // invariant holds.
    let mut cache = PipelineCache::new();
    cache.clear();
    assert!(cache.is_empty(), "clear on empty cache must leave it empty");
    assert_eq!(cache.len(), 0);
}

#[test]
fn test_pipeline_cache_evict_absent_key_returns_false() {
    let mut cache = PipelineCache::new();
    let key = PipelineCacheKey::new("src", "main", "r-w");
    let removed = cache.evict(&key);
    assert!(!removed, "evict on absent key must return false");
    assert_eq!(cache.len(), 0);
}

#[test]
fn test_pipeline_cache_evict_specific_key() {
    // Without a GPU we can only probe the empty-cache path, but we can also
    // verify that a failed-factory run does not populate the cache and that
    // evict correctly returns false for a key that was never inserted.
    let mut cache = PipelineCache::new();
    let key_a = PipelineCacheKey::new("src_a", "main", "r-w");
    let key_b = PipelineCacheKey::new("src_b", "main", "r-w");

    // Attempt inserts that will fail (no GPU).
    let _: Result<Arc<wgpu::ComputePipeline>, &str> =
        cache.get_or_insert_with(key_a.clone(), || Err("no gpu"));
    let _: Result<Arc<wgpu::ComputePipeline>, &str> =
        cache.get_or_insert_with(key_b.clone(), || Err("no gpu"));

    // Nothing should have been stored.
    assert_eq!(cache.len(), 0);
    assert!(!cache.evict(&key_a));
    assert_eq!(cache.len(), 0);
}

#[test]
fn test_pipeline_cache_error_factory_not_cached() {
    let mut cache = PipelineCache::new();
    let key = PipelineCacheKey::new("shader", "ep", "r-r-w");

    let result: Result<Arc<wgpu::ComputePipeline>, String> =
        cache.get_or_insert_with(key.clone(), || Err("compilation failed".to_owned()));

    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "compilation failed");
    assert_eq!(cache.len(), 0, "failed factory must not pollute the cache");
}

#[test]
fn test_pipeline_cache_factory_called_on_miss() {
    let mut cache = PipelineCache::new();
    let key = PipelineCacheKey::new("src", "main", "r-w");

    let mut call_count = 0u32;
    let _: Result<Arc<wgpu::ComputePipeline>, &str> = cache.get_or_insert_with(key.clone(), || {
        call_count += 1;
        Err("no gpu")
    });

    assert_eq!(
        call_count, 1,
        "factory must be invoked exactly once on miss"
    );
}

#[test]
fn test_pipeline_cache_retain_noop_on_empty() {
    let mut cache = PipelineCache::new();
    // retain with a predicate that rejects everything should be fine on empty.
    cache.retain(|_| false);
    assert!(cache.is_empty());
}

#[test]
fn test_pipeline_cache_keys_iter_on_empty() {
    let cache = PipelineCache::new();
    let count = cache.keys().count();
    assert_eq!(count, 0);
}

// ─────────────────────────────────────────────────────────────────────────────
// SharedPipelineCache tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_shared_pipeline_cache_is_empty_on_creation() {
    let shared = new_shared_pipeline_cache();
    let guard = shared.lock().unwrap();
    assert!(guard.is_empty());
    assert_eq!(guard.len(), 0);
}

#[test]
fn test_shared_pipeline_cache_arc_ptr_equality() {
    let cache = new_shared_pipeline_cache();
    let cache2 = Arc::clone(&cache);
    assert!(
        Arc::ptr_eq(&cache, &cache2),
        "clone must share the same allocation"
    );
}

#[test]
fn test_shared_pipeline_cache_lock_and_evict() {
    let shared = new_shared_pipeline_cache();
    {
        let mut guard = shared.lock().unwrap();
        let key = PipelineCacheKey::new("src", "main", "r-w");
        // Evict a key that was never inserted — must not panic.
        assert!(!guard.evict(&key));
    }
    // Lock is released; a second lock must succeed.
    let guard = shared.lock().unwrap();
    assert!(guard.is_empty());
}

#[test]
fn test_shared_pipeline_cache_clear_via_lock() {
    let shared = new_shared_pipeline_cache();
    {
        let mut guard = shared.lock().unwrap();
        guard.clear();
    }
    let guard = shared.lock().unwrap();
    assert!(guard.is_empty());
}

// ─────────────────────────────────────────────────────────────────────────────
// GpuContext pipeline_cache accessor (compile-time check)
// ─────────────────────────────────────────────────────────────────────────────

/// Verify that `GpuContext::pipeline_cache()` returns the expected type.
///
/// This is a compile-time type check; no GPU is required because the function
/// body is never called at runtime in this test — only its signature is
/// exercised by the type-checker.
#[allow(dead_code)]
fn _assert_gpu_context_pipeline_cache_accessor_type(
    ctx: &oxigdal_gpu::GpuContext,
) -> &oxigdal_gpu::SharedPipelineCache {
    ctx.pipeline_cache()
}
