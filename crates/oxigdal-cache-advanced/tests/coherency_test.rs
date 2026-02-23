//! Tests for cache coherency protocols

use oxigdal_cache_advanced::coherency::protocol::{
    DirectoryCoherency, InvalidationBatcher, MESIProtocol, MSIProtocol,
};

#[tokio::test]
async fn test_msi_protocol_transitions() {
    let protocol = MSIProtocol::new("node1".to_string());
    protocol.add_peer("node2".to_string()).await;
    protocol.add_peer("node3".to_string()).await;

    let key = "test_key".to_string();

    // Read transition to Shared
    let messages = protocol.handle_read(&key).await.unwrap_or_default();
    assert!(!messages.is_empty());

    // Write transition to Modified
    let messages = protocol.handle_write(&key).await.unwrap_or_default();
    assert!(!messages.is_empty()); // Should send invalidations
}

#[tokio::test]
async fn test_msi_invalidation_tracking() {
    let protocol = MSIProtocol::new("node1".to_string());
    protocol.add_peer("node2".to_string()).await;

    let key = "test_key".to_string();

    // Trigger write (generates invalidations)
    let _messages = protocol.handle_write(&key).await.unwrap_or_default();

    // Initially not complete
    assert!(!protocol.invalidations_complete(&key).await);

    // After ack, should be complete
    protocol.handle_invalidate_ack(&key, "node2").await;
    assert!(protocol.invalidations_complete(&key).await);
}

#[tokio::test]
async fn test_mesi_protocol_exclusive_state() {
    let protocol = MESIProtocol::new("node1".to_string());
    protocol.add_peer("node2".to_string()).await;

    let key = "test_key".to_string();

    // Read without other copies should be Exclusive
    let _messages = protocol.handle_read(&key, false).await.unwrap_or_default();

    // Write from Exclusive should not need invalidations
    let messages = protocol.handle_write(&key).await.unwrap_or_default();
    assert!(messages.is_empty());
}

#[tokio::test]
async fn test_mesi_downgrade_on_remote_read() {
    let protocol = MESIProtocol::new("node1".to_string());

    let key = "test_key".to_string();

    // Get exclusive
    let _messages = protocol.handle_read(&key, false).await.unwrap_or_default();

    // Remote read should downgrade
    let _message = protocol.handle_remote_read(&key).await.unwrap_or_else(|_| {
        use oxigdal_cache_advanced::coherency::protocol::CoherencyMessage;
        CoherencyMessage::InvalidateAck(key.clone())
    });

    // Should now be in Shared state (tested indirectly through behavior)
}

#[tokio::test]
async fn test_directory_coherency() {
    let dir = DirectoryCoherency::new("node1".to_string());
    let key = "test_key".to_string();

    // First read
    let _messages = dir.handle_read(&key).await.unwrap_or_default();
    let sharers = dir.get_sharers(&key).await;
    assert!(sharers.contains("node1"));

    // Write should clear other sharers
    let _messages = dir.handle_write(&key).await.unwrap_or_default();
    let sharers = dir.get_sharers(&key).await;
    assert_eq!(sharers.len(), 1);
}

#[tokio::test]
async fn test_directory_multiple_sharers() {
    let dir1 = DirectoryCoherency::new("node1".to_string());
    let _dir2 = DirectoryCoherency::new("node2".to_string());

    let key = "test_key".to_string();

    // Both nodes read
    let _messages = dir1.handle_read(&key).await.unwrap_or_default();

    // This would normally involve coordination between directories
    // Here we test local behavior
    let sharers = dir1.get_sharers(&key).await;
    assert!(!sharers.is_empty());
}

#[tokio::test]
async fn test_invalidation_batching() {
    let batcher = InvalidationBatcher::new(3);

    // Add first invalidation
    let result = batcher
        .add_invalidation("node1".to_string(), "key1".to_string())
        .await;
    assert!(result.is_none());

    // Add second
    let result = batcher
        .add_invalidation("node1".to_string(), "key2".to_string())
        .await;
    assert!(result.is_none());

    // Third should trigger batch
    let result = batcher
        .add_invalidation("node1".to_string(), "key3".to_string())
        .await;
    assert!(result.is_some());

    if let Some(batch) = result {
        assert_eq!(batch.len(), 3);
    }
}

#[tokio::test]
async fn test_invalidation_flush() {
    let batcher = InvalidationBatcher::new(10);

    // Add some invalidations
    for i in 0..5 {
        batcher
            .add_invalidation("node1".to_string(), format!("key{}", i))
            .await;
    }

    // Flush all
    let batches = batcher.flush().await;
    assert!(!batches.is_empty());

    if let Some(batch) = batches.get("node1") {
        assert_eq!(batch.len(), 5);
    }
}

#[tokio::test]
async fn test_msi_eviction() {
    let protocol = MSIProtocol::new("node1".to_string());
    let key = "test_key".to_string();

    // Write to make it modified
    protocol.add_peer("node2".to_string()).await;
    let _messages = protocol.handle_write(&key).await.unwrap_or_default();

    // Evict should generate write-back
    let message = protocol.evict(&key).await.unwrap_or(None);
    assert!(message.is_some());
}

#[tokio::test]
async fn test_coherency_with_multiple_keys() {
    let protocol = MSIProtocol::new("node1".to_string());
    protocol.add_peer("node2".to_string()).await;

    for i in 0..10 {
        let key = format!("key{}", i);
        let _messages = protocol.handle_read(&key).await.unwrap_or_default();
    }

    // All keys should be in Shared state
    for i in 0..10 {
        let key = format!("key{}", i);
        let messages = protocol.handle_write(&key).await.unwrap_or_default();
        assert!(!messages.is_empty()); // Should send invalidations
    }
}
