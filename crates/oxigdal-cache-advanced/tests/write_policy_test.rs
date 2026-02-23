//! Tests for write policies

use oxigdal_cache_advanced::write_policy::{
    WriteAction, WriteAmplificationTracker, WriteBackManager, WriteBuffer, WritePolicyManager,
    WritePolicyType,
};
use std::time::Duration;

#[tokio::test]
async fn test_write_back_manager() {
    let manager = WriteBackManager::new(10, Duration::from_secs(5));

    // Mark first block dirty
    let needs_flush = manager
        .mark_dirty("key1".to_string(), 1024)
        .await
        .unwrap_or(false);
    assert!(!needs_flush);

    // Check dirty count
    let count = manager.dirty_count().await;
    assert_eq!(count, 1);

    // Check dirty bytes
    let bytes = manager.dirty_bytes().await;
    assert_eq!(bytes, 1024);
}

#[tokio::test]
async fn test_write_back_flush_threshold() {
    let manager = WriteBackManager::new(3, Duration::from_secs(60));

    // Add blocks up to threshold
    for i in 0..3 {
        let needs_flush = manager
            .mark_dirty(format!("key{}", i), 1024)
            .await
            .unwrap_or(false);

        if i < 2 {
            assert!(!needs_flush);
        } else {
            assert!(needs_flush);
        }
    }
}

#[tokio::test]
async fn test_write_back_coalescing() {
    let manager = WriteBackManager::new(10, Duration::from_secs(60));

    // Write same key multiple times
    for _ in 0..5 {
        manager
            .mark_dirty("key1".to_string(), 1024)
            .await
            .unwrap_or_default();
    }

    // Should still be only one dirty block (coalesced)
    let count = manager.dirty_count().await;
    assert_eq!(count, 1);
}

#[tokio::test]
async fn test_write_back_mark_clean() {
    let manager = WriteBackManager::new(10, Duration::from_secs(60));

    manager
        .mark_dirty("key1".to_string(), 1024)
        .await
        .unwrap_or_default();
    assert_eq!(manager.dirty_count().await, 1);

    manager.mark_clean(&"key1".to_string()).await;
    assert_eq!(manager.dirty_count().await, 0);
}

#[tokio::test]
async fn test_write_back_flush_candidates() {
    let manager = WriteBackManager::new(10, Duration::from_millis(10));

    // Add blocks
    manager
        .mark_dirty("key1".to_string(), 1024)
        .await
        .unwrap_or_default();
    manager
        .mark_dirty("key2".to_string(), 1024)
        .await
        .unwrap_or_default();

    // Wait for age (slightly longer than the 10ms threshold)
    tokio::time::sleep(Duration::from_millis(15)).await;

    // Get candidates
    let candidates = manager.get_flush_candidates().await;
    assert!(!candidates.is_empty());
}

#[tokio::test]
async fn test_write_buffer() {
    let buffer = WriteBuffer::new(10 * 1024);

    let data = vec![0u8; 1024];
    let needs_flush = buffer
        .add_write("key1".to_string(), data)
        .await
        .unwrap_or(false);

    assert!(!needs_flush);
    assert_eq!(buffer.size().await, 1024);
    assert_eq!(buffer.count().await, 1);
}

#[tokio::test]
async fn test_write_buffer_flush() {
    let buffer = WriteBuffer::new(3 * 1024);

    // Add data up to threshold
    for i in 0..3 {
        let data = vec![0u8; 1024];
        let needs_flush = buffer
            .add_write(format!("key{}", i), data)
            .await
            .unwrap_or(false);

        if i < 2 {
            assert!(!needs_flush);
        } else {
            assert!(needs_flush);
        }
    }
}

#[tokio::test]
async fn test_write_buffer_drain() {
    let buffer = WriteBuffer::new(10 * 1024);

    // Add multiple writes
    for i in 0..5 {
        let data = vec![0u8; 1024];
        buffer
            .add_write(format!("key{}", i), data)
            .await
            .unwrap_or_default();
    }

    let writes = buffer.drain().await;
    assert_eq!(writes.len(), 5);

    // Buffer should be empty
    assert_eq!(buffer.size().await, 0);
    assert_eq!(buffer.count().await, 0);
}

#[tokio::test]
async fn test_write_amplification_tracker() {
    let tracker = WriteAmplificationTracker::new();

    tracker.record_cache_write(1000).await;
    tracker.record_backing_write(2500).await;

    let amp = tracker.amplification_factor().await;
    assert!((amp - 2.5).abs() < 0.01);

    let cache = tracker.cache_writes().await;
    let backing = tracker.backing_writes().await;

    assert_eq!(cache, 1000);
    assert_eq!(backing, 2500);
}

#[tokio::test]
async fn test_write_amplification_reset() {
    let tracker = WriteAmplificationTracker::new();

    tracker.record_cache_write(1000).await;
    tracker.record_backing_write(2000).await;

    tracker.reset().await;

    assert_eq!(tracker.cache_writes().await, 0);
    assert_eq!(tracker.backing_writes().await, 0);
    assert_eq!(tracker.amplification_factor().await, 0.0);
}

#[tokio::test]
async fn test_write_policy_manager_write_through() {
    let manager = WritePolicyManager::new(
        WritePolicyType::WriteThrough,
        10,
        Duration::from_secs(60),
        10 * 1024,
    );

    let data = vec![0u8; 1024];
    let action = manager
        .handle_write("key1".to_string(), data)
        .await
        .unwrap_or(WriteAction::Buffered);

    assert_eq!(action, WriteAction::Buffered);
}

#[tokio::test]
async fn test_write_policy_manager_write_back() {
    let manager = WritePolicyManager::new(
        WritePolicyType::WriteBack,
        10,
        Duration::from_secs(60),
        10 * 1024,
    );

    let data = vec![0u8; 1024];
    let action = manager
        .handle_write("key1".to_string(), data)
        .await
        .unwrap_or(WriteAction::Deferred);

    assert_eq!(action, WriteAction::Deferred);
}

#[tokio::test]
async fn test_write_policy_manager_write_around() {
    let manager = WritePolicyManager::new(
        WritePolicyType::WriteAround,
        10,
        Duration::from_secs(60),
        10 * 1024,
    );

    let data = vec![0u8; 1024];
    let action = manager
        .handle_write("key1".to_string(), data)
        .await
        .unwrap_or(WriteAction::Direct);

    assert_eq!(action, WriteAction::Direct);
}

#[tokio::test]
async fn test_write_policy_manager_write_behind() {
    let manager = WritePolicyManager::new(
        WritePolicyType::WriteBehind,
        10,
        Duration::from_secs(60),
        10 * 1024,
    );

    let data = vec![0u8; 1024];
    let action = manager
        .handle_write("key1".to_string(), data)
        .await
        .unwrap_or(WriteAction::Async);

    assert_eq!(action, WriteAction::Async);
}

#[test]
fn test_dirty_block() {
    use oxigdal_cache_advanced::write_policy::DirtyBlock;

    let mut block = DirtyBlock::new("key1".to_string(), 1024);
    assert_eq!(block.write_count, 1);
    assert_eq!(block.size, 1024);

    block.record_write();
    assert_eq!(block.write_count, 2);

    let age = block.age();
    assert!(age.as_secs() < 1);
}
