//! Tests for batch processing module.

use super::*;
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::RasterDataType;

#[test]
fn test_batch_config_builder() {
    let config = BatchConfig::builder()
        .max_batch_size(64)
        .batch_timeout_ms(200)
        .dynamic_batching(false)
        .parallel_batches(8)
        .memory_pooling(false)
        .build();

    assert_eq!(config.max_batch_size, 64);
    assert_eq!(config.batch_timeout_ms, 200);
    assert!(!config.dynamic_batching);
    assert_eq!(config.parallel_batches, 8);
    assert!(!config.memory_pooling);
}

#[test]
fn test_batch_stats() {
    let stats = BatchStats {
        total_requests: 100,
        total_batches: 10,
        max_batch_size: 20,
        total_latency_ms: 1000,
    };

    assert!((stats.avg_batch_size() - 10.0).abs() < 1e-6);
    assert!((stats.avg_latency_ms() - 10.0).abs() < 1e-6);
    assert!((stats.throughput() - 100.0).abs() < 1e-6);
}

#[test]
fn test_batch_scheduler() {
    let config = BatchConfig {
        max_batch_size: 3,
        batch_timeout_ms: 100,
        ..Default::default()
    };

    let mut scheduler = BatchScheduler::new(config);
    assert_eq!(scheduler.pending_count(), 0);
    assert!(!scheduler.should_form_batch());

    // Add requests
    for _ in 0..3 {
        scheduler.add_request(RasterBuffer::zeros(256, 256, RasterDataType::Float32));
    }

    assert_eq!(scheduler.pending_count(), 3);
    assert!(scheduler.should_form_batch()); // Max size reached

    let batch = scheduler.form_batch();
    assert_eq!(batch.len(), 3);
    assert_eq!(scheduler.pending_count(), 0);
}

#[test]
fn test_batch_scheduler_partial() {
    let config = BatchConfig {
        max_batch_size: 5,
        ..Default::default()
    };

    let mut scheduler = BatchScheduler::new(config);

    // Add 3 requests (less than max)
    for _ in 0..3 {
        scheduler.add_request(RasterBuffer::zeros(256, 256, RasterDataType::Float32));
    }

    // Should form partial batch when requested
    let batch = scheduler.form_batch();
    assert_eq!(batch.len(), 3);
}

#[test]
fn test_auto_tune_batch_size() {
    // Test with different sample sizes
    let sample_size_1mb = 1024 * 1024; // 1 MB
    let batch_size_1 = BatchConfig::auto_tune_batch_size(sample_size_1mb, 0.5);
    assert!(batch_size_1 > 0 && batch_size_1 <= 256);

    let sample_size_10mb = 10 * 1024 * 1024; // 10 MB
    let batch_size_2 = BatchConfig::auto_tune_batch_size(sample_size_10mb, 0.5);
    assert!(batch_size_2 > 0 && batch_size_2 <= 256);

    // Larger samples should result in smaller batch sizes
    assert!(batch_size_1 >= batch_size_2);

    // Test fraction clamping
    let batch_size_3 = BatchConfig::auto_tune_batch_size(sample_size_1mb, 2.0);
    assert!(batch_size_3 > 0 && batch_size_3 <= 256);

    // Test with zero sample size
    let batch_size_4 = BatchConfig::auto_tune_batch_size(0, 0.5);
    assert_eq!(batch_size_4, 32); // Should return default
}

// ========================================================================
// Dynamic Batching Tests
// ========================================================================

#[test]
fn test_priority_level_ordering() {
    assert!(PriorityLevel::Critical > PriorityLevel::High);
    assert!(PriorityLevel::High > PriorityLevel::Normal);
    assert!(PriorityLevel::Normal > PriorityLevel::Low);
    assert!(PriorityLevel::Low > PriorityLevel::Background);
}

#[test]
fn test_priority_level_default() {
    assert_eq!(PriorityLevel::default(), PriorityLevel::Normal);
}

#[test]
fn test_dynamic_batch_config_builder() {
    let config = DynamicBatchConfig::builder()
        .max_batch_size(64)
        .min_batch_size(8)
        .initial_batch_size(16)
        .batch_timeout_ms(200)
        .critical_timeout_ms(20)
        .enable_adaptive_sizing(false)
        .enable_padding(false)
        .padding_strategy(PaddingStrategy::Replicate)
        .target_latency_ms(100)
        .max_queue_length(512)
        .num_workers(8)
        .enable_coalescing(true)
        .memory_limit_bytes(1024 * 1024 * 1024)
        .build();

    assert_eq!(config.max_batch_size, 64);
    assert_eq!(config.min_batch_size, 8);
    assert_eq!(config.initial_batch_size, 16);
    assert_eq!(config.batch_timeout_ms, 200);
    assert_eq!(config.critical_timeout_ms, 20);
    assert!(!config.enable_adaptive_sizing);
    assert!(!config.enable_padding);
    assert_eq!(config.padding_strategy, PaddingStrategy::Replicate);
    assert_eq!(config.target_latency_ms, 100);
    assert_eq!(config.max_queue_length, 512);
    assert_eq!(config.num_workers, 8);
    assert!(config.enable_coalescing);
    assert_eq!(config.memory_limit_bytes, Some(1024 * 1024 * 1024));
}

#[test]
fn test_dynamic_batch_config_defaults() {
    let config = DynamicBatchConfig::default();

    assert_eq!(config.max_batch_size, 32);
    assert_eq!(config.min_batch_size, 1);
    assert_eq!(config.initial_batch_size, 8);
    assert_eq!(config.batch_timeout_ms, 100);
    assert_eq!(config.critical_timeout_ms, 10);
    assert!(config.enable_adaptive_sizing);
    assert!(config.enable_padding);
    assert_eq!(config.padding_strategy, PaddingStrategy::Zero);
    assert_eq!(config.target_latency_ms, 50);
    assert_eq!(config.max_queue_length, 1024);
    assert_eq!(config.num_workers, 4);
    assert!(!config.enable_coalescing);
    assert!(config.memory_limit_bytes.is_none());
}

#[test]
fn test_dynamic_batch_config_low_latency() {
    let config = DynamicBatchConfig::low_latency();

    assert_eq!(config.max_batch_size, 8);
    assert_eq!(config.min_batch_size, 1);
    assert_eq!(config.initial_batch_size, 2);
    assert_eq!(config.batch_timeout_ms, 10);
    assert_eq!(config.critical_timeout_ms, 2);
    assert_eq!(config.target_latency_ms, 10);
}

#[test]
fn test_dynamic_batch_config_high_throughput() {
    let config = DynamicBatchConfig::high_throughput();

    assert_eq!(config.max_batch_size, 64);
    assert_eq!(config.min_batch_size, 16);
    assert_eq!(config.initial_batch_size, 32);
    assert_eq!(config.batch_timeout_ms, 200);
    assert_eq!(config.critical_timeout_ms, 50);
    assert_eq!(config.target_latency_ms, 100);
}

#[test]
fn test_padding_strategy_default() {
    assert_eq!(PaddingStrategy::default(), PaddingStrategy::Zero);
}

#[test]
fn test_padding_strategy_variants() {
    let strategies = [
        PaddingStrategy::Zero,
        PaddingStrategy::Reflect,
        PaddingStrategy::Replicate,
        PaddingStrategy::Constant(128),
        PaddingStrategy::None,
    ];

    // Ensure all variants are distinct
    for (i, s1) in strategies.iter().enumerate() {
        for (j, s2) in strategies.iter().enumerate() {
            if i != j {
                assert_ne!(s1, s2);
            }
        }
    }
}

#[test]
fn test_dynamic_batch_stats() {
    let stats = DynamicBatchStats {
        total_requests: 100,
        total_batches: 10,
        avg_latency_us: 50000.0,
        timeout_ratio: 0.3,
        current_batch_size: 8,
        rejected_count: 5,
        queue_length: 3,
        queue_memory_bytes: 1024 * 1024,
    };

    // Average batch size
    let avg_batch = stats.avg_batch_size();
    assert!((avg_batch - 10.0).abs() < 1e-6);

    // Average latency in ms
    let avg_latency = stats.avg_latency_ms();
    assert!((avg_latency - 50.0).abs() < 1e-6);

    // Throughput: (10 requests/batch * 1_000_000 us/s) / 50_000 us = 200 req/s
    let throughput = stats.throughput();
    assert!((throughput - 200.0).abs() < 1.0);
}

#[test]
fn test_dynamic_batch_stats_edge_cases() {
    // Empty stats
    let stats = DynamicBatchStats {
        total_requests: 0,
        total_batches: 0,
        avg_latency_us: 0.0,
        timeout_ratio: 0.0,
        current_batch_size: 8,
        rejected_count: 0,
        queue_length: 0,
        queue_memory_bytes: 0,
    };

    assert!((stats.avg_batch_size() - 0.0).abs() < 1e-6);
    assert!((stats.avg_latency_ms() - 0.0).abs() < 1e-6);
    assert!((stats.throughput() - 0.0).abs() < 1e-6);
}
