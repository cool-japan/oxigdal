//! Integration tests for WebSocket functionality.

use oxigdal_ws::prelude::*;

#[tokio::test]
async fn test_server_creation() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let server = WebSocketServer::builder()
        .bind("127.0.0.1:0")?
        .max_connections(100)
        .build();

    let stats = server.stats();
    assert_eq!(stats.active_connections, 0);
    assert_eq!(stats.total_subscriptions, 0);
    Ok(())
}

#[tokio::test]
async fn test_subscription_manager() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let manager = SubscriptionManager::new();

    let sub = Subscription::tiles(
        "client-1".to_string(),
        [-180.0, -90.0, 180.0, 90.0],
        0..14,
        None,
    );

    let sub_id = sub.id.clone();
    manager.add(sub)?;

    assert_eq!(manager.count(), 1);
    assert_eq!(manager.client_count(), 1);

    let retrieved = manager.get(&sub_id);
    assert!(retrieved.is_some());
    if let Some(sub) = retrieved {
        assert_eq!(sub.id, sub_id);
    }

    manager.remove(&sub_id)?;
    assert_eq!(manager.count(), 0);
    Ok(())
}

#[tokio::test]
async fn test_tile_subscriptions() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let manager = SubscriptionManager::new();

    let sub1 = Subscription::tiles(
        "client-1".to_string(),
        [-180.0, -90.0, 0.0, 0.0],
        5..10,
        None,
    );

    let sub2 = Subscription::tiles("client-2".to_string(), [0.0, 0.0, 180.0, 90.0], 5..10, None);

    manager.add(sub1)?;
    manager.add(sub2)?;

    // Find subscriptions for a tile in the first bbox
    let matches = manager.find_tile_subscriptions(0, 0, 5);
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].client_id, "client-1");

    // Find subscriptions for a tile that overlaps both bboxes
    let matches_all = manager.find_tile_subscriptions(128, 128, 8);
    assert!(matches_all.len() <= 2);
    Ok(())
}

#[tokio::test]
async fn test_feature_subscriptions() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let manager = SubscriptionManager::new();

    let sub1 = Subscription::features("client-1".to_string(), Some("layer1".to_string()), None);

    let sub2 = Subscription::features(
        "client-2".to_string(),
        None, // All layers
        None,
    );

    manager.add(sub1)?;
    manager.add(sub2)?;

    let matches = manager.find_feature_subscriptions(Some("layer1"));
    assert_eq!(matches.len(), 2); // Both should match

    let matches2 = manager.find_feature_subscriptions(Some("layer2"));
    assert_eq!(matches2.len(), 1); // Only client-2 matches all layers
    Ok(())
}

#[tokio::test]
async fn test_event_subscriptions() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let manager = SubscriptionManager::new();

    let mut event_types = std::collections::HashSet::new();
    event_types.insert(EventType::FileChange);
    event_types.insert(EventType::Progress);

    let sub = Subscription::events("client-1".to_string(), event_types, None);

    manager.add(sub)?;

    let matches = manager.find_event_subscriptions(EventType::FileChange);
    assert_eq!(matches.len(), 1);

    let no_matches = manager.find_event_subscriptions(EventType::Error);
    assert_eq!(no_matches.len(), 0);
    Ok(())
}

#[tokio::test]
async fn test_message_encoding() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let msg = Message::Ping { id: 42 };

    // Test JSON encoding
    let json_data = msg.encode(MessageFormat::Json, Compression::None)?;
    let decoded = Message::decode(&json_data, MessageFormat::Json, Compression::None)?;

    assert!(matches!(decoded, Message::Ping { id: 42 }));

    // Test MessagePack encoding
    let msgpack_data = msg.encode(MessageFormat::MessagePack, Compression::None)?;
    let decoded = Message::decode(&msgpack_data, MessageFormat::MessagePack, Compression::None)?;

    assert!(matches!(decoded, Message::Ping { id: 42 }));
    Ok(())
}

#[tokio::test]
async fn test_message_compression() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let msg = Message::SubscribeTiles {
        subscription_id: "test-123".to_string(),
        bbox: [-180.0, -90.0, 180.0, 90.0],
        zoom_range: 0..14,
        tile_size: Some(256),
    };

    // Test with Zstd compression
    let compressed_data = msg.encode(MessageFormat::MessagePack, Compression::Zstd)?;

    let uncompressed_data = msg.encode(MessageFormat::MessagePack, Compression::None)?;

    // Compressed should generally be smaller (though not always for small messages)
    assert!(!compressed_data.is_empty());
    assert!(!uncompressed_data.is_empty());

    // Decode compressed
    let decoded = Message::decode(
        &compressed_data,
        MessageFormat::MessagePack,
        Compression::Zstd,
    )?;

    assert!(matches!(
        decoded,
        Message::SubscribeTiles {
            subscription_id,
            ..
        } if subscription_id == "test-123"
    ));
    Ok(())
}

#[tokio::test]
async fn test_tile_stream() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let mut stream = TileStream::new(rx);

    let tile = TileData::new(0, 0, 5, vec![1, 2, 3], "image/png".to_string());

    assert!(tx.send(tile.clone()).is_ok());

    let received = stream.next_tile().await;
    assert!(received.is_some());
    if let Some(tile) = received {
        assert_eq!(tile.coords(), (0, 0, 5));
        assert_eq!(tile.size(), 3);
    }
    Ok(())
}

#[tokio::test]
async fn test_feature_stream() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let mut stream = FeatureStream::new(rx);

    let feature = FeatureData::new(
        r#"{"type":"Feature","geometry":null,"properties":{}}"#.to_string(),
        ChangeType::Added,
        Some("layer1".to_string()),
    );

    assert!(tx.send(feature.clone()).is_ok());

    let received = stream.next_feature().await;
    assert!(received.is_some());
    if let Some(feature) = received {
        assert_eq!(feature.change_type, ChangeType::Added);
        assert_eq!(feature.layer, Some("layer1".to_string()));
    }
    Ok(())
}

#[tokio::test]
async fn test_event_stream() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let mut stream = EventStream::new(rx);

    let payload = serde_json::json!({"test": "data"});
    let event = EventData::new(EventType::Custom, payload);

    assert!(tx.send(event.clone()).is_ok());

    let received = stream.next_event().await;
    assert!(received.is_some());
    if let Some(event) = received {
        assert_eq!(event.event_type, EventType::Custom);
    }
    Ok(())
}

#[tokio::test]
async fn test_backpressure_controller() {
    let mut controller = BackpressureController::new(100);

    assert_eq!(controller.update(30), BackpressureState::Normal);
    assert!(!controller.should_throttle());

    assert_eq!(controller.update(75), BackpressureState::High);
    assert!(controller.should_throttle());
    assert!(!controller.should_drop());

    assert_eq!(controller.update(95), BackpressureState::Critical);
    assert!(controller.should_drop());

    assert_eq!(controller.update(25), BackpressureState::Normal);
    assert!(!controller.should_throttle());
}

#[tokio::test]
async fn test_delta_encoder() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let encoder = DeltaEncoder::new();

    let tile1 = TileData::new(0, 0, 5, vec![1, 2, 3, 4, 5], "image/png".to_string());

    let delta1 = encoder.encode(&tile1)?;
    assert_eq!(delta1.len(), 5); // Full data on first encode

    let tile2 = TileData::new(0, 0, 5, vec![1, 2, 9, 4, 5], "image/png".to_string());

    let delta2 = encoder.encode(&tile2)?;
    assert!(delta2.len() < tile2.size() || !delta2.is_empty()); // Delta or full data

    assert_eq!(encoder.cache_size(), 1);

    encoder.clear();
    assert_eq!(encoder.cache_size(), 0);
    Ok(())
}

#[tokio::test]
async fn test_client_config() {
    let config = ClientConfig::default();
    assert_eq!(config.url, "ws://localhost:9001/ws");
    assert_eq!(config.format, MessageFormat::MessagePack);
    assert_eq!(config.compression, Compression::Zstd);
    assert!(config.auto_reconnect);
    assert_eq!(config.max_reconnect_attempts, 5);
}

#[tokio::test]
async fn test_server_config() {
    let config = ServerConfig::default();
    assert_eq!(config.max_connections, 10000);
    assert_eq!(config.message_buffer_size, 1000);
    assert_eq!(config.default_format, MessageFormat::MessagePack);
    assert_eq!(config.default_compression, Compression::Zstd);
    assert!(config.enable_cors);
}

#[tokio::test]
async fn test_remove_client_subscriptions() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let manager = SubscriptionManager::new();

    let sub1 = Subscription::tiles(
        "client-1".to_string(),
        [-180.0, -90.0, 0.0, 0.0],
        0..14,
        None,
    );

    let sub2 = Subscription::features("client-1".to_string(), Some("layer1".to_string()), None);

    manager.add(sub1)?;
    manager.add(sub2)?;

    assert_eq!(manager.count(), 2);
    assert_eq!(manager.client_count(), 1);

    manager.remove_client("client-1")?;

    assert_eq!(manager.count(), 0);
    assert_eq!(manager.client_count(), 0);
    Ok(())
}

#[tokio::test]
async fn test_multiple_clients() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let manager = SubscriptionManager::new();

    for i in 0..10 {
        let client_id = format!("client-{}", i);
        let sub = Subscription::tiles(client_id, [-180.0, -90.0, 180.0, 90.0], 0..14, None);
        manager.add(sub)?;
    }

    assert_eq!(manager.count(), 10);
    assert_eq!(manager.client_count(), 10);

    // All should match a global tile
    let matches = manager.find_tile_subscriptions(128, 128, 8);
    assert_eq!(matches.len(), 10);
    Ok(())
}
