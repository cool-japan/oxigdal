//! Tests for the prefetch module.

use super::*;

#[test]
fn test_extract_trailing_number() {
    assert_eq!(extract_trailing_number("file_5"), Some(5));
    assert_eq!(extract_trailing_number("tile_10_20_3"), Some(3));
    assert_eq!(extract_trailing_number("data"), None);
}

#[test]
fn test_increment_key() {
    assert_eq!(increment_key("file_5", 1), Some("file_6".to_string()));
    assert_eq!(increment_key("file_5", -1), Some("file_4".to_string()));
    assert_eq!(
        increment_key("tile_10_20", 1),
        Some("tile_10_21".to_string())
    );
    assert_eq!(increment_key("file_0", -1), None);
}

#[test]
fn test_temporal_pattern_analyzer() {
    let mut analyzer = TemporalPatternAnalyzer::new(20);

    // Record consistent intervals
    for _ in 0..10 {
        analyzer.record_interval(1000); // 1 second intervals
    }

    assert!(analyzer.detected_period().is_some());
    let period = analyzer.detected_period();
    assert!(period.is_some_and(|p| (p as i64 - 1000).abs() < 100));
}

#[test]
fn test_temporal_burst_detection() {
    let mut analyzer = TemporalPatternAnalyzer::new(20);

    // Record rapid intervals
    for _ in 0..10 {
        analyzer.record_interval(50); // 50ms intervals = 20/sec
    }

    assert!(analyzer.is_burst());
}

#[test]
fn test_spatial_locality_analyzer() {
    let mut analyzer = SpatialLocalityAnalyzer::new(10);

    // Record a path moving right
    analyzer.record_access(0, 0, 0);
    analyzer.record_access(1, 0, 0);
    analyzer.record_access(2, 0, 0);

    let predictions = analyzer.predict_adjacent(4);
    assert!(!predictions.is_empty());

    // First prediction should be in the direction of movement (right)
    assert!(predictions.iter().any(|&(x, _, _)| x == 3));
}

#[test]
fn test_pattern_analyzer_sequential() {
    let mut analyzer = PatternAnalyzer::new(10);

    analyzer.record_access(AccessRecord::new("file_0".to_string()));
    analyzer.record_access(AccessRecord::new("file_1".to_string()));
    analyzer.record_access(AccessRecord::new("file_2".to_string()));
    analyzer.record_access(AccessRecord::new("file_3".to_string()));

    assert!(matches!(
        analyzer.current_pattern(),
        AccessPattern::SequentialForward | AccessPattern::SequentialBackward
    ));

    let predictions = analyzer.predict_next(3);
    assert!(!predictions.is_empty());
}

#[test]
fn test_pattern_analyzer_spatial() {
    let mut analyzer = PatternAnalyzer::new(10);

    analyzer.record_access(AccessRecord::with_coordinates(
        "tile_0_0_0".to_string(),
        0,
        0,
        0,
    ));
    analyzer.record_access(AccessRecord::with_coordinates(
        "tile_1_0_0".to_string(),
        1,
        0,
        0,
    ));
    analyzer.record_access(AccessRecord::with_coordinates(
        "tile_2_0_0".to_string(),
        2,
        0,
        0,
    ));
    analyzer.record_access(AccessRecord::with_coordinates(
        "tile_3_0_0".to_string(),
        3,
        0,
        0,
    ));

    assert_eq!(analyzer.current_pattern(), AccessPattern::Spatial);

    let predictions = analyzer.predict_next(4);
    assert!(!predictions.is_empty());
}

#[test]
fn test_prefetch_target_priority_ordering() {
    let targets = [
        PrefetchTarget::new("low".to_string(), PrefetchPriority::Low, 0.5),
        PrefetchTarget::new("critical".to_string(), PrefetchPriority::Critical, 0.9),
        PrefetchTarget::new("medium".to_string(), PrefetchPriority::Medium, 0.7),
    ];

    let mut sorted: Vec<_> = targets.iter().map(|t| t.priority).collect();
    sorted.sort();

    assert_eq!(sorted[0], PrefetchPriority::Low);
    assert_eq!(sorted[1], PrefetchPriority::Medium);
    assert_eq!(sorted[2], PrefetchPriority::Critical);
}

#[test]
fn test_memory_aware_prefetcher() {
    let prefetcher = MemoryAwarePrefetcher::new(1000);

    assert!(prefetcher.can_prefetch(500));
    assert!(prefetcher.allocate(500));
    assert_eq!(prefetcher.current_usage(), 500);

    assert!(prefetcher.can_prefetch(400));
    assert!(!prefetcher.can_prefetch(600)); // Would exceed limit

    prefetcher.release(200);
    assert_eq!(prefetcher.current_usage(), 300);
}

#[test]
fn test_buffer_stats() {
    let stats = BufferStats::default();

    stats.record_prefetch(true, 100);
    stats.record_prefetch(true, 200);
    stats.record_prefetch(false, 150);

    assert_eq!(stats.total_prefetches.load(Ordering::Relaxed), 3);
    assert_eq!(stats.successful_prefetches.load(Ordering::Relaxed), 2);
    assert_eq!(stats.wasted_prefetches.load(Ordering::Relaxed), 1);

    let hit_rate = stats.hit_rate();
    assert!((hit_rate - 0.666).abs() < 0.01);
}

#[test]
fn test_prefetch_config_builder() {
    let config = PrefetchConfig::new()
        .with_enabled(true)
        .with_prefetch_count(10)
        .with_max_concurrent(8)
        .with_bandwidth_limit(1_000_000)
        .with_memory_limit(128 * 1024 * 1024)
        .with_adaptive_sizing(true);

    assert!(config.enabled);
    assert_eq!(config.prefetch_count, 10);
    assert_eq!(config.max_concurrent, 8);
    assert_eq!(config.bandwidth_limit, Some(1_000_000));
    assert_eq!(config.memory_limit, 128 * 1024 * 1024);
    assert!(config.adaptive_sizing);
}

#[cfg(feature = "async")]
#[tokio::test]
async fn test_prefetch_queue() {
    let queue = PrefetchQueue::new();

    queue
        .enqueue(PrefetchTarget::new(
            "low".to_string(),
            PrefetchPriority::Low,
            0.5,
        ))
        .await;
    queue
        .enqueue(PrefetchTarget::new(
            "high".to_string(),
            PrefetchPriority::High,
            0.9,
        ))
        .await;
    queue
        .enqueue(PrefetchTarget::new(
            "medium".to_string(),
            PrefetchPriority::Medium,
            0.7,
        ))
        .await;

    assert_eq!(queue.len().await, 3);

    // Should dequeue highest priority first
    let first = queue.dequeue().await;
    assert!(first.is_some());
    assert_eq!(first.map(|t| t.key), Some("high".to_string()));

    let second = queue.dequeue().await;
    assert!(second.is_some());
    assert_eq!(second.map(|t| t.key), Some("medium".to_string()));
}

#[cfg(feature = "async")]
#[tokio::test]
async fn test_prefetch_queue_deduplication() {
    let queue = PrefetchQueue::new();

    queue
        .enqueue(PrefetchTarget::new(
            "key1".to_string(),
            PrefetchPriority::High,
            0.9,
        ))
        .await;
    queue
        .enqueue(PrefetchTarget::new(
            "key1".to_string(),
            PrefetchPriority::Critical,
            0.95,
        ))
        .await;

    // Should only have one entry
    assert_eq!(queue.len().await, 1);
}

#[cfg(feature = "async")]
#[tokio::test]
async fn test_prefetch_manager() {
    let config = PrefetchConfig::new()
        .with_prefetch_count(3)
        .with_memory_limit(1024 * 1024);
    let manager = PrefetchManager::new(config);

    // Record sequential accesses
    manager
        .record_access(AccessRecord::new("file_0".to_string()))
        .await;
    manager
        .record_access(AccessRecord::new("file_1".to_string()))
        .await;
    manager
        .record_access(AccessRecord::new("file_2".to_string()))
        .await;

    let predictions = manager
        .record_access(AccessRecord::new("file_3".to_string()))
        .await;

    // Should predict sequential pattern
    assert!(!predictions.is_empty());
}

#[cfg(feature = "async")]
#[tokio::test]
async fn test_prefetch_manager_memory_pressure() {
    let config = PrefetchConfig::new().with_memory_limit(1000);
    let manager = PrefetchManager::new(config);

    // Allocate most of the memory
    assert!(manager.allocate_memory(800));
    assert!(manager.memory_pressure() > 0.7);

    // Should not allow more prefetching
    assert!(!manager.can_prefetch(300).await);

    // Release memory
    manager.release_memory(500);
    assert!(manager.can_prefetch(300).await);
}

#[cfg(feature = "async")]
#[tokio::test]
async fn test_bandwidth_aware_prefetcher() {
    let prefetcher = BandwidthAwarePrefetcher::new(1000);

    assert!(prefetcher.can_prefetch(500).await);
    prefetcher.record_usage(500).await;

    assert!(prefetcher.can_prefetch(400).await);
    prefetcher.record_usage(400).await;

    // Should be near limit
    assert!(!prefetcher.can_prefetch(200).await);
    assert!(prefetcher.remaining_bandwidth() < 200);
}

// -- PrefetchManager::run_prefetch_cycle: real I/O driver -------------

#[cfg(feature = "cache")]
mod run_prefetch_cycle_tests {
    use super::*;
    use crate::backends::CloudStorageBackend;
    use crate::cache::{CacheConfig, MultiLevelCache};
    use crate::error::{CloudError, S3Error};
    use std::collections::HashMap;
    use std::sync::atomic::AtomicUsize;
    use tokio::sync::Mutex;

    struct FakeBackend {
        objects: Mutex<HashMap<String, bytes::Bytes>>,
        call_count: AtomicUsize,
    }

    impl FakeBackend {
        fn new() -> Self {
            Self {
                objects: Mutex::new(HashMap::new()),
                call_count: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait::async_trait]
    impl CloudStorageBackend for FakeBackend {
        async fn get(&self, key: &str) -> crate::error::Result<bytes::Bytes> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            self.objects.lock().await.get(key).cloned().ok_or_else(|| {
                CloudError::S3(S3Error::Sdk {
                    message: format!("missing key {key}"),
                })
            })
        }

        async fn put(&self, key: &str, data: &[u8]) -> crate::error::Result<()> {
            self.objects
                .lock()
                .await
                .insert(key.to_string(), bytes::Bytes::copy_from_slice(data));
            Ok(())
        }

        async fn delete(&self, key: &str) -> crate::error::Result<()> {
            self.objects.lock().await.remove(key);
            Ok(())
        }

        async fn exists(&self, key: &str) -> crate::error::Result<bool> {
            Ok(self.objects.lock().await.contains_key(key))
        }

        async fn list_prefix(&self, prefix: &str) -> crate::error::Result<Vec<String>> {
            Ok(self
                .objects
                .lock()
                .await
                .keys()
                .filter(|k| k.starts_with(prefix))
                .cloned()
                .collect())
        }
    }

    #[tokio::test]
    async fn test_run_prefetch_cycle_fetches_and_caches_queued_targets() {
        let backend = FakeBackend::new();
        backend.put("tile-1", b"hello").await.expect("put failed");
        backend.put("tile-2", b"world!").await.expect("put failed");

        let manager = PrefetchManager::new(PrefetchConfig::new());
        manager
            .queue
            .enqueue(PrefetchTarget::new(
                "tile-1".to_string(),
                PrefetchPriority::High,
                0.9,
            ))
            .await;
        manager
            .queue
            .enqueue(PrefetchTarget::new(
                "tile-2".to_string(),
                PrefetchPriority::Medium,
                0.7,
            ))
            .await;

        let cache = MultiLevelCache::new(CacheConfig::new()).expect("cache creation failed");

        let fetched = manager.run_prefetch_cycle(&backend, &cache).await;
        assert_eq!(fetched, 2);
        assert_eq!(backend.call_count.load(Ordering::SeqCst), 2);

        let cached1 = cache
            .get(&"tile-1".to_string())
            .await
            .expect("tile-1 should be cached after prefetch");
        assert_eq!(cached1, bytes::Bytes::from_static(b"hello"));
        let cached2 = cache
            .get(&"tile-2".to_string())
            .await
            .expect("tile-2 should be cached after prefetch");
        assert_eq!(cached2, bytes::Bytes::from_static(b"world!"));

        assert_eq!(manager.queue_len().await, 0);
    }

    #[tokio::test]
    async fn test_run_prefetch_cycle_skips_missing_objects_without_failing() {
        let backend = FakeBackend::new();
        // Note: "missing-tile" is never `put`.

        let manager = PrefetchManager::new(PrefetchConfig::new());
        manager
            .queue
            .enqueue(PrefetchTarget::new(
                "missing-tile".to_string(),
                PrefetchPriority::Low,
                0.5,
            ))
            .await;

        let cache = MultiLevelCache::new(CacheConfig::new()).expect("cache creation failed");
        let fetched = manager.run_prefetch_cycle(&backend, &cache).await;
        assert_eq!(fetched, 0, "a failed fetch must not count as fetched");
        assert!(cache.get(&"missing-tile".to_string()).await.is_err());
    }

    #[tokio::test]
    async fn test_run_prefetch_cycle_respects_memory_budget() {
        let backend = FakeBackend::new();
        backend
            .put("big-tile", &[0u8; 1000])
            .await
            .expect("put failed");

        // Configure a memory budget far too small for the estimated size.
        let manager = PrefetchManager::new(PrefetchConfig::new().with_memory_limit(10));
        manager
            .queue
            .enqueue(
                PrefetchTarget::new("big-tile".to_string(), PrefetchPriority::High, 0.9)
                    .with_estimated_size(1000),
            )
            .await;

        let cache = MultiLevelCache::new(CacheConfig::new()).expect("cache creation failed");
        let fetched = manager.run_prefetch_cycle(&backend, &cache).await;
        assert_eq!(
            fetched, 0,
            "a target over the memory budget must be skipped"
        );
        assert_eq!(
            backend.call_count.load(Ordering::SeqCst),
            0,
            "backend must never be called for a budget-rejected target"
        );
    }

    #[tokio::test]
    async fn test_run_prefetch_cycle_empty_queue_returns_zero() {
        let backend = FakeBackend::new();
        let manager = PrefetchManager::new(PrefetchConfig::new());
        let cache = MultiLevelCache::new(CacheConfig::new()).expect("cache creation failed");

        let fetched = manager.run_prefetch_cycle(&backend, &cache).await;
        assert_eq!(fetched, 0);
        assert_eq!(backend.call_count.load(Ordering::SeqCst), 0);
    }
}
