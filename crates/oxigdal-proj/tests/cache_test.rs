//! Integration tests for [`oxigdal_proj::TransformerCache`].
//!
//! These tests cover the public contract of the LRU cache:
//!
//! * insertion / hit semantics
//! * `Arc` single-copy invariant (hits and concurrent inserts return
//!   the **same** `Arc` instance, not just equivalent transformers)
//! * MRU promotion on hit
//! * eviction order at capacity
//! * capacity clamping at construction
//! * `clear` empties the cache
//! * `contains` reflects insertion state
//! * `cached_keys` returns MRU-first snapshot
//! * thread-safety: many concurrent threads requesting the same key
//!   collapse to a single cached instance

#![allow(clippy::expect_used)]

use std::sync::Arc;
use std::thread;

use oxigdal_proj::{TransformerCache, TransformerKey};

/// EPSG codes that all exist in the embedded registry; combinations
/// are used to stress the cache without hitting unknown-code errors.
const WGS84: u32 = 4326;
const WEB_MERC: u32 = 3857;
const UTM33N: u32 = 32633;
const BNG: u32 = 27700;

#[test]
fn test_cache_empty_starts_with_zero_len() {
    let cache = TransformerCache::new(8);
    assert_eq!(cache.len(), 0);
    assert!(cache.is_empty());
    assert!(cache.cached_keys().is_empty());
}

#[test]
fn test_cache_get_or_build_inserts_on_miss() {
    let cache = TransformerCache::new(4);
    let _t = cache
        .get_or_build(WGS84, WEB_MERC)
        .expect("EPSG:4326→3857 must build");
    assert_eq!(cache.len(), 1);
    assert!(!cache.is_empty());
    assert!(cache.contains(WGS84, WEB_MERC));
}

#[test]
fn test_cache_get_or_build_returns_same_arc_on_hit() {
    let cache = TransformerCache::new(4);
    let first = cache.get_or_build(WGS84, WEB_MERC).expect("first build");
    let second = cache.get_or_build(WGS84, WEB_MERC).expect("cached hit");
    assert!(
        Arc::ptr_eq(&first, &second),
        "second call must return the SAME Arc, not just an equivalent one"
    );
    assert_eq!(cache.len(), 1);
}

#[test]
fn test_cache_evicts_oldest_when_full() {
    let cache = TransformerCache::new(2);

    let _a = cache.get_or_build(WGS84, WEB_MERC).expect("A");
    let _b = cache.get_or_build(WGS84, UTM33N).expect("B");
    assert_eq!(cache.len(), 2);
    assert!(cache.contains(WGS84, WEB_MERC));
    assert!(cache.contains(WGS84, UTM33N));

    // Inserting C must evict A (oldest, untouched after B was inserted).
    let _c = cache.get_or_build(WGS84, BNG).expect("C");
    assert_eq!(cache.len(), 2);
    assert!(
        !cache.contains(WGS84, WEB_MERC),
        "A (oldest) must have been evicted"
    );
    assert!(cache.contains(WGS84, UTM33N));
    assert!(cache.contains(WGS84, BNG));
}

#[test]
fn test_cache_hit_moves_key_to_mru() {
    let cache = TransformerCache::new(2);

    let _a = cache.get_or_build(WGS84, WEB_MERC).expect("A");
    let _b = cache.get_or_build(WGS84, UTM33N).expect("B");

    // Hit on A — must promote A to MRU, leaving B as the oldest.
    let _a2 = cache.get_or_build(WGS84, WEB_MERC).expect("A again");

    // Inserting C should now evict B (oldest), not A.
    let _c = cache.get_or_build(WGS84, BNG).expect("C");

    assert!(
        cache.contains(WGS84, WEB_MERC),
        "A must remain after being promoted via hit"
    );
    assert!(
        !cache.contains(WGS84, UTM33N),
        "B must be evicted as it became the oldest after A's hit"
    );
    assert!(cache.contains(WGS84, BNG));
}

#[test]
fn test_cache_capacity_clamped_to_at_least_one() {
    let cache = TransformerCache::new(0);
    assert_eq!(cache.capacity(), 1, "capacity(0) must clamp to 1");
    let _t = cache.get_or_build(WGS84, WEB_MERC).expect("insertion ok");
    assert_eq!(cache.len(), 1);
}

#[test]
fn test_cache_clear_empties() {
    let cache = TransformerCache::new(4);
    let _ = cache.get_or_build(WGS84, WEB_MERC).expect("A");
    let _ = cache.get_or_build(WGS84, UTM33N).expect("B");
    assert_eq!(cache.len(), 2);

    cache.clear();
    assert_eq!(cache.len(), 0);
    assert!(cache.is_empty());
    assert!(!cache.contains(WGS84, WEB_MERC));
    assert!(!cache.contains(WGS84, UTM33N));
    assert!(cache.cached_keys().is_empty());
}

#[test]
fn test_cache_contains_after_insert() {
    let cache = TransformerCache::new(4);
    assert!(!cache.contains(WGS84, WEB_MERC));
    let _ = cache.get_or_build(WGS84, WEB_MERC).expect("insert");
    assert!(cache.contains(WGS84, WEB_MERC));
    assert!(
        !cache.contains(WEB_MERC, WGS84),
        "reverse direction is a distinct key"
    );
}

#[test]
fn test_cache_thread_safe_concurrent_inserts() {
    // Many concurrent threads requesting the same (src, dst) pair must
    // all observe the same cached Arc — never two separate Transformer
    // allocations for one key.
    let cache = TransformerCache::new(4);
    let mut handles = Vec::with_capacity(8);
    for _ in 0..8 {
        let cache = cache.clone();
        handles.push(thread::spawn(move || {
            cache
                .get_or_build(WGS84, WEB_MERC)
                .expect("concurrent build must succeed")
        }));
    }

    let arcs: Vec<_> = handles
        .into_iter()
        .map(|h| h.join().expect("thread panicked"))
        .collect();

    assert_eq!(arcs.len(), 8);
    let reference = &arcs[0];
    for (idx, arc) in arcs.iter().enumerate().skip(1) {
        assert!(
            Arc::ptr_eq(reference, arc),
            "thread {idx} returned a different Arc — race lost single-copy invariant"
        );
    }

    // Cache should hold exactly one entry — the contended key.
    assert_eq!(cache.len(), 1);
    assert!(cache.contains(WGS84, WEB_MERC));
}

#[test]
fn test_cache_cached_keys_in_mru_order() {
    let cache = TransformerCache::new(4);

    let _ = cache.get_or_build(WGS84, WEB_MERC).expect("A");
    let _ = cache.get_or_build(WGS84, UTM33N).expect("B");
    let _ = cache.get_or_build(WGS84, BNG).expect("C");

    // After A, B, C inserted in order, MRU sequence is [C, B, A].
    let keys = cache.cached_keys();
    assert_eq!(keys.len(), 3);
    assert_eq!(keys[0], TransformerKey::new(WGS84, BNG));
    assert_eq!(keys[1], TransformerKey::new(WGS84, UTM33N));
    assert_eq!(keys[2], TransformerKey::new(WGS84, WEB_MERC));

    // Hitting A promotes it to the front.
    let _ = cache.get_or_build(WGS84, WEB_MERC).expect("A hit");
    let keys = cache.cached_keys();
    assert_eq!(keys.len(), 3);
    assert_eq!(keys[0], TransformerKey::new(WGS84, WEB_MERC));
    assert_eq!(keys[1], TransformerKey::new(WGS84, BNG));
    assert_eq!(keys[2], TransformerKey::new(WGS84, UTM33N));
}

#[test]
fn test_cache_propagates_build_errors() {
    // Unknown EPSG codes must surface as errors, not poison the cache.
    let cache = TransformerCache::new(4);
    let result = cache.get_or_build(999_999, 999_998);
    assert!(result.is_err(), "unknown EPSG must error");
    assert_eq!(
        cache.len(),
        0,
        "failed build must not leave a partial entry"
    );
}

#[test]
fn test_cache_clone_shares_state() {
    // Cloning the cache must share the underlying storage — entries
    // inserted via one handle must be visible through the other.
    let cache = TransformerCache::new(4);
    let twin = cache.clone();

    let _ = cache
        .get_or_build(WGS84, WEB_MERC)
        .expect("insert via primary");
    assert_eq!(twin.len(), 1, "twin must see entry inserted via primary");
    assert!(twin.contains(WGS84, WEB_MERC));

    let from_primary = cache
        .get_or_build(WGS84, WEB_MERC)
        .expect("hit via primary");
    let from_twin = twin.get_or_build(WGS84, WEB_MERC).expect("hit via twin");
    assert!(
        Arc::ptr_eq(&from_primary, &from_twin),
        "both handles must yield the same Arc"
    );
}
