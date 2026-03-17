//! Integration tests for ML pipeline enhancements:
//! - ONNX model hot-reload (ModelWatcher)
//! - Content-addressed inference cache (InferenceCache)
//! - Adaptive batch prediction (AdaptiveBatcher)
//! - Model versioning and A/B testing (ModelRegistry, AbTestConfig)

use std::fs;
use std::time::{Duration, SystemTime};

use oxigdal_ml::batch_predict::{AdaptiveBatchConfig, AdaptiveBatcher, PredictionRequest};
use oxigdal_ml::hot_reload::{HotReloadConfig, ModelWatcher, ReloadEvent};
use oxigdal_ml::inference_cache::{CacheEntry, InferenceCache};
use oxigdal_ml::model_versioning::{AbTestConfig, ModelMetadata, ModelRegistry, ModelVersion};

// ═════════════════════════════════════════════════════════════════════════════
// Helpers
// ═════════════════════════════════════════════════════════════════════════════

fn tmp_path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(name)
}

fn make_entry(outputs: Vec<Vec<f32>>) -> CacheEntry {
    CacheEntry {
        outputs,
        created_at: SystemTime::now(),
        hit_count: 0,
        input_size_bytes: 16,
    }
}

fn make_request(id: u64) -> PredictionRequest {
    PredictionRequest {
        id,
        inputs: vec![vec![1.0, 2.0, 3.0]],
        input_shapes: vec![vec![3]],
    }
}

fn v(major: u32, minor: u32, patch: u32) -> ModelVersion {
    ModelVersion::new(major, minor, patch)
}

fn meta(name: &str, version: ModelVersion) -> ModelMetadata {
    ModelMetadata::new(name, version, "integration test model")
}

// ═════════════════════════════════════════════════════════════════════════════
// HotReload tests (10+)
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn hot_reload_watcher_construction() {
    let watcher = ModelWatcher::new("/tmp/test.onnx", HotReloadConfig::default());
    assert_eq!(watcher.path(), std::path::Path::new("/tmp/test.onnx"));
}

#[test]
fn hot_reload_check_nonexistent_file_returns_none() {
    let p = tmp_path("oxigdal_no_such_file_1234567890.onnx");
    let watcher = ModelWatcher::new(&p, HotReloadConfig::default());
    let result = watcher.check_for_update().expect("should not error");
    assert!(result.is_none());
}

#[test]
fn hot_reload_check_existing_file_first_call_returns_none() {
    let p = tmp_path("oxigdal_hr_first_call.onnx");
    fs::write(&p, b"data").expect("write");

    let watcher = ModelWatcher::new(&p, HotReloadConfig::default());
    let result = watcher.check_for_update().expect("check");
    // First check establishes baseline → no event
    assert!(
        result.is_none(),
        "first check should establish baseline, not fire event"
    );

    let _ = fs::remove_file(&p);
}

#[test]
fn hot_reload_unchanged_file_returns_none_on_second_check() {
    let p = tmp_path("oxigdal_hr_unchanged.onnx");
    fs::write(&p, b"data").expect("write");

    let watcher = ModelWatcher::new(&p, HotReloadConfig::default());
    let _ = watcher.check_for_update().expect("first");
    let result = watcher.check_for_update().expect("second");
    assert!(
        result.is_none(),
        "unchanged file should return None on second check"
    );

    let _ = fs::remove_file(&p);
}

#[test]
fn hot_reload_mark_reloaded_increments_version() {
    let watcher = ModelWatcher::new("/tmp/dummy.onnx", HotReloadConfig::default());
    assert_eq!(watcher.current_version().expect("v"), 0);

    let v1 = watcher.mark_reloaded().expect("r1");
    assert_eq!(v1, 1);

    let v2 = watcher.mark_reloaded().expect("r2");
    assert_eq!(v2, 2);
}

#[test]
fn hot_reload_reload_count_tracks_mark_calls() {
    let watcher = ModelWatcher::new("/tmp/dummy.onnx", HotReloadConfig::default());
    assert_eq!(watcher.reload_count().expect("rc0"), 0);

    watcher.mark_reloaded().expect("r1");
    watcher.mark_reloaded().expect("r2");
    watcher.mark_reloaded().expect("r3");

    assert_eq!(watcher.reload_count().expect("rc"), 3);
}

#[test]
fn hot_reload_config_default_values() {
    let cfg = HotReloadConfig::default();
    assert_eq!(cfg.poll_interval, Duration::from_secs(5));
    assert_eq!(cfg.reload_timeout, Duration::from_secs(30));
    assert!(cfg.validate_before_swap);
}

#[test]
fn hot_reload_poll_interval_custom() {
    let cfg = HotReloadConfig {
        poll_interval: Duration::from_millis(250),
        ..Default::default()
    };
    let watcher = ModelWatcher::new("/tmp/dummy.onnx", cfg);
    assert_eq!(watcher.config().poll_interval, Duration::from_millis(250));
}

#[test]
fn hot_reload_reload_timeout_custom() {
    let cfg = HotReloadConfig {
        reload_timeout: Duration::from_secs(120),
        ..Default::default()
    };
    let watcher = ModelWatcher::new("/tmp/dummy.onnx", cfg);
    assert_eq!(watcher.config().reload_timeout, Duration::from_secs(120));
}

#[test]
fn hot_reload_version_starts_at_zero() {
    let watcher = ModelWatcher::new("/tmp/any.onnx", HotReloadConfig::default());
    assert_eq!(watcher.current_version().expect("v"), 0);
}

#[test]
fn hot_reload_reload_event_fields_populated() {
    let now = SystemTime::now();
    let event = ReloadEvent {
        path: tmp_path("event_model.onnx"),
        timestamp: now,
        version: 7,
    };
    assert_eq!(event.version, 7);
    assert!(event.path.to_str().expect("str").contains("event_model"));
}

#[test]
fn hot_reload_validate_before_swap_default_true() {
    assert!(HotReloadConfig::default().validate_before_swap);
}

// ═════════════════════════════════════════════════════════════════════════════
// InferenceCache tests (15+)
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn cache_construction_empty() {
    let cache = InferenceCache::new(10);
    assert_eq!(cache.len(), 0);
    assert!(cache.is_empty());
}

#[test]
fn cache_compute_key_is_deterministic() {
    let k1 = InferenceCache::compute_key(b"model", &[1.0_f32, 2.0, 3.0]);
    let k2 = InferenceCache::compute_key(b"model", &[1.0_f32, 2.0, 3.0]);
    assert_eq!(k1, k2);
}

#[test]
fn cache_compute_key_differs_for_different_inputs() {
    let k1 = InferenceCache::compute_key(b"m", &[1.0_f32]);
    let k2 = InferenceCache::compute_key(b"m", &[2.0_f32]);
    assert_ne!(k1, k2);
}

#[test]
fn cache_compute_key_differs_for_different_model_hash() {
    let input = [0.5_f32, 1.5, 2.5];
    let k1 = InferenceCache::compute_key(b"v1", &input);
    let k2 = InferenceCache::compute_key(b"v2", &input);
    assert_ne!(k1, k2);
}

#[test]
fn cache_insert_and_get_round_trip() {
    let mut cache = InferenceCache::new(5);
    let key = InferenceCache::compute_key(b"m", &[1.0_f32]);
    let entry = make_entry(vec![vec![0.9, 0.1]]);
    cache.insert(key, entry).expect("insert");

    let got = cache.get(&key).expect("get");
    assert_eq!(got.outputs[0], vec![0.9, 0.1]);
}

#[test]
fn cache_lru_eviction_at_capacity() {
    let mut cache = InferenceCache::new(3);
    let k1 = InferenceCache::compute_key(b"m", &[1.0_f32]);
    let k2 = InferenceCache::compute_key(b"m", &[2.0_f32]);
    let k3 = InferenceCache::compute_key(b"m", &[3.0_f32]);
    let k4 = InferenceCache::compute_key(b"m", &[4.0_f32]);

    cache.insert(k1, make_entry(vec![vec![1.0]])).expect("k1");
    cache.insert(k2, make_entry(vec![vec![2.0]])).expect("k2");
    cache.insert(k3, make_entry(vec![vec![3.0]])).expect("k3");
    cache.insert(k4, make_entry(vec![vec![4.0]])).expect("k4");

    assert_eq!(cache.len(), 3);
    assert!(cache.get(&k1).is_none(), "k1 should be evicted (LRU)");
    assert!(cache.get(&k4).is_some());
    assert_eq!(cache.stats().evictions, 1);
}

#[test]
fn cache_hit_rate_zero_no_lookups() {
    let cache = InferenceCache::new(10);
    assert_eq!(cache.stats().hit_rate(), 0.0);
}

#[test]
fn cache_hit_rate_all_misses() {
    let mut cache = InferenceCache::new(10);
    let missing = InferenceCache::compute_key(b"x", &[99.0_f32]);
    cache.get(&missing);
    cache.get(&missing);
    assert_eq!(cache.stats().hit_rate(), 0.0);
}

#[test]
fn cache_hit_rate_fifty_percent() {
    let mut cache = InferenceCache::new(10);
    let key = InferenceCache::compute_key(b"m", &[1.0_f32]);
    cache
        .insert(key, make_entry(vec![vec![1.0]]))
        .expect("insert");
    cache.get(&key); // hit
    let other = InferenceCache::compute_key(b"m", &[99.0_f32]);
    cache.get(&other); // miss
    assert!((cache.stats().hit_rate() - 0.5).abs() < 1e-9);
}

#[test]
fn cache_hit_rate_all_hits() {
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
fn cache_invalidate_model_removes_sentinel_key() {
    let mut cache = InferenceCache::new(10);
    let sentinel = InferenceCache::compute_key(b"old_model", &[]);
    cache
        .insert(sentinel, make_entry(vec![vec![0.1]]))
        .expect("insert");
    assert_eq!(cache.len(), 1);
    cache.invalidate_model(b"old_model");
    assert_eq!(cache.len(), 0);
}

#[test]
fn cache_clear_empties_everything() {
    let mut cache = InferenceCache::new(10);
    for i in 0..5_u8 {
        let k = InferenceCache::compute_key(&[i], &[i as f32]);
        cache
            .insert(k, make_entry(vec![vec![i as f32]]))
            .expect("insert");
    }
    cache.clear();
    assert!(cache.is_empty());
    assert_eq!(cache.stats().memory_bytes, 0);
}

#[test]
fn cache_stats_tracks_hits_and_misses() {
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
fn cache_max_entry_size_rejects_large_entries() {
    let mut cache = InferenceCache::new(10).with_max_entry_size(4); // 1 float
    let k = InferenceCache::compute_key(b"m", &[1.0_f32]);
    let big = make_entry(vec![vec![1.0_f32, 2.0]]); // 8 bytes
    assert!(cache.insert(k, big).is_err());
}

#[test]
fn cache_max_entry_size_accepts_exactly_fitting_entry() {
    let mut cache = InferenceCache::new(10).with_max_entry_size(8); // 2 floats
    let k = InferenceCache::compute_key(b"m", &[1.0_f32]);
    let entry = make_entry(vec![vec![1.0_f32, 2.0]]); // 8 bytes
    assert!(cache.insert(k, entry).is_ok());
}

// ═════════════════════════════════════════════════════════════════════════════
// AdaptiveBatcher tests (10+)
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn batcher_construction_default_config() {
    let batcher = AdaptiveBatcher::new(AdaptiveBatchConfig::default());
    assert_eq!(
        batcher.recommended_batch_size(),
        AdaptiveBatchConfig::default().min_batch_size
    );
}

#[test]
fn batcher_starts_at_min_batch_size() {
    let cfg = AdaptiveBatchConfig {
        min_batch_size: 4,
        max_batch_size: 32,
        ..Default::default()
    };
    let batcher = AdaptiveBatcher::new(cfg);
    assert_eq!(batcher.recommended_batch_size(), 4);
}

#[test]
fn batcher_update_latency_grows_when_fast() {
    let mut batcher = AdaptiveBatcher::new(AdaptiveBatchConfig {
        min_batch_size: 1,
        max_batch_size: 128,
        target_latency_ms: 100.0,
        adaptation_rate: 0.5,
    });
    let init = batcher.recommended_batch_size();
    batcher.update_latency(5.0, init); // very fast
    assert!(batcher.recommended_batch_size() > init);
}

#[test]
fn batcher_update_latency_shrinks_when_slow() {
    let mut batcher = AdaptiveBatcher::new(AdaptiveBatchConfig {
        min_batch_size: 1,
        max_batch_size: 64,
        target_latency_ms: 50.0,
        adaptation_rate: 0.5,
    });
    // Grow first
    for _ in 0..10 {
        let sz = batcher.recommended_batch_size();
        batcher.update_latency(1.0, sz);
    }
    let high = batcher.recommended_batch_size();
    batcher.update_latency(99999.0, high);
    assert!(batcher.recommended_batch_size() < high);
}

#[test]
fn batcher_create_batches_splits_correctly() {
    let mut batcher = AdaptiveBatcher::new(AdaptiveBatchConfig {
        min_batch_size: 3,
        max_batch_size: 3,
        ..Default::default()
    });
    batcher.update_latency(50.0, 3); // keep size at 3
    // Force exact size for test predictability by constructing with fixed size
    let batcher2 = AdaptiveBatcher::new(AdaptiveBatchConfig {
        min_batch_size: 3,
        max_batch_size: 3,
        target_latency_ms: 50.0,
        adaptation_rate: 0.0,
    });
    let reqs: Vec<_> = (0..7).map(make_request).collect();
    let batches = batcher2.create_batches(reqs);
    assert_eq!(batches.len(), 3);
    assert_eq!(batches[0].len(), 3);
    assert_eq!(batches[2].len(), 1);
}

#[test]
fn batcher_create_batches_fewer_than_batch_size() {
    let batcher = AdaptiveBatcher::new(AdaptiveBatchConfig {
        min_batch_size: 16,
        max_batch_size: 16,
        ..Default::default()
    });
    let reqs: Vec<_> = (0..5).map(make_request).collect();
    let batches = batcher.create_batches(reqs);
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].len(), 5);
}

#[test]
fn batcher_create_batches_empty() {
    let batcher = AdaptiveBatcher::new(AdaptiveBatchConfig::default());
    assert!(batcher.create_batches(vec![]).is_empty());
}

#[test]
fn batcher_average_latency_no_observations() {
    let batcher = AdaptiveBatcher::new(AdaptiveBatchConfig::default());
    assert_eq!(batcher.average_latency_ms(), 0.0);
}

#[test]
fn batcher_average_latency_single() {
    let mut batcher = AdaptiveBatcher::new(AdaptiveBatchConfig::default());
    batcher.update_latency(37.5, 1);
    assert!((batcher.average_latency_ms() - 37.5).abs() < 1e-9);
}

#[test]
fn batcher_average_latency_multiple() {
    let mut batcher = AdaptiveBatcher::new(AdaptiveBatchConfig::default());
    batcher.update_latency(10.0, 1);
    batcher.update_latency(30.0, 1);
    assert!((batcher.average_latency_ms() - 20.0).abs() < 1e-9);
}

#[test]
fn batcher_total_items_and_batches() {
    let mut batcher = AdaptiveBatcher::new(AdaptiveBatchConfig::default());
    batcher.update_latency(50.0, 8);
    batcher.update_latency(50.0, 4);
    assert_eq!(batcher.total_batches(), 2);
    assert_eq!(batcher.total_items(), 12);
}

// ═════════════════════════════════════════════════════════════════════════════
// ModelVersioning tests (15+)
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn versioning_ordering_major() {
    assert!(v(1, 0, 0) < v(2, 0, 0));
}

#[test]
fn versioning_ordering_minor() {
    assert!(v(1, 0, 0) < v(1, 1, 0));
}

#[test]
fn versioning_ordering_patch() {
    assert!(v(1, 1, 0) < v(1, 1, 1));
}

#[test]
fn versioning_display_no_tag() {
    assert_eq!(v(1, 2, 3).to_string(), "1.2.3");
}

#[test]
fn versioning_display_with_tag() {
    assert_eq!(v(1, 0, 0).with_tag("rc1").to_string(), "1.0.0-rc1");
}

#[test]
fn versioning_with_tag_stores_tag() {
    let ver = v(2, 0, 0).with_tag("prod");
    assert_eq!(ver.tag.as_deref(), Some("prod"));
}

#[test]
fn versioning_registry_register_and_get_latest() {
    let mut reg = ModelRegistry::new();
    reg.register(meta("unet", v(1, 0, 0)));
    reg.register(meta("unet", v(1, 1, 0)));
    assert_eq!(reg.get_latest("unet").expect("latest").version, v(1, 1, 0));
}

#[test]
fn versioning_registry_get_specific_version() {
    let mut reg = ModelRegistry::new();
    reg.register(meta("det", v(1, 0, 0)));
    reg.register(meta("det", v(2, 0, 0)));
    let found = reg.get_version("det", &v(1, 0, 0)).expect("v1");
    assert_eq!(found.version, v(1, 0, 0));
}

#[test]
fn versioning_registry_list_versions_sorted_ascending() {
    let mut reg = ModelRegistry::new();
    reg.register(meta("seg", v(3, 0, 0)));
    reg.register(meta("seg", v(1, 0, 0)));
    reg.register(meta("seg", v(2, 0, 0)));
    let versions = reg.list_versions("seg");
    assert_eq!(versions.len(), 3);
    for i in 0..versions.len() - 1 {
        assert!(versions[i] < versions[i + 1]);
    }
}

#[test]
fn versioning_registry_list_models() {
    let mut reg = ModelRegistry::new();
    reg.register(meta("a", v(1, 0, 0)));
    reg.register(meta("b", v(1, 0, 0)));
    let mut names = reg.list_models();
    names.sort();
    assert_eq!(names, vec!["a", "b"]);
}

#[test]
fn versioning_registry_unregister_version() {
    let mut reg = ModelRegistry::new();
    reg.register(meta("m", v(1, 0, 0)));
    reg.register(meta("m", v(2, 0, 0)));
    assert!(reg.unregister_version("m", &v(1, 0, 0)));
    assert!(reg.get_version("m", &v(1, 0, 0)).is_none());
    assert!(reg.get_version("m", &v(2, 0, 0)).is_some());
}

#[test]
fn versioning_registry_unregister_nonexistent_returns_false() {
    let mut reg = ModelRegistry::new();
    assert!(!reg.unregister_version("ghost", &v(1, 0, 0)));
}

#[test]
fn ab_routing_is_deterministic() {
    let ab = AbTestConfig::new("a", "b", 0.5).expect("ok");
    assert_eq!(ab.route(42), ab.route(42));
    assert_eq!(ab.route(999), ab.route(999));
}

#[test]
fn ab_invalid_split_negative_errors() {
    assert!(AbTestConfig::new("a", "b", -0.1).is_err());
}

#[test]
fn ab_invalid_split_over_one_errors() {
    assert!(AbTestConfig::new("a", "b", 1.1).is_err());
}

#[test]
fn ab_split_one_routes_all_to_a() {
    let ab = AbTestConfig::new("model_a", "model_b", 1.0).expect("ok");
    for id in 0..50_u64 {
        assert_eq!(ab.route(id), "model_a");
    }
}

#[test]
fn ab_split_zero_routes_all_to_b() {
    let ab = AbTestConfig::new("model_a", "model_b", 0.0).expect("ok");
    for id in 0..50_u64 {
        assert_eq!(ab.route(id), "model_b");
    }
}

#[test]
fn ab_roughly_balanced_with_fifty_percent_split() {
    let ab = AbTestConfig::new("a", "b", 0.5).expect("ok");
    let a_count = (0_u64..10_000).filter(|id| ab.route(*id) == "a").count();
    assert!(
        a_count > 4500 && a_count < 5500,
        "expected ~50% routed to a, got {a_count}/10000"
    );
}
