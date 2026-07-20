//! Tests for cloud storage module

use super::*;
use crate::chunk::ChunkCoord;
use crate::error::{StorageError, ZarrError};
use crate::storage::StoreKey;
use std::sync::atomic::Ordering;
use std::time::Duration;

#[test]
fn test_cloud_storage_config() {
    let config = CloudStorageConfig::new();
    assert_eq!(
        config.max_concurrent_requests,
        config::DEFAULT_MAX_CONCURRENT_REQUESTS
    );
    assert_eq!(config.max_retries, config::DEFAULT_MAX_RETRIES);
}

#[test]
fn test_config_builder() {
    let config = CloudStorageConfig::new()
        .with_max_concurrent_requests(32)
        .with_max_retries(3)
        .with_prefetch(true)
        .with_batching(true);

    assert_eq!(config.max_concurrent_requests, 32);
    assert_eq!(config.max_retries, 3);
    assert!(config.enable_prefetch);
    assert!(config.enable_batching);
}

#[test]
fn test_high_throughput_config() {
    let config = CloudStorageConfig::high_throughput();
    assert_eq!(config.max_concurrent_requests, 128);
    assert!(config.enable_prefetch);
    assert!(config.enable_streaming);
}

#[test]
fn test_low_latency_config() {
    let config = CloudStorageConfig::low_latency();
    assert_eq!(config.max_concurrent_requests, 32);
    assert!(!config.enable_prefetch);
    assert!(!config.enable_streaming);
}

#[test]
fn test_retry_policy() {
    let policy = RetryPolicy::default();

    // First attempt has no delay
    assert_eq!(policy.calculate_delay(0), Duration::ZERO);

    // Delays should increase exponentially
    let delay1 = policy.calculate_delay(1);
    let delay2 = policy.calculate_delay(2);
    let delay3 = policy.calculate_delay(3);

    assert!(delay1 < delay2);
    assert!(delay2 < delay3);
}

#[test]
fn test_retry_policy_max_delay() {
    let policy = RetryPolicy::new(10, Duration::from_millis(1000), Duration::from_millis(5000));

    // High attempt number should be clamped to max_delay
    let delay = policy.calculate_delay(20);
    assert!(delay <= Duration::from_millis(5000 + (5000.0 * policy.jitter_factor) as u64));
}

#[test]
fn test_retry_context() {
    let policy = RetryPolicy::default();
    let mut context = RetryContext::new(policy);

    assert_eq!(context.attempt(), 0);

    let network_error = StorageError::Network {
        message: "timeout".to_string(),
    };
    assert!(context.should_retry(&network_error));

    let _delay = context.record_retry();
    assert_eq!(context.attempt(), 1);

    let not_found = StorageError::KeyNotFound {
        key: "test".to_string(),
    };
    // Key not found is not retryable
    let new_context = RetryContext::new(RetryPolicy::default());
    assert!(!new_context.should_retry(&not_found));
}

#[test]
fn test_byte_range() {
    let range = ByteRange::new(0, 100).expect("valid range");
    assert_eq!(range.len(), 100);
    assert!(!range.is_empty());
    assert_eq!(range.to_http_header(), "bytes=0-99");

    let invalid = ByteRange::new(100, 50);
    assert!(invalid.is_none());
}

#[test]
fn test_byte_range_overlap() {
    let range1 = ByteRange::new(0, 100).expect("valid range");
    let range2 = ByteRange::new(50, 150).expect("valid range");
    let range3 = ByteRange::new(200, 300).expect("valid range");

    assert!(range1.overlaps(&range2));
    assert!(!range1.overlaps(&range3));
}

#[test]
fn test_byte_range_merge() {
    let range1 = ByteRange::new(0, 100).expect("valid range");
    let range2 = ByteRange::new(50, 150).expect("valid range");
    let range3 = ByteRange::new(100, 200).expect("valid range");
    let range4 = ByteRange::new(300, 400).expect("valid range");

    // Overlapping ranges should merge
    let merged = range1.merge(&range2);
    assert!(merged.is_some());
    let merged = merged.expect("should merge");
    assert_eq!(merged.start, 0);
    assert_eq!(merged.end, 150);

    // Contiguous ranges should merge
    let merged2 = range1.merge(&range3);
    assert!(merged2.is_some());
    let merged2 = merged2.expect("should merge");
    assert_eq!(merged2.start, 0);
    assert_eq!(merged2.end, 200);

    // Non-contiguous ranges should not merge
    assert!(range1.merge(&range4).is_none());
}

#[test]
fn test_request_batch() {
    let mut batch = RequestBatch::new(1024 * 1024, Duration::from_secs(1));

    let request1 = BatchedRequest {
        key: StoreKey::new("chunk/0.0.0".to_string()),
        range: None,
        priority: 1,
    };

    let request2 = BatchedRequest {
        key: StoreKey::new("chunk/0.0.1".to_string()),
        range: None,
        priority: 2,
    };

    assert!(batch.add(request1, 1024));
    assert!(batch.add(request2, 1024));
    assert_eq!(batch.len(), 2);

    batch.sort_by_priority();
    let requests = batch.take();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].priority, 1);
}

#[test]
fn test_connection_pool_stats() {
    let stats = ConnectionPoolStats::new();

    stats.record_connection_created();
    stats.record_connection_created();
    stats.record_request();
    stats.record_reuse();
    stats.record_connection_released();

    let summary = stats.summary();
    assert_eq!(summary.connections_created, 2);
    assert_eq!(summary.active_connections, 1);
    assert_eq!(summary.peak_connections, 2);
    assert_eq!(summary.requests_served, 1);
    assert_eq!(summary.connection_reuses, 1);
}

#[test]
fn test_prefetch_manager() {
    let manager = PrefetchManager::new(10);

    // Record sequential accesses
    for i in 0..5 {
        let coord = ChunkCoord::new_unchecked(vec![0, 0, i]);
        manager.record_access(&coord);
    }

    // Should detect sequential pattern
    assert_eq!(manager.detected_pattern(), AccessPattern::Sequential);

    // Generate hints
    let current = ChunkCoord::new_unchecked(vec![0, 0, 5]);
    let hints = manager.generate_hints(&current, 3);
    assert_eq!(hints.len(), 3);
}

#[test]
fn test_streaming_chunk_reader() {
    let mut reader = StreamingChunkReader::new(1024);

    let data = b"Hello, World!";
    let written = reader.write(data).expect("write should succeed");
    assert_eq!(written, data.len());

    let mut buf = [0u8; 5];
    let read = reader.read(&mut buf);
    assert_eq!(read, 5);
    assert_eq!(&buf, b"Hello");

    assert_eq!(reader.available(), 8);
}

#[test]
fn test_chunk_fetch_result() {
    let coord = ChunkCoord::new_unchecked(vec![0, 0, 0]);
    let data = vec![1, 2, 3, 4];

    let result = ChunkFetchResult::success(coord.clone(), data, Duration::from_millis(10), 0);
    assert!(result.is_success());
    assert_eq!(result.retries, 0);

    let error = ZarrError::Storage(StorageError::Network {
        message: "timeout".to_string(),
    });
    let failure = ChunkFetchResult::failure(coord.clone(), error, Duration::from_millis(100), 3);
    assert!(!failure.is_success());
    assert_eq!(failure.retries, 3);
}

#[test]
fn test_parallel_fetch_stats() {
    let stats = ParallelFetchStats::new();

    let coord = ChunkCoord::new_unchecked(vec![0, 0, 0]);
    let data = vec![0u8; 1024];

    let result = ChunkFetchResult::success(coord, data, Duration::from_millis(50), 1);
    stats.record(&result);

    assert_eq!(stats.total_chunks.load(Ordering::Relaxed), 1);
    assert_eq!(stats.successful.load(Ordering::Relaxed), 1);
    assert_eq!(stats.total_bytes.load(Ordering::Relaxed), 1024);
    assert_eq!(stats.total_retries.load(Ordering::Relaxed), 1);
    assert_eq!(stats.success_rate(), 1.0);
}

#[test]
fn test_cloud_storage_metrics() {
    let metrics = CloudStorageMetrics::new();

    metrics.record_range_request();
    metrics.record_range_request();
    metrics.record_batched_request(5);
    metrics.record_prefetch_hit();
    metrics.record_prefetch_miss();

    let summary = metrics.summary();
    assert_eq!(summary.range_requests, 2);
    assert_eq!(summary.batched_requests, 5);
    assert!((summary.prefetch_hit_ratio - 0.5).abs() < 0.001);
}

#[test]
fn test_retryable_status() {
    use super::retry::is_retryable_status;

    assert!(is_retryable_status(429)); // Too Many Requests
    assert!(is_retryable_status(500)); // Internal Server Error
    assert!(is_retryable_status(503)); // Service Unavailable
    assert!(!is_retryable_status(200)); // OK
    assert!(!is_retryable_status(404)); // Not Found
    assert!(!is_retryable_status(401)); // Unauthorized
}

#[test]
fn test_access_pattern_detection() {
    let manager = PrefetchManager::new(10);

    // Random accesses should result in random pattern
    manager.record_access(&ChunkCoord::new_unchecked(vec![0, 0, 5]));
    manager.record_access(&ChunkCoord::new_unchecked(vec![1, 2, 3]));
    manager.record_access(&ChunkCoord::new_unchecked(vec![0, 1, 0]));
    manager.record_access(&ChunkCoord::new_unchecked(vec![2, 0, 1]));

    assert_eq!(manager.detected_pattern(), AccessPattern::Random);
}

#[test]
fn test_prefetch_queue_management() {
    let manager = PrefetchManager::new(3);

    let hints = vec![
        PrefetchHint {
            coord: ChunkCoord::new_unchecked(vec![0, 0, 1]),
            priority: 1,
            estimated_access: None,
        },
        PrefetchHint {
            coord: ChunkCoord::new_unchecked(vec![0, 0, 2]),
            priority: 2,
            estimated_access: None,
        },
        PrefetchHint {
            coord: ChunkCoord::new_unchecked(vec![0, 0, 3]),
            priority: 3,
            estimated_access: None,
        },
    ];

    manager.enqueue(hints);
    assert_eq!(manager.queue_size(), 3);

    // Adding more should evict oldest
    manager.enqueue(vec![PrefetchHint {
        coord: ChunkCoord::new_unchecked(vec![0, 0, 4]),
        priority: 0,
        estimated_access: None,
    }]);
    assert_eq!(manager.queue_size(), 3);

    // Dequeue should return highest priority (lowest number)
    let hint = manager.dequeue().expect("should have hint");
    assert_eq!(hint.priority, 0);
}
