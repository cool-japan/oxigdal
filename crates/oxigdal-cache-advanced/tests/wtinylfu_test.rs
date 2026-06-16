//! Integration tests for W-TinyLFU eviction policy and Count-Min Sketch.

use oxigdal_cache_advanced::{
    CountMinSketch, EvictionPolicy, EvictionPolicyType, WTinyLfuEviction,
};

// ─── Count-Min Sketch Tests ───────────────────────────────────────────────────

#[test]
fn test_cms_increment_and_estimate_returns_positive() {
    let mut cms = CountMinSketch::new(100);
    let h = 42u64;
    for _ in 0..5 {
        cms.increment(h);
    }
    assert_eq!(cms.estimate(h), 5);
}

#[test]
fn test_cms_reset_halves_counters() {
    let mut cms = CountMinSketch::new(100);
    let h = 99u64;
    for _ in 0..8 {
        cms.increment(h);
    }
    assert_eq!(cms.estimate(h), 8);
    cms.reset();
    assert_eq!(cms.estimate(h), 4);
}

#[test]
fn test_cms_aging_triggers_at_sample_size() {
    // capacity=10 → sample_size=100
    let mut cms = CountMinSketch::new(10);
    let start_additions = cms.additions();
    assert_eq!(start_additions, 0);
    assert_eq!(cms.sample_size(), 100);

    // Increment 99 times with distinct hashes so the count approaches the threshold
    for i in 0..99u64 {
        cms.increment(i * 7 + 1);
    }
    // The 100th increment should trigger reset, resetting additions to 0
    cms.increment(777u64);
    assert_eq!(
        cms.additions(),
        0,
        "reset should have triggered after 100 increments"
    );
}

#[test]
fn test_cms_saturates_at_15() {
    let mut cms = CountMinSketch::new(100);
    let h = 1u64;
    for _ in 0..20 {
        cms.increment(h);
    }
    assert_eq!(cms.estimate(h), 15, "nibble must saturate at 15");
}

#[test]
fn test_cms_double_hashing_independent_rows() {
    let mut cms = CountMinSketch::new(64);
    cms.increment(1u64);
    cms.increment(2u64);

    // We incremented hash=1 once, and never incremented hash=99999
    let e1 = cms.estimate(1u64);
    let e2 = cms.estimate(99999u64);
    assert_eq!(e1, 1, "hash=1 was incremented once");
    assert_eq!(e2, 0, "hash=99999 was never incremented");
}

// ─── W-TinyLFU Structural / Safety Tests ─────────────────────────────────────

#[test]
fn test_wtinylfu_new_does_not_panic() {
    let _ = WTinyLfuEviction::<u64>::new(100);
    let _ = WTinyLfuEviction::<String>::new(1000);
}

#[test]
fn test_wtinylfu_insert_and_victim_selection() {
    let mut cache = WTinyLfuEviction::<u64>::new(10);
    for i in 0..20u64 {
        cache.on_insert(i, 1);
    }
    // With 20 inserts into a capacity-10 policy, at least one segment must
    // contain a candidate for eviction.
    let victim = cache.select_victim();
    assert!(
        victim.is_some(),
        "should be able to select a victim after 20 inserts"
    );
}

#[test]
fn test_wtinylfu_on_access_does_not_panic() {
    let mut cache = WTinyLfuEviction::<u64>::new(10);
    cache.on_insert(1u64, 1);
    cache.on_access(&1u64);
    // Accessing a key that is not in any segment should be a no-op, not a panic
    cache.on_access(&999u64);
}

#[test]
fn test_wtinylfu_clear_empties_all_segments() {
    let mut cache = WTinyLfuEviction::<u64>::new(10);
    for i in 0..20u64 {
        cache.on_insert(i, 1);
    }
    cache.clear();
    assert!(
        cache.select_victim().is_none(),
        "after clear(), select_victim must return None"
    );
}

#[test]
fn test_wtinylfu_admission_filters_rare_keys() {
    let mut cache = WTinyLfuEviction::<u64>::new(5);
    let popular_key = 42u64;

    // Pre-warm CMS with a popular key before inserting it
    for _ in 0..20 {
        cache.on_access(&popular_key);
    }
    cache.on_insert(popular_key, 1);

    // Insert enough items to trigger window overflow repeatedly
    for i in 100..120u64 {
        cache.on_insert(i, 1);
    }

    // The cache should remain consistent and function without panic
    let _ = cache.select_victim();
}

#[test]
fn test_wtinylfu_on_remove_works() {
    let mut cache = WTinyLfuEviction::<u64>::new(10);
    cache.on_insert(1u64, 1);
    cache.on_insert(2u64, 1);
    cache.on_remove(&1u64);
    // Removing again must not panic
    cache.on_remove(&1u64);
    // Removing a key that was never inserted must not panic
    cache.on_remove(&999u64);
}

#[test]
fn test_wtinylfu_stats_track_insertions() {
    let mut cache = WTinyLfuEviction::<u64>::new(20);
    for i in 0..10u64 {
        cache.on_insert(i, 1);
    }
    let s = cache.stats();
    assert_eq!(s.items_tracked, 10);
}

#[test]
fn test_wtinylfu_stats_track_evictions() {
    let mut cache = WTinyLfuEviction::<u64>::new(10);
    for i in 0..20u64 {
        cache.on_insert(i, 1);
    }
    cache.select_victim();
    let s = cache.stats();
    assert!(s.evictions >= 1);
}

#[test]
fn test_wtinylfu_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<WTinyLfuEviction<u64>>();
    assert_send_sync::<WTinyLfuEviction<String>>();
}

#[test]
fn test_eviction_policy_type_has_wtinylfu_variant() {
    let _v = EvictionPolicyType::WTinyLfu;
    // Verify it compiles and is Debug
    let _ = format!("{:?}", _v);
}

#[test]
fn test_wtinylfu_policy_type_method() {
    let cache = WTinyLfuEviction::<u64>::new(100);
    assert_eq!(cache.policy_type(), EvictionPolicyType::WTinyLfu);
}

#[test]
fn test_cms_multiple_keys_independent() {
    let mut cms = CountMinSketch::new(256);
    // Increment different keys different numbers of times
    for _ in 0..3 {
        cms.increment(10u64);
    }
    for _ in 0..7 {
        cms.increment(20u64);
    }
    for _ in 0..1 {
        cms.increment(30u64);
    }

    assert_eq!(cms.estimate(10u64), 3);
    assert_eq!(cms.estimate(20u64), 7);
    assert_eq!(cms.estimate(30u64), 1);
    // A key never incremented should have estimate 0 (or very close due to collisions)
    // with width=256 and depth=4, collisions are extremely unlikely for distinct u64 hashes
    assert_eq!(cms.estimate(40u64), 0);
}

#[test]
fn test_cms_zero_capacity_does_not_panic() {
    // capacity=0 → capacity.max(1)=1 → sample_size=10
    let mut cms = CountMinSketch::new(0);
    cms.increment(1u64);
    let _ = cms.estimate(1u64);
    cms.reset();
}

#[test]
fn test_wtinylfu_probation_promotion_on_access() {
    // Insert enough items to overflow the window into probation, then access
    // a probation item to promote it to protected.
    let mut cache = WTinyLfuEviction::<u64>::new(20);
    // Fill beyond window capacity (window_capacity = max(1, 20/100) = 1)
    for i in 0..10u64 {
        cache.on_insert(i, 1);
    }
    // Access key 3 multiple times — it should be in probation or window
    for _ in 0..5 {
        cache.on_access(&3u64);
    }
    // The cache should remain consistent throughout
    let _ = cache.select_victim();
}
