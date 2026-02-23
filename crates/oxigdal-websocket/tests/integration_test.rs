//! Integration tests for oxigdal-websocket

use oxigdal_websocket::prelude::*;

#[test]
fn test_server_config() {
    let config = ServerConfig::default();
    assert!(config.max_connections > 0);
    assert!(config.max_message_size > 0);
}

#[test]
fn test_protocol_config() {
    let config = ProtocolConfig::default();
    assert_eq!(config.format, MessageFormat::Binary);
    assert!(config.enable_framing);
}

#[test]
fn test_broadcast_config() {
    let config = BroadcastConfig::default();
    assert!(config.enable_filtering);
    assert!(config.max_topics > 0);
}

#[test]
fn test_update_config() {
    let config = UpdateConfig::default();
    assert!(config.enable_tile_updates);
    assert!(config.enable_feature_updates);
    assert!(config.enable_change_streams);
}

#[tokio::test]
async fn test_broadcast_system() {
    let config = BroadcastConfig::default();
    let system = BroadcastSystem::new(config);

    let stats = system.stats().await;
    assert_eq!(stats.topic_count, 0);
}

#[tokio::test]
async fn test_update_system() {
    let config = UpdateConfig::default();
    let system = UpdateSystem::new(config);

    let stats = system.stats().await;
    assert_eq!(stats.tile_updates, 0);
    assert_eq!(stats.feature_updates, 0);
}

#[test]
fn test_message_creation() {
    let msg = Message::ping();
    assert_eq!(msg.message_type(), MessageType::Ping);

    let msg = Message::pong();
    assert_eq!(msg.message_type(), MessageType::Pong);
}

#[test]
fn test_client_sdk_generation() {
    let config = ClientSdkConfig::default();
    let js_code = generate_javascript_client(&config);

    assert!(js_code.contains("OxiGDALWebSocketClient"));
    assert!(js_code.contains("connect"));
    assert!(js_code.contains("subscribe"));
}

#[test]
fn test_typescript_definitions() {
    let ts_defs = generate_typescript_definitions();

    assert!(ts_defs.contains("OxiGDALWebSocketClient"));
    assert!(ts_defs.contains("MessageType"));
    assert!(ts_defs.contains("export"));
}

#[tokio::test]
async fn test_tile_update_manager() -> Result<()> {
    use oxigdal_websocket::updates::tile_updates::{TileCoord, TileUpdate, TileUpdateManager};

    let manager = TileUpdateManager::new(100);
    let coord = TileCoord::new(10, 512, 384);
    let update = TileUpdate::full(coord, vec![1, 2, 3, 4], "png".to_string());

    manager.add_update(update)?;

    let stats = manager.stats().await;
    assert_eq!(stats.total_updates, 1);
    assert_eq!(stats.full_updates, 1);

    Ok(())
}

#[tokio::test]
async fn test_feature_update_manager() -> Result<()> {
    use oxigdal_websocket::updates::feature_updates::{FeatureUpdate, FeatureUpdateManager};

    let manager = FeatureUpdateManager::new(100);
    let feature = serde_json::json!({"type": "Feature"});
    let update = FeatureUpdate::created("f1".to_string(), "layer1".to_string(), feature);

    manager.add_update(update)?;

    let stats = manager.stats().await;
    assert_eq!(stats.total_updates, 1);
    assert_eq!(stats.created, 1);

    Ok(())
}

#[test]
fn test_change_stream() -> Result<()> {
    use oxigdal_websocket::protocol::message::ChangeType;
    use oxigdal_websocket::updates::change_stream::{ChangeStream, ChangeStreamConfig};

    let config = ChangeStreamConfig::default();
    let stream = ChangeStream::new("test".to_string(), config);

    let change_id = stream.add_event(
        "collection".to_string(),
        ChangeType::Created,
        "doc1".to_string(),
        None,
    )?;

    assert_eq!(change_id, 1);

    let stats = stream.stats();
    assert_eq!(stats.total_events, 1);

    Ok(())
}

#[test]
fn test_incremental_update_manager() -> Result<()> {
    use bytes::Bytes;
    use oxigdal_websocket::updates::incremental::{
        DeltaEncoding, IncrementalUpdateManager, UpdateDelta,
    };

    let manager = IncrementalUpdateManager::new();

    manager.register("entity1".to_string(), 1, Some(Bytes::from(vec![1, 2, 3])))?;

    let delta = UpdateDelta::new(1, 2, Bytes::from(vec![4, 5]), DeltaEncoding::BinaryDiff);
    manager.add_delta("entity1", delta)?;

    let stats = manager.stats();
    assert_eq!(stats.entity_count, 1);
    assert_eq!(stats.total_deltas, 1);

    Ok(())
}

#[tokio::test]
async fn test_room_manager() -> Result<()> {
    use uuid::Uuid;

    let manager = RoomManager::new(10, 100);
    let member = Uuid::new_v4();

    manager.join("test_room", member).await?;

    let stats = manager.stats().await;
    assert_eq!(stats.total_rooms, 1);
    assert_eq!(stats.total_members, 1);

    manager.leave("test_room", &member).await?;

    let stats = manager.stats().await;
    assert_eq!(stats.total_rooms, 0); // Room deleted when empty

    Ok(())
}

#[test]
fn test_protocol_codec() -> Result<()> {
    let config = ProtocolConfig {
        format: MessageFormat::Binary,
        compression: None,
        enable_framing: false,
        ..Default::default()
    };

    let codec = ProtocolCodec::new(config);
    let message = Message::ping();

    let encoded = codec.encode(&message)?;
    let decoded = codec.decode(&encoded)?;

    assert_eq!(message.message_type(), decoded.message_type());
    assert_eq!(message.id, decoded.id);

    Ok(())
}

#[test]
fn test_compression() -> Result<()> {
    use oxigdal_websocket::protocol::compression::{
        CompressionCodec, CompressionLevel, CompressionType,
    };

    let codec = CompressionCodec::new(CompressionType::Zstd, CompressionLevel::Default);
    let data = b"Hello, World! This is a test message.".repeat(10);

    let compressed = codec.compress(&data)?;
    let decompressed = codec.decompress(&compressed)?;

    assert_eq!(data.as_slice(), decompressed.as_ref());
    assert!(compressed.len() < data.len());

    Ok(())
}

#[test]
fn test_framing() -> Result<()> {
    use bytes::Bytes;
    use oxigdal_websocket::protocol::framing::{Frame, FrameCodec};

    let codec = FrameCodec::new();
    let payload = Bytes::from(vec![1, 2, 3, 4, 5]);
    let frame = Frame::data(1, false, payload.clone());

    let encoded = codec.encode(&frame)?;
    let decoded = codec.decode(&encoded)?;

    assert_eq!(frame.payload, decoded.payload);

    Ok(())
}

#[test]
fn test_message_filter() {
    use oxigdal_websocket::broadcast::filter::{FilterChain, MessageFilter};
    use uuid::Uuid;

    let chain = FilterChain::new_and()
        .add_filter(MessageFilter::message_types(vec![MessageType::Ping]))
        .add_filter(MessageFilter::all());

    let ping = Message::ping();
    let conn_id = Uuid::new_v4();

    assert!(chain.should_deliver(&ping, &conn_id));
}

#[tokio::test]
async fn test_server_builder() {
    let server = Server::builder()
        .max_connections(1000)
        .max_message_size(8 * 1024 * 1024)
        .build();

    let stats = server.stats().await;
    assert_eq!(stats.active_connections, 0);
}
